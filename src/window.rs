// SPDX-License-Identifier: GPL-3.0-or-later

use std::str::FromStr;

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::{
    app_page::AppPage, app_row::AppRow, application::settings, background, backup_dialog,
    create_app_dialog::CreateAppDialog, home_page::HomePage, launcher::PortalLauncher,
    model::AppId, permissions_dialog, privacy_dialog, service::AppService,
};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/bastle/window.ui")]
    pub struct BastleWindow {
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub apps_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BastleWindow {
        const NAME: &'static str = "BastleWindow";
        type Type = super::BastleWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for BastleWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let window = self.obj();
            window.setup_gactions();
            window.load_window_size();
            window.refresh();
        }
    }
    impl WidgetImpl for BastleWindow {}
    impl WindowImpl for BastleWindow {
        fn close_request(&self) -> glib::Propagation {
            let (width, height) = self.obj().default_size();
            let settings = settings();
            if let Err(error) = settings.set_int("window-width", width) {
                eprintln!("Failed to save main window width: {error}");
            }
            if let Err(error) = settings.set_int("window-height", height) {
                eprintln!("Failed to save main window height: {error}");
            }
            glib::Propagation::Proceed
        }
    }
    impl ApplicationWindowImpl for BastleWindow {}
    impl AdwApplicationWindowImpl for BastleWindow {}

    #[gtk::template_callbacks]
    impl BastleWindow {
        #[template_callback]
        fn on_add_clicked(&self, _button: gtk::Button) {
            CreateAppDialog::new().present(Some(&*self.obj()));
        }

        #[template_callback]
        fn on_app_selected(&self, row: Option<AppRow>) {
            if let Some(config) = row.and_then(|row| row.config()) {
                self.split_view.set_content(Some(&AppPage::new(config)));
                self.split_view.set_show_content(true);
            }
        }
    }
}

glib::wrapper! {
    pub struct BastleWindow(ObjectSubclass<imp::BastleWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl BastleWindow {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        // The window template constructs this custom widget by its GType name,
        // so register it before GTK parses the template.
        let _ = HomePage::static_type();
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("refresh")
                .activate(|window: &Self, _, _| window.refresh())
                .build(),
            gio::ActionEntry::builder("notify")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(message) = parameter.and_then(|value| value.get::<String>()) {
                        window.toast(&message);
                    }
                })
                .build(),
            gio::ActionEntry::builder("delete")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        window.confirm_delete(id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("repair")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        window.repair(id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("permissions")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        permissions_dialog::start(window.upcast_ref(), id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("privacy")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        privacy_dialog::start(window, id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("backup")
                .activate(|window: &Self, _, _| backup_dialog::start_backup(window))
                .build(),
            gio::ActionEntry::builder("restore")
                .activate(|window: &Self, _, _| backup_dialog::start_restore(window))
                .build(),
            gio::ActionEntry::builder("capabilities")
                .activate(|window: &Self, _, _| window.show_capabilities())
                .build(),
        ]);
    }

    fn show_capabilities(&self) {
        let window = self.clone();
        glib::spawn_future_local(async move {
            let background_capability = background::capability()
                .await
                .map(|version| format!("{} (v{version})", gettext("Available")))
                .unwrap_or_else(|error| format!("{} ({error})", gettext("Unavailable")));
            let (heading, body) = match PortalLauncher::capabilities().await {
                Ok(capabilities) => (
                    gettext("Portal Available"),
                    format!(
                        "{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n\n{}",
                        gettext("Desktop session"),
                        capabilities.desktop,
                        gettext("Dynamic Launcher version"),
                        capabilities.portal_version,
                        gettext("Application launchers"),
                        availability(capabilities.application_launchers),
                        gettext("Web application launchers"),
                        availability(capabilities.web_application_launchers),
                        gettext("Background activity"),
                        background_capability,
                        gettext("Bastle uses portals only and never writes launchers directly to the host."),
                    ),
                ),
                Err(error) => (
                    gettext("Portal Unavailable"),
                    format!(
                        "{error:#}\n\n{}",
                        gettext("Creating, repairing, and restoring applications requires a Dynamic Launcher Portal implementation for this desktop session.")
                    ),
                ),
            };
            let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
            dialog.add_response("close", &gettext("Close"));
            dialog.set_default_response(Some("close"));
            dialog.set_close_response("close");
            dialog.present(Some(&window));
        });
    }

    fn confirm_delete(&self, id: AppId) {
        let window = self.clone();
        glib::spawn_future_local(async move {
            let dialog = adw::AlertDialog::new(
                Some(&gettext("Delete this application?")),
                Some(&gettext(
                    "Its launcher, settings, WebKit profile, cookies, and cache will be removed.",
                )),
            );
            dialog.add_responses(&[
                ("cancel", &gettext("Cancel")),
                ("delete", &gettext("Delete")),
            ]);
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            if dialog.choose_future(Some(&window)).await == "delete" {
                let result = AppService::portal().delete(&id).await;
                match result {
                    Ok(_) => {
                        window.refresh();
                        window.toast(&gettext("Application deleted"));
                    }
                    Err(error) => window.toast(&error.to_string()),
                }
            }
        });
    }

    fn repair(&self, id: AppId) {
        let window = self.clone();
        glib::spawn_future_local(async move {
            let parent = WindowIdentifier::from_native(&window).await;
            match AppService::portal().repair(&id, parent.as_ref()).await {
                Ok(()) => window.toast(&gettext("Launcher repaired")),
                Err(error) => window.toast(&error.to_string()),
            }
        });
    }

    pub(crate) fn refresh(&self) {
        let list = &self.imp().apps_listbox;
        list.remove_all();
        match AppService::portal().list() {
            Ok(report) => {
                for app in report.apps {
                    list.append(&AppRow::new(app));
                }
                if let Some(warning) = report.warnings.first() {
                    self.toast(&format!(
                        "{}: {}",
                        gettext("Some application data could not be loaded"),
                        warning.path.display()
                    ));
                }
            }
            Err(error) => self.toast(&error.to_string()),
        }
        if list.row_at_index(0).is_none() {
            self.imp()
                .split_view
                .set_content(Some(&HomePage::default()));
            self.imp().split_view.set_show_content(false);
        }
    }

    fn load_window_size(&self) {
        let settings = settings();
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
    }

    pub(crate) fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }
}

fn availability(available: bool) -> String {
    if available {
        gettext("Available")
    } else {
        gettext("Unavailable")
    }
}

fn parse_action_id(parameter: Option<&glib::Variant>) -> Option<AppId> {
    parameter
        .and_then(|value| value.get::<String>())
        .and_then(|value| AppId::from_str(&value).ok())
}
