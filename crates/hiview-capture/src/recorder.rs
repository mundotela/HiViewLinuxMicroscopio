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

/// Reserved `moov`-index duration: 5 min of samples ≈ 0.4 MB of fixed
/// header per file. Proven NOT to be a recording-length limit (report
/// Revisão 4): past the reserve the muxer keeps the periodic `moov`
/// rewrite working and EOS always writes a complete index — the value
/// only bounds the upfront allocation.
const RESERVED_MAX_DURATION: gst::ClockTime = gst::ClockTime::from_seconds(300);

/// Audio capture rate fed to `opusenc` (M10.5): mono 48 kHz.
const AUDIO_CAPS_RATE: i32 = 48_000;

/// Opus bitrate (bits/s) for the audio track (M10.5).
const OPUS_BITRATE_BPS: u64 = 64_000;

/// `level` element message period (M10.5): the UI meter polls the
/// latest value, so messages stay cheap and never reach the log.
const LEVEL_INTERVAL_NS: u64 = 100_000_000;

/// Element names in the audio branch share this prefix — a bus `Error`
/// whose source matches it means the microphone path died mid-recording
/// (`RecordEvent::AudioLost`), distinct from fatal muxer/sink errors.
const AUDIO_NAME_PREFIX: &str = "hiview-audio";

/// Microphone source for the audio track (M10.5). Injectable like
/// [`CaptureSource`](crate::CaptureSource): tests use `Test`
/// (`audiotestsrc`) or `Failing` (forced start failure) without a real
/// microphone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSource {
    /// `pulsesrc`; `device` is the PulseAudio/PipeWire source name —
    /// `None` records the system default.
    Pulse {
        /// PulseAudio source name; `None` = default input.
        device: Option<String>,
    },
    /// `audiotestsrc` — deterministic tone, no hardware (tests/CI).
    Test,
    /// Always fails to construct — exercises the "microphone
    /// unavailable" fallback that records video-only.
    Failing,
    /// `audiotestsrc` wrapped so its NULL→READY transition sleeps —
    /// reproduces a real `pulsesrc` opening its device *inside* the
    /// pipeline's `set_state` call (M10.5 deterministic-base_time
    /// test). The delay must cover at least the A/V tolerance.
    /// Test-only: compiled under the `test-support` feature.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    Delayed {
        /// How long the source transition takes.
        delay: std::time::Duration,
    },
}

/// The capture pipeline's clock and base time (M10.5, §6.7 A/V sync).
///
/// Video frames arrive stamped in *capture running time*. The recorder
/// shares the same [`gst::Clock`] (`use_clock`), so a frame captured at
/// absolute time `t` must be stamped `t − rec_base_time` in recorder
/// running time — i.e. `frame_pts + capture_base_time − rec_base_time`.
/// Audio buffers are timestamped by the recorder pipeline natively in
/// the same domain, which puts both tracks on one timeline. Never
/// compare these values with other clock domains (CONVENTIONS §7).
#[derive(Debug, Clone)]
pub struct ClockRef {
    /// The capture pipeline's clock — forced onto the recorder via
    /// `use_clock` so both share one time base.
    pub clock: gst::Clock,
    /// Capture pipeline `base_time`: the clock reading at its
    /// running-time zero.
    pub base_time: gst::ClockTime,
}

/// Optional inputs that shape a recording beyond its geometry (M10.5).
#[derive(Debug, Default)]
pub struct RecorderOptions {
    /// Audio track source; `None` records video-only (M4 behavior).
    pub audio: Option<AudioSource>,
    /// Capture-side clock reference, required for correct A/V sync
    /// whenever `audio` is set. `None` falls back to rebase-to-zero
    /// PTS handling (fine for video-only recordings).
    pub clock_ref: Option<ClockRef>,
}

