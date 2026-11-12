// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Bancada "Medição" panel and the calibration dialogs (§6.9, M8).
//! Built imperatively — the measurement list is dynamic and the tool
//! combo must not re-emit while syncing, the same pattern the image
//! adjustments use (`build_adjust_ui`).

use gtk::glib;
use hiview_core::{AppConfig, MeasureSet, MeasureTool};
use relm4::adw::prelude::*;
use std::rc::Rc;

use relm4::{adw, gtk};

use crate::app::Input;
use crate::strings;

/// Handles kept for [`sync`] — every widget the model needs to update
/// or silence while pushing state into the panel.
pub struct MeasurePanel {
    /// The owning group — measurement rows are appended/removed here.
    group: adw::PreferencesGroup,
    /// "Ferramenta" combo (None/Régua/Ângulo/Círculo).
    tool_row: adw::ComboRow,
    tool_handler: glib::SignalHandlerId,
    /// "Nova calibração…" row — its subtitle flags calibration mode
    /// while the Calibrate tool is armed.
    calibrate_row: adw::ActionRow,
    /// "Calibração ativa" combo (None + every stored profile).
    profile_row: adw::ComboRow,
    profile_model: gtk::StringList,
    profile_handler: glib::SignalHandlerId,
    /// Profile management — sensitive only with a profile selected.
    rename_btn: gtk::Button,
    delete_btn: gtk::Button,
    /// One `adw::ActionRow` per committed measurement.
    list_rows: Vec<adw::ActionRow>,
    /// Signature of what `list_rows` shows — item ids plus the active
    /// scale: a calibration switch re-labels the same ids in mm/µm.
    /// The list rebuilds only when this changes.
    listed: (Vec<u64>, Option<f64>),
    /// "Limpar medições" — sensitive only with items on screen.
    clear_row: adw::ActionRow,
}

/// Builds the whole Medição group; the caller inserts it into the
/// sidebar's `adw::PreferencesPage`.
pub fn build(sink: &Rc<dyn Fn(Input)>) -> (adw::PreferencesGroup, MeasurePanel) {
    let group = adw::PreferencesGroup::new();
    group.set_title(strings::MEASURE_GROUP_TITLE);

    // — Armed tool: a combo reads better than three toggle buttons and
    //   matches every other row in the panel.
    let tool_row = adw::ComboRow::new();
    tool_row.set_title(strings::MEASURE_TOOL_LABEL);
    swallow_scroll(&tool_row);
    tool_row.set_model(Some(&gtk::StringList::new(&strings::TOOL_NAMES)));
    let tool_handler = tool_row.connect_selected_notify({
        let sink = sink.clone();
        move |row| (sink)(Input::ArmMeasureIndex(row.selected()))
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
        let sink = sink.clone();
        move |_| (sink)(Input::StartCalibration)
    });
    group.add(&calibrate_row);

    // — Active profile + management buttons in the row's suffix.
    //   `use_subtitle` puts the title on the second line so the
    //   dropdown gets the full row width — profile names must not
    //   ellipsize to "Ne…" in the narrow panel.
    let profile_model = gtk::StringList::new(&[strings::CALIBRATION_NONE]);
    let profile_row = adw::ComboRow::new();
    profile_row.set_title(strings::CALIBRATION_ACTIVE_LABEL);
    profile_row.set_use_subtitle(true);
    swallow_scroll(&profile_row);
    profile_row.set_model(Some(&profile_model));
    let profile_handler = profile_row.connect_selected_notify({
        let sink = sink.clone();
        move |row| (sink)(Input::SelectCalibration(row.selected()))
    });

    let rename_btn = icon_button(
        "document-edit-symbolic",
        strings::CALIBRATION_RENAME_LABEL,
        {
            let sink = sink.clone();
            move |_| (sink)(Input::RequestRenameCalibration)
        },
    );
    let delete_btn = icon_button("user-trash-symbolic", strings::CALIBRATION_DELETE_LABEL, {
        let sink = sink.clone();
        move |_| (sink)(Input::RequestDeleteCalibration)
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
        let sink = sink.clone();
        move |_| (sink)(Input::ClearMeasurements)
    });
    group.add(&clear_row);

    let panel = MeasurePanel {
        group: group.clone(),
        tool_row,
        tool_handler,
        calibrate_row,
        profile_row,
        profile_model,
        profile_handler,
        rename_btn,
        delete_btn,
        list_rows: Vec::new(),
        listed: (Vec::new(), None),
        clear_row,
    };
    (group, panel)
}

