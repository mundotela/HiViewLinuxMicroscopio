// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Error type for `hiview-button` (docs/CONVENTIONS.md §5).

/// Errors produced while opening or running a [`crate::ButtonSource`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Enumerating input devices failed.
    #[error("evdev enumeration: {0}")]
    EvdevEnumerate(std::io::Error),
    /// The chosen input device could not be opened/read.
    #[error("evdev device: {0}")]
    Evdev(std::io::Error),
    /// The source worker thread could not be spawned.
    #[error("thread spawn: {0}")]
    ThreadSpawn(std::io::Error),
}

/// Crate-local result type.
pub type Result<T> = std::result::Result<T, Error>;
