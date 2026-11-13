// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Noise reduction by frame averaging (M10): N consecutive frames are
//! summed per byte in `u32` and divided with round-half-up
//! (`(sum + N/2) / N`). Integer math is bit-exact and faster than the
//! `f32` accumulator sketched in §6.6 — the report proposes the doc
//! change. Averaging runs on the post-adjust frames as published on
//! the FrameBus: LUT stages are per-pixel and unsharp is linear, so
//! `avg(adjust(x)) ≈ adjust(avg(x))` up to clamping — no "raw" variant.

/// Why a frame could not be folded into the average.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AverageError {
    /// Frame geometry differs from the first frame's (rotation,
    /// resolution switch mid-capture — the capture is cancelled).
    #[error(
        "frame size changed mid-capture: {expected_w}x{expected_h} stride {expected_stride}, got {got_w}x{got_h} stride {got_stride}"
    )]
    DimensionMismatch {
        /// Width locked by the first frame.
        expected_w: u32,
        /// Height locked by the first frame.
        expected_h: u32,
        /// Row stride locked by the first frame.
        expected_stride: usize,
        /// Width of the rejected frame.
        got_w: u32,
        /// Height of the rejected frame.
        got_h: u32,
        /// Row stride of the rejected frame.
        got_stride: usize,
    },

    /// Buffer shorter than `stride * height`.
    #[error("frame buffer too short: {len} bytes, need at least {needed}")]
    BadLength {
        /// Bytes actually provided.
        len: usize,
        /// Bytes required (`stride * height`).
        needed: usize,
    },

    /// [`FrameAverager::finish`] called before any [`FrameAverager::push`].
    #[error("no frames to average")]
    Empty,
}

/// Accumulates RGBA frames of one geometry into a per-byte `u32` sum.
///
/// `u32` holds 255 × ~16.8 M frames, far beyond the 16-frame menu —
/// overflow is not a concern at any realistic capture length.
#[derive(Default)]
pub struct FrameAverager {
    dims: Option<(u32, u32, usize)>,
    sum: Vec<u32>,
    count: u32,
}

impl FrameAverager {
    /// An empty accumulator; the first `push` locks the geometry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Frames folded in so far.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Adds one frame. The first call fixes `width × height × stride`;
    /// later frames must match exactly or the capture is invalid —
    /// a resolution change mid-capture cannot be averaged.
    ///
    /// `data` may carry a tail beyond `stride * height`; only the
    /// declared rows participate. Row padding bytes are summed too —
    /// nobody reads them downstream and averaging the whole buffer is
    /// the cheapest correct definition.
    ///
    /// # Errors
    /// [`AverageError::DimensionMismatch`] when geometry differs from
    /// the first frame, [`AverageError::BadLength`] when the buffer
    /// cannot hold `stride * height` bytes.
    pub fn push(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
        stride: usize,
    ) -> Result<(), AverageError> {
        let needed = stride
            .checked_mul(height as usize)
            .ok_or(AverageError::BadLength {
                len: data.len(),
                needed: usize::MAX,
            })?;
        if data.len() < needed {
            return Err(AverageError::BadLength {
                len: data.len(),
                needed,
            });
        }
        match self.dims {
            None => {
                self.sum = vec![0; needed];
                self.dims = Some((width, height, stride));
            }
            Some((w, h, s)) if (w, h, s) != (width, height, stride) => {
                return Err(AverageError::DimensionMismatch {
                    expected_w: w,
                    expected_h: h,
                    expected_stride: s,
                    got_w: width,
                    got_h: height,
                    got_stride: stride,
                });
            }
            _ => {}
        }
        for (acc, &v) in self.sum.iter_mut().zip(&data[..needed]) {
            *acc += u32::from(v);
        }
        self.count += 1;
        Ok(())
    }

