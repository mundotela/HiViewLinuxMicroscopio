// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! M10 measurement tool (not part of the app): captures real frames
//! from the microscope, averages consecutive blocks of N and prints
//! the noise σ of a user-chosen region for N = 1/4/8/16 plus the
//! collection time of each block.
//!
//! Reference is INDEPENDENT: a temporal mean of 64 frames captured
//! after the test set (no overlap) — a self-referential mean would
//! shrink residuals for large N (the M10 round-1 review caught
//! σ16 = 0.25 ≈ the quantization floor for exactly this reason).
//!
//! Usage: `cargo run --release -p hiview-capture --example noise_measure -- [x y w h]`
//! Default region: centered 100×100. Keep the scene STILL during the
//! ~5 s capture — the residual method is texture-free but not
//! motion-free.

use std::time::{Duration, Instant};

use hiview_capture::{CapturePipeline, CaptureSource, Frame, FrameBus};
use hiview_core::FrameAverager;

/// Frames in the test set: 5 blocks of the largest N (16).
const TEST_FRAMES: usize = 80;
/// Independent frames forming the temporal-mean reference.
const REF_FRAMES: usize = 64;

/// Spatial σ of an RGBA patch (R, G, B together; alpha skipped) —
/// reported once as scene+noise context, NOT the noise metric.
fn patch_sigma(frame: &Frame, x0: usize, y0: usize, w: usize, h: usize) -> f64 {
    let mut sum = 0f64;
    let mut sq = 0f64;
    let mut n = 0f64;
    for y in y0..(y0 + h).min(frame.height as usize) {
        let row = &frame.data[y * frame.stride..];
        for x in x0..(x0 + w).min(frame.width as usize) {
            for c in &row[x * 4..x * 4 + 3] {
                let v = f64::from(*c);
                sum += v;
                sq += v * v;
                n += 1.0;
            }
        }
    }
    (sq / n - (sum / n).powi(2)).sqrt()
}

fn avg_of(block: &[Frame]) -> Frame {
    let mut avg = FrameAverager::new();
    for f in block {
        avg.push(&f.data, f.width, f.height, f.stride)
            .unwrap_or_else(|e| panic!("push: {e}"));
    }
    let data = avg.finish().unwrap_or_else(|e| panic!("finish: {e}"));
    let last = block.last().unwrap_or_else(|| panic!("empty block"));
    Frame {
        width: last.width,
        height: last.height,
        stride: last.stride,
        format: last.format,
        data: gst::glib::Bytes::from_owned(data),
        pts: None,
    }
}

/// Per-pixel temporal mean of `frames` (f64 accumulator — measurement
/// tool, not the shipped path).
fn temporal_mean(frames: &[Frame]) -> Vec<f64> {
    let f0 = &frames[0];
    let mut mean = vec![0f64; f0.stride * f0.height as usize];
    for f in frames {
        for (m, &v) in mean.iter_mut().zip(&*f.data) {
            *m += f64::from(v) / frames.len() as f64;
        }
    }
    mean
}

/// σ of (frame − independent reference mean) over the patch.
fn residual_sigma(frame: &Frame, mean: &[f64], x0: usize, y0: usize, w: usize, h: usize) -> f64 {
    let mut sq = 0f64;
    let mut n = 0f64;
    for y in y0..(y0 + h).min(frame.height as usize) {
        let row = &frame.data[y * frame.stride..];
        let mrow = &mean[y * frame.stride..];
        for x in x0..(x0 + w).min(frame.width as usize) {
            for c in 0..3 {
                let d = f64::from(row[x * 4 + c]) - mrow[x * 4 + c];
                sq += d * d;
                n += 1.0;
            }
        }
    }
    (sq / n).sqrt()
}

