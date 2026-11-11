// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Persistent configuration (docs/ARCHITECTURE.md §6.12): `AppConfig`
//! serialized as TOML at `$XDG_CONFIG_HOME/hiview/config.toml`.
//!
//! An invalid file never crashes the app: it is renamed to
//! `config.toml.bak` and defaults are loaded (the UI shows a warning).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::imaging::{PhotoFormat, PhotoResolution};

/// Top-level application configuration (§6.12).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Photo preferences.
    pub photo: PhotoConfig,
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
        save_config(&config, &path).unwrap_or_else(|e| panic!("save: {e}"));
        match load_config(&path) {
            ConfigLoad::Loaded(loaded) => assert_eq!(loaded, config),
            _ => panic!("expected Loaded"),
        }
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
