// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Frame type and the handoff between the streaming thread and the UI
//! (docs/ARCHITECTURE.md §6.2).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use gst::glib;

/// Pixel layout of a [`Frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// Packed RGBA, four bytes per pixel (the only format delivered in M1).
    Rgba,
}

/// A decoded video frame, safe to share across threads.
///
/// `data` is reference-counted, so cloning a `Frame` is cheap.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row in `data` (may exceed `width * 4` for padding).
    pub stride: usize,
    /// Pixel layout of `data`.
    pub format: PixelFormat,
    /// Pixel bytes.
    pub data: glib::Bytes,
    /// Presentation timestamp, when the source provides one.
    pub pts: Option<gst::ClockTime>,
}

type NotifyFn = Box<dyn Fn() + Send + Sync>;

/// Depth of each full-subscriber queue (docs/ARCHITECTURE.md §6.3):
/// when full, the frame is dropped and counted instead of blocking the
/// streaming thread or growing memory without bound.
pub const FRAME_SUBSCRIPTION_CAPACITY: usize = 8;

struct Subscriber {
    tx: mpsc::SyncSender<Frame>,
    dropped: Arc<AtomicU64>,
}

#[derive(Default)]
struct FrameBusInner {
    latest: Mutex<Option<Frame>>,
    ui_notify: OnceLock<NotifyFn>,
    ui_pending: AtomicBool,
    published: AtomicU64,
    subscribers: Mutex<Vec<Subscriber>>,
}

/// A full-subscription to a [`FrameBus`]: every published frame, in order.
///
/// Unlike `take_latest_for_ui` (which coalesces), subscribers get their own
/// bounded queue. Frames dropped on a full queue are counted and must be
/// surfaced in diagnostics — the video they were part of is missing them.
pub struct FrameSubscription {
    rx: mpsc::Receiver<Frame>,
    dropped: Arc<AtomicU64>,
}

impl FrameSubscription {
    /// Receives the next frame, blocking until one is published or every
    /// publisher is gone. Intended for a dedicated feeder thread.
    pub fn recv(&self) -> Result<Frame, mpsc::RecvError> {
        self.rx.recv()
    }

    /// Like [`FrameSubscription::recv`], but wakes after `timeout` so the
    /// feeder can poll a stop flag while the camera is silent.
    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Frame, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }

    /// Drains every buffered frame, counting them as dropped. Used by
    /// the recorder to discard frames captured while its pipeline was
    /// still coming up — they predate the recording's time base and do
    /// not belong in the file (M10.5).
    pub fn drain(&self) -> u64 {
        let mut drained = 0;
        while self.rx.try_recv().is_ok() {
            drained += 1;
        }
        self.dropped.fetch_add(drained, Ordering::Relaxed);
        drained
    }

    /// Frames dropped because this subscription's queue was full.
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// A handle to the dropped-frame counter, shareable with diagnostics.
    #[must_use]
    pub fn dropped_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.dropped)
    }
}

/// Coalescing frame channel between the streaming thread and the UI.
///
/// The streaming thread publishes frames; the UI is notified at most once per
/// read (`docs/ARCHITECTURE.md §6.2`). Clones share the same underlying channel.
#[derive(Clone, Default)]
pub struct FrameBus {
    inner: Arc<FrameBusInner>,
}

impl FrameBus {
    /// Registers the callback invoked once per pending frame batch.
    ///
    /// Must be called before streaming starts; only the first callback is kept.
    pub fn set_ui_notify(&self, notify: NotifyFn) {
        let _ = self.inner.ui_notify.set(notify);
    }

    /// Stores `frame` as the latest frame, forwards it to every full
    /// subscriber and notifies the UI at most once.
    pub fn publish(&self, frame: Frame) {
        self.inner.published.fetch_add(1, Ordering::Relaxed);
        self.deliver_to_subscribers(&frame);
        *lock(&self.inner.latest) = Some(frame);
        if self.inner.ui_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(notify) = self.inner.ui_notify.get() {
            notify();
        }
    }

