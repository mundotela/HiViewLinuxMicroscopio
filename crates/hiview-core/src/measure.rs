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
//! stored points via [`OrientationOp`] so they stay glued to the
//! measured feature.
//!
//! Pure logic — no GTK/GStreamer — unit tested.

use serde::{Deserialize, Serialize};

use crate::view::{Point, Size};

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
                    "⌀ {}",
                    format_length_px(2.0 * distance(center, edge), px_per_mm)
                )
            }
            Self::Angle { a, vertex, b } => format_angle_deg(angle_deg(a, vertex, b)),
        }
    }
}

/// A committed measurement — stays on screen until the user removes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// Stable id for per-item delete (monotonic within the session).
    pub id: u64,
    /// The geometry, in image coordinates.
    pub shape: MeasureShape,
}

/// Raw pointer events the viewport forwards while a tool is armed —
/// already converted to image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeasureGesture {
    /// Left-button press point (ruler and angle tools).
    Click(Point),
    /// Left-button press starting a drag (circle, calibration).
    DragStart(Point),
    /// Live position during the drag — preview only.
    DragUpdate(Point),
    /// Release point — commits the circle/calibration line.
    DragEnd(Point),
    /// Plain cursor movement while a tool is armed (rubber-band preview).
    Hover(Point),
}

/// The in-progress measurement: which clicks already landed and where
/// the cursor is. One exists per armed tool; the app keeps it, the
/// renderer draws [`MeasureSession::view`].
#[derive(Debug, Clone)]
pub struct MeasureSession {
    tool: MeasureTool,
    /// Committed clicks of the current measurement (0–3).
    points: Vec<Point>,
    /// Current pointer position for the rubber-band preview.
    cursor: Option<Point>,
}

impl MeasureSession {
    /// Starts a session for `tool` with no points.
    #[must_use]
    pub fn new(tool: MeasureTool) -> Self {
        Self {
            tool,
            points: Vec::new(),
            cursor: None,
        }
    }

    /// The tool this session was armed for.
    #[must_use]
    pub fn tool(&self) -> MeasureTool {
        self.tool
    }

    /// Feeds one gesture; returns the finished shape on commit.
    pub fn gesture(&mut self, g: MeasureGesture) -> Option<MeasureShape> {
        match g {
            MeasureGesture::Hover(p) => {
                self.cursor = Some(p);
                None
            }
            MeasureGesture::Click(p) => self.click(p),
            MeasureGesture::DragStart(p) => {
                self.points = vec![p];
                self.cursor = Some(p);
                None
            }
            MeasureGesture::DragUpdate(p) => {
                self.cursor = Some(p);
                None
            }
            MeasureGesture::DragEnd(p) => self.drag_end(p),
        }
    }

    /// Ruler/angle click accumulation.
    fn click(&mut self, p: Point) -> Option<MeasureShape> {
        match self.tool {
            MeasureTool::Ruler => {
                self.points.push(p);
                if self.points.len() == 2 {
                    let shape = MeasureShape::Line {
                        a: self.points[0],
                        b: self.points[1],
                    };
                    self.clear();
                    Some(shape)
                } else {
                    None
                }
            }
            MeasureTool::Angle => {
                self.points.push(p);
                if self.points.len() == 3 {
                    let shape = MeasureShape::Angle {
                        a: self.points[0],
                        vertex: self.points[1],
                        b: self.points[2],
                    };
                    self.clear();
                    Some(shape)
                } else {
                    None
                }
            }
            // Circle/Calibrate draw by drag — stray clicks are ignored.
            MeasureTool::Circle | MeasureTool::Calibrate => None,
        }
    }

    /// Circle/calibration commit on release.
    fn drag_end(&mut self, p: Point) -> Option<MeasureShape> {
        match self.tool {
            MeasureTool::Circle => {
                let shape = self.points.first().map(|center| MeasureShape::Circle {
                    center: *center,
                    edge: p,
                });
                self.clear();
                shape
            }
            MeasureTool::Calibrate => {
                let shape = self
                    .points
                    .first()
                    .map(|a| MeasureShape::Line { a: *a, b: p });
                self.clear();
                shape
            }
            MeasureTool::Ruler | MeasureTool::Angle => None,
        }
    }

    /// Whether there is anything in progress to cancel (`Esc`).
    #[must_use]
    pub fn in_progress(&self) -> bool {
        !self.points.is_empty()
    }

    /// Drops the in-progress points; the tool stays armed.
    pub fn clear(&mut self) {
        self.points.clear();
        self.cursor = None;
    }

