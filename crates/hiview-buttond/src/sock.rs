// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
#![forbid(unsafe_code)]

//! Unix-socket publisher: `/run/hiview/button.sock`, mode 0660, group
//! `hiview`. The ONLY bytes ever written are `"PRESSED\n"` and
//! `"RELEASED\n"` — the daemon's privacy contract (no USB payload
//! ever leaves this process).

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::Duration;

use anyhow::{Context, bail};
use hiview_core::ButtonEvent;

/// The on-the-wire contract consumed by `hiview-button`'s
/// `HelperSource`. Keep in sync — app and daemon version together.
pub fn wire_line(event: ButtonEvent) -> &'static [u8] {
    match event {
        ButtonEvent::Pressed => b"PRESSED\n",
        ButtonEvent::Released => b"RELEASED\n",
    }
}

/// Binds `path` (removing a stale socket), sets 0660 + group, accepts
/// clients, and broadcasts every [`ButtonEvent`] from `rx` to all of
/// them. Runs until `rx` disconnects; an `Err` return means the
/// listener itself is broken — the daemon exits and systemd logs it.
pub fn serve(path: &Path, group: Option<&str>, rx: &Receiver<ButtonEvent>) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("removing stale {}", path.display())),
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))
        .with_context(|| format!("chmod 0660 {}", path.display()))?;
    if let Some(name) = group {
        apply_group(path, name)?;
    }
    tracing::info!(path = %path.display(), "button socket ready");

    // Accept thread — each client becomes a non-blocking broadcast sink.
    let (ct, cr) = channel::<UnixStream>();
    thread::Builder::new()
        .name("hiview-accept".into())
        .spawn(move || {
            for conn in listener.incoming() {
                match conn {
                    Ok(s) => {
                        if s.set_nonblocking(true).is_ok() {
                            let _ = ct.send(s);
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "socket accept failed"),
                }
            }
        })
        .context("spawning accept thread")?;

    let mut clients: Vec<UnixStream> = Vec::new();
    loop {
        while let Ok(client) = cr.try_recv() {
            clients.push(client);
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => broadcast(&mut clients, wire_line(event)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                bail!("event channel closed")
            }
        }
    }
}

/// Writes `line` to every client, dropping those that fail — a dead
/// app must never stall the daemon. `WouldBlock` (reader stalled,
/// buffer full) keeps the client but skips the event: a stalled GUI
/// missing an edge is safer than the daemon buffering unboundedly.
fn broadcast(clients: &mut Vec<UnixStream>, line: &[u8]) {
    clients.retain_mut(|c| match c.write_all(line) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            tracing::warn!("client stalled — dropping one button event");
            true
        }
        Err(_) => false,
    });
}

/// `chgrp path group` — the socket is 0660, so the group is what lets
/// the desktop app connect without root.
fn apply_group(path: &Path, group: &str) -> anyhow::Result<()> {
    use nix::unistd::{Group, chown};
    let gid = Group::from_name(group)
        .with_context(|| format!("resolving group {group}"))?
        .with_context(|| format!("group {group} does not exist"))?
        .gid;
    chown(path, None, Some(gid)).with_context(|| format!("chgrp {group} {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::Shutdown;
    use tempfile::TempDir;

    /// Sends events to a `serve` running in a temp dir; returns the
    /// sender + socket path.
    fn spawn_server() -> (
        std::sync::mpsc::Sender<ButtonEvent>,
        TempDir,
        std::path::PathBuf,
    ) {
        let dir = TempDir::new().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("button.sock");
        let (tx, rx) = channel::<ButtonEvent>();
        let serve_path = path.clone();
        thread::spawn(move || {
            let _ = serve(&serve_path, None, &rx);
        });
        (tx, dir, path)
    }

    /// Waits until `serve` has bound the socket.
    fn wait_socket(sock: &Path) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline && !sock.exists() {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(sock.exists(), "listener never bound {}", sock.display());
    }

    /// Connects AND waits long enough for the accept→serve handoff
    /// (the drain interval is 200 ms) — events broadcast before the
    /// client is registered are lost to it by design.
    fn connect(sock: &Path) -> UnixStream {
        wait_socket(sock);
        let s = UnixStream::connect(sock).unwrap_or_else(|e| panic!("connect: {e}"));
        s.set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap_or_else(|e| panic!("timeout: {e}"));
        thread::sleep(Duration::from_millis(400));
        s
    }

    /// Reads until `want` bytes are buffered or the deadline hits.
    fn read_until(s: &mut UnixStream, want: &[u8]) -> Vec<u8> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut out = Vec::new();
        let mut buf = [0u8; 64];
        while std::time::Instant::now() < deadline && !out.ends_with(want) {
            match s.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
        out
    }

    #[test]
    fn pressed_is_written_verbatim() {
        let (tx, _dir, sock) = spawn_server();
        let mut s = connect(&sock);
        tx.send(ButtonEvent::Pressed)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read_until(&mut s, b"PRESSED\n"), b"PRESSED\n");
    }

    #[test]
    fn both_edges_and_only_the_contract() {
        let (tx, _dir, sock) = spawn_server();
        let mut s = connect(&sock);
        tx.send(ButtonEvent::Pressed)
            .unwrap_or_else(|e| panic!("{e}"));
        tx.send(ButtonEvent::Released)
            .unwrap_or_else(|e| panic!("{e}"));
        let got = read_until(&mut s, b"RELEASED\n");
        assert_eq!(got, b"PRESSED\nRELEASED\n");
    }

    #[test]
    fn socket_is_0660() {
        let (_tx, _dir, sock) = spawn_server();
        wait_socket(&sock);
        let mode = fs::metadata(&sock)
            .unwrap_or_else(|e| panic!("stat: {e}"))
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o660);
    }

    #[test]
    fn a_dead_client_does_not_stall_broadcast() {
        let (tx, _dir, sock) = spawn_server();
        let dead = connect(&sock); // registered, then killed mid-flight
        dead.shutdown(Shutdown::Both).ok();
        drop(dead);
        let mut live = connect(&sock);
        tx.send(ButtonEvent::Pressed)
            .unwrap_or_else(|e| panic!("{e}"));
        tx.send(ButtonEvent::Released)
            .unwrap_or_else(|e| panic!("{e}"));
        // The live client still receives — the dead one was pruned.
        assert_eq!(read_until(&mut live, b"RELEASED\n"), b"PRESSED\nRELEASED\n");
    }
}
