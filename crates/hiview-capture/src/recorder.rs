// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Video recorder: a separate pipeline fed by `appsrc` from a bounded
//! FrameBus subscription (docs/ARCHITECTURE.md §6.7, ADR-006).
//!
//! The mandatory stop sequence — `appsrc end-of-stream` → wait `EOS` on
//! the bus (5 s timeout) → `Null` — is what writes the `moov` atom that
//! makes the MP4 playable. It runs the same way on user stop, window
//! close and camera unplug (the record pipeline is independent of the
//! capture side, so a dead camera still lets the file finalize).

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use chrono::Local;
use gst::prelude::*;
use tracing::{debug, error, info, warn};

use hiview_core::{unique_photo_path, video_filename};

use crate::error::Error;
use crate::frame::{Frame, FrameSubscription, PixelFormat};
use crate::pipeline::{caps_filter, make};

/// Minimum free space on the target filesystem to start a recording
/// (docs/ARCHITECTURE.md §6.7).
pub const MIN_FREE_BYTES: u64 = 500 * 1024 * 1024;

/// Maximum wait for `EOS` on the record bus after `end_of_stream`
/// (docs/ARCHITECTURE.md §6.7: 5 s).
const EOS_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the feeder thread re-checks its stop flag when idle.
const FEED_IDLE: Duration = Duration::from_millis(200);

/// Target H.264 bitrate: 2.5 Mbps (docs/ARCHITECTURE.md §6.7). Roughly
/// 19 MB per recorded minute at 640×480.
const BITRATE_KBPS: u32 = 2500;

/// H.264 encoder usable on this system (§6.7: x264enc preferred,
/// openh264enc as runtime fallback).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoEncoder {
    /// `x264enc` (gstreamer1.0-plugins-ugly), configured for
    /// constrained-baseline via the capsfilter after it.
    X264,
    /// `openh264enc` (gstreamer1.0-plugins-bad); already emits baseline.
    OpenH264,
}

/// Picks the best H.264 encoder installed, or `None` when recording is
/// unsupported (the UI disables the record button then).
#[must_use]
pub fn available_encoder() -> Option<VideoEncoder> {
    if crate::init().is_err() {
        return None;
    }
    if gst::ElementFactory::find("x264enc").is_some() {
        Some(VideoEncoder::X264)
    } else if gst::ElementFactory::find("openh264enc").is_some() {
        Some(VideoEncoder::OpenH264)
    } else {
        None
    }
}

/// Why a recording was refused before starting.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RecordDenied {
    /// Less than [`MIN_FREE_BYTES`] free on the target filesystem.
    #[error("only {free_mb} MB free on disk (500 MB required)")]
    LowDiskSpace {
        /// Free space detected, in MiB.
        free_mb: u64,
    },
    /// The free-space probe could not answer.
    #[error("could not determine free disk space")]
    FreeSpaceUnknown,
}

/// Pre-flight disk check (§6.7). The probe is injectable so the gate can
/// be unit-tested without touching the real filesystem.
///
/// # Errors
/// [`RecordDenied::LowDiskSpace`] below 500 MB,
/// [`RecordDenied::FreeSpaceUnknown`] when the probe fails.
pub fn check_disk_space(
    dir: &Path,
    free_space: impl Fn(&Path) -> Option<u64>,
) -> Result<(), RecordDenied> {
    match free_space(dir) {
        Some(bytes) if bytes < MIN_FREE_BYTES => Err(RecordDenied::LowDiskSpace {
            free_mb: bytes / (1024 * 1024),
        }),
        Some(_) => Ok(()),
        None => Err(RecordDenied::FreeSpaceUnknown),
    }
}

/// Outcome reported by [`Recorder::stop`].
#[derive(Debug)]
pub struct RecordStop {
    /// Final path of the recording (with collision suffix if needed).
    pub path: PathBuf,
    /// `Some` when the file could not be finalized cleanly (bus error or
    /// EOS timeout); the MP4 may be missing its `moov` atom and callers
    /// should delete it.
    pub error: Option<String>,
    /// Frames dropped because the bounded subscription queue was full.
    pub dropped_frames: u64,
    /// Frames rejected for mismatched geometry while recording.
    pub rejected_frames: u64,
}

/// Terminal event observed on the record pipeline bus.
enum BusOutcome {
    /// Clean end-of-stream: the muxer wrote the trailing `moov`.
    Eos,
    /// A fatal pipeline error (e.g. disk full in filesink).
    Failed(String),
}