    /// What the renderer draws as the draft: fixed points plus the
    /// rubber-band extension to the cursor.
    #[must_use]
    pub fn view(&self) -> DraftView {
        DraftView {
            tool: self.tool,
            points: self.points.clone(),
            cursor: self.cursor,
        }
    }
}

/// The renderer-facing snapshot of a [`MeasureSession`].
#[derive(Debug, Clone, PartialEq)]
pub struct DraftView {
    /// The armed tool (drives the preview shape and color).
    pub tool: MeasureTool,
    /// Points already clicked/dragged.
    pub points: Vec<Point>,
    /// Live cursor for the rubber band.
    pub cursor: Option<Point>,
}

impl DraftView {
    /// The shape the draft would commit right now (with the cursor as
    /// the pending point), used to draw the live preview label.
    #[must_use]
    pub fn preview_shape(&self) -> Option<MeasureShape> {
        let p = self.points.as_slice();
        let c = self.cursor;
        match self.tool {
            MeasureTool::Ruler | MeasureTool::Calibrate => match (p, c) {
                ([a], Some(b)) => Some(MeasureShape::Line { a: *a, b }),
                ([a, b], _) => Some(MeasureShape::Line { a: *a, b: *b }),
                _ => None,
            },
            MeasureTool::Angle => match (p, c) {
                ([a, v], Some(b)) => Some(MeasureShape::Angle {
                    a: *a,
                    vertex: *v,
                    b,
                }),
                ([a, v, b], _) => Some(MeasureShape::Angle {
                    a: *a,
                    vertex: *v,
                    b: *b,
                }),
                _ => None,
            },
            MeasureTool::Circle => match (p, c) {
                ([center], Some(edge)) => Some(MeasureShape::Circle {
                    center: *center,
                    edge,
                }),
                _ => None,
            },
        }
    }
}

/// One orientation change applied to stored image-space points so
/// measurements stay glued to the feature (`H`, `V`, `[`, `]`).
///
/// `size` is the frame size **before** the operation; the rotated
/// variants swap it (640×480 → 480×640).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrientationOp {
    /// Horizontal mirror (`H`).
    MirrorH,
    /// Vertical mirror (`V`).
    MirrorV,
    /// 90° clockwise (`]`).
    RotateCw,
    /// 90° counterclockwise (`[`).
    RotateCcw,
}

/// Maps a displayed-image point into the post-operation frame's
/// coordinates. Continuous image space: pixel `i` spans `[i, i+1)`.
#[must_use]
pub fn map_point(op: OrientationOp, p: Point, size: Size) -> Point {
    match op {
        OrientationOp::MirrorH => Point::new(size.width - p.x, p.y),
        OrientationOp::MirrorV => Point::new(p.x, size.height - p.y),
        OrientationOp::RotateCw => Point::new(size.height - p.y, p.x),
        OrientationOp::RotateCcw => Point::new(p.y, size.width - p.x),
    }
}

/// The frame size after `op` — rotations swap the axes.
#[must_use]
pub fn mapped_size(op: OrientationOp, size: Size) -> Size {
    match op {
        OrientationOp::RotateCw | OrientationOp::RotateCcw => Size::new(size.height, size.width),
        OrientationOp::MirrorH | OrientationOp::MirrorV => size,
    }
}

impl MeasureShape {
    /// Remaps every stored point through `op`.
    #[must_use]
    pub fn mapped(&self, op: OrientationOp, size: Size) -> Self {
        let mut out = *self;
        for p in out.points_mut() {
            *p = map_point(op, *p, size);
        }
        out
    }
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

/// A named calibration: how many image pixels cover one millimetre in
/// this ring/height setup (§6.9). Persisted under `[[calibration]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalibrationProfile {
    /// Display name, unique among profiles.
    pub name: String,
    /// Image pixels per millimetre.
    pub px_per_mm: f64,
}

impl Default for CalibrationProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            px_per_mm: 1.0,
        }
    }
}

impl CalibrationProfile {
    /// Builds a profile from a drawn line over `real_mm` millimetres.
    /// Returns `None` for zero-length lines or non-positive distances —
    /// the wizard only accepts a real, positive reference.
    #[must_use]
    pub fn from_line(a: Point, b: Point, real_mm: f64, name: String) -> Option<Self> {
        let px = distance(a, b);
        if px <= f64::EPSILON || !real_mm.is_finite() || real_mm <= 0.0 {
            return None;
        }
        Some(Self {
            name,
            px_per_mm: px / real_mm,
        })
    }

    /// The displayed uncertainty `± X`: one image pixel converted by
    /// this calibration, in millimetres.
    #[must_use]
    pub fn uncertainty_mm(&self) -> f64 {
        1.0 / self.px_per_mm
    }
}

