// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! [`EvdevSource`]: a `KEY_CAMERA` input device on the camera's USB
//! port (docs/ARCHITECTURE.md §6.10).
//!
//! The current kernel does not create an input device for this
//! microscope (`bTriggerUsage=0`), but the source is future-proof: if
//! a kernel update or the upstream `uvcvideo` quirk ever creates the
//! node, the button works with no helper at all — and this source is
//! preferred over the helper socket.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use tracing::{info, warn};

use crate::error::{Error, Result};
use crate::{ButtonEdge, ButtonEvent, ButtonSource, SourceKind, SourceState, SourceUpdate};

/// An input device that emits `KEY_CAMERA` on the camera's USB port.
pub struct EvdevSource {
    path: PathBuf,
}

impl EvdevSource {
    /// The device node this source reads (e.g. `/dev/input/event5`).
    #[must_use]
    pub fn dev_node(&self) -> &std::path::Path {
        &self.path
    }

    /// Finds an input device emitting `KEY_CAMERA` whose `phys` sits on
    /// the same USB port as the camera identified by `id_path` (udev
    /// `ID_PATH`, e.g. `pci-0000:00:14.0-usb-0:5:1.0`).
    ///
    /// # Errors
    /// Returns an error only if enumeration itself fails. No matching
    /// device is `Ok(None)`, not an error.
    pub fn probe(id_path: &str) -> Result<Option<Self>> {
        for (path, device) in evdev::enumerate() {
            let is_camera_button = device
                .supported_keys()
                .is_some_and(|keys| keys.contains(evdev::KeyCode::KEY_CAMERA))
                && device
                    .physical_path()
                    .is_some_and(|phys| phys_matches(phys, id_path));
            if is_camera_button {
                return Ok(Some(Self { path }));
            }
        }
        Ok(None)
    }
}

impl ButtonSource for EvdevSource {
    fn start(self: Box<Self>, tx: Sender<SourceUpdate>) -> Result<()> {
        let path = self.path.clone();
        thread::Builder::new()
            .name("hiview-button".into())
            .spawn(move || pump(&path, &tx))
            .map_err(Error::ThreadSpawn)?;
        Ok(())
    }
}

/// Maps an evdev `phys` string to the USB port it hangs on and compares
/// it with the camera's `ID_PATH` port.
///
/// `ID_PATH`: `pci-0000:00:14.0-usb-0:5:1.0` → USB port chain `5`.
/// evdev `phys`: `usb-0000:00:14.0-5/button` → same controller + port.
#[must_use]
pub fn phys_matches(phys: &str, id_path: &str) -> bool {
    expected_phys_prefix(id_path).is_some_and(|prefix| phys.starts_with(&prefix))
}

/// The `phys` prefix for the camera's USB port: `usb-<pci>-<ports>/`.
fn expected_phys_prefix(id_path: &str) -> Option<String> {
    let parts: Vec<&str> = id_path.split('-').collect();
    let usb_idx = parts.iter().position(|&p| p == "usb")?;
    // Token before "usb" is the PCI address ("0000:00:14.0"); the
    // token after carries "usb_bus:port_chain[:interface]".
    let pci = usb_idx.checked_sub(1).and_then(|i| parts.get(i))?;
    let port_chain = parts.get(usb_idx + 1)?.split(':').nth(1)?;
    Some(format!("usb-{pci}-{port_chain}/"))
}

/// Blocking read loop for the device (own thread). Reports Connected
/// once open and Disconnected if the node goes away (unplug); the
/// app re-probes on camera events.
fn pump(path: &std::path::Path, tx: &Sender<SourceUpdate>) {
    loop {
        match evdev::Device::open(path) {
            Ok(device) => {
                info!("snap button: reading {}", path.display());
                if !send(
                    tx,
                    SourceUpdate::State(SourceKind::Evdev, SourceState::Connected),
                ) {
                    return;
                }
                if !read_events(device, tx) {
                    return;
                }
            }
            Err(err) => {
                warn!("snap button: cannot open {}: {err}", path.display());
            }
        }
        if !send(
            tx,
            SourceUpdate::State(SourceKind::Evdev, SourceState::Disconnected),
        ) {
            return;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Forwards `KEY_CAMERA` edges until the device stops answering.
/// Returns `false` when the UI channel closed (app exiting).
fn read_events(mut device: evdev::Device, tx: &Sender<SourceUpdate>) -> bool {
    loop {
        let Ok(events) = device.fetch_events() else {
            return true; // device error/unplug → caller retries
        };
        for event in events {
            if event.event_type() != evdev::EventType::KEY
                || event.code() != evdev::KeyCode::KEY_CAMERA.0
            {
                continue;
            }
            let kind = match event.value() {
                1 => ButtonEvent::Pressed,
                0 => ButtonEvent::Released,
                // 2 = autorepeat — never an edge.
                _ => continue,
            };
            // The kernel stamps input events on CLOCK_MONOTONIC — the
            // same domain as the usbmon header and glib::monotonic_time.
            let ts_us = event
                .timestamp()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| u64::try_from(d.as_micros()).unwrap_or(0))
                .unwrap_or(0);
            if !send(tx, SourceUpdate::Event(ButtonEdge { event: kind, ts_us })) {
                return false;
            }
        }
    }
}

/// mpsc send that never panics: a dead UI ends the source.
fn send(tx: &Sender<SourceUpdate>, update: SourceUpdate) -> bool {
    tx.send(update).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMERA_ID_PATH: &str = "pci-0000:00:14.0-usb-0:5:1.0";

    /// The uvc input node for port 5 reports
    /// `usb-0000:00:14.0-5/<name>` — same controller, same port.
    #[test]
    fn phys_on_the_camera_port_matches() {
        assert!(phys_matches("usb-0000:00:14.0-5/button", CAMERA_ID_PATH));
        assert!(phys_matches("usb-0000:00:14.0-5/input0", CAMERA_ID_PATH));
    }

    /// A `KEY_CAMERA` device on another port (e.g. a webcam on port 4)
    /// must not match.
    #[test]
    fn phys_on_another_port_does_not_match() {
        assert!(!phys_matches("usb-0000:00:14.0-4/button", CAMERA_ID_PATH));
        // A different controller never matches either.
        assert!(!phys_matches("usb-0000:02:00.0-5/button", CAMERA_ID_PATH));
    }

    /// Chained hubs keep the full port chain on both sides.
    #[test]
    fn phys_through_a_hub_matches() {
        assert!(phys_matches(
            "usb-0000:00:14.0-5.4/button",
            "pci-0000:00:14.0-usb-0:5.4:1.0"
        ));
    }

    /// Malformed inputs never crash and never match.
    #[test]
    fn phys_garbage_does_not_match() {
        for (phys, id) in [
            ("", CAMERA_ID_PATH),
            ("ps2-serio-0/input0", CAMERA_ID_PATH),
            ("usb-0000:00:14.0-5", CAMERA_ID_PATH), // no trailing slash
            ("usb-0000:00:14.0-5/button", ""),
            ("usb-0000:00:14.0-5/button", "pci-0000:00:14.0"),
            ("usb-0000:00:14.0-5/button", "usb-0:5:1.0"),
        ] {
            assert!(!phys_matches(phys, id), "{phys} vs {id}");
        }
    }
}
