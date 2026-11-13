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
use hiview_capture::{
    AudioSource, CapturePipeline, CaptureSource, ClockRef, Frame, FrameBus, PixelFormat,
    RecordEvent, Recorder, RecorderOptions,
};

const TIMEOUT: Duration = Duration::from_secs(15);

static GST_INIT: LazyLock<()> = LazyLock::new(|| {
    hiview_capture::init().unwrap_or_else(|e| panic!("gst init: {e}"));
});

/// Recorder event sink for tests: any fatal error fails the test.
fn panic_on_event(event: RecordEvent) {
    if let RecordEvent::Error(err) = event {
        panic!("record pipeline error: {err}");
    }
}

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

    let recorder = Recorder::start(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions::default(),
        panic_on_event,
    )
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

    // M10.5 regression: with audio off the file stays exactly like M4 —
    // one video track, no audio stream.
    let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(15))
        .unwrap_or_else(|e| panic!("discoverer: {e}"));
    let uri = gst::glib::filename_to_uri(&stop.path, None).unwrap_or_else(|e| panic!("uri: {e}"));
    let info = discoverer
        .discover_uri(&uri)
        .unwrap_or_else(|e| panic!("discover: {e}"));
    assert!(
        info.audio_streams().is_empty(),
        "video-only recording must not carry an audio track"
    );

    let decoded = decode_frame_count(&stop.path);
    assert!(
        decoded >= 60,
        "only {decoded} frames decoded from a 3 s recording"
    );
}

/// Publishes `count` RGBA frames of random noise at ~30 fps. Noise is
/// worst-case content: a compressible pattern lets the encoder
/// undershoot the target bitrate.
fn publish_noise_frames(bus: &FrameBus, count: u64) {
    let mut seed = 0x1234_5678_9abc_def0_u64;
    for n in 0..count {
        let data: Vec<u8> = (0..640 * 480 * 4)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as u8
            })
            .collect();
        bus.publish(Frame {
            width: 640,
            height: 480,
            stride: 640 * 4,
            format: PixelFormat::Rgba,
            data: gst::glib::Bytes::from_owned(data),
            pts: Some(gst::ClockTime::from_mseconds(n * 33)),
        });
        std::thread::sleep(Duration::from_millis(33));
    }
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
    let recorder = Recorder::start(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions::default(),
        panic_on_event,
    )
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));

    publish_noise_frames(&bus, 300);
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

/// Revisão 4 (a): the reserved-moov index is NOT a recording-length
/// limit. With a test-only 10 s reserve, a ~20 s recording stopped
/// normally must still produce a complete, fully decodable file.
#[test]
fn recording_beyond_reserved_duration_finalizes_fully() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let bus = FrameBus::default();
    let recorder = Recorder::start_with_reserve(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions::default(),
        panic_on_event,
        gst::ClockTime::from_seconds(10),
    )
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));

    publish_noise_frames(&bus, 600); // ~20 s, twice the reserve
    let stop = recorder.stop();
    assert!(stop.error.is_none(), "unclean stop: {:?}", stop.error);

    let (duration, profile, width, height) = probe_video(&stop.path);
    assert!(
        (19.0..=20.5).contains(&duration),
        "duration {duration:.2}s of ~20 s feed unreasonable"
    );
    assert_eq!(profile, "constrained-baseline");
    assert_eq!((width, height), (640, 480));
    let decoded = decode_frame_count(&stop.path);
    assert!(
        decoded >= 300,
        "only {decoded} frames decoded from a ~20 s recording"
    );
}

