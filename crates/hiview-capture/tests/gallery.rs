// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Gallery tests (docs/ARCHITECTURE.md §6.8, MILESTONES M5):
//!
//! - `probe_video` must pull the first frame through `openh264dec` —
//!   the only H.264 decoder on the target — and report the duration.
//! - `populate_fixture_set` is the bench helper: it writes 200 real
//!   photos plus a few real MP4s into a temp dir, then the scan +
//!   thumbnail sweep is timed (run with `--release --nocapture` for the
//!   numbers that go into the milestone report).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime};

use gst::prelude::*;
use hiview_capture::probe_video;
use hiview_core::{
    MediaKind, PhotoFormat, PhotoResolution, encode_photo, fit_thumb, photo_thumbnail, scan_media,
    write_thumb_png,
};

static GST_INIT: LazyLock<()> = LazyLock::new(|| {
    hiview_capture::init().unwrap_or_else(|e| panic!("gst init: {e}"));
});

/// First frame of the real microscope recording kept as a fixture
/// (M1): must decode with `openh264dec` and report ~5,3 s.
#[test]
fn probe_extracts_frame_and_duration_from_fixture() {
    LazyLock::force(&GST_INIT);
    let probe = probe_video(Path::new(
        "../../tests/fixtures/calibration_cross_0p1mm.mp4",
    ))
    .unwrap_or_else(|e| panic!("probe: {e}"));
    assert_eq!((probe.width, probe.height), (640, 480));
    assert_eq!(probe.rgba.len(), 640 * 480 * 4);
    let secs = probe
        .duration
        .unwrap_or_else(|| panic!("no duration reported"))
        .as_secs_f64();
    assert!((secs - 5.28).abs() < 0.5, "duration {secs:.2} ≈ 5.28");
}

