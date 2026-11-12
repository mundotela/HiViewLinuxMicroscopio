// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
//
//! Single-pass software image adjustments (docs/ARCHITECTURE.md §6.11).
//!
//! The GStreamer filter elements (`videobalance`, `gamma`,
//! `gaussianblur`) could not meet the §7 CPU budget — the cheapest
//! nonzero gaussianblur kernel alone cost ~30% of a core. Instead the
//! capture crate applies this plan to each RGBA frame inside the
//! appsink callback, before the frame is published to the `FrameBus`,
//! so live view, photos and recordings all receive identical pixels.
//!
//! A [`AdjustPlan`] is compiled once per `set_adjust` call; per frame
//! only integer work runs. With every value at its default the plan is
//! `None` and the frame passes through untouched — zero cost.
//!
//! Stage order (fixed by design, matches the previous pipeline order):
//! 1. brightness + contrast + gamma — fused into one 256-entry LUT;
//! 2. saturation — blend with BT.601 luma, integer math;
//! 3. sharpen — 3×3 unsharp mask, integer math, borders replicated.

use crate::ImageAdjust;

/// Luma coefficients for Rec.601 scaled by 256 (they sum to 256, so a
/// single `>> 8` normalizes): 0.299/0.587/0.114.
const LUMA_R: i32 = 77;
const LUMA_G: i32 = 150;
const LUMA_B: i32 = 29;

/// Compiled adjustment: LUT + fixed-point factors.
#[derive(Debug, Clone)]
pub struct AdjustPlan {
    /// `None` when brightness/contrast/gamma are all neutral.
    lut: Option<Box<[u8; 256]>>,
    /// Saturation × 256 (256 = neutral).
    sat_q: i32,
    /// Sharpen strength × 256 (0 = off; the slider's 0–100%).
    sharpen_q: i32,
}

impl AdjustPlan {
    /// Builds the plan for `adjust`. Returns `None` when every field is
    /// at its neutral default — the caller then skips the pass and the
    /// frame costs nothing.
    #[must_use]
    pub fn compile(adjust: &ImageAdjust) -> Option<Self> {
        let a = adjust.clamped();
        let lut = needs_lut(&a).then(|| Box::new(build_lut(&a)));
        let sat_q = (a.saturation * 256.0).round() as i32;
        let sharpen_q = (a.sharpen * 256.0 / 100.0).round() as i32;
        if lut.is_none() && sat_q == 256 && sharpen_q == 0 {
            return None;
        }
        Some(Self {
            lut,
            sat_q,
            sharpen_q,
        })
    }

    /// Applies the plan in place over an RGBA8 frame (`stride` bytes per
    /// row, `width`×`height` pixels, alpha untouched).
    pub fn apply(&self, data: &mut [u8], width: usize, height: usize, stride: usize) {
        if self.lut.is_some() || self.sat_q != 256 {
            self.apply_lut_and_saturation(data, width, height, stride);
        }
        if self.sharpen_q != 0 {
            self.apply_unsharp(data, width, height, stride);
        }
    }

