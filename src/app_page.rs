// SPDX-License-Identifier: GPL-3.0-only

use std::cell::{Cell, RefCell};

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::glib;

use crate::{
    chromium::EngineAvailability,
    compatibility::{reason_description, CompatibilityCatalogV1},
    model::{AppConfigV3, Engine},
    service::AppService,
    util,
};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/bastle/app_page.ui")]
    pub struct AppPage {
        pub config: RefCell<Option<AppConfigV3>>,
        pub availability: RefCell<Option<EngineAvailability>>,
        pub pending_icon: RefCell<Option<Vec<u8>>>,
        pub populating: Cell<bool>,
        pub dirty: Cell<bool>,
        #[template_child]
        pub details_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub details_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub details_domain: TemplateChild<gtk::Label>,
        #[template_child]
        pub engine_status: TemplateChild<gtk::Label>,
        #[template_child]
        pub engine_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub launch_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub permissions_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub privacy_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub repair_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub delete_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub content_stack: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub icon_image: TemplateChild<gtk::Image>,
        #[template_child]
        pub url_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub title_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub headerbar_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub normal_headerbar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        pub edit_headerbar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        pub save_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub titlebar_color: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub engine_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub engine_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub recommendation_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub user_agent_expander: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub user_agent_entry: TemplateChild<adw::EntryRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppPage {
        const NAME: &'static str = "BastleAppPage";
        type Type = super::AppPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AppPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.user_agent_expander
                .connect_enable_expansion_notify(glib::clone!(
                    #[weak(rename_to = page)]
                    self.obj(),
                    move |_| {
                        if !page.imp().populating.get() {
                            page.mark_dirty();
                        }
                    }
                ));
            self.engine_row.connect_selected_notify(glib::clone!(
                #[weak(rename_to = page)]
                self.obj(),
                move |_| {
                    if !page.imp().populating.get() {
                        page.mark_dirty();
                    }
                }
            ));
        }
    }
    impl WidgetImpl for AppPage {}
    impl NavigationPageImpl for AppPage {}

    #[gtk::template_callbacks]
    impl AppPage {
        #[template_callback]
        fn on_edit_clicked(&self, _button: gtk::Button) {
            self.obj().begin_edit();
        }

        #[template_callback]
        fn on_addons_clicked(&self, _banner: adw::Banner) {
            let _ = self.obj().activate_action("win.addons", None);
        }

        #[template_callback]
        fn on_cancel_clicked(&self, _button: gtk::Button) {
            self.obj().cancel_changes();
        }

        #[template_callback]
        async fn on_save_clicked(&self, _button: gtk::Button) {
            let Some(mut config) = self.config.borrow().clone() else {
                return;
            };
            config.title = self.title_entry.text().to_string();
            config.start_url = self.url_entry.text().to_string();
            config.use_theme_color = self.titlebar_color.is_active();
            config.engine = if self.engine_row.selected() == 1 {
                Engine::Chromium
            } else {
                Engine::WebKit
            };
            config.user_agent = self
                .user_agent_expander
                .enables_expansion()
                .then(|| self.user_agent_entry.text().to_string())
                .filter(|value| !value.trim().is_empty());
            if let Err(error) = config.normalize_and_validate() {
                self.obj().notify_error(&error.to_string());
                return;
            }

            let parent = match self.obj().root().and_downcast::<gtk::Window>() {
                Some(window) => WindowIdentifier::from_native(&window).await,
                None => None,
            };
            let service = AppService::portal();
            let pending_icon = self.pending_icon.borrow().clone();
            match service
                .update(config.clone(), pending_icon.as_deref(), parent.as_ref())
                .await
            {
                Ok(saved) => {
                    self.config.replace(Some(saved.clone()));
                    self.pending_icon.replace(None);
                    self.obj().show_config(&saved);
                    self.obj().finish_edit();
                    let _ = self.obj().activate_action("win.refresh", None);
                }
                Err(error) => self.obj().notify_error(&error.to_string()),
            }
        }

        #[template_callback]
        async fn on_icon_clicked(&self, _button: gtk::Button) {
            let window = self.obj().root().and_downcast::<gtk::Window>();
            let result = async {
                let file = util::icon_from_dialog(window.as_ref()).await?;
                let (bytes, _) = file.load_contents_future().await?;
                let icon = util::normalize_icon(bytes.to_vec()).await?;
                let texture = util::load_texture(icon.clone()).await?;
                anyhow::Ok((icon, texture))
            }
            .await;
            match result {
                Ok((icon, texture)) => {
                    self.pending_icon.replace(Some(icon));
                    self.icon_image.set_paintable(Some(&texture));
                    self.details_icon.set_paintable(Some(&texture));
                    self.obj().mark_dirty();
                }
                Err(error) => self.obj().notify_error(&error.to_string()),
            }
        }

        #[template_callback]
        fn on_url_apply(&self, _entry: adw::EntryRow) {
            self.obj().refresh_recommendation();
            self.obj().mark_dirty();
        }

        #[template_callback]
        fn update_unsaved_details_cb(&self, _widget: gtk::Widget) {
            self.obj().mark_dirty();
        }
    }
}

