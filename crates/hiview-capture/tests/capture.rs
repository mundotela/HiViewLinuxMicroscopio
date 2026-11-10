// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Headless integration tests: `videotestsrc` proves the pipeline path up to
//! the `FrameBus` without any hardware, and a generated Theora clip proves the
//! file source path (decode + appsink). Both must run in CI.

use std::sync::LazyLock;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gst::prelude::*;
use hiview_capture::{
    CameraEvent, CapturePipeline, CaptureSource, FrameBus, PixelFormat, enumerate,
};

const TIMEOUT: Duration = Duration::from_secs(15);

static GST_INIT: LazyLock<()> = LazyLock::new(|| {
    hiview_capture::init().unwrap_or_else(|e| panic!("gst init: {e}"));
});

fn wait_for_frame(bus: &FrameBus) -> Option<hiview_capture::Frame> {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Some(frame) = bus.take_latest_for_ui() {
            return Some(frame);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

fn wait_for_streaming(rx: &mpsc::Receiver<CameraEvent>) -> bool {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(CameraEvent::Streaming) => return true,
            Ok(CameraEvent::Error(detail)) => panic!("pipeline error: {detail}"),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
    false
}

#[test]
fn test_source_delivers_640x480_rgba_frames() {
    LazyLock::force(&GST_INIT);
    let bus = FrameBus::default();
    let (tx, rx) = mpsc::channel();

    let pipeline = match CapturePipeline::start(CaptureSource::Test, bus.clone(), move |e| {
        let _ = tx.send(e);
    }) {
        Ok(pipeline) => pipeline,
        Err(err) => panic!("pipeline failed to start: {err}"),
    };

    assert!(wait_for_streaming(&rx), "pipeline never reached Playing");
    let frame = wait_for_frame(&bus).unwrap_or_else(|| panic!("no frame within {TIMEOUT:?}"));

    assert_eq!(frame.format, PixelFormat::Rgba);
    assert_eq!(frame.width, 640);
    assert_eq!(frame.height, 480);
    assert_eq!(frame.data.len(), frame.stride * 480);

    // Acceptance criterion: ≥ 30 frames delivered within 2 s (the milestone
    // CI check). The bus coalesces for the UI, so count via published_count.
    let before = bus.published_count();
    std::thread::sleep(Duration::from_secs(2));
    let delivered = bus.published_count() - before;
    assert!(delivered >= 30, "only {delivered} frames in 2 s");

    drop(pipeline);
}

#[test]
fn file_source_decodes_and_delivers_frames() {
    // Encoded with plugins-base only (theora/ogg) so the test works in CI
    // without gst-libav or plugins-bad.
    let dir = std::env::temp_dir().join(format!("hiview-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir {}: {e}", dir.display()));
    let file = dir.join("sample.ogg");

    LazyLock::force(&GST_INIT);
    encode_clip(&file);

    let bus = FrameBus::default();
    let (tx, rx) = mpsc::channel();
    let pipeline =
        match CapturePipeline::start(CaptureSource::File(file.clone()), bus.clone(), move |e| {
            let _ = tx.send(e);
        }) {
            Ok(pipeline) => pipeline,
            Err(err) => panic!("file pipeline failed to start: {err}"),
        };

    assert!(
        wait_for_streaming(&rx),
        "file pipeline never reached Playing"
    );
    let frame = wait_for_frame(&bus).unwrap_or_else(|| panic!("no decoded frame"));
    assert_eq!(frame.format, PixelFormat::Rgba);

    // The clip is ~0.3 s long; the second read must still deliver frames,
    // proving the EOS loop-back works.
    std::thread::sleep(Duration::from_millis(600));
    let later = wait_for_frame(&bus);
    assert!(later.is_some(), "no frames after loop-back");

    drop(pipeline);
}

/// Writes a ~0.3 s Theora clip via a one-shot encoder pipeline
/// (`gst::parse::launch` is allowed in tests).
fn encode_clip(path: &std::path::Path) {
    let pipeline = gst::parse::launch(&format!(
        "videotestsrc num-buffers=10 ! video/x-raw,width=320,height=240 \
         ! videoconvert ! theoraenc ! oggmux ! filesink location={}",
        path.display()
    ))
    .unwrap_or_else(|e| panic!("encoder pipeline: {e}"))
    .downcast::<gst::Pipeline>()
    .unwrap_or_else(|_| panic!("encoder is not a pipeline"));

    pipeline
        .set_state(gst::State::Playing)
        .unwrap_or_else(|e| panic!("encoder start: {e}"));
    let bus = pipeline.bus().unwrap_or_else(|| panic!("no bus"));
    const TYPES: &[gst::MessageType] = &[gst::MessageType::Eos, gst::MessageType::Error];
    while let Some(msg) = bus.timed_pop_filtered(None::<gst::ClockTime>, TYPES) {
        match msg.view() {
            gst::MessageView::Eos(_) => break,
            gst::MessageView::Error(err) => panic!("encoder error: {}", err.error()),
            _ => {}
        }
    }
    pipeline
        .set_state(gst::State::Null)
        .unwrap_or_else(|e| panic!("encoder stop: {e}"));
}

/// Hardware-only check: verifies that the real microscope (05e3:f12a) is
/// discovered and delivers frames. Run explicitly with
/// `cargo test -p hiview-capture -- --ignored`.
#[test]
#[ignore = "requires microscope"]
fn microscope_is_discovered_and_streams() {
    LazyLock::force(&GST_INIT);
    let cameras = enumerate().unwrap_or_else(|e| panic!("enumerate: {e}"));
    let microscope = cameras
        .iter()
        .find(|c| c.is_microscope)
        .unwrap_or_else(|| panic!("microscope 05e3:f12a not found in {cameras:?}"));

    let bus = FrameBus::default();
    let (tx, rx) = mpsc::channel();
    let pipeline = match CapturePipeline::start(
        CaptureSource::Device(microscope.clone()),
        bus.clone(),
        move |e| {
            let _ = tx.send(e);
        },
    ) {
        Ok(pipeline) => pipeline,
        Err(err) => panic!("microscope pipeline failed: {err}"),
    };

    assert!(wait_for_streaming(&rx), "microscope never streamed");
    let frame = wait_for_frame(&bus).unwrap_or_else(|| panic!("no microscope frame"));
    assert_eq!(frame.width, 640);
    assert_eq!(frame.height, 480);

    // Measure the real delivery rate over 3 s.
    std::thread::sleep(Duration::from_secs(3));
    let before = bus.published_count();
    let started = Instant::now();
    std::thread::sleep(Duration::from_secs(3));
    let elapsed = started.elapsed().as_secs_f64();
    let fps = f64::from(u32::try_from(bus.published_count() - before).unwrap_or(0)) / elapsed;
    eprintln!("microscope delivered {fps:.1} fps over {elapsed:.1} s");
    assert!(fps >= 29.0, "microscope delivered only {fps:.1} fps");

    drop(pipeline);
}
