// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, collections::HashMap, process::Command, str::FromStr};

use adw::prelude::*;
use adw::subclass::prelude::*;
use anyhow::{anyhow, Context, Result};
use gettextrs::gettext;
use glib::{OptionArg, OptionFlags};
use gtk::{gio, glib};

use crate::{app_window::AppWindow, config, model::AppId, service::AppService, BastleWindow};

pub fn settings() -> gio::Settings {
    gio::Settings::new(config::APP_ID)
}

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct BastleApplication {
        pub web_notifications: RefCell<HashMap<String, (String, webkit::Notification)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BastleApplication {
        const NAME: &'static str = "BastleApplication";
        type Type = super::BastleApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for BastleApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let app = self.obj();
            app.setup_gactions();
            app.add_main_option(
                "list-applications",
                glib::Char::from(b'l'),
                OptionFlags::NONE,
                OptionArg::None,
                "List known applications",
                None,
            );
            app.add_main_option(
                "background",
                glib::Char::from(0),
                OptionFlags::NONE,
                OptionArg::None,
                "Start opted-in web applications in the background",
                None,
            );
            #[cfg(feature = "ui-tests")]
            app.add_main_option(
                "ui-test-app-page",
                glib::Char::from(0),
                OptionFlags::HIDDEN,
                OptionArg::None,
                "Run the app-page UI regression test",
                None,
            );
            app.set_accels_for_action("app.quit", &["<primary>q"]);
            app.set_accels_for_action("win.back", &["<alt>Left", "Back"]);
            app.set_accels_for_action("win.forward", &["<alt>Right", "Forward"]);
            app.set_accels_for_action("win.reload", &["<primary>r", "F5"]);
            app.set_accels_for_action("win.reload-bypass-cache", &["<primary><shift>r"]);
            app.set_accels_for_action("win.home", &["<alt>Home"]);
            app.set_accels_for_action("win.zoom-in", &["<primary>plus", "<primary>equal"]);
            app.set_accels_for_action("win.zoom-out", &["<primary>minus"]);
            app.set_accels_for_action("win.zoom-reset", &["<primary>0"]);
            app.set_accels_for_action("win.toggle-fullscreen", &["F11"]);
        }
    }

    impl ApplicationImpl for BastleApplication {
        fn command_line(&self, command_line: &gio::ApplicationCommandLine) -> glib::ExitCode {
            #[cfg(feature = "ui-tests")]
            if command_line
                .options_dict()
                .lookup::<bool>("ui-test-app-page")
                .ok()
                .flatten()
                .unwrap_or(false)
            {
                let result = crate::app_page::run_ui_smoke_test()
                    .and_then(|()| crate::download_manager::run_ui_smoke_test(&*self.obj()))
                    .and_then(|()| crate::backup_dialog::run_ui_smoke_test(&*self.obj()))
                    .and_then(|()| crate::privacy_dialog::run_ui_smoke_test(&*self.obj()))
                    .and_then(|()| crate::app_window::run_background_ui_smoke_test(&*self.obj()));
                return match result {
                    Ok(()) => glib::ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("UI smoke test failed: {error:#}");
                        glib::ExitCode::FAILURE
                    }
                };
            }

            let service = AppService::portal();
            if command_line
                .options_dict()
                .lookup::<bool>("background")
                .ok()
                .flatten()
                .unwrap_or(false)
            {
                return match service.list() {
                    Ok(report) => {
                        for config in report.apps {
                            match service.load_policy(&config.id) {
                                Ok(policy)
                                    if policy.background.enabled && policy.background.autostart =>
                                {
                                    AppWindow::new(&*self.obj(), &config).start_in_background();
                                }
                                Ok(_) => {}
                                Err(error) => eprintln!(
                                    "Failed to load background policy for {}: {error:#}",
                                    config.id
                                ),
                            }
                        }
                        glib::ExitCode::SUCCESS
                    }
                    Err(error) => {
                        eprintln!("Error: {error:#}");
                        glib::ExitCode::FAILURE
                    }
                };
            }
            if command_line
                .options_dict()
                .lookup::<bool>("list-applications")
                .ok()
                .flatten()
                .unwrap_or(false)
            {
                match service.list() {
                    Ok(report) => {
                        for app in report.apps {
                            println!("{}\t{}", app.id, app.title);
                        }
                        return glib::ExitCode::SUCCESS;
                    }
                    Err(error) => {
                        eprintln!("Error: {error:#}");
                        return glib::ExitCode::FAILURE;
                    }
                }
            }

            let window: Result<gtk::Window> = match command_line.arguments().get(1) {
                Some(value) => AppId::from_str(&value.to_string_lossy())
                    .and_then(|id| service.load(&id))
                    .map(|app| AppWindow::new(&self.obj().clone(), &app).upcast()),
                None => Ok(BastleWindow::new(&*self.obj()).upcast()),
            };
            match window {
                Ok(window) => {
                    window.present();
                    glib::ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Error: {error:#}");
                    glib::ExitCode::FAILURE
                }
            }
        }
    }

    impl GtkApplicationImpl for BastleApplication {}
    impl AdwApplicationImpl for BastleApplication {}
}