/// An unreadable/corrupt file must error — the UI turns that into the
/// placeholder tile; it must never panic or hang.
#[test]
fn probe_fails_fast_on_corrupt_video() {
    LazyLock::force(&GST_INIT);
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let path = dir.path().join("quebrado.mp4");
    std::fs::write(&path, b"isto nao e um mp4").unwrap_or_else(|e| panic!("write: {e}"));
    let started = Instant::now();
    assert!(probe_video(&path).is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// Bench helper (task M5): writes `n_photos` real PNGs (640×480 RGBA
/// noise) and `n_videos` real MP4s into `dir`, with descending mtimes so
/// the scan order is deterministic. Returns (photo_dir, video_dir).
fn populate_fixture_set(root: &Path, n_photos: usize, n_videos: usize) -> (PathBuf, PathBuf) {
    LazyLock::force(&GST_INIT);
    let photos = root.join("fotos");
    let videos = root.join("videos");
    std::fs::create_dir_all(&photos).unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::create_dir_all(&videos).unwrap_or_else(|e| panic!("mkdir: {e}"));
    let base = SystemTime::now() - Duration::from_secs(3600);

    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let data: Vec<u8> = (0..640 * 480 * 4)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    for i in 0..n_photos {
        let png = encode_photo(
            &data,
            640,
            480,
            640 * 4,
            PhotoResolution::Native,
            PhotoFormat::Png,
        )
        .unwrap_or_else(|e| panic!("encode: {e}"));
        let path = photos.join(format!("HiView_foto_{i:04}.png"));
        std::fs::write(&path, png).unwrap_or_else(|e| panic!("write: {e}"));
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap_or_else(|e| panic!("open: {e}"))
            .set_modified(base + Duration::from_secs(i as u64))
            .unwrap_or_else(|e| panic!("mtime: {e}"));
    }

    for i in 0..n_videos {
        let path = videos.join(format!("HiView_video_{i:02}.mp4"));
        let pipeline = gst::parse::launch(&format!(
            "videotestsrc num-buffers=30 ! video/x-raw,format=I420,framerate=30/1 \
             ! x264enc speed-preset=superfast tune=zerolatency \
             ! video/x-h264,profile=constrained-baseline ! h264parse ! mp4mux \
             ! filesink location={}",
            path.display()
        ))
        .unwrap_or_else(|e| panic!("video pipeline: {e}"))
        .downcast::<gst::Pipeline>()
        .unwrap_or_else(|_| panic!("not a pipeline"));
        pipeline
            .set_state(gst::State::Playing)
            .unwrap_or_else(|e| panic!("play: {e}"));
        let bus = pipeline.bus().unwrap_or_else(|| panic!("no bus"));
        bus.timed_pop_filtered(
            gst::ClockTime::from_seconds(15),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        pipeline
            .set_state(gst::State::Null)
            .unwrap_or_else(|e| panic!("null: {e}"));
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap_or_else(|e| panic!("open: {e}"))
            .set_modified(base + Duration::from_secs(3600 + i as u64))
            .unwrap_or_else(|e| panic!("mtime: {e}"));
    }
    (photos, videos)
}

/// Process user+system CPU in clock ticks (`/proc/self/stat` fields
/// 14+15) — used to express the sweep's cost as % of one core (§7 CPU
/// budget). Linux-only, which matches the target platform.
fn proc_cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    // comm (field 2) sits inside parens and may contain spaces — parse
    // the fields after its closing paren: index 0 is state (field 3).
    let after = stat.rsplit_once(')').map(|(_, rest)| rest).unwrap_or("");
    let fields: Vec<&str> = after.split_whitespace().collect();
    let utime: u64 = fields
        .get(11)
        .and_then(|v| v.parse().ok())
        .unwrap_or_default();
    let stime: u64 = fields
        .get(12)
        .and_then(|v| v.parse().ok())
        .unwrap_or_default();
    utime + stime
}

/// 200 photos + 3 MP4s: scan, then generate every thumbnail exactly as
/// the worker does (cache miss → generate → cache hit on the second
/// sweep). Prints the timings used in the M5 report — run with
/// `cargo test -p hiview-capture --release --test gallery -- --nocapture`.
#[test]
fn scan_and_thumbnail_sweep_200_files() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let cache = dir.path().join("thumbs");
    let (photos, videos) = populate_fixture_set(dir.path(), 200, 3);

    let scan_start = Instant::now();
    let items = scan_media(&photos, &videos);
    let scan_time = scan_start.elapsed();
    assert_eq!(items.len(), 203);
    // Videos were stamped newest: they sort first.
    assert!(items[..3].iter().all(|i| i.kind == MediaKind::Video));

    let sweep = Instant::now();
    let cpu0 = proc_cpu_ticks();
    let mut first_done = None;
    let mut ready = 0usize;
    for item in &items {
        let (png, _) = hiview_core::cache_files(&cache, &item.path, item.size, item.mtime);
        let (w, h, rgba, dur) = if png.exists() {
            let (w, h, rgba) =
                hiview_core::decode_png(&png).unwrap_or_else(|e| panic!("cache decode: {e}"));
            (w, h, rgba, hiview_core::read_duration_sidecar(&png))
        } else {
            match item.kind {
                MediaKind::Photo => {
                    let thumb = photo_thumbnail(&item.path)
                        .unwrap_or_else(|e| panic!("photo thumb {}: {e}", item.path.display()));
                    let (w, h) = thumb.dimensions();
                    write_thumb_png(
                        &cache,
                        &item.path,
                        item.size,
                        item.mtime,
                        w,
                        h,
                        thumb.as_raw(),
                    )
                    .unwrap_or_else(|e| panic!("cache write: {e}"));
                    (w, h, thumb.into_raw(), None)
                }
                MediaKind::Video => {
                    let probe = probe_video(&item.path)
                        .unwrap_or_else(|e| panic!("video thumb {}: {e}", item.path.display()));
                    let thumb = fit_thumb(
                        image::RgbaImage::from_raw(probe.width, probe.height, probe.rgba)
                            .unwrap_or_else(|| panic!("rgba dims")),
                    );
                    let (w, h) = thumb.dimensions();
                    write_thumb_png(
                        &cache,
                        &item.path,
                        item.size,
                        item.mtime,
                        w,
                        h,
                        thumb.as_raw(),
                    )
                    .unwrap_or_else(|e| panic!("cache write: {e}"));
                    let dur = probe.duration.map(|d| d.as_secs()).unwrap_or(0);
                    hiview_core::write_duration_sidecar(&png, dur)
                        .unwrap_or_else(|e| panic!("dur: {e}"));
                    (w, h, thumb.into_raw(), Some(dur))
                }
            }
        };
        assert!(w > 0 && h > 0 && !rgba.is_empty());
        if item.kind == MediaKind::Video {
            assert_eq!(dur, Some(1), "30-frame clip should read ~1 s");
        }
        ready += 1;
        if first_done.is_none() {
            first_done = Some(sweep.elapsed());
        }
    }
    let cold_sweep = sweep.elapsed();
    // USER_HZ is 100 on Linux: ticks/100 = CPU-seconds burned.
    let cpu_ticks = proc_cpu_ticks() - cpu0;
    let cpu_pct = (cpu_ticks as f64 / 100.0) / cold_sweep.as_secs_f64() * 100.0;

    // Second sweep: every entry must come straight from the cache.
    let warm = Instant::now();
    for item in &items {
        let (png, _) = hiview_core::cache_files(&cache, &item.path, item.size, item.mtime);
        assert!(
            png.exists(),
            "missing cache entry for {}",
            item.path.display()
        );
        hiview_core::decode_png(&png).unwrap_or_else(|e| panic!("warm decode: {e}"));
    }
    let warm_sweep = warm.elapsed();

    // HIVIEW_BENCH_KEEP=1 keeps the fixture set so the real app can be
    // pointed at it for the release-mode page timing (M5 §11.0).
    if std::env::var_os("HIVIEW_BENCH_KEEP").is_some() {
        let root = dir.path().to_path_buf();
        std::mem::forget(dir);
        eprintln!("  kept media at: {}", root.display());
    }

    eprintln!("M5 bench ({} items):", items.len());
    eprintln!("  scan:                 {scan_time:?}");
    eprintln!(
        "  first thumbnail:      {:?}",
        first_done.unwrap_or_default()
    );
    eprintln!("  all {ready} thumbnails (cold): {cold_sweep:?}");
    eprintln!("  cold-sweep CPU (one core):     {cpu_pct:.0}%");
    eprintln!("  all thumbnails (warm cache):   {warm_sweep:?}");
    assert_eq!(ready, 203);
    assert!(
        scan_time < Duration::from_millis(200),
        "scan too slow: {scan_time:?}"
    );
}

/// The corrupt placeholder path end-to-end: a broken file inside the
/// scanned folder still yields one MediaItem, and thumbnail generation
/// fails cleanly (the UI shows the warning tile for it).
#[test]
fn corrupt_file_in_listing_maps_to_error() {
    LazyLock::force(&GST_INIT);
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let bad_photo = dir.path().join("fotos");
    let bad_video = dir.path().join("videos");
    std::fs::create_dir_all(&bad_photo).unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::create_dir_all(&bad_video).unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(bad_photo.join("x.png"), b"lixo").unwrap_or_else(|e| panic!("w: {e}"));
    std::fs::write(bad_video.join("y.mp4"), b"lixo").unwrap_or_else(|e| panic!("w: {e}"));

    let items = scan_media(&bad_photo, &bad_video);
    assert_eq!(items.len(), 2, "corrupt files still appear as tiles");
    assert!(photo_thumbnail(&bad_photo.join("x.png")).is_err());
    assert!(probe_video(&bad_video.join("y.mp4")).is_err());
}
