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
    jobs: mpsc::Sender<PhotoJob>,
}

impl PhotoService {
    /// Spawns the worker thread. `on_done` is called for every finished
    /// job (from the worker thread — callers forward it to the Relm4
    /// sender, which is safe to call cross-thread).
    pub fn new<F>(on_done: F) -> Self
    where
        F: Fn(PhotoOutcome) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<PhotoJob>();
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

    /// Queues a photo save. Cheap: the frame's pixel buffer is refcounted.
    pub fn take(&self, frame: &Frame, prefs: &PhotoConfig, dir: PathBuf) {
        let job = PhotoJob {
            data: frame.data.clone(),
            width: frame.width,
            height: frame.height,
            stride: frame.stride,
            prefs: prefs.clone(),
            dir,
        };
        if self.jobs.send(job).is_err() {
            tracing::error!("photo worker is gone; photo dropped");
        }
    }
}
