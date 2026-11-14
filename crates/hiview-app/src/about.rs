// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! The About dialog (M11). No repository link — it is private.

use relm4::{adw, gtk};

use crate::strings;

/// Builds the About dialog (presented by the caller).
pub fn build() -> adw::AboutDialog {
    let about = adw::AboutDialog::new();
    about.set_application_name(strings::APP_TITLE);
    about.set_application_icon(strings::APP_ID);
    about.set_version(env!("CARGO_PKG_VERSION"));
    about.set_developer_name("MundoTela Tecnologia");
    about.set_developers(&[strings::ABOUT_DEV_HENRIQUE]);
    about.set_copyright("© 2026 MundoTela Tecnologia");
    about.add_credit_section(
        Some(strings::ABOUT_AI_CREDIT_TITLE),
        strings::ABOUT_AI_CREDITS,
    );
    about.set_license_type(gtk::License::Gpl30);
    about
}
