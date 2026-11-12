// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Overlay state and geometry (docs/ARCHITECTURE.md §6.9): grid,
//! crosshair and the Explorar/Bancada mode that gates them.
//!
//! All geometry lives in **image coordinates**; the renderer maps it to
//! the screen or to a photo through a [`ViewTransform`], so overlays
//! follow zoom, pan, fit, mirror and 90° rotation for free.
//!
//! Pure logic — no GTK/GStreamer — unit tested.

use serde::{Deserialize, Serialize};

use crate::view::{Point, Size};

/// Default grid spacing, in image pixels.
pub const DEFAULT_GRID_SPACING_PX: f64 = 40.0;
/// Grid spacing bounds exposed in the side panel.
pub const MIN_GRID_SPACING_PX: f64 = 5.0;
/// Grid spacing bounds exposed in the side panel.
pub const MAX_GRID_SPACING_PX: f64 = 200.0;
/// Default tick spacing of the scale crosshair, in image pixels.
pub const DEFAULT_TICK_SPACING_PX: f64 = 10.0;
/// Default grid line opacity (60%).
pub const DEFAULT_GRID_OPACITY: f64 = 0.6;
/// Crosshair line opacity (fixed; only the color is configurable).
pub const CROSSHAIR_OPACITY: f64 = 0.85;
/// Fraction of the smaller frame side used as the crosshair circle
/// radius (image coordinates).
pub const CROSSHAIR_CIRCLE_FRACTION: f64 = 0.25;
/// Half-length of each scale tick on the crosshair, in image pixels.
pub const CROSSHAIR_TICK_LEN_PX: f64 = 3.0;

/// Application mode (docs/ARCHITECTURE.md §1): clean "Explorar" UI vs.
/// the electronics workbench with tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    /// Clean UI: no side panel, overlays hidden, tool shortcuts off.
    #[default]
    Explore,
    /// Workbench UI: side panel with overlay tools.
    Bench,
}

impl AppMode {
    /// The other mode (`M` key toggles between the two).
    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::Explore => Self::Bench,
            Self::Bench => Self::Explore,
        }
    }
}

/// An overlay color, linear RGB components in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OverlayColor {
    /// Red component.
    pub r: f64,
    /// Green component.
    pub g: f64,
    /// Blue component.
    pub b: f64,
}

impl OverlayColor {
    /// Default grid color: green.
    pub const GREEN: Self = Self::new(0.15, 0.9, 0.2);
    /// Crosshair color preset: yellow.
    pub const YELLOW: Self = Self::new(0.95, 0.8, 0.15);
    /// Color preset: cyan.
    pub const CYAN: Self = Self::new(0.2, 0.85, 0.95);
    /// Color preset: white.
    pub const WHITE: Self = Self::new(0.95, 0.95, 0.95);
    /// Color preset: red.
    pub const RED: Self = Self::new(0.9, 0.2, 0.2);

    /// All color presets, in the order the side panel lists them.
    pub const PRESETS: [Self; 5] = [
        Self::GREEN,
        Self::YELLOW,
        Self::CYAN,
        Self::WHITE,
        Self::RED,
    ];

    /// Creates a color.
    #[must_use]
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    /// Index of this color in [`Self::PRESETS`], or `0` when the color is
    /// not a preset (e.g. a hand-edited config file).
    #[must_use]
    pub fn preset_index(&self) -> u32 {
        Self::PRESETS.iter().position(|p| p == self).unwrap_or(0) as u32
    }
}

impl Default for OverlayColor {
    fn default() -> Self {
        Self::GREEN
    }
}

/// Grid spacing unit (docs/ARCHITECTURE.md §6.9). M6 only supports image
/// pixels; M8 adds the `Millimeters` variant once a calibration profile
/// supplies `px_per_mm` — the panel then feeds the active calibration
/// into [`GridSpacing::pixels`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "lowercase")]
pub enum GridSpacing {
    /// Spacing in image pixels.
    Pixels(f64),
}

impl Default for GridSpacing {
    fn default() -> Self {
        Self::Pixels(DEFAULT_GRID_SPACING_PX)
    }
}

impl GridSpacing {
    /// Spacing in image pixels. `px_per_mm` is the active calibration —
    /// unused until M8 adds the millimeter unit.
    #[must_use]
    pub fn pixels(&self, _px_per_mm: Option<f64>) -> f64 {
        match self {
            Self::Pixels(px) => *px,
        }
    }
}

/// Grid overlay settings (§6.9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridConfig {
    /// Whether the grid is drawn (`G` toggles it, Bancada only).
    pub enabled: bool,
    /// Spacing between lines, in image pixels (M8: millimeters).
    pub spacing: GridSpacing,
    /// Line color.
    pub color: OverlayColor,
    /// Line opacity, `0.0..=1.0`.
    pub opacity: f64,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            spacing: GridSpacing::default(),
            color: OverlayColor::GREEN,
            opacity: DEFAULT_GRID_OPACITY,
        }
    }
}

/// Crosshair style variants (§6.9), in the order the panel lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CrosshairStyle {
    /// Plain cross through the image center.
    #[default]
    Cross,
    /// Cross plus a circle of [`CROSSHAIR_CIRCLE_FRACTION`] radius.
    Circle,
    /// Cross plus scale ticks every
    /// [`CrosshairConfig::tick_spacing_px`] image pixels.
    Ticks,
}

