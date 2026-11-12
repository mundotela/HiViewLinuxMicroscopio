// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! The single overlay renderer (docs/ARCHITECTURE.md §6.9, ADR-012).
//!
//! Every coordinate in [`hiview_core::OverlayState`] is in **image
//! space**; [`OverlayRenderer::draw`] maps them through a
//! [`ViewTransform`], so the same code draws on the viewport (widget
//! coordinates, following zoom/pan/rotation) and on a photo
//! (output-resolution coordinates, `scale = out/native`).

use gtk::cairo;
use hiview_core::{
    CROSSHAIR_CIRCLE_FRACTION, CROSSHAIR_OPACITY, CROSSHAIR_TICK_LEN_PX, CrosshairConfig,
    CrosshairStyle, DraftView, GridConfig, MeasureOverlay, MeasureShape, MeasureTool, OverlayState,
    Point, Size, ViewTransform, angle_deg, distance, grid_line_positions, image_center,
};

/// Draws grid and crosshair overlays on a cairo context (§6.9).
///
/// ARCHITECTURE.md sketches `draw(cr, view, state)`; two extra
/// parameters are needed in practice: `frame`, the image size in image
/// pixels (the transform does not know it), and `px`, the size of one
/// device pixel in target units (`1/scale_factor` on screen, `1.0` on a
/// photo), used to keep every line one pixel crisp.
pub struct OverlayRenderer;

impl OverlayRenderer {
    /// Draws `state` on `cr` clipped to the image bounds. `view` maps
    /// image coordinates to the target surface's coordinates.
    pub fn draw(
        cr: &cairo::Context,
        frame: Size,
        view: &ViewTransform,
        state: &OverlayState,
        px: f64,
    ) {
        if px <= 0.0 {
            return;
        }
        let top_left = view.image_to_widget(Point::new(0.0, 0.0));
        let w = frame.width * view.scale;
        let h = frame.height * view.scale;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let _clip = ClipGuard::push(cr, top_left.x, top_left.y, w, h);
        if state.grid.enabled {
            draw_grid(cr, frame, view, &state.grid, state.measure.px_per_mm, px);
        }
        if state.crosshair.enabled {
            draw_crosshair(
                cr,
                frame,
                view,
                &state.crosshair,
                state.measure.px_per_mm,
                px,
            );
        }
        draw_measurements(cr, view, &state.measure, px);
    }
}

/// Snap a coordinate to the device-pixel half-grid so a `px`-wide line
/// lands on whole pixels instead of straddling two.
fn snap(v: f64, px: f64) -> f64 {
    ((v / px) - 0.5).round() * px + px * 0.5
}

/// Scoped `save`/`clip`/`restore` — the caller's context state must be
/// untouched when `draw` returns.
struct ClipGuard<'a>(&'a cairo::Context);

impl<'a> ClipGuard<'a> {
    fn push(cr: &'a cairo::Context, x: f64, y: f64, w: f64, h: f64) -> Self {
        cr.save().ok();
        cr.rectangle(x, y, w, h);
        cr.clip();
        Self(cr)
    }
}

impl Drop for ClipGuard<'_> {
    fn drop(&mut self) {
        self.0.restore().ok();
    }
}

/// Measurement line color (cyan) — distinct from the grid's green and
/// the crosshair's configurable presets.
const MEASURE_COLOR: (f64, f64, f64) = (0.2, 0.85, 0.95);
/// Calibration draft color (accent orange) — the reference line is not
/// a measurement yet.
const CALIBRATE_COLOR: (f64, f64, f64) = (1.0, 0.6, 0.15);
/// Measurement line width, in device pixels.
const MEASURE_LINE_PX: f64 = 2.0;
/// Dot radius on measurement vertices, in device pixels.
const MEASURE_DOT_PX: f64 = 3.0;
/// Measurement label font size, in device pixels.
const MEASURE_FONT_PX: f64 = 12.0;
/// Angle arc radius, in device pixels.
const MEASURE_ARC_PX: f64 = 16.0;