/// A running recording. `Drop` is only a safety net — owners must call
/// [`Recorder::stop`], which runs the mandatory EOS sequence.
pub struct Recorder {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    feeding: Arc<AtomicBool>,
    bus_thread: Option<JoinHandle<()>>,
    feeder_thread: Option<JoinHandle<()>>,
    done_rx: mpsc::Receiver<BusOutcome>,
    path: PathBuf,
    dropped: Arc<AtomicU64>,
    rejected: Arc<AtomicU64>,
}

impl fmt::Debug for Recorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Recorder")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Recorder {
    /// Starts a recording into `dir` (created when missing), consuming
    /// frames from `subscription`. `width`/`height` are the expected frame
    /// geometry — mismatched frames are dropped and counted, because a
    /// video cannot change resolution mid-file (§6.4). `fps_num`/`fps_den`
    /// is the nominal rate for caps and buffer durations; real timing
    /// comes from the capture PTS.
    ///
    /// `on_error` is invoked from the bus thread on a fatal muxer/sink
    /// failure mid-recording; [`Recorder::stop`] still has to run.
    ///
    /// # Errors
    /// Missing GStreamer elements, link/state failures, or `dir` not
    /// writable.
    pub fn start(
        subscription: FrameSubscription,
        width: u32,
        height: u32,
        fps_num: i32,
        fps_den: i32,
        dir: &Path,
        on_error: impl Fn(String) + Send + 'static,
    ) -> Result<Self, Error> {
        crate::init()?;
        let encoder = available_encoder()
            .ok_or_else(|| Error::VideoDevice("no H.264 encoder available".into()))?;
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::VideoDevice(format!("cannot create {}: {e}", dir.display())))?;
        let path = unique_photo_path(dir, &video_filename(Local::now()));

        let (pipeline, appsrc) =
            build_record_pipeline(encoder, width, height, fps_num, fps_den, &path)?;

        let feeding = Arc::new(AtomicBool::new(true));
        let dropped = subscription.dropped_counter();
        let rejected = Arc::new(AtomicU64::new(0));
        let feeder_thread = spawn_feeder(
            appsrc.clone(),
            subscription,
            width,
            height,
            Arc::clone(&feeding),
            Arc::clone(&rejected),
            frame_duration(fps_num, fps_den),
        )?;
        let (done_tx, done_rx) = mpsc::channel();
        let bus_thread = spawn_bus_thread(&pipeline, done_tx, on_error)?;

        pipeline.set_state(gst::State::Playing)?;
        info!(
            "recording to {} ({encoder:?}, {width}x{height})",
            path.display()
        );

        Ok(Self {
            pipeline,
            appsrc,
            feeding,
            bus_thread: Some(bus_thread),
            feeder_thread: Some(feeder_thread),
            done_rx,
            path,
            dropped,
            rejected,
        })
    }

    /// The file this recording writes to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Frames dropped because the bounded subscription queue was full
    /// (docs/ARCHITECTURE.md §6.3 — visible only in debug output).
    #[must_use]
    pub fn dropped_frames(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Frames dropped because their geometry did not match the recording
    /// (defensive; rotation is already disabled in the UI while recording).
    #[must_use]
    pub fn rejected_frames(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }

    /// Runs the mandatory stop sequence (§6.7): `end_of_stream`, wait for
    /// `EOS` on the bus up to 5 s, then `Null`. Blocks for at most
    /// `EOS_TIMEOUT` + drain; call it from a worker thread, never the UI
    /// thread (docs/CONVENTIONS.md §7).
    pub fn stop(mut self) -> RecordStop {
        self.feeding.store(false, Ordering::Relaxed);
        if let Err(err) = self.appsrc.end_of_stream() {
            // Flushing/not-linked while the pipeline is already going down
            // is harmless — the bus wait below still bounds the stop.
            debug!("appsrc end_of_stream returned {err}");
        }
        let error = match self.done_rx.recv_timeout(EOS_TIMEOUT) {
            Ok(BusOutcome::Eos) => None,
            Ok(BusOutcome::Failed(detail)) => Some(format!("record pipeline: {detail}")),
            Err(_) => {
                warn!("no EOS from record pipeline within {EOS_TIMEOUT:?}");
                Some("timed out waiting for recorder EOS".into())
            }
        };
        self.teardown();
        if error.is_none() {
            info!("recording finalized: {}", self.path.display());
        }
        RecordStop {
            path: self.path.clone(),
            error,
            dropped_frames: self.dropped.load(Ordering::Relaxed),
            rejected_frames: self.rejected.load(Ordering::Relaxed),
        }
    }

    /// Wakes the bus thread, stops the pipeline and joins both threads.
    /// Shared tail of `stop` (clean or not) and `Drop`.
    fn teardown(&mut self) {
        self.feeding.store(false, Ordering::Relaxed);
        if let Some(bus) = self.pipeline.bus() {
            let quit = gst::message::Application::new(gst::Structure::new_empty("hiview-quit"));
            if let Err(err) = bus.post(quit) {
                warn!("failed to wake recorder bus thread: {err}");
            }
        }
        if let Err(err) = self.pipeline.set_state(gst::State::Null) {
            warn!("failed to stop record pipeline: {err}");
        }
        if let Some(thread) = self.bus_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.feeder_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if self.bus_thread.is_some() {
            warn!("recorder dropped without stop(); file may be unplayable");
            self.teardown();
        }
    }
}