    /// The rounded mean of every pushed frame: `(sum + count/2) /
    /// count` per byte — round-half-up, bit-exact. The buffer keeps
    /// the original `stride * height` layout.
    ///
    /// # Errors
    /// [`AverageError::Empty`] when nothing was pushed.
    pub fn finish(&self) -> Result<Vec<u8>, AverageError> {
        let n = self.count;
        if n == 0 {
            return Err(AverageError::Empty);
        }
        let half = n / 2;
        Ok(self.sum.iter().map(|&s| ((s + half) / n) as u8).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 4;
    const H: u32 = 2;
    const STRIDE: usize = (W * 4) as usize;
    const BYTES: usize = STRIDE * H as usize;

    /// A frame filled with `v`.
    fn frame(v: u8) -> Vec<u8> {
        vec![v; BYTES]
    }

    /// Deterministic byte noise: a tiny xorshift32 so tests need no
    /// `rand` dependency.
    struct XorShift(u32);

    impl XorShift {
        fn next(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            x
        }
    }

    /// Sample standard deviation of a buffer.
    fn stddev(data: &[u8]) -> f64 {
        let mean = data.iter().map(|&v| f64::from(v)).sum::<f64>() / data.len() as f64;
        let var = data
            .iter()
            .map(|&v| {
                let d = f64::from(v) - mean;
                d * d
            })
            .sum::<f64>()
            / data.len() as f64;
        var.sqrt()
    }

    /// Averaging N copies of one frame returns the frame itself —
    /// the operation is a no-op on a still scene (spec test 1).
    #[test]
    fn identical_frames_average_to_themselves() {
        let data = frame(137);
        let mut avg = FrameAverager::new();
        for _ in 0..16 {
            avg.push(&data, W, H, STRIDE)
                .unwrap_or_else(|e| panic!("push: {e}"));
        }
        assert_eq!(avg.count(), 16);
        assert_eq!(avg.finish().unwrap_or_else(|e| panic!("{e}")), data);
    }

    /// Round-half-up, documented per spec test 2: with N=4 and byte
    /// values 0/1, sums 0..=4 map to 0,0,1,1,1 — `(sum + 2) / 4`.
    #[test]
    fn rounding_is_half_up() {
        for (ones, expected) in [(0u8, 0u8), (1, 0), (2, 1), (3, 1), (4, 1)] {
            let mut avg = FrameAverager::new();
            for _ in 0..ones {
                avg.push(&frame(1), W, H, STRIDE)
                    .unwrap_or_else(|e| panic!("{e}"));
            }
            for _ in ones..4 {
                avg.push(&frame(0), W, H, STRIDE)
                    .unwrap_or_else(|e| panic!("{e}"));
            }
            assert_eq!(
                avg.finish().unwrap_or_else(|e| panic!("{e}"))[0],
                expected,
                "{ones} ones out of 4 should average to {expected}"
            );
        }
    }

    /// Zero-mean noise (base ± toggle) must shrink by ~√N (±15%) —
    /// the whole point of the feature (spec test 3).
    #[test]
    fn noise_shrinks_by_sqrt_n() {
        const N: u32 = 16;
        let mut rng = XorShift(0x9E37_79B9);
        let mut frames = Vec::new();
        let mut avg = FrameAverager::new();
        for _ in 0..N {
            let data: Vec<u8> = (0..4096)
                .map(|_| 128 + (rng.next() % 41) as u8 - 20) // ±20 uniform
                .collect();
            avg.push(&data, 32, 32, 128)
                .unwrap_or_else(|e| panic!("{e}"));
            frames.push(data);
        }
        let single = stddev(&frames[0]);
        let pooled = stddev(&avg.finish().unwrap_or_else(|e| panic!("{e}")));
        let expected = single / (N as f64).sqrt();
        let tolerance = expected * 0.15;
        assert!(
            (pooled - expected).abs() <= tolerance,
            "sigma {pooled:.2} not within 15% of {expected:.2} (single {single:.2})"
        );
    }

    /// A mid-capture geometry change is an error, never a panic —
    /// the caller cancels the capture (spec test 4).
    #[test]
    fn dimension_change_is_an_error() {
        let mut avg = FrameAverager::new();
        avg.push(&frame(7), W, H, STRIDE)
            .unwrap_or_else(|e| panic!("{e}"));
        let err = match avg.push(&frame(7), H, W, (H * 4) as usize) {
            Err(e) => e,
            Ok(()) => panic!("expected DimensionMismatch"),
        };
        assert!(matches!(err, AverageError::DimensionMismatch { .. }));
        // The accumulator still holds only the first frame.
        assert_eq!(avg.count(), 1);
        assert_eq!(avg.finish().unwrap_or_else(|e| panic!("{e}")), frame(7));
    }

    /// Buffers too short for the geometry are rejected before summing.
    #[test]
    fn short_buffer_is_rejected() {
        let mut avg = FrameAverager::new();
        let short = vec![0u8; BYTES - 1];
        assert!(matches!(
            avg.push(&short, W, H, STRIDE),
            Err(AverageError::BadLength { .. })
        ));
        assert_eq!(avg.count(), 0);
        assert_eq!(avg.finish(), Err(AverageError::Empty));
    }

    /// Extra bytes past `stride * height` are ignored, not rejected.
    #[test]
    fn trailing_tail_is_ignored() {
        let mut data = frame(200);
        data.extend_from_slice(&[9, 9, 9]);
        let mut avg = FrameAverager::new();
        avg.push(&data, W, H, STRIDE)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(avg.finish().unwrap_or_else(|e| panic!("{e}")).len(), BYTES);
    }
}
