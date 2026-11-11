// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Recorder integration tests (docs/ARCHITECTURE.md §6.7, MILESTONES M4).
//!
//! `videotestsrc` feeds the capture pipeline exactly as the microscope
//! would; the recording is then *decoded* with `openh264dec` — the only
//! H.264 decoder on the target system — and probed with `gst-pbutils`.
//! Both tests must run in CI without hardware.

use std::sync::LazyLock;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gst::prelude::*;
use gst_pbutils::prelude::DiscovererStreamInfoExt;
use hiview_capture::{CapturePipeline, CaptureSource, Frame, FrameBus, PixelFormat, Recorder};

const TIMEOUT: Duration = Duration::from_secs(15);

static GST_INIT: LazyLock<()> = LazyLock::new(|| {
    hiview_capture::init().unwrap_or_else(|e| panic!("gst init: {e}"));
});

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hiview-rec-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir {}: {e}", dir.display()));
    dir
}

/// Waits until the pipeline is actually publishing frames (returns once
/// the first frame reached the bus).
fn wait_for_first_frame(bus: &FrameBus) {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if bus.take_latest_for_ui().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("no frame within {TIMEOUT:?}");
}

/// Probes `path` with the discoverer: returns (duration_secs, profile,
/// width, height) of the first video stream.
fn probe_video(path: &std::path::Path) -> (f64, String, u32, u32) {
    let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(15))
        .unwrap_or_else(|e| panic!("discoverer: {e}"));
    let uri = gst::glib::filename_to_uri(path, None)
        .unwrap_or_else(|e| panic!("uri for {}: {e}", path.display()));
    let info = discoverer
        .discover_uri(&uri)
        .unwrap_or_else(|e| panic!("discover {}: {e}", path.display()));

    let duration = info.duration().map(|d| d.seconds_f64()).unwrap_or(f64::NAN);
    let video = info
        .video_streams()
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no video stream in {}", path.display()));
    let caps = video
        .caps()
        .unwrap_or_else(|| panic!("video stream without caps"));
    let structure = caps.structure(0).unwrap_or_else(|| panic!("caps empty"));
    assert_eq!(structure.name(), "video/x-h264");
    let profile = structure
        .get::<&str>("profile")
        .unwrap_or_default()
        .to_string();
    (duration, profile, video.width(), video.height())
}

/// Decodes the whole file with `openh264dec` — *the* decoder of the
/// target system — and returns the number of frames that came out.
/// Decoding, not probing, is what proves the recording is playable there.
fn decode_frame_count(path: &std::path::Path) -> u64 {
    let pipeline = gst::parse::launch(&format!(
        "filesrc location={} ! qtdemux ! h264parse ! openh264dec \
         ! videoconvert ! appsink name=sink",
        path.display()
    ))
    .unwrap_or_else(|e| panic!("decode pipeline: {e}"))
    .downcast::<gst::Pipeline>()
    .unwrap_or_else(|_| panic!("decoder is not a pipeline"));
    let sink = pipeline
        .by_name("sink")
        .and_then(|e| e.downcast::<gst_app::AppSink>().ok())
        .unwrap_or_else(|| panic!("no appsink in decode pipeline"));
    pipeline
        .set_state(gst::State::Playing)
        .unwrap_or_else(|e| panic!("decoder start: {e}"));

    let mut frames = 0u64;
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        match sink.pull_sample() {
            Ok(_sample) => frames += 1,
            Err(_) => break, // EOS
        }
    }
    pipeline
        .set_state(gst::State::Null)
        .unwrap_or_else(|e| panic!("decoder stop: {e}"));
    frames
}

/// Acceptance: 3 s of `videotestsrc` produce an MP4 the target system can
/// actually play — constrained-baseline H.264, 640×480, ~3.0 s long.
#[test]
fn records_three_seconds_of_videotestsrc() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let bus = FrameBus::default();
    let (tx, _rx) = mpsc::channel();
    let pipeline = CapturePipeline::start(CaptureSource::Test, bus.clone(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap_or_else(|e| panic!("capture failed to start: {e}"));
    wait_for_first_frame(&bus);

    let recorder = Recorder::start(bus.subscribe(), 640, 480, 30, 1, &dir, |err| {
        panic!("record pipeline error: {err}")
    })
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));

    std::thread::sleep(Duration::from_secs(3));
    let stop = recorder.stop();
    assert!(stop.error.is_none(), "unclean stop: {:?}", stop.error);
    drop(pipeline);

    let (duration, profile, width, height) = probe_video(&stop.path);
    assert!(
        (duration - 3.0).abs() <= 0.2,
        "duration {duration:.2}s outside 3.0 ± 0.2"
    );
    assert_eq!(profile, "constrained-baseline");
    assert_eq!((width, height), (640, 480));

    let decoded = decode_frame_count(&stop.path);
    assert!(
        decoded >= 60,
        "only {decoded} frames decoded from a 3 s recording"
    );
}

