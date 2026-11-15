// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Calibration dialogs (§6.9, M8): the wizard that turns a drawn
//! reference line into a named profile, the rename form and the
//! delete confirmation. Every dialog returns focus to the viewport
//! on close (CONVENTIONS §7).

use relm4::adw::prelude::*;
use std::rc::Rc;

use relm4::{adw, gtk};

use crate::app::Input;
use crate::strings;
use crate::widgets::viewport::Viewport;

/// The shared scaffold of both calibration dialogs: a named entry row
/// inside a preferences group, Cancel/Created buttons in the header,
/// focus returned to the viewport on close (CONVENTIONS §7).
struct FormDialog {
    dialog: adw::Dialog,
    name: adw::EntryRow,
    confirm: gtk::Button,
}

/// Builds the scaffold: `extra_rows` are added above the name entry.
/// `existing_names` gates the confirm button on a free, non-blank
/// name (collision paints the entry red with an explaining tooltip).
fn form_dialog(
    window: &adw::ApplicationWindow,
    viewport: &Viewport,
    title: &'static str,
    confirm_label: &'static str,
    initial_name: &str,
    existing_names: &[String],
    extra_rows: &[&adw::PreferencesRow],
) -> FormDialog {
    let dialog = adw::Dialog::new();
    dialog.set_title(title);
    dialog.set_content_width(380);

    let confirm = gtk::Button::with_label(confirm_label);
    confirm.add_css_class("suggested-action");
    confirm.set_valign(gtk::Align::Center);

    let name = adw::EntryRow::new();
    name.set_title(strings::CALIBRATION_NAME_LABEL);
    name.set_text(initial_name);

    // The collision message is a label under the entry, never a
    // tooltip: `set_tooltip_text` re-triggers GTK's pick under the
    // pointer on every keystroke — the GTK 4.14 pick-recursion path
    // that freezes the UI (v1.1.2 rule: tooltips are set once, at
    // construction).
    let collision_hint = gtk::Label::new(Some(strings::CALIBRATION_NAME_TAKEN));
    collision_hint.add_css_class("error");
    collision_hint.add_css_class("caption");
    collision_hint.set_xalign(0.0);
    collision_hint.set_margin_start(12);
    collision_hint.set_margin_end(12);
    collision_hint.set_visible(false);

    let taken: Vec<String> = existing_names.to_vec();
    let validate = {
        let name = name.clone();
        let confirm = confirm.clone();
        let hint = collision_hint.clone();
        move || {
            let text = name.text().to_string();
            let collision = taken.iter().any(|n| n == &text);
            hint.set_visible(collision);
            if collision {
                name.add_css_class("error");
            } else {
                name.remove_css_class("error");
            }
            confirm.set_sensitive(!text.trim().is_empty() && !collision);
        }
    };
    name.connect_changed({
        let validate = validate.clone();
        move |_| validate()
    });
    name.connect_apply({
        let confirm = confirm.clone();
        move |_| {
            if confirm.is_sensitive() {
                confirm.emit_clicked();
            }
        }
    });
    validate();

    let group = adw::PreferencesGroup::new();
    for row in extra_rows {
        group.add(*row);
    }
    group.add(&name);
    group.add(&collision_hint);
    let page = adw::PreferencesPage::new();
    page.add(&group);
    let clamp = adw::Clamp::new();
    clamp.set_child(Some(&page));

    let cancel = gtk::Button::with_label(strings::DIALOG_CANCEL);
    cancel.set_valign(gtk::Align::Center);
    cancel.connect_clicked({
        let dialog = dialog.clone();
        move |_| {
            dialog.close();
        }
    });

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(title, "")));
    header.pack_start(&cancel);
    header.pack_end(&confirm);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&clamp));
    dialog.set_child(Some(&toolbar));

    crate::focus::watch_dialog(&dialog, viewport);
    dialog.present(Some(window));

    FormDialog {
        dialog,
        name,
        confirm,
    }
}

/// The calibration wizard (§6.9): the user drew the reference line
/// (`line_px` image pixels), here they type the real millimetres and
/// the profile name. Commit emits `Input::CommitCalibration`.
pub fn open_calibration_dialog(
    window: &adw::ApplicationWindow,
    viewport: &Viewport,
    line_px: f64,
    existing_names: Vec<String>,
    default_name: String,
    sink: &Rc<dyn Fn(Input)>,
) {
    let line_row = adw::ActionRow::new();
    line_row.set_title(&strings::calibration_line_label(line_px));
    let mm = adw::SpinRow::with_range(0.01, 1000.0, 0.1);
    crate::measure_ui::swallow_scroll(&mm);
    mm.set_title(strings::CALIBRATION_REAL_LABEL);
    mm.set_digits(2);
    mm.set_value(1.0);

    let form = form_dialog(
        window,
        viewport,
        strings::CALIBRATION_DIALOG_TITLE,
        strings::CALIBRATION_CONFIRM_LABEL,
        &default_name,
        &existing_names,
        &[line_row.upcast_ref(), mm.upcast_ref()],
    );

    form.confirm.connect_clicked({
        let sink = sink.clone();
        let name = form.name.clone();
        let dialog = form.dialog.clone();
        move |_| {
            (sink)(Input::CommitCalibration {
                name: name.text().to_string(),
                mm: mm.value(),
            });
            dialog.close();
        }
    });
}

/// Rename dialog for the active profile — same scaffold minus the
/// distance row.
pub fn open_rename_dialog(
    window: &adw::ApplicationWindow,
    viewport: &Viewport,
    current_name: &str,
    existing_names: Vec<String>,
    sink: &Rc<dyn Fn(Input)>,
) {
    let form = form_dialog(
        window,
        viewport,
        strings::RENAME_DIALOG_TITLE,
        strings::RENAME_CONFIRM_LABEL,
        current_name,
        &existing_names
            .into_iter()
            .filter(|n| n != current_name)
            .collect::<Vec<_>>(),
        &[],
    );
    form.name.select_region(0, -1);
    form.confirm.connect_clicked({
        let sink = sink.clone();
        let name = form.name.clone();
        let dialog = form.dialog.clone();
        move |_| {
            (sink)(Input::CalibrationRenamed(name.text().to_string()));
            dialog.close();
        }
    });
}

/// Destructive confirmation before deleting the active profile.
pub fn confirm_delete_profile(
    window: &adw::ApplicationWindow,
    name: &str,
    sink: &Rc<dyn Fn(Input)>,
) {
    let dialog = adw::AlertDialog::new(
        Some(strings::CALIBRATION_DELETE_TITLE),
        Some(&strings::calibration_delete_body(name)),
    );
    dialog.add_response("cancel", strings::DIALOG_CANCEL);
    dialog.add_response("delete", strings::DIALOG_DELETE);
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let sink = sink.clone();
    dialog.choose(Some(window), gtk::gio::Cancellable::NONE, move |response| {
        if response == "delete" {
            (sink)(Input::DeleteCalibration);
        }
    });
}
