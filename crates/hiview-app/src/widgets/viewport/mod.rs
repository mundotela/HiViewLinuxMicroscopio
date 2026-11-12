// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Custom live-video viewport (docs/ARCHITECTURE.md §6.5): draws the
//! current frame with `append_scaled_texture` under a [`ViewTransform`]
//! (fit, 100%, cursor-centered zoom, drag pan), then overlays with
//! `append_cairo` via [`crate::overlay::OverlayRenderer`] (§6.9).
//!
//! Gestures (docs/USER_MANUAL.md §6): mouse wheel zooms around the cursor,
//! dragging pans, double-click toggles between fit and 100%.

mod imp;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;

use hiview_capture::Frame;
use hiview_core::{OverlayState, Size, ViewTransform};

glib::wrapper! {
    /// Widget that renders camera frames with a view transform.
    pub struct Viewport(ObjectSubclass<imp::Viewport>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Viewport {
    /// Creates an empty viewport.
    #[must_use]
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Uploads a frame and draws it, unless frozen. Frame-size changes
    /// (e.g. a 90° rotation swaps 640×480 to 480×640) refit or re-clamp
    /// the transform.
    pub fn set_frame(&self, frame: Frame) {
        let imp = self.imp();
        if imp.frozen.get() {
            return;
        }
        let texture = gdk::MemoryTexture::new(
            frame.width as i32,
            frame.height as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &frame.data,
            frame.stride,
        );
        let new_size = Size::new(f64::from(frame.width), f64::from(frame.height));
        if new_size != imp.frame_size.get() {
            imp.frame_size.replace(new_size);
            let widget = imp.widget_size();
            // Defer the refit while the widget still has no real
            // allocation; Fit mode recomputes on size_allocate.
            if widget.width > 0.0 {
                imp.transform.borrow_mut().on_resize(new_size, widget);
            }
        }
        imp.frame.replace(Some(frame));
        imp.texture.replace(Some(texture));
        self.queue_draw();
    }

    /// Freezes/unfreezes the displayed frame. The pipeline keeps streaming;
    /// while frozen `set_frame` is ignored, so the frozen pixels stay.
    pub fn set_frozen(&self, frozen: bool) {
        self.imp().frozen.set(frozen);
    }

    /// The frame currently displayed while frozen, if any.
    #[must_use]
    pub fn frozen_frame(&self) -> Option<Frame> {
        let imp = self.imp();
        if imp.frozen.get() {
            imp.frame.borrow().clone()
        } else {
            None
        }
    }

    /// The frame a photo should capture (§6.6): the frozen frame while
    /// frozen, otherwise the latest displayed frame.
    #[must_use]
    pub fn photo_frame(&self) -> Option<Frame> {
        self.frozen_frame()
            .or_else(|| self.imp().frame.borrow().clone())
    }

    /// Zooms in around the widget center (`+` key/button).
    pub fn zoom_in(&self) {
        self.imp().zoom_at_center(imp::ZOOM_STEP);
    }

    /// Zooms out around the widget center (`-` key/button).
    pub fn zoom_out(&self) {
        self.imp().zoom_at_center(1.0 / imp::ZOOM_STEP);
    }

    /// Fits the frame to the window (`0` key).
    pub fn fit(&self) {
        let imp = self.imp();
        imp.transform
            .replace(ViewTransform::fit(imp.frame_size.get(), imp.widget_size()));
        self.queue_draw();
    }

    /// Shows the frame at native size (`1` key).
    pub fn zoom_100(&self) {
        let imp = self.imp();
        imp.transform.replace(ViewTransform::zoom_100(
            imp.frame_size.get(),
            imp.widget_size(),
        ));
        self.queue_draw();
    }

    /// Resets to fit mode, unfreezes and drops the stale frame (e.g.
    /// when a stream restarts). Clearing the frame size forces a refit
    /// on the next frame even when it matches the old size.
    pub fn reset(&self) {
        let imp = self.imp();
        imp.frozen.set(false);
        imp.texture.replace(None);
        imp.frame.replace(None);
        imp.frame_size.set(Size::default());
        imp.transform.replace(ViewTransform::default());
        self.queue_draw();
    }

    /// Sets the overlays drawn over the frame (§6.9), or `None` to hide
    /// them (Explorar mode, or every overlay toggled off).
    pub fn set_overlays(&self, overlays: Option<OverlayState>) {
        self.imp().overlays.replace(overlays);
        self.queue_draw();
    }

    /// Current transform, for tests/debug.
    #[must_use]
    pub fn transform(&self) -> ViewTransform {
        *self.imp().transform.borrow()
    }

    /// Current zoom level as a percentage, for the toolbar label.
    #[must_use]
    pub fn zoom_percent(&self) -> i32 {
        (self.imp().transform.borrow().scale * 100.0).round() as i32
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}
