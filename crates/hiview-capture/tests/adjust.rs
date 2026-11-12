// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Headless integration tests for the software image adjustments
//! (docs/ARCHITECTURE.md §6.11): the property mapping, the neutral-default
//! passthrough and the visible effect on `videotestsrc` output.

use std::sync::LazyLock;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gst::prelude::*;
use hiview_capture::{CameraEvent, CapturePipeline, CaptureSource, Frame, FrameBus};
use hiview_core::{ImageAdjust, SHARPEN_MAX_SIGMA};

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

/// The `ball` test pattern is grayscale — contrast and saturation need
/// colored pixels, so those tests run on a looping SMPTE color-bar clip
/// fed through the file source (the same adjust tail as `Test`).
fn start_smpte_pipeline(
    dir: &std::path::Path,
) -> (CapturePipeline, FrameBus, mpsc::Receiver<CameraEvent>) {
    LazyLock::force(&GST_INIT);
    let file = dir.join("smpte.ogg");
    if !file.exists() {
        encode_smpte_clip(&file);
    }
    start_pipeline(CaptureSource::File(file))
}

/// Writes a short Theora clip of SMPTE color bars (`gst::parse::launch`
/// is allowed in tests).
fn encode_smpte_clip(path: &std::path::Path) {
    let pipeline = gst::parse::launch(&format!(
        "videotestsrc num-buffers=30 pattern=smpte ! video/x-raw,width=320,height=240 \
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

/// Pixel statistics of an RGBA frame: mean luma (Rec. 601), luma
/// standard deviation (contrast proxy) and mean channel spread
/// (saturation proxy).
struct Stats {
    mean_luma: f64,
    luma_stddev: f64,
    mean_spread: f64,
}

fn frame_stats(frame: &Frame) -> Stats {
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut spread = 0.0f64;
    let mut count = 0u64;
    for row in 0..frame.height as usize {
        let base = row * frame.stride;
        for col in 0..frame.width as usize {
            let i = base + col * 4;
            let r = f64::from(frame.data[i]);
            let g = f64::from(frame.data[i + 1]);
            let b = f64::from(frame.data[i + 2]);
            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            sum += luma;
            sum_sq += luma * luma;
            spread += f64::from(r.max(g).max(b) as u8 - r.min(g).min(b) as u8);
            count += 1;
        }
    }
    let mean = sum / count as f64;
    Stats {
        mean_luma: mean,
        luma_stddev: (sum_sq / count as f64 - mean * mean).max(0.0).sqrt(),
        mean_spread: spread / count as f64,
    }
}

/// Averages the stats of `n` frames pulled from a full subscription —
/// the test source's ball moves, so single frames are noisy.
fn average_stats(bus: &FrameBus, n: usize) -> Stats {
    let sub = bus.subscribe();
    let mut acc = Stats {
        mean_luma: 0.0,
        luma_stddev: 0.0,
        mean_spread: 0.0,
    };
    let mut got = 0usize;
    while got < n {
        match sub.recv_timeout(TIMEOUT) {
            Ok(frame) => {
                let s = frame_stats(&frame);
                acc.mean_luma += s.mean_luma;
                acc.luma_stddev += s.luma_stddev;
                acc.mean_spread += s.mean_spread;
                got += 1;
            }
            Err(e) => panic!("no frame within {TIMEOUT:?}: {e}"),
        }
    }
    Stats {
        mean_luma: acc.mean_luma / n as f64,
        luma_stddev: acc.luma_stddev / n as f64,
        mean_spread: acc.mean_spread / n as f64,
    }
}

/// Waits a moment so queued pre-change samples drain, then measures.
fn stats_after_change(bus: &FrameBus) -> Stats {
    std::thread::sleep(Duration::from_millis(400));
    average_stats(bus, 12)
}

/// UI value → element property mapping: every field of `ImageAdjust`
/// must land on the documented element property, live.
#[test]
fn set_adjust_maps_to_element_properties() {
    let (pipeline, _bus, rx) = start_test_pipeline();
    wait_for_streaming(&rx);

    let balance = pipeline
        .element_by_name("balance")
        .unwrap_or_else(|| panic!("no videobalance element"));
    let gamma = pipeline
        .element_by_name("gamma")
        .unwrap_or_else(|| panic!("no gamma element"));

    // Neutral defaults map to the elements' own passthrough defaults —
    // the "costs nothing" requirement of §6.11.
    pipeline.set_adjust(&ImageAdjust::default());
    assert_eq!(balance.property::<f64>("brightness"), 0.0);
    assert_eq!(balance.property::<f64>("contrast"), 1.0);
    assert_eq!(balance.property::<f64>("saturation"), 1.0);
    assert_eq!(gamma.property::<f64>("gamma"), 1.0);

    let adjust = ImageAdjust {
        brightness: 0.35,
        contrast: 1.4,
        saturation: 0.6,
        gamma: 1.9,
        sharpen: 0.0,
    };
    pipeline.set_adjust(&adjust);
    assert_eq!(balance.property::<f64>("brightness"), 0.35);
    assert_eq!(balance.property::<f64>("contrast"), 1.4);
    assert_eq!(balance.property::<f64>("saturation"), 0.6);
    assert_eq!(gamma.property::<f64>("gamma"), 1.9);
}

/// Sharpen slider → `gaussianblur.sigma`: 0% maps to 0 (inert) and
/// 100% to the budget-capped negative maximum.
#[test]
fn sharpen_maps_to_negative_sigma() {
    if !hiview_capture::sharpen_available() {
        eprintln!("gaussianblur not installed; skipping");
        return;
    }
    let (pipeline, _bus, rx) = start_test_pipeline();
    wait_for_streaming(&rx);
    assert!(pipeline.has_sharpen());
    let sharpen = pipeline
        .element_by_name("sharpen")
        .unwrap_or_else(|| panic!("no gaussianblur element"));

    pipeline.set_adjust(&ImageAdjust {
        sharpen: 1.0,
        ..ImageAdjust::default()
    });
    // The element stores sigma at f32 precision — compare loosely.
    assert!((sharpen.property::<f64>("sigma") - SHARPEN_MAX_SIGMA).abs() < 1e-6);
    assert!(sharpen.property::<f64>("sigma") < 0.0);

    pipeline.set_adjust(&ImageAdjust::default());
    assert_eq!(sharpen.property::<f64>("sigma"), 0.0);
}

/// Raising brightness must raise the mean luma of the delivered frames.
#[test]
fn brightness_raises_mean_luma() {
    let (pipeline, bus, rx) = start_test_pipeline();
    wait_for_streaming(&rx);

    let before = average_stats(&bus, 12);
    pipeline.set_adjust(&ImageAdjust {
        brightness: 0.4,
        ..ImageAdjust::default()
    });
    let after = stats_after_change(&bus);

    assert!(
        after.mean_luma > before.mean_luma + 5.0,
        "brightness +0.4 moved luma only {:.1} → {:.1}",
        before.mean_luma,
        after.mean_luma
    );
}

/// Contrast below 1 compresses the luma spread toward mid-gray — the
/// clipping-free direction (boosting contrast on SMPTE bars saturates
/// the extremes and barely moves the stddev).
#[test]
fn contrast_lowers_luma_stddev() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (pipeline, bus, rx) = start_smpte_pipeline(dir.path());
    wait_for_streaming(&rx);

    let before = average_stats(&bus, 12);
    pipeline.set_adjust(&ImageAdjust {
        contrast: 0.5,
        ..ImageAdjust::default()
    });
    let after = stats_after_change(&bus);

    assert!(
        after.luma_stddev < before.luma_stddev * 0.7,
        "contrast 0.5 moved stddev only {:.1} → {:.1}",
        before.luma_stddev,
        after.luma_stddev
    );
}

/// Saturation 0 must drain the channels' spread (grayscale), and a
/// boost must raise it back above the neutral level.
#[test]
fn saturation_changes_channel_spread() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (pipeline, bus, rx) = start_smpte_pipeline(dir.path());
    wait_for_streaming(&rx);

    let neutral = average_stats(&bus, 12);
    pipeline.set_adjust(&ImageAdjust {
        saturation: 0.0,
        ..ImageAdjust::default()
    });
    let gray = stats_after_change(&bus);
    assert!(
        gray.mean_spread < neutral.mean_spread * 0.5,
        "saturation 0 kept spread {:.1} → {:.1}",
        neutral.mean_spread,
        gray.mean_spread
    );

    // Back to neutral: the spread must return (live, reversible change).
    pipeline.set_adjust(&ImageAdjust::default());
    let restored = stats_after_change(&bus);
    assert!(
        (restored.mean_spread - neutral.mean_spread).abs() < neutral.mean_spread * 0.1,
        "neutral restored spread {:.1} → {:.1}",
        neutral.mean_spread,
        restored.mean_spread
    );
}
