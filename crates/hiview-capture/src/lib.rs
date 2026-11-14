// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Video capture hardware layer for HiView: camera discovery (udev/V4L2),
//! hotplug monitoring and GStreamer pipelines delivering frames to the UI.

#[cfg(feature = "test-support")]
mod delayed_bin;
mod device;
mod error;
mod events;
mod frame;
mod pipeline;
mod recorder;
mod thumbs;

pub use device::{CameraDescriptor, CaptureMode, DeviceWatcher, enumerate};
pub use error::Error;
pub use events::{CameraEvent, CameraState};
pub use frame::{Frame, FrameBus, FrameSubscription, PixelFormat};
pub use pipeline::{CapturePipeline, CaptureSource};
pub use recorder::{
    AudioDevice, AudioSource, ClockRef, MIN_FREE_BYTES, RecordDenied, RecordEvent, RecordStop,
    Recorder, RecorderOptions, VideoEncoder, audio_sources, available_encoder, check_disk_space,
};
pub use thumbs::{VideoProbe, probe_video};

/// Initializes GStreamer. Idempotent and safe to call from any thread.
///
/// # Errors
/// Returns an error if `gst_init` fails.
pub fn init() -> Result<(), Error> {
    gst::init().map_err(Error::Init)
}
