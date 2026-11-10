// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! hiview — HiView desktop application.
//!
//! This file contains only the CLI parsing, the logging setup and the Relm4
//! application entry point (docs/CONVENTIONS.md §4).

mod app;
mod cli;
mod services;
mod strings;
mod widgets;

use clap::Parser;

fn main() {
    let args = cli::Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        source = %args.source,
        debug_overlay = args.debug_overlay,
        "starting hiview"
    );

    if let Err(err) = hiview_capture::init() {
        // The app still starts: the error page explains the failure and the
        // details stay in the log.
        tracing::error!("gstreamer initialization failed: {err}");
    }

    let app = relm4::RelmApp::new(strings::APP_ID);
    relm4::set_global_css(app::GLOBAL_CSS);
    // GIO re-parses argv and would reject our own flags ("unknown option"),
    // so only the program name is forwarded.
    app.with_args(std::env::args().take(1).collect())
        .run::<app::AppModel>(args);
}
