// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! M10.5 measurement tool (not part of the app): records N seconds on
//! the real microscope with a real `pulsesrc` microphone on the shared
//! clock, then reports the engineering numbers —
//!
//! * video fps and dropped/rejected frame counts,
//! * process CPU time spent while recording (encoder + level + mux),
//! * both track durations of the resulting MP4 and their difference.
//!
//! Usage:
//!   cargo run --release -p hiview-capture --example av_record_measure -- [SECONDS] [--mic DEVICE] [--test]
//!
//! `--test` uses `videotestsrc`/`audiotestsrc` (no hardware);
//! `--mic DEVICE` picks a specific PulseAudio source instead of the
//! system default. Run in a native Wayland session and report the
//! power profile alongside (docs/CONVENTIONS.md §11.0).

use std::process::exit;
use std::time::{Duration, Instant};

use hiview_capture::{
    AudioSource, CapturePipeline, CaptureSource, FrameBus, Recorder, RecorderOptions,
};

/// Process CPU time (user + system jiffies from /proc/self/stat) —
/// the honest cost of the recorder threads, independent of wall clock.
fn cpu_time() -> Duration {
    let stat = std::fs::read_to_string("/proc/self/stat")
        .unwrap_or_else(|e| panic!("/proc/self/stat: {e}"));
    // Fields 14/15 (utime, stime) come after the comm field, which is
    // the only one that may contain spaces — split on the last ')'.
    let after = stat.rsplit_once(')').map(|(_, rest)| rest).unwrap_or("");
    let fields: Vec<&str> = after.split_whitespace().collect();
    // `after` starts at field 3 (state) — utime/stime sit at 11/12.
    let jiffies = |i: usize| {
        fields
            .get(i)
            .and_then(|f| f.parse::<u64>().ok())
            .unwrap_or(0)
    };
    Duration::from_millis((jiffies(11) + jiffies(12)) * 10)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let test_mode = args.iter().any(|a| a == "--test");
    // `--no-audio` records video-only: the A/B delta is the measured
    // cost of the `level + opusenc` audio branch (CONVENTIONS §11).
    let no_audio = args.iter().any(|a| a == "--no-audio");
    let mic = args
        .iter()
        .position(|a| a == "--mic")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let seconds = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-') && a.parse::<u64>().is_ok())
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(60);

    let source = if test_mode {
        CaptureSource::Test
    } else {
        let cameras = hiview_capture::enumerate().unwrap_or_else(|e| panic!("enumerate: {e}"));
        let desc = cameras
            .iter()
            .find(|c| c.is_microscope)
            .unwrap_or_else(|| panic!("no microscope found"));
        println!(
            "device: {} ({})",
            desc.display_name(),
            desc.dev_node.display()
        );
        CaptureSource::Device(desc.clone())
    };
    let audio = if test_mode {
        Some(AudioSource::Test)
    } else {
        Some(AudioSource::Pulse { device: mic })
    }
    .filter(|_| !no_audio);

    let bus = FrameBus::default();
    let pipeline = CapturePipeline::start(source, bus.clone(), |_| {})
        .unwrap_or_else(|e| panic!("pipeline: {e}"));

    // Warm-up: auto-exposure settles, the clock/base_time publish.
    let sub = bus.subscribe();
    for _ in 0..15 {
        let _ = sub.recv_timeout(Duration::from_secs(2));
    }
    drop(sub);

    let clock_ref = pipeline
        .clock_ref()
        .unwrap_or_else(|| panic!("capture pipeline must expose a clock"));
    let dir = std::env::temp_dir().join("hiview-av-measure");
    let cpu0 = cpu_time();
    let recorder = Recorder::start(
        bus.subscribe(),
        640,
        480,
        30,
        1,
        &dir,
        RecorderOptions {
            audio,
            clock_ref: Some(clock_ref),
        },
        |event| println!("event: {event:?}"),
    )
    .unwrap_or_else(|e| panic!("recorder: {e}"));
    println!("has_audio={}", recorder.has_audio());

    let t0 = Instant::now();
    let mut frames = 0u64;
    let meter = bus.subscribe();
    while t0.elapsed() < Duration::from_secs(seconds) {
        match meter.recv_timeout(Duration::from_secs(2)) {
            Ok(_) => frames += 1,
            Err(_) => panic!("frame stream stalled"),
        }
    }
    let wall = t0.elapsed();
    let dropped = recorder.dropped_frames();
    let rejected = recorder.rejected_frames();
    let stop = recorder.stop();
    let cpu = cpu_time() - cpu0;
    drop(pipeline);
    assert!(stop.error.is_none(), "unclean stop: {:?}", stop.error);
    println!(
        "wall={wall:.1?} frames={frames} fps={:.2} dropped={dropped} rejected={rejected}",
        frames as f64 / wall.as_secs_f64()
    );
    println!(
        "cpu={cpu:.1?} of {wall:.1?} wall ({:.1}%)",
        100.0 * cpu.as_secs_f64() / wall.as_secs_f64()
    );
    println!("file={}", stop.path.display());
    // Per-track durations are analyzed externally:
    //   ffprobe -v error -show_entries stream=codec_type,duration -of csv <file>
    exit(0);
}