/// Revisão 4 (b): same setup, but the recorder is dropped without EOS
/// past the reserve — the kill/crash case. The periodic `moov` rewrite
/// keeps tracking the samples, so the file must still report close to
/// the real duration (measured: ~1 s of index lag), not just the
/// reserved range.
#[test]
fn abrupt_teardown_beyond_reserve_keeps_real_duration() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let bus = FrameBus::default();
    let recorder = Recorder::start_with_reserve(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions::default(),
        panic_on_event,
        gst::ClockTime::from_seconds(10),
    )
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));

    publish_noise_frames(&bus, 600); // ~20 s, twice the reserve
    let path = recorder.path().to_path_buf();
    drop(recorder); // no EOS: same on-disk state as a killed process

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
    let (duration, profile, _, _) =
        probe.unwrap_or_else(|| panic!("file never became discoverable"));
    assert_eq!(profile, "constrained-baseline");
    assert!(
        duration >= 17.0,
        "killed file reports only {duration:.2}s of a ~20 s feed past the reserve"
    );
    assert!(
        decode_frame_count(&path) >= 200,
        "killed file does not decode past the reserve"
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

    let recorder = Recorder::start(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions::default(),
        panic_on_event,
    )
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

    let recorder = Recorder::start(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions::default(),
        panic_on_event,
    )
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

// ---------------------------------------------------------------------
// M10.5 — audio track
// ---------------------------------------------------------------------

/// Per-track stats collected by demux pad probes while decoding a file.
#[derive(Default)]
struct TrackStats {
    buffers: u64,
    first_pts: Option<gst::ClockTime>,
    last_end: Option<gst::ClockTime>,
}

/// Starts a real capture pipeline (`videotestsrc`) and a recorder with
/// `audiotestsrc` on the shared clock — the exact production wiring.
fn start_av_recording(dir: &std::path::Path) -> (CapturePipeline, Recorder, FrameBus) {
    let bus = FrameBus::default();
    let (tx, _rx) = mpsc::channel();
    let pipeline = CapturePipeline::start(CaptureSource::Test, bus.clone(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap_or_else(|e| panic!("capture failed to start: {e}"));
    wait_for_first_frame(&bus);
    // Let the capture pipeline run a moment: in production the
    // recorder always starts *after* capture, which is what makes the
    // base-time offset non-trivial (M10.5 sync path).
    std::thread::sleep(Duration::from_millis(500));
    let clock_ref: ClockRef = pipeline
        .clock_ref()
        .unwrap_or_else(|| panic!("capture pipeline must expose a clock"));
    let recorder = Recorder::start(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        dir,
        RecorderOptions {
            audio: Some(AudioSource::Test),
            clock_ref: Some(clock_ref),
        },
        panic_on_event,
    )
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));
    (pipeline, recorder, bus)
}

/// Decodes `path` with the system's real decoders — `openh264dec` for
/// video, `opusdec` for audio — and returns per-track stats gathered
/// at the demux pads (packet count, first PTS, last PTS+duration).
fn decode_track_stats(path: &std::path::Path) -> (TrackStats, TrackStats) {
    let video_stats = std::sync::Arc::new(std::sync::Mutex::new(TrackStats::default()));
    let audio_stats = std::sync::Arc::new(std::sync::Mutex::new(TrackStats::default()));

    let pipeline = gst::Pipeline::builder().build();
    let src = gst::ElementFactory::make("filesrc")
        .property("location", path.to_string_lossy().as_ref())
        .build()
        .unwrap_or_else(|e| panic!("filesrc: {e}"));
    let demux = gst::ElementFactory::make("qtdemux")
        .build()
        .unwrap_or_else(|e| panic!("qtdemux: {e}"));
    let video_chain = gst::parse::bin_from_description(
        "queue ! h264parse ! openh264dec ! fakesink sync=false",
        true,
    )
    .unwrap_or_else(|e| panic!("video chain: {e}"))
    .upcast::<gst::Element>();
    let audio_chain =
        gst::parse::bin_from_description("queue ! opusdec ! fakesink sync=false", true)
            .unwrap_or_else(|e| panic!("audio chain: {e}"))
            .upcast::<gst::Element>();
    pipeline
        .add_many([&src, &demux])
        .unwrap_or_else(|e| panic!("add decode elements: {e}"));
    src.link(&demux)
        .unwrap_or_else(|e| panic!("link demux: {e}"));

    let v_stats = std::sync::Arc::clone(&video_stats);
    let a_stats = std::sync::Arc::clone(&audio_stats);
    let pipeline_weak = pipeline.downgrade();
    demux.connect_pad_added(move |_demux, pad| {
        let caps = pad
            .current_caps()
            .unwrap_or_else(|| panic!("pad without caps"));
        let name = caps
            .structure(0)
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        let (chain, stats) = if name.starts_with("video/") {
            (video_chain.clone(), std::sync::Arc::clone(&v_stats))
        } else if name.starts_with("audio/") {
            (audio_chain.clone(), std::sync::Arc::clone(&a_stats))
        } else {
            return;
        };
        let Some(sink_pad) = chain.static_pad("sink") else {
            panic!("decode chain has no sink pad");
        };
        if sink_pad.is_linked() {
            return;
        }
        // The chain joins the pipeline only when its track exists —
        // an unlinked fakesink would never see EOS and would block
        // the pipeline EOS aggregation forever.
        let pipeline = pipeline_weak
            .upgrade()
            .unwrap_or_else(|| panic!("decode gone"));
        pipeline
            .add(&chain)
            .unwrap_or_else(|e| panic!("add chain: {e}"));
        chain
            .sync_state_with_parent()
            .unwrap_or_else(|e| panic!("chain state: {e}"));
        // Count packets and track the timestamp range *at the demux
        // pad* — MP4 timestamps, before any decoding shifts them.
        let cell = stats;
        pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
            if let Some(buffer) = info.buffer() {
                let mut s = cell.lock().unwrap_or_else(|e| e.into_inner());
                s.buffers += 1;
                if s.first_pts.is_none() {
                    s.first_pts = buffer.pts();
                }
                if let Some(pts) = buffer.pts() {
                    s.last_end = Some(pts + buffer.duration().unwrap_or(gst::ClockTime::ZERO));
                }
            }
            gst::PadProbeReturn::Ok
        });
        pad.link(&sink_pad)
            .unwrap_or_else(|e| panic!("link {name}: {e}"));
    });

    pipeline
        .set_state(gst::State::Playing)
        .unwrap_or_else(|e| panic!("decode start: {e}"));
    let bus = pipeline.bus().unwrap_or_else(|| panic!("no bus"));
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if Instant::now() > deadline {
            panic!("decode pipeline never reached EOS");
        }
        match bus.timed_pop_filtered(
            Some(gst::ClockTime::from_mseconds(500)),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        ) {
            Some(msg) => match msg.view() {
                gst::MessageView::Eos(_) => break,
                gst::MessageView::Error(err) => {
                    panic!("decode error: {}", err.error());
                }
                _ => {}
            },
            None => continue,
        }
    }
    pipeline
        .set_state(gst::State::Null)
        .unwrap_or_else(|e| panic!("decode stop: {e}"));

    let video = std::mem::take(&mut *video_stats.lock().unwrap_or_else(|e| e.into_inner()));
    let audio = std::mem::take(&mut *audio_stats.lock().unwrap_or_else(|e| e.into_inner()));
    (video, audio)
}

