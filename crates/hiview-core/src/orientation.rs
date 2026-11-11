// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Video orientation state (docs/ARCHITECTURE.md §6.4): mirror H/V and
//! ±90° rotation composed into a single transform that `hiview-capture`
//! maps onto `videoflip`'s `video-direction`.

/// One of the eight dihedral transforms `videoflip` can apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Unchanged image.
    Identity,
    /// Rotated 90° clockwise.
    Rot90Cw,
    /// Rotated 180°.
    Rot180,
    /// Rotated 90° counterclockwise.
    Rot90Ccw,
    /// Mirrored horizontally.
    FlipH,
    /// Mirrored vertically.
    FlipV,
    /// Mirrored across the upper-left/lower-right diagonal.
    Transpose,
    /// Mirrored across the upper-right/lower-left diagonal.
    AntiTranspose,
}

/// What the user sees, expressed as independent toggles: mirror flags are
/// applied in screen space, after the rotation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewOrientation {
    quarters_cw: u8,
    mirror_h: bool,
    mirror_v: bool,
}

impl ViewOrientation {
    /// Rotates the view 90° clockwise (`]`).
    pub fn rotate_cw(&mut self) {
        self.quarters_cw = (self.quarters_cw + 1) % 4;
    }

    /// Rotates the view 90° counterclockwise (`[`).
    pub fn rotate_ccw(&mut self) {
        self.quarters_cw = (self.quarters_cw + 3) % 4;
    }

    /// Toggles horizontal mirroring of the displayed image (`H`).
    pub fn toggle_mirror_h(&mut self) {
        self.mirror_h = !self.mirror_h;
    }

    /// Toggles vertical mirroring of the displayed image (`V`).
    pub fn toggle_mirror_v(&mut self) {
        self.mirror_v = !self.mirror_v;
    }

    /// Quarter turns applied, clockwise.
    #[must_use]
    pub fn quarters_cw(&self) -> u8 {
        self.quarters_cw
    }

    /// The single `videoflip` method equivalent to the current state.
    ///
    /// Composition is `screen_mirror ∘ rotation`: the rotation turns the
    /// frame, then H/V mirroring is applied to what the user sees.
    #[must_use]
    pub fn method(&self) -> Orientation {
        const ROT: [Orientation; 4] = [
            Orientation::Identity,
            Orientation::Rot90Cw,
            Orientation::Rot180,
            Orientation::Rot90Ccw,
        ];
        let q = self.quarters_cw as usize;
        match (self.mirror_h, self.mirror_v) {
            (false, false) => ROT[q],
            // H then V (or vice versa) is a 180° rotation: adds two quarters.
            (true, true) => ROT[(q + 2) % 4],
            (true, false) => match q {
                0 => Orientation::FlipH,
                1 => Orientation::AntiTranspose,
                2 => Orientation::FlipV,
                _ => Orientation::Transpose,
            },
            (false, true) => match q {
                0 => Orientation::FlipV,
                1 => Orientation::Transpose,
                2 => Orientation::FlipH,
                _ => Orientation::AntiTranspose,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_identity() {
        assert_eq!(ViewOrientation::default().method(), Orientation::Identity);
    }

    #[test]
    fn four_rotations_return_to_identity() {
        let mut o = ViewOrientation::default();
        for _ in 0..4 {
            o.rotate_cw();
        }
        assert_eq!(o.method(), Orientation::Identity);
        for _ in 0..4 {
            o.rotate_ccw();
        }
        assert_eq!(o.method(), Orientation::Identity);
    }

    #[test]
    fn cw_and_ccw_cancel() {
        let mut o = ViewOrientation::default();
        o.rotate_cw();
        o.rotate_ccw();
        assert_eq!(o.method(), Orientation::Identity);
    }

    #[test]
    fn mirror_toggles_are_idempotent() {
        let mut o = ViewOrientation::default();
        o.toggle_mirror_h();
        assert_eq!(o.method(), Orientation::FlipH);
        o.toggle_mirror_h();
        assert_eq!(o.method(), Orientation::Identity);
        o.toggle_mirror_v();
        assert_eq!(o.method(), Orientation::FlipV);
        o.toggle_mirror_v();
        assert_eq!(o.method(), Orientation::Identity);
    }

    #[test]
    fn mirror_h_plus_v_equals_180() {
        let mut o = ViewOrientation::default();
        o.toggle_mirror_h();
        o.toggle_mirror_v();
        assert_eq!(o.method(), Orientation::Rot180);
    }

    #[test]
    fn mirror_after_rotation_is_a_diagonal_flip() {
        let mut o = ViewOrientation::default();
        o.rotate_cw();
        o.toggle_mirror_h();
        let method = o.method();
        assert!(
            matches!(method, Orientation::Transpose | Orientation::AntiTranspose),
            "expected a diagonal flip, got {method:?}"
        );
    }
}
