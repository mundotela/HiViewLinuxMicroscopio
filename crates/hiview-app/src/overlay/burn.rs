// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Photo overlay burn-in (docs/ARCHITECTURE.md §6.6, ADR-012): the same
//! [`OverlayRenderer`] used by the viewport draws onto a
//! `cairo::ImageSurface` at the photo's output resolution, so the saved
//! file shows exactly the overlay that was on screen, proportionally
//! scaled.

use gtk::cairo;
use hiview_core::{OverlayState, Point, Size, ViewMode, ViewTransform};
use image::RgbaImage;

use super::OverlayRenderer;

/// Burns the overlays into `image` (RGBA8 at output resolution) and
/// returns it. `source` is the frame size in image coordinates — e.g.
/// 640×480 native, 480×640 after a 90° rotation — so
/// `scale = out/source` maps the overlay exactly like the screen.
///
/// On any cairo failure the photo is returned unburned: the overlay is
/// a cosmetic extra, never worth losing a photo.
#[must_use]
pub fn burn_in(image: RgbaImage, source: Size, state: &OverlayState) -> RgbaImage {
    let (w, h) = image.dimensions();
    let mut bytes = image.into_raw();
    if let Err(err) = burn_bytes(&mut bytes, w, h, source, state) {
        tracing::warn!("could not burn overlays into photo: {err}");
    }
    match RgbaImage::from_raw(w, h, bytes) {
        Some(image) => image,
        // INVARIANT: `bytes` still holds exactly w*h*4 — burn_bytes only
        // recolors pixels in place.
        None => unreachable!(),
    }
}