/// M10.5 acceptance: 3 s with audio produces an MP4 with an H.264
/// constrained-baseline track **and** an Opus track, both ~3.0 s long,
/// both decodable with the system's decoders, and both starting on the
/// same timeline (offset ≤ 40 ms — the engineering A/V sync bound).
#[test]
fn records_video_and_audio_tracks_in_sync() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let (pipeline, recorder, _bus) = start_av_recording(&dir);
    assert!(recorder.has_audio(), "audio track should be present");

    std::thread::sleep(Duration::from_secs(3));
    let stop = recorder.stop();
    assert!(stop.error.is_none(), "unclean stop: {:?}", stop.error);
    drop(pipeline);

    let (duration, profile, _, _) = probe_video(&stop.path);
    assert!(
        (duration - 3.0).abs() <= 0.2,
        "container duration {duration:.2}s outside 3.0 ± 0.2"
    );
    assert_eq!(profile, "constrained-baseline");

    // The audio stream must exist and be Opus.
    let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(15))
        .unwrap_or_else(|e| panic!("discoverer: {e}"));
    let uri = gst::glib::filename_to_uri(&stop.path, None).unwrap_or_else(|e| panic!("uri: {e}"));
    let info = discoverer
        .discover_uri(&uri)
        .unwrap_or_else(|e| panic!("discover: {e}"));
    let audio = info
        .audio_streams()
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no audio stream in {}", stop.path.display()));
    let caps_name = audio
        .caps()
        .and_then(|c| c.structure(0).map(|s| s.name().to_string()))
        .unwrap_or_default();
    assert_eq!(caps_name, "audio/x-opus", "audio codec must be Opus");

    let (video, audio) = decode_track_stats(&stop.path);
    assert!(video.buffers >= 60, "only {} video packets", video.buffers);
    assert!(audio.buffers >= 100, "only {} audio packets", audio.buffers);
    for (label, end) in [("video", video.last_end), ("audio", audio.last_end)] {
        let end = end.unwrap_or_else(|| panic!("{label} track empty"));
        assert!(
            (end.seconds_f64() - 3.0).abs() <= 0.2,
            "{label} track ends at {:.2}s, expected 3.0 ± 0.2",
            end.seconds_f64()
        );
    }
    let v0 = video.first_pts.unwrap_or_else(|| panic!("no video pts"));
    let a0 = audio.first_pts.unwrap_or_else(|| panic!("no audio pts"));
    let delta = v0.nseconds().abs_diff(a0.nseconds());
    assert!(
        delta <= 40_000_000,
        "A/V start offset {delta} ns exceeds the 40 ms bound"
    );
}

/// M10.5 fallback: when the microphone cannot be opened at recording
/// start (injected failing source), the recording proceeds video-only
/// and `has_audio()` reports the outcome for the UI toast.
#[test]
fn mic_failure_at_start_records_video_only() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let bus = FrameBus::default();
    let recorder = Recorder::start(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions {
            audio: Some(AudioSource::Failing),
            ..RecorderOptions::default()
        },
        panic_on_event,
    )
    .unwrap_or_else(|e| panic!("recorder must still start video-only: {e}"));
    assert!(!recorder.has_audio(), "failing source must not claim audio");
    assert!(recorder.audio_level().is_none());

    publish_noise_frames(&bus, 90); // ~3 s
    let stop = recorder.stop();
    assert!(stop.error.is_none(), "unclean stop: {:?}", stop.error);

    // Regression shape: exactly the M4 single-track file.
    let (duration, profile, _, _) = probe_video(&stop.path);
    assert!((duration - 3.0).abs() <= 0.4, "duration {duration:.2}s");
    assert_eq!(profile, "constrained-baseline");
    let (video, audio) = decode_track_stats(&stop.path);
    assert!(video.buffers >= 60, "only {} video packets", video.buffers);
    assert_eq!(audio.buffers, 0, "video-only file must have no audio");
}

