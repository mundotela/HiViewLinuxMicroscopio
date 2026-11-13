// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! [`HelperSource`]: client of the `hiview-buttond` Unix socket.
//!
//! The daemon publishes only `PRESSED <ts_us>\n` / `RELEASED <ts_us>\n`
//! lines (docs/ARCHITECTURE.md §6.10): the device clock in µs, so the
//! press duration survives delivery batching. The source reconnects
//! automatically: if the daemon restarts — or is installed while the
//! app is already running — the button starts working without
//! touching the app.

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use tracing::info;

use crate::error::{Error, Result};
use crate::{ButtonEdge, ButtonEvent, ButtonSource, SourceKind, SourceState, SourceUpdate};

/// Socket path published by `hiview-buttond` (§6.10).
pub const DEFAULT_SOCKET: &str = "/run/hiview/button.sock";

/// Pause between reconnect attempts after a refused/lost connection.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Reads Snap events from the `hiview-buttond` socket.
pub struct HelperSource {
    path: PathBuf,
}

impl HelperSource {
    /// A source for a specific socket path (tests use a temp dir).
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// A source for the daemon's system socket (`/run/hiview/…`).
    #[must_use]
    pub fn system() -> Self {
        Self::new(PathBuf::from(DEFAULT_SOCKET))
    }
}

impl ButtonSource for HelperSource {
    fn start(self: Box<Self>, tx: Sender<SourceUpdate>) -> Result<()> {
        let path = self.path.clone();
        thread::Builder::new()
            .name("hiview-button".into())
            .spawn(move || pump(&path, &tx))
            .map_err(Error::ThreadSpawn)?;
        Ok(())
    }
}

/// Reconnect loop: read events until the socket drops, sleep, retry.
/// Exits only when the UI channel is gone (app shutting down).
fn pump(path: &Path, tx: &Sender<SourceUpdate>) {
    loop {
        match connect_and_read(path, tx) {
            ReadEnd::ChannelClosed => return,
            ReadEnd::Lost => {
                if !send(
                    tx,
                    SourceUpdate::State(SourceKind::Helper, SourceState::Disconnected),
                ) {
                    return;
                }
            }
        }
        thread::sleep(RECONNECT_DELAY);
    }
}

/// Whether `connect_and_read` ended because the transport dropped or
/// because the UI went away.
enum ReadEnd {
    Lost,
    ChannelClosed,
}

/// One socket session: connect, announce `Connected`, forward events
/// until EOF or error.
fn connect_and_read(path: &Path, tx: &Sender<SourceUpdate>) -> ReadEnd {
    let stream = match UnixStream::connect(path) {
        Ok(s) => s,
        Err(err) => {
            // ENOENT/EACCES are the normal "daemon not installed" and
            // are not worth a warn every second.
            tracing::debug!("button socket {}: {err}", path.display());
            return ReadEnd::Lost;
        }
    };
    info!("snap button: connected to {}", path.display());
    if !send(
        tx,
        SourceUpdate::State(SourceKind::Helper, SourceState::Connected),
    ) {
        return ReadEnd::ChannelClosed;
    }

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else {
            return ReadEnd::Lost;
        };
        if let Some(edge) = parse_line(&line)
            && !send(tx, SourceUpdate::Event(edge))
        {
            return ReadEnd::ChannelClosed;
        }
    }
    ReadEnd::Lost
}

/// `"PRESSED <ts_us>"` / `"RELEASED <ts_us>"` — strict contract;
/// anything else on the wire is ignored, never forwarded.
fn parse_line(line: &str) -> Option<ButtonEdge> {
    let (tag, ts) = line.trim().split_once(' ')?;
    let event = match tag {
        "PRESSED" => ButtonEvent::Pressed,
        "RELEASED" => ButtonEvent::Released,
        _ => return None,
    };
    Some(ButtonEdge {
        event,
        ts_us: ts.parse().ok()?,
    })
}