/// PT-BR decimal comma ("1,25") — the UI locale.
fn pt(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

/// Formats a length in image pixels for display (§6.9):
/// - no calibration → `"123 px"` (whole pixels, no ±);
/// - ≥ 1 mm → `"1,25 mm ± 0,01"`;
/// - < 1 mm → `"523 µm ± 11"`.
///
/// `± X` is one image pixel converted by the active calibration.
#[must_use]
pub fn format_length_px(px: f64, px_per_mm: Option<f64>) -> String {
    let Some(ppm) = px_per_mm.filter(|p| p.is_finite() && *p > 0.0) else {
        return format!("{:.0} px", px.round());
    };
    let mm = px / ppm;
    let unc_mm = 1.0 / ppm;
    if mm >= 1.0 {
        format!("{} mm ± {}", pt(mm, 2), pt(unc_mm, 2))
    } else {
        format!("{} µm ± {}", pt(mm * 1000.0, 0), pt(unc_mm * 1000.0, 0))
    }
}

/// Formats an angle for display: one decimal, degrees (`"45,3°"`).
#[must_use]
pub fn format_angle_deg(deg: f64) -> String {
    format!("{}°", pt(deg, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: Size = Size {
        width: 640.0,
        height: 480.0,
    };

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
    fn ruler_session_commits_on_second_click() {
        let mut s = MeasureSession::new(MeasureTool::Ruler);
        assert!(s.gesture(MeasureGesture::Click(p(10.0, 10.0))).is_none());
        assert!(s.in_progress());
        let shape = s.gesture(MeasureGesture::Click(p(110.0, 10.0)));
        assert_eq!(
            shape,
            Some(MeasureShape::Line {
                a: p(10.0, 10.0),
                b: p(110.0, 10.0)
            })
        );
        assert!(!s.in_progress());
    }

    #[test]
    fn angle_session_middle_click_is_vertex() {
        let mut s = MeasureSession::new(MeasureTool::Angle);
        s.gesture(MeasureGesture::Click(p(0.0, 100.0)));
        s.gesture(MeasureGesture::Click(p(100.0, 100.0)));
        let shape = s.gesture(MeasureGesture::Click(p(100.0, 0.0)));
        let Some(MeasureShape::Angle { a, vertex, b }) = shape else {
            panic!("expected Angle")
        };
        assert_eq!(vertex, p(100.0, 100.0));
        assert_eq!(a, p(0.0, 100.0));
        assert_eq!(b, p(100.0, 0.0));
        assert!((angle_deg(a, vertex, b) - 90.0).abs() < 1e-6);
    }

    #[test]
    fn circle_session_commits_diameter_on_release() {
        let mut s = MeasureSession::new(MeasureTool::Circle);
        assert!(
            s.gesture(MeasureGesture::DragStart(p(100.0, 100.0)))
                .is_none()
        );
        s.gesture(MeasureGesture::DragUpdate(p(140.0, 100.0)));
        let shape = s.gesture(MeasureGesture::DragEnd(p(150.0, 100.0)));
        assert_eq!(
            shape,
            Some(MeasureShape::Circle {
                center: p(100.0, 100.0),
                edge: p(150.0, 100.0)
            })
        );
        // Radius 50 px → the label shows 100 px of diameter.
        assert_eq!(
            shape.unwrap_or_else(|| panic!("shape")).label(None),
            "⌀ 100 px"
        );
    }

    #[test]
    fn calibrate_session_commits_the_reference_line() {
        let mut s = MeasureSession::new(MeasureTool::Calibrate);
        s.gesture(MeasureGesture::DragStart(p(50.0, 60.0)));
        let shape = s.gesture(MeasureGesture::DragEnd(p(139.0, 60.0)));
        assert_eq!(
            shape,
            Some(MeasureShape::Line {
                a: p(50.0, 60.0),
                b: p(139.0, 60.0)
            })
        );
        // …and stray clicks are ignored for drag tools.
        assert_eq!(s.gesture(MeasureGesture::Click(p(1.0, 1.0))), None);
    }

    #[test]
    fn hover_feeds_the_rubber_band_preview() {
        let mut s = MeasureSession::new(MeasureTool::Ruler);
        s.gesture(MeasureGesture::Click(p(10.0, 10.0)));
        s.gesture(MeasureGesture::Hover(p(60.0, 10.0)));
        let draft = s.view();
        assert_eq!(
            draft.preview_shape(),
            Some(MeasureShape::Line {
                a: p(10.0, 10.0),
                b: p(60.0, 10.0)
            })
        );
    }

    #[test]
    fn clear_drops_the_in_progress_points() {
        let mut s = MeasureSession::new(MeasureTool::Angle);
        s.gesture(MeasureGesture::Click(p(1.0, 1.0)));
        s.gesture(MeasureGesture::Click(p(2.0, 2.0)));
        assert!(s.in_progress());
        s.clear();
        assert!(!s.in_progress());
        // A third click now starts a fresh measurement, not a commit.
        s.gesture(MeasureGesture::Click(p(3.0, 3.0)));
        assert_eq!(s.view().points.len(), 1);
    }

    #[test]
    fn calibration_from_line_and_uncertainty() {
        // 89 px over 1,0 mm — the fixture's measured scale (§8 README).
        let p_ = CalibrationProfile::from_line(p(0.0, 0.0), p(89.0, 0.0), 1.0, "slide".into())
            .unwrap_or_else(|| panic!("valid line"));
        assert!((p_.px_per_mm - 89.0).abs() < 1e-9);
        // ±1 px = 1/89 mm ≈ 11 µm.
        assert!((p_.uncertainty_mm() - 1.0 / 89.0).abs() < 1e-9);
        assert!((p_.uncertainty_mm() * 1000.0 - 11.24).abs() < 0.01);
        // Diagonal lines measure the same scale.
        let d = CalibrationProfile::from_line(p(0.0, 0.0), p(63.0, 63.0), 1.0, "d".into())
            .unwrap_or_else(|| panic!("valid line"));
        assert!((d.px_per_mm - 89.095).abs() < 0.01);
    }

    #[test]
    fn calibration_rejects_degenerate_input() {
        assert!(CalibrationProfile::from_line(p(1.0, 1.0), p(1.0, 1.0), 1.0, "x".into()).is_none());
        assert!(CalibrationProfile::from_line(p(0.0, 0.0), p(5.0, 0.0), 0.0, "x".into()).is_none());
        assert!(
            CalibrationProfile::from_line(p(0.0, 0.0), p(5.0, 0.0), -2.0, "x".into()).is_none()
        );
    }

    #[test]
    fn length_formatting_rules() {
        // No calibration → plain pixels.
        assert_eq!(format_length_px(123.4, None), "123 px");
        assert_eq!(format_length_px(123.4, Some(0.0)), "123 px");
        // ≥ 1 mm → two decimals, mm, ± one pixel in mm.
        assert_eq!(format_length_px(111.25, Some(89.0)), "1,25 mm ± 0,01");
        assert_eq!(format_length_px(89.0, Some(89.0)), "1,00 mm ± 0,01");
        // < 1 mm → whole µm, ± one pixel in µm.
        assert_eq!(format_length_px(44.5, Some(89.0)), "500 µm ± 11");
        assert_eq!(format_length_px(2.0, Some(89.0)), "22 µm ± 11");
    }

    #[test]
    fn angle_formatting_one_decimal() {
        assert_eq!(format_angle_deg(45.34), "45,3°");
        assert_eq!(format_angle_deg(90.0), "90,0°");
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
        assert_eq!(circle.label(Some(89.0)), "⌀ 500 µm ± 11");
    }

    #[test]
    fn mirror_maps_points_inside_the_same_frame() {
        // Mirroring keeps the frame size; a point at x=100 of 640 lands
        // on the mirrored feature at x=540.
        let line = MeasureShape::Line {
            a: p(100.0, 200.0),
            b: p(200.0, 200.0),
        };
        let m = line.mapped(OrientationOp::MirrorH, F);
        assert_eq!(
            m,
            MeasureShape::Line {
                a: p(540.0, 200.0),
                b: p(440.0, 200.0)
            }
        );
        // Length is mirror-invariant.
        let MeasureShape::Line { a, b } = m else {
            panic!("line")
        };
        assert_eq!(distance(a, b), 100.0);
    }

    #[test]
    fn rotation_maps_points_into_the_swapped_frame() {
        // CW on 640×480 → 480×640: (x,y) → (480−y, x).
        let r = MeasureShape::Line {
            a: p(100.0, 100.0),
            b: p(200.0, 100.0),
        };
        let m = r.mapped(OrientationOp::RotateCw, F);
        assert_eq!(
            mapped_size(OrientationOp::RotateCw, F),
            Size::new(480.0, 640.0)
        );
        assert_eq!(
            m,
            MeasureShape::Line {
                a: p(380.0, 100.0),
                b: p(380.0, 200.0)
            }
        );
        // CCW: (x,y) → (y, 640−x).
        let m2 = r.mapped(OrientationOp::RotateCcw, F);
        assert_eq!(
            m2,
            MeasureShape::Line {
                a: p(100.0, 540.0),
                b: p(100.0, 440.0)
            }
        );
    }
}
