// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Bancada "Medição" panel and the calibration dialogs (§6.9, M8).
//! Built imperatively — the measurement list is dynamic and the tool
//! combo must not re-emit while syncing, the same pattern the image
//! adjustments use (`build_adjust_ui`).

use gtk::glib;
use hiview_core::{AppConfig, MeasureSet, MeasureTool};
use relm4::adw::prelude::*;
use relm4::{ComponentSender, adw, gtk};

use crate::app::{AppModel, Input};
use crate::strings;

/// Handles kept for [`sync`] — every widget the model needs to update
/// or silence while pushing state into the panel.
pub struct MeasurePanel {
    /// The owning group — measurement rows are appended/removed here.
    group: adw::PreferencesGroup,
    /// "Ferramenta" combo (None/Régua/Ângulo/Círculo).
    tool_row: adw::ComboRow,
    tool_handler: glib::SignalHandlerId,
    /// "Calibração ativa" combo (None + every stored profile).
    profile_row: adw::ComboRow,
    profile_model: gtk::StringList,
    profile_handler: glib::SignalHandlerId,
    /// Profile management — sensitive only with a profile selected.
    rename_btn: gtk::Button,
    delete_btn: gtk::Button,
    /// One `adw::ActionRow` per committed measurement.
    list_rows: Vec<adw::ActionRow>,
    /// "Limpar medições" — sensitive only with items on screen.
    clear_row: adw::ActionRow,
}

/// Builds the whole Medição group; the caller inserts it into the
/// sidebar's `adw::PreferencesPage`.
pub fn build(sender: &ComponentSender<AppModel>) -> (adw::PreferencesGroup, MeasurePanel) {
    let group = adw::PreferencesGroup::new();
    group.set_title(strings::MEASURE_GROUP_TITLE);

    // — Armed tool: a combo reads better than three toggle buttons and
    //   matches every other row in the panel.
    let tool_row = adw::ComboRow::new();
    tool_row.set_title(strings::MEASURE_TOOL_LABEL);
    tool_row.set_model(Some(&gtk::StringList::new(&strings::TOOL_NAMES)));
    let tool_handler = tool_row.connect_selected_notify({
        let sender = sender.clone();
        move |row| sender.input(Input::ArmMeasureIndex(row.selected()))
    });
    group.add(&tool_row);

    // — Calibration wizard entry point.
    let calibrate_row = adw::ActionRow::new();
    calibrate_row.set_title(strings::CALIBRATE_BUTTON_LABEL);
    calibrate_row.set_activatable(true);
    let add_icon = gtk::Image::from_icon_name("list-add-symbolic");
    add_icon.set_valign(gtk::Align::Center);
    calibrate_row.add_suffix(&add_icon);
    calibrate_row.connect_activated({
        let sender = sender.clone();
        move |_| sender.input(Input::StartCalibration)
    });
    group.add(&calibrate_row);

    // — Active profile + management buttons in the row's suffix.
    let profile_model = gtk::StringList::new(&[strings::CALIBRATION_NONE]);
    let profile_row = adw::ComboRow::new();
    profile_row.set_title(strings::CALIBRATION_ACTIVE_LABEL);
    profile_row.set_model(Some(&profile_model));
    let profile_handler = profile_row.connect_selected_notify({
        let sender = sender.clone();
        move |row| sender.input(Input::SelectCalibration(row.selected()))
    });

    let rename_btn = icon_button(
        "document-edit-symbolic",
        strings::CALIBRATION_RENAME_LABEL,
        {
            let sender = sender.clone();
            move |_| sender.input(Input::RequestRenameCalibration)
        },
    );
    let delete_btn = icon_button("user-trash-symbolic", strings::CALIBRATION_DELETE_LABEL, {
        let sender = sender.clone();
        move |_| sender.input(Input::RequestDeleteCalibration)
    });
    let manage = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    manage.set_valign(gtk::Align::Center);
    manage.append(&rename_btn);
    manage.append(&delete_btn);
    profile_row.add_suffix(&manage);
    group.add(&profile_row);

    // — "Limpar medições" sits below the dynamic list; `sync` keeps it
    //   last by re-appending it after rebuilding the item rows.
    let clear_row = adw::ActionRow::new();
    clear_row.set_title(strings::MEASURE_CLEAR_LABEL);
    clear_row.set_activatable(true);
    let clear_icon = gtk::Image::from_icon_name("edit-clear-all-symbolic");
    clear_icon.set_valign(gtk::Align::Center);
    clear_row.add_suffix(&clear_icon);
    clear_row.connect_activated({
        let sender = sender.clone();
        move |_| sender.input(Input::ClearMeasurements)
    });
    group.add(&clear_row);

    let panel = MeasurePanel {
        group: group.clone(),
        tool_row,
        tool_handler,
        profile_row,
        profile_model,
        profile_handler,
        rename_btn,
        delete_btn,
        list_rows: Vec::new(),
        clear_row,
    };
    (group, panel)
}