/// Rebases a capture PTS to zero at record start (§6.7: no `do-timestamp`
/// on the appsrc). Frames without PTS fall back to `seq * frame_dur`;
/// frames stamped before the base clamp to zero instead of going negative.
fn rebase_pts(
    pts: Option<gst::ClockTime>,
    base: &mut Option<gst::ClockTime>,
    seq: u64,
    frame_dur: gst::ClockTime,
) -> gst::ClockTime {
    match pts {
        Some(pts) => {
            let base = base.get_or_insert(pts);
            if pts > *base {
                pts - *base
            } else {
                gst::ClockTime::ZERO
            }
        }
        None => gst::ClockTime::from_nseconds(frame_dur.nseconds().saturating_mul(seq)),
    }
}

fn frame_duration(fps_num: i32, fps_den: i32) -> gst::ClockTime {
    if fps_num <= 0 || fps_den <= 0 {
        return gst::ClockTime::from_mseconds(33);
    }
    gst::ClockTime::from_nseconds(
        gst::ClockTime::SECOND
            .nseconds()
            .saturating_mul(u64::try_from(fps_den).unwrap_or(1))
            / u64::try_from(fps_num).unwrap_or(30),
    )
}

/// Builds the `appsrc → … → mp4mux → filesink` pipeline (§6.7). The
/// `video/x-h264,profile=constrained-baseline` capsfilter is mandatory:
/// `openh264dec` — the only decoder on the system — reads nothing else.
fn build_record_pipeline(
    encoder: VideoEncoder,
    width: u32,
    height: u32,
    fps_num: i32,
    fps_den: i32,
    path: &Path,
) -> Result<(gst::Pipeline, gst_app::AppSrc), Error> {
    let pipeline = gst::Pipeline::builder().name("hiview-record").build();
    let src = gst_app::AppSrc::builder()
        .name("src")
        .is_live(true)
        .format(gst::Format::Time)
        .caps(
            &gst::Caps::builder("video/x-raw")
                .field("format", "RGBA")
                .field("width", i32::try_from(width).unwrap_or(640))
                .field("height", i32::try_from(height).unwrap_or(480))
                .field("framerate", gst::Fraction::new(fps_num, fps_den))
                .build(),
        )
        .build();
    let convert = make("videoconvert")?;
    let i420 = caps_filter(
        gst::Caps::builder("video/x-raw")
            .field("format", "I420")
            .build(),
    )?;
    let enc = make(match encoder {
        VideoEncoder::X264 => "x264enc",
        VideoEncoder::OpenH264 => "openh264enc",
    })?;
    match encoder {
        VideoEncoder::X264 => {
            enc.set_property_from_str("tune", "zerolatency");
            enc.set_property_from_str("speed-preset", "veryfast");
            enc.set_property("bitrate", BITRATE_KBPS);
        }
        VideoEncoder::OpenH264 => {
            enc.set_property("bitrate", BITRATE_KBPS * 1000);
        }
    }
    let profile = caps_filter(
        gst::Caps::builder("video/x-h264")
            .field("profile", "constrained-baseline")
            .build(),
    )?;
    let parse = make("h264parse")?;
    let mux = make("mp4mux")?;
    let sink = make("filesink")?;
    sink.set_property("location", path.to_string_lossy().as_ref());

    let src_elem = src.clone().upcast::<gst::Element>();
    pipeline.add_many([
        &src_elem, &convert, &i420, &enc, &profile, &parse, &mux, &sink,
    ])?;
    gst::Element::link_many([
        &src_elem, &convert, &i420, &enc, &profile, &parse, &mux, &sink,
    ])?;
    Ok((pipeline, src))
}

