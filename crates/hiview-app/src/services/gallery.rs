// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Gallery thumbnail worker (docs/ARCHITECTURE.md §6.8, §7). All decode
//! and disk I/O happens on a dedicated thread fed by a **bounded**
//! queue: when it is full the caller keeps the item in its own backlog
//! and retries later — the channel itself never grows without limit.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

use hiview_core::{self, MediaItem, MediaKind};

/// Maximum thumbnail jobs waiting in the worker queue (§7).
const THUMB_QUEUE_CAPACITY: usize = 32;

/// Result of one thumbnail job, reported back to the UI.
pub enum ThumbOutcome {
    /// Thumbnail pixels ready (from cache or freshly generated).
    Ready {
        /// Media file the thumbnail belongs to.
        path: PathBuf,
        /// Pixel dimensions of the thumbnail.
        width: u32,
        /// Pixel dimensions of the thumbnail.
        height: u32,
        /// Tightly packed RGBA8 pixels.
        rgba: Arc<[u8]>,
        /// Clip duration in seconds — videos only.
        duration_secs: Option<u64>,
    },
    /// The file could not be decoded; the tile shows a warning icon.
    Corrupt {
        /// Media file that failed.
        path: PathBuf,
    },
}

impl std::fmt::Debug for ThumbOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready {
                path,
                width,
                height,
                rgba,
                duration_secs,
            } => f
                .debug_struct("Ready")
                .field("path", path)
                .field("width", width)
                .field("height", height)
                .field("rgba_len", &rgba.len())
                .field("duration_secs", duration_secs)
                .finish(),
            Self::Corrupt { path } => f.debug_struct("Corrupt").field("path", path).finish(),
        }
    }
}

/// Why a job could not be queued.
#[derive(Debug, PartialEq, Eq)]
pub enum ThumbRejected {
    /// Queue is full — the caller retries on the next completion.
    Busy,
    /// The worker thread is gone.
    Gone,
}

/// Handle to the background thumbnail worker.
pub struct ThumbService {
    jobs: mpsc::SyncSender<MediaItem>,
}

impl ThumbService {
    /// Spawns the worker thread. `on_done` is invoked from the worker
    /// for every finished job — callers forward it to the Relm4 sender,
    /// which is safe to call cross-thread.
    pub fn new<F>(cache_dir: PathBuf, on_done: F) -> Self
    where
        F: Fn(ThumbOutcome) + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel::<MediaItem>(THUMB_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("hiview-thumb".into())
            .spawn(move || {
                while let Ok(item) = rx.recv() {
                    on_done(generate(&cache_dir, item));
                }
            })
            .map_or_else(
                |e| tracing::error!("could not spawn thumbnail worker: {e}"),
                |_| (),
            );
        Self { jobs: tx }
    }

    /// Queues a thumbnail job (non-blocking). Returns
    /// [`ThumbRejected::Busy`] when the queue is full — the caller keeps
    /// the item in its backlog and retries on the next completion.
    ///
    /// # Errors
    /// Returns [`ThumbRejected`] when the job could not be queued.
    pub fn request(&self, item: &MediaItem) -> Result<(), ThumbRejected> {
        match self.jobs.try_send(item.clone()) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(ThumbRejected::Busy),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                tracing::error!("thumbnail worker is gone");
                Err(ThumbRejected::Gone)
            }
        }
    }
}

/// Runs one thumbnail job on the worker thread: serve the cache when
/// the (path, size, mtime) key hits, otherwise decode/scale and store.
fn generate(cache_dir: &std::path::Path, item: MediaItem) -> ThumbOutcome {
    let (png, _) = hiview_core::cache_files(cache_dir, &item.path, item.size, item.mtime);
    if png.exists()
        && let Ok((width, height, rgba)) = hiview_core::decode_png(&png)
    {
        return ThumbOutcome::Ready {
            path: item.path,
            width,
            height,
            rgba: rgba.into(),
            duration_secs: hiview_core::read_duration_sidecar(&png),
        };
    }

    let generated = match item.kind {
        MediaKind::Photo => hiview_core::photo_thumbnail(&item.path)
            .map(|thumb| (thumb, None))
            .map_err(|e| e.to_string()),
        MediaKind::Video => hiview_capture::probe_video(&item.path)
            .map_err(|e| e.to_string())
            .and_then(|probe| {
                let duration_secs = probe.duration.map(|d| d.as_secs());
                hiview_core::thumb_from_rgba(probe.width, probe.height, probe.rgba)
                    .map(|thumb| (thumb, duration_secs))
                    .ok_or_else(|| "decoded frame did not match its geometry".to_string())
            }),
    };

    match generated {
        Ok((thumb, duration_secs)) => {
            let (width, height) = thumb.dimensions();
            let write = hiview_core::write_thumb_png(
                cache_dir,
                &item.path,
                item.size,
                item.mtime,
                width,
                height,
                thumb.as_raw(),
            );
            if let (Ok(png), Some(secs)) = (&write, duration_secs)
                && let Err(e) = hiview_core::write_duration_sidecar(png, secs)
            {
                tracing::warn!("could not write duration sidecar: {e}");
            }
            ThumbOutcome::Ready {
                path: item.path,
                width,
                height,
                rgba: thumb.into_raw().into(),
                duration_secs,
            }
        }
        Err(err) => {
            tracing::debug!("thumbnail failed for {}: {err}", item.path.display());
            ThumbOutcome::Corrupt { path: item.path }
        }
    }
}