/// Mid-recording event reported through the `on_event` callback.
#[derive(Debug)]
pub enum RecordEvent {
    /// Fatal muxer/sink failure — the file will not finalize cleanly;
    /// [`Recorder::stop`] still has to run.
    Error(String),
    /// The audio branch failed mid-recording (microphone unplugged or
    /// errored). Engineering decision (M10.5, minimum accepted): the
    /// whole recording is finalized now — EOS is injected into the
    /// audio source pad so the muxer closes the audio track cleanly.
    AudioLost,
}

/// A PulseAudio/PipeWire capture source discovered for the
/// "Microfone" preferences combo (M10.5).
#[derive(Debug, Clone)]
pub struct AudioDevice {
    /// `pulsesrc` `device` property value (PulseAudio source name).
    pub device_name: String,
    /// Human-readable label shown in the UI.
    pub display_name: String,
}

/// Lists `pulsesrc`-backed audio sources via `gst::DeviceMonitor`
/// (M10.5). Returns an empty vector on headless systems or when no
/// capture device exists — never fails.
#[must_use]
pub fn audio_sources() -> Vec<AudioDevice> {
    if crate::init().is_err() {
        return Vec::new();
    }
    let monitor = gst::DeviceMonitor::new();
    let caps = gst::Caps::builder("audio/x-raw").build();
    monitor.add_filter(Some("Audio/Source"), Some(&caps));
    if monitor.start().is_err() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for device in monitor.devices() {
        let Ok(element) = device.create_element(None) else {
            continue;
        };
        // Only pulsesrc sources feed the recorder branch — other
        // factories (e.g. pipewiresrc) are skipped so `device_name`
        // is always a valid `pulsesrc` `device` value.
        if element.factory().map(|f| f.name().to_string()).as_deref() != Some("pulsesrc") {
            continue;
        }
        // `device` is nullable on the default source — skip those.
        let Some(device_name) = element
            .property::<Option<String>>("device")
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        out.push(AudioDevice {
            device_name,
            display_name: device.display_name().to_string(),
        });
    }
    monitor.stop();
    out
}

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
    /// Audio source element, when the recording has an audio track —
    /// receives the injected EOS on stop (M10.5).
    audio_src: Option<gst::Element>,
    /// Latest microphone peak level (linear 0..1), coalesced for the
    /// UI meter; `None` for video-only recordings.
    audio_level: Option<Arc<AtomicU64>>,
    /// Whether this recording carries an audio track.
    has_audio: bool,
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
    /// `options` selects the audio track and the A/V clock reference
    /// (M10.5). When an audio source is requested but cannot be built
    /// or started, the recording proceeds **video-only** — recording is
    /// never blocked by the microphone (engineering decision) — and
    /// [`Recorder::has_audio`] reports the outcome for the UI toast.
    ///
    /// `on_event` is invoked from the bus thread on fatal muxer/sink
    /// failures ([`RecordEvent::Error`]) and on mid-recording audio
    /// loss ([`RecordEvent::AudioLost`]); [`Recorder::stop`] still has
    /// to run in both cases.
    ///
    /// # Errors
    /// Missing GStreamer elements, link/state failures, or `dir` not
    /// writable.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        subscription: FrameSubscription,
        width: u32,
        height: u32,
        fps_num: i32,
        fps_den: i32,
        dir: &Path,
        options: RecorderOptions,
        on_event: impl Fn(RecordEvent) + Send + 'static,
    ) -> Result<Self, Error> {
        Self::start_with_reserve(
            subscription,
            width,
            height,
            fps_num,
            fps_den,
            dir,
            options,
            on_event,
            RESERVED_MAX_DURATION,
        )
    }

    /// Like [`Recorder::start`] with an explicit reserved-moov index
    /// duration. Production always uses [`RESERVED_MAX_DURATION`]; the
    /// parameter exists so tests can exercise recordings that outgrow
    /// the reserve without waiting minutes.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn start_with_reserve(
        subscription: FrameSubscription,
        width: u32,
        height: u32,
        fps_num: i32,
        fps_den: i32,
        dir: &Path,
        options: RecorderOptions,
        on_event: impl Fn(RecordEvent) + Send + 'static,
        reserved_max_duration: gst::ClockTime,
    ) -> Result<Self, Error> {
        crate::init()?;
        let encoder = available_encoder()
            .ok_or_else(|| Error::VideoDevice("no H.264 encoder available".into()))?;
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::VideoDevice(format!("cannot create {}: {e}", dir.display())))?;
        let path = unique_photo_path(dir, &video_filename(Local::now()));

        // Try the audio build first when requested; any failure —
        // missing element, bad device name, unreachable server —
        // falls back to a plain video pipeline (M10.5: recording is
        // never blocked by the microphone).
        let mut has_audio = options.audio.is_some();
        let mut built = None;
        if let Some(audio) = &options.audio {
            match build_record_pipeline(
                encoder,
                width,
                height,
                fps_num,
                fps_den,
                &path,
                reserved_max_duration,
                Some(audio),
            ) {
                Ok(b) => built = Some(b),
                Err(err) => {
                    warn!("audio track unavailable ({err}); recording video-only");
                    has_audio = false;
                }
            }
        }
        let built = match built {
            Some(b) => b,
            None => {
                has_audio = false;
                build_record_pipeline(
                    encoder,
                    width,
                    height,
                    fps_num,
                    fps_den,
                    &path,
                    reserved_max_duration,
                    None,
                )?
            }
        };

        let BuiltPipeline {
            pipeline,
            appsrc,
            audio_src,
            audio_level,
        } = built;

        // A/V sync (§6.7, M10.5): share the capture clock so one time
        // base serves both pipelines, and fix the recorder's base_time
        // OURSELVES instead of letting the transition stamp it. The
        // canonical pattern is `use_clock` + `set_start_time(NONE)` +
        // `set_base_time`: with start_time invalid the pipeline never
        // recomputes base_time, so `rec_base` below is *the* value —
        // not an estimate. This matters when a child is slow inside
        // `set_state` (a real `pulsesrc` can take hundreds of ms to
        // open): without an explicit base_time the pipeline stamps it
        // *after* the slow child, and the whole open latency leaks
        // into every video PTS. With it, a slow microphone simply
        // makes the audio track start later on the shared timeline —
        // correct and synchronized; never compensated manually.
        let rec_base = options.clock_ref.as_ref().map(|reference| {
            pipeline.use_clock(Some(&reference.clock));
            pipeline.set_start_time(gst::ClockTime::NONE);
            let base = reference.clock.time();
            pipeline.set_base_time(base);
            base
        });
        if let Err(err) = pipeline.set_state(gst::State::Playing) {
            if has_audio {
                // The audio source failed to reach Playing (dead
                // device, refused connection): retry video-only.
                warn!("audio pipeline failed to start ({err}); recording video-only");
                let _ = pipeline.set_state(gst::State::Null);
                return Self::start_with_reserve(
                    subscription,
                    width,
                    height,
                    fps_num,
                    fps_den,
                    dir,
                    RecorderOptions {
                        clock_ref: options.clock_ref,
                        ..RecorderOptions::default()
                    },
                    on_event,
                    reserved_max_duration,
                );
            }
            return Err(err.into());
        }

        let pts_offset = options.clock_ref.and_then(|reference| {
            rec_base.map(|rec_base| {
                // rec_running = cap_running + cap_base − rec_base.
                reference.base_time.nseconds() as i64 - rec_base.nseconds() as i64
            })
        });

        // Frames queued while the record pipeline was still coming up
        // — build + `set_state`, which can take hundreds of ms with a
        // real `pulsesrc` opening — predate the recorder's go-live and
        // do not belong in the file. Draining here (right after the
        // transition, on this thread) is what keeps the first video
        // PTS on the same timeline spot as the audio track's first
        // PTS (M10.5). Pushing the backlog would also stamp many
        // frames at the same clamped zero, and duplicated PTS makes
        // `h264parse` emit timestamp-less buffers that `mp4mux`
        // rejects.
        let backlog = subscription.drain();
        if backlog > 0 {
            debug!("recorder discarded {backlog} pre-record frames");
        }

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
            pts_offset,
        )?;
        let (done_tx, done_rx) = mpsc::channel();
        let bus_thread = spawn_bus_thread(
            &pipeline,
            audio_src.clone(),
            audio_level.clone(),
            done_tx,
            on_event,
        )?;

        info!(
            "recording to {} ({encoder:?}, {width}x{height}, audio={has_audio})",
            path.display()
        );

        Ok(Self {
            pipeline,
            appsrc,
            audio_src,
            audio_level,
            has_audio,
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

    /// Whether this recording carries an audio track (M10.5). `false`
    /// either when audio was not requested or when the microphone
    /// could not be opened and the fallback recorded video-only.
    #[must_use]
    pub fn has_audio(&self) -> bool {
        self.has_audio
    }

    /// Test hook: the audio source element, so an integration test can
    /// post an error as if the microphone died mid-recording and
    /// exercise the `AudioLost` path the bus thread takes for real
    /// device failures.
    #[doc(hidden)]
    #[must_use]
    pub fn audio_src_element(&self) -> Option<gst::Element> {
        self.audio_src.clone()
    }

    /// Latest microphone peak level as a linear 0..1 value, for the
    /// recording indicator's meter (M10.5). Coalesced — the `level`
    /// element posts at 100 ms and the UI reads the freshest value on
    /// its own schedule. `None` for video-only recordings.
    #[must_use]
    pub fn audio_level(&self) -> Option<f32> {
        self.audio_level
            .as_ref()
            .map(|level| f64::from_bits(level.load(Ordering::Relaxed)).clamp(0.0, 1.0) as f32)
    }

    /// Runs the mandatory stop sequence (§6.7): `end_of_stream` on the
    /// appsrc **and** EOS injected into the audio source (M10.5 — both
    /// tracks must end for the muxer to write `moov`), wait for `EOS`
    /// on the bus up to 5 s, then `Null`. Blocks for at most
    /// `EOS_TIMEOUT` + drain; call it from a worker thread, never the
    /// UI thread (docs/CONVENTIONS.md §7).
    pub fn stop(mut self) -> RecordStop {
        self.feeding.store(false, Ordering::Relaxed);
        // EOS on the audio source src-pad travels the audio branch to
        // the muxer; basesrc forwards it even when its streaming task
        // already died (mic unplugged mid-recording).
        if let Some(asrc) = &self.audio_src
            && !asrc.send_event(gst::event::Eos::new())
        {
            debug!("audio source refused EOS (already flushing?)");
        }
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
        // set_state may report Async: wait for the transition so filesink
        // has flushed and closed the MP4 before this returns.
        let _ = self.pipeline.state(gst::ClockTime::from_seconds(5));
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

/// Translates a capture PTS into the recorder's running-time domain
/// (M10.5 A/V sync): `rec_running = cap_running + cap_base − rec_base`,
/// where `offset = cap_base − rec_base` (signed — negative when the
/// recorder started after the capture pipeline, the common case).
/// Timestamps landing before zero are clamped — never negative.
fn translate_pts(
    pts: Option<gst::ClockTime>,
    offset: i64,
    seq: u64,
    frame_dur: gst::ClockTime,
) -> gst::ClockTime {
    match pts {
        Some(pts) => {
            let translated = pts.nseconds() as i64 + offset;
            if translated > 0 {
                gst::ClockTime::from_nseconds(translated as u64)
            } else {
                gst::ClockTime::ZERO
            }
        }
        None => gst::ClockTime::from_nseconds(frame_dur.nseconds().saturating_mul(seq)),
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

/// The record pipeline plus the pieces the [`Recorder`] keeps handles to.
struct BuiltPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    /// Audio source element for the stop-time EOS injection (`None`
    /// for video-only recordings).
    audio_src: Option<gst::Element>,
    /// Shared cell the bus thread fills with the latest `level` peak.
    audio_level: Option<Arc<AtomicU64>>,
}

/// Builds the `appsrc → … → mp4mux → filesink` pipeline (§6.7), plus
/// the M10.5 audio branch when `audio` is set:
///
/// ```text
/// <audio src> ! audioconvert ! audioresample
///   ! audio/x-raw,rate=48000,channels=1 ! level ! queue
///   ! opusenc bitrate=64000 ! queue ! mp4mux (audio_%u request pad)
/// ```
///
/// The `video/x-h264,profile=constrained-baseline` capsfilter is
/// mandatory: `openh264dec` — the only decoder on the system — reads
/// nothing else.
#[allow(clippy::too_many_arguments)]
fn build_record_pipeline(
    encoder: VideoEncoder,
    width: u32,
    height: u32,
    fps_num: i32,
    fps_den: i32,
    path: &Path,
    reserved_max_duration: gst::ClockTime,
    audio: Option<&AudioSource>,
) -> Result<BuiltPipeline, Error> {
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
            enc.set_property_from_str("speed-preset", "superfast");
            enc.set_property("bitrate", BITRATE_KBPS);
            // One encoder thread: at 640×480 the default pool costs more
            // in synchronization than it saves in encode time (§7 budget).
            enc.set_property("threads", 1_u32);
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
    // Crash resilience (engineering decision, §6.7): reserve index
    // space and rewrite the on-disk `moov` every second. A crash,
    // SIGKILL or power loss leaves a seekable file whose header already
    // covers (nearly) everything written — the fragmented-MP4 modes
    // were measured to report only the first fragment's duration.
    // EOS past the reserve still writes a complete index (Revisão 4).
    mux.set_property("reserved-max-duration", reserved_max_duration);
    mux.set_property("reserved-moov-update-period", 1_000_000_000_u64);
    let sink = make("filesink")?;
    sink.set_property("location", path.to_string_lossy().as_ref());

    let src_elem = src.clone().upcast::<gst::Element>();
    pipeline.add_many([
        &src_elem, &convert, &i420, &enc, &profile, &parse, &mux, &sink,
    ])?;
    gst::Element::link_many([
        &src_elem, &convert, &i420, &enc, &profile, &parse, &mux, &sink,
    ])?;

    let mut audio_src = None;
    let mut audio_level = None;
    if let Some(source) = audio {
        let (asrc, level_cell) = build_audio_branch(&pipeline, &mux, source).inspect_err(|_| {
            // Leave the video-only pipeline usable for the
            // video-only retry the caller performs on error.
            let _ = pipeline.set_state(gst::State::Null);
        })?;
        audio_src = Some(asrc);
        audio_level = Some(level_cell);
    }

    Ok(BuiltPipeline {
        pipeline,
        appsrc: src,
        audio_src,
        audio_level,
    })
}

/// Builds the audio branch into `pipeline` and requests the muxer's
/// `audio_%u` pad (M10.5). The source is the only piece that varies:
/// `pulsesrc` (with an optional `device` name) in production,
/// `audiotestsrc` in tests. All branch elements carry the
/// [`AUDIO_NAME_PREFIX`] so the bus thread can tell an audio failure
/// from a fatal muxer/sink error.
///
/// Returns the source element (for stop-time EOS) and the shared cell
/// the `level` messages update.
fn build_audio_branch(
    pipeline: &gst::Pipeline,
    mux: &gst::Element,
    source: &AudioSource,
) -> Result<(gst::Element, Arc<AtomicU64>), Error> {
    // `eos_target` is the element that receives stop-time EOS — the
    // real source inside the wrapper for `Delayed`.
    let (asrc, eos_target) = match source {
        AudioSource::Pulse { device } => {
            let src = make("pulsesrc")?;
            if let Some(device) = device {
                src.set_property("device", device);
            }
            let eos = src.clone();
            (src, eos)
        }
        AudioSource::Test => {
            let src = make("audiotestsrc")?;
            src.set_property("is-live", true);
            let eos = src.clone();
            (src, eos)
        }
        #[cfg(feature = "test-support")]
        AudioSource::Delayed { delay } => {
            let src = make("audiotestsrc")?;
            src.set_property("is-live", true);
            src.set_property("name", format!("{AUDIO_NAME_PREFIX}-src-inner"));
            (
                crate::delayed_bin::DelayedBin::new(&src, *delay)?.upcast(),
                src,
            )
        }
        AudioSource::Failing => {
            return Err(Error::VideoDevice("injected audio source failure".into()));
        }
    };
    asrc.set_property("name", format!("{AUDIO_NAME_PREFIX}-src"));

    // Preflight: reaching Paused forces the source to open its device
    // (pulsesrc connects to the server here), so a dead microphone is
    // caught before the pipeline ever runs — the caller falls back to
    // video-only instead of starting a doomed recording.
    if let Err(err) = asrc.set_state(gst::State::Paused) {
        return Err(Error::VideoDevice(format!(
            "audio source refused to open: {err}"
        )));
    }
    let (result, state, _pending) = asrc.state(Some(gst::ClockTime::from_seconds(3)));
    let opened = result.is_ok() && state == gst::State::Paused;
    if !opened {
        let _ = asrc.set_state(gst::State::Null);
        return Err(Error::VideoDevice("audio source failed to open".into()));
    }
    let _ = asrc.set_state(gst::State::Null);

    let aconv = make("audioconvert")?;
    let ares = make("audioresample")?;
    let acaps = caps_filter(
        gst::Caps::builder("audio/x-raw")
            .field("rate", AUDIO_CAPS_RATE)
            .field("channels", 1_i32)
            .build(),
    )?;
    let level = make("level")?;
    level.set_property("name", format!("{AUDIO_NAME_PREFIX}-level"));
    level.set_property("post-messages", true);
    level.set_property("interval", LEVEL_INTERVAL_NS);
    let queue_in = make("queue")?;
    let aenc = make("opusenc")?;
    aenc.set_property_from_str("bitrate", &OPUS_BITRATE_BPS.to_string());
    let queue_out = make("queue")?;
    // Short audio queues (~250 ms): a mid-recording audio EOS must
    // actually cut the track near the loss point — a deep queue
    // drains its backlog through after the EOS and the tail of the
    // file would still contain ~1 s of "post-loss" audio (M10.5).
    queue_in.set_property("max-size-time", 250_000_000_u64);
    queue_out.set_property("max-size-time", 250_000_000_u64);

    for (index, element) in [&aconv, &ares, &acaps, &queue_in, &aenc, &queue_out]
        .into_iter()
        .enumerate()
    {
        element.set_property("name", format!("{AUDIO_NAME_PREFIX}-{index}"));
    }

    let level_cell = Arc::new(AtomicU64::new(0f64.to_bits()));

    pipeline.add_many([
        &asrc, &aconv, &ares, &acaps, &level, &queue_in, &aenc, &queue_out,
    ])?;
    gst::Element::link_many([
        &asrc, &aconv, &ares, &acaps, &level, &queue_in, &aenc, &queue_out,
    ])?;
    // Request muxer pads explicitly — `audio_%u` is an on-request
    // template, not a static pad.
    let mux_pad = mux
        .request_pad_simple("audio_%u")
        .ok_or_else(|| Error::VideoDevice("mp4mux refused an audio pad".into()))?;
    queue_out
        .static_pad("src")
        .ok_or_else(|| Error::VideoDevice("audio queue has no src pad".into()))?
        .link(&mux_pad)
        .map_err(Error::Link)?;

    Ok((eos_target, level_cell))
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
    pts_offset: Option<i64>,
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
                // With a shared clock (audio recordings, M10.5) the PTS
                // is translated capture→recorder running time so the
                // video track lands on the audio track's timeline;
                // video-only keeps the M4 rebase-to-zero behavior.
                let pts = match pts_offset {
                    Some(offset) => translate_pts(frame.pts, offset, seq, frame_dur),
                    None => rebase_pts(frame.pts, &mut base, seq, frame_dur),
                };
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

/// Extracts the first channel's peak (dB) from a `level` element
/// message and stores it as a linear 0..1 value. The dB range −60..0
/// is the conventional VU scale.
fn update_level_cell(structure: &gst::StructureRef, cell: &AtomicU64) {
    let Ok(peaks) = structure.get::<gst::List>("peak") else {
        return;
    };
    let Some(peak_db) = peaks.first().and_then(|v| v.get::<f64>().ok()) else {
        return;
    };
    let linear = ((peak_db + 60.0) / 60.0).clamp(0.0, 1.0);
    cell.store(linear.to_bits(), Ordering::Relaxed);
}

/// Watches the record bus: `EOS` and `Error` both terminate the stream
/// into `done` (the error also fires `on_event` for the UI); `Element`
/// messages from `level` update the shared meter cell; an
/// `Application` message is the private shutdown signal.
///
/// An `Error` whose source belongs to the audio branch
/// ([`AUDIO_NAME_PREFIX`]) means the microphone died mid-recording —
/// EOS is injected into the audio source immediately so the muxer can
/// close that track, and the UI is told via [`RecordEvent::AudioLost`].
fn spawn_bus_thread(
    pipeline: &gst::Pipeline,
    audio_src: Option<gst::Element>,
    audio_level: Option<Arc<AtomicU64>>,
    done: mpsc::Sender<BusOutcome>,
    on_event: impl Fn(RecordEvent) + Send + 'static,
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
                gst::MessageType::Element,
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
                        let from_audio = msg
                            .src()
                            .map(|o| o.name().starts_with(AUDIO_NAME_PREFIX))
                            .unwrap_or(false);
                        if from_audio {
                            // Audio died but video is still healthy:
                            // close the audio track and let the UI
                            // finalize the file (M10.5 minimum path).
                            warn!("audio track failed mid-recording: {detail}");
                            if let Some(asrc) = &audio_src {
                                let _ = asrc.send_event(gst::event::Eos::new());
                            }
                            on_event(RecordEvent::AudioLost);
                            continue;
                        }
                        error!("record pipeline error: {detail}");
                        on_event(RecordEvent::Error(detail.clone()));
                        let _ = done.send(BusOutcome::Failed(detail));
                        break;
                    }
                    gst::MessageView::Element(element) => {
                        if let Some(cell) = &audio_level
                            && element.structure().is_some_and(|s| s.name() == "level")
                            && let Some(s) = element.structure()
                        {
                            update_level_cell(s, cell);
                        }
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

    /// M10.5 A/V sync: a frame captured at absolute time
    /// `cap_running + cap_base` must land at `t − rec_base` in recorder
    /// running time — `offset = cap_base − rec_base` is typically
    /// negative (the recorder starts after capture).
    #[test]
    fn translate_pts_places_frame_on_recorder_timeline() {
        let dur = gst::ClockTime::from_mseconds(33);
        // Capture playing for 5 s when the recorder starts:
        // offset = cap_base − rec_base = −5 s.
        let offset = -5_000_000_000i64;
        // Frame at capture running time 5.010 s → absolute capture+base,
        // i.e. 10 ms after the recorder went Playing.
        assert_eq!(
            translate_pts(Some(gst::ClockTime::from_mseconds(5010)), offset, 0, dur),
            gst::ClockTime::from_mseconds(10)
        );
        // A frame sitting in the subscription queue from before the
        // recorder's base time clamps to zero, never negative.
        assert_eq!(
            translate_pts(Some(gst::ClockTime::from_mseconds(4990)), offset, 0, dur),
            gst::ClockTime::ZERO
        );
        // Missing PTS still advances synthetically.
        assert_eq!(
            translate_pts(None, offset, 3, dur),
            gst::ClockTime::from_mseconds(99)
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
        let rec = Recorder::start(
            bus.subscribe(),
            64,
            64,
            30,
            1,
            dir.path(),
            RecorderOptions::default(),
            |_| {},
        )
        .unwrap_or_else(|e| panic!("recorder starts: {e}"));
        assert_ne!(rec.path(), &*existing);
        assert_eq!(rec.path().extension().and_then(|e| e.to_str()), Some("mp4"));
        let stop = rec.stop();
        assert!(stop.error.is_none(), "clean stop: {:?}", stop.error);
    }
}
