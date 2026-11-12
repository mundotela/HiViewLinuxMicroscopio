// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Measurement tools and calibration (docs/ARCHITECTURE.md §6.9):
//! ruler, angle and circle/diameter over the live image, the in-progress
//! draft state machine, named calibration profiles and the µm/mm/±
//! formatting rules.
//!
//! All geometry lives in **image coordinates** — the viewport maps
//! pointer events through `ViewTransform`, so measurements follow zoom,
//! pan and freeze for free. Orientation changes (mirror, ±90°) remap the
//! stored points through [`remap_point`] so they stay glued to the
//! measured feature.
//!
//! Pure logic — no GTK/GStreamer — unit tested.

mod calibration;
mod format;
mod orient;
mod session;
mod set;
mod shape;
mod view_tests;

pub use calibration::CalibrationProfile;
pub use format::{format_angle_deg, format_length_px};
pub use orient::{remap_point, remapped_size};
pub use session::{DraftView, MeasureSession};
pub use set::{MeasureCommit, MeasureSet};
pub use shape::{MeasureShape, Measurement, angle_deg, distance};

use crate::view::Point;

/// Which measuring interaction the viewport should run next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureTool {
    /// Ruler: two clicks — the distance between them.
    Ruler,
    /// Angle: three clicks — the middle point is the vertex.
    Angle,
    /// Circle: press the center, drag to the edge — shows the diameter.
    Circle,
    /// Calibration line: press-drag-release over a known distance, then
    /// the wizard asks for the real millimetres.
    Calibrate,
}

/// Raw pointer events the viewport forwards while a tool is armed —
/// already converted to image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeasureGesture {
    /// Left-button press point (ruler and angle tools).
    Click(Point),
    /// Left-button press starting a drag (circle, calibration).
    DragStart(Point),
    /// Left-button drag position (circle, calibration) — preview only.
    DragUpdate(Point),
    /// Release point — commits the circle/calibration line.
    DragEnd(Point),
    /// Plain cursor movement while a tool is armed (rubber-band preview).
    Hover(Point),
    /// The cursor left the viewport — drop the rubber-band preview.
    Leave,
}