    /// Stage 1+2 fused per pixel: LUT on each channel, then pull each
    /// channel toward (sat < 1) or away from (sat > 1) its BT.601 luma.
    fn apply_lut_and_saturation(
        &self,
        data: &mut [u8],
        width: usize,
        height: usize,
        stride: usize,
    ) {
        for y in 0..height {
            let row = &mut data[y * stride..y * stride + width * 4];
            for px in row.as_chunks_mut::<4>().0 {
                if let Some(lut) = &self.lut {
                    px[0] = lut[usize::from(px[0])];
                    px[1] = lut[usize::from(px[1])];
                    px[2] = lut[usize::from(px[2])];
                }
                if self.sat_q != 256 {
                    let (r, g, b) = (i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
                    let luma = (r * LUMA_R + g * LUMA_G + b * LUMA_B) >> 8;
                    px[0] = clamp_u8(luma + (((r - luma) * self.sat_q) >> 8));
                    px[1] = clamp_u8(luma + (((g - luma) * self.sat_q) >> 8));
                    px[2] = clamp_u8(luma + (((b - luma) * self.sat_q) >> 8));
                }
            }
        }
    }

    /// Stage 3: unsharp mask `out = v + k·(v − avg4)` with
    /// `k = sharpen_q/256`. Computed as `v + (4v − Σ4)·q/1024` so 100 %
    /// reproduces the classic full-strength unsharp. Reads the
    /// post-stage-1 frame from a copy (neighbours must be the already
    /// adjusted values) and clamps neighbour indices at the borders.
    fn apply_unsharp(&self, data: &mut [u8], width: usize, height: usize, stride: usize) {
        let src = data.to_vec();
        let q = self.sharpen_q;
        for y in 0..height {
            let up = y.saturating_sub(1) * stride;
            let me = y * stride;
            let dn = (y + 1).min(height - 1) * stride;
            for x in 0..width {
                let lf = x.saturating_sub(1) * 4;
                let cc = x * 4;
                let rt = (x + 1).min(width - 1) * 4;
                for c in 0..3 {
                    let v = i32::from(src[me + cc + c]);
                    let edge = 4 * v
                        - (i32::from(src[up + cc + c])
                            + i32::from(src[dn + cc + c])
                            + i32::from(src[me + lf + c])
                            + i32::from(src[me + rt + c]));
                    data[me + cc + c] = clamp_u8(v + ((edge * q) >> 10));
                }
            }
        }
    }
}

/// LUT needed only when one of the three fused fields is off-default.
fn needs_lut(a: &ImageAdjust) -> bool {
    a.brightness != 0.0 || a.contrast != 1.0 || a.gamma != 1.0
}

/// Reference formula, applied in `videobalance`→`gamma` order:
/// `y = clamp(((x − ½)·contrast + ½ + brightness) ^ (1/γ))` with
/// `x = v/255` — contrast pivots on mid-gray, brightness offsets,
/// `gamma` `> 1` lifts midtones (same direction the `gamma` element had).
fn build_lut(a: &ImageAdjust) -> [u8; 256] {
    let mut lut = [0u8; 256];
    for (i, out) in lut.iter_mut().enumerate() {
        let x = i as f64 / 255.0;
        let x = (x - 0.5) * a.contrast + 0.5 + a.brightness;
        *out = (x.clamp(0.0, 1.0).powf(1.0 / a.gamma) * 255.0).round() as u8;
    }
    lut
}

fn clamp_u8(v: i32) -> u8 {
    u8::try_from(v.clamp(0, 255)).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adjust() -> ImageAdjust {
        ImageAdjust {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            sharpen: 0.0,
        }
    }

    /// Every field neutral → no plan at all (the zero-cost default path).
    #[test]
    fn defaults_compile_to_noop() {
        assert!(AdjustPlan::compile(&adjust()).is_none());
    }

    /// Each single non-default field must produce a plan.
    #[test]
    fn each_field_alone_compiles() {
        for kind in crate::ImageAdjust::KINDS {
            let mut a = adjust();
            let (min, max) = crate::range_of(kind);
            a.set(kind, min + (max - min) * 0.6);
            assert!(AdjustPlan::compile(&a).is_some(), "{kind:?}");
        }
    }

    /// LUT vs. the documented reference formula, checked at sample
    /// points with an independent implementation.
    #[test]
    fn lut_matches_reference_formula() {
        let a = ImageAdjust {
            brightness: 0.1,
            contrast: 1.4,
            gamma: 1.6,
            ..adjust()
        };
        let plan = AdjustPlan::compile(&a).unwrap_or_else(|| panic!("plan"));
        let lut = plan.lut.as_ref().unwrap_or_else(|| panic!("lut"));
        for v in [0u8, 32, 64, 128, 192, 255] {
            let x = f64::from(v) / 255.0;
            let x = (x - 0.5) * a.contrast + 0.5 + a.brightness;
            let expected = (x.clamp(0.0, 1.0).powf(1.0 / a.gamma) * 255.0).round() as u8;
            assert_eq!(lut[usize::from(v)], expected, "v={v}");
        }
    }

    /// Direction checks: brightness raises, gamma>1 lifts midtones,
    /// contrast<1 compresses toward mid-gray.
    #[test]
    fn lut_directions() {
        let bright = AdjustPlan::compile(&ImageAdjust {
            brightness: 0.3,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"));
        let lut = bright.lut.as_ref().unwrap_or_else(|| panic!("lut"));
        assert!(lut[128] > 128);

        let gamma = AdjustPlan::compile(&ImageAdjust {
            gamma: 2.0,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"));
        let lut = gamma.lut.as_ref().unwrap_or_else(|| panic!("lut"));
        assert!(lut[128] > 128);

        let contrast = AdjustPlan::compile(&ImageAdjust {
            contrast: 0.5,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"));
        let lut = contrast.lut.as_ref().unwrap_or_else(|| panic!("lut"));
        assert!(lut[200] < 200 && lut[200] > 128, "compressed toward mid");
    }

    /// A plan with only brightness active leaves the frame's alpha and
    /// applies the LUT to RGB.
    #[test]
    fn apply_changes_only_rgb() {
        let plan = AdjustPlan::compile(&ImageAdjust {
            brightness: 0.5,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"));
        let mut px = [10u8, 20, 30, 255];
        plan.apply(&mut px, 1, 1, 4);
        assert_eq!(px[3], 255);
        assert!(px[0] > 10 && px[1] > 20 && px[2] > 30);
    }

    /// Saturation 0 collapses every pixel to its luma (grayscale).
    #[test]
    fn saturation_zero_is_grayscale() {
        let plan = AdjustPlan::compile(&ImageAdjust {
            saturation: 0.0,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"));
        let mut px = [200u8, 40, 60, 255, 10, 20, 220, 255];
        plan.apply(&mut px, 2, 1, 8);
        assert_eq!(px[0], px[1]);
        assert_eq!(px[1], px[2]);
        let luma0 = (200 * LUMA_R + 40 * LUMA_G + 60 * LUMA_B) >> 8;
        assert_eq!(px[0] as i32, luma0);
        let luma1 = (10 * LUMA_R + 20 * LUMA_G + 220 * LUMA_B) >> 8;
        assert_eq!(px[4] as i32, luma1);
    }

    /// Unsharp on a step edge must increase the local gradient — pixels
    /// on the dark side go darker, on the bright side brighter.
    #[test]
    fn sharpen_increases_step_contrast() {
        const W: usize = 8;
        const H: usize = 5;
        let mut frame = vec![0u8; W * H * 4];
        for y in 0..H {
            for x in 0..W {
                let v = if x < W / 2 { 100u8 } else { 160 };
                let i = (y * W + x) * 4;
                frame[i] = v;
                frame[i + 1] = v;
                frame[i + 2] = v;
                frame[i + 3] = 255;
            }
        }
        let before = frame[(2 * W + 4) * 4] - frame[(2 * W + 3) * 4];
        let plan = AdjustPlan::compile(&ImageAdjust {
            sharpen: 100.0,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"));
        plan.apply(&mut frame, W, H, W * 4);
        let dark = frame[(2 * W + 3) * 4];
        let bright = frame[(2 * W + 4) * 4];
        assert!(
            dark < 100 && bright > 160,
            "edge overshoot: {dark}..{bright}"
        );
        assert!(bright - dark > before);
        // Far from the edge nothing moves.
        assert_eq!(frame[0], 100);
        assert_eq!(frame[(2 * W + 7) * 4], 160);
    }

    /// Sharpen below 100% weakens but does not change the direction.
    #[test]
    fn sharpen_scales_with_strength() {
        const W: usize = 6;
        const H: usize = 3;
        let make = || {
            let mut f = vec![0u8; W * H * 4];
            for y in 0..H {
                for x in 0..W {
                    let i = (y * W + x) * 4;
                    f[i] = if x < W / 2 { 100 } else { 160 };
                    f[i + 1] = f[i];
                    f[i + 2] = f[i];
                    f[i + 3] = 255;
                }
            }
            f
        };
        let mut half = make();
        AdjustPlan::compile(&ImageAdjust {
            sharpen: 50.0,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"))
        .apply(&mut half, W, H, W * 4);
        let half_dark = half[(H / 2 * W + 2) * 4];
        let mut full = make();
        AdjustPlan::compile(&ImageAdjust {
            sharpen: 100.0,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"))
        .apply(&mut full, W, H, W * 4);
        let full_dark = full[(H / 2 * W + 2) * 4];
        assert!(half_dark < 100 && half_dark > full_dark);
    }
}
