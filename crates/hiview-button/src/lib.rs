// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Physical Snap button abstraction (docs/ARCHITECTURE.md §6.10).
//!
//! Sources in order of preference:
//!
//! 1. [`EvdevSource`] — an input device with `KEY_CAMERA` on the same
//!    USB port as the camera. The current kernel does not create one
//!    for this microscope (`bTriggerUsage=0`), but the source starts
//!    working by itself if a future kernel or quirk adds it.
//! 2. [`HelperSource`] — a client of `hiview-buttond`'s Unix socket.
//! 3. Nothing — the button is unavailable; the app keeps working.
//!
//! Interpretation (short = photo, long = record) lives in the app,
//! not in the source.

mod error;
mod evdev_source;
mod helper;

use std::sync::mpsc::Sender;

pub use error::{Error, Result};
pub use evdev_source::EvdevSource;
pub use helper::{DEFAULT_SOCKET, HelperSource};
pub use hiview_core::{ButtonEdge, ButtonEvent};

/// How the button is currently reaching the app (preferences status).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Direct evdev input device.
    Evdev,
    /// The `hiview-buttond` socket.
    Helper,
}

/// Connectivity state pushed by a source so the UI can show a live
/// "conectado / não disponível" status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    /// The transport is delivering events.
    Connected,
    /// The transport dropped (daemon died, device unplugged); the
    /// source keeps retrying in the background.
    Disconnected,
}

/// Everything a source can report to the UI thread.
///
/// Deviation from §6.10's `Sender<ButtonEvent>`: the status row in
/// Preferences needs connectivity changes, not only button edges, so
/// the channel carries this enum instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceUpdate {
    /// A physical button edge, stamped with the device clock (µs) —
    /// press durations must use it, never delivery time.
    Event(ButtonEdge),
    /// Transport connectivity changed (kind of the reporting source).
    State(SourceKind, SourceState),
}

/// A source of physical Snap button events (§6.10).
///
/// `start` opens what it needs, spawns its worker thread and returns;
/// events and state changes arrive on `tx` from that thread.
pub trait ButtonSource: Send {
    /// Opens the transport and starts the worker thread.
    ///
    /// # Errors
    /// Returns an error if the worker thread cannot be spawned. A
    /// missing transport (no socket, no device) is NOT an error: the
    /// worker retries and reports [`SourceState`] instead.
    fn start(self: Box<Self>, tx: Sender<SourceUpdate>) -> Result<()>;
}

/// Picks the best available source (§6.10 order): evdev when a
/// `KEY_CAMERA` input device exists on the camera's USB port, the
/// helper socket otherwise.
///
/// `usb_path` is the camera's udev `ID_PATH` (e.g.
/// `pci-0000:00:14.0-usb-0:5:1.0`); `None` skips the evdev probe.
#[must_use]
pub fn pick_source(usb_path: Option<&str>) -> Box<dyn ButtonSource> {
    if let Some(path) = usb_path
        && let Ok(Some(source)) = EvdevSource::probe(path)
    {
        tracing::info!(
            "snap button: using evdev device {}",
            source.dev_node().display()
        );
        return Box::new(source);
    }
    tracing::info!("snap button: falling back to the helper socket");
    Box::new(HelperSource::system())
}
