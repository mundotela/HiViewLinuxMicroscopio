// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
// `unsafe` lives ONLY in `usbmon_ffi` (crate-level `deny` + per-module
// `forbid` everywhere else — a root `forbid` would also forbid the
// one allowed module).
#![deny(unsafe_code)]

//! `hiview-buttond` — the root half of the Snap-button bridge
//! (ARCHITECTURE §6.10):
//!
//! ```text
//! udev → Target { busnum, devnum }          (hotplug re-resolve)
//!           │
//!   /dev/usbmon<bus> → record filter → PRESSED/RELEASED
//!           │
//!   /run/hiview/button.sock (0660, group hiview) → desktop app
//! ```
//!
//! The daemon publishes ONLY the two wire lines — no USB payload ever
//! leaves this process, and there is no network code at all.

mod reader;
mod sock;
mod usbdev;
mod usbmon_ffi;

use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;

use hiview_core::ButtonEvent;

/// Where the app picks up events — inside `RuntimeDirectory=hiview`
/// so systemd creates/cleans it.
const SOCK_PATH: &str = "/run/hiview/button.sock";

/// The group owning the socket — the installer puts the desktop user
/// in it so the app needs no privileges.
const SOCK_GROUP: &str = "hiview";

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
    tracing::info!("hiview-buttond starting");

    // Current device address; None while the scope is unplugged.
    let target: reader::SharedTarget = Mutex::new(usbdev::resolve(&usbdev::enumerate()));
    log_target(&target);
    let target = Arc::new(target);

    // Hotplug: any usb add/remove re-resolves — devnum changes on
    // every replug, so the reader must never cache it.
    {
        let shared = Arc::clone(&target);
        usbdev::watch_hotplug(move || {
            let next = usbdev::resolve(&usbdev::enumerate());
            let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
            if *t != next {
                *t = next;
                log_target(&shared);
            }
        });
    }

    // Unbounded is fine: button edges are a few per second at most.
    let (tx, rx) = channel::<ButtonEvent>();
    {
        let shared = Arc::clone(&target);
        thread::Builder::new()
            .name("hiview-usbmon".into())
            .spawn(move || reader::run(&shared, &tx))?;
    }

    // Blocks forever publishing events; Err = broken listener.
    sock::serve(Path::new(SOCK_PATH), Some(SOCK_GROUP), &rx)
}

fn log_target(target: &Mutex<Option<usbdev::Target>>) {
    match *target.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(t) => tracing::info!(busnum = t.busnum, devnum = t.devnum, "microscope present"),
        None => tracing::info!("microscope absent"),
    }
}