/// Consumes the subscription and pushes buffers until `feeding` is
/// cleared or the channel dies. `recv_timeout` keeps the loop responsive
/// to the stop flag without busy-waiting.
#[allow(clippy::too_many_arguments)]
fn spawn_feeder(
    appsrc: gst_app::AppSrc,
    subscription: FrameSubscription,
    width: u32,
    height: u32,
    feeding: Arc<AtomicBool>,
    rejected: Arc<AtomicU64>,
    frame_dur: gst::ClockTime,
) -> Result<JoinHandle<()>, Error> {
    thread::Builder::new()
        .name("hiview-rec-feed".into())
        .spawn(move || {
            let mut base: Option<gst::ClockTime> = None;
            let mut seq = 0u64;
            while feeding.load(Ordering::Relaxed) {
                let frame = match subscription.recv_timeout(FEED_IDLE) {
                    Ok(frame) => frame,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if frame.width != width || frame.height != height {
                    if rejected.fetch_add(1, Ordering::Relaxed) == 0 {
                        warn!(
                            "recorder dropping {}x{} frame (recording is {}x{})",
                            frame.width, frame.height, width, height
                        );
                    }
                    continue;
                }
                let Some(mut buffer) = frame_buffer(&frame) else {
                    warn!("recorder dropping undersized frame buffer");
                    continue;
                };
                let pts = rebase_pts(frame.pts, &mut base, seq, frame_dur);
                seq += 1;
                {
                    let buf = buffer.make_mut();
                    buf.set_pts(pts);
                    buf.set_duration(frame_dur);
                }
                if let Err(err) = appsrc.push_buffer(buffer) {
                    debug!("appsrc stopped accepting buffers: {err}");
                    break;
                }
            }
        })
        .map_err(|source| Error::ThreadSpawn {
            name: "hiview-rec-feed",
            source,
        })
}

/// Tightly packed RGBA pixels for the appsrc buffer; `None` when the
/// frame data is short or not RGBA. Repacks when the source has stride
/// padding so downstream elements see a contiguous frame.
fn frame_buffer(frame: &Frame) -> Option<gst::Buffer> {
    if frame.format != PixelFormat::Rgba || frame.width == 0 || frame.height == 0 {
        return None;
    }
    let row = frame.width as usize * 4;
    let height = frame.height as usize;
    let needed = height.saturating_sub(1) * frame.stride + row;
    if frame.data.len() < needed {
        return None;
    }
    if frame.stride == row {
        return Some(gst::Buffer::from_slice(frame.data.to_vec()));
    }
    let mut packed = Vec::with_capacity(row * height);
    for y in 0..height {
        packed.extend_from_slice(&frame.data[y * frame.stride..y * frame.stride + row]);
    }
    Some(gst::Buffer::from_slice(packed))
}

/// Watches the record bus: `EOS` and `Error` both terminate the stream
/// into `done` (the error also fires `on_error` for the UI); an
/// `Application` message is the private shutdown signal.
fn spawn_bus_thread(
    pipeline: &gst::Pipeline,
    done: mpsc::Sender<BusOutcome>,
    on_error: impl Fn(String) + Send + 'static,
) -> Result<JoinHandle<()>, Error> {
    let bus = pipeline
        .bus()
        .ok_or_else(|| Error::VideoDevice("record pipeline has no bus".into()))?;
    thread::Builder::new()
        .name("hiview-rec-bus".into())
        .spawn(move || {
            const TYPES: &[gst::MessageType] = &[
                gst::MessageType::Error,
                gst::MessageType::Eos,
                gst::MessageType::Application,
            ];
            while let Some(msg) = bus.timed_pop_filtered(None::<gst::ClockTime>, TYPES) {
                match msg.view() {
                    gst::MessageView::Eos(_) => {
                        let _ = done.send(BusOutcome::Eos);
                        break;
                    }
                    gst::MessageView::Error(err) => {
                        let detail = err.error().to_string();
                        error!("record pipeline error: {detail}");
                        on_error(detail.clone());
                        let _ = done.send(BusOutcome::Failed(detail));
                        break;
                    }
                    gst::MessageView::Application(_) => break,
                    _ => {}
                }
            }
        })
        .map_err(|source| Error::ThreadSpawn {
            name: "hiview-rec-bus",
            source,
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn first_frame_pts_becomes_zero() {
        let mut base = None;
        let dur = gst::ClockTime::from_mseconds(33);
        let t0 = gst::ClockTime::from_seconds(100);
        assert_eq!(
            rebase_pts(Some(t0), &mut base, 0, dur),
            gst::ClockTime::ZERO
        );
        assert_eq!(
            rebase_pts(
                Some(t0 + gst::ClockTime::from_mseconds(33)),
                &mut base,
                1,
                dur
            ),
            gst::ClockTime::from_mseconds(33)
        );
    }

    /// Frames timestamped before the base (clock jitter) clamp to zero —
    /// negative PTS would corrupt the mux timeline.
    #[test]
    fn pts_earlier_than_base_clamps_to_zero() {
        let mut base = Some(gst::ClockTime::from_seconds(100));
        let dur = gst::ClockTime::from_mseconds(33);
        assert_eq!(
            rebase_pts(Some(gst::ClockTime::from_seconds(99)), &mut base, 0, dur),
            gst::ClockTime::ZERO
        );
    }

    /// Frames without PTS advance by the nominal frame duration so the
    /// file still has a monotonic timeline.
    #[test]
    fn missing_pts_uses_synthetic_running_time() {
        let mut base = None;
        let dur = gst::ClockTime::from_mseconds(33);
        assert_eq!(
            rebase_pts(None, &mut base, 3, dur),
            gst::ClockTime::from_mseconds(99)
        );
    }

    #[test]
    fn disk_gate_accepts_at_threshold() {
        let dir = PathBuf::from("/x");
        assert_eq!(check_disk_space(&dir, |_| Some(MIN_FREE_BYTES)), Ok(()));
    }

    #[test]
    fn disk_gate_denies_below_threshold() {
        let dir = PathBuf::from("/x");
        assert_eq!(
            check_disk_space(&dir, |_| Some(MIN_FREE_BYTES - 1)),
            Err(RecordDenied::LowDiskSpace { free_mb: 499 })
        );
    }

    #[test]
    fn disk_gate_denies_when_probe_fails() {
        let dir = PathBuf::from("/x");
        assert_eq!(
            check_disk_space(&dir, |_| None),
            Err(RecordDenied::FreeSpaceUnknown)
        );
    }

    /// A strided frame is repacked contiguously for the appsrc caps.
    #[test]
    fn strided_frame_is_repacked() {
        crate::init().unwrap_or_else(|e| panic!("gst init: {e}"));
        let frame = Frame {
            width: 2,
            height: 2,
            stride: 12, // row = 8 bytes + 4 padding
            format: PixelFormat::Rgba,
            data: gst::glib::Bytes::from_owned(vec![7u8; 20]),
            pts: None,
        };
        let buf = frame_buffer(&frame).unwrap_or_else(|| panic!("frame maps to buffer"));
        assert_eq!(buf.size(), 16);
    }

    #[test]
    fn undersized_frame_data_is_rejected() {
        let frame = Frame {
            width: 640,
            height: 480,
            stride: 2560,
            format: PixelFormat::Rgba,
            data: gst::glib::Bytes::from_owned(vec![0u8; 100]),
            pts: None,
        };
        assert!(frame_buffer(&frame).is_none());
    }

    /// Encoder detection finds at least one H.264 encoder on a system
    /// that can record (x264enc or openh264enc).
    #[test]
    fn an_h264_encoder_is_available() {
        assert!(available_encoder().is_some());
    }

    /// `start` picks a collision-free name inside `dir` (§6.7).
    #[test]
    fn output_path_uses_mp4_collision_naming() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let existing = dir.path().join(video_filename(Local::now()));
        fs::write(&existing, b"busy").unwrap_or_else(|e| panic!("write: {e}"));
        let bus = crate::FrameBus::default();
        let rec = Recorder::start(bus.subscribe(), 64, 64, 30, 1, dir.path(), |_| {})
            .unwrap_or_else(|e| panic!("recorder starts: {e}"));
        assert_ne!(rec.path(), &*existing);
        assert_eq!(rec.path().extension().and_then(|e| e.to_str()), Some("mp4"));
        let stop = rec.stop();
        assert!(stop.error.is_none(), "clean stop: {:?}", stop.error);
    }
}
