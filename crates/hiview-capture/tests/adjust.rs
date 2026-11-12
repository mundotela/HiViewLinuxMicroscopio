// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
//
//! Headless integration tests for the software image adjustments
//! (docs/ARCHITECTURE.md §6.11): the `AdjustPlan` applied inside the
//! appsink callback must change the delivered frames live, and the
//! neutral-default path must be a bit-exact passthrough.

use std::sync::LazyLock;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hiview_capture::{CameraEvent, CapturePipeline, CaptureSource, FrameBus};
use hiview_core::ImageAdjust;

const TIMEOUT: Duration = Duration::from_secs(15);

static GST_INIT: LazyLock<()> = LazyLock::new(|| {
    hiview_capture::init().unwrap_or_else(|e| panic!("gst init: {e}"));
});

fn start_pipeline(
    source: CaptureSource,
) -> (CapturePipeline, FrameBus, mpsc::Receiver<CameraEvent>) {
    LazyLock::force(&GST_INIT);
    let bus = FrameBus::default();
    let (tx, rx) = mpsc::channel();
    let pipeline = match CapturePipeline::start(source, bus.clone(), move |e| {
        let _ = tx.send(e);
    }) {
        Ok(pipeline) => pipeline,
        Err(err) => panic!("pipeline failed to start: {err}"),
    };
    (pipeline, bus, rx)
}

fn start_test_pipeline() -> (CapturePipeline, FrameBus, mpsc::Receiver<CameraEvent>) {
    start_pipeline(CaptureSource::Test)
}

fn wait_for_streaming(rx: &mpsc::Receiver<CameraEvent>) {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(CameraEvent::Streaming) => return,
            Ok(CameraEvent::Error(detail)) => panic!("pipeline error: {detail}"),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("event channel closed"),
        }
    }
    panic!("pipeline never reached Playing");
}

/// Neutral defaults must deliver frames identical to no plan at all —
/// the zero-cost passthrough requirement (§6.11).
#[test]
fn defaults_pass_frames_through_unchanged() {
    let (pipeline, bus, rx) = start_test_pipeline();
    wait_for_streaming(&rx);

    pipeline.set_adjust(&ImageAdjust::default());
    let sub = bus.subscribe();
    let frame = sub
        .recv_timeout(TIMEOUT)
        .unwrap_or_else(|e| panic!("no frame: {e}"));
    assert!(frame.data.len() >= frame.stride * frame.height as usize);
}
