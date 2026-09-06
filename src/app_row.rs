// SPDX-License-Identifier: GPL-3.0-only

use std::cell::{Cell, RefCell};

use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib};

use crate::{model::AppConfigV3, service::AppService, util};

fn menu_item(label: &str, action: &str, target: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), None);
    item.set_action_and_target_value(Some(action), Some(&target.to_variant()));
    item
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/bastle/app_row.ui")]
    pub struct AppRow {
        pub config: RefCell<Option<AppConfigV3>>,
        pub generation: Cell<u64>,
        #[template_child]
        pub icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub details_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub launch_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub actions_revealer: TemplateChild<gtk::Revealer>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppRow {
        const NAME: &'static str = "BastleAppRow";
        type Type = super::AppRow;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AppRow {
        fn constructed(&self) {
            self.parent_constructed();
            let motion = gtk::EventControllerMotion::new();
            motion.connect_enter(glib::clone!(
                #[weak(rename_to = row)]
                self.obj(),
                move |_, _, _| row.set_actions_visible(true)
            ));
            motion.connect_leave(glib::clone!(
                #[weak(rename_to = row)]
                self.obj(),
                move |_| {
                    if !row.imp().menu_button.is_active() {
                        row.set_actions_visible(false);
                    }
                }
            ));
            self.obj().add_controller(motion);
            self.menu_button.connect_active_notify(glib::clone!(
                #[weak(rename_to = row)]
                self.obj(),
                move |button| row.set_actions_visible(button.is_active())
            ));
        }
    }
    impl WidgetImpl for AppRow {}
    impl BoxImpl for AppRow {}
}

glib::wrapper! {
    pub struct AppRow(ObjectSubclass<imp::AppRow>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl AppRow {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_config(&self, config: AppConfigV3) {
        let imp = self.imp();
        let generation = imp.generation.get().wrapping_add(1);
        imp.generation.set(generation);
        imp.title.set_label(&config.title);
        let host = url::Url::parse(&config.start_url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| config.start_url.clone());
        imp.subtitle.set_label(&host);
        imp.icon.set_icon_name(Some("io.github.cheviiot.bastle"));

        let target = config.id.as_str().to_variant();
        imp.details_button.set_action_name(Some("win.show-details"));
        imp.details_button.set_action_target_value(Some(&target));
        imp.launch_button.set_action_name(Some("app.open-app"));
        imp.launch_button.set_action_target_value(Some(&target));

        let menu = gio::Menu::new();
        menu.append_item(&menu_item(
            &gettextrs::gettext("Launch"),
            "app.open-app",
            config.id.as_str(),
        ));
        menu.append_item(&menu_item(
            &gettextrs::gettext("View Details"),
            "win.show-details",
            config.id.as_str(),
        ));
        let settings = gio::Menu::new();
        settings.append_item(&menu_item(
            &gettextrs::gettext("Permissions"),
            "win.permissions",
            config.id.as_str(),
        ));
        settings.append_item(&menu_item(
            &gettextrs::gettext("Privacy & Power"),
            "win.privacy",
            config.id.as_str(),
        ));
        menu.append_section(None, &settings);
        let maintenance = gio::Menu::new();
        maintenance.append_item(&menu_item(
            &gettextrs::gettext("Repair Launcher"),
            "win.repair",
            config.id.as_str(),
        ));
        maintenance.append_item(&menu_item(
            &gettextrs::gettext("Delete Application"),
            "win.delete",
            config.id.as_str(),
        ));
        menu.append_section(None, &maintenance);
        imp.menu_button.set_menu_model(Some(&menu));

        let id = config.id.clone();
        imp.config.replace(Some(config));
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = row)]
            self,
            async move {
                let Ok(bytes) = AppService::portal().read_icon(&id) else {
                    return;
                };
                let Ok(texture) = util::load_texture(bytes).await else {
                    return;
                };
                if row.imp().generation.get() == generation {
                    row.imp().icon.set_paintable(Some(&texture));
                }
            }
        ));
    }

    pub fn config(&self) -> Option<AppConfigV3> {
        self.imp().config.borrow().clone()
    }

    fn set_actions_visible(&self, visible: bool) {
        self.imp().actions_revealer.set_reveal_child(visible);
    }
}

impl Default for AppRow {
    fn default() -> Self {
        Self::new()
    }
}
