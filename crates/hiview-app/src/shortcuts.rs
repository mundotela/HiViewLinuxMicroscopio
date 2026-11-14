// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Keyboard shortcuts — the single source of truth (CONVENTIONS §13,
//! M11). [`SHORTCUTS`] feeds BOTH the capture-phase key matcher
//! ([`shortcut_action`]) and the `gtk::ShortcutsWindow` built by
//! [`build_window`], so the window can never drift from what the keys
//! actually do.

use relm4::gtk::{self, gdk::Key, gdk::ModifierType};

use crate::strings;

/// How `Shift` participates in a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    /// Shift does not change the binding (`Shift+G` = `G`). Needed for
    /// keys like `+`, which arrives with Shift already held.
    Any,
    /// The binding only fires with Shift held (`Shift+R`).
    Required,
    /// The binding only fires without Shift — keeps `Shift+R` from
    /// also triggering the plain `R` binding.
    Forbidden,
}

/// One row of the canonical table: how the key is matched, which
/// `win.*` action it activates, and how the shortcuts window displays
/// it.
pub struct ShortcutDef {
    /// GTK accelerator syntax for the window (`"space"`, `"<Shift>r"`,
    /// `"<Control>comma"`) — NOT a display string; the widget parses
    /// and renders it. `gtk::accelerator_parse` must accept it.
    pub accel: &'static str,
    /// PT-BR label shown in the window.
    pub label: &'static str,
    /// GDK keys activating the binding (`to_lower` matched).
    pub keys: &'static [Key],
    /// Requires Ctrl held.
    pub ctrl: bool,
    /// How Shift participates.
    pub shift: Shift,
    /// Activated `win.*` action.
    pub action: &'static str,
    /// Group title this entry is listed under.
    pub section: &'static str,
}

use Shift::{Any, Forbidden, Required};

