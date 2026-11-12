// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Software image adjustments (docs/ARCHITECTURE.md §6.11): brightness,
//! contrast, saturation, gamma and sharpen applied inside the capture
//! pipeline, so the live view, photos and videos all show them.
//!
//! Values are stored per device under `[devices."VID:PID"]` in
//! `config.toml` and re-applied on connect. Pure logic — no GTK/GStreamer —
//! unit tested.

use serde::{Deserialize, Serialize};

/// `videobalance.brightness` bounds (§6.11).
pub const BRIGHTNESS_RANGE: (f64, f64) = (-1.0, 1.0);
/// `videobalance.contrast` bounds (§6.11).
pub const CONTRAST_RANGE: (f64, f64) = (0.0, 2.0);
/// `videobalance.saturation` bounds (§6.11).
pub const SATURATION_RANGE: (f64, f64) = (0.0, 2.0);
/// `gamma.gamma` bounds (§6.11).
pub const GAMMA_RANGE: (f64, f64) = (0.3, 3.0);
/// Sharpen slider bounds, a 0–100% fraction (§6.11).
pub const SHARPEN_RANGE: (f64, f64) = (0.0, 1.0);

/// Most negative `gaussianblur.sigma` the UI may request.
///
/// §6.11 suggested mapping 100% → −2.0, but gaussianblur's kernel is
/// `1 + 2·ceil(2.5·|sigma|)` taps and costs ~4 ms/frame at kernel 3
/// already (~11% of a core at 30 fps, measured on the target machine);
/// |sigma| ≥ 0.4 jumps to kernel 5 (~18%) and −2.0 (kernel 11) costs
/// ~34% — alone over the 25% steady-state budget of §7. −0.39 is the
/// strongest sharpen that stays inside kernel 3 and inside budget.
/// See `docs/reports/M7.md` §9.
pub const SHARPEN_MAX_SIGMA: f64 = -0.39;

/// One adjustable image property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustKind {
    /// `videobalance.brightness`.
    Brightness,
    /// `videobalance.contrast`.
    Contrast,
    /// `videobalance.saturation`.
    Saturation,
    /// `gamma.gamma`.
    Gamma,
    /// `gaussianblur.sigma` mapped from a 0–100% slider.
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
    /// Sharpen amount as a 0–100% fraction. Default 0.
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

    /// True when every field is at its neutral default (the adjustment
    /// elements then run in passthrough and cost nothing).
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

    /// `gaussianblur.sigma` for the current sharpen fraction: 0% → 0.0
    /// (1×1 kernel, effectively a copy), 100% → [`SHARPEN_MAX_SIGMA`].
    #[must_use]
    pub fn sharpen_sigma(&self) -> f64 {
        self.sharpen * SHARPEN_MAX_SIGMA
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

/// Decimal digits shown by the slider's value label.
#[must_use]
pub fn digits_of(kind: AdjustKind) -> u32 {
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
        adjust.set(AdjustKind::Sharpen, 1.5);
        assert_eq!(adjust.sharpen, 1.0);
    }

    #[test]
    fn clamped_fixes_out_of_range_fields() {
        let bad = ImageAdjust {
            brightness: 3.0,
            contrast: -1.0,
            saturation: 7.0,
            gamma: 0.0,
            sharpen: 2.0,
        };
        let fixed = bad.clamped();
        assert_eq!(fixed.brightness, 1.0);
        assert_eq!(fixed.contrast, 0.0);
        assert_eq!(fixed.saturation, 2.0);
        assert_eq!(fixed.gamma, 0.3);
        assert_eq!(fixed.sharpen, 1.0);
        assert!(bad.clamped().get(AdjustKind::Gamma) >= GAMMA_RANGE.0);
    }

    /// UI → element property mapping (§6.11): 0% sharpen must land on
    /// sigma 0 (passthrough cost), 100% on the budget-capped maximum.
    #[test]
    fn sharpen_maps_to_negative_sigma() {
        let mut adjust = ImageAdjust::default();
        assert_eq!(adjust.sharpen_sigma(), 0.0);
        adjust.sharpen = 0.5;
        assert!((adjust.sharpen_sigma() - SHARPEN_MAX_SIGMA / 2.0).abs() < 1e-9);
        adjust.sharpen = 1.0;
        assert_eq!(adjust.sharpen_sigma(), SHARPEN_MAX_SIGMA);
        assert!(SHARPEN_MAX_SIGMA < 0.0);
        // Kernel is 1 + 2·ceil(2.5·|sigma|): never leave kernel 3, the
        // only size that meets the §7 CPU budget on the target machine.
        assert!(2.5 * SHARPEN_MAX_SIGMA.abs() <= 1.0);
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
