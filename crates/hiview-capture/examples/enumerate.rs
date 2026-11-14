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
            }
        }
        Err(e) => {
            eprintln!("ERR: {e}");
            exit(1);
        }
    }
}
