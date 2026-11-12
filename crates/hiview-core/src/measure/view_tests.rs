// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Gesture pipeline tests (§6.9): pointer positions on screen go
//! through `ViewTransform::widget_to_image` — exactly what the
//! viewport does — so a measurement must read the same image-space
//! value under fit, zoom+pan and on the 480×640 rotated frame.

#[cfg(test)]
mod tests {
    use crate::measure::{
        MeasureGesture, MeasureSet, MeasureShape, MeasureTool, angle_deg, distance,
    };
    use crate::view::{Point, Size, ViewTransform};

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    /// Feeds a click at `img_pt` as the viewport would: the widget
    /// position is mapped back through `widget_to_image`.
    fn click_image(set: &mut MeasureSet, view: &ViewTransform, img_pt: Point) {
        let widget = view.image_to_widget(img_pt);
        set.gesture(MeasureGesture::Click(view.widget_to_image(widget)));
    }

    /// The image-space length of the committed `n`-th ruler line.
    fn line_length(set: &MeasureSet) -> f64 {
        match set.items.last().map(|m| &m.shape) {
            Some(MeasureShape::Line { a, b }) => distance(*a, *b),
            _ => panic!("expected a line measurement"),
        }
    }

    #[test]
    fn ruler_measures_image_distance_under_fit() {
        // 640×480 frame fitted into 800×600 → scale 1.25, letterboxed.
        let view = ViewTransform::fit(Size::new(640.0, 480.0), Size::new(800.0, 600.0));
        let mut set = MeasureSet::default();
        set.arm(Some(MeasureTool::Ruler));
        click_image(&mut set, &view, p(100.0, 200.0));
        click_image(&mut set, &view, p(400.0, 200.0));
        assert!((line_length(&set) - 300.0).abs() < 1e-9);
    }

    #[test]
    fn ruler_measures_the_same_distance_zoomed_and_panned() {
        let frame = Size::new(640.0, 480.0);
        let widget = Size::new(800.0, 600.0);
        let mut view = ViewTransform::fit(frame, widget);
        view.zoom_at(p(400.0, 300.0), 2.0, frame, widget);
        view.pan_by(p(120.0, 80.0), frame, widget);

        let mut set = MeasureSet::default();
        set.arm(Some(MeasureTool::Ruler));
        click_image(&mut set, &view, p(100.0, 200.0));
        click_image(&mut set, &view, p(400.0, 200.0));
        assert!((line_length(&set) - 300.0).abs() < 1e-9);
    }

    #[test]
    fn angle_and_circle_on_the_rotated_frame() {
        // After a 90° rotation the displayed frame is 480×640 (§6.9);
        // measurements still read image coordinates of that frame.
        let view = ViewTransform::fit(Size::new(480.0, 640.0), Size::new(600.0, 800.0));
        let mut set = MeasureSet::default();

        set.arm(Some(MeasureTool::Angle));
        click_image(&mut set, &view, p(340.0, 320.0)); // right of vertex
        click_image(&mut set, &view, p(240.0, 320.0)); // vertex
        click_image(&mut set, &view, p(240.0, 420.0)); // below vertex
        match set.items.last().map(|m| &m.shape) {
            Some(MeasureShape::Angle { a, vertex, b }) => {
                assert!((angle_deg(*a, *vertex, *b) - 90.0).abs() < 1e-9);
            }
            _ => panic!("expected an angle measurement"),
        }

        set.arm(Some(MeasureTool::Circle));
        // The circle drags from its center to the edge — diameter = 2r.
        let center = view.image_to_widget(p(240.0, 320.0));
        set.gesture(MeasureGesture::DragStart(view.widget_to_image(center)));
        let edge = view.image_to_widget(p(290.0, 320.0));
        set.gesture(MeasureGesture::DragEnd(view.widget_to_image(edge)));
        match set.items.last().map(|m| &m.shape) {
            Some(MeasureShape::Circle { center, edge }) => {
                assert!((2.0 * distance(*center, *edge) - 100.0).abs() < 1e-9);
            }
            _ => panic!("expected a circle measurement"),
        }
    }
}