/// Vertical and horizontal grid lines spanning the whole image.
fn draw_grid(
    cr: &cairo::Context,
    frame: Size,
    view: &ViewTransform,
    grid: &GridConfig,
    px_per_mm: Option<f64>,
    px: f64,
) {
    let spacing = grid.spacing.pixels(px_per_mm);
    let x0 = view.offset.x;
    let y0 = view.offset.y;
    let x1 = x0 + frame.width * view.scale;
    let y1 = y0 + frame.height * view.scale;

    cr.set_source_rgba(grid.color.r, grid.color.g, grid.color.b, grid.opacity);
    cr.set_line_width(px);
    for x in grid_line_positions(frame.width, spacing) {
        let wx = snap(view.image_to_widget(Point::new(x, 0.0)).x, px);
        cr.move_to(wx, y0);
        cr.line_to(wx, y1);
    }
    for y in grid_line_positions(frame.height, spacing) {
        let wy = snap(view.image_to_widget(Point::new(0.0, y)).y, px);
        cr.move_to(x0, wy);
        cr.line_to(x1, wy);
    }
    let _ = cr.stroke();
}

/// Center crosshair: plain cross, cross + circle or cross + scale ticks.
fn draw_crosshair(
    cr: &cairo::Context,
    frame: Size,
    view: &ViewTransform,
    cross: &CrosshairConfig,
    px_per_mm: Option<f64>,
    px: f64,
) {
    let c = image_center(frame);
    let wc = view.image_to_widget(c);
    let x0 = view.offset.x;
    let y0 = view.offset.y;
    let x1 = x0 + frame.width * view.scale;
    let y1 = y0 + frame.height * view.scale;
    let sx = snap(wc.x, px);
    let sy = snap(wc.y, px);

    cr.set_source_rgba(
        cross.color.r,
        cross.color.g,
        cross.color.b,
        CROSSHAIR_OPACITY,
    );
    cr.set_line_width(px);
    cr.move_to(x0, sy);
    cr.line_to(x1, sy);
    cr.move_to(sx, y0);
    cr.line_to(sx, y1);
    let _ = cr.stroke();

    match cross.style {
        CrosshairStyle::Cross => {}
        CrosshairStyle::Circle => {
            let radius = frame.width.min(frame.height) * CROSSHAIR_CIRCLE_FRACTION * view.scale;
            cr.arc(sx, sy, radius, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
        }
        CrosshairStyle::Ticks => {
            draw_ticks(cr, frame, view, cross, Point::new(sx, sy), px_per_mm, px);
        }
    }
}

/// Perpendicular ticks every `tick_spacing` (px or calibrated mm) along
/// both crosshair arms, center-anchored like the grid.
fn draw_ticks(
    cr: &cairo::Context,
    frame: Size,
    view: &ViewTransform,
    cross: &CrosshairConfig,
    wc: Point,
    px_per_mm: Option<f64>,
    px: f64,
) {
    let half = CROSSHAIR_TICK_LEN_PX * view.scale;
    let tick_spacing = cross.tick_spacing.pixels(px_per_mm);
    // Ticks on the horizontal arm are short vertical marks; on the
    // vertical arm, short horizontal marks.
    for x in grid_line_positions(frame.width, tick_spacing) {
        let wx = snap(view.image_to_widget(Point::new(x, 0.0)).x, px);
        cr.move_to(wx, wc.y - half);
        cr.line_to(wx, wc.y + half);
    }
    for y in grid_line_positions(frame.height, tick_spacing) {
        let wy = snap(view.image_to_widget(Point::new(0.0, y)).y, px);
        cr.move_to(wc.x - half, wy);
        cr.line_to(wc.x + half, wy);
    }
    let _ = cr.stroke();
}

/// Committed measurements plus the in-progress draft (§6.9). All shapes
/// arrive in image coordinates; `view` maps them to the target surface.
fn draw_measurements(cr: &cairo::Context, view: &ViewTransform, layer: &MeasureOverlay, px: f64) {
    for item in &layer.items {
        draw_shape(cr, view, &item.shape, MEASURE_COLOR, false, px);
        let label = item.shape.label(layer.px_per_mm);
        draw_label(
            cr,
            &label,
            label_anchor(view, &item.shape),
            MEASURE_COLOR,
            px,
        );
    }
    if let Some(draft) = &layer.draft {
        draw_draft(cr, view, draft, layer.px_per_mm, px);
    }
}

/// The widget-space anchor a shape's label attaches to: the line's
/// midpoint, the angle's vertex, the circle's top edge.
fn label_anchor(view: &ViewTransform, shape: &MeasureShape) -> Point {
    let (anchor, dx, dy) = match *shape {
        MeasureShape::Line { a, b } => {
            (Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), 6.0, -10.0)
        }
        MeasureShape::Angle { vertex, .. } => (vertex, 10.0, 6.0),
        MeasureShape::Circle { center, edge } => {
            let r = distance(center, edge);
            (Point::new(center.x, center.y - r), -20.0, -14.0)
        }
    };
    let w = view.image_to_widget(anchor);
    Point::new(w.x + dx, w.y + dy)
}

