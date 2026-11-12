// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

use std::cell::{Cell, RefCell};

use gtk::gdk;
use gtk::glib;
use gtk::graphene;
use gtk::gsk;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use hiview_capture::Frame;
use hiview_core::{NEAREST_FILTER_SCALE, OverlayState, Point, Size, ViewMode, ViewTransform};

use crate::overlay::OverlayRenderer;

/// Per-wheel-notch zoom factor (docs/USER_MANUAL.md §6: smooth steps).
pub(super) const ZOOM_STEP: f64 = 1.25;

#[derive(Default)]
pub struct Viewport {
    pub(super) texture: RefCell<Option<gdk::MemoryTexture>>,
    pub(super) frame: RefCell<Option<Frame>>,
    pub(super) frame_size: Cell<Size>,
    pub(super) transform: RefCell<ViewTransform>,
    pub(super) drag_start: RefCell<Option<ViewTransform>>,
    pub(super) frozen: Cell<bool>,
    /// Overlays to draw over the frame (§6.9); `None` hides them
    /// entirely (Explorar mode or everything toggled off).
    pub(super) overlays: RefCell<Option<OverlayState>>,
}

#[glib::object_subclass]
impl ObjectSubclass for Viewport {
    const NAME: &'static str = "HiViewViewport";
    type Type = super::Viewport;
    type ParentType = gtk::Widget;
}

impl ObjectImpl for Viewport {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();
        obj.set_hexpand(true);
        obj.set_vexpand(true);
        self.install_gestures();
    }
}

impl WidgetImpl for Viewport {
    fn snapshot(&self, snapshot: &gtk::Snapshot) {
        let Some(texture) = self.texture.borrow().clone() else {
            return;
        };
        // Copy the transform out — a borrow must never live across a
        // GTK call (re-entrant callbacks + RefCell panic = abort).
        let t = *self.transform.borrow();
        let bounds = graphene::Rect::new(
            t.offset.x as f32,
            t.offset.y as f32,
            self.frame_size.get().width as f32 * t.scale as f32,
            self.frame_size.get().height as f32 * t.scale as f32,
        );
        let filter = if t.scale < NEAREST_FILTER_SCALE {
            gsk::ScalingFilter::Linear
        } else {
            gsk::ScalingFilter::Nearest
        };
        snapshot.append_scaled_texture(&texture, filter, &bounds);
        // Overlay state is copied out too — a borrow must never live
        // across a GTK call (re-entrant callbacks + RefCell panic).
        if let Some(state) = self.overlays.borrow().clone() {
            let obj = self.obj();
            let bounds = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
            // The cairo node is appended when the context is dropped.
            let cr = snapshot.append_cairo(&bounds);
            let device_px = 1.0 / f64::from(obj.scale_factor().max(1));
            OverlayRenderer::draw(&cr, self.frame_size.get(), &t, &state, device_px);
        }
    }

    fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
        if self.frame_size.get().width <= 0.0 {
            return;
        }
        let mut t = self.transform.borrow_mut();
        let widget = Size::new(f64::from(width), f64::from(height));
        if t.mode == ViewMode::Fit {
            *t = ViewTransform::fit(self.frame_size.get(), widget);
        } else {
            t.clamp_offset(self.frame_size.get(), widget);
        }
    }
}

impl Viewport {
    fn install_gestures(&self) {
        let obj = self.obj();

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(glib::clone!(
            #[weak]
            obj,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |scroll, _dx, dy| {
                let factor = if dy > 0.0 { 1.0 / ZOOM_STEP } else { ZOOM_STEP };
                let anchor = scroll
                    .current_event()
                    .and_then(|e| e.position())
                    .map(|(x, y)| Point::new(x, y))
                    .unwrap_or_else(|| {
                        Point::new(f64::from(obj.width()) / 2.0, f64::from(obj.height()) / 2.0)
                    });
                obj.imp().zoom_at(anchor, factor);
                glib::Propagation::Stop
            }
        ));
        obj.add_controller(scroll);

        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin(glib::clone!(
            #[weak]
            obj,
            move |_drag, _x, _y| {
                let t = *obj.imp().transform.borrow();
                obj.imp().drag_start.replace(Some(t));
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak]
            obj,
            move |_drag, dx, dy| {
                let imp = obj.imp();
                let Some(start) = imp.drag_start.borrow().as_ref().copied() else {
                    return;
                };
                let mut t = start;
                t.pan_by(Point::new(dx, dy), imp.frame_size.get(), imp.widget_size());
                imp.transform.replace(t);
                obj.queue_draw();
            }
        ));
        drag.connect_drag_end(glib::clone!(
            #[weak]
            obj,
            move |_drag, _dx, _dy| {
                obj.imp().drag_start.replace(None);
            }
        ));
        obj.add_controller(drag);

        let click = gtk::GestureClick::new();
        click.connect_pressed(glib::clone!(
            #[weak]
            obj,
            move |_click, n_press, _x, _y| {
                if n_press >= 2 {
                    obj.imp().toggle_fit_100();
                }
            }
        ));
        obj.add_controller(click);
    }

    pub(super) fn widget_size(&self) -> Size {
        let obj = self.obj();
        Size::new(f64::from(obj.width()), f64::from(obj.height()))
    }

    fn zoom_at(&self, anchor: Point, factor: f64) {
        let frame = self.frame_size.get();
        let widget = self.widget_size();
        let mut t = *self.transform.borrow();
        t.zoom_at(anchor, factor, frame, widget);
        *self.transform.borrow_mut() = t;
        self.obj().queue_draw();
    }

    pub(super) fn zoom_at_center(&self, factor: f64) {
        let w = self.widget_size();
        self.zoom_at(Point::new(w.width / 2.0, w.height / 2.0), factor);
    }

    fn toggle_fit_100(&self) {
        let frame = self.frame_size.get();
        let widget = self.widget_size();
        let mode = self.transform.borrow().mode;
        *self.transform.borrow_mut() = if mode == ViewMode::Fit {
            ViewTransform::zoom_100(frame, widget)
        } else {
            ViewTransform::fit(frame, widget)
        };
        self.obj().queue_draw();
    }
}
