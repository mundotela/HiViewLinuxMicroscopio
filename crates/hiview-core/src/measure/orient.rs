// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Remapping stored measurement points across orientation changes
//! (`H`, `V`, `[`, `]`) so they stay glued to the measured feature
//! (docs/ARCHITECTURE.md §6.4, §6.9).

use crate::orientation::Orientation;
use crate::view::{Point, Size};

/// Continuous-space map of one dihedral [`Orientation`] (the
/// `videoflip` method): returns the displayed point and the displayed
/// frame size for a raw point in a `size` raw frame. Pixel `i` spans
/// `[i, i+1)` — no ±0.5 anywhere, so mapping is exact at borders.
fn dihedral(o: Orientation, p: Point, size: Size) -> (Point, Size) {
    let (w, h) = (size.width, size.height);
    match o {
        Orientation::Identity => (p, size),
        Orientation::Rot90Cw => (Point::new(h - p.y, p.x), Size::new(h, w)),
        Orientation::Rot180 => (Point::new(w - p.x, h - p.y), size),
        Orientation::Rot90Ccw => (Point::new(p.y, w - p.x), Size::new(h, w)),
        Orientation::FlipH => (Point::new(w - p.x, p.y), size),
        Orientation::FlipV => (Point::new(p.x, h - p.y), size),
        // Main-diagonal flip: axes swap.
        Orientation::Transpose => (Point::new(p.y, p.x), Size::new(h, w)),
        // Anti-diagonal flip: axes swap and both coordinates reflect.
        Orientation::AntiTranspose => (Point::new(h - p.y, w - p.x), Size::new(h, w)),
    }
}

/// Whether `o` swaps the frame's axes (odd rotations, diagonal flips).
fn swaps_axes(o: Orientation) -> bool {
    matches!(
        o,
        Orientation::Rot90Cw
            | Orientation::Rot90Ccw
            | Orientation::Transpose
            | Orientation::AntiTranspose
    )
}

/// The inverse transform, used to walk a displayed point back to raw
/// coordinates. For every method the forward formula applied to the
/// *output* size is its own inverse shape — only rotations differ.
fn inverse(o: Orientation) -> Orientation {
    match o {
        Orientation::Rot90Cw => Orientation::Rot90Ccw,
        Orientation::Rot90Ccw => Orientation::Rot90Cw,
        other => other,
    }
}

/// Remaps a point in the displayed frame across an orientation change
/// (`H`, `V`, `[`, `]`), so stored measurements stay glued to the
/// physical feature.
///
/// `from`/`to` are `ViewOrientation::method()` before and after the
/// change; `displayed` is the frame size **before** it. The delta is
/// `to ∘ from⁻¹` in displayed coordinates — with the pipeline's
/// mirror-then-rotate composition a rotation is always a plain CW/CCW
/// step on screen, while a mirror toggled under rotation comes out as
/// the conjugated flip; computing it generically covers every case.
#[must_use]
pub fn remap_point(from: Orientation, to: Orientation, p: Point, displayed: Size) -> Point {
    let raw_size = if swaps_axes(from) {
        Size::new(displayed.height, displayed.width)
    } else {
        displayed
    };
    let (raw, _) = dihedral(inverse(from), p, displayed);
    dihedral(to, raw, raw_size).0
}

/// The displayed frame size after the same orientation change.
#[must_use]
pub fn remapped_size(from: Orientation, to: Orientation, displayed: Size) -> Size {
    if swaps_axes(from) == swaps_axes(to) {
        displayed
    } else {
        Size::new(displayed.height, displayed.width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::{MeasureShape, distance};

    const F: Size = Size {
        width: 640.0,
        height: 480.0,
    };

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }
    #[test]
    fn mirror_maps_points_inside_the_same_frame() {
        // Mirroring keeps the frame size; a point at x=100 of 640 lands
        // on the mirrored feature at x=540.
        let line = MeasureShape::Line {
            a: p(100.0, 200.0),
            b: p(200.0, 200.0),
        };
        let m = line.remapped(Orientation::Identity, Orientation::FlipH, F);
        assert_eq!(
            m,
            MeasureShape::Line {
                a: p(540.0, 200.0),
                b: p(440.0, 200.0)
            }
        );
        // Length is mirror-invariant.
        let MeasureShape::Line { a, b } = m else {
            panic!("line")
        };
        assert_eq!(distance(a, b), 100.0);
    }

    #[test]
    fn rotation_maps_points_into_the_swapped_frame() {
        // CW on 640×480 → 480×640: (x,y) → (480−y, x).
        let r = MeasureShape::Line {
            a: p(100.0, 100.0),
            b: p(200.0, 100.0),
        };
        let m = r.remapped(Orientation::Identity, Orientation::Rot90Cw, F);
        assert_eq!(
            remapped_size(Orientation::Identity, Orientation::Rot90Cw, F),
            Size::new(480.0, 640.0)
        );
        assert_eq!(
            m,
            MeasureShape::Line {
                a: p(380.0, 100.0),
                b: p(380.0, 200.0)
            }
        );
        // CCW: (x,y) → (y, 640−x).
        let m2 = r.remapped(Orientation::Identity, Orientation::Rot90Ccw, F);
        assert_eq!(
            m2,
            MeasureShape::Line {
                a: p(100.0, 540.0),
                b: p(100.0, 440.0)
            }
        );
    }

    #[test]
    fn mirror_toggled_under_rotation_is_the_conjugated_flip() {
        // With the pipeline's mirror-then-rotate composition, toggling
        // H at 90° CW changes the method Rot90Cw → AntiTranspose; in
        // displayed coordinates that delta is a vertical flip.
        let d = Size::new(480.0, 640.0);
        let q = remap_point(
            Orientation::Rot90Cw,
            Orientation::AntiTranspose,
            p(100.0, 200.0),
            d,
        );
        assert_eq!(q, p(100.0, 440.0));
        // Axes stay swapped through the change — size is preserved.
        assert_eq!(
            remapped_size(Orientation::Rot90Cw, Orientation::AntiTranspose, d),
            d
        );
    }

    #[test]
    fn identity_change_keeps_every_point() {
        for o in [
            Orientation::Identity,
            Orientation::Rot90Cw,
            Orientation::FlipH,
            Orientation::Transpose,
        ] {
            let size = if swaps_axes(o) {
                Size::new(F.height, F.width)
            } else {
                F
            };
            for q in [p(0.0, 0.0), p(37.5, 199.25), p(size.width, size.height)] {
                assert_eq!(remap_point(o, o, q, size), q);
            }
        }
    }
}
