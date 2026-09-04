// SPDX-License-Identifier: GPL-3.0-or-later

mod app_page;
mod app_row;
mod app_window;
mod application;
mod config;
mod create_app_dialog;
mod home_page;
mod import_dialog;
mod launcher;
mod legacy;
mod model;
mod repository;
mod service;
mod util;
mod window;

use application::BastleApplication;
use config::{GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR};
use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};
use gtk::{gio, glib, prelude::*};
use window::BastleWindow;

fn main() -> glib::ExitCode {
    let gettext_result = bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR)
        .and_then(|_| bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8"))
        .and_then(|_| textdomain(GETTEXT_PACKAGE));
    if let Err(error) = gettext_result {
        eprintln!("Failed to initialize translations: {error}");
        return glib::ExitCode::FAILURE;
    }

    #[cfg(feature = "ui-tests")]
    let resource_path = std::env::var("BASTLE_TEST_RESOURCE")
        .unwrap_or_else(|_| format!("{PKGDATADIR}/bastle.gresource"));
    #[cfg(not(feature = "ui-tests"))]
    let resource_path = format!("{PKGDATADIR}/bastle.gresource");
    let resources = match gio::Resource::load(&resource_path) {
        Ok(resources) => resources,
        Err(error) => {
            eprintln!("Failed to load {resource_path}: {error}");
            return glib::ExitCode::FAILURE;
        }
    };
    gio::resources_register(&resources);

    BastleApplication::new(
        gio::ApplicationFlags::HANDLES_COMMAND_LINE | gio::ApplicationFlags::NON_UNIQUE,
    )
    .run()
}