/// Draws the overlay through an `ARgb32` surface. Cairo stores that
/// format as B,G,R,A on little-endian systems, so R and B swap on the
/// way in and on the way back.
fn burn_bytes(
    bytes: &mut [u8],
    w: u32,
    h: u32,
    source: Size,
    state: &OverlayState,
) -> Result<(), String> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32)
        .map_err(|e| e.to_string())?;
    let stride = surface.stride() as usize;
    let row = w as usize * 4;
    {
        let mut data = surface.data().map_err(|e| e.to_string())?;
        for y in 0..h as usize {
            let src = &bytes[y * row..y * row + row];
            let dst = &mut data[y * stride..y * stride + row];
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0.iter())
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        }
    }
    {
        let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
        let view = ViewTransform {
            scale: f64::from(w) / source.width,
            offset: Point::new(0.0, 0.0),
            mode: ViewMode::Fixed,
        };
        OverlayRenderer::draw(&cr, source, &view, state, 1.0);
    }
    surface.flush();
    {
        let data = surface.data().map_err(|e| e.to_string())?;
        for y in 0..h as usize {
            let src = &data[y * stride..y * stride + row];
            let dst = &mut bytes[y * row..y * row + row];
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0.iter())
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hiview_core::{CrosshairStyle, GridSpacing, OverlayColor};

    use super::*;

    const BLACK: [u8; 4] = [0, 0, 0, 255];

    fn black_image(w: u32, h: u32) -> RgbaImage {
        let mut image = RgbaImage::new(w, h);
        for px in image.pixels_mut() {
            *px = image::Rgba(BLACK);
        }
        image
    }

    fn overlays_grid_only(spacing: f64) -> OverlayState {
        let mut state = OverlayState::default();
        state.grid.enabled = true;
        state.grid.spacing = GridSpacing::Pixels(spacing);
        state.grid.opacity = 1.0;
        state
    }

    /// A grid line at image x=64 lands at output x=64·scale: 64 at
    /// native, 160 at the 1600×1200 interpolation (§6.6).
    #[test]
    fn burn_in_scales_grid_to_output_resolution() {
        let state = overlays_grid_only(64.0);
        let source = Size::new(640.0, 480.0);

        let native = burn_in(black_image(640, 480), source, &state);
        // Grid color is the green preset.
        assert!(native.get_pixel(64, 100).0[1] > 200);
        assert_eq!(native.get_pixel(63, 100).0, BLACK);
        assert_eq!(native.get_pixel(65, 100).0, BLACK);

        let upscaled = burn_in(black_image(1600, 1200), source, &state);
        assert!(upscaled.get_pixel(160, 250).0[1] > 200);
        assert_eq!(upscaled.get_pixel(159, 250).0, BLACK);
        assert_eq!(upscaled.get_pixel(161, 250).0, BLACK);
    }

    /// The crosshair is centered on the image whatever the output size:
    /// (320,240) at native, (800,600) at 1600×1200.
    #[test]
    fn burn_in_centers_crosshair_at_output_resolution() {
        let mut state = OverlayState::default();
        state.crosshair.enabled = true;
        state.crosshair.color = OverlayColor::RED;
        let source = Size::new(640.0, 480.0);

        let native = burn_in(black_image(640, 480), source, &state);
        // Crosshair opacity is 0.85 over black → red ≈ 195.
        assert!(native.get_pixel(320, 240).0[0] > 150);

        let upscaled = burn_in(black_image(1600, 1200), source, &state);
        assert!(upscaled.get_pixel(800, 600).0[0] > 150);
        // Two pixels off the cross lines in both axes: truly blank.
        assert_eq!(upscaled.get_pixel(802, 602).0, BLACK);
    }

    /// A 90°-rotated frame (480×640) keeps the overlay centered and
    /// gridded against its own axes.
    #[test]
    fn burn_in_adapts_to_rotated_frame() {
        let mut state = overlays_grid_only(64.0);
        state.crosshair.enabled = true;
        state.crosshair.color = OverlayColor::WHITE;
        let source = Size::new(480.0, 640.0);
        let out = burn_in(black_image(480, 640), source, &state);
        // Crosshair center: (240, 320).
        assert!(out.get_pixel(240, 320).0[0] > 200);
        // Grid positions for length 480, spacing 64: center 240 is a
        // multiple, so lines fall on 48,112,176,240,304,368,432.
        assert!(out.get_pixel(48, 500).0[1] > 200);
        assert_eq!(out.get_pixel(49, 500).0[0], 0);
    }

    /// A committed ruler line burns onto the photo at the output
    /// resolution — image x=160→x=480 lands at output x=400→x=1200 on
    /// the 1600×1200 interpolation (§6.9).
    #[test]
    fn burn_in_scales_measurements_to_output_resolution() {
        let mut state = OverlayState::default();
        state.measure.px_per_mm = None;
        state.measure.items.push(hiview_core::Measurement {
            id: 1,
            tool: hiview_core::MeasureTool::Ruler,
            shape: hiview_core::MeasureShape::Line {
                a: Point::new(160.0, 240.0),
                b: Point::new(480.0, 240.0),
            },
        });
        let source = Size::new(640.0, 480.0);

        // Measurement lines are drawn in cyan — blue+green high, red low.
        let native = burn_in(black_image(640, 480), source, &state);
        let mid = native.get_pixel(320, 240).0;
        assert!(mid[1] > 200 && mid[2] > 200 && mid[0] < 120, "got {mid:?}");

        let upscaled = burn_in(black_image(1600, 1200), source, &state);
        let mid = upscaled.get_pixel(800, 600).0;
        assert!(mid[1] > 200 && mid[2] > 200 && mid[0] < 120, "got {mid:?}");
        // Away from the line the photo stays untouched.
        assert_eq!(upscaled.get_pixel(800, 620).0, BLACK);
    }

    /// Ticks style still centers the cross and adds tick marks.
    #[test]
    fn burn_in_renders_scale_ticks() {
        let mut state = OverlayState::default();
        state.crosshair.enabled = true;
        state.crosshair.style = CrosshairStyle::Ticks;
        state.crosshair.color = OverlayColor::WHITE;
        state.crosshair.tick_spacing = GridSpacing::Pixels(64.0);
        let out = burn_in(black_image(640, 480), Size::new(640.0, 480.0), &state);
        // Cross center.
        assert!(out.get_pixel(320, 240).0[0] > 200);
        // Tick at image x=256 on the horizontal arm: vertical mark of
        // ±3 image px around y=240 → (256, 242) is painted, (256, 245) not.
        assert!(out.get_pixel(256, 242).0[0] > 200);
        assert_eq!(out.get_pixel(256, 245).0, BLACK);
    }
}
