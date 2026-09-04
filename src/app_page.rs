// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::{
    compatibility::{reason_description, CompatibilityCatalogV1},
    model::{AppConfigV3, Engine},
    service::AppService,
    util,
};

fn menu_item(label: &str, action: &str, target: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), None);
    item.set_action_and_target_value(Some(action), Some(&target.to_variant()));
    item
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/bastle/app_page.ui")]
    pub struct AppPage {
        pub config: RefCell<Option<AppConfigV3>>,
        pub pending_icon: RefCell<Option<Vec<u8>>>,
        pub populating: Cell<bool>,
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
        pub titlebar_color: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub engine_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub recommendation_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub user_agent_expander: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub user_agent_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub page_menu: TemplateChild<gio::Menu>,
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
                    self.config.replace(Some(saved));
                    self.pending_icon.replace(None);
                    self.headerbar_stack
                        .set_visible_child(&self.normal_headerbar.get());
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
    pub fn new(config: AppConfigV3) -> Self {
        let page: Self = glib::Object::builder().build();
        page.imp().populating.set(true);
        page.imp()
            .engine_row
            .set_model(Some(&gtk::StringList::new(&[
                "WebKitGTK",
                "Chromium companion",
            ])));
        page.show_config(&config);
        let id = config.id.clone();
        page.imp().page_menu.append_item(&menu_item(
            &gettext("Permissions"),
            "win.permissions",
            id.as_str(),
        ));
        page.imp().page_menu.append_item(&menu_item(
            &gettext("Privacy & Power"),
            "win.privacy",
            id.as_str(),
        ));
        page.imp().page_menu.append_item(&menu_item(
            &gettext("Repair Launcher"),
            "win.repair",
            id.as_str(),
        ));
        page.imp().page_menu.append_item(&menu_item(
            &gettext("Delete Application"),
            "win.delete",
            id.as_str(),
        ));
        page.imp().config.replace(Some(config));

        glib::spawn_future_local(glib::clone!(
            #[weak]
            page,
            async move {
                if let Ok(bytes) = AppService::portal().read_icon(&id) {
                    if let Ok(texture) = util::load_texture(bytes).await {
                        page.imp().icon_image.set_paintable(Some(&texture));
                    }
                }
            }
        ));
        page
    }

    fn show_config(&self, config: &AppConfigV3) {
        let imp = self.imp();
        imp.populating.set(true);
        self.set_title(&config.title);
        imp.url_entry.set_text(&config.start_url);
        imp.title_entry.set_text(&config.title);
        imp.titlebar_color.set_active(config.use_theme_color);
        imp.engine_row.set_selected(match config.engine {
            Engine::WebKit => 0,
            Engine::Chromium => 1,
        });
        imp.user_agent_expander
            .set_enable_expansion(config.user_agent.is_some());
        imp.user_agent_entry
            .set_text(config.user_agent.as_deref().unwrap_or_default());
        self.refresh_recommendation();
        imp.populating.set(false);
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

    fn cancel_changes(&self) {
        let imp = self.imp();
        if let Some(config) = imp.config.borrow().as_ref() {
            self.show_config(config);
        }
        imp.pending_icon.replace(None);
        imp.headerbar_stack
            .set_visible_child(&imp.normal_headerbar.get());
    }

    fn mark_dirty(&self) {
        self.imp()
            .headerbar_stack
            .set_visible_child(&self.imp().edit_headerbar.get());
    }

    fn notify_error(&self, message: &str) {
        let _ = self.activate_action("win.notify", Some(&message.to_variant()));
    }

    #[cfg(feature = "ui-tests")]
    fn is_dirty(&self) -> bool {
        self.imp().headerbar_stack.visible_child().as_ref()
            == Some(self.imp().edit_headerbar.get().upcast_ref())
    }
}

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test() -> anyhow::Result<()> {
    use anyhow::ensure;

    for user_agent in [None, Some("Bastle UI smoke test".to_owned())] {
        let mut config = AppConfigV3::new("UI smoke test", "https://example.org", 0)?;
        config.user_agent = user_agent;
        let page = AppPage::new(config);
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