/// The canonical shortcut table (CONVENTIONS §13 + `Shift+R` from the
/// M10.5 update). Order inside each group is the display order.
pub const SHORTCUTS: &[ShortcutDef] = &[
    // --- Captura ---
    ShortcutDef {
        accel: "space",
        label: strings::SC_TAKE_PHOTO,
        keys: &[Key::space],
        ctrl: false,
        shift: Any,
        action: "win.take-photo",
        section: strings::SC_GROUP_CAPTURE,
    },
    ShortcutDef {
        accel: "r",
        label: strings::SC_RECORD,
        keys: &[Key::r],
        ctrl: false,
        shift: Forbidden,
        action: "win.record",
        section: strings::SC_GROUP_CAPTURE,
    },
    ShortcutDef {
        accel: "<Shift>r",
        label: strings::SC_TOGGLE_MIC,
        keys: &[Key::r],
        ctrl: false,
        shift: Required,
        action: "win.toggle-mic",
        section: strings::SC_GROUP_CAPTURE,
    },
    ShortcutDef {
        accel: "f",
        label: strings::SC_FREEZE,
        keys: &[Key::f],
        ctrl: false,
        shift: Any,
        action: "win.freeze",
        section: strings::SC_GROUP_CAPTURE,
    },
    // --- Visualização ---
    ShortcutDef {
        accel: "plus",
        label: strings::SC_ZOOM_IN,
        keys: &[Key::plus, Key::equal, Key::KP_Add],
        ctrl: false,
        shift: Any,
        action: "win.zoom-in",
        section: strings::SC_GROUP_VIEW,
    },
    ShortcutDef {
        accel: "minus",
        label: strings::SC_ZOOM_OUT,
        keys: &[Key::minus, Key::KP_Subtract],
        ctrl: false,
        shift: Any,
        action: "win.zoom-out",
        section: strings::SC_GROUP_VIEW,
    },
    ShortcutDef {
        accel: "0",
        label: strings::SC_FIT_VIEW,
        keys: &[Key::_0, Key::KP_0],
        ctrl: false,
        shift: Any,
        action: "win.fit-view",
        section: strings::SC_GROUP_VIEW,
    },
    ShortcutDef {
        accel: "1",
        label: strings::SC_ZOOM_100,
        keys: &[Key::_1, Key::KP_1],
        ctrl: false,
        shift: Any,
        action: "win.zoom-100",
        section: strings::SC_GROUP_VIEW,
    },
    ShortcutDef {
        accel: "h",
        label: strings::SC_MIRROR_H,
        keys: &[Key::h],
        ctrl: false,
        shift: Any,
        action: "win.mirror-h",
        section: strings::SC_GROUP_VIEW,
    },
    ShortcutDef {
        accel: "v",
        label: strings::SC_MIRROR_V,
        keys: &[Key::v],
        ctrl: false,
        shift: Any,
        action: "win.mirror-v",
        section: strings::SC_GROUP_VIEW,
    },
    ShortcutDef {
        accel: "bracketleft",
        label: strings::SC_ROTATE_CCW,
        keys: &[Key::bracketleft],
        ctrl: false,
        shift: Any,
        action: "win.rotate-ccw",
        section: strings::SC_GROUP_VIEW,
    },
    ShortcutDef {
        accel: "bracketright",
        label: strings::SC_ROTATE_CW,
        keys: &[Key::bracketright],
        ctrl: false,
        shift: Any,
        action: "win.rotate-cw",
        section: strings::SC_GROUP_VIEW,
    },
    ShortcutDef {
        accel: "F11",
        label: strings::SC_FULLSCREEN,
        keys: &[Key::F11],
        ctrl: false,
        shift: Any,
        action: "win.fullscreen",
        section: strings::SC_GROUP_VIEW,
    },
    // --- Sobreposições e medição ---
    ShortcutDef {
        accel: "g",
        label: strings::SC_GRID,
        keys: &[Key::g],
        ctrl: false,
        shift: Any,
        action: "win.toggle-grid",
        section: strings::SC_GROUP_MEASURE,
    },
    ShortcutDef {
        accel: "c",
        label: strings::SC_CROSSHAIR,
        keys: &[Key::c],
        ctrl: false,
        shift: Any,
        action: "win.toggle-crosshair",
        section: strings::SC_GROUP_MEASURE,
    },
    ShortcutDef {
        accel: "l",
        label: strings::SC_MEASURE_RULER,
        keys: &[Key::l],
        ctrl: false,
        shift: Any,
        action: "win.measure-ruler",
        section: strings::SC_GROUP_MEASURE,
    },
    ShortcutDef {
        accel: "a",
        label: strings::SC_MEASURE_ANGLE,
        keys: &[Key::a],
        ctrl: false,
        shift: Any,
        action: "win.measure-angle",
        section: strings::SC_GROUP_MEASURE,
    },
    ShortcutDef {
        accel: "o",
        label: strings::SC_MEASURE_CIRCLE,
        keys: &[Key::o],
        ctrl: false,
        shift: Any,
        action: "win.measure-circle",
        section: strings::SC_GROUP_MEASURE,
    },
    // --- Janela ---
    ShortcutDef {
        accel: "m",
        label: strings::SC_TOGGLE_MODE,
        keys: &[Key::m],
        ctrl: false,
        shift: Any,
        action: "win.toggle-mode",
        section: strings::SC_GROUP_WINDOW,
    },
    ShortcutDef {
        accel: "<Control>g",
        label: strings::SC_GALLERY,
        keys: &[Key::g],
        ctrl: true,
        shift: Any,
        action: "win.gallery",
        section: strings::SC_GROUP_WINDOW,
    },
    ShortcutDef {
        accel: "<Control>comma",
        label: strings::SC_PREFERENCES,
        keys: &[Key::comma],
        ctrl: true,
        shift: Any,
        action: "win.preferences",
        section: strings::SC_GROUP_WINDOW,
    },
    ShortcutDef {
        accel: "<Control>question",
        label: strings::SC_SHORTCUTS,
        keys: &[Key::question],
        ctrl: true,
        shift: Any,
        action: "win.shortcuts",
        section: strings::SC_GROUP_WINDOW,
    },
    ShortcutDef {
        accel: "Escape",
        label: strings::SC_ESCAPE,
        keys: &[Key::Escape],
        ctrl: false,
        shift: Any,
        action: "win.escape",
        section: strings::SC_GROUP_WINDOW,
    },
    ShortcutDef {
        accel: "<Control>q",
        label: strings::SC_QUIT,
        keys: &[Key::q],
        ctrl: true,
        shift: Any,
        action: "win.quit",
        section: strings::SC_GROUP_WINDOW,
    },
];

/// Keymap for the `win.*` actions (CONVENTIONS §13): resolves a
/// capture-phase key event to the action name, or `None` to let it
/// propagate. `Ctrl` bindings match first (so `Ctrl+G` is the gallery,
/// not the grid); `Alt`/`Super`/`Hyper`/`Meta` make a key
/// not-a-shortcut. Built from [`SHORTCUTS`] — same rows the window
/// lists.
pub fn shortcut_action(key: Key, state: ModifierType) -> Option<&'static str> {
    let key = key.to_lower();
    if state.contains(ModifierType::CONTROL_MASK) {
        return SHORTCUTS
            .iter()
            .find(|s| s.ctrl && s.keys.contains(&key))
            .map(|s| s.action);
    }
    if state.intersects(
        ModifierType::ALT_MASK
            | ModifierType::SUPER_MASK
            | ModifierType::HYPER_MASK
            | ModifierType::META_MASK,
    ) {
        return None;
    }
    let shifted = state.contains(ModifierType::SHIFT_MASK);
    SHORTCUTS
        .iter()
        .find(|s| {
            !s.ctrl
                && s.keys.contains(&key)
                && match s.shift {
                    Any => true,
                    Required => shifted,
                    Forbidden => !shifted,
                }
        })
        .map(|s| s.action)
}

