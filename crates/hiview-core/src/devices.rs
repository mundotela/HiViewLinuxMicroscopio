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
