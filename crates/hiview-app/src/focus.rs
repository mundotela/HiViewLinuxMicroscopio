// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Shared keyboard-focus return (docs/CONVENTIONS.md §7 "Foco do
//! teclado"). Single-key accelerators (`G`, `C`, `M`, `Espaço`…) die
//! whenever an editable widget, a slider or a leftover dialog/popover
//! keeps focus — GTK routes bare letters into it and the window
//! accelerators never see the key.
//!
//! One helper watches every escape hatch: `install` wires the window
//! (Enter commits an edit, window reactivation rescues stuck focus —
//! which covers **every** presented dialog, and the pointer leaving the
//! Bancada panel ends an edit session), while [`watch_dialog`] and
//! [`watch_popover`] return focus when each dialog/popover the app owns
//! closes. `adw::AlertDialog` has no `closed` signal — the `is-active`
//! watcher covers it because a modal dialog deactivates the window.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::adw::prelude::AdwDialogExt as _;
use relm4::gtk::prelude::GtkWindowExt;
use relm4::gtk::{self, glib};
use relm4::{adw, gtk::prelude::*};

use crate::widgets::viewport::Viewport;

/// Lazily-filled focus target: a component created before the viewport
/// exists (the gallery launches before `view_output!`) receives the
/// empty cell and the app fills it once the widget is built.
pub type Slot = Rc<RefCell<Option<gtk::Widget>>>;

/// Whether focus on `widget` counts as "stuck editing": single-key
/// accelerators are either swallowed (editables) or should be released
/// once the interaction ends (sliders — CONVENTIONS §7 lists them).
fn is_editing(widget: &gtk::Widget) -> bool {
    widget.is::<gtk::Editable>() || widget.is::<gtk::TextView>() || widget.is::<gtk::Range>()
}

/// Window-level focus returns, installed once at startup:
///
/// - Enter inside an editable commits (bubble phase runs after the
///   widget's own handling) and gives focus back to the viewport;
/// - when the window becomes active again, focus stuck in an editable
///   — left there by a closed dialog or popover — returns too;
/// - the pointer leaving the Bancada panel ends an edit session.
pub fn install(window: &adw::ApplicationWindow, viewport: &Viewport, sidebar: &gtk::Widget) {
    let key = gtk::EventControllerKey::new();
    key.connect_key_pressed({
        let viewport = viewport.clone();
        let window = window.clone();
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
                let editing = GtkWindowExt::focus(&window).is_some_and(|f| is_editing(&f));
                if editing {
                    viewport.grab_focus();
                }
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key.clone());

    window.connect_notify_local(Some("is-active"), {
        let viewport = viewport.clone();
        move |win, _| {
            if !win.is_active() {
                return;
            }
            if GtkWindowExt::focus(win).is_some_and(|f| is_editing(&f)) {
                viewport.grab_focus();
            }
        }
    });

    let motion = gtk::EventControllerMotion::new();
    motion.connect_leave({
        let viewport = viewport.clone();
        let window = window.clone();
        move |_| {
            if GtkWindowExt::focus(&window).is_some_and(|f| is_editing(&f)) {
                viewport.grab_focus();
            }
        }
    });
    sidebar.add_controller(motion);
}

/// Returns focus to `target` when `dialog` closes — call on every
/// `adw::Dialog` the app presents (Preferences…).
pub fn watch_dialog(dialog: &impl IsA<adw::Dialog>, target: &impl IsA<gtk::Widget>) {
    let target = target.clone().upcast::<gtk::Widget>();
    dialog.connect_closed(move |_| {
        target.grab_focus();
    });
}

/// Returns focus to `target` when `popover` closes — call on every
/// `gtk::Popover` the app owns (context menus, adjust popover…).
pub fn watch_popover(popover: &gtk::Popover, target: &impl IsA<gtk::Widget>) {
    let target = target.clone().upcast::<gtk::Widget>();
    popover.connect_closed(move |_| {
        target.grab_focus();
    });
}

/// Like [`watch_popover`], but resolves the target through a [`Slot`]
/// the app fills once the focus target exists.
pub fn watch_popover_slot(popover: &gtk::Popover, target: Slot) {
    popover.connect_closed(move |_| {
        if let Some(target) = target.borrow().as_ref() {
            target.grab_focus();
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The whole contract: the "stuck" classification drives the rescue
    /// paths (editables, text views and sliders per CONVENTIONS §7), and
    /// a `closed` signal on a popover or an `adw::Dialog` must land
    /// focus back on the return target — here, its window's focus
    /// widget.
    ///
    /// NOT a `#[test]`: GTK can only init on one thread, so every GTK
    /// check runs inside the single `crate::tests::gtk_ui_contracts`
    /// test.
    pub(crate) fn check() {
        if gtk::init().is_err() {
            return;
        }
        assert!(is_editing(&gtk::Entry::new().upcast::<gtk::Widget>()));
        assert!(is_editing(
            &gtk::SpinButton::with_range(0.0, 10.0, 1.0).upcast::<gtk::Widget>()
        ));
        assert!(is_editing(
            &gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 10.0, 1.0)
                .upcast::<gtk::Widget>()
        ));
        assert!(!is_editing(&gtk::Button::new().upcast::<gtk::Widget>()));
        assert!(!is_editing(&gtk::Label::new(None).upcast::<gtk::Widget>()));

        let window = gtk::Window::new();
        let target = gtk::Button::new();
        window.set_child(Some(&target));

        let popover = gtk::Popover::new();
        watch_popover(&popover, &target);
        popover.emit_by_name::<()>("closed", &[]);
        assert_eq!(
            GtkWindowExt::focus(&window).as_ref(),
            Some(target.upcast_ref::<gtk::Widget>())
        );

        let dialog = adw::Dialog::new();
        watch_dialog(&dialog, &target);
        dialog.emit_by_name::<()>("closed", &[]);
        assert_eq!(
            GtkWindowExt::focus(&window).as_ref(),
            Some(target.upcast_ref::<gtk::Widget>())
        );
    }
}