/// Group titles in table order, for the window builder.
fn sections() -> Vec<&'static str> {
    let mut seen = Vec::new();
    for def in SHORTCUTS {
        if !seen.contains(&def.section) {
            seen.push(def.section);
        }
    }
    seen
}

/// Builds the `gtk::ShortcutsWindow` from [`SHORTCUTS`] — ONE
/// `ShortcutsSection` (so the section dropdown never appears) holding
/// one `ShortcutsGroup` per group title. Deprecated upstream but the
/// only built-in option on the pinned GTK 4.14 (§4); an `adw`
/// replacement does not exist yet.
#[allow(deprecated)]
pub fn build_window() -> gtk::ShortcutsWindow {
    let window = gtk::ShortcutsWindow::builder().build();
    let section = gtk::ShortcutsSection::builder()
        .section_name("default")
        .title(strings::SC_SECTION_TITLE)
        .build();
    for group_title in sections() {
        let group = gtk::ShortcutsGroup::builder().title(group_title).build();
        for def in SHORTCUTS.iter().filter(|d| d.section == group_title) {
            let row = gtk::ShortcutsShortcut::builder()
                .title(def.label)
                .accelerator(def.accel)
                .build();
            group.add_shortcut(&row);
        }
        section.add_group(&group);
    }
    window.add_section(&section);
    window
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Every row of the display table must resolve through
    /// `shortcut_action` when its first key is pressed with the
    /// declared modifiers — this is what keeps the window honest:
    /// the listed shortcut is exactly the one that fires.
    #[test]
    fn every_listed_shortcut_fires_its_action() {
        for def in SHORTCUTS {
            let mut state = ModifierType::empty();
            if def.ctrl {
                state |= ModifierType::CONTROL_MASK;
            }
            if def.shift == Shift::Required {
                state |= ModifierType::SHIFT_MASK;
            }
            assert_eq!(
                shortcut_action(def.keys[0], state),
                Some(def.action),
                "{} does not resolve to {}",
                def.accel,
                def.action
            );
        }
    }

    /// GTK-dependent check, called by `crate::tests::gtk_ui_contracts`
    /// (single thread — `gtk4::rt` panics on a second caller).
    /// Every `accel` string must be valid GTK accelerator syntax and
    /// parse to a real key — otherwise the window renders the row
    /// without any key, which is exactly the M11 review defect.
    pub(crate) fn check() {
        for def in SHORTCUTS {
            let Some((key, _mods)) = gtk::accelerator_parse(def.accel) else {
                panic!("'{}' is not valid accelerator syntax", def.accel);
            };
            // The parsed key must be one of the matched keys, so the
            // displayed binding and the functional binding cannot
            // diverge.
            assert!(
                def.keys.contains(&key.to_lower()),
                "'{}' parses to {key:?} but keys are {:?}",
                def.accel,
                def.keys
            );
        }
    }

    /// Shift+R must not also fire plain `record`; plain `r` must not
    /// reach `toggle-mic`.
    #[test]
    fn shift_r_and_r_are_exclusive() {
        assert_eq!(
            shortcut_action(Key::R, ModifierType::SHIFT_MASK),
            Some("win.toggle-mic")
        );
        assert_eq!(
            shortcut_action(Key::r, ModifierType::empty()),
            Some("win.record")
        );
    }

    /// Alternate keys for zoom are bound, and `Ctrl` bindings win
    /// over the same bare letter.
    #[test]
    fn alternate_keys_and_ctrl_precedence() {
        for key in [Key::plus, Key::equal, Key::KP_Add] {
            assert_eq!(
                shortcut_action(key, ModifierType::empty()),
                Some("win.zoom-in")
            );
        }
        assert_eq!(
            shortcut_action(Key::g, ModifierType::CONTROL_MASK),
            Some("win.gallery")
        );
        assert_eq!(
            shortcut_action(Key::question, ModifierType::CONTROL_MASK),
            Some("win.shortcuts")
        );
        assert_eq!(
            shortcut_action(Key::q, ModifierType::CONTROL_MASK),
            Some("win.quit")
        );
    }

    /// `Alt`/`Super` combos are never shortcuts.
    #[test]
    fn foreign_modifiers_do_not_match() {
        assert_eq!(shortcut_action(Key::g, ModifierType::ALT_MASK), None);
        assert_eq!(shortcut_action(Key::g, ModifierType::SUPER_MASK), None);
    }
}
