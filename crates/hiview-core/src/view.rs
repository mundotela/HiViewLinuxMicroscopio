// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! View transform math (docs/ARCHITECTURE.md §6.5): converts between
//! image-space (frame pixels) and widget-space (screen pixels) for zoom,
//! pan and fit-to-window. Pure logic — unit tested, no GTK/GStreamer.

/// Minimum zoom, 25%.
pub const MIN_SCALE: f64 = 0.25;
/// Maximum zoom, 800%.
pub const MAX_SCALE: f64 = 8.0;
/// Scale at or above which the viewport switches to the Nearest filter
/// (200%, for pixel-level inspection).
pub const NEAREST_FILTER_SCALE: f64 = 2.0;

/// A width/height pair, in pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Size {
    /// Horizontal extent.
    pub width: f64,
    /// Vertical extent.
    pub height: f64,
}

impl Size {
    /// Creates a size.
    #[must_use]
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// A 2D point, in pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

impl Point {
    /// Creates a point.
    #[must_use]
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// How the view scale is chosen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewMode {
    /// Follows "fit to window": the scale is recomputed whenever the frame
    /// or the widget changes size.
    #[default]
    Fit,
    /// An explicit zoom chosen by the user (100%, wheel, +/- keys, pan).
    Fixed,
}

/// Image ↔ widget coordinate transform (docs/ARCHITECTURE.md §6.5).
///
/// A widget-space point `w` maps to image space as `i = (w - offset) / scale`;
/// the inverse is `w = i * scale + offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    /// Widget pixels per image pixel (`1.0` = 100%).
    pub scale: f64,
    /// Widget-space position of the image's top-left corner.
    pub offset: Point,
    /// Fit-to-window or explicit zoom.
    pub mode: ViewMode,
}

impl Default for ViewTransform {
    /// Starts fitted; the first frame/resize applies `fit`.
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset: Point::default(),
            mode: ViewMode::Fit,
        }
    }
}

impl ViewTransform {
    /// Scales the whole frame to the largest size that fits `widget`,
    /// centered, clamped to the zoom range.
    #[must_use]
    pub fn fit(frame: Size, widget: Size) -> Self {
        let scale = fit_scale(frame, widget);
        Self {
            scale,
            offset: centered_offset(frame, widget, scale),
            mode: ViewMode::Fit,
        }
    }

    /// 100% zoom (one image pixel per widget pixel), centered.
    #[must_use]
    pub fn zoom_100(frame: Size, widget: Size) -> Self {
        Self {
            scale: 1.0,
            offset: centered_offset(frame, widget, 1.0),
            mode: ViewMode::Fixed,
        }
    }

    /// Maps a widget-space point into image space.
    #[must_use]
    pub fn widget_to_image(&self, p: Point) -> Point {
        Point::new(
            (p.x - self.offset.x) / self.scale,
            (p.y - self.offset.y) / self.scale,
        )
    }

    /// Maps an image-space point into widget space.
    #[must_use]
    pub fn image_to_widget(&self, p: Point) -> Point {
        Point::new(
            p.x * self.scale + self.offset.x,
            p.y * self.scale + self.offset.y,
        )
    }

    /// Zooms by `factor` keeping the image point under `anchor` (a widget-
    /// space position, typically the cursor) fixed. Switches to `Fixed`.
    pub fn zoom_at(&mut self, anchor: Point, factor: f64, frame: Size, widget: Size) {
        let new_scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let image_pt = self.widget_to_image(anchor);
        self.offset = Point::new(
            anchor.x - image_pt.x * new_scale,
            anchor.y - image_pt.y * new_scale,
        );
        self.scale = new_scale;
        self.mode = ViewMode::Fixed;
        self.clamp_offset(frame, widget);
    }

    /// Zooms around the widget center (keyboard/buttons have no cursor).
    pub fn zoom_at_center(&mut self, factor: f64, frame: Size, widget: Size) {
        self.zoom_at(
            Point::new(widget.width / 2.0, widget.height / 2.0),
            factor,
            frame,
            widget,
        );
    }

    /// Pans the image by a widget-space delta (drag). Only switches to
    /// `Fixed` if the pan actually moved the image, so dragging a fitted
    /// image does not break the fit-follow behavior.
    pub fn pan_by(&mut self, delta: Point, frame: Size, widget: Size) {
        let mut moved = *self;
        moved.offset = Point::new(self.offset.x + delta.x, self.offset.y + delta.y);
        moved.clamp_offset(frame, widget);
        if moved.offset != self.offset {
            moved.mode = ViewMode::Fixed;
            *self = moved;
        }
    }

    /// Recomputes the transform after the frame size changed (e.g. a 90°
    /// rotation swaps 640×480 to 480×640): refit in `Fit`, keep the scale
    /// and re-clamp in `Fixed`.
    pub fn on_frame_resize(&mut self, frame: Size, widget: Size) {
        if self.mode == ViewMode::Fit {
            *self = Self::fit(frame, widget);
        } else {
            self.clamp_offset(frame, widget);
        }
    }

    /// Keeps the image from being dragged out of view: larger-than-widget
    /// dimensions clamp the offset, smaller ones stay centered.
    pub fn clamp_offset(&mut self, frame: Size, widget: Size) {
        self.offset.x = clamp_axis(self.offset.x, frame.width * self.scale, widget.width);
        self.offset.y = clamp_axis(self.offset.y, frame.height * self.scale, widget.height);
    }
}

/// The scale that shows the whole frame inside the widget, clamped to the
/// zoom range.
fn fit_scale(frame: Size, widget: Size) -> f64 {
    (widget.width / frame.width)
        .min(widget.height / frame.height)
        .clamp(MIN_SCALE, MAX_SCALE)
}

