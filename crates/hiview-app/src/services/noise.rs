// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Noise-reduction photo capture (M10): a short-lived worker collects
//! N consecutive frames from a full FrameBus subscription, averages
//! them with `hiview_core::FrameAverager` and reports the result — the
//! UI thread never blocks and the encode still runs on the photo
//! worker afterwards (§7).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hiview_capture::{Frame, FrameSubscription, PixelFormat};
use hiview_core::{AverageError, FrameAverager};
use relm4::gtk::glib;
use tracing::{debug, warn};

/// How long the worker waits for N frames before giving up (M10 spec).
pub const NOISE_TIMEOUT: Duration = Duration::from_secs(3);

/// Why a capture ended without a photo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseCancel {
    /// N frames did not arrive within the deadline.
    Timeout,
    /// Frame geometry changed mid-capture (rotation/resolution).
    SizeChanged,
    /// The frame publisher went away (camera service torn down).
    Disconnected,
}

/// One worker report to the UI, on the app's own message channel.
#[derive(Debug)]
pub enum NoiseOutcome {
    /// `got` of `total` frames collected — drives the progress bar.
    Progress { got: u32, total: u32 },
    /// The averaged frame and how many frames fed it — the photo
    /// metadata records this, not the (possibly changed) preference.
    Done { frame: Frame, frames: u32 },
    /// The capture gave up; the UI shows the matching toast.
    Cancelled(NoiseCancel),
}

/// Handle to a running capture. `cancel()` — also run on `Drop`, so
/// abandoning the handle on shutdown still stops the thread — asks the
/// worker to exit quietly: the app already showed the right toast.
pub struct NoiseCapture {
    cancel: Arc<AtomicBool>,
    _thread: JoinHandle<()>,
}

impl NoiseCapture {
    /// Stops collection without a photo; the worker exits silently.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for NoiseCapture {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Starts collecting `frames` consecutive frames off `subscription`.
///
/// `on_update` receives [`NoiseOutcome`]s from the worker thread —
/// callers forward them to the Relm4 input sender. The last report is
/// always `Done` or `Cancelled` unless [`NoiseCapture::cancel`] fired
/// first, in which case the thread simply exits.
pub fn capture<F>(subscription: FrameSubscription, frames: u32, on_update: F) -> NoiseCapture
where
    F: Fn(NoiseOutcome) + Send + 'static,
{
    capture_with_timeout(subscription, frames, NOISE_TIMEOUT, on_update)
}

/// [`capture`] with an explicit deadline — tests use short timeouts.
pub fn capture_with_timeout<F>(
    subscription: FrameSubscription,
    frames: u32,
    timeout: Duration,
    on_update: F,
) -> NoiseCapture
where
    F: Fn(NoiseOutcome) + Send + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let thread = std::thread::Builder::new()
        .name("hiview-noise".into())
        .spawn(move || collect(subscription, frames, timeout, &flag, &on_update))
        .unwrap_or_else(|e| panic!("spawn noise worker: {e}"));
    NoiseCapture {
        cancel,
        _thread: thread,
    }
}

