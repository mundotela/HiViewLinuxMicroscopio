// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Pure domain logic for HiView: configuration, calibration, view transforms,
//! frame processing, file naming and the UVC status packet parser.
//!
//! This crate must not depend on GTK or GStreamer (docs/ARCHITECTURE.md §5).

mod devices;
mod orientation;
mod view;

pub use devices::{KNOWN_MICROSCOPES, KnownMicroscope, is_known_microscope};
pub use orientation::{Orientation, ViewOrientation};
pub use view::{MAX_SCALE, MIN_SCALE, NEAREST_FILTER_SCALE, Point, Size, ViewMode, ViewTransform};
