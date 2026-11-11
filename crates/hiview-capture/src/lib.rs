// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Video capture hardware layer for HiView: camera discovery (udev/V4L2),
//! hotplug monitoring and GStreamer pipelines delivering frames to the UI.

mod device;
mod error;
mod events;
mod frame;
mod pipeline;
mod recorder;

pub use device::{CameraDescriptor, CaptureMode, DeviceWatcher, enumerate};
pub use error::Error;
pub use events::{CameraEvent, CameraState};
pub use frame::{Frame, FrameBus, FrameSubscription, PixelFormat};
pub use pipeline::{CapturePipeline, CaptureSource};
pub use recorder::{
    MIN_FREE_BYTES, RecordDenied, RecordStop, Recorder, VideoEncoder, available_encoder,
    check_disk_space,
};

/// Initializes GStreamer. Idempotent and safe to call from any thread.
///
/// # Errors
/// Returns an error if `gst_init` fails.
pub fn init() -> Result<(), Error> {
    gst::init().map_err(Error::Init)
}
