// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
#![forbid(unsafe_code)]

//! usbmon reader: keeps `/dev/usbmon<bus>` open for the microscope's
//! current bus and forwards Snap edges. Only the pure
//! [`hiview_core::snap_button_event`] filter decides what becomes an
//! event — every other USB record is dropped here and never logged
//! or forwarded (the daemon's privacy contract).
//!
//! Timing model (see the M9 security review):
//!
//! - `poll(2)` **blocks** on the fd and wakes the instant data is
//!   ready — edges are never delayed by the tick.
//! - The poll timeout is the re-enumeration tick (`TICK`): the unit
//!   runs in a private network namespace, so kernel uevents never
//!   reach a udev monitor inside it — periodic sysfs re-enumeration
//!   is the only reliable hotplug signal.
//! - Events carry the **device timestamp** from the usbmon header, so
//!   a PRESSED/RELEASED pair drained in one pass still measures the
//!   real hold duration downstream.

use std::fs::File;
use std::os::unix::io::AsFd;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use hiview_core::{ButtonEdge, snap_button_event};
use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};

use crate::usbdev::{Target, UsbDev, enumerate, resolve};
use crate::usbmon_ffi::{UsbmonPacket, fetch_event, record_of};

/// How often sysfs is re-enumerated: the hotplug signal under
/// `PrivateNetwork=yes` (udev uevents can't cross netns boundaries).
/// 1.5 s — the poll still wakes instantly on button data.
const TICK: Duration = Duration::from_millis(1_500);

/// Runs the reader forever: resolves the target, keeps its usbmon bus
/// open, forwards button edges to `events` and re-enumerates on every
/// tick so a replugged scope's new devnum is picked up.
pub fn run(events: &Sender<ButtonEdge>) -> ! {
    let mut target: Option<Target> = None;
    let mut open: Option<(u16, File)> = None;
    let mut data = Vec::with_capacity(256);
    loop {
        let Some(t) = target else {
            open = None; // scope gone — the bus fd is worthless to keep
            thread::sleep(TICK);
            re_enumerate(&mut target, enumerate);
            continue;
        };
        if open.as_ref().map(|(bus, _)| *bus) != Some(t.busnum) {
            open = open_bus(t.busnum);
            if open.is_none() {
                thread::sleep(TICK);
                re_enumerate(&mut target, enumerate);
                continue;
            }
        }
        let Some((_, file)) = open.as_ref() else {
            continue;
        };
        if wait_readable(file, TICK) {
            drain(file, &mut data, t, events);
        }
        // Timeout or a drained batch: refresh the target — the devnum
        // may have changed while records flowed.
        re_enumerate(&mut target, enumerate);
    }
}

/// Re-resolves the microscope and logs a target change — the whole
/// hotplug handling under a private netns. `enumerate` is injected so
/// the devnum-change contract is testable without sysfs.
fn re_enumerate(target: &mut Option<Target>, mut enumerate: impl FnMut() -> Vec<UsbDev>) {
    let next = resolve(&enumerate());
    if *target != next {
        match next {
            Some(t) => tracing::info!(busnum = t.busnum, devnum = t.devnum, "microscope present"),
            None => tracing::info!("microscope absent"),
        }
        *target = next;
    }
}

