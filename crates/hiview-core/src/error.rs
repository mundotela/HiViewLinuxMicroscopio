// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Crate-wide error type (docs/CONVENTIONS.md §5).

/// Errors produced by `hiview-core` operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Image encoding or decoding failure.
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    /// File-system failure.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML (de)serialization failure.
    #[error("config error: {0}")]
    Toml(#[from] toml::ser::Error),

    /// A frame buffer too short for its declared geometry.
    #[error("frame buffer too short: {len} bytes for {width}x{height} stride {stride}")]
    BadFrame {
        /// Declared frame width in pixels.
        width: u32,
        /// Declared frame height in pixels.
        height: u32,
        /// Declared row stride in bytes.
        stride: usize,
        /// Actual buffer length.
        len: usize,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;
