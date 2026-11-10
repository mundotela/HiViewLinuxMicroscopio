// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! GStreamer capture pipeline (docs/ARCHITECTURE.md §6.2).
//!
//! Elements are built and linked explicitly — `gst::parse::launch` is not used
//! in production code.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use gst::prelude::*;
use tracing::{debug, warn};

use hiview_core::Orientation;

use crate::device::CameraDescriptor;
use crate::error::Error;
use crate::events::CameraEvent;
use crate::frame::{Frame, FrameBus, PixelFormat};

/// Caps requested from the measured microscope (ARCHITECTURE.md §2):
/// YUYV 640x480 at 30 fps.
const MICROSCOPE_FORMAT: &str = "YUY2";
const MICROSCOPE_WIDTH: i32 = 640;
const MICROSCOPE_HEIGHT: i32 = 480;
const MICROSCOPE_FPS_NUM: i32 = 30;
const MICROSCOPE_FPS_DEN: i32 = 1;

/// What feeds the pipeline.
#[derive(Debug, Clone)]
pub enum CaptureSource {
    /// A physical camera described by discovery.
    Device(CameraDescriptor),
    /// `videotestsrc` synthetic source (also used in CI).
    Test,
    /// A media file replayed through `decodebin`, looping on EOS.
    File(PathBuf),
}

/// Owns a running GStreamer pipeline delivering frames to a [`FrameBus`].
///
/// `Drop` always transitions the pipeline to `Null` and stops the bus thread.
pub struct CapturePipeline {
    pipeline: gst::Pipeline,
    frame_bus: FrameBus,
    source: CaptureSource,
    dev_node: Option<PathBuf>,
    flip: Option<gst::Element>,
    bus_thread: Option<JoinHandle<()>>,
}

impl CapturePipeline {
    /// Builds and starts a pipeline for `source`.
    ///
    /// Frames are published to `frame_bus`; `on_event` receives `Streaming`
    /// once the pipeline reaches `Playing` and `Error` on a fatal bus error.
    /// The callback runs on the bus thread.
    ///
    /// # Errors
    /// Fails if GStreamer is missing elements, elements cannot be linked or
    /// the pipeline cannot enter `Playing`.
    pub fn start<F>(source: CaptureSource, frame_bus: FrameBus, on_event: F) -> Result<Self, Error>
    where
        F: Fn(CameraEvent) + Send + Sync + 'static,
    {
        crate::init()?;
        let pipeline = build_pipeline(&source, &frame_bus)?;
        let loop_playback = matches!(source, CaptureSource::File(_));
        let bus_thread = spawn_bus_thread(&pipeline, loop_playback, Arc::new(on_event))?;

        debug!("starting pipeline for {source:?}");
        pipeline.set_state(gst::State::Playing)?;

        Ok(Self {
            dev_node: match &source {
                CaptureSource::Device(desc) => Some(desc.dev_node.clone()),
                _ => None,
            },
            flip: pipeline.by_name("flip"),
            pipeline,
            frame_bus,
            source,
            bus_thread: Some(bus_thread),
        })
    }

    /// The frame bus this pipeline publishes to.
    #[must_use]
    pub fn frame_bus(&self) -> &FrameBus {
        &self.frame_bus
    }

    /// The source this pipeline was built from.
    #[must_use]
    pub fn source(&self) -> &CaptureSource {
        &self.source
    }

    /// Device node currently in use, for `Device` sources.
    #[must_use]
    pub fn dev_node(&self) -> Option<&PathBuf> {
        self.dev_node.as_ref()
    }

    /// Changes the `videoflip` `video-direction` live (docs/ARCHITECTURE.md
    /// §6.4). No-op while the pipeline element is unavailable.
    pub fn set_orientation(&self, orientation: Orientation) {
        let Some(flip) = &self.flip else {
            warn!("set_orientation: pipeline has no videoflip element");
            return;
        };
        flip.set_property(
            "video-direction",
            match orientation {
                Orientation::Identity => gst_video::VideoOrientationMethod::Identity,
                Orientation::Rot90Cw => gst_video::VideoOrientationMethod::_90r,
                Orientation::Rot180 => gst_video::VideoOrientationMethod::_180,
                Orientation::Rot90Ccw => gst_video::VideoOrientationMethod::_90l,
                Orientation::FlipH => gst_video::VideoOrientationMethod::Horiz,
                Orientation::FlipV => gst_video::VideoOrientationMethod::Vert,
                Orientation::Transpose => gst_video::VideoOrientationMethod::UlLr,
                Orientation::AntiTranspose => gst_video::VideoOrientationMethod::UrLl,
            },
        );
    }
}