/// Whether a record is readable within `timeout` — `POLLIN` wakes it
/// instantly; `POLLERR`/`POLLNVAL`/`POLLHUP` count as readable so the
/// following `fetch_event` reports the real errno.
fn wait_readable(file: &File, timeout: Duration) -> bool {
    let ms = u16::try_from(timeout.as_millis()).unwrap_or(u16::MAX);
    let mut fds = [PollFd::new(file.as_fd(), PollFlags::POLLIN)];
    match poll(&mut fds, PollTimeout::from(ms)) {
        Ok(n) => n > 0,
        Err(Errno::EINTR) => false,
        Err(e) => {
            tracing::warn!(errno = ?e, "usbmon poll failed");
            false
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

/// Drains every queued record after a readable poll, forwarding each
/// Snap edge stamped with its device timestamp.
fn drain(file: &File, data: &mut Vec<u8>, target: Target, events: &Sender<ButtonEdge>) {
    loop {
        match fetch_event(file.as_fd(), data) {
            Ok(packet) => {
                if let Some(edge) = event_from_packet(&packet, data, target)
                    && events.send(edge).is_err()
                {
                    // Publisher gone — the daemon has no purpose left;
                    // exit so systemd's restart policy decides.
                    std::process::exit(0);
                }
            }
            // No more records right now — the fd is non-blocking
            // (EWOULDBLOCK == EAGAIN on Linux).
            Err(Errno::EAGAIN) => break,
            Err(Errno::EINTR) => continue,
            Err(e) => {
                tracing::warn!(errno = ?e, "usbmon read failed; reopening");
                break;
            }
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

/// Header+payload → a [`ButtonEdge`] iff the record is the Snap
/// button on `target` (the single place raw USB bytes cross into the
/// app's vocabulary). The edge carries the DEVICE timestamp — press
/// durations downstream never depend on delivery latency.
fn event_from_packet(packet: &UsbmonPacket, data: &[u8], target: Target) -> Option<ButtonEdge> {
    snap_button_event(&record_of(packet, data), target.busnum, target.devnum).map(|event| {
        ButtonEdge {
            event,
            ts_us: u64::try_from(record_of(packet, data).ts_us).unwrap_or(0),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complete-interrupt record on ep 0x83 carrying `data`,
    /// timestamped `ts_us`.
    fn packet(busnum: u16, devnum: u8, len_cap: usize, ts_us: i64) -> UsbmonPacket {
        UsbmonPacket {
            event_type: b'C',
            xfer_type: 1,
            epnum: 0x83,
            devnum,
            busnum,
            len_cap: len_cap as u32,
            ts_sec: ts_us / 1_000_000,
            ts_usec: i32::try_from(ts_us % 1_000_000).unwrap_or(0),
            ..UsbmonPacket::default()
        }
    }

    fn target() -> Target {
        Target {
            busnum: 3,
            devnum: 7,
        }
    }

    fn scope(busnum: u16, devnum: u8) -> UsbDev {
        UsbDev {
            vid: crate::usbdev::MICROSCOPE_VID,
            pid: crate::usbdev::MICROSCOPE_PID,
            busnum,
            devnum,
        }
    }

    #[test]
    fn real_pressed_bytes_emit_stamped_pressed() {
        let p = packet(3, 7, 4, 12_345_678);
        assert_eq!(
            event_from_packet(&p, &[0x02, 0x01, 0x00, 0x01], target()),
            Some(ButtonEdge {
                event: hiview_core::ButtonEvent::Pressed,
                ts_us: 12_345_678,
            })
        );
    }

    #[test]
    fn real_released_bytes_emit_stamped_released() {
        let p = packet(3, 7, 4, 13_045_678);
        let edge = event_from_packet(&p, &[0x02, 0x01, 0x00, 0x00], target());
        assert_eq!(
            edge.map(|e| (e.event, e.ts_us)),
            Some((hiview_core::ButtonEvent::Released, 13_045_678))
        );
        // The hold duration is a device-clock delta — a PRESSED drained
        // in the same pass still measures the real ~700 ms.
    }

    #[test]
    fn wrong_devnum_is_dropped() {
        let p = packet(3, 9, 4, 1);
        assert_eq!(
            event_from_packet(&p, &[0x02, 0x01, 0x00, 0x01], target()),
            None
        );
    }

    #[test]
    fn submit_records_are_dropped() {
        let mut p = packet(3, 7, 4, 1);
        p.event_type = b'S';
        assert_eq!(
            event_from_packet(&p, &[0x02, 0x01, 0x00, 0x01], target()),
            None
        );
    }

    #[test]
    fn short_payloads_are_dropped() {
        let p = packet(3, 7, 2, 1);
        assert_eq!(event_from_packet(&p, &[0x02, 0x01], target()), None);
    }

    /// Replug contract under `PrivateNetwork=yes`: the periodic
    /// re-enumeration tick must pick up the new devnum — this is the
    /// mechanism that replaces the udev monitor.
    #[test]
    fn tick_picks_up_devnum_change_on_replug() {
        let mut target = Some(Target {
            busnum: 3,
            devnum: 7,
        });
        // Plugged out then back in: same bus, new devnum.
        re_enumerate(&mut target, || vec![scope(3, 12)]);
        assert_eq!(
            target,
            Some(Target {
                busnum: 3,
                devnum: 12
            })
        );
    }

    #[test]
    fn tick_detects_unplug_and_replug() {
        let mut target = Some(Target {
            busnum: 3,
            devnum: 7,
        });
        re_enumerate(&mut target, Vec::new);
        assert_eq!(target, None, "unplug must clear the target");
        re_enumerate(&mut target, || vec![scope(3, 15)]);
        assert_eq!(target.map(|t| t.devnum), Some(15));
    }

    #[test]
    fn tick_is_quiet_when_nothing_changed() {
        let mut target = Some(Target {
            busnum: 3,
            devnum: 7,
        });
        let mut calls = 0u32;
        re_enumerate(&mut target, || {
            calls += 1;
            vec![scope(3, 7)]
        });
        assert_eq!(target.unwrap_or_else(|| panic!("gone")).devnum, 7);
        assert_eq!(calls, 1, "enumerates exactly once per tick");
    }
}