/// GTK controls eat the wheel — `GtkDropDown` cycles `selected`,
/// `GtkRange` and `SpinButton` nudge their values — so scrolling the
/// side panel over a control silently rewrote configuration (the
/// "profile lost" regression). A capture-phase scroll controller on
/// the control claims the wheel first and forwards the delta to the
/// enclosing `ScrolledWindow` — the wheel only ever scrolls the
/// panel, never changes a value.
pub(crate) fn swallow_scroll<W: IsA<gtk::Widget>>(widget: &W) {
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
    scroll.connect_scroll(|ctrl, dx, dy| {
        if let Some(w) = ctrl.widget()
            && let Some(ancestor) = w.ancestor(gtk::ScrolledWindow::static_type())
            && let Ok(scrolled) = ancestor.downcast::<gtk::ScrolledWindow>()
        {
            nudge(&scrolled.vadjustment(), dy);
            nudge(&scrolled.hadjustment(), dx);
        }
        glib::Propagation::Stop
    });
    widget.add_controller(scroll);
}

/// Pixels one wheel unit moves the panel — close to GtkScrolledWindow's
/// own per-notch scroll for typical row heights.
const SCROLL_STEP_PX: f64 = 60.0;

/// Moves `adj` by `delta` wheel units, clamped to the scrollable range.
fn nudge(adj: &gtk::Adjustment, delta: f64) {
    if delta != 0.0 {
        adj.set_value((adj.value() + delta * SCROLL_STEP_PX).clamp(
            adj.lower(),
            (adj.upper() - adj.page_size()).max(adj.lower()),
        ));
    }
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
    sink: &Rc<dyn Fn(Input)>,
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

    // While Calibrate is armed the tool combo reads "Nenhuma" — the
    // calibrate row's subtitle makes the mode obvious instead.
    if measure.armed_tool() == Some(MeasureTool::Calibrate) {
        panel
            .calibrate_row
            .set_subtitle(strings::CALIBRATION_ARMED_HINT);
    } else {
        panel.calibrate_row.set_subtitle("");
    }

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
    // re-appended last so it always sits below them. Rebuilt only when
    // the id set changes — gesture/hover syncs leave the DOM alone.
    let px_per_mm = config.active_profile().map(|p| p.px_per_mm);
    let listed: (Vec<u64>, Option<f64>) = (measure.items.iter().map(|m| m.id).collect(), px_per_mm);
    if listed == panel.listed {
        panel.clear_row.set_sensitive(!listed.0.is_empty());
        return;
    }
    panel.listed = listed;
    for row in panel.list_rows.drain(..) {
        panel.group.remove(&row);
    }
    panel.group.remove(&panel.clear_row);
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
            let sink = sink.clone();
            move |_| (sink)(Input::DeleteMeasurement(id))
        });
        row.add_suffix(&del);
        panel.group.add(&row);
        panel.list_rows.push(row);
    }
    panel.clear_row.set_sensitive(!measure.items.is_empty());
    panel.group.add(&panel.clear_row);
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use hiview_core::{CalibrationProfile, MeasureGesture, Point};

    use super::*;

    /// The per-row delete button lives in the row's suffix box.
    fn row_delete_button(row: &adw::ActionRow) -> gtk::Button {
        row.first_child()
            .and_then(|b| b.last_child())
            .and_then(|s| s.first_child())
            .and_then(|w| w.downcast::<gtk::Button>().ok())
            .unwrap_or_else(|| panic!("row has a delete button in its suffix"))
    }

    fn ruler(set: &mut MeasureSet, ax: f64, bx: f64) {
        set.gesture(MeasureGesture::Click(Point::new(ax, 10.0)));
        set.gesture(MeasureGesture::Click(Point::new(bx, 10.0)));
    }

    /// Medição panel contract (§6.9): committed items list one row
    /// each — delete emits `DeleteMeasurement`, "Limpar" emits
    /// `ClearMeasurements` — and the rows re-label mm/µm when the
    /// active calibration changes without rebuilding on hover syncs.
    ///
    /// NOT a `#[test]`: GTK can only init on one thread, so this runs
    /// inside the single `crate::tests::gtk_ui_contracts`.
    pub(crate) fn check() {
        let emitted = Rc::new(RefCell::new(Vec::<Input>::new()));
        let sink: Rc<dyn Fn(Input)> = {
            let emitted = emitted.clone();
            Rc::new(move |input| emitted.borrow_mut().push(input))
        };
        let (_group, mut panel) = build(&sink);

        let mut set = MeasureSet::default();
        set.arm(Some(MeasureTool::Ruler));
        ruler(&mut set, 0.0, 100.0);
        ruler(&mut set, 0.0, 200.0);
        let config = AppConfig::default();

        sync(&mut panel, &set, &config, &sink);
        assert_eq!(panel.list_rows.len(), 2);
        assert!(panel.clear_row.is_sensitive());
        // No calibration → pixel labels.
        assert!(panel.list_rows[0].title().ends_with("px"));

        // A second sync with the same signature must not rebuild:
        // the widget instances stay identical (hover/draft churn).
        let first = panel.list_rows[0].clone();
        sync(&mut panel, &set, &config, &sink);
        assert_eq!(panel.list_rows[0], first);

        // Activating a profile re-labels the same ids in mm/µm.
        let mut config2 = AppConfig::default();
        config2.upsert_profile(
            CalibrationProfile::from_line(
                Point::new(0.0, 0.0),
                Point::new(89.0, 0.0),
                1.0,
                "teste".into(),
            )
            .unwrap_or_else(|| panic!("profile valid")),
        );
        sync(&mut panel, &set, &config2, &sink);
        assert_eq!(panel.list_rows.len(), 2);
        let title = panel.list_rows[0].title().to_string();
        assert!(
            title.contains("mm") || title.contains("µm"),
            "title: {title}"
        );

        // The profile combo lists the new profile and selects it —
        // the "Criar perfil → combo stays Nenhuma" regression.
        assert_eq!(panel.profile_model.n_items(), 2);
        assert_eq!(panel.profile_model.string(1).as_deref(), Some("teste"));
        assert_eq!(panel.profile_row.selected(), 1);
        assert!(panel.rename_btn.is_sensitive());
        assert!(panel.delete_btn.is_sensitive());

        // Arming Calibrate keeps the tool combo on "Nenhuma" but the
        // calibrate row's subtitle flags the armed mode.
        let mut cal = MeasureSet::default();
        cal.arm(Some(MeasureTool::Calibrate));
        sync(&mut panel, &cal, &config2, &sink);
        assert_eq!(panel.tool_row.selected(), 0);
        assert_eq!(
            panel.calibrate_row.subtitle().as_deref(),
            Some(strings::CALIBRATION_ARMED_HINT)
        );
        cal.arm(Some(MeasureTool::Ruler));
        sync(&mut panel, &cal, &config2, &sink);
        assert_eq!(panel.calibrate_row.subtitle().as_deref(), Some(""));

        // Re-sync the measured set so the delete/clear checks below
        // still find their rows.
        sync(&mut panel, &set, &config2, &sink);
        assert_eq!(panel.list_rows.len(), 2);

        // Scroll guard: both combos carry the capture scroll
        // controller (the wheel scrolls the panel, never `selected`).
        for row in [&panel.tool_row, &panel.profile_row] {
            let controllers = row.observe_controllers();
            assert_eq!(
                controllers
                    .iter::<glib::Object>()
                    .filter(|o| o
                        .as_ref()
                        .ok()
                        .is_some_and(|c| c.is::<gtk::EventControllerScroll>()))
                    .count(),
                1,
                "combo has exactly one scroll guard"
            );
        }

        // The forwarded wheel delta moves the panel's adjustment and
        // clamps at the scrollable bounds.
        let adj = gtk::Adjustment::new(100.0, 0.0, 1000.0, 10.0, 50.0, 200.0);
        nudge(&adj, 1.0);
        assert!((adj.value() - 100.0 - SCROLL_STEP_PX).abs() < 1e-9);
        nudge(&adj, -99.0);
        assert_eq!(adj.value(), adj.lower());
        nudge(&adj, 99.0);
        assert_eq!(adj.value(), adj.upper() - adj.page_size());

        // Per-item delete and "Limpar" reach the model.
        row_delete_button(&panel.list_rows[0]).emit_clicked();
        panel.clear_row.emit_activate();
        assert!(
            emitted
                .borrow()
                .iter()
                .any(|i| matches!(i, Input::DeleteMeasurement(_)))
        );
        assert!(
            emitted
                .borrow()
                .iter()
                .any(|i| matches!(i, Input::ClearMeasurements))
        );
    }
}
