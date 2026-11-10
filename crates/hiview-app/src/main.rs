//! hiview — HiView desktop application.
//!
//! This file contains only the CLI parsing, the logging setup and the Relm4
//! application entry point (docs/CONVENTIONS.md §4).

mod app;
mod strings;

use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;

/// Video capture source selected via `--source`.
///
/// Only parsed and logged for now; it takes effect starting in M1.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// Real microscope or any UVC camera chosen automatically.
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
struct Args {
    /// Video capture source: 'device', 'test' or 'file:<path>'.
    #[arg(long, value_name = "SOURCE", default_value = "device")]
    source: Source,
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        source = %args.source,
        "starting hiview"
    );

    let app = relm4::RelmApp::new(strings::APP_ID);
    // GIO re-parses argv and would reject our own flags ("unknown option"),
    // so only the program name is forwarded.
    app.with_args(std::env::args().take(1).collect())
        .run::<app::AppModel>(());
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
