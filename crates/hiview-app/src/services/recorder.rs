// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! App-side glue for video recording (docs/ARCHITECTURE.md §6.7): the
//! free-space probe used by the disk gate and the state carried while a
//! recording is active or being finalized.

use std::path::Path;
use std::time::Instant;

use hiview_capture::Recorder;
use relm4::gtk::gio;
use relm4::gtk::prelude::*;

/// Free bytes on the filesystem containing `dir` — the probe injected
/// into `hiview_capture::check_disk_space`. Probing the *parent* XDG
/// directory keeps the check working before `HiView/` exists.
#[must_use]
pub fn filesystem_free_bytes(dir: &Path) -> Option<u64> {
    gio::File::for_path(dir)
        .query_filesystem_info(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE, gio::Cancellable::NONE)
        .ok()
        .map(|info| info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE))
}

/// Why the current recording is being finalized (drives the right toast).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// User pressed `R` or the stop button.
    User,
    /// The camera disappeared mid-recording.
    Disconnected,
    /// The record pipeline reported a fatal error.
    Failed,
    /// The window is closing; quit follows the finalize.
    Close,
}

/// A recording in flight: the recorder plus bookkeeping for the UI.
pub struct ActiveRecording {
    /// The recorder handle; `stop()` finalizes the MP4.
    pub recorder: Recorder,
    /// When recording began — drives the elapsed-time indicator.
    pub started: Instant,
}