impl Drop for CapturePipeline {
    fn drop(&mut self) {
        // Post a shutdown marker: it wakes the bus thread's timed_pop so it
        // can exit the loop and be joined (flushing alone is not enough).
        if let Some(bus) = self.pipeline.bus() {
            let shutdown =
                gst::message::Application::new(gst::Structure::new_empty("hiview-shutdown"));
            if let Err(err) = bus.post(shutdown) {
                warn!("failed to post shutdown message: {err}");
            }
        }
        if let Err(err) = self.pipeline.set_state(gst::State::Null) {
            warn!("failed to stop pipeline on drop: {err}");
        }
        if let Some(thread) = self.bus_thread.take() {
            let _ = thread.join();
        }
    }
}

fn make(name: &'static str) -> Result<gst::Element, Error> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|source| Error::ElementUnavailable { name, source })
}

fn caps_filter(caps: gst::Caps) -> Result<gst::Element, Error> {
    let filter = make("capsfilter")?;
    filter.set_property("caps", &caps);
    Ok(filter)
}

/// First frames a V4L2 device delivers are uninitialized driver buffers
/// (solid-color garbage; observed on the 05e3:f12a microscope). Skipping a
/// few keeps garbage off the live view and out of future snapshots (M3).
const DEVICE_WARMUP_FRAMES: u64 = 3;

/// `sync` paces delivery to buffer timestamps. It must stay off for live
/// sources: the device already paces capture, and syncing a 4-buffer appsink
/// queue overflows it and drops ~40% of frames. For files it keeps decode at
/// playback speed instead of max CPU.
///
/// `skip_first` discards that many initial samples (device warmup frames).
fn build_appsink(frame_bus: &FrameBus, sync: bool, skip_first: u64) -> gst_app::AppSink {
    let rgba = gst::Caps::builder("video/x-raw")
        .field("format", "RGBA")
        .build();
    let appsink = gst_app::AppSink::builder()
        .name("frame-sink")
        .caps(&rgba)
        .max_buffers(4)
        .drop(true)
        .sync(sync)
        .build();

    let bus = frame_bus.clone();
    let sample_count = std::sync::atomic::AtomicU64::new(0);
    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_preroll(|sink| {
                let _ = sink.pull_preroll();
                Ok(gst::FlowSuccess::Ok)
            })
            .new_sample(move |sink| {
                if sample_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < skip_first {
                    let _ = sink.pull_sample();
                    return Ok(gst::FlowSuccess::Ok);
                }
                match extract_frame(sink) {
                    Ok(Some(frame)) => {
                        bus.publish(frame);
                        Ok(gst::FlowSuccess::Ok)
                    }
                    Ok(None) => Ok(gst::FlowSuccess::Ok),
                    Err(err) => {
                        warn!("dropping undecodable sample: {err}");
                        Ok(gst::FlowSuccess::Ok)
                    }
                }
            })
            .build(),
    );
    appsink
}

fn extract_frame(sink: &gst_app::AppSink) -> Result<Option<Frame>, String> {
    let sample = match sink.pull_sample() {
        Ok(sample) => sample,
        // EOS/flushing during shutdown or seeks is not a failure.
        Err(_) => return Ok(None),
    };
    let caps = sample
        .caps()
        .ok_or_else(|| "sample without caps".to_string())?;
    let info = gst_video::VideoInfo::from_caps(caps)
        .map_err(|e| format!("unsupported caps {caps}: {e}"))?;
    let buffer = sample
        .buffer()
        .ok_or_else(|| "sample without buffer".to_string())?;
    let mapped = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
        .map_err(|e| format!("cannot map frame: {e}"))?;
    let data = mapped
        .plane_data(0)
        .map_err(|e| format!("cannot read plane: {e}"))?;

    Ok(Some(Frame {
        width: info.width(),
        height: info.height(),
        stride: usize::try_from(info.stride()[0]).unwrap_or(0),
        format: PixelFormat::Rgba,
        data: gst::glib::Bytes::from(data),
        pts: buffer.pts(),
    }))
}

