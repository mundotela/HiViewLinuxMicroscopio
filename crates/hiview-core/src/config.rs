// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Persistent configuration (docs/ARCHITECTURE.md §6.12): `AppConfig`
//! serialized as TOML at `$XDG_CONFIG_HOME/hiview/config.toml`.
//!
//! An invalid file never crashes the app: it is renamed to
//! `config.toml.bak` and defaults are loaded (the UI shows a warning).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adjust::ImageAdjust;
use crate::error::Result;
use crate::imaging::{PhotoFormat, PhotoResolution};
use crate::measure::CalibrationProfile;
use crate::overlay::{AppMode, OverlayState};

/// Top-level application configuration (§6.12).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Photo preferences.
    pub photo: PhotoConfig,
    /// Explorar/Bancada mode (`M` toggles it).
    pub mode: AppMode,
    /// Grid and crosshair overlay settings (§6.9).
    pub overlays: OverlayState,
    /// Software image adjustments per device, keyed by `"VID:PID"` (or a
    /// stable id for non-USB cameras; `"test"`/`"file"` for the fixed
    /// CLI sources). Serialized as `[devices."05e3:f12a"]` (§6.12).
    pub devices: BTreeMap<String, ImageAdjust>,
    /// Named calibration profiles, serialized as `[[calibration]]`
    /// (§6.12).
    pub calibration: Vec<CalibrationProfile>,
    /// Name of the profile whose `px_per_mm` converts measurements
    /// (`active_calibration = "Anel no máximo"`); `None` → results in
    /// pixels.
    pub active_calibration: Option<String>,
}

impl AppConfig {
    /// The stored adjustments for `key`, neutral defaults when absent.
    #[must_use]
    pub fn adjust_for(&self, key: &str) -> ImageAdjust {
        self.devices.get(key).copied().unwrap_or_default().clamped()
    }

    /// Stores `adjust` under `key`; a default value removes the entry so
    /// untouched devices keep the file small.
    pub fn set_adjust_for(&mut self, key: &str, adjust: ImageAdjust) {
        if adjust.is_default() {
            self.devices.remove(key);
        } else {
            self.devices.insert(key.to_string(), adjust.clamped());
        }
    }

    /// The active calibration profile, resolved by name. A dangling
    /// `active_calibration` (hand-edited or a deleted profile) behaves
    /// like no calibration — results in pixels (§6.9).
    #[must_use]
    pub fn active_profile(&self) -> Option<&CalibrationProfile> {
        self.active_calibration
            .as_deref()
            .and_then(|name| self.calibration.iter().find(|p| p.name == name))
    }

    /// Adds or replaces a profile and makes it active. Names are the
    /// identity — same-name entries are overwritten so the combo never
    /// shows duplicates.
    pub fn upsert_profile(&mut self, profile: CalibrationProfile) {
        if let Some(existing) = self.calibration.iter_mut().find(|p| p.name == profile.name) {
            *existing = profile.clone();
        } else {
            self.calibration.push(profile.clone());
        }
        self.active_calibration = Some(profile.name);
    }

    /// Removes the named profile; clears `active_calibration` when it
    /// was the active one.
    pub fn remove_profile(&mut self, name: &str) {
        self.calibration.retain(|p| p.name != name);
        if self.active_calibration.as_deref() == Some(name) {
            self.active_calibration = None;
        }
    }
}

/// Photo preferences shown in the "Fotos" preferences page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhotoConfig {
    /// File format (PNG default).
    pub format: PhotoFormat,
    /// Saved-photo resolution (native default).
    pub resolution: PhotoResolution,
    /// Custom save folder. `None` uses the localized
    /// `Pictures/HiView` default resolved by the app layer.
    pub folder: Option<PathBuf>,
    /// Burn the grid/crosshair overlays into the saved photo (§6.6).
    pub burn_overlays: bool,
}

/// Result of [`load_config`].
pub enum ConfigLoad {
    /// File existed and parsed.
    Loaded(AppConfig),
    /// File missing or unreadable — defaults loaded.
    Defaults(AppConfig),
    /// File was invalid — renamed to `.bak`, defaults loaded. The UI
    /// should warn the user.
    Recovered(AppConfig),
}

impl ConfigLoad {
    /// The configuration to use, whichever branch loaded.
    pub fn into_config(self) -> AppConfig {
        match self {
            Self::Loaded(c) | Self::Defaults(c) | Self::Recovered(c) => c,
        }
    }

    /// Whether an invalid file was renamed to `.bak`.
    pub fn was_recovered(&self) -> bool {
        matches!(self, Self::Recovered(_))
    }
}

/// Absolute path of the configuration file:
/// `$XDG_CONFIG_HOME/hiview/config.toml` (or `~/.config/…`).
#[must_use]
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(
            || {
                std::env::var_os("HOME").map_or_else(
                    || PathBuf::from(".config"),
                    |home| PathBuf::from(home).join(".config"),
                )
            },
            PathBuf::from,
        );
    base.join("hiview").join("config.toml")
}

