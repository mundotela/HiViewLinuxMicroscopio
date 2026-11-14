// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! M11 diagnostic tool (not part of the app): prints what camera
//! discovery returns, to check v4l2 enumeration on systems where
//! `gstreamer1.0-pipewire` shadows `gst::DeviceMonitor` devices.
//!
//! Usage: cargo run -p hiview-capture --example enumerate

use std::process::exit;

fn main() {
    if let Err(err) = hiview_capture::init() {
        eprintln!("gstreamer init: {err}");
        exit(1);
    }
    match hiview_capture::enumerate() {
        Ok(cams) => {
            println!("{} camera(s) found", cams.len());
            for c in &cams {
                println!(
                    "  {} name={} vid={:04x?}:{:04x?} microscope={} modes={}",
                    c.dev_node.display(),
                    c.name,
                    c.usb_vid,
                    c.usb_pid,
                    c.is_microscope,
                    c.modes.len()
                );
                for m in &c.modes {
                    println!(
                        "      {} {}x{} fps={:?}",
                        m.fourcc, m.width, m.height, m.frame_rates
                    );
                }
                if let Some(pick) = hiview_capture::select_default_mode(&c.modes) {
                    println!(
                        "      pick: {} {}x{} {:?}",
                        pick.fourcc,
                        pick.width,
                        pick.height,
                        hiview_capture::best_frame_rate(&pick)
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("ERR: {e}");
            exit(1);
        }
    }
}
