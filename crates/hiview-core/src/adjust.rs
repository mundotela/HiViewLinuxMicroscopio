// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Software image adjustments (docs/ARCHITECTURE.md §6.11): brightness,
//! contrast, saturation, gamma and sharpen applied to each RGBA frame
//! in the appsink callback — see [`crate::image_adjust`] — so the live
//! view, photos and videos all show them.
//!
//! Values are stored per device under `[devices."VID:PID"]` in
//! `config.toml` and re-applied on connect. Pure logic — no GTK/GStreamer —
//! unit tested.

use serde::{Deserialize, Serialize};

/// Brightness offset bounds (§6.11).
pub const BRIGHTNESS_RANGE: (f64, f64) = (-1.0, 1.0);
/// Contrast multiplier bounds (§6.11).
pub const CONTRAST_RANGE: (f64, f64) = (0.0, 2.0);
/// Saturation multiplier bounds (§6.11).
pub const SATURATION_RANGE: (f64, f64) = (0.0, 2.0);
/// Gamma exponent bounds (§6.11).
pub const GAMMA_RANGE: (f64, f64) = (0.3, 3.0);
/// Sharpen slider bounds, a 0–100% strength (§6.11).
pub const SHARPEN_RANGE: (f64, f64) = (0.0, 100.0);

/// One adjustable image property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustKind {
    /// Brightness offset.
    Brightness,
    /// Contrast multiplier.
    Contrast,
    /// Saturation multiplier.
    Saturation,
    /// Gamma exponent.
    Gamma,
    /// Unsharp-mask strength, 0–100%.
    Sharpen,
}

/// The five software image adjustments, all defaulting to neutral
/// (which maps to element passthrough — zero CPU at defaults).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageAdjust {
    /// Brightness offset, −1.0 (darker) … +1.0 (brighter). Default 0.
    pub brightness: f64,
    /// Contrast multiplier, 0 … 2.0. Default 1.
    pub contrast: f64,
    /// Saturation multiplier, 0 (grayscale) … 2.0. Default 1.
    pub saturation: f64,
    /// Gamma correction exponent, 0.3 … 3.0. Default 1.
    pub gamma: f64,
    /// Unsharp-mask strength, 0–100%. Default 0.
    pub sharpen: f64,
}

impl Default for ImageAdjust {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            sharpen: 0.0,
        }
    }
}

impl ImageAdjust {
    /// All five kinds, in panel display order.
    pub const KINDS: [AdjustKind; 5] = [
        AdjustKind::Brightness,
        AdjustKind::Contrast,
        AdjustKind::Saturation,
        AdjustKind::Gamma,
        AdjustKind::Sharpen,
    ];

    /// True when every field is at its neutral default — the compiled
    /// plan is then `None` and frames pass through untouched.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The same adjustments with every field clamped to its range —
    /// protects the pipeline from stale or hand-edited config values.
    #[must_use]
    pub fn clamped(&self) -> Self {
        let mut out = *self;
        for kind in Self::KINDS {
            out.set(kind, out.get(kind));
        }
        out
    }

    /// Value of one field.
    #[must_use]
    pub fn get(&self, kind: AdjustKind) -> f64 {
        match kind {
            AdjustKind::Brightness => self.brightness,
            AdjustKind::Contrast => self.contrast,
            AdjustKind::Saturation => self.saturation,
            AdjustKind::Gamma => self.gamma,
            AdjustKind::Sharpen => self.sharpen,
        }
    }

    /// Sets one field, clamped to its valid range.
    pub fn set(&mut self, kind: AdjustKind, value: f64) {
        let (min, max) = range_of(kind);
        let clamped = value.clamp(min, max);
        match kind {
            AdjustKind::Brightness => self.brightness = clamped,
            AdjustKind::Contrast => self.contrast = clamped,
            AdjustKind::Saturation => self.saturation = clamped,
            AdjustKind::Gamma => self.gamma = clamped,
            AdjustKind::Sharpen => self.sharpen = clamped,
        }
    }
}

/// Valid range of one field (slider bounds and clamps share this).
#[must_use]
pub fn range_of(kind: AdjustKind) -> (f64, f64) {
    match kind {
        AdjustKind::Brightness => BRIGHTNESS_RANGE,
        AdjustKind::Contrast => CONTRAST_RANGE,
        AdjustKind::Saturation => SATURATION_RANGE,
        AdjustKind::Gamma => GAMMA_RANGE,
        AdjustKind::Sharpen => SHARPEN_RANGE,
    }
}

/// Slider step for one field (finer for the unit-scale controls).
#[must_use]
pub fn step_of(kind: AdjustKind) -> f64 {
    match kind {
        AdjustKind::Sharpen => 1.0,
        _ => 0.01,
    }
}

/// Decimal digits shown by the slider's value label (`gtk::Scale`
/// takes `i32`).
#[must_use]
pub fn digits_of(kind: AdjustKind) -> i32 {
    match kind {
        AdjustKind::Sharpen => 0,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_neutral() {
        let adjust = ImageAdjust::default();
        assert_eq!(adjust.brightness, 0.0);
        assert_eq!(adjust.contrast, 1.0);
        assert_eq!(adjust.saturation, 1.0);
        assert_eq!(adjust.gamma, 1.0);
        assert_eq!(adjust.sharpen, 0.0);
        assert!(adjust.is_default());
    }

    #[test]
    fn any_change_breaks_default_detection() {
        for kind in ImageAdjust::KINDS {
            let mut adjust = ImageAdjust::default();
            let (min, max) = range_of(kind);
            adjust.set(kind, if max > 0.0 { max } else { min });
            assert!(!adjust.is_default(), "{kind:?} should break default");
        }
    }

    #[test]
    fn set_clamps_to_range() {
        let mut adjust = ImageAdjust::default();
        adjust.set(AdjustKind::Brightness, -7.0);
        assert_eq!(adjust.brightness, -1.0);
        adjust.set(AdjustKind::Contrast, 99.0);
        assert_eq!(adjust.contrast, 2.0);
        adjust.set(AdjustKind::Gamma, 0.0);
        assert_eq!(adjust.gamma, 0.3);
        adjust.set(AdjustKind::Sharpen, 150.0);
        assert_eq!(adjust.sharpen, 100.0);
    }

    #[test]
    fn clamped_fixes_out_of_range_fields() {
        let bad = ImageAdjust {
            brightness: 3.0,
            contrast: -1.0,
            saturation: 7.0,
            gamma: 0.0,
            sharpen: 200.0,
        };
        let fixed = bad.clamped();
        assert_eq!(fixed.brightness, 1.0);
        assert_eq!(fixed.contrast, 0.0);
        assert_eq!(fixed.saturation, 2.0);
        assert_eq!(fixed.gamma, 0.3);
        assert_eq!(fixed.sharpen, 100.0);
        assert!(bad.clamped().get(AdjustKind::Gamma) >= GAMMA_RANGE.0);
    }

    /// Round-trips a partially specified TOML fragment: missing fields
    /// fall back to neutral defaults (hand-edited config must not lose
    /// the other adjustments).
    #[test]
    fn partial_toml_fills_defaults() {
        let adjust: ImageAdjust =
            toml::from_str("brightness = 0.25").unwrap_or_else(|e| panic!("parse: {e}"));
        assert_eq!(adjust.brightness, 0.25);
        assert_eq!(adjust.contrast, 1.0);
        assert_eq!(adjust.gamma, 1.0);
    }
}
