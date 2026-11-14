// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Known microscope models, identified strictly by USB VID:PID (never by name).

/// A microscope model recognized by HiView.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownMicroscope {
    /// USB vendor ID.
    pub vid: u16,
    /// USB product ID.
    pub pid: u16,
    /// Internal model identifier (not user-facing).
    pub model: &'static str,
}

/// Microscopes recognized by HiView, in order of preference.
pub const KNOWN_MICROSCOPES: &[KnownMicroscope] = &[KnownMicroscope {
    vid: 0x05e3,
    pid: 0xf12a,
    model: "genesys-05e3-f12a",
}];

/// Whether the given USB VID:PID belongs to a known microscope.
#[must_use]
pub fn is_known_microscope(vid: u16, pid: u16) -> bool {
    KNOWN_MICROSCOPES
        .iter()
        .any(|m| m.vid == vid && m.pid == pid)
}

/// Per-device config key of the primary microscope (`"VID:PID"` —
/// §6.12). Calibration profiles created before M12 have no `device`
/// field and migrate to this key.
pub const MICROSCOPE_DEVICE_KEY: &str = "05e3:f12a";

/// The per-device config key for a USB identity (`"vid:pid"` in lower
/// hex) — shared by image adjustments (§6.11) and calibration (§6.9).
#[must_use]
pub fn usb_device_key(vid: u16, pid: u16) -> String {
    format!("{vid:04x}:{pid:04x}")
}
