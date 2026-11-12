// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! The in-progress measurement state machines (docs/ARCHITECTURE.md
//! §6.9): fixed points plus a rubber-band cursor preview, per armed
//! tool. All geometry is image-space.

use crate::orientation::Orientation;
use crate::view::{Point, Size};

use super::orient::remap_point;
use super::shape::distance;
use super::{MeasureGesture, MeasureShape, MeasureTool};

/// Below half an image pixel the "measurement" is an accidental
/// click, not geometry — discard it instead of committing `0 px`.
const MIN_MEASURE_PX: f64 = 0.5;

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
            MeasureGesture::Leave => {
                self.cursor = None;
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
                    let (a, b) = (self.points[0], self.points[1]);
                    self.clear();
                    (distance(a, b) >= MIN_MEASURE_PX).then_some(MeasureShape::Line { a, b })
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
                let shape = self.points.first().and_then(|center| {
                    (distance(*center, p) >= MIN_MEASURE_PX).then_some(MeasureShape::Circle {
                        center: *center,
                        edge: p,
                    })
                });
                self.clear();
                shape
            }
            MeasureTool::Calibrate => {
                let shape = self.points.first().and_then(|a| {
                    (distance(*a, p) >= MIN_MEASURE_PX)
                        .then_some(MeasureShape::Line { a: *a, b: p })
                });
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

    /// Remaps stored points and the cursor across an orientation change.
    pub fn remap(&mut self, from: Orientation, to: Orientation, size: Size) {
        for p in &mut self.points {
            *p = remap_point(from, to, *p, size);
        }
        if let Some(c) = self.cursor.as_mut() {
            *c = remap_point(from, to, *c, size);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::angle_deg;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
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
}
