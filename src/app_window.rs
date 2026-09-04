// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use adw::{prelude::*, subclass::prelude::*};
use anyhow::{Context, Result};
use gettextrs::gettext;
use glib::clone;
use gtk::{gdk, gio, glib};
use webkit::{prelude::*, HardwareAccelerationPolicy, PolicyDecisionType, WebContext, WebView};

use crate::{
    model::{AppConfigV1, WindowState},
    service::AppService,
    util,
};

fn relative_luminance(color: &gdk::RGBA) -> f32 {
    0.2126 * color.red() + 0.7152 * color.green() + 0.0722 * color.blue()
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/bastle/app_window.ui")]
    pub struct AppWindow {
        #[template_child]
        pub webview_container: TemplateChild<adw::Bin>,
        #[template_child]
        pub progress_bar: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub back_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub forward_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        pub config: RefCell<Option<AppConfigV1>>,
        pub webview: RefCell<Option<WebView>>,
        pub provider: RefCell<Option<gtk::CssProvider>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppWindow {
        const NAME: &'static str = "BastleAppWindow";
        type Type = super::AppWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AppWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_gestures();
            self.obj().setup_gactions();
        }
    }
    impl WidgetImpl for AppWindow {}
    impl WindowImpl for AppWindow {
        fn close_request(&self) -> glib::Propagation {
            if let Some(config) = self.config.borrow().as_ref() {
                let (width, height) = self.obj().default_size();
                let window = WindowState {
                    width,
                    height,
                    maximized: self.obj().is_maximized(),
                };
                if let Err(error) = AppService::portal().save_runtime_state(&config.id, window) {
                    eprintln!("Failed to save Bastle window state: {error:#}");
                }
            }
            glib::Propagation::Proceed
        }
    }
    impl ApplicationWindowImpl for AppWindow {}
    impl AdwApplicationWindowImpl for AppWindow {}

    #[gtk::template_callbacks]
    impl AppWindow {
        pub(super) fn create_webview(&self, config: &AppConfigV1) -> Result<WebView> {
            let service = AppService::portal();
            let profile = service.profile_dir(&config.id);
            let cache = service.cache_dir(&config.id);
            std::fs::create_dir_all(&profile)
                .with_context(|| format!("failed to create {}", profile.display()))?;
            std::fs::create_dir_all(&cache)
                .with_context(|| format!("failed to create {}", cache.display()))?;

            let mut settings = webkit::Settings::builder()
                .enable_webgl(true)
                .enable_webrtc(true)
                .enable_webaudio(true)
                .enable_media(true)
                .enable_mediasource(true)
                .enable_encrypted_media(true)
                .enable_media_capabilities(true)
                .hardware_acceleration_policy(HardwareAccelerationPolicy::Always)
                .enable_2d_canvas_acceleration(true)
                .enable_html5_local_storage(true)
                .enable_html5_database(true)
                .enable_site_specific_quirks(true);
            if let Some(user_agent) = config.user_agent.as_deref() {
                settings = settings.user_agent(user_agent);
            }
            let settings = settings.build();
            let network_session = webkit::NetworkSession::builder()
                .cache_directory(cache.to_string_lossy().as_ref())
                .data_directory(profile.to_string_lossy().as_ref())
                .build();
            if let Some(cookie_manager) = network_session.cookie_manager() {
                cookie_manager.set_persistent_storage(
                    profile.join("cookies.sqlite").to_string_lossy().as_ref(),
                    webkit::CookiePersistentStorage::Sqlite,
                );
            }

            network_session.connect_download_started(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                move |_, download| {
                    download.connect_failed(clone!(
                        #[weak]
                        window,
                        move |_, error| window.toast(&error.to_string())
                    ));
                    download.connect_decide_destination(clone!(
                        #[weak]
                        window,
                        #[upgrade_or]
                        true,
                        move |download, suggested_name| {
                            let suggested_name = suggested_name.to_owned();
                            glib::spawn_future_local(clone!(
                                #[weak]
                                download,
                                #[weak]
                                window,
                                async move {
                                    let dialog = gtk::FileDialog::builder()
                                        .accept_label(gettext("Save"))
                                        .title(gettext("Download File"))
                                        .modal(true)
                                        .initial_name(&suggested_name)
                                        .build();
                                    match dialog.save_future(Some(&window)).await {
                                        Ok(file) => download.set_destination(file.uri().as_str()),
                                        Err(_) => download.cancel(),
                                    }
                                }
                            ));
                            true
                        }
                    ));
                }
            ));

            let content_manager = webkit::UserContentManager::new();
            if config.use_theme_color {
                let script = webkit::UserScript::new(
                    include_str!("inject.js"),
                    webkit::UserContentInjectedFrames::TopFrame,
                    webkit::UserScriptInjectionTime::End,
                    &[],
                    &[],
                );
                if content_manager.register_script_message_handler("themeColor", None) {
                    content_manager.connect_script_message_received(
                        Some("themeColor"),
                        clone!(
                            #[weak(rename_to = window)]
                            self.obj(),
                            move |_, value| {
                                let parsed = util::valid_theme_color(value.to_str().as_str());
                                window.load_colors(parsed.as_deref());
                            }
                        ),
                    );
                    content_manager.add_script(&script);
                }
            }

            let context = WebContext::new();
            context.set_spell_checking_enabled(true);
            let view = WebView::builder()
                .network_session(&network_session)
                .settings(&settings)
                .user_content_manager(&content_manager)
                .web_context(&context)
                .build();
            self.connect_webview(&view);
            Ok(view)
        }

        fn connect_webview(&self, view: &WebView) {
            view.connect_decide_policy(|view, decision, kind| {
                if kind == PolicyDecisionType::NewWindowAction {
                    let uri = decision
                        .clone()
                        .downcast::<webkit::NavigationPolicyDecision>()
                        .ok()
                        .and_then(|policy| policy.navigation_action())
                        .and_then(|action| action.request())
                        .and_then(|request| request.uri());
                    if let Some(uri) = uri.filter(|uri| {
                        url::Url::parse(uri)
                            .ok()
                            .is_some_and(|url| matches!(url.scheme(), "http" | "https"))
                    }) {
                        let _ = gio::AppInfo::launch_default_for_uri(
                            &uri,
                            None::<&gio::AppLaunchContext>,
                        );
                    }
                    decision.ignore();
                    return true;
                }
                if kind == PolicyDecisionType::Response {
                    let should_download = decision
                        .clone()
                        .downcast::<webkit::ResponsePolicyDecision>()
                        .ok()
                        .and_then(|policy| policy.response())
                        .and_then(|response| response.http_headers())
                        .and_then(|headers| headers.one("Content-Type"))
                        .is_some_and(|mime| !view.can_show_mime_type(mime.as_str()));
                    if should_download {
                        decision.download();
                        return true;
                    }
                }
                false
            });
            view.connect_estimated_load_progress_notify(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                move |view| {
                    let progress = view.estimated_load_progress();
                    window.imp().progress_bar.set_fraction(progress);
                    window.imp().progress_bar.set_visible(progress < 1.0);
                }
            ));
            view.connect_notify_local(
                Some("can-go-back"),
                clone!(
                    #[weak(rename_to = window)]
                    self.obj(),
                    move |view, _| window.imp().back_button.set_sensitive(view.can_go_back())
                ),
            );
            view.connect_notify_local(
                Some("can-go-forward"),
                clone!(
                    #[weak(rename_to = window)]
                    self.obj(),
                    move |view, _| window
                        .imp()
                        .forward_button
                        .set_sensitive(view.can_go_forward())
                ),
            );
        }

        pub(super) fn go_back(&self) {
            if let Some(view) = self.webview.borrow().as_ref() {
                view.go_back();
            }
        }

        pub(super) fn go_forward(&self) {
            if let Some(view) = self.webview.borrow().as_ref() {
                view.go_forward();
            }
        }

        #[template_callback]
        fn on_back_clicked(&self, _button: gtk::Button) {
            self.go_back();
        }

        #[template_callback]
        fn on_forward_clicked(&self, _button: gtk::Button) {
            self.go_forward();
        }
    }
}

