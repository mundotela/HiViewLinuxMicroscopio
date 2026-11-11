// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Pure domain logic for HiView: configuration, calibration, view transforms,
//! frame processing, file naming and the UVC status packet parser.
//!
//! This crate must not depend on GTK or GStreamer (docs/ARCHITECTURE.md §5).

mod config;
mod devices;
mod error;
mod imaging;
mod orientation;
mod view;

pub use config::{AppConfig, ConfigLoad, PhotoConfig, config_path, load_config, save_config};
pub use devices::{KNOWN_MICROSCOPES, KnownMicroscope, is_known_microscope};
pub use error::{Error, Result};
pub use imaging::{
    JPEG_QUALITY, PhotoFormat, PhotoResolution, encode_photo, photo_filename, rgba_image,
    save_photo, unique_photo_path,
};
pub use orientation::{Orientation, ViewOrientation};
pub use view::{MAX_SCALE, MIN_SCALE, NEAREST_FILTER_SCALE, Point, Size, ViewMode, ViewTransform};
