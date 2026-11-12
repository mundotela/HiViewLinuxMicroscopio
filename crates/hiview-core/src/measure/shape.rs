// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Measurement shapes and the pure geometry they are built from
//! (docs/ARCHITECTURE.md §6.9). All coordinates are image-space.

use crate::orientation::Orientation;
use crate::view::{Point, Size};

use super::MeasureTool;
use super::format::{format_angle_deg, format_length_px};
use super::orient::remap_point;

/// A finished (or in-progress) shape, in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeasureShape {
    /// Ruler: a segment from `a` to `b`.
    Line {
        /// First endpoint.
        a: Point,
        /// Second endpoint.
        b: Point,
    },
    /// Angle: two segments `vertex → a` and `vertex → b`.
    Angle {
        /// End of the first arm.
        a: Point,
        /// Middle click — the vertex the angle is measured at.
        vertex: Point,
        /// End of the second arm.
        b: Point,
    },
    /// Circle: `center` to `edge` is the radius — the label shows the
    /// diameter (§6.9).
    Circle {
        /// Center click.
        center: Point,
        /// Edge point (drag release).
        edge: Point,
    },
}

impl MeasureShape {
    /// Every stored point, for remapping on orientation changes.
    fn points_mut(&mut self) -> Vec<&mut Point> {
        match self {
            Self::Line { a, b } => vec![a, b],
            Self::Angle { a, vertex, b } => vec![a, vertex, b],
            Self::Circle { center, edge } => vec![center, edge],
        }
    }

    /// The measurement label for the overlay/panel, honoring the active
    /// calibration (`None` → image pixels).
    #[must_use]
    pub fn label(&self, px_per_mm: Option<f64>) -> String {
        match *self {
            Self::Line { a, b } => format_length_px(distance(a, b), px_per_mm),
            Self::Circle { center, edge } => {
                format!(
                    "Ø {}",
                    format_length_px(2.0 * distance(center, edge), px_per_mm)
                )
            }
            Self::Angle { a, vertex, b } => format_angle_deg(angle_deg(a, vertex, b)),
        }
    }
}

impl MeasureShape {
    /// Remaps every stored point through an orientation change.
    #[must_use]
    pub fn remapped(&self, from: Orientation, to: Orientation, size: Size) -> Self {
        let mut out = *self;
        for p in out.points_mut() {
            *p = remap_point(from, to, *p, size);
        }
        out
    }
}
/// A committed measurement — stays on screen until the user removes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// Stable id for per-item delete (monotonic within the session).
    pub id: u64,
    /// The tool that created it (ruler/angle/circle) — the list label.
    pub tool: MeasureTool,
    /// The geometry, in image coordinates.
    pub shape: MeasureShape,
}

/// Euclidean distance in image pixels.
#[must_use]
pub fn distance(a: Point, b: Point) -> f64 {
    (b.x - a.x).hypot(b.y - a.y)
}

/// The angle at `vertex` between the rays to `a` and `b`, in degrees
/// `0..=180`. Degenerate (zero-length) arms yield `0`.
#[must_use]
pub fn angle_deg(a: Point, vertex: Point, b: Point) -> f64 {
    let ux = a.x - vertex.x;
    let uy = a.y - vertex.y;
    let vx = b.x - vertex.x;
    let vy = b.y - vertex.y;
    let denom = ux.hypot(uy) * vx.hypot(vy);
    if denom <= f64::EPSILON {
        return 0.0;
    }
    ((ux * vx + uy * vy) / denom)
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }
    #[test]
    fn distance_is_pythagorean() {
        assert!((distance(p(0.0, 0.0), p(3.0, 4.0)) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn angle_is_right_for_perpendicular_arms() {
        let deg = angle_deg(p(1.0, 0.0), p(0.0, 0.0), p(0.0, -1.0));
        assert!((deg - 90.0).abs() < 1e-6);
    }

    #[test]
    fn angle_covers_full_range() {
        assert!((angle_deg(p(1.0, 0.0), p(0.0, 0.0), p(-1.0, 0.0)) - 180.0).abs() < 1e-6);
        assert!((angle_deg(p(1.0, 0.0), p(0.0, 0.0), p(1.0, 0.0))).abs() < 1e-6);
        // ~45° for a diagonal arm.
        let deg = angle_deg(p(1.0, 0.0), p(0.0, 0.0), p(1.0, 1.0));
        assert!((deg - 45.0).abs() < 1e-6);
    }

    #[test]
    fn angle_zero_arm_is_zero() {
        assert_eq!(angle_deg(p(0.0, 0.0), p(0.0, 0.0), p(1.0, 1.0)), 0.0);
    }

    #[test]
    fn shape_labels_follow_the_calibration() {
        let line = MeasureShape::Line {
            a: p(0.0, 0.0),
            b: p(44.5, 0.0),
        };
        assert_eq!(line.label(None), "45 px");
        assert_eq!(line.label(Some(89.0)), "500 µm ± 11");
        let circle = MeasureShape::Circle {
            center: p(0.0, 0.0),
            edge: p(22.25, 0.0),
        };
        assert_eq!(circle.label(Some(89.0)), "Ø 500 µm ± 11");
    }
}