glib::wrapper! {
    pub struct AppWindow(ObjectSubclass<imp::AppWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl AppWindow {
    pub fn new<P: IsA<gtk::Application>>(application: &P, config: &AppConfigV1) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", application)
            .build();
        window.set_config(config);
        window
    }

    fn set_config(&self, config: &AppConfigV1) {
        self.set_widget_name(&format!("b{}", config.id));
        self.set_title(Some(&config.title));
        self.set_default_size(config.window.width, config.window.height);
        if config.window.maximized {
            self.maximize();
        }
        match self.imp().create_webview(config) {
            Ok(view) => {
                view.load_uri(&config.start_url);
                self.imp().webview_container.set_child(Some(&view));
                self.imp().webview.replace(Some(view));
            }
            Err(error) => self.toast(&error.to_string()),
        }
        self.imp().config.replace(Some(config.clone()));
        self.load_colors(None);
    }

    fn load_colors(&self, background: Option<&str>) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let provider = self
            .imp()
            .provider
            .borrow_mut()
            .get_or_insert_with(|| {
                let provider = gtk::CssProvider::new();
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
                provider
            })
            .clone();
        let parsed = background.and_then(|value| gdk::RGBA::parse(value).ok());
        let foreground = parsed.as_ref().map_or("@window_fg_color", |color| {
            if relative_luminance(color) > 0.5 {
                "black"
            } else {
                "white"
            }
        });
        let background = parsed
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "@window_bg_color".to_owned());
        provider.load_from_string(&format!(
            "window#{} {{ background: {}; color: {}; }}",
            self.widget_name(),
            background,
            foreground
        ));
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("forward")
                .activate(|window: &Self, _, _| window.imp().go_forward())
                .build(),
            gio::ActionEntry::builder("back")
                .activate(|window: &Self, _, _| window.imp().go_back())
                .build(),
        ]);
    }

    fn setup_gestures(&self) {
        let gesture = gtk::GestureClick::new();
        gesture.set_button(0);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        gesture.connect_pressed(clone!(
            #[weak(rename_to = window)]
            self,
            move |gesture, _, _, _| match gesture.current_button() {
                8 => {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    window.imp().go_back();
                }
                9 => {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    window.imp().go_forward();
                }
                _ => {}
            }
        ));
        self.add_controller(gesture);
    }

    fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }
}
