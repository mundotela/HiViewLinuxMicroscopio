// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Error type for the capture crate (docs/CONVENTIONS.md §5).

use std::fmt;

/// Errors produced by device discovery and pipeline construction.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A required GStreamer element is not installed on the system.
    #[error("gstreamer element '{name}' is not available: {source}")]
    ElementUnavailable {
        /// Element factory name (e.g. `"v4l2src"`).
        name: &'static str,
        /// The underlying gstreamer error.
        source: gst::glib::BoolError,
    },

    /// A GStreamer operation (add, link, property, ...) failed.
    #[error("gstreamer operation failed: {0}")]
    Gst(#[from] gst::glib::BoolError),

    /// Linking two pipeline pads failed.
    #[error("failed to link gstreamer pads: {0}")]
    Link(#[from] gst::PadLinkError),

    /// A pipeline state transition failed.
    #[error("pipeline state change failed: {0}")]
    StateChange(#[from] gst::StateChangeError),

    /// GStreamer library initialization failed.
    #[error("gstreamer initialization failed: {0}")]
    Init(#[from] gst::glib::Error),

    /// A udev enumeration or monitor operation failed.
    #[error("udev error: {0}")]
    Udev(#[from] std::io::Error),

    /// The device exposes no video-capture format we can use.
    #[error("device {0} exposes no usable capture format")]
    NoCaptureFormat(String),

    /// The `v4l2deviceprovider` GStreamer plugin is missing or failed to
    /// start, so camera nodes cannot be probed at all.
    #[error("v4l2 device provider unavailable: {0}")]
    NoV4l2Provider(String),

    /// The given device node could not be described as a video device.
    #[error("video device description failed: {0}")]
    VideoDevice(String),

    /// Spawning a pipeline support thread failed.
    #[error("failed to spawn thread '{name}': {source}")]
    ThreadSpawn {
        /// Thread name for diagnostics.
        name: &'static str,
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

impl Error {
    /// One-line technical summary for logging.
    #[must_use]
    pub fn summary(&self) -> String {
        fmt::format(format_args!("{self}"))
    }
}
