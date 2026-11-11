// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Video thumbnail probe (docs/ARCHITECTURE.md §6.8): pulls the first
//! decodable frame of an MP4 through the same decoder the player uses
//! (`filesrc → qtdemux → h264parse → openh264dec → videoconvert →
//! RGBA`) and reads the clip duration from the demuxer.
//!
//! Runs on the thumbnail worker thread, never the UI thread. Any
//! failure — corrupt file, missing decoder, timeout — comes back as an
//! [`Error`], which the gallery maps to the placeholder tile.

use std::path::Path;
use std::time::Duration;

use gst::prelude::*;

use crate::error::Error;

/// How long the probe waits for the first decoded frame before giving
/// up on the file.
const PULL_TIMEOUT: gst::ClockTime = gst::ClockTime::from_seconds(5);

/// First decoded frame plus the clip duration.
pub struct VideoProbe {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Tightly packed RGBA8 pixels (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
    /// Clip duration; `None` when the demuxer does not report one.
    pub duration: Option<Duration>,
}

fn make_element(name: &'static str) -> Result<gst::Element, Error> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|source| Error::ElementUnavailable { name, source })
}

/// Pulls the first decodable frame of the MP4 at `path` together with
/// its duration.
///
/// # Errors
/// Returns [`Error`] when any element is missing, the file cannot be
/// demuxed/decoded, or no frame arrives within [`PULL_TIMEOUT`].
pub fn probe_video(path: &Path) -> Result<VideoProbe, Error> {
    crate::init()?;

    let pipeline = gst::Pipeline::new();
    let src = make_element("filesrc").inspect(|e| e.set_property("location", path))?;
    let demux = make_element("qtdemux")?;
    let parse = make_element("h264parse")?;
    let decode = make_element("openh264dec")?;
    let convert = make_element("videoconvert")?;
    let capsfilter = make_element("capsfilter").inspect(|e| {
        e.set_property(
            "caps",
            gst::Caps::builder("video/x-raw")
                .field("format", "RGBA")
                .build(),
        )
    })?;
    let sink = make_element("appsink").inspect(|e| {
        e.set_property("max-buffers", 1u32);
        e.set_property("drop", true);
    })?;
    let sink = sink
        .downcast::<gst_app::AppSink>()
        .map_err(|_| Error::VideoDevice("appsink downcast failed".into()))?;

    pipeline.add_many([
        &src,
        &demux,
        &parse,
        &decode,
        &convert,
        &capsfilter,
        sink.upcast_ref(),
    ])?;
    src.link(&demux)?;
    gst::Element::link_many([&parse, &decode, &convert, &capsfilter, sink.upcast_ref()])?;

    // qtdemux exposes its src pad only once it parsed the headers.
    let parse_sink = parse
        .static_pad("sink")
        .ok_or_else(|| Error::VideoDevice("h264parse has no sink pad".into()))?;
    demux.connect_pad_added(move |_, pad| {
        let linked = pad
            .current_caps()
            .or_else(|| Some(pad.query_caps(None)))
            .and_then(|caps| caps.structure(0).map(|s| s.name().starts_with("video/")))
            .unwrap_or(false)
            && pad.link(&parse_sink).is_ok();
        if !linked {
            tracing::debug!("thumb probe: ignoring demux pad {}", pad.name());
        }
    });

    pipeline.set_state(gst::State::Playing)?;
    let result = pull_first_frame(&pipeline, &sink, path);
    if let Err(err) = pipeline.set_state(gst::State::Null) {
        tracing::warn!("thumb probe pipeline did not reach Null: {err}");
    }
    result
}

/// Waits for the first decoded sample and reads the stream duration.
fn pull_first_frame(
    pipeline: &gst::Pipeline,
    sink: &gst_app::AppSink,
    path: &Path,
) -> Result<VideoProbe, Error> {
    let sample = sink.try_pull_sample(PULL_TIMEOUT).ok_or_else(|| {
        Error::VideoDevice(format!(
            "no decodable frame in {} within {PULL_TIMEOUT:?}",
            path.display()
        ))
    })?;
    let duration = pipeline
        .query_duration::<gst::ClockTime>()
        .map(|d| Duration::from_nanos(d.nseconds()));
    let caps = sample
        .caps()
        .ok_or_else(|| Error::VideoDevice("decoded sample without caps".into()))?;
    let info = gst_video::VideoInfo::from_caps(caps)
        .map_err(|e| Error::VideoDevice(format!("unusable video caps: {e}")))?;
    let buffer = sample
        .buffer()
        .ok_or_else(|| Error::VideoDevice("decoded sample without buffer".into()))?;
    let map = buffer
        .map_readable()
        .map_err(|e| Error::VideoDevice(format!("unreadable frame: {e}")))?;

    let (width, height, stride) = (info.width(), info.height(), info.stride()[0] as usize);
    let row_bytes = width as usize * 4;
    let mut rgba = vec![0u8; row_bytes * height as usize];
    for y in 0..height as usize {
        let src = y * stride;
        let dst = y * row_bytes;
        if src + row_bytes > map.as_slice().len() {
            return Err(Error::VideoDevice("decoded frame shorter than caps".into()));
        }
        rgba[dst..dst + row_bytes].copy_from_slice(&map.as_slice()[src..src + row_bytes]);
    }

    Ok(VideoProbe {
        width,
        height,
        rgba,
        duration,
    })
}