/// Offset that centers a scaled frame inside the widget.
fn centered_offset(frame: Size, widget: Size, scale: f64) -> Point {
    Point::new(
        (widget.width - frame.width * scale) / 2.0,
        (widget.height - frame.height * scale) / 2.0,
    )
}

/// Clamps one axis: centered when the scaled frame is smaller than the
/// widget, otherwise pinned to the widget edges.
fn clamp_axis(offset: f64, scaled_frame: f64, widget: f64) -> f64 {
    if scaled_frame <= widget {
        (widget - scaled_frame) / 2.0
    } else {
        offset.clamp(widget - scaled_frame, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_640X480: Size = Size {
        width: 640.0,
        height: 480.0,
    };
    const WIDGET: Size = Size {
        width: 800.0,
        height: 600.0,
    };

    #[test]
    fn fit_scales_and_centers_in_window() {
        let t = ViewTransform::fit(FRAME_640X480, WIDGET);
        assert_eq!(t.mode, ViewMode::Fit);
        assert_eq!(t.scale, 1.25);
        // 640*1.25 = 800 fills the width exactly; vertically centered.
        assert_eq!(t.offset.x, 0.0);
        assert_eq!(t.offset.y, (600.0 - 480.0 * 1.25) / 2.0);
    }

    #[test]
    fn zoom_100_centers_at_native_size() {
        let t = ViewTransform::zoom_100(FRAME_640X480, WIDGET);
        assert_eq!(t.mode, ViewMode::Fixed);
        assert_eq!(t.scale, 1.0);
        assert_eq!(t.offset, Point::new(80.0, 60.0));
    }

    #[test]
    fn zoom_keeps_point_under_cursor_fixed() {
        let mut t = ViewTransform::zoom_100(FRAME_640X480, WIDGET);
        let cursor = Point::new(300.0, 200.0);
        let under_cursor = t.widget_to_image(cursor);
        t.zoom_at(cursor, 1.5, FRAME_640X480, WIDGET);
        let after = t.image_to_widget(under_cursor);
        assert!((after.x - cursor.x).abs() < 1e-9);
        assert!((after.y - cursor.y).abs() < 1e-9);
    }

    #[test]
    fn zoom_clamps_to_25_and_800_percent() {
        let mut t = ViewTransform::fit(FRAME_640X480, WIDGET);
        for _ in 0..50 {
            t.zoom_at_center(0.5, FRAME_640X480, WIDGET);
        }
        assert_eq!(t.scale, MIN_SCALE);
        for _ in 0..80 {
            t.zoom_at_center(1.5, FRAME_640X480, WIDGET);
        }
        assert_eq!(t.scale, MAX_SCALE);
    }

    #[test]
    fn pan_stops_at_image_edges() {
        let mut t = ViewTransform::zoom_100(FRAME_640X480, WIDGET);
        // 640x480 scaled at 4.0 -> 2560x1920, larger than the 800x600 widget.
        t.zoom_at_center(4.0, FRAME_640X480, WIDGET);
        t.pan_by(Point::new(10_000.0, 10_000.0), FRAME_640X480, WIDGET);
        assert_eq!(t.offset, Point::new(0.0, 0.0));
        t.pan_by(Point::new(-100_000.0, -100_000.0), FRAME_640X480, WIDGET);
        assert_eq!(
            t.offset,
            Point::new(800.0 - 640.0 * 4.0, 600.0 - 480.0 * 4.0)
        );
    }

    #[test]
    fn pan_keeps_small_frame_centered() {
        let mut t = ViewTransform::fit(FRAME_640X480, WIDGET);
        t.zoom_at_center(0.5, FRAME_640X480, WIDGET); // 640*0.625=400 < 800
        let before = t.offset;
        t.pan_by(Point::new(50.0, 50.0), FRAME_640X480, WIDGET);
        assert_eq!(t.offset, before);
        assert_eq!(t.mode, ViewMode::Fixed);
    }

    #[test]
    fn widget_and_image_coordinates_round_trip() {
        let mut t = ViewTransform::fit(FRAME_640X480, WIDGET);
        t.zoom_at(Point::new(123.0, 45.0), 2.3, FRAME_640X480, WIDGET);
        let widget_pt = Point::new(511.0, 377.0);
        let back = t.image_to_widget(t.widget_to_image(widget_pt));
        assert!((back.x - widget_pt.x).abs() < 1e-9);
        assert!((back.y - widget_pt.y).abs() < 1e-9);
    }

    #[test]
    fn frame_resize_after_rotation_refits() {
        let rotated: Size = Size::new(480.0, 640.0); // 90° swapped 640x480
        let mut t = ViewTransform::fit(FRAME_640X480, WIDGET);
        t.on_frame_resize(rotated, WIDGET);
        assert_eq!(t.mode, ViewMode::Fit);
        // min(800/480, 600/640) = 600/640 = 0.9375
        assert_eq!(t.scale, 0.9375);
        assert_eq!(t.offset.x, (800.0 - 480.0 * 0.9375) / 2.0);
        assert_eq!(t.offset.y, 0.0);
    }

    #[test]
    fn frame_resize_keeps_fixed_scale() {
        let rotated: Size = Size::new(480.0, 640.0);
        let mut t = ViewTransform::zoom_100(FRAME_640X480, WIDGET);
        t.on_frame_resize(rotated, WIDGET);
        assert_eq!(t.mode, ViewMode::Fixed);
        assert_eq!(t.scale, 1.0);
        // Width re-centers (480 < 800); height clamps to the widget edge
        // (640 > 600, so the top-left sits at y = 0).
        assert_eq!(t.offset, Point::new(160.0, 0.0));
    }
}