/// Loads the config from `path` (§6.12 recovery rules).
pub fn load_config(path: &Path) -> ConfigLoad {
    match fs::read_to_string(path) {
        Err(_) => ConfigLoad::Defaults(AppConfig::default()),
        Ok(text) => match toml::from_str::<AppConfig>(&text) {
            Ok(config) => ConfigLoad::Loaded(config),
            Err(_) => {
                let backup = path.with_extension("toml.bak");
                if fs::rename(path, &backup).is_err() {
                    tracing::warn!("could not back up invalid config {backup:?}");
                }
                ConfigLoad::Recovered(AppConfig::default())
            }
        },
    }
}

/// Serializes the config to `path`, creating parent directories.
///
/// # Errors
/// Returns [`crate::error::Error`] on serialization or I/O failure.
pub fn save_config(config: &AppConfig, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config)?;
    fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("sub").join("config.toml");
        let mut config = AppConfig::default();
        config.photo.format = PhotoFormat::Jpeg;
        config.photo.resolution = PhotoResolution::X2p5;
        config.photo.folder = Some(PathBuf::from("/tmp/minhas-fotos"));
        config.photo.burn_overlays = true;
        config.mode = crate::overlay::AppMode::Bench;
        config.overlays.grid.enabled = true;
        config.overlays.grid.spacing = crate::overlay::GridSpacing::Pixels(55.0);
        config.overlays.grid.color = crate::overlay::OverlayColor::CYAN;
        config.overlays.grid.opacity = 0.42;
        config.overlays.crosshair.enabled = true;
        config.overlays.crosshair.style = crate::overlay::CrosshairStyle::Ticks;
        config.overlays.crosshair.color = crate::overlay::OverlayColor::WHITE;
        config.overlays.crosshair.tick_spacing = crate::overlay::GridSpacing::Pixels(7.5);
        config.set_adjust_for(
            "05e3:f12a",
            crate::adjust::ImageAdjust {
                brightness: 0.2,
                contrast: 1.4,
                saturation: 1.1,
                gamma: 1.8,
                sharpen: 0.35,
            },
        );
        config.set_adjust_for(
            "test",
            crate::adjust::ImageAdjust {
                sharpen: 1.0,
                ..crate::adjust::ImageAdjust::default()
            },
        );
        save_config(&config, &path).unwrap_or_else(|e| panic!("save: {e}"));
        match load_config(&path) {
            ConfigLoad::Loaded(loaded) => assert_eq!(loaded, config),
            _ => panic!("expected Loaded"),
        }
    }

    /// §6.12: adjustments live under `[devices."VID:PID"]` in TOML and
    /// come back keyed the same way — this is what lets a reconnect or
    /// an app restart restore the same values.
    #[test]
    fn devices_section_round_trips_by_vid_pid() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("config.toml");
        let mut config = AppConfig::default();
        config.set_adjust_for(
            "05e3:f12a",
            crate::adjust::ImageAdjust {
                brightness: -0.4,
                ..crate::adjust::ImageAdjust::default()
            },
        );
        save_config(&config, &path).unwrap_or_else(|e| panic!("save: {e}"));

        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read: {e}"));
        assert!(
            text.contains("[devices.\"05e3:f12a\"]"),
            "expected a [devices.\"05e3:f12a\"] section, got:\n{text}"
        );

        match load_config(&path) {
            ConfigLoad::Loaded(loaded) => {
                let adjust = loaded.adjust_for("05e3:f12a");
                assert_eq!(adjust.brightness, -0.4);
                assert_eq!(adjust.contrast, 1.0);
                // Unknown device falls back to neutral defaults.
                assert!(loaded.adjust_for("0408:403a").is_default());
            }
            _ => panic!("expected Loaded"),
        }
    }

    /// A device whose values all return to neutral is removed from the
    /// map, so the config file does not accumulate empty sections.
    #[test]
    fn defaults_remove_the_device_entry() {
        let mut config = AppConfig::default();
        let tuned = crate::adjust::ImageAdjust {
            contrast: 1.7,
            ..crate::adjust::ImageAdjust::default()
        };
        config.set_adjust_for("05e3:f12a", tuned);
        assert_eq!(config.adjust_for("05e3:f12a"), tuned);
        config.set_adjust_for("05e3:f12a", crate::adjust::ImageAdjust::default());
        assert!(!config.devices.contains_key("05e3:f12a"));
    }

    /// §6.12: calibration profiles live under `[[calibration]]` and the
    /// active one under `active_calibration` — full CRUD round-trip.
    #[test]
    fn calibration_profiles_round_trip() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("config.toml");
        let mut config = AppConfig::default();
        config.upsert_profile(crate::measure::CalibrationProfile {
            name: "Anel no máximo".into(),
            px_per_mm: 89.0,
        });
        config.upsert_profile(crate::measure::CalibrationProfile {
            name: "Anel no mínimo".into(),
            px_per_mm: 31.5,
        });
        config.active_calibration = Some("Anel no máximo".into());
        save_config(&config, &path).unwrap_or_else(|e| panic!("save: {e}"));

        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read: {e}"));
        assert!(
            text.contains("[[calibration]]"),
            "expected a [[calibration]] section, got:\n{text}"
        );
        assert!(text.contains("active_calibration"));

        match load_config(&path) {
            ConfigLoad::Loaded(loaded) => {
                assert_eq!(loaded, config);
                let active = loaded
                    .active_profile()
                    .unwrap_or_else(|| panic!("active profile"));
                assert_eq!(active.name, "Anel no máximo");
                assert!((active.px_per_mm - 89.0).abs() < 1e-9);
            }
            _ => panic!("expected Loaded"),
        }
    }

    /// The wizard flow end-to-end at the domain level (§6.9): a
    /// Calibrate drag commits a `CalibrationLine` — the line becomes
    /// a profile that is upserted, persisted and active after reload.
    /// Regression for the "Criar perfil creates nothing" report.
    #[test]
    fn wizard_create_persists_and_activates() {
        use crate::measure::{MeasureGesture, MeasureSet, MeasureTool};

        let mut set = MeasureSet::default();
        set.arm(Some(MeasureTool::Calibrate));
        let commit = set.gesture(MeasureGesture::DragStart(crate::view::Point::new(0.0, 0.0)));
        assert!(matches!(commit, crate::measure::MeasureCommit::Nothing));
        let commit = set.gesture(MeasureGesture::DragEnd(crate::view::Point::new(188.0, 0.0)));
        let crate::measure::MeasureCommit::CalibrationLine { a, b } = commit else {
            panic!("expected CalibrationLine");
        };

        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("config.toml");
        let mut config = AppConfig::default();
        let profile = crate::measure::CalibrationProfile::from_line(a, b, 1.0, "Lâmina".into())
            .unwrap_or_else(|| panic!("line is valid"));
        config.upsert_profile(profile);
        save_config(&config, &path).unwrap_or_else(|e| panic!("save: {e}"));

        match load_config(&path) {
            ConfigLoad::Loaded(loaded) => {
                let active = loaded
                    .active_profile()
                    .unwrap_or_else(|| panic!("the new profile is active"));
                assert_eq!(active.name, "Lâmina");
                assert!((active.px_per_mm - 188.0).abs() < 1e-9);
            }
            _ => panic!("expected Loaded"),
        }
    }

    /// The same-name upsert edits in place (the combo never lists
    /// duplicates), and removing the active profile clears the pointer.
    #[test]
    fn profile_upsert_and_remove() {
        let mut config = AppConfig::default();
        config.upsert_profile(crate::measure::CalibrationProfile {
            name: "a".into(),
            px_per_mm: 10.0,
        });
        config.upsert_profile(crate::measure::CalibrationProfile {
            name: "a".into(),
            px_per_mm: 20.0,
        });
        assert_eq!(config.calibration.len(), 1);
        assert_eq!(config.active_profile().map(|p| p.px_per_mm), Some(20.0));

        config.remove_profile("a");
        assert!(config.calibration.is_empty());
        assert!(config.active_profile().is_none());
        assert_eq!(config.active_calibration, None);
        // Removing an absent profile touches nothing.
        config.remove_profile("ghost");
    }

    /// A dangling `active_calibration` name (hand-edited file, deleted
    /// profile) resolves to no calibration — pixel results, no crash.
    #[test]
    fn dangling_active_name_is_no_calibration() {
        let config = AppConfig {
            active_calibration: Some("gone".into()),
            ..AppConfig::default()
        };
        assert!(config.active_profile().is_none());
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        match load_config(&dir.path().join("nope.toml")) {
            ConfigLoad::Defaults(c) => assert_eq!(c, AppConfig::default()),
            _ => panic!("expected Defaults"),
        }
    }

    #[test]
    fn invalid_config_is_backed_up_and_defaults_loaded() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("config.toml");
        fs::write(&path, "this is not [valid toml {{{").unwrap_or_else(|e| panic!("write: {e}"));
        let outcome = load_config(&path);
        assert!(outcome.was_recovered());
        assert!(!path.exists());
        assert!(path.with_extension("toml.bak").exists());
    }

    #[test]
    fn wrong_types_are_treated_as_invalid() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("config.toml");
        fs::write(&path, "[photo]\nformat = 42\n").unwrap_or_else(|e| panic!("write: {e}"));
        assert!(load_config(&path).was_recovered());
    }

    #[test]
    fn partial_config_fills_missing_fields_with_defaults() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("config.toml");
        fs::write(&path, "[photo]\nformat = \"jpeg\"\n").unwrap_or_else(|e| panic!("write: {e}"));
        match load_config(&path) {
            ConfigLoad::Loaded(c) => {
                assert_eq!(c.photo.format, PhotoFormat::Jpeg);
                assert_eq!(c.photo.resolution, PhotoResolution::Native);
                assert_eq!(c.photo.folder, None);
            }
            _ => panic!("expected Loaded"),
        }
    }
}