/// A flat icon button for the profile-row suffix.
fn icon_button(
    icon: &str,
    tooltip: &str,
    on_click: impl Fn(&gtk::Button) + 'static,
) -> gtk::Button {
    let btn = gtk::Button::from_icon_name(icon);
    btn.set_tooltip_text(Some(tooltip));
    btn.add_css_class("flat");
    btn.set_valign(gtk::Align::Center);
    btn.connect_clicked(on_click);
    btn
}

/// Pushes the model state into the panel. `set_selected` re-emits
/// `notify::selected`, so both combos are signal-blocked while they
/// are synced — otherwise every frame-motion sync would re-arm tools.
pub fn sync(
    panel: &mut MeasurePanel,
    measure: &MeasureSet,
    config: &AppConfig,
    sender: &ComponentSender<AppModel>,
) {
    let tool_index = match measure.armed_tool() {
        None | Some(MeasureTool::Calibrate) => 0,
        Some(MeasureTool::Ruler) => 1,
        Some(MeasureTool::Angle) => 2,
        Some(MeasureTool::Circle) => 3,
    };
    panel.tool_row.block_signal(&panel.tool_handler);
    panel.tool_row.set_selected(tool_index);
    panel.tool_row.unblock_signal(&panel.tool_handler);

    // Profile combo: rebuild only when the list/selection changed —
    // rebuilding every sync would steal the open popup's state.
    let names: Vec<String> = std::iter::once(strings::CALIBRATION_NONE.to_string())
        .chain(config.calibration.iter().map(|p| p.name.clone()))
        .collect();
    let active_index = config
        .active_calibration
        .as_deref()
        .and_then(|name| config.calibration.iter().position(|p| p.name == name))
        .map_or(0, |i| i + 1) as u32;
    let current: Vec<String> = (0..panel.profile_model.n_items())
        .filter_map(|i| panel.profile_model.string(i).map(|s| s.to_string()))
        .collect();
    panel.profile_row.block_signal(&panel.profile_handler);
    if current != names {
        let strs: Vec<&str> = names.iter().map(String::as_str).collect();
        panel.profile_model.splice(0, current.len() as u32, &strs);
    }
    panel.profile_row.set_selected(active_index);
    panel.profile_row.unblock_signal(&panel.profile_handler);

    let has_active = config.active_profile().is_some();
    panel.rename_btn.set_sensitive(has_active);
    panel.delete_btn.set_sensitive(has_active);

    // The measurement list: one row per committed item, "Limpar"
    // re-appended last so it always sits below them.
    for row in panel.list_rows.drain(..) {
        panel.group.remove(&row);
    }
    panel.group.remove(&panel.clear_row);
    let px_per_mm = config.active_profile().map(|p| p.px_per_mm);
    for m in &measure.items {
        let row = adw::ActionRow::new();
        row.set_title(&m.shape.label(px_per_mm));
        let tool = match m.tool {
            MeasureTool::Ruler => strings::TOOL_SHORT_NAMES[0],
            MeasureTool::Angle => strings::TOOL_SHORT_NAMES[1],
            MeasureTool::Circle | MeasureTool::Calibrate => strings::TOOL_SHORT_NAMES[2],
        };
        row.set_subtitle(tool);
        let del = gtk::Button::from_icon_name("edit-delete-symbolic");
        del.add_css_class("flat");
        del.set_valign(gtk::Align::Center);
        del.set_tooltip_text(Some(strings::DIALOG_DELETE));
        let id = m.id;
        del.connect_clicked({
            let sender = sender.clone();
            move |_| sender.input(Input::DeleteMeasurement(id))
        });
        row.add_suffix(&del);
        panel.group.add(&row);
        panel.list_rows.push(row);
    }
    panel.clear_row.set_sensitive(!measure.items.is_empty());
    panel.group.add(&panel.clear_row);
}
