// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
//
// `unsafe` lives ONLY in `usbmon_ffi` (crate-level `deny` + per-module
// `forbid` everywhere else — a root `forbid` would also forbid the
// one allowed module).
#![deny(unsafe_code)]

//! `hiview-buttond` — the root half of the Snap-button bridge
//! (ARCHITECTURE §6.10):
//!
//! ```text
//! /sys enumeration → Target { busnum, devnum }   (tick re-enumeration
//!           │                                     replaces udev uevents,
//!   /dev/usbmon<bus> → record filter →            which a private netns
//!           │                                     cannot receive)
//!   PRESSED <ts_us> / RELEASED <ts_us>
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
use std::thread;

use hiview_core::ButtonEdge;

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

    let (tx, rx) = channel::<ButtonEdge>();
    thread::Builder::new()
        .name("hiview-usbmon".into())
        .spawn(move || reader::run(&tx))?;

    // Blocks forever publishing events; Err = broken listener.
    sock::serve(Path::new(SOCK_PATH), Some(SOCK_GROUP), &rx)
}
