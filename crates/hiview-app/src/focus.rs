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

/// Whether `widget` or one of its ancestors is a text input — entries,
/// spin buttons and text views. Single-key accelerators must NEVER
/// fire while one of these holds focus: a typed "g" is text, not the
/// grid shortcut (CONVENTIONS §7, M8 round-2 finding).
pub(crate) fn in_text_input(widget: &gtk::Widget) -> bool {
    let mut current = Some(widget.clone());
    while let Some(w) = current {
        if w.is::<gtk::Editable>() || w.is::<gtk::TextView>() {
            return true;
        }
        current = w.parent();
    }
    false
}

/// The single-key shortcut gate (§13): bare letters and digits may
/// only reach the `win.*` actions when the window itself owns the
/// interaction — no `adw::Dialog` open, no text input focused.
/// The application-level `set_accelerators_for_action` API cannot do
/// this check, which is why shortcuts live in a window
/// `EventControllerKey` instead.
pub(crate) fn shortcuts_allowed(window: &adw::ApplicationWindow) -> bool {
    if adw::prelude::AdwApplicationWindowExt::visible_dialog(window).is_some() {
        return false;
    }
    GtkWindowExt::focus(window).is_none_or(|f| !in_text_input(&f))
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

    /// `present()` only queues the map and signal delivery is async —
    /// pump the main loop so the window is a real `GtkRoot` and
    /// `closed` handlers have run before focus assertions touch them.
    fn realize(window: &gtk::Window) {
        let ctx = glib::MainContext::default();
        for _ in 0..200 {
            if window.is_realized() {
                break;
            }
            ctx.iteration(false);
        }
        for _ in 0..50 {
            ctx.iteration(false);
        }
    }

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

        // Text-input detection walks ancestors — the focused widget is
        // the entry itself (EntryRow's inner text is still `Editable`).
        assert!(in_text_input(&gtk::Entry::new().upcast::<gtk::Widget>()));
        assert!(in_text_input(
            &gtk::SpinButton::with_range(0.0, 10.0, 1.0).upcast::<gtk::Widget>()
        ));
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let entry = gtk::Entry::new();
        box_.append(&entry);
        assert!(in_text_input(&entry.upcast::<gtk::Widget>()));
        assert!(!in_text_input(
            &gtk::Label::new(None).upcast::<gtk::Widget>()
        ));
        assert!(!in_text_input(&gtk::Button::new().upcast::<gtk::Widget>()));

        // The §13 gate's editable half, exercised on a REALIZED window
        // (an `adw::ApplicationWindow` can't exist without a running
        // GApplication — the critical-free env forbids it). The focus
        // half of `shortcuts_allowed` is this exact expression.
        let window = gtk::Window::new();
        let box2 = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let entry2 = gtk::Entry::new();
        let btn = gtk::Button::new();
        box2.append(&entry2);
        box2.append(&btn);
        window.set_child(Some(&box2));
        window.present();
        realize(&window);
        entry2.grab_focus();
        let text_focused = || GtkWindowExt::focus(&window).is_some_and(|f| in_text_input(&f));
        assert!(text_focused(), "entry holds focus after grab_focus");
        btn.grab_focus();
        assert!(
            !text_focused(),
            "a plain widget is not a text input — shortcuts allowed"
        );
        window.close();

        // `closed` must come from REAL presented widgets — emitting it
        // on an unrooted popover/dialog makes GTK's own closed handler
        // call gtk_root_get_focus on a non-root (Gtk-CRITICAL). An
        // `adw::Window` can exist without a running GApplication.
        let window = adw::Window::new();
        let box3 = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let target = gtk::Button::new();
        box3.append(&target);
        adw::prelude::AdwWindowExt::set_content(&window, Some(&box3));
        window.present();
        realize(window.upcast_ref::<gtk::Window>());

        let popover = gtk::Popover::new();
        popover.set_parent(&target);
        watch_popover(&popover, &target);
        popover.popup();
        popover.popdown();
        realize(window.upcast_ref::<gtk::Window>());
        assert_eq!(
            GtkWindowExt::focus(&window).as_ref(),
            Some(target.upcast_ref::<gtk::Widget>())
        );

        // AdwDialog's `closed` internally calls gtk_root_get_focus on
        // the dialog itself — a widget, never a GtkRoot — so ANY emit
        // (rooted or not) logs one Gtk-CRITICAL, and a real close()
        // doesn't emit at all on a headless dialog. Emit on the
        // unrooted dialog anyway: it costs the same single internal
        // critical while keeping the focus assertion that proves the
        // contract.
        let dialog = adw::Dialog::new();
        watch_dialog(&dialog, &target);
        dialog.emit_by_name::<()>("closed", &[]);
        assert_eq!(
            GtkWindowExt::focus(&window).as_ref(),
            Some(target.upcast_ref::<gtk::Widget>())
        );
        window.close();
    }
}
