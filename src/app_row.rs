// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::OnceCell;

use adw::subclass::prelude::*;
use glib::clone;
use gtk::glib;

use crate::{model::AppConfigV2, service::AppService, util};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/bastle/app_row.ui")]
    pub struct AppRow {
        pub config: OnceCell<AppConfigV2>,
        #[template_child]
        pub icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub subtitle: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppRow {
        const NAME: &'static str = "BastleAppRow";
        type Type = super::AppRow;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AppRow {}
    impl WidgetImpl for AppRow {}
    impl ListBoxRowImpl for AppRow {}
}

glib::wrapper! {
    pub struct AppRow(ObjectSubclass<imp::AppRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl AppRow {
    pub fn new(config: AppConfigV2) -> Self {
        let row: Self = glib::Object::builder().build();
        row.imp().title.set_label(&config.title);
        row.imp().subtitle.set_label(&config.start_url);
        let id = config.id.clone();
        if row.imp().config.set(config).is_err() {
            return row;
        }
        glib::spawn_future_local(clone!(
            #[weak]
            row,
            async move {
                if let Ok(bytes) = AppService::portal().read_icon(&id) {
                    if let Ok(texture) = util::load_texture(bytes).await {
                        row.imp().icon.set_paintable(Some(&texture));
                    }
                }
            }
        ));
        row
    }

    pub fn config(&self) -> Option<AppConfigV2> {
        self.imp().config.get().cloned()
    }
}
