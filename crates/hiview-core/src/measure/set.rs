// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! The app's measurement state (docs/ARCHITECTURE.md §6.9): the armed
//! tool's session plus every committed measurement, in image
//! coordinates. Measurements are runtime state — never persisted.

use crate::orientation::Orientation;
use crate::view::{Point, Size};

use super::session::MeasureSession;
use super::{MeasureGesture, MeasureShape, MeasureTool, Measurement};

/// The app's measurement state: the armed tool's session plus every
/// committed measurement, in image coordinates. Measurements are
/// runtime state — they are never persisted (§6.9).
#[derive(Debug, Clone, Default)]
pub struct MeasureSet {
    /// Finished measurements on screen, oldest first.
    pub items: Vec<Measurement>,
    /// The armed tool's in-progress session, if any.
    pub session: Option<MeasureSession>,
    next_id: u64,
}

/// What a gesture produced — the app reacts to commits (circle ends a
/// drag; the calibration line opens the wizard).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeasureCommit {
    /// Nothing finished yet (a partial click, a hover).
    Nothing,
    /// A measurement shape was completed.
    Shape(MeasureShape),
    /// The calibration reference line was drawn — open the wizard.
    CalibrationLine { a: Point, b: Point },
}

impl MeasureSet {
    /// Arms `tool` (`None` disarms). Arming or switching tools drops
    /// any in-progress points — a half-finished ruler must not leak
    /// into the next tool.
    pub fn arm(&mut self, tool: Option<MeasureTool>) {
        self.session = tool.map(MeasureSession::new);
    }

    /// The armed tool, if any.
    #[must_use]
    pub fn armed_tool(&self) -> Option<MeasureTool> {
        self.session.as_ref().map(MeasureSession::tool)
    }

    /// Feeds one gesture into the armed session.
    pub fn gesture(&mut self, g: MeasureGesture) -> MeasureCommit {
        let Some(session) = self.session.as_mut() else {
            return MeasureCommit::Nothing;
        };
        match session.gesture(g) {
            Some(MeasureShape::Line { a, b }) if session.tool() == MeasureTool::Calibrate => {
                MeasureCommit::CalibrationLine { a, b }
            }
            Some(shape) => {
                let id = self.next_id;
                self.next_id += 1;
                let tool = session.tool();
                self.items.push(Measurement { id, tool, shape });
                MeasureCommit::Shape(shape)
            }
            None => MeasureCommit::Nothing,
        }
    }

    /// Cancels the in-progress points (`Esc`); the tool stays armed.
    /// Returns `true` when there was something to cancel.
    pub fn cancel(&mut self) -> bool {
        self.session.as_mut().is_some_and(|s| {
            let had = s.in_progress();
            s.clear();
            had
        })
    }

    /// Removes one measurement by id (`true` when it existed).
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|m| m.id != id);
        self.items.len() != before
    }

    /// "Limpar medições" — drops every committed measurement.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Remaps every stored point across an orientation change so
    /// measurements stay glued to the measured feature.
    pub fn remap(&mut self, from: Orientation, to: Orientation, size: Size) {
        for m in &mut self.items {
            m.shape = m.shape.remapped(from, to, size);
        }
        if let Some(session) = self.session.as_mut() {
            session.remap(from, to, size);
        }
    }

    /// The render snapshot for [`crate::overlay::MeasureOverlay`].
    #[must_use]
    pub fn overlay(&self, px_per_mm: Option<f64>) -> crate::overlay::MeasureOverlay {
        crate::overlay::MeasureOverlay {
            items: self.items.clone(),
            draft: self.session.as_ref().map(MeasureSession::view),
            px_per_mm,
        }
    }
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
    fn measure_set_commits_and_ids() {
        let mut set = MeasureSet::default();
        set.arm(Some(MeasureTool::Ruler));
        assert_eq!(
            set.gesture(MeasureGesture::Click(p(0.0, 0.0))),
            MeasureCommit::Nothing
        );
        assert!(matches!(
            set.gesture(MeasureGesture::Click(p(10.0, 0.0))),
            MeasureCommit::Shape(_)
        ));
        assert_eq!(set.items.len(), 1);
        // Second measurement gets a different, higher id.
        set.gesture(MeasureGesture::Click(p(0.0, 0.0)));
        set.gesture(MeasureGesture::Click(p(5.0, 0.0)));
        assert_eq!(set.items.len(), 2);
        assert!(set.items[1].id > set.items[0].id);
        assert!(set.remove(set.items[0].id));
        assert_eq!(set.items.len(), 1);
        assert!(!set.remove(999));
        set.clear();
        assert!(set.items.is_empty());
    }

    #[test]
    fn measure_set_calibration_line_is_not_a_measurement() {
        let mut set = MeasureSet::default();
        set.arm(Some(MeasureTool::Calibrate));
        set.gesture(MeasureGesture::DragStart(p(1.0, 1.0)));
        let commit = set.gesture(MeasureGesture::DragEnd(p(11.0, 1.0)));
        assert_eq!(
            commit,
            MeasureCommit::CalibrationLine {
                a: p(1.0, 1.0),
                b: p(11.0, 1.0)
            }
        );
        assert!(set.items.is_empty());
    }

    #[test]
    fn measure_set_remap_covers_items_and_session() {
        let mut set = MeasureSet::default();
        set.arm(Some(MeasureTool::Ruler));
        set.gesture(MeasureGesture::Click(p(10.0, 10.0)));
        set.gesture(MeasureGesture::Click(p(110.0, 10.0)));
        set.gesture(MeasureGesture::Click(p(20.0, 20.0)));
        set.remap(Orientation::Identity, Orientation::FlipH, F);
        let MeasureShape::Line { a, b } = set.items[0].shape else {
            panic!("line")
        };
        assert_eq!((a, b), (p(630.0, 10.0), p(530.0, 10.0)));
        assert_eq!(
            set.session.map(|s| s.view().points[0]),
            Some(p(620.0, 20.0))
        );
    }

    #[test]
    fn cancel_keeps_the_tool_armed() {
        let mut set = MeasureSet::default();
        set.arm(Some(MeasureTool::Angle));
        set.gesture(MeasureGesture::Click(p(1.0, 1.0)));
        assert!(set.cancel());
        assert_eq!(set.armed_tool(), Some(MeasureTool::Angle));
        assert!(!set.cancel());
    }
}