/// mpsc send that never panics: a dead UI ends the source.
fn send(tx: &Sender<SourceUpdate>, update: SourceUpdate) -> bool {
    tx.send(update).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, channel};
    use std::sync::{Arc, Mutex};

    /// A fake daemon: accepts clients and serves scripted lines;
    /// `crash` drops every client to simulate a restart.
    struct FakeDaemon {
        path: PathBuf,
        alive: Arc<AtomicBool>,
        clients: Arc<Mutex<Vec<UnixStream>>>,
        accept: thread::JoinHandle<()>,
    }

    impl FakeDaemon {
        /// Binds `path` (unlinked first, so a restart can rebind) and
        /// serves until `crash` is called.
        fn start(path: &Path) -> Self {
            let _ = std::fs::remove_file(path);
            let listener =
                UnixListener::bind(path).unwrap_or_else(|e| panic!("bind {path:?}: {e}"));
            listener
                .set_nonblocking(true)
                .unwrap_or_else(|e| panic!("nonblocking: {e}"));
            let alive = Arc::new(AtomicBool::new(true));
            let clients = Arc::new(Mutex::new(Vec::new()));
            let accept = {
                let alive = alive.clone();
                let clients = clients.clone();
                thread::spawn(move || {
                    while alive.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => clients
                                .lock()
                                .unwrap_or_else(|e| panic!("lock: {e}"))
                                .push(stream),
                            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(10));
                            }
                            Err(_) => return,
                        }
                    }
                })
            };
            Self {
                path: path.to_path_buf(),
                alive,
                clients,
                accept,
            }
        }

        /// Writes a raw line to every connected client. Waits briefly
        /// for the accept loop to register the connection — the client
        /// sees `Connected` a few ms before the server's list does.
        fn write(&self, line: &str) {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let mut clients = self.clients.lock().unwrap_or_else(|e| panic!("lock: {e}"));
                clients.retain_mut(|c| writeln!(c, "{line}").is_ok());
                if !clients.is_empty() {
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "no client ever connected to the fake daemon"
                );
                drop(clients);
                thread::sleep(Duration::from_millis(10));
            }
        }

        /// Simulates a restart: listener stops and every client sees
        /// EOF (what a real daemon stop looks like to the socket).
        fn crash(self) {
            self.alive.store(false, Ordering::Relaxed);
            self.clients
                .lock()
                .unwrap_or_else(|e| panic!("lock: {e}"))
                .clear();
            let _ = self.accept.join();
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Receives updates until `pred` matches or the deadline passes.
    fn wait_for(rx: &Receiver<SourceUpdate>, pred: impl Fn(&SourceUpdate) -> bool) -> SourceUpdate {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for a source update"
            );
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(u) if pred(&u) => return u,
                Ok(_) | Err(_) => continue,
            }
        }
    }

    fn is_connected(u: &SourceUpdate) -> bool {
        matches!(
            u,
            SourceUpdate::State(SourceKind::Helper, SourceState::Connected)
        )
    }

    #[test]
    fn helper_forwards_press_and_release() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let daemon = FakeDaemon::start(&dir.path().join("button.sock"));
        let (tx, rx) = channel();
        Box::new(HelperSource::new(daemon.path.clone()))
            .start(tx)
            .unwrap_or_else(|e| panic!("start: {e}"));

        assert!(is_connected(&wait_for(&rx, is_connected)));
        daemon.write("PRESSED 1000");
        assert_eq!(
            wait_for(&rx, |u| matches!(u, SourceUpdate::Event(_))),
            SourceUpdate::Event(ButtonEdge {
                event: ButtonEvent::Pressed,
                ts_us: 1000,
            })
        );
        daemon.write("RELEASED 1700");
        assert_eq!(
            wait_for(&rx, |u| matches!(u, SourceUpdate::Event(_))),
            SourceUpdate::Event(ButtonEdge {
                event: ButtonEvent::Released,
                ts_us: 1700,
            })
        );
        daemon.crash();
    }

    /// Garbage lines on the socket are ignored — only the two
    /// contract strings produce events (§6.10 security boundary).
    #[test]
    fn helper_ignores_non_contract_lines() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let daemon = FakeDaemon::start(&dir.path().join("button.sock"));
        let (tx, rx) = channel();
        Box::new(HelperSource::new(daemon.path.clone()))
            .start(tx)
            .unwrap_or_else(|e| panic!("start: {e}"));
        wait_for(&rx, is_connected);

        daemon.write("KEYBOARD_DUMP aabbc");
        daemon.write("PRESSED"); // missing timestamp — off contract
        daemon.write("PRESSED not_a_number");
        daemon.write("PRESSED 42");
        assert_eq!(
            wait_for(&rx, |u| matches!(u, SourceUpdate::Event(_))),
            SourceUpdate::Event(ButtonEdge {
                event: ButtonEvent::Pressed,
                ts_us: 42,
            }),
            "the first Event after garbage must be the well-formed press"
        );
        daemon.crash();
    }

    /// Daemon restart on the SAME path: the client reports
    /// Disconnected, reconnects by itself and events flow again —
    /// no app restart needed.
    #[test]
    fn helper_reconnects_after_daemon_restart() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("button.sock");
        let daemon = FakeDaemon::start(&path);
        let (tx, rx) = channel();
        Box::new(HelperSource::new(path.clone()))
            .start(tx)
            .unwrap_or_else(|e| panic!("start: {e}"));
        wait_for(&rx, is_connected);

        daemon.write("PRESSED 7");
        wait_for(&rx, |u| matches!(u, SourceUpdate::Event(_)));

        daemon.crash();
        assert_eq!(
            wait_for(&rx, |u| matches!(
                u,
                SourceUpdate::State(SourceKind::Helper, SourceState::Disconnected)
            )),
            SourceUpdate::State(SourceKind::Helper, SourceState::Disconnected)
        );

        // The daemon comes back on the same socket path.
        let daemon2 = FakeDaemon::start(&path);
        assert!(
            is_connected(&wait_for(&rx, is_connected)),
            "client must reconnect without the app doing anything"
        );
        daemon2.write("RELEASED 9");
        assert_eq!(
            wait_for(&rx, |u| matches!(u, SourceUpdate::Event(_))),
            SourceUpdate::Event(ButtonEdge {
                event: ButtonEvent::Released,
                ts_us: 9,
            })
        );
        daemon2.crash();
    }

    /// Socket absent at startup: the source still reports Disconnected
    /// and connects later when the daemon appears.
    #[test]
    fn helper_connects_when_daemon_appears_later() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("button.sock");
        let (tx, rx) = channel();
        Box::new(HelperSource::new(path.clone()))
            .start(tx)
            .unwrap_or_else(|e| panic!("start: {e}"));

        let daemon = FakeDaemon::start(&path);
        assert!(is_connected(&wait_for(&rx, is_connected)));
        daemon.crash();
    }
}
