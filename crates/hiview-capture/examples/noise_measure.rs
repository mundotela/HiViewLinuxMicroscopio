// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! M10 measurement tool (not part of the app): captures real frames
//! from the microscope, averages consecutive blocks of N and prints
//! the noise σ of a user-chosen region for N = 1/4/8/16 plus the
//! collection time of each block.
//!
//! Usage: `cargo run --release -p hiview-capture --example noise_measure -- [x y w h]`
//! Default region: centered 100×100. Point the microscope at a
//! SMOOTH, STILL surface first (spatial σ ≈ sensor noise only there).

use std::time::{Duration, Instant};

use hiview_capture::{CapturePipeline, CaptureSource, Frame, FrameBus};
use hiview_core::FrameAverager;

const FRAMES: usize = 16;

/// Spatial σ of an RGBA patch (R, G, B channels together; alpha skipped).
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

/// Per-pixel temporal mean of `frames` (f32 accumulator is fine for
/// a measurement tool — this is not the shipped code path).
fn temporal_mean(frames: &[Frame]) -> Vec<f32> {
    let f0 = &frames[0];
    let mut mean = vec![0f32; f0.stride * f0.height as usize];
    for f in frames {
        for (m, &v) in mean.iter_mut().zip(&*f.data) {
            *m += f32::from(v) / frames.len() as f32;
        }
    }
    mean
}

/// σ of (frame − temporal mean) over the patch — per-pixel noise,
/// free of scene texture as long as the scene is still.
fn residual_sigma(frame: &Frame, mean: &[f32], x0: usize, y0: usize, w: usize, h: usize) -> f64 {
    let mut sq = 0f64;
    let mut n = 0f64;
    for y in y0..(y0 + h).min(frame.height as usize) {
        let row = &frame.data[y * frame.stride..];
        let mrow = &mean[y * frame.stride..];
        for x in x0..(x0 + w).min(frame.width as usize) {
            for c in 0..3 {
                let d = f32::from(row[x * 4 + c]) - mrow[x * 4 + c];
                sq += f64::from(d * d);
                n += 1.0;
            }
        }
    }
    (sq / n).sqrt()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (rx, ry, rw, rh) = if args.len() == 5 {
        (
            args[1].parse().unwrap_or_else(|_| panic!("x")),
            args[2].parse().unwrap_or_else(|_| panic!("y")),
            args[3].parse().unwrap_or_else(|_| panic!("w")),
            args[4].parse().unwrap_or_else(|_| panic!("h")),
        )
    } else {
        (270usize, 190usize, 100usize, 100usize)
    };

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

    let bus = FrameBus::default();
    let sub = bus.subscribe();
    let _pipe = CapturePipeline::start(CaptureSource::Device(desc.clone()), bus, |_| {})
        .unwrap_or_else(|e| panic!("pipeline: {e}"));

    // Warm-up: drop the first frames (auto-exposure settling).
    for _ in 0..15 {
        let _ = sub.recv_timeout(Duration::from_secs(2));
    }
    let mut frames = Vec::new();
    let t0 = Instant::now();
    let mut marks = [Duration::ZERO; 4];
    for i in 0..FRAMES {
        frames.push(
            sub.recv_timeout(Duration::from_secs(2))
                .unwrap_or_else(|e| panic!("frame {i}: {e}")),
        );
        for (j, &n) in [4usize, 8, 16].iter().enumerate() {
            if i + 1 == n {
                marks[j] = t0.elapsed();
            }
        }
    }
    let (w, h) = (frames[0].width, frames[0].height);
    println!(
        "{}x{}, region {}x{} at {rx},{ry}",
        w,
        h,
        rw.min(w as usize - rx),
        rh.min(h as usize - ry)
    );

    let mean = temporal_mean(&frames);
    let s1 = frames
        .iter()
        .map(|f| residual_sigma(f, &mean, rx, ry, rw, rh))
        .sum::<f64>()
        / FRAMES as f64;
    let spatial = frames
        .iter()
        .map(|f| patch_sigma(f, rx, ry, rw, rh))
        .sum::<f64>()
        / FRAMES as f64;
    println!("\nσ espacial do quadro (cena + ruído): {spatial:.2}");
    println!("| N | quadros | σ ruído | esperado σ₁/√N | coleta |");
    println!("|---|---------|---------|----------------|--------|");
    println!(
        "| 1 | 16 | {s1:.2} | — | {:.0} ms/frame |",
        marks[0].as_secs_f64() * 1000.0 / 4.0
    );
    for (n, mark) in [(4usize, marks[0]), (8, marks[1]), (16, marks[2])] {
        let sigma = frames
            .chunks(n)
            .map(|b| residual_sigma(&avg_of(b), &mean, rx, ry, rw, rh))
            .sum::<f64>()
            / (FRAMES / n) as f64;
        println!(
            "| {n} | {} | {sigma:.2} | {:.2} | {} ms |",
            FRAMES / n,
            s1 / (n as f64).sqrt(),
            mark.as_millis()
        );
    }
    println!("\ndropped frames on subscription: {}", sub.dropped_count());
}
