// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Named calibration profiles (docs/ARCHITECTURE.md §6.9): a drawn
//! reference line over a known distance yields the px/mm ratio every
//! measurement label is converted with. Persisted under
//! `[[calibration]]` in `config.toml`.

use serde::{Deserialize, Serialize};

use crate::view::Point;

use super::shape::distance;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
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
}