glib::wrapper! {
    pub struct AppPage(ObjectSubclass<imp::AppPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AppPage {
    pub fn new(config: AppConfigV3, availability: EngineAvailability) -> Self {
        let page: Self = glib::Object::builder().build();
        page.imp().populating.set(true);
        page.imp().availability.replace(Some(availability));
        let chromium_label = gettext("Chromium (add-on)");
        page.imp()
            .engine_row
            .set_model(Some(&gtk::StringList::new(&[
                "WebKitGTK",
                chromium_label.as_str(),
            ])));
        page.show_config(&config);
        page.set_action_targets(&config);
        page.imp().config.replace(Some(config.clone()));

        let id = config.id;
        glib::spawn_future_local(glib::clone!(
            #[weak]
            page,
            async move {
                if let Ok(bytes) = AppService::portal().read_icon(&id) {
                    if let Ok(texture) = util::load_texture(bytes).await {
                        page.imp().icon_image.set_paintable(Some(&texture));
                        page.imp().details_icon.set_paintable(Some(&texture));
                    }
                }
            }
        ));
        page
    }

    fn set_action_targets(&self, config: &AppConfigV3) {
        let target = config.id.as_str().to_variant();
        for (button, action) in [
            (
                self.imp().launch_button.get().upcast::<gtk::Actionable>(),
                "app.open-app",
            ),
            (
                self.imp()
                    .permissions_button
                    .get()
                    .upcast::<gtk::Actionable>(),
                "win.permissions",
            ),
            (
                self.imp().privacy_button.get().upcast::<gtk::Actionable>(),
                "win.privacy",
            ),
            (
                self.imp().repair_button.get().upcast::<gtk::Actionable>(),
                "win.repair",
            ),
            (
                self.imp().delete_button.get().upcast::<gtk::Actionable>(),
                "win.delete",
            ),
        ] {
            button.set_action_name(Some(action));
            button.set_action_target_value(Some(&target));
        }
    }

    fn show_config(&self, config: &AppConfigV3) {
        let imp = self.imp();
        imp.populating.set(true);
        self.set_title(&config.title);
        imp.details_title.set_label(&config.title);
        let domain = url::Url::parse(&config.start_url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| config.start_url.clone());
        imp.details_domain.set_label(&domain);
        imp.url_entry.set_text(&config.start_url);
        imp.title_entry.set_text(&config.title);
        imp.titlebar_color.set_active(config.use_theme_color);
        imp.engine_row.set_selected(match config.engine {
            Engine::WebKit => 0,
            Engine::Chromium => 1,
        });
        let availability = imp
            .availability
            .borrow()
            .clone()
            .unwrap_or(EngineAvailability::Missing);
        let show_engine = availability.is_available() || config.engine == Engine::Chromium;
        imp.engine_group.set_visible(show_engine);
        imp.engine_status.set_visible(show_engine);
        let engine_available = config.engine == Engine::WebKit || availability.is_available();
        let engine_status = match config.engine {
            Engine::WebKit => "WebKitGTK".to_owned(),
            Engine::Chromium if engine_available => gettext("Chromium · Ready"),
            Engine::Chromium => gettext("Chromium · Add-on Required"),
        };
        imp.engine_status.set_label(&engine_status);
        imp.engine_banner
            .set_revealed(config.engine == Engine::Chromium && !engine_available);
        imp.user_agent_expander
            .set_enable_expansion(config.user_agent.is_some());
        imp.user_agent_entry
            .set_text(config.user_agent.as_deref().unwrap_or_default());
        self.refresh_recommendation();
        imp.populating.set(false);
        self.set_dirty(false);
    }

    fn refresh_recommendation(&self) {
        let recommendation = CompatibilityCatalogV1::bundled()
            .and_then(|catalog| {
                catalog
                    .recommendation(&self.imp().url_entry.text())
                    .map(|entry| {
                        entry
                            .filter(|entry| entry.recommended_engine() == Engine::Chromium)
                            .map(|entry| entry.reason_code().to_owned())
                    })
            })
            .ok()
            .flatten();
        self.imp()
            .recommendation_row
            .set_visible(recommendation.is_some());
        if let Some(reason_code) = recommendation {
            self.imp()
                .recommendation_row
                .set_subtitle(&reason_description(&reason_code));
        }
    }

    fn begin_edit(&self) {
        self.imp().headerbar_stack.set_visible_child_name("edit");
        self.imp().content_stack.set_visible_child_name("edit");
        self.set_dirty(false);
        self.imp().title_entry.grab_focus();
    }

    fn finish_edit(&self) {
        self.imp().headerbar_stack.set_visible_child_name("details");
        self.imp().content_stack.set_visible_child_name("details");
        self.set_dirty(false);
    }

    fn cancel_changes(&self) {
        let config = self.imp().config.borrow().clone();
        if let Some(config) = config.as_ref() {
            self.show_config(config);
        }
        self.imp().pending_icon.replace(None);
        self.finish_edit();
    }

    fn mark_dirty(&self) {
        if !self.imp().populating.get() {
            self.set_dirty(true);
        }
    }

    fn set_dirty(&self, dirty: bool) {
        self.imp().dirty.set(dirty);
        self.imp().save_button.set_sensitive(dirty);
    }

    fn notify_error(&self, message: &str) {
        let _ = self.activate_action("win.notify", Some(&message.to_variant()));
    }

    #[cfg(feature = "ui-tests")]
    fn is_dirty(&self) -> bool {
        self.imp().dirty.get()
    }
}

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test() -> anyhow::Result<()> {
    use std::collections::BTreeSet;

    use anyhow::ensure;

    let availability = EngineAvailability::Available(crate::chromium::ChromiumCapabilities {
        protocol_version: crate::chromium::PROTOCOL_VERSION,
        features: BTreeSet::from([crate::chromium::RUNTIME_SHELL_FEATURE.to_owned()]),
    });
    for user_agent in [None, Some("Bastle UI smoke test".to_owned())] {
        let mut config = AppConfigV3::new("UI smoke test", "https://example.org", 0)?;
        config.user_agent = user_agent;
        let page = AppPage::new(config, availability.clone());
        ensure!(
            !page.is_dirty(),
            "opening the details page marked it as dirty"
        );

        page.begin_edit();
        ensure!(!page.is_dirty(), "opening the editor marked it as dirty");
        let enabled = page.imp().user_agent_expander.enables_expansion();
        page.imp()
            .user_agent_expander
            .set_enable_expansion(!enabled);
        ensure!(
            page.is_dirty(),
            "changing the user-agent toggle stayed clean"
        );

        page.cancel_changes();
        ensure!(!page.is_dirty(), "cancelling the edit stayed dirty");
        page.begin_edit();
        page.imp().engine_row.set_selected(1);
        ensure!(page.is_dirty(), "changing the browser engine stayed clean");
        page.cancel_changes();
        ensure!(
            page.imp().engine_row.selected() == 0,
            "cancelling did not restore the browser engine"
        );
        ensure!(!page.is_dirty(), "cancelling the engine edit stayed dirty");
    }
    Ok(())
}