/// M10.5 crash resilience: dropping the recorder without EOS (kill -9
/// equivalent) must still leave a playable file with BOTH tracks —
/// the reserved `moov` index covers audio the same way it covers video.
#[test]
fn abrupt_teardown_with_audio_keeps_playable() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let (pipeline, recorder, _bus) = start_av_recording(&dir);

    std::thread::sleep(Duration::from_millis(2500));
    let path = recorder.path().to_path_buf();
    drop(recorder); // no EOS: same on-disk state as a killed process
    std::thread::sleep(Duration::from_millis(400));
    drop(pipeline);

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
    let (duration, _, _, _) = probe.unwrap_or_else(|| panic!("file never became discoverable"));
    assert!(
        duration >= 1.5,
        "killed file reports only {duration:.2}s of a 2.5 s feed"
    );

    let (video, audio) = decode_track_stats(&path);
    assert!(
        video.buffers >= 15,
        "only {} video packets in killed file",
        video.buffers
    );
    assert!(
        audio.buffers >= 15,
        "only {} audio packets in killed file — the reserved index must cover audio too",
        audio.buffers
    );
}

/// M10.5 mid-recording microphone loss (engineering decision — the
/// minimum accepted path): when the audio branch posts an error, the
/// bus thread injects EOS into the audio source so the muxer closes
/// that track, and `AudioLost` reaches the app so it can finalize the
/// recording with a toast — the file must stay valid on both tracks.
#[test]
fn audio_loss_mid_recording_finalizes_cleanly() {
    LazyLock::force(&GST_INIT);
    let dir = temp_dir();
    let bus = FrameBus::default();
    let (tx, _rx) = mpsc::channel();
    let pipeline = CapturePipeline::start(CaptureSource::Test, bus.clone(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap_or_else(|e| panic!("capture failed to start: {e}"));
    wait_for_first_frame(&bus);
    std::thread::sleep(Duration::from_millis(500));
    let clock_ref: ClockRef = pipeline
        .clock_ref()
        .unwrap_or_else(|| panic!("capture pipeline must expose a clock"));
    let (event_tx, event_rx) = mpsc::channel();
    let recorder = Recorder::start(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions {
            audio: Some(AudioSource::Test),
            clock_ref: Some(clock_ref),
        },
        move |event| {
            let _ = event_tx.send(event);
        },
    )
    .unwrap_or_else(|e| panic!("recorder failed to start: {e}"));

    // Drive the real path: post an error message as if the audio
    // source element itself failed — exactly what pulsesrc does when
    // the device dies.
    std::thread::sleep(Duration::from_secs(1));
    let asrc = recorder
        .audio_src_element()
        .unwrap_or_else(|| panic!("audio track must be present"));
    let msg = gst::message::Error::builder(gst::ResourceError::Failed, "simulated mic loss")
        .src(&asrc)
        .build();
    asrc.post_message(msg)
        .unwrap_or_else(|_| panic!("posting the audio error failed"));

    match event_rx.recv_timeout(TIMEOUT) {
        Ok(RecordEvent::AudioLost) => {}
        Ok(other) => panic!("expected AudioLost, got {other:?}"),
        Err(e) => panic!("no AudioLost event within {TIMEOUT:?}: {e}"),
    }
    std::thread::sleep(Duration::from_secs(1));
    let stop = recorder.stop();
    assert!(stop.error.is_none(), "unclean stop: {:?}", stop.error);
    drop(pipeline);

    // The file must be valid and keep BOTH tracks: the audio track
    // ends early (~1 s in) while the video runs to the stop (~2 s).
    let (video, audio) = decode_track_stats(&stop.path);
    assert!(video.buffers >= 30, "only {} video packets", video.buffers);
    assert!(audio.buffers >= 10, "only {} audio packets", audio.buffers);
    let v_end = video
        .last_end
        .unwrap_or_else(|| panic!("video track empty"));
    let a_end = audio
        .last_end
        .unwrap_or_else(|| panic!("audio track empty"));
    assert!(
        v_end.seconds_f64() >= 1.5,
        "video ended early at {:.2}s",
        v_end.seconds_f64()
    );
    assert!(
        a_end.seconds_f64() < v_end.seconds_f64(),
        "audio should have been cut by the loss: {:.2}s vs video {:.2}s",
        a_end.seconds_f64(),
        v_end.seconds_f64()
    );
}
