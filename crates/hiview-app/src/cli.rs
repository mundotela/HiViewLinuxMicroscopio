// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Command-line interface (docs/CONVENTIONS.md §4).

use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;

/// Video capture source selected via `--source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Real microscope or any UVC camera, discovered automatically.
    Device,
    /// Synthetic `videotestsrc` pattern, for testing without hardware.
    Test,
    /// Loop playback of a recorded video file.
    File(PathBuf),
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Device => f.write_str("device"),
            Self::Test => f.write_str("test"),
            Self::File(path) => write!(f, "file:{}", path.display()),
        }
    }
}

impl FromStr for Source {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "device" => Ok(Self::Device),
            "test" => Ok(Self::Test),
            _ => match s.strip_prefix("file:") {
                Some(path) if !path.is_empty() => Ok(Self::File(PathBuf::from(path))),
                _ => Err(format!(
                    "invalid source '{s}': expected 'device', 'test' or 'file:<path>'"
                )),
            },
        }
    }
}

/// HiView — USB microscope viewer.
#[derive(Debug, Parser)]
#[command(name = "hiview", version, about)]
pub struct Args {
    /// Video capture source: 'device', 'test' or 'file:<path>'.
    #[arg(long, value_name = "SOURCE", default_value = "device")]
    pub source: Source,

    /// Show the GTK inspector overlay with FPS and render statistics.
    #[arg(long)]
    pub debug_overlay: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_parses_device_and_test() {
        assert_eq!("device".parse::<Source>(), Ok(Source::Device));
        assert_eq!("test".parse::<Source>(), Ok(Source::Test));
    }

    #[test]
    fn source_parses_file_with_path() {
        assert_eq!(
            "file:videos/sample.mp4".parse::<Source>(),
            Ok(Source::File(PathBuf::from("videos/sample.mp4")))
        );
    }

    #[test]
    fn source_rejects_empty_path_and_unknown_values() {
        assert!("file:".parse::<Source>().is_err());
        assert!("camera".parse::<Source>().is_err());
        assert!("".parse::<Source>().is_err());
    }
}
