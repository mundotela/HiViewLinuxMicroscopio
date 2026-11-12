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
//! 3. sharpen — 3×3 unsharp mask on **luma only**, integer math,
//!    borders replicated. The luma delta is added back equally to
//!    R, G and B: ~⅓ of the per-channel work and no coloured
//!    fringes at edges (hue stays put by construction).

use crate::ImageAdjust;

/// Luma coefficients for Rec.601 scaled by 256 (they sum to 256, so a
/// single `>> 8` normalizes): 0.299/0.587/0.114.
// u16 suffices for the luma dot product: 255·(77+150+29) = 65280.
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
    /// row, `width`×`height` pixels, alpha untouched). With sharpen on,
    /// [`Self::apply_fused`] produces every stage in one pass; otherwise
    /// only the per-pixel stage runs. Everything runs on the calling
    /// thread — the §7 CPU budget counts the time of every thread, so
    /// workers could hide latency but never cost.
    pub fn apply(&self, data: &mut [u8], width: usize, height: usize, stride: usize) {
        if self.sharpen_q != 0 {
            self.apply_fused(data, width, height, stride);
        } else if self.lut.is_some() || self.sat_q != 256 {
            self.apply_lut_and_saturation(data, width, height, stride);
        }
    }

    /// The whole plan in one pass when sharpen is on: a three-row ring
    /// holds the stage-1 pixels of rows y−1, y, y+1 and a parallel ring
    /// their BT.601 luma — as it advances, the next row's stage-1
    /// version is produced on the fly and the unsharp output of the
    /// current row is written back. No extra pass over the frame and
    /// no full-frame copy: the working set stays hot in cache, which
    /// is what the §7 CPU budget sees.
    ///
    /// Unsharp mask on **luma**: `Δ = (4·Y − ΣY4)·q/1024` (q =
    /// sharpen_q, so 100 % reproduces the classic full-strength
    /// unsharp), then `out[c] = stage1[c] + Δ` — the same delta for
    /// all three channels keeps every channel difference, so hue can
    /// never drift at an edge. Border rows and columns replicate the
    /// edge pixel.
    fn apply_fused(&self, data: &mut [u8], width: usize, height: usize, stride: usize) {
        // ring[r % 3]/lring[r % 3] hold the stage-1 pixels and luma of
        // row r. Row 0 doubles as the clamped `up` row of y=0.
        let mut ring = [vec![0u8; stride], vec![0u8; stride], vec![0u8; stride]];
        let mut lring = [vec![0u8; width], vec![0u8; width], vec![0u8; width]];
        let mut lrow4 = vec![0u8; stride];
        let mut delta4 = vec![0i16; stride];
        self.stage1_row(
            &data[..stride],
            &mut ring[0],
            &mut lring[0],
            &mut lrow4,
            width,
        );
        let q = self.sharpen_q;
        for y in 0..height {
            // Row y+1 is still untouched in `data`; write its stage-1
            // version into the ring before row y is overwritten. At the
            // last row `dn` clamps to y itself.
            let dn_row = (y + 1).min(height - 1);
            self.stage1_row(
                &data[dn_row * stride..dn_row * stride + stride],
                &mut ring[dn_row % 3],
                &mut lring[dn_row % 3],
                &mut lrow4,
                width,
            );
            let (up_l, me_l, dn_l) = (
                &lring[y.saturating_sub(1) % 3],
                &lring[y % 3],
                &lring[dn_row % 3],
            );
            let me = &ring[y % 3];
            let out = &mut data[y * stride..y * stride + stride];

            // Interior luma deltas, expanded to `[d,d,d,0]` per pixel
            // so the output pass is a flat byte loop and alpha passes
            // through for free — one zipped pass over the five shifted
            // slices, no per-pixel indexing or bounds checks.
            if width >= 3 {
                for (d4, ((((c, l), r), u), dn)) in delta4[4..width * 4 - 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(
                        me_l[1..width - 1]
                            .iter()
                            .zip(&me_l[..width - 2])
                            .zip(&me_l[2..width])
                            .zip(&up_l[1..width - 1])
                            .zip(&dn_l[1..width - 1]),
                    )
                {
                    let edge = 4 * i32::from(*c)
                        - (i32::from(*l) + i32::from(*r) + i32::from(*u) + i32::from(*dn));
                    let dv = ((edge * q) >> 10) as i16;
                    d4[0] = dv;
                    d4[1] = dv;
                    d4[2] = dv;
                    d4[3] = 0;
                }
            }
            // Edge columns replicate the border pixel (l=c at x=0,
            // r=c at x=w−1).
            {
                let c = i32::from(me_l[0]);
                let r = i32::from(me_l[usize::from(width > 1)]);
                let edge = 3 * c - (r + i32::from(up_l[0]) + i32::from(dn_l[0]));
                let dv = ((edge * q) >> 10) as i16;
                delta4[0] = dv;
                delta4[1] = dv;
                delta4[2] = dv;
                delta4[3] = 0;
            }
            if width > 1 {
                let x = width - 1;
                let c = i32::from(me_l[x]);
                let l = i32::from(me_l[x - 1]);
                let edge = 3 * c - (l + i32::from(up_l[x]) + i32::from(dn_l[x]));
                let dv = ((edge * q) >> 10) as i16;
                delta4[x * 4] = dv;
                delta4[x * 4 + 1] = dv;
                delta4[x * 4 + 2] = dv;
                delta4[x * 4 + 3] = 0;
            }

            // Write RGBA: stage-1 pixel + the shared luma delta — a
            // flat byte loop the compiler can vectorize; alpha gets
            // delta 0 and passes through.
            for ((s, d), &dv) in me[..width * 4]
                .iter()
                .zip(out[..width * 4].iter_mut())
                .zip(delta4.iter())
            {
                *d = (i16::from(*s) + dv).clamp(0, 255) as u8;
            }
        }
    }

    /// Writes the stage-1 version of `srow` into `drow` and its luma
    /// into `lrow` — flat slice passes so the compiler can vectorize
    /// them. Luma is invariant under the saturation mix (it blends
    /// around luma itself), so `lrow` is computed before it and stays
    /// correct for the unsharp ring. `lrow4` is scratch for the mix.
    fn stage1_row(
        &self,
        srow: &[u8],
        drow: &mut [u8],
        lrow: &mut [u8],
        lrow4: &mut [u8],
        width: usize,
    ) {
        if let Some(lut) = &self.lut {
            for (s, d) in srow[..width * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .zip(drow[..width * 4].as_chunks_mut::<4>().0.iter_mut())
            {
                d[0] = lut[usize::from(s[0])];
                d[1] = lut[usize::from(s[1])];
                d[2] = lut[usize::from(s[2])];
                d[3] = s[3];
            }
        } else {
            drow.copy_from_slice(srow);
        }
        luma_row(drow, lrow, width);
        if self.sat_q != 256 {
            for ((l4, &y), d) in lrow4[..width * 4]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(lrow.iter())
                .zip(drow[..width * 4].as_chunks::<4>().0.iter())
            {
                l4[0] = y;
                l4[1] = y;
                l4[2] = y;
                l4[3] = d[3];
            }
            self.satmix_row(drow, lrow4, width);
        }
    }

    /// In-place saturation mix on one row: `out = (c·q + l·(256−q))>>8`
    /// per byte — bit-exact with `luma + (c−luma)·q>>8` because `256·l`
    /// is a multiple of the shift. The alpha byte reads `l = a`, so it
    /// survives untouched for free.
    ///
    /// For sat ≤ 1 (`qm ≥ 0`) both products fit u16 and the sum is
    /// computed via the exact low-byte carry split — `(a+b)>>8 ==
    /// (a>>8)+(b>>8)+((a&255)+(b&255))>>8` — so the whole loop runs in
    /// 16-bit lanes, and its result is provably in `0..=255` (weights
    /// sum to 256), needing no clamp.
    fn satmix_row(&self, drow: &mut [u8], lrow4: &[u8], width: usize) {
        let (q, qm) = (self.sat_q, 256 - self.sat_q);
        if qm >= 0 {
            let (q, qm) = (q as u16, qm as u16);
            for (px, &l) in drow[..width * 4].iter_mut().zip(lrow4.iter()) {
                // Exact 17-bit sum in u16 lanes: `(a+b)>>8` equals
                // `((a+b)>>1)>>7`, and the half-sum `a/2+b/2+(a&b&1)`
                // never exceeds 65281 — no carry into a wider lane.
                let a = u16::from(*px) * q;
                let b = u16::from(l) * qm;
                *px = (((a >> 1) + (b >> 1) + (a & b & 1)) >> 7) as u8;
            }
        } else {
            for (px, &l) in drow[..width * 4].iter_mut().zip(lrow4.iter()) {
                *px = clamp_u8((i32::from(*px) * q + i32::from(l) * qm) >> 8);
            }
        }
    }

    /// LUT then saturation, in place — used when unsharp is off but at
    /// least one pointwise stage is active. Same flat passes as
    /// [`Self::stage1_row`], minus the ring/luma outputs.
    fn apply_lut_and_saturation(
        &self,
        data: &mut [u8],
        width: usize,
        height: usize,
        stride: usize,
    ) {
        let mut lrow = vec![0u8; width];
        let mut lrow4 = vec![0u8; stride];
        for y in 0..height {
            let row = &mut data[y * stride..y * stride + stride];
            if let Some(lut) = &self.lut {
                for px in row[..width * 4].as_chunks_mut::<4>().0 {
                    px[0] = lut[usize::from(px[0])];
                    px[1] = lut[usize::from(px[1])];
                    px[2] = lut[usize::from(px[2])];
                }
            }
            if self.sat_q != 256 {
                luma_row(row, &mut lrow, width);
                for ((l4, &y), px) in lrow4[..width * 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(lrow.iter())
                    .zip(row[..width * 4].as_chunks::<4>().0.iter())
                {
                    l4[0] = y;
                    l4[1] = y;
                    l4[2] = y;
                    l4[3] = px[3];
                }
                self.satmix_row(row, &lrow4, width);
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

/// BT.601 luma of one RGB pixel, 0–255.
fn luma(r: u8, g: u8, b: u8) -> u8 {
    // u16 arithmetic: the dot product tops out at 65280 and every term
    // is positive, so it vectorizes at twice i32's lane width.
    ((u16::from(r) * LUMA_R as u16 + u16::from(g) * LUMA_G as u16 + u16::from(b) * LUMA_B as u16)
        >> 8) as u8
}

/// The luma row of a stage-1 RGBA row — one zipped pass.
fn luma_row(srow: &[u8], lrow: &mut [u8], width: usize) {
    for (s, l) in srow[..width * 4]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(lrow.iter_mut())
    {
        *l = luma(s[0], s[1], s[2]);
    }
}

fn clamp_u8(v: i32) -> u8 {
    // `as u8` after the clamp is exact — and branch-free.
    v.clamp(0, 255) as u8
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

    /// Luma-only unsharp adds one shared delta to R, G and B, so
    /// channel differences survive untouched — a sharpened coloured
    /// edge can never pick up coloured fringes (no hue shift). The
    /// bit-exact clip rails are the only exception; mid-range pixels
    /// keep their hue exactly.
    #[test]
    fn sharpen_preserves_hue_on_colored_edge() {
        const W: usize = 8;
        const H: usize = 5;
        let color = |x: usize| -> [i32; 3] {
            if x < W / 2 {
                [200, 80, 90]
            } else {
                [40, 60, 180]
            }
        };
        let mut frame = vec![0u8; W * H * 4];
        for y in 0..H {
            for x in 0..W {
                let i = (y * W + x) * 4;
                let [r, g, b] = color(x);
                frame[i] = r as u8;
                frame[i + 1] = g as u8;
                frame[i + 2] = b as u8;
                frame[i + 3] = 255;
            }
        }
        let plan = AdjustPlan::compile(&ImageAdjust {
            sharpen: 100.0,
            ..adjust()
        })
        .unwrap_or_else(|| panic!("plan"));
        plan.apply(&mut frame, W, H, W * 4);
        let mut moved = 0;
        for y in 0..H {
            for x in 0..W {
                let i = (y * W + x) * 4;
                let [r0, g0, b0] = color(x);
                let (r1, g1, b1) = (
                    i32::from(frame[i]),
                    i32::from(frame[i + 1]),
                    i32::from(frame[i + 2]),
                );
                if (r1, g1, b1) != (r0, g0, b0) {
                    moved += 1;
                }
                if r1 > 0 && r1 < 255 && g1 > 0 && g1 < 255 && b1 > 0 && b1 < 255 {
                    assert_eq!(r1 - g1, r0 - g0, "({x},{y}) R−G drifted");
                    assert_eq!(g1 - b1, g0 - b0, "({x},{y}) G−B drifted");
                }
            }
        }
        assert!(moved > 0, "edge pixels must move");
    }
}
