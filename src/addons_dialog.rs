// SPDX-License-Identifier: GPL-3.0-only

use adw::prelude::*;
use gettextrs::gettext;
use gtk::gio;

use crate::chromium::EngineAvailability;

const CHROMIUM_REF: &str = "https://cheviiot.github.io/bastle/bastle-chromium.flatpakref";

pub fn present(parent: &gtk::Window, availability: &EngineAvailability) {
    let dialog = adw::Dialog::builder()
        .title(gettext("Add-ons"))
        .content_width(560)
        .content_height(440)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Browser Engines"))
        .description(gettext(
            "Optional engines are installed separately and become available after Bastle is restarted.",
        ))
        .build();
    let row = adw::ActionRow::builder()
        .title(gettext("Chromium Engine"))
        .subtitle(availability_subtitle(availability))
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("web-browser-symbolic"));
    let button = gtk::Button::builder()
        .label(availability_action(availability))
        .valign(gtk::Align::Center)
        .build();
    if availability.is_available() {
        button.add_css_class("flat");
    } else {
        button.add_css_class("suggested-action");
    }
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    group.add(&row);

    let restart = adw::ActionRow::builder()
        .title(gettext("Restart Required"))
        .subtitle(gettext(
            "Close and reopen Bastle after installing or removing this add-on.",
        ))
        .build();
    restart.add_prefix(&gtk::Image::from_icon_name("view-refresh-symbolic"));
    group.add(&restart);
    page.add(&group);
    toolbar.set_content(Some(&page));
    dialog.set_child(Some(&toolbar));

    let alert_parent = parent.clone();
    button.connect_clicked(move |_| {
        if let Err(error) =
            gio::AppInfo::launch_default_for_uri(CHROMIUM_REF, None::<&gio::AppLaunchContext>)
        {
            let alert = adw::AlertDialog::new(
                Some(&gettext("Could Not Open the Add-on Installer")),
                Some(&error.to_string()),
            );
            alert.add_response("close", &gettext("Close"));
            alert.present(Some(&alert_parent));
        }
    });

    dialog.present(Some(parent));
}

fn availability_subtitle(availability: &EngineAvailability) -> String {
    match availability {
        EngineAvailability::Missing => gettext("Not installed — WebKitGTK remains the default"),
        EngineAvailability::Available(_) => gettext("Installed and ready"),
        EngineAvailability::Incompatible(message) => {
            format!("{}: {message}", gettext("Update required"))
        }
        EngineAvailability::Broken(message) => {
            format!("{}: {message}", gettext("Installed but unavailable"))
        }
    }
}

fn availability_action(availability: &EngineAvailability) -> String {
    match availability {
        EngineAvailability::Missing => gettext("Install"),
        EngineAvailability::Available(_) => gettext("Manage"),
        EngineAvailability::Incompatible(_) | EngineAvailability::Broken(_) => gettext("Reinstall"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_addon_is_presented_as_optional() {
        assert!(availability_subtitle(&EngineAvailability::Missing).contains("WebKitGTK"));
        assert_eq!(availability_action(&EngineAvailability::Missing), "Install");
    }
}