/// Crosshair overlay settings (§6.9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrosshairConfig {
    /// Whether the crosshair is drawn (`C` toggles it, Bancada only).
    pub enabled: bool,
    /// Visual style.
    pub style: CrosshairStyle,
    /// Line color.
    pub color: OverlayColor,
    /// Tick spacing for [`CrosshairStyle::Ticks`], in image pixels (M8
    /// turns this into calibrated millimeters).
    pub tick_spacing_px: f64,
}

impl Default for CrosshairConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            style: CrosshairStyle::default(),
            color: OverlayColor::YELLOW,
            tick_spacing_px: DEFAULT_TICK_SPACING_PX,
        }
    }
}

/// All overlay settings, persisted in `config.toml` (`[overlays]`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayState {
    /// Grid overlay.
    pub grid: GridConfig,
    /// Crosshair overlay.
    pub crosshair: CrosshairConfig,
}

/// The overlays to actually draw in `mode` (docs/ARCHITECTURE.md §1):
/// none in Explorar, none when everything is off. The returned state is
/// a copy of `state`, so switching modes never loses settings.
#[must_use]
pub fn overlays_for(mode: AppMode, state: &OverlayState) -> Option<OverlayState> {
    if mode == AppMode::Bench && (state.grid.enabled || state.crosshair.enabled) {
        Some(state.clone())
    } else {
        None
    }
}

/// Image-space position of each grid line along one axis of length
/// `length`, center-anchored: a line always crosses the middle and the
/// rest step out in `spacing` increments up to both edges.
#[must_use]
pub fn grid_line_positions(length: f64, spacing_px: f64) -> Vec<f64> {
    if length <= 0.0 || spacing_px <= 0.0 {
        return Vec::new();
    }
    let center = length / 2.0;
    // Steps needed to reach the left edge; lines then walk rightwards.
    let steps = (center / spacing_px).ceil() as i32;
    (0..=2 * steps)
        .map(|k| center + f64::from(k - steps) * spacing_px)
        .filter(|x| *x >= -f64::EPSILON && *x <= length + f64::EPSILON)
        .collect()
}

/// Center of the frame in image coordinates — the crosshair anchor.
#[must_use]
pub fn image_center(frame: Size) -> Point {
    Point::new(frame.width / 2.0, frame.height / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_toggles_between_explore_and_bench() {
        assert_eq!(AppMode::Explore.toggled(), AppMode::Bench);
        assert_eq!(AppMode::Bench.toggled(), AppMode::Explore);
    }

    #[test]
    fn explore_hides_overlays() {
        let mut state = OverlayState::default();
        state.grid.enabled = true;
        state.crosshair.enabled = true;
        assert_eq!(overlays_for(AppMode::Explore, &state), None);
    }

    #[test]
    fn bench_shows_and_restores_overlay_state() {
        let mut state = OverlayState::default();
        state.grid.enabled = true;
        state.grid.spacing = GridSpacing::Pixels(25.0);
        state.grid.color = OverlayColor::RED;
        state.grid.opacity = 0.9;
        state.crosshair.enabled = true;
        state.crosshair.style = CrosshairStyle::Ticks;
        // Explorar hides everything; switching back to Bancada must
        // restore the exact same settings (none were lost).
        assert_eq!(overlays_for(AppMode::Explore, &state), None);
        assert_eq!(overlays_for(AppMode::Bench, &state), Some(state.clone()));
    }

    #[test]
    fn bench_with_everything_off_draws_nothing() {
        let state = OverlayState::default();
        assert_eq!(overlays_for(AppMode::Bench, &state), None);
    }

    #[test]
    fn grid_lines_are_center_anchored_and_reach_both_edges() {
        // 640 px frame, spacing 100: center 320 is a multiple of 100 only
        // through the anchor — lines land at 20,120,...,620.
        let lines = grid_line_positions(640.0, 100.0);
        assert_eq!(lines, vec![20.0, 120.0, 220.0, 320.0, 420.0, 520.0, 620.0]);
    }

    #[test]
    fn grid_lines_reach_the_edges_when_spacing_divides_the_frame() {
        let lines = grid_line_positions(640.0, 64.0);
        assert_eq!(lines.first().copied(), Some(0.0));
        assert_eq!(lines.last().copied(), Some(640.0));
        assert_eq!(lines.len(), 11);
    }

    #[test]
    fn grid_lines_adapt_to_rotated_frame() {
        // After a 90° rotation the frame is 480×640: each axis is
        // gridded against its own length.
        let lines = grid_line_positions(480.0, 100.0);
        assert_eq!(lines, vec![40.0, 140.0, 240.0, 340.0, 440.0]);
    }

    #[test]
    fn grid_lines_empty_for_degenerate_inputs() {
        assert!(grid_line_positions(0.0, 10.0).is_empty());
        assert!(grid_line_positions(640.0, 0.0).is_empty());
        assert!(grid_line_positions(640.0, -5.0).is_empty());
    }

    #[test]
    fn crosshair_center_tracks_the_frame_size() {
        assert_eq!(
            image_center(Size::new(640.0, 480.0)),
            Point::new(320.0, 240.0)
        );
        // Rotated 480×640: the center moves with the frame.
        assert_eq!(
            image_center(Size::new(480.0, 640.0)),
            Point::new(240.0, 320.0)
        );
    }

    #[test]
    fn color_preset_index_falls_back_to_zero() {
        assert_eq!(OverlayColor::CYAN.preset_index(), 2);
        assert_eq!(OverlayColor::new(0.1, 0.2, 0.3).preset_index(), 0);
    }
}