/// The collection loop: every frame feeds the averager, reports
/// progress and returns the mean once `frames` arrived. Timeout and
/// geometry changes cancel; a user/disconnect cancel exits quietly.
fn collect<F>(
    subscription: FrameSubscription,
    frames: u32,
    timeout: Duration,
    cancel: &AtomicBool,
    on_update: &F,
) where
    F: Fn(NoiseOutcome) + Send + 'static,
{
    let emit = |outcome: NoiseOutcome| on_update(outcome);
    // Arrival-clock deadline (§7 conventions): never compare capture
    // timestamps against the app's monotonic clock — only elapsed time
    // measured in this thread decides the timeout.
    let deadline = Instant::now() + timeout;
    let mut averager = FrameAverager::new();
    let mut got = 0u32;
    loop {
        if cancel.load(Ordering::Relaxed) {
            debug!("noise capture cancelled by the app");
            return;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            emit(NoiseOutcome::Cancelled(NoiseCancel::Timeout));
            return;
        };
        let frame = match subscription.recv_timeout(remaining) {
            Ok(frame) => frame,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                emit(NoiseOutcome::Cancelled(NoiseCancel::Timeout));
                return;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                emit(NoiseOutcome::Cancelled(NoiseCancel::Disconnected));
                return;
            }
        };
        match averager.push(&frame.data, frame.width, frame.height, frame.stride) {
            Ok(()) => {
                got += 1;
                emit(NoiseOutcome::Progress { got, total: frames });
                if got == frames {
                    let skipped = subscription.dropped_count();
                    if skipped > 0 {
                        // Skipped frames are accepted by design (§6.3):
                        // the average still used N consecutive
                        // deliveries — count them for diagnostics.
                        debug!("noise capture accepted {skipped} skipped frame(s)");
                    }
                    match averager.finish() {
                        Ok(data) => emit(NoiseOutcome::Done {
                            frame: Frame {
                                width: frame.width,
                                height: frame.height,
                                stride: frame.stride,
                                format: PixelFormat::Rgba,
                                data: glib::Bytes::from_owned(data),
                                pts: None,
                            },
                            frames: got,
                        }),
                        // count > 0 ⇒ finish cannot be Empty.
                        Err(e) => warn!("noise average failed: {e}"),
                    }
                    return;
                }
            }
            Err(AverageError::DimensionMismatch { .. } | AverageError::BadLength { .. }) => {
                emit(NoiseOutcome::Cancelled(NoiseCancel::SizeChanged));
                return;
            }
            Err(e @ AverageError::Empty) => warn!("noise average failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use hiview_capture::FrameBus;

    use super::*;

    fn frame(v: u8) -> Frame {
        Frame {
            width: 2,
            height: 2,
            stride: 8,
            format: PixelFormat::Rgba,
            data: glib::Bytes::from_owned(vec![v; 16]),
            pts: None,
        }
    }

    fn recv_outcome(rx: &mpsc::Receiver<NoiseOutcome>) -> NoiseOutcome {
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|e| panic!("outcome never arrived: {e}"))
    }

    /// N published frames → Progress×N then Done with the mean.
    #[test]
    fn collects_n_frames_and_averages() {
        let bus = FrameBus::default();
        let (tx, rx) = mpsc::channel();
        let _cap = capture_with_timeout(bus.subscribe(), 4, Duration::from_secs(10), move |o| {
            let _ = tx.send(o);
        });
        for v in [10u8, 11, 12, 13] {
            bus.publish(frame(v));
        }
        let mut progresses = 0;
        let done = loop {
            match recv_outcome(&rx) {
                NoiseOutcome::Progress { got, total } => {
                    progresses += 1;
                    assert_eq!(total, 4);
                    assert_eq!(got, progresses);
                }
                done @ NoiseOutcome::Done { .. } => break done,
                other => panic!("unexpected {other:?}"),
            }
        };
        assert_eq!(progresses, 4);
        let NoiseOutcome::Done { frame, frames } = done else {
            panic!("expected Done");
        };
        assert_eq!(frames, 4);
        // mean of 10..=13 = 11.5 → rounds to 12.
        assert_eq!(&*frame.data, &[12u8; 16]);
    }

    /// Frames that never arrive hit the deadline — the app shows
    /// "tempo esgotado" (spec: timeout testable without hardware).
    #[test]
    fn missing_frames_time_out() {
        let bus = FrameBus::default();
        let (tx, rx) = mpsc::channel();
        let _cap = capture_with_timeout(bus.subscribe(), 8, Duration::from_millis(150), move |o| {
            let _ = tx.send(o);
        });
        bus.publish(frame(1)); // one frame, then silence
        assert!(matches!(
            recv_outcome(&rx),
            NoiseOutcome::Progress { got: 1, total: 8 }
        ));
        assert!(matches!(
            recv_outcome(&rx),
            NoiseOutcome::Cancelled(NoiseCancel::Timeout)
        ));
    }

    /// A geometry change mid-capture cancels with SizeChanged.
    #[test]
    fn geometry_change_cancels() {
        let bus = FrameBus::default();
        let (tx, rx) = mpsc::channel();
        let _cap = capture_with_timeout(bus.subscribe(), 4, Duration::from_secs(10), move |o| {
            let _ = tx.send(o);
        });
        bus.publish(frame(1));
        bus.publish(Frame {
            width: 4,
            height: 1,
            stride: 16,
            ..frame(2)
        });
        // Progress(1), then the mismatched frame cancels.
        assert!(matches!(
            recv_outcome(&rx),
            NoiseOutcome::Progress { got: 1, total: 4 }
        ));
        assert!(matches!(
            recv_outcome(&rx),
            NoiseOutcome::Cancelled(NoiseCancel::SizeChanged)
        ));
    }

    /// `cancel()` stops the worker quietly — no outcome races the
    /// app's own "câmera desconectada" toast.
    #[test]
    fn app_cancel_exits_silently() {
        let bus = FrameBus::default();
        let (tx, rx) = mpsc::channel();
        let cap = capture_with_timeout(bus.subscribe(), 16, Duration::from_secs(60), move |o| {
            let _ = tx.send(o);
        });
        cap.cancel();
        // Nothing arrives: the worker exits instead of reporting.
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    /// End-to-end at file level: the averaged frame goes through the
    /// normal photo save and `noise_reduction_frames` lands in the PNG
    /// `tEXt` chunk that tools can read back.
    #[test]
    fn averaged_photo_carries_the_frame_count_in_png() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let bus = FrameBus::default();
        let (tx, rx) = mpsc::channel();
        let _cap = capture_with_timeout(bus.subscribe(), 4, Duration::from_secs(10), move |o| {
            let _ = tx.send(o);
        });
        for v in [10u8, 11, 12, 13] {
            bus.publish(frame(v));
        }
        let frame = loop {
            match recv_outcome(&rx) {
                NoiseOutcome::Done { frame, .. } => break frame,
                NoiseOutcome::Progress { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        };
        let path = hiview_core::save_photo_with(
            &frame.data,
            frame.width,
            frame.height,
            frame.stride,
            hiview_core::PhotoResolution::Native,
            hiview_core::PhotoFormat::Png,
            dir.path(),
            |_| {},
            &[("noise_reduction_frames".into(), "4".into())],
        )
        .unwrap_or_else(|e| panic!("save: {e}"));
        let png = std::fs::read(&path).unwrap_or_else(|e| panic!("read: {e}"));
        let text = hiview_core::extract_png_text(&png);
        assert_eq!(
            text.iter()
                .find(|(k, _)| k == "noise_reduction_frames")
                .map(|(_, v)| v.as_str()),
            Some("4")
        );
    }

    /// When the bus dies mid-capture the worker reports Disconnected —
    /// the app maps it to the same "câmera desconectada" toast.
    #[test]
    fn dead_bus_reports_disconnected() {
        let bus = FrameBus::default();
        let (tx, rx) = mpsc::channel();
        let _cap = capture_with_timeout(bus.subscribe(), 4, Duration::from_secs(60), move |o| {
            let _ = tx.send(o);
        });
        bus.publish(frame(1));
        drop(bus);
        let mut saw_disconnected = false;
        // Drain until Disconnected (Progress may arrive first).
        for _ in 0..3 {
            if matches!(
                recv_outcome(&rx),
                NoiseOutcome::Cancelled(NoiseCancel::Disconnected)
            ) {
                saw_disconnected = true;
                break;
            }
        }
        assert!(saw_disconnected);
    }
}
