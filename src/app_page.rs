// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::{model::AppConfigV1, service::AppService, util};

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
        pub config: RefCell<Option<AppConfigV1>>,
        pub pending_icon: RefCell<Option<Vec<u8>>>,
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

    impl ObjectImpl for AppPage {}
    impl WidgetImpl for AppPage {}
    impl NavigationPageImpl for AppPage {}

    #[gtk::template_callbacks]
    impl AppPage {
        #[template_callback]
        fn on_cancel_clicked(&self, _button: gtk::Button) {
            if let Some(config) = self.config.borrow().as_ref() {
                self.obj().show_config(config);
            }
            self.pending_icon.replace(None);
            self.headerbar_stack
                .set_visible_child(&self.normal_headerbar.get());
        }

        #[template_callback]
        async fn on_save_clicked(&self, _button: gtk::Button) {
            let Some(mut config) = self.config.borrow().clone() else {
                return;
            };
            config.title = self.title_entry.text().to_string();
            config.start_url = self.url_entry.text().to_string();
            config.use_theme_color = self.titlebar_color.is_active();
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
        fn update_unsaved_details_cb(&self, _widget: gtk::Widget) {
            self.obj().mark_dirty();
        }

        #[template_callback]
        fn update_unsaved_details_notify_cb(&self, _widget: gtk::Widget, _pspec: glib::ParamSpec) {
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
    pub fn new(config: AppConfigV1) -> Self {
        let page: Self = glib::Object::builder().build();
        page.show_config(&config);
        let id = config.id.clone();
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

    fn show_config(&self, config: &AppConfigV1) {
        let imp = self.imp();
        self.set_title(&config.title);
        imp.url_entry.set_text(&config.start_url);
        imp.title_entry.set_text(&config.title);
        imp.titlebar_color.set_active(config.use_theme_color);
        imp.user_agent_expander
            .set_enable_expansion(config.user_agent.is_some());
        imp.user_agent_entry
            .set_text(config.user_agent.as_deref().unwrap_or_default());
    }

    fn mark_dirty(&self) {
        self.imp()
            .headerbar_stack
            .set_visible_child(&self.imp().edit_headerbar.get());
    }

    fn notify_error(&self, message: &str) {
        let _ = self.activate_action("win.notify", Some(&message.to_variant()));
    }
}
