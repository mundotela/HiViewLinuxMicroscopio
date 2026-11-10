// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Pure domain logic for HiView: configuration, calibration, view transforms,
//! frame processing, file naming and the UVC status packet parser.
//!
//! This crate must not depend on GTK or GStreamer (docs/ARCHITECTURE.md §5).

mod devices;

pub use devices::{KNOWN_MICROSCOPES, KnownMicroscope, is_known_microscope};
