// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Live video viewport.
//!
//! M1 renders frames into a `gtk::Picture` configured with
//! `ContentFit::Contain`, which is exactly "fit to window": the image is
//! scaled down/up to the largest size that fits while preserving aspect
//! ratio. Zoom and transforms arrive in M5.

use gtk;
use hiview_capture::Frame;

/// Uploads a captured frame into the viewport widget.
///
/// `MemoryTexture` references the frame's `glib::Bytes` without copying the
/// pixel data; the texture keeps the bytes alive for as long as it needs.
pub fn set_frame(picture: &gtk::Picture, frame: &Frame) {
    debug_assert_eq!(frame.format, hiview_capture::PixelFormat::Rgba);
    let texture = gtk::gdk::MemoryTexture::new(
        frame.width as i32,
        frame.height as i32,
        gtk::gdk::MemoryFormat::R8g8b8a8,
        &frame.data,
        frame.stride,
    );
    picture.set_paintable(Some(&texture));
}

/// Clears the viewport back to empty.
pub fn clear(picture: &gtk::Picture) {
    picture.set_paintable(None::<&gtk::gdk::Paintable>);
}