/// One measurement shape: outline + vertex dots, solid or dashed.
fn draw_shape(
    cr: &cairo::Context,
    view: &ViewTransform,
    shape: &MeasureShape,
    color: (f64, f64, f64),
    dashed: bool,
    px: f64,
) {
    cr.set_source_rgb(color.0, color.1, color.2);
    cr.set_line_width(MEASURE_LINE_PX * px);
    if dashed {
        cr.set_dash(&[6.0 * px, 4.0 * px], 0.0);
    } else {
        cr.set_dash(&[], 0.0);
    }
    let w = |p: Point| view.image_to_widget(p);
    match *shape {
        MeasureShape::Line { a, b } => {
            let (wa, wb) = (w(a), w(b));
            cr.move_to(wa.x, wa.y);
            cr.line_to(wb.x, wb.y);
            let _ = cr.stroke();
            draw_dot(cr, wa, px);
            draw_dot(cr, wb, px);
        }
        MeasureShape::Angle { a, vertex, b } => {
            let (wa, wv, wb) = (w(a), w(vertex), w(b));
            cr.move_to(wa.x, wa.y);
            cr.line_to(wv.x, wv.y);
            cr.line_to(wb.x, wb.y);
            let _ = cr.stroke();
            // The arc radius in widget units: uniform scale keeps the
            // angle true on screen and on the photo.
            let r = MEASURE_ARC_PX * px;
            let a0 = (wa.y - wv.y).atan2(wa.x - wv.x);
            let a1 = (wb.y - wv.y).atan2(wb.x - wv.x);
            let (lo, hi) = (a0.min(a1), a0.max(a1));
            // Draw the arc through the smaller sweep when the arms
            // straddle ±π — the measured angle is always ≤ 180°.
            if angle_deg(a, vertex, b).to_radians() < hi - lo {
                cr.arc_negative(wv.x, wv.y, r, hi, lo + std::f64::consts::TAU);
            } else {
                cr.arc(wv.x, wv.y, r, lo, hi);
            }
            let _ = cr.stroke();
            draw_dot(cr, wa, px);
            draw_dot(cr, wv, px);
            draw_dot(cr, wb, px);
        }
        MeasureShape::Circle { center, edge } => {
            let (wc, we) = (w(center), w(edge));
            cr.arc(wc.x, wc.y, distance(wc, we), 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            // The diameter chord through the center shows what is
            // actually being reported.
            cr.move_to(2.0 * wc.x - we.x, 2.0 * wc.y - we.y);
            cr.line_to(we.x, we.y);
            let _ = cr.stroke();
            draw_dot(cr, wc, px);
            draw_dot(cr, we, px);
        }
    }
}

/// A filled vertex dot.
fn draw_dot(cr: &cairo::Context, at: Point, px: f64) {
    cr.arc(at.x, at.y, MEASURE_DOT_PX * px, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

/// The rubber-band draft: fixed points as dots plus the dashed preview
/// of what a commit right now would produce, with its live label.
fn draw_draft(
    cr: &cairo::Context,
    view: &ViewTransform,
    draft: &DraftView,
    px_per_mm: Option<f64>,
    px: f64,
) {
    let color = if draft.tool == MeasureTool::Calibrate {
        CALIBRATE_COLOR
    } else {
        MEASURE_COLOR
    };
    cr.set_source_rgb(color.0, color.1, color.2);
    for p in &draft.points {
        draw_dot(cr, view.image_to_widget(*p), px);
    }
    if let Some(shape) = draft.preview_shape() {
        draw_shape(cr, view, &shape, color, true, px);
        let label = shape.label(px_per_mm);
        draw_label(cr, &label, label_anchor(view, &shape), color, px);
    }
}

/// A measurement value on a dark chip, readable over any frame content.
fn draw_label(cr: &cairo::Context, text: &str, at: Point, color: (f64, f64, f64), px: f64) {
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(MEASURE_FONT_PX * px);
    let Ok(ext) = cr.text_extents(text) else {
        return;
    };
    let pad = 3.0 * px;
    // `at` is the chip's top-left; the text sits inside it.
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
    cr.rectangle(
        at.x,
        at.y,
        ext.width() + 2.0 * pad,
        ext.height() + 2.0 * pad,
    );
    let _ = cr.fill();
    cr.set_source_rgb(color.0, color.1, color.2);
    cr.move_to(at.x + pad, at.y + pad + ext.height());
    let _ = cr.show_text(text);
}

#[cfg(test)]
mod tests {
    use hiview_core::{GridSpacing, OverlayColor, ViewMode};

    use super::*;

    const FRAME: Size = Size {
        width: 640.0,
        height: 480.0,
    };
    const WIDGET: Size = Size {
        width: 800.0,
        height: 600.0,
    };

    /// Draws `state` onto an `ARgb32` surface of `widget` size with `view`
    /// and returns the B,G,R,A pixel data.
    fn render(
        frame: Size,
        widget: Size,
        view: &ViewTransform,
        state: &OverlayState,
    ) -> (Vec<u8>, usize) {
        let stride = widget.width as usize * 4;
        let surface = cairo::ImageSurface::create_for_data(
            vec![0u8; stride * widget.height as usize],
            cairo::Format::ARgb32,
            widget.width as i32,
            widget.height as i32,
            stride as i32,
        )
        .unwrap_or_else(|e| panic!("surface: {e}"));
        {
            let cr = cairo::Context::new(&surface).unwrap_or_else(|e| panic!("context: {e}"));
            OverlayRenderer::draw(&cr, frame, view, state, 1.0);
        }
        surface.flush();
        let data = surface.take_data().unwrap_or_else(|e| panic!("data: {e}"));
        (data.as_ref().to_vec(), stride)
    }

    fn pixel(data: &[u8], stride: usize, x: u32, y: u32) -> [u8; 4] {
        let o = y as usize * stride + x as usize * 4;
        [data[o], data[o + 1], data[o + 2], data[o + 3]]
    }

    /// B,G,R,A → is the green grid color present?
    fn is_greenish(px: [u8; 4]) -> bool {
        px[1] > 150 && px[0] < 100 && px[2] < 100
    }

    fn is_blank(px: [u8; 4]) -> bool {
        px == [0, 0, 0, 0]
    }

    fn grid_state(spacing: f64) -> OverlayState {
        let mut state = OverlayState::default();
        state.grid.enabled = true;
        state.grid.spacing = GridSpacing::Pixels(spacing);
        state.grid.opacity = 1.0;
        state
    }

    /// Fit transform: 640×480 in an 800×600 widget is scale 1.25 with a
    /// 0×0 offset — a grid line at image x=64 lands at widget x=80. The
    /// horizontal axis is center-anchored (240 is not a multiple of 64),
    /// so its lines sit at image y = 48,112,…,432 → widget 60,140,….
    #[test]
    fn grid_lines_land_at_transformed_positions_fit() {
        let view = ViewTransform::fit(FRAME, WIDGET);
        let (data, stride) = render(FRAME, WIDGET, &view, &grid_state(64.0));
        // Row 100 crosses no horizontal line.
        assert!(is_greenish(pixel(&data, stride, 80, 100)));
        assert!(is_blank(pixel(&data, stride, 79, 100)));
        assert!(is_blank(pixel(&data, stride, 81, 100)));
        // Horizontal line at image y=48 → widget y=60. x=404 avoids
        // the vertical line at x=400 (image x=320).
        assert!(is_greenish(pixel(&data, stride, 404, 60)));
        assert!(is_blank(pixel(&data, stride, 404, 59)));
    }

    /// Zoomed (4×) and panned (offset −100,−50): image x=64 lands at
    /// widget x=156, image y=48 at widget y=142.
    #[test]
    fn grid_lines_follow_zoom_and_pan() {
        let view = ViewTransform {
            scale: 4.0,
            offset: Point::new(-100.0, -50.0),
            mode: ViewMode::Fixed,
        };
        let (data, stride) = render(FRAME, WIDGET, &view, &grid_state(64.0));
        assert!(is_greenish(pixel(&data, stride, 156, 100)));
        assert!(is_blank(pixel(&data, stride, 155, 100)));
        assert!(is_blank(pixel(&data, stride, 157, 100)));
        assert!(is_greenish(pixel(&data, stride, 400, 142)));
    }

    /// Rotated 480×640 frame fitted: scale = 600/640 = 0.9375, offset
    /// x = (800 − 450)/2 = 175. The grid is re-anchored to each new axis.
    #[test]
    fn grid_lines_adapt_to_rotated_frame() {
        let rotated = Size::new(480.0, 640.0);
        let view = ViewTransform::fit(rotated, WIDGET);
        let mut state = grid_state(64.0);
        state.crosshair.enabled = true;
        state.crosshair.color = OverlayColor::RED;
        let (data, stride) = render(rotated, WIDGET, &view, &state);
        let data = data.as_slice();
        // Grid line at image x=176 → widget 176·0.9375 + 175 = 340.
        assert!(is_greenish(pixel(data, stride, 340, 100)));
        // Image x=240 is both a grid line and the crosshair vertical:
        // widget x = 240·0.9375 + 175 = 400 — the red crosshair is drawn
        // over the green grid.
        let vx = pixel(data, stride, 400, 100);
        assert!(
            vx[2] > 150 && vx[1] < 100,
            "expected red crosshair, got {vx:?}"
        );
        // The crosshair horizontal at image y=320 → widget y=300.
        let px = pixel(data, stride, 500, 300);
        assert!(
            px[2] > 150 && px[1] < 100,
            "expected red crosshair, got {px:?}"
        );
    }

    /// Nothing is painted outside the image bounds: the clip must hide
    /// every stroke that would leak into the letterbox area. A 900×800
    /// widget letterboxes the frame vertically (offset y = 62.5).
    #[test]
    fn overlays_are_clipped_to_the_image() {
        let widget = Size::new(900.0, 800.0);
        let view = ViewTransform::fit(FRAME, widget);
        let (data, stride) = render(FRAME, widget, &view, &grid_state(64.0));
        // Grid line at image x=64 → widget x=90, painted only inside.
        assert!(is_blank(pixel(&data, stride, 90, 0)));
        assert!(is_blank(pixel(&data, stride, 90, 799)));
        assert!(is_greenish(pixel(&data, stride, 90, 100)));
        assert!(is_greenish(pixel(&data, stride, 90, 700)));
    }
}
