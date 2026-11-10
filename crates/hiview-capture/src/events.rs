// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Camera state machine events and states (docs/ARCHITECTURE.md §6.3).

use crate::device::CameraDescriptor;

/// High-level camera state exposed to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraState {
    /// No usable camera found yet; watching for one.
    #[default]
    Searching,
    /// A camera was selected and its pipeline is starting.
    Starting,
    /// Frames are flowing.
    Streaming,
    /// The active camera was unplugged; waiting for it to return.
    Disconnected,
    /// The capture pipeline failed; details are in the log.
    Error,
}

/// Asynchronous camera event produced by device monitoring or the pipeline bus.
#[derive(Debug, Clone)]
pub enum CameraEvent {
    /// A video capture node appeared (enumerated or hot-plugged).
    Connected(CameraDescriptor),
    /// A video capture node disappeared.
    Disconnected(CameraDescriptor),
    /// The pipeline reached `Playing`; frames are expected.
    Streaming,
    /// A fatal pipeline error; the payload is a technical summary for the log.
    Error(String),
}