glib::wrapper! {
    pub struct BastleApplication(ObjectSubclass<imp::BastleApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl BastleApplication {
    pub fn new(flags: gio::ApplicationFlags) -> Self {
        glib::Object::builder()
            .property("flags", flags)
            .property("application-id", Self::instance_app_id())
            .build()
    }

    fn instance_app_id() -> String {
        std::env::args()
            .nth(1)
            .and_then(|value| AppId::from_str(&value).ok())
            .filter(|id| AppService::portal().contains(id))
            .map(|id| format!("{}.{}", config::APP_ID, id))
            .unwrap_or_else(|| config::APP_ID.to_owned())
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("quit")
                .activate(|app: &Self, _, _| app.quit())
                .build(),
            gio::ActionEntry::builder("about")
                .activate(|app: &Self, _, _| app.show_about())
                .build(),
            gio::ActionEntry::builder("open-app")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|app: &Self, _, parameter| {
                    let result = parameter
                        .and_then(|value| value.get::<String>())
                        .ok_or_else(|| anyhow!("missing app id"))
                        .and_then(|id| app.open_app(&id));
                    if let Err(error) = result {
                        eprintln!("Failed to open app: {error:#}");
                    }
                })
                .build(),
            gio::ActionEntry::builder("notification-activated")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|app: &Self, _, parameter| app.activate_web_notification(parameter))
                .build(),
            gio::ActionEntry::builder("show-background")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|app: &Self, _, parameter| {
                    if let Some(window) = app.background_window(parameter) {
                        window.show_from_background();
                    }
                })
                .build(),
            gio::ActionEntry::builder("stop-background")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|app: &Self, _, parameter| {
                    if let Some(window) = app.background_window(parameter) {
                        window.stop_background();
                    }
                })
                .build(),
        ]);
    }

    fn background_window(&self, parameter: Option<&glib::Variant>) -> Option<AppWindow> {
        let id = parameter
            .and_then(|value| value.get::<String>())
            .and_then(|value| AppId::from_str(&value).ok())?;
        self.windows().into_iter().find_map(|window| {
            window
                .downcast::<AppWindow>()
                .ok()
                .filter(|window| window.app_id().as_ref() == Some(&id))
        })
    }

    pub fn send_web_notification(&self, id: &AppId, web_notification: &webkit::Notification) {
        let token = format!("{}:{}", id, web_notification.id());
        let notification_id = format!("web-{token}");
        let title = web_notification
            .title()
            .map(|title| title.to_string())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| gettext("Website Notification"));
        let notification = gio::Notification::new(&title);
        if let Some(body) = web_notification.body().filter(|body| !body.is_empty()) {
            notification.set_body(Some(body.as_str()));
        }
        notification.set_icon(&gio::ThemedIcon::new(config::APP_ID));
        notification.set_default_action_and_target_value(
            "app.notification-activated",
            Some(&token.to_variant()),
        );

        self.imp().web_notifications.borrow_mut().insert(
            token.clone(),
            (notification_id.clone(), web_notification.clone()),
        );
        web_notification.connect_closed(glib::clone!(
            #[weak(rename_to = app)]
            self,
            #[strong]
            token,
            move |_| {
                if let Some((notification_id, _)) =
                    app.imp().web_notifications.borrow_mut().remove(&token)
                {
                    app.withdraw_notification(&notification_id);
                }
            }
        ));
        self.send_notification(Some(&notification_id), &notification);
    }

    fn activate_web_notification(&self, parameter: Option<&glib::Variant>) {
        let Some(token) = parameter.and_then(|value| value.get::<String>()) else {
            return;
        };
        if let Some((notification_id, notification)) =
            self.imp().web_notifications.borrow_mut().remove(&token)
        {
            self.withdraw_notification(&notification_id);
            notification.clicked();
        }
        if let Some(window) = self.active_window() {
            window.present();
            return;
        }
        if let Some((id, _)) = token.split_once(':') {
            if let Err(error) = self.open_app(id) {
                eprintln!("Failed to open app from notification: {error:#}");
            }
        }
    }

    fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name("Bastle")
            .application_icon(config::APP_ID)
            .developer_name("Cheviiot")
            .version(config::VERSION)
            .developers(vec!["Cheviiot", "Zaedus (original Spider author)"])
            .copyright("© 2024–2026 Zaedus and Bastle contributors")
            .license_type(gtk::License::Gpl30)
            .website("https://github.com/Cheviiot/bastle")
            .issue_url("https://github.com/Cheviiot/bastle/issues")
            .build();
        dialog.present(self.active_window().as_ref());
    }

    fn open_app(&self, id: &str) -> Result<()> {
        let id = AppId::from_str(id)?;
        if !AppService::portal().contains(&id) {
            return Err(anyhow!("unknown app id {id}"));
        }
        let executable = std::env::current_exe().unwrap_or_else(|_| "bastle".into());
        Command::new(executable)
            .arg(id.as_str())
            .spawn()
            .context("failed to start the isolated app process")?;
        Ok(())
    }
}
