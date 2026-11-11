// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Photo save worker (docs/ARCHITECTURE.md §6.6, §7). Encoding and disk
//! I/O never run on the UI thread: jobs queue on a dedicated thread that
//! also serializes filename collision checks, so a burst of rapid shots
//! always produces distinct files.

use std::path::PathBuf;
use std::sync::mpsc;

use hiview_capture::Frame;
use hiview_core::{PhotoConfig, save_photo};
use relm4::gtk::glib;

/// Result of one save job, reported back to the UI.
#[derive(Debug)]
pub enum PhotoOutcome {
    /// Photo written at this path.
    Saved(PathBuf),
    /// Save failed; the string is a short technical reason for the log.
    Failed(String),
}

/// Maximum photos waiting in the worker queue (M3 review): further
/// shots are rejected instead of piling up.
const PHOTO_QUEUE_CAPACITY: usize = 16;

/// Why a shot could not be queued.
#[derive(Debug, PartialEq, Eq)]
pub enum TakeRejected {
    /// Queue is full — the worker is still encoding previous shots.
    Busy,
    /// The worker thread is gone.
    Gone,
}

struct PhotoJob {
    data: glib::Bytes,
    width: u32,
    height: u32,
    stride: usize,
    prefs: PhotoConfig,
    dir: PathBuf,
}

/// Handle to the background photo worker.
pub struct PhotoService {
    jobs: mpsc::SyncSender<PhotoJob>,
}

impl PhotoService {
    /// Spawns the worker thread. `on_done` is called for every finished
    /// job (from the worker thread — callers forward it to the Relm4
    /// sender, which is safe to call cross-thread).
    pub fn new<F>(on_done: F) -> Self
    where
        F: Fn(PhotoOutcome) + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel::<PhotoJob>(PHOTO_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("hiview-photo".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    let outcome = save_photo(
                        &job.data,
                        job.width,
                        job.height,
                        job.stride,
                        job.prefs.resolution,
                        job.prefs.format,
                        &job.dir,
                    )
                    .map_or_else(|e| PhotoOutcome::Failed(e.to_string()), PhotoOutcome::Saved);
                    on_done(outcome);
                }
            })
            .map_or_else(
                |e| tracing::error!("could not spawn photo worker: {e}"),
                |_| (),
            );
        Self { jobs: tx }
    }

    /// Queues a photo save (non-blocking). Cheap: the frame's pixel
    /// buffer is refcounted. Returns [`TakeRejected::Busy`] when the
    /// queue is full — the shot is dropped, never deferred silently.
    ///
    /// # Errors
    /// Returns [`TakeRejected`] when the job could not be queued.
    pub fn take(
        &self,
        frame: &Frame,
        prefs: &PhotoConfig,
        dir: PathBuf,
    ) -> Result<(), TakeRejected> {
        let job = PhotoJob {
            data: frame.data.clone(),
            width: frame.width,
            height: frame.height,
            stride: frame.stride,
            prefs: prefs.clone(),
            dir,
        };
        match self.jobs.try_send(job) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                tracing::debug!("photo queue full ({PHOTO_QUEUE_CAPACITY}); shot dropped");
                Err(TakeRejected::Busy)
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                tracing::error!("photo worker is gone; photo dropped");
                Err(TakeRejected::Gone)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use hiview_capture::PixelFormat;

    use super::*;

    fn frame() -> Frame {
        Frame {
            width: 1,
            height: 1,
            stride: 4,
            format: PixelFormat::Rgba,
            data: glib::Bytes::from_owned(vec![0; 4]),
            pts: None,
        }
    }

    /// A shot beyond the 16-deep queue is rejected immediately —
    /// `try_send` never blocks the UI thread.
    #[test]
    fn full_queue_rejects_shot_without_blocking() {
        let (tx, rx) = mpsc::sync_channel::<PhotoJob>(PHOTO_QUEUE_CAPACITY);
        let service = PhotoService { jobs: tx };
        let prefs = PhotoConfig::default();
        let dir = PathBuf::from("/tmp");
        for _ in 0..PHOTO_QUEUE_CAPACITY {
            service
                .take(&frame(), &prefs, dir.clone())
                .unwrap_or_else(|e| panic!("queue should accept: {e:?}"));
        }
        // Receiver alive but never drained → the next shot is rejected.
        assert_eq!(service.take(&frame(), &prefs, dir), Err(TakeRejected::Busy));
        drop(rx);
    }
}
