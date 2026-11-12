// SPDX-License-Identifier: GPL-3.0-or-later
// Temporary M7 benchmark: per-stage cost of the adjustment pass on
// 640x480 RGBA frames — median and p95 per frame (ARCHITECTURE §6.11
// budget: median <= 1.0 ms, p95 <= 2.0 ms with all five active).
use hiview_core::{AdjustPlan, ImageAdjust};
use std::time::Instant;

const W: usize = 640;
const H: usize = 480;
const FRAMES: usize = 400;

fn frame() -> Vec<u8> {
    // Pseudo-noise + a hard edge — representative work for every stage.
    let mut f = vec![0u8; W * H * 4];
    let mut s = 0x9E3779B9u32;
    for y in 0..H {
        for x in 0..W {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let i = (y * W + x) * 4;
            let v = if x < W / 2 { s as u8 } else { (s >> 8) as u8 };
            f[i] = v;
            f[i + 1] = (s >> 4) as u8;
            f[i + 2] = (s >> 12) as u8;
            f[i + 3] = 255;
        }
    }
    f
}

fn bench(name: &str, adjust: &ImageAdjust) {
    let Some(plan) = AdjustPlan::compile(adjust) else {
        println!("{name}: no plan (defaults)");
        return;
    };
    let mut data = frame();
    // Warm-up.
    for _ in 0..20 {
        plan.apply(&mut data, W, H, W * 4);
    }
    let mut ts = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let t = Instant::now();
        plan.apply(&mut data, W, H, W * 4);
        ts.push(t.elapsed().as_secs_f64() * 1e3);
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = ts[FRAMES / 2];
    let p95 = ts[FRAMES * 95 / 100];
    println!(
        "{name}: median={median:.3} ms  p95={p95:.3} ms  (min {:.3} / max {:.3})",
        ts[0],
        ts[FRAMES - 1]
    );
}

fn main() {
    let base = ImageAdjust::default();
    bench(
        "lut (b/c/g)",
        &ImageAdjust {
            brightness: 0.2,
            contrast: 1.3,
            gamma: 1.4,
            ..base
        },
    );
    bench(
        "saturation",
        &ImageAdjust {
            saturation: 1.4,
            ..base
        },
    );
    bench(
        "sharpen 100%",
        &ImageAdjust {
            sharpen: 100.0,
            ..base
        },
    );
    bench(
        "lut+sharpen",
        &ImageAdjust {
            brightness: 0.15,
            contrast: 1.3,
            gamma: 1.4,
            sharpen: 100.0,
            ..base
        },
    );
    bench(
        "sat+sharpen",
        &ImageAdjust {
            saturation: 1.3,
            sharpen: 100.0,
            ..base
        },
    );
    bench(
        "ALL FIVE",
        &ImageAdjust {
            brightness: 0.15,
            contrast: 1.3,
            saturation: 1.3,
            gamma: 1.4,
            sharpen: 100.0,
        },
    );
}
