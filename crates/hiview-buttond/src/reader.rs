// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
#![forbid(unsafe_code)]

//! usbmon reader: keeps `/dev/usbmon<bus>` open for the microscope's
//! current bus and forwards Snap edges. Only the pure
//! [`hiview_core::snap_button_event`] filter decides what becomes an
//! event — every other USB record is dropped here and never logged
//! or forwarded (the daemon's privacy contract).

use std::fs::File;
use std::os::unix::io::AsFd;
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use hiview_core::{ButtonEvent, snap_button_event};
use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};

use crate::usbdev::Target;
use crate::usbmon_ffi::{UsbmonPacket, fetch_event, record_of};

/// How often the loop re-reads the shared [`Target`] — the devnum on a
/// replugged microscope changes without the daemon noticing otherwise.
const RETARGET_MS: u16 = 500;

/// The shared, hotplug-updated device address.
pub type SharedTarget = Mutex<Option<Target>>;

/// Runs the reader forever: opens/closes the usbmon bus as the target
/// appears/disappears and forwards button edges to `events`. The file
/// descriptor is non-blocking so the loop always wakes to re-check
/// the target — a replugged scope gets its new devnum picked up even
/// while the bus is silent.
pub fn run(target: &SharedTarget, events: &Sender<ButtonEvent>) -> ! {
    let mut open: Option<(u16, File)> = None;
    let mut data = Vec::with_capacity(256);
    loop {
        let current = *target.lock().unwrap_or_else(|e| e.into_inner());
        let Some(t) = current else {
            open = None; // scope gone — the bus fd is worthless to keep
            thread::sleep(Duration::from_millis(RETARGET_MS.into()));
            continue;
        };
        if open.as_ref().map(|(bus, _)| *bus) != Some(t.busnum) {
            open = open_bus(t.busnum);
            if open.is_none() {
                thread::sleep(Duration::from_millis(RETARGET_MS.into()));
                continue;
            }
        }
        let Some((_, file)) = open.as_ref() else {
            continue;
        };
        if !wait_readable(file) {
            continue; // timeout/error — re-check the target
        }
        // Drain every queued record; the target is re-read per record so
        // a mid-batch devnum change still filters correctly.
        loop {
            match fetch_event(file.as_fd(), &mut data) {
                Ok(packet) => {
                    let now = *target.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(now) = now
                        && let Some(event) = event_from_packet(&packet, &data, now)
                        && events.send(event).is_err()
                    {
                        // Publisher gone — the daemon has no purpose
                        // left; exit so systemd's restart policy
                        // decides what happens.
                        std::process::exit(0);
                    }
                }
                // No more records right now — the fd is non-blocking
                // (EWOULDBLOCK == EAGAIN on Linux).
                Err(Errno::EAGAIN) => break,
                Err(Errno::EINTR) => continue,
                Err(e) => {
                    // Unexpected ioctl failure — reopen on next pass.
                    tracing::warn!(errno = ?e, "usbmon read failed; reopening");
                    open = None;
                    break;
                }
            }
        }
    }
}

/// Opens `/dev/usbmon<busnum>` read-only + non-blocking.
fn open_bus(busnum: u16) -> Option<(u16, File)> {
    let path = format!("/dev/usbmon{busnum}");
    match File::open(&path) {
        Ok(file) => {
            if let Err(e) = set_nonblocking(&file) {
                tracing::warn!(path, error = %e, "could not set O_NONBLOCK");
                return None;
            }
            Some((busnum, file))
        }
        Err(e) => {
            tracing::warn!(path, error = %e, "usbmon open failed");
            None
        }
    }
}

/// `POLLIN` for `RETARGET_MS`, then back to re-checking the target.
/// `POLLERR`/`POLLNVAL`/`POLLHUP` are treated as readable so the
/// following `fetch_event` reports the real errno.
fn wait_readable(file: &File) -> bool {
    let mut fds = [PollFd::new(file.as_fd(), PollFlags::POLLIN)];
    match poll(&mut fds, PollTimeout::from(RETARGET_MS)) {
        Ok(n) => n > 0,
        Err(Errno::EINTR) => false,
        Err(e) => {
            tracing::warn!(errno = ?e, "usbmon poll failed");
            false
        }
    }
}

fn set_nonblocking(file: &File) -> nix::Result<()> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    let flags = fcntl(file.as_fd(), FcntlArg::F_GETFL)?;
    let flags = OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK;
    fcntl(file.as_fd(), FcntlArg::F_SETFL(flags))?;
    Ok(())
}

/// Header+payload → a [`ButtonEvent`] iff the record is the Snap
/// button on `target` (single place where raw USB bytes cross into
/// the app's vocabulary).
fn event_from_packet(packet: &UsbmonPacket, data: &[u8], target: Target) -> Option<ButtonEvent> {
    snap_button_event(&record_of(packet, data), target.busnum, target.devnum)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complete-interrupt record on ep 0x83 carrying `data`.
    fn packet(busnum: u16, devnum: u8, len_cap: usize) -> UsbmonPacket {
        UsbmonPacket {
            event_type: b'C',
            xfer_type: 1,
            epnum: 0x83,
            devnum,
            busnum,
            len_cap: len_cap as u32,
            ..UsbmonPacket::default()
        }
    }

    fn target() -> Target {
        Target {
            busnum: 3,
            devnum: 7,
        }
    }

    #[test]
    fn real_pressed_bytes_emit_pressed() {
        let p = packet(3, 7, 4);
        assert_eq!(
            event_from_packet(&p, &[0x02, 0x01, 0x00, 0x01], target()),
            Some(ButtonEvent::Pressed)
        );
    }

    #[test]
    fn real_released_bytes_emit_released() {
        let p = packet(3, 7, 4);
        assert_eq!(
            event_from_packet(&p, &[0x02, 0x01, 0x00, 0x00], target()),
            Some(ButtonEvent::Released)
        );
    }

    #[test]
    fn wrong_devnum_is_dropped() {
        let p = packet(3, 9, 4); // some other device on the same bus
        assert_eq!(
            event_from_packet(&p, &[0x02, 0x01, 0x00, 0x01], target()),
            None
        );
    }

    #[test]
    fn submit_records_are_dropped() {
        let mut p = packet(3, 7, 4);
        p.event_type = b'S';
        assert_eq!(
            event_from_packet(&p, &[0x02, 0x01, 0x00, 0x01], target()),
            None
        );
    }

    #[test]
    fn short_payloads_are_dropped() {
        let p = packet(3, 7, 2);
        assert_eq!(event_from_packet(&p, &[0x02, 0x01], target()), None);
    }
}
