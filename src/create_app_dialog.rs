// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::glib;

use crate::{
    config,
    model::{parse_web_url, AppConfigV2},
    service::AppService,
    util,
};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/io/github/cheviiot/bastle/create_app_dialog.ui")]
    #[properties(wrapper_type = super::CreateAppDialog)]
    pub struct CreateAppDialog {
        pub pending_icon: RefCell<Option<Vec<u8>>>,
        #[template_child]
        pub url_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub button: TemplateChild<gtk::Button>,
        #[template_child]
        pub button_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub button_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub button_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub icon_image: TemplateChild<gtk::Image>,
        #[template_child]
        pub title_entry: TemplateChild<adw::EntryRow>,
        #[property(get, set)]
        pub loading: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CreateAppDialog {
        const NAME: &'static str = "BastleCreateAppDialog";
        type Type = super::CreateAppDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CreateAppDialog {}
    impl WidgetImpl for CreateAppDialog {}
    impl AdwDialogImpl for CreateAppDialog {}

    #[gtk::template_callbacks]
    impl CreateAppDialog {
        #[template_callback]
        async fn on_url_apply(&self, _entry: adw::EntryRow) {
            let Ok(url) = parse_web_url(&self.url_entry.text()) else {
                self.url_entry.add_css_class("error");
                self.obj().validate_input();
                return;
            };
            self.url_entry.remove_css_class("error");
            self.url_entry.set_text(url.as_str());
            self.obj().set_loading(true);
            match util::get_website_meta(url).await {
                Ok(meta) => {
                    if let Some(title) = meta.title {
                        self.title_entry.set_text(&title);
                    }
                    if let Some(icon) = meta.icon {
                        if let Ok(texture) = util::load_texture(icon.clone()).await {
                            self.icon_image.set_paintable(Some(&texture));
                            self.pending_icon.replace(Some(icon));
                        }
                    }
                }
                Err(_) => self.obj().toast(&gettext(
                    "The website could not be reached. You can still create this application.",
                )),
            }
            self.obj().set_loading(false);
            self.obj().validate_input();
        }

        #[template_callback]
        fn validate_input_cb(&self, _widget: gtk::Widget) {
            self.obj().validate_input();
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
                }
                Err(error) => self.obj().toast(&error.to_string()),
            }
        }

        #[template_callback]
        async fn on_create_clicked(&self, _button: gtk::Button) {
            if !self.obj().validate_input() {
                return;
            }
            self.obj().set_loading(true);
            let service = AppService::portal();
            let sort_order = service
                .list()
                .map(|report| report.apps.len() as u32)
                .unwrap_or_default();
            let result = async {
                let mut app = AppConfigV2::new(
                    self.title_entry.text().as_str(),
                    self.url_entry.text().as_str(),
                    sort_order,
                )?;
                while service.contains(&app.id) {
                    app.id = crate::model::AppId::generate();
                }
                let pending_icon = self.pending_icon.borrow().clone();
                let icon = match pending_icon {
                    Some(icon) => icon,
                    None => util::default_icon().await?,
                };
                let parent = match self.obj().root().and_downcast::<gtk::Window>() {
                    Some(window) => WindowIdentifier::from_native(&window).await,
                    None => None,
                };
                service.create(app, &icon, parent.as_ref()).await
            }
            .await;
            match result {
                Ok(_) => {
                    let _ = self.obj().activate_action("win.refresh", None);
                    self.obj().close();
                }
                Err(error) => self.obj().toast(&error.to_string()),
            }
            self.obj().set_loading(false);
        }
    }
}

glib::wrapper! {
    pub struct CreateAppDialog(ObjectSubclass<imp::CreateAppDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CreateAppDialog {
    pub fn new() -> Self {
        let dialog: Self = glib::Object::builder().build();
        dialog.imp().icon_image.set_icon_name(Some(config::APP_ID));
        dialog
    }

    fn validate_input(&self) -> bool {
        let valid = parse_web_url(&self.imp().url_entry.text()).is_ok()
            && !self.imp().title_entry.text().trim().is_empty()
            && !self.loading();
        self.imp().button.set_sensitive(valid);
        valid
    }

    fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }
}

impl Default for CreateAppDialog {
    fn default() -> Self {
        Self::new()
    }
}