/// Bitrate guard (M4 review): a 10 s recording must stay within
/// 2.0-3.0 Mbit/s — the `reserved-moov` header is a fixed-size block
/// that must not dominate the file. High-entropy frames are required:
/// a compressible test pattern lets the encoder undershoot the target.
#[test]
fn recording_stays_near_target_bitrate() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let bus = FrameBus::default();
    let recorder = Recorder::start(bus.subscribe(), 640, 480, 30, 1, &dir, |err| {
        panic!("record pipeline error: {err}")
    })
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));

    // Worst-case content: per-frame random noise forces the encoder to
    // spend real bitrate (a 640x480 YUYV frame needs ~600 kB/frame).
    let noise = |seed: &mut u64, n: usize| -> Vec<u8> {
        (0..n)
            .map(|_| {
                *seed ^= *seed << 13;
                *seed ^= *seed >> 7;
                *seed ^= *seed << 17;
                *seed as u8
            })
            .collect()
    };
    let mut seed = 0x1234_5678_9abc_def0_u64;
    for n in 0..300u64 {
        bus.publish(Frame {
            width: 640,
            height: 480,
            stride: 640 * 4,
            format: PixelFormat::Rgba,
            data: gst::glib::Bytes::from_owned(noise(&mut seed, 640 * 480 * 4)),
            pts: Some(gst::ClockTime::from_mseconds(n * 33)),
        });
        std::thread::sleep(Duration::from_millis(33));
    }
    let stop = recorder.stop();
    assert!(stop.error.is_none(), "unclean stop: {:?}", stop.error);

    let bytes = std::fs::metadata(&stop.path)
        .unwrap_or_else(|e| panic!("stat {}: {e}", stop.path.display()))
        .len();
    let (duration, _, _, _) = probe_video(&stop.path);
    let mbit_s = (bytes as f64 * 8.0) / duration / 1e6;
    assert!(
        (2.0..=3.0).contains(&mbit_s),
        "bitrate {mbit_s:.2} Mbit/s outside 2.0-3.0 ({bytes} B / {duration:.2} s)"
    );
}

/// The record pipeline is independent of the capture side: when the
/// source dies mid-recording (unplug / capture error), the same EOS
/// sequence still finalizes a playable file (M4 requirement).
#[test]
fn source_death_mid_recording_keeps_playable_file() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let bus = FrameBus::default();
    let (tx, _rx) = mpsc::channel();
    let pipeline = CapturePipeline::start(CaptureSource::Test, bus.clone(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap_or_else(|e| panic!("capture failed to start: {e}"));
    wait_for_first_frame(&bus);

    let recorder = Recorder::start(bus.subscribe(), 640, 480, 30, 1, &dir, |err| {
        panic!("record pipeline error: {err}")
    })
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));

    std::thread::sleep(Duration::from_millis(1500));
    drop(pipeline); // capture side dies — the camera "unplugged"
    std::thread::sleep(Duration::from_millis(300));

    let stop = recorder.stop();
    assert!(stop.error.is_none(), "unclean stop: {:?}", stop.error);

    let (duration, profile, width, height) = probe_video(&stop.path);
    assert!(
        (1.0..=2.5).contains(&duration),
        "duration {duration:.2}s unreasonable after unplug"
    );
    assert_eq!(profile, "constrained-baseline");
    assert_eq!((width, height), (640, 480));
    assert!(decode_frame_count(&stop.path) >= 15, "file does not decode");
}

/// Abrupt teardown without EOS — the equivalent of SIGKILL or a crash:
/// `Drop` sets the pipeline to `Null` without ever sending
/// `end_of_stream`. Thanks to `mp4mux fragment-duration=1000` the file
/// must still be discoverable and decodable up to the last fragment.
#[test]
fn abrupt_teardown_keeps_killed_file_playable() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let bus = FrameBus::default();
    let (tx, _rx) = mpsc::channel();
    let pipeline = CapturePipeline::start(CaptureSource::Test, bus.clone(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap_or_else(|e| panic!("capture failed to start: {e}"));
    wait_for_first_frame(&bus);

    let recorder = Recorder::start(bus.subscribe(), 640, 480, 30, 1, &dir, |err| {
        panic!("record pipeline error: {err}")
    })
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));

    std::thread::sleep(Duration::from_millis(2500));
    let path = recorder.path().to_path_buf();
    drop(recorder); // no EOS: same on-disk state as a killed process
    std::thread::sleep(Duration::from_millis(400));
    drop(pipeline);

    // The tail `moov` lands when the muxer shuts down; poll briefly
    // instead of racing it.
    let mut probe = None;
    for _ in 0..40 {
        match std::panic::catch_unwind(|| probe_video(&path)) {
            Ok(p) => {
                probe = Some(p);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let (duration, profile, width, height) =
        probe.unwrap_or_else(|| panic!("file never became discoverable"));
    assert_eq!(profile, "constrained-baseline");
    assert_eq!((width, height), (640, 480));
    // The reserved-moov index is rewritten every second, so a killed
    // file must report close to the real duration (>= fed - 1 s), not
    // just be discoverable.
    assert!(
        duration >= 1.5,
        "killed file reports only {duration:.2}s of a 2.5 s feed"
    );
    let decoded = decode_frame_count(&path);
    assert!(
        decoded >= 15,
        "only {decoded} frames decodable from a killed recording"
    );
}