    /// Registers a bounded full subscriber (§6.3). Dropping the returned
    /// [`FrameSubscription`] disconnects it.
    #[must_use]
    pub fn subscribe(&self) -> FrameSubscription {
        let (tx, rx) = mpsc::sync_channel(FRAME_SUBSCRIPTION_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        lock(&self.inner.subscribers).push(Subscriber {
            tx,
            dropped: Arc::clone(&dropped),
        });
        FrameSubscription { rx, dropped }
    }

    /// Takes the latest frame for the UI and clears the pending flag.
    pub fn take_latest_for_ui(&self) -> Option<Frame> {
        self.inner.ui_pending.store(false, Ordering::Release);
        lock(&self.inner.latest).take()
    }

    /// Total frames ever published on this bus (monotonic; for tests and
    /// diagnostics, since the UI only ever sees the latest).
    #[must_use]
    pub fn published_count(&self) -> u64 {
        self.inner.published.load(Ordering::Relaxed)
    }

    /// Non-blocking delivery to all live subscribers; dead channels are
    /// pruned, full queues count a dropped frame (§6.3).
    fn deliver_to_subscribers(&self, frame: &Frame) {
        let mut subscribers = lock(&self.inner.subscribers);
        subscribers.retain_mut(|sub| match sub.tx.try_send(frame.clone()) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(_)) => {
                let dropped = sub.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped == 1 || dropped.is_multiple_of(30) {
                    tracing::warn!("frame subscriber queue full; dropped ({dropped} total)");
                }
                true
            }
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        });
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn coalesces_ui_notifications() {
        use std::sync::atomic::AtomicUsize;

        let bus = FrameBus::default();
        let count = Arc::new(AtomicUsize::new(0));
        let count2 = Arc::clone(&count);
        bus.set_ui_notify(Box::new(move || {
            count2.fetch_add(1, Ordering::SeqCst);
        }));

        bus.publish(frame(1));
        bus.publish(frame(2));
        assert_eq!(count.load(Ordering::SeqCst), 1);

        let latest = bus.take_latest_for_ui();
        assert_eq!(latest.map(|f| f.data[0]), Some(2));
        assert!(bus.take_latest_for_ui().is_none());
    }

    /// Full subscribers receive every frame in order, independent of the
    /// coalesced UI path (docs/ARCHITECTURE.md §6.3).
    #[test]
    fn subscriber_gets_every_frame() {
        let bus = FrameBus::default();
        let sub = bus.subscribe();
        for v in 0..FRAME_SUBSCRIPTION_CAPACITY as u8 {
            bus.publish(frame(v));
        }
        for v in 0..FRAME_SUBSCRIPTION_CAPACITY as u8 {
            assert_eq!(sub.recv().map(|f| f.data[0]), Ok(v));
        }
        assert_eq!(sub.dropped_count(), 0);
    }

    /// A queue left full drops frames and counts them; the channel never
    /// blocks the publishing thread and never grows past capacity (§6.3).
    #[test]
    fn full_subscriber_queue_drops_and_counts() {
        let bus = FrameBus::default();
        let sub = bus.subscribe();
        for v in 0..(FRAME_SUBSCRIPTION_CAPACITY as u8 + 5) {
            bus.publish(frame(v));
        }
        assert_eq!(sub.dropped_count(), 5);
        // The queued frames are the first eight — drops are the newest.
        for v in 0..FRAME_SUBSCRIPTION_CAPACITY as u8 {
            assert_eq!(sub.recv().map(|f| f.data[0]), Ok(v));
        }
    }

    /// Dropping a subscription disconnects it without affecting delivery
    /// to the remaining subscribers.
    #[test]
    fn dead_subscriber_is_pruned() {
        let bus = FrameBus::default();
        let dead = bus.subscribe();
        let live = bus.subscribe();
        drop(dead);
        bus.publish(frame(7));
        assert_eq!(live.recv().map(|f| f.data[0]), Ok(7));
    }
}
