// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! First-run welcome tour (M11): a three-page `adw::Carousel` in an
//! `adw::Dialog`, shown once (a `[ui]` config flag) and reopenable
//! from the ☰ menu. Simple Portuguese, big icons, "Pular" always
//! available.

use relm4::adw::prelude::*;
use relm4::{adw, gtk};

use crate::strings;

/// The three pages: symbolic icon, title, body.
const PAGES: [(&str, &str, &str); 3] = [
    (
        "camera-web-symbolic",
        strings::TOUR_P1_TITLE,
        strings::TOUR_P1_BODY,
    ),
    (
        "media-record-symbolic",
        strings::TOUR_P2_TITLE,
        strings::TOUR_P2_BODY,
    ),
    (
        "applications-engineering-symbolic",
        strings::TOUR_P3_TITLE,
        strings::TOUR_P3_BODY,
    ),
];

fn page(icon: &str, title: &str, body: &str) -> gtk::Widget {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 18);
    vbox.set_valign(gtk::Align::Center);
    vbox.set_margin_top(24);
    vbox.set_margin_bottom(24);
    vbox.set_margin_start(36);
    vbox.set_margin_end(36);

    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(96);
    image.add_css_class("accent");
    vbox.append(&image);

    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("title-1");
    heading.set_wrap(true);
    heading.set_justify(gtk::Justification::Center);
    vbox.append(&heading);

    let text = gtk::Label::new(Some(body));
    text.set_wrap(true);
    text.set_justify(gtk::Justification::Center);
    text.set_max_width_chars(44);
    text.add_css_class("body");
    vbox.append(&text);

    vbox.upcast()
}

/// Builds the tour dialog. `on_finish` runs exactly once when the user
/// leaves the tour — via "Começar", "Pular" or the header close
/// button — so the caller can persist the seen flag.
pub fn build(on_finish: impl Fn() + 'static) -> adw::Dialog {
    let dialog = adw::Dialog::new();
    dialog.set_title(strings::TOUR_TITLE);
    dialog.set_content_width(480);
    dialog.set_content_height(460);

    let finished = std::cell::Cell::new(false);
    let on_finish = std::rc::Rc::new(on_finish);
    let finish = {
        let on_finish = on_finish.clone();
        let finished = finished.clone();
        move |dialog: &adw::Dialog| {
            if !finished.replace(true) {
                on_finish();
            }
            let _ = dialog.close();
        }
    };

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    header.set_show_start_title_buttons(false);
    toolbar.add_top_bar(&header);

    let carousel = adw::Carousel::new();
    carousel.set_vexpand(true);
    for (icon, title, body) in PAGES {
        carousel.append(&page(icon, title, body));
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&carousel);

    let dots = adw::CarouselIndicatorDots::new();
    dots.set_carousel(Some(&carousel));
    content.append(&dots);

    // Bottom bar: "Pular" on the left, "Avançar"→"Começar" suggested
    // on the right.
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    buttons.set_margin_top(12);
    buttons.set_margin_bottom(18);
    buttons.set_margin_start(18);
    buttons.set_margin_end(18);
    buttons.set_homogeneous(true);

    let skip = gtk::Button::with_label(strings::TOUR_SKIP);
    skip.set_halign(gtk::Align::Start);
    skip.add_css_class("flat");
    buttons.append(&skip);

    let next = gtk::Button::with_label(strings::TOUR_NEXT);
    next.set_halign(gtk::Align::End);
    next.add_css_class("suggested-action");
    next.add_css_class("pill");
    buttons.append(&next);
    content.append(&buttons);
    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));

    // Keep the primary button honest: "Avançar" until the last page,
    // then "Começar". Page changes also happen by swipe/arrows, so
    // watch `position` instead of assuming our own clicks.
    let on_position = {
        let next = next.clone();
        let carousel = carousel.clone();
        move || {
            let last = carousel.position() as i64 >= i64::from(PAGES.len() as i32 - 1);
            next.set_label(if last {
                strings::TOUR_START
            } else {
                strings::TOUR_NEXT
            });
        }
    };
    carousel.connect_position_notify({
        let on_position = on_position.clone();
        move |_| on_position()
    });

    next.connect_clicked({
        let carousel = carousel.clone();
        let dialog = dialog.clone();
        let finish = finish.clone();
        move |_| {
            let last = carousel.position() as i64 >= i64::from(PAGES.len() as i32 - 1);
            if last {
                finish(&dialog);
            } else {
                let page = carousel.nth_page(carousel.position() as u32 + 1);
                carousel.scroll_to(&page, true);
            }
        }
    });
    skip.connect_clicked({
        let dialog = dialog.clone();
        let finish = finish.clone();
        move |_| finish(&dialog)
    });

    // Closing via the header X (or Escape) counts as finished too.
    dialog.connect_closed({
        let finish = finish.clone();
        move |dialog| finish(dialog)
    });

    dialog
}