fn build_pipeline(source: &CaptureSource, frame_bus: &FrameBus) -> Result<gst::Pipeline, Error> {
    let pipeline = gst::Pipeline::builder().name("hiview-capture").build();
    let sync = matches!(source, CaptureSource::File(_));
    let skip_first = match source {
        CaptureSource::Device(_) => DEVICE_WARMUP_FRAMES,
        _ => 0,
    };
    let appsink = build_appsink(frame_bus, sync, skip_first).upcast::<gst::Element>();
    let convert = make("videoconvert")?;
    // Kept between convert and appsink in every source so `set_orientation`
    // can swap `video-direction` live (docs/ARCHITECTURE.md §6.4).
    let flip = make("videoflip")?;
    flip.set_property("name", "flip");

    match source {
        CaptureSource::Test => {
            let src = make("videotestsrc")?;
            src.set_property("is-live", true);
            src.set_property_from_str("pattern", "ball");
            let caps = caps_filter(microscope_caps())?;
            pipeline.add_many([&src, &caps, &convert, &flip, &appsink])?;
            gst::Element::link_many([&src, &caps, &convert, &flip, &appsink])?;
        }
        CaptureSource::Device(desc) => {
            let src = make("v4l2src")?;
            src.set_property("device", desc.dev_node.to_string_lossy().as_ref());
            src.set_property("do-timestamp", true);
            pipeline.add_many([&src, &convert, &flip, &appsink])?;
            if desc.is_microscope {
                let caps = caps_filter(microscope_caps())?;
                pipeline.add(&caps)?;
                gst::Element::link_many([&src, &caps, &convert, &flip, &appsink])?;
            } else {
                gst::Element::link_many([&src, &convert, &flip, &appsink])?;
            }
        }
        CaptureSource::File(path) => {
            let src = make("filesrc")?;
            src.set_property("location", path.to_string_lossy().as_ref());
            let decodebin = make("decodebin")?;
            pipeline.add_many([&src, &decodebin, &convert, &flip, &appsink])?;
            gst::Element::link_many([&src, &decodebin])?;
            gst::Element::link_many([&convert, &flip, &appsink])?;
            link_decodebin_dynamically(&decodebin, &convert);
        }
    }

    Ok(pipeline)
}

fn microscope_caps() -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("format", MICROSCOPE_FORMAT)
        .field("width", MICROSCOPE_WIDTH)
        .field("height", MICROSCOPE_HEIGHT)
        .field(
            "framerate",
            gst::Fraction::new(MICROSCOPE_FPS_NUM, MICROSCOPE_FPS_DEN),
        )
        .build()
}

/// `decodebin` exposes source pads only after parsing; link them to the
/// converter as they appear and ignore non-video streams.
fn link_decodebin_dynamically(decodebin: &gst::Element, convert: &gst::Element) {
    let convert_weak = convert.downgrade();
    decodebin.connect_pad_added(move |_decodebin, src_pad| {
        let Some(convert) = convert_weak.upgrade() else {
            return;
        };
        let Some(sink_pad) = convert.static_pad("sink") else {
            return;
        };
        if sink_pad.is_linked() || !src_pad.can_link(&sink_pad) {
            return;
        }
        if let Err(err) = src_pad.link(&sink_pad) {
            warn!("failed to link decodebin pad {}: {err}", src_pad.name());
        }
    });
}

fn spawn_bus_thread(
    pipeline: &gst::Pipeline,
    loop_playback: bool,
    on_event: Arc<dyn Fn(CameraEvent) + Send + Sync>,
) -> Result<JoinHandle<()>, Error> {
    let bus = pipeline
        .bus()
        .ok_or_else(|| Error::VideoDevice("pipeline has no bus".into()))?;
    let pipeline = pipeline.clone();

    thread::Builder::new()
        .name("hiview-gst-bus".into())
        .spawn(move || {
            use gst::MessageView;
            const TYPES: &[gst::MessageType] = &[
                gst::MessageType::Error,
                gst::MessageType::Eos,
                gst::MessageType::StateChanged,
                gst::MessageType::Application,
            ];
            while let Some(msg) = bus.timed_pop_filtered(None::<gst::ClockTime>, TYPES) {
                match msg.view() {
                    MessageView::Error(err) => {
                        let detail = format!(
                            "{} ({})",
                            err.error(),
                            err.debug()
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "no debug info".into())
                        );
                        on_event(CameraEvent::Error(detail));
                        if let Err(err) = pipeline.set_state(gst::State::Null) {
                            warn!("failed to stop errored pipeline: {err}");
                        }
                        break;
                    }
                    MessageView::Eos(_) => {
                        if !loop_playback {
                            break;
                        }
                        if let Err(err) = pipeline.seek_simple(
                            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                            gst::ClockTime::ZERO,
                        ) {
                            warn!("loop seek failed: {err}");
                            break;
                        }
                    }
                    MessageView::StateChanged(changed) => {
                        let is_pipeline = msg.src() == Some(pipeline.upcast_ref::<gst::Object>());
                        if is_pipeline && changed.current() == gst::State::Playing {
                            on_event(CameraEvent::Streaming);
                        }
                    }
                    MessageView::Application(_) => break,
                    _ => {}
                }
            }
        })
        .map_err(|source| Error::ThreadSpawn {
            name: "hiview-gst-bus",
            source,
        })
}