fn stats(samples: &[f64]) -> (f64, f64) {
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let var = samples.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / samples.len() as f64;
    (mean, var.sqrt())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `--source-file <path>` replays a fixture (methodology sanity);
    // otherwise [x y w h] picks the patch on the live microscope.
    let file = args
        .iter()
        .position(|a| a == "--source-file")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    let (rx, ry, rw, rh) = (270usize, 190usize, 100usize, 100usize);

    let source = match &file {
        Some(path) => CaptureSource::File(path.clone()),
        None => {
            let cameras = hiview_capture::enumerate().unwrap_or_else(|e| panic!("enumerate: {e}"));
            let desc = cameras
                .iter()
                .find(|c| c.is_microscope)
                .unwrap_or_else(|| panic!("no microscope found"));
            println!(
                "device: {} ({})",
                desc.display_name(),
                desc.dev_node.display()
            );
            CaptureSource::Device(desc.clone())
        }
    };

    let bus = FrameBus::default();
    let sub = bus.subscribe();
    let _pipe =
        CapturePipeline::start(source, bus, |_| {}).unwrap_or_else(|e| panic!("pipeline: {e}"));

    // Warm-up: drop the first frames (auto-exposure settling).
    for _ in 0..15 {
        let _ = sub.recv_timeout(Duration::from_secs(2));
    }
    let grab = |count: usize| -> Vec<Frame> {
        (0..count)
            .map(|i| {
                sub.recv_timeout(Duration::from_secs(2))
                    .unwrap_or_else(|e| panic!("frame {i}: {e}"))
            })
            .collect()
    };

    let t0 = Instant::now();
    let mut test = Vec::new();
    let mut marks = [Duration::ZERO; 3];
    for i in 0..TEST_FRAMES {
        test.push(
            sub.recv_timeout(Duration::from_secs(2))
                .unwrap_or_else(|e| panic!("test frame {i}: {e}")),
        );
        for (j, &n) in [4usize, 8, 16].iter().enumerate() {
            if i + 1 == n {
                marks[j] = t0.elapsed();
            }
        }
    }
    let reference = grab(REF_FRAMES);
    let mean = temporal_mean(&reference);
    let (w, h) = (test[0].width, test[0].height);
    println!("{w}x{h}, region {rw}x{rh} at {rx},{ry}; test={TEST_FRAMES} ref={REF_FRAMES} frames");
    let spatial = test
        .iter()
        .map(|f| patch_sigma(f, rx, ry, rw, rh))
        .sum::<f64>()
        / TEST_FRAMES as f64;
    println!("σ espacial do quadro (cena + ruído): {spatial:.2}");

    // σ₁ measured on every test frame vs the independent reference.
    let s1_samples: Vec<f64> = test
        .iter()
        .map(|f| residual_sigma(f, &mean, rx, ry, rw, rh))
        .collect();
    let (s1_med, s1_std) = stats(&s1_samples);
    // The reference mean itself carries σ₁/√64 of noise; remove it in
    // quadrature: σ_med² = σ_true² + σ_ref². For N=1 this gives
    // σc₁ = σm₁·√(64/65).
    let s1 = s1_med * (64.0f64 / 65.0).sqrt();
    let ref_var = s1 * s1 / REF_FRAMES as f64;

    println!(
        "\n| N | blocos | σ medido (média±desvio) | σ corrigido | esperado σ₁/√N | ±20%? | coleta |"
    );
    println!(
        "|---|--------|-------------------------|-------------|----------------|-------|--------|"
    );
    println!("| 1 | {TEST_FRAMES} | {s1_med:.2}±{s1_std:.2} | {s1:.2} | — | — | ~33 ms/quadro |");
    for (n, mark) in [(4usize, marks[0]), (8, marks[1]), (16, marks[2])] {
        let samples: Vec<f64> = test
            .chunks(n)
            .map(|b| residual_sigma(&avg_of(b), &mean, rx, ry, rw, rh))
            .collect();
        let (med, std) = stats(&samples);
        let corrected = (med * med - ref_var).max(0.0).sqrt();
        let expected = s1 / (n as f64).sqrt();
        let within = (corrected - expected).abs() <= 0.20 * expected;
        println!(
            "| {n} | {} | {med:.2}±{std:.2} | {corrected:.2} | {expected:.2} | {} | {} ms |",
            TEST_FRAMES / n,
            if within { "sim" } else { "NÃO" },
            mark.as_millis()
        );
    }
    println!(
        "\nNota: σ corrigido não pode cair abaixo do piso de quantização          (~0,29 para arredondamento em inteiro); se o esperado σ₁/√N já for          menor, a tabela mede o piso, não o ruído real."
    );
    println!("dropped frames on subscription: {}", sub.dropped_count());
}
