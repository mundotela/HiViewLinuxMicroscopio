// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Frame type and the handoff between the streaming thread and the UI
//! (docs/ARCHITECTURE.md §6.2).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

#[derive(Default)]
struct FrameBusInner {
    latest: Mutex<Option<Frame>>,
    ui_notify: OnceLock<NotifyFn>,
    ui_pending: AtomicBool,
    published: AtomicU64,
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

    /// Stores `frame` as the latest frame and notifies the UI at most once.
    pub fn publish(&self, frame: Frame) {
        self.inner.published.fetch_add(1, Ordering::Relaxed);
        *lock(&self.inner.latest) = Some(frame);
        if self.inner.ui_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(notify) = self.inner.ui_notify.get() {
            notify();
        }
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
}
