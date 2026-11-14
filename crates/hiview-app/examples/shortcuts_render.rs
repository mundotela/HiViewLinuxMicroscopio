// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Renders the `ShortcutsWindow` offscreen to a PNG (report aid).
//! Usage: cargo run -p hiview-app --example shortcuts_render -- out.png

// Tooling, not shipped code: the #[path]-included strings.rs declares
// hundreds of app constants the example does not use, and .expect() on
// init/render paths is the correct failure mode for a one-shot tool.
#![allow(dead_code, clippy::expect_used)]

/// Minimal stub so `strings.rs` compiles without pulling the whole app.
mod services {
    pub mod noise {
        pub enum NoiseCancel {
            Timeout,
            SizeChanged,
            Disconnected,
        }
    }
}

#[path = "../src/shortcuts.rs"]
mod shortcuts;
#[path = "../src/strings.rs"]
mod strings;

use relm4::gtk::{self, gdk, graphene, gsk, prelude::*};

fn main() {
    gtk::init().expect("GTK init");
    let window = shortcuts::build_window();
    window.present();

    // Pump the main loop so the window maps and gets a real allocation.
    for _ in 0..60 {
        while gtk::glib::MainContext::default().iteration(false) {}
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let width = window.width().max(1);
    let height = window.height().max(1);

    let paintable = gtk::WidgetPaintable::new(Some(&window));
    let snapshot = gtk::Snapshot::new();
    gdk::prelude::PaintableExt::snapshot(&paintable, &snapshot, width as f64, height as f64);
    let node = snapshot.to_node().expect("render node");
    let bounds = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);

    let renderer = gsk::CairoRenderer::new();
    renderer.realize(None).expect("renderer realize");
    let texture = renderer.render_texture(&node, Some(&bounds));

    let path = std::env::args().nth(1).expect("output path");
    texture.save_to_png(&path).expect("save png");
    println!("saved {path} ({width}x{height})");

    renderer.unrealize();
    window.close();
}
