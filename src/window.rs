// SPDX-License-Identifier: GPL-3.0-only

use std::{
    cell::{Cell, RefCell},
    cmp::Ordering,
    str::FromStr,
};

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::{
    addons_dialog,
    app_page::AppPage,
    app_row::AppRow,
    application::settings,
    background, backup_dialog,
    chromium::EngineAvailability,
    create_app_dialog::CreateAppDialog,
    model::{AppConfigV3, AppId},
    permissions_dialog,
    portal::{self, PortalFeature},
    privacy_dialog,
    repository::RepositoryWarning,
    service::AppService,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LibrarySort {
    #[default]
    TitleAscending,
    TitleDescending,
    Newest,
    Oldest,
}

impl LibrarySort {
    fn from_key(value: &str) -> Self {
        match value {
            "title-desc" => Self::TitleDescending,
            "newest" => Self::Newest,
            "oldest" => Self::Oldest,
            _ => Self::TitleAscending,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::TitleAscending => "title-asc",
            Self::TitleDescending => "title-desc",
            Self::Newest => "newest",
            Self::Oldest => "oldest",
        }
    }

    fn compare(self, left: &AppConfigV3, right: &AppConfigV3) -> Ordering {
        let title = || {
            left.title
                .to_lowercase()
                .cmp(&right.title.to_lowercase())
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        };
        match self {
            Self::TitleAscending => title(),
            Self::TitleDescending => title().reverse(),
            Self::Newest => right.sort_order.cmp(&left.sort_order).then_with(title),
            Self::Oldest => left.sort_order.cmp(&right.sort_order).then_with(title),
        }
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/bastle/window.ui")]
    pub struct BastleWindow {
        pub apps: RefCell<Vec<AppConfigV3>>,
        pub warnings: RefCell<Vec<RepositoryWarning>>,
        pub store: RefCell<Option<gio::ListStore>>,
        pub engine_availability: RefCell<Option<EngineAvailability>>,
        pub search_text: RefCell<String>,
        pub(super) sort_mode: Cell<LibrarySort>,
        pub syncing_search: Cell<bool>,
        #[template_child]
        pub navigation_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub apps_grid: TemplateChild<gtk::GridView>,
        #[template_child]
        pub view_stack: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub compact_search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub compact_search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub diagnostics_banner: TemplateChild<adw::Banner>,
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
            window.setup_grid();
            window.setup_search();
            window.setup_gactions();
            window.load_window_size();
            window.refresh_engine_availability();
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
        fn on_diagnostics_clicked(&self, _banner: adw::Banner) {
            self.obj().show_repository_warnings();
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
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    fn setup_grid(&self) {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::NoSelection::new(Some(store.clone()));
        self.imp().apps_grid.set_model(Some(&selection));
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, object| {
            if let Some(item) = object.downcast_ref::<gtk::ListItem>() {
                item.set_child(Some(&AppRow::new()));
            }
        });
        factory.connect_bind(|_, object| {
            let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(row) = item.child().and_downcast::<AppRow>() else {
                return;
            };
            let Some(value) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            row.set_config(value.borrow::<AppConfigV3>().clone());
        });
        self.imp().apps_grid.set_factory(Some(&factory));
        self.imp().store.replace(Some(store));
    }

    fn setup_search(&self) {
        self.imp().search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |entry| window.set_search_text(entry.text().as_str(), false)
        ));
        self.imp()
            .compact_search_entry
            .connect_search_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |entry| window.set_search_text(entry.text().as_str(), true)
            ));
    }

    fn setup_gactions(&self) {
        let saved_sort = LibrarySort::from_key(&settings().string("library-sort-mode"));
        self.imp().sort_mode.set(saved_sort);
        self.add_action_entries([
            gio::ActionEntry::builder("add")
                .activate(|window: &Self, _, _| {
                    CreateAppDialog::new(window.engine_availability()).present(Some(window));
                })
                .build(),
            gio::ActionEntry::builder("show-details")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        window.show_details(&id);
                    }
                })
                .build(),
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
            gio::ActionEntry::builder("sort")
                .parameter_type(Some(&String::static_variant_type()))
                .state(saved_sort.key().to_variant())
                .change_state(|window: &Self, action, value| {
                    let Some(value) = value.and_then(|value| value.get::<String>()) else {
                        return;
                    };
                    let mode = LibrarySort::from_key(&value);
                    action.set_state(&mode.key().to_variant());
                    window.imp().sort_mode.set(mode);
                    let _ = settings().set_string("library-sort-mode", mode.key());
                    window.rebuild_grid();
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
            gio::ActionEntry::builder("addons")
                .activate(|window: &Self, _, _| {
                    addons_dialog::present(window.upcast_ref(), &window.engine_availability());
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

    fn set_search_text(&self, value: &str, from_compact: bool) {
        if self.imp().syncing_search.replace(true) {
            return;
        }
        let value = value.to_owned();
        self.imp().search_text.replace(value.clone());
        if from_compact {
            self.imp().search_entry.set_text(&value);
        } else {
            self.imp().compact_search_entry.set_text(&value);
        }
        self.imp().syncing_search.set(false);
        self.rebuild_grid();
    }

    fn rebuild_grid(&self) {
        let Some(store) = self.imp().store.borrow().clone() else {
            return;
        };
        store.remove_all();
        let query = self.imp().search_text.borrow().trim().to_lowercase();
        let mut visible = self
            .imp()
            .apps
            .borrow()
            .iter()
            .filter(|app| matches_search(app, &query))
            .cloned()
            .collect::<Vec<_>>();
        let mode = self.imp().sort_mode.get();
        visible.sort_by(|left, right| mode.compare(left, right));
        for app in visible {
            store.append(&glib::BoxedAnyObject::new(app));
        }
        let page = if self.imp().apps.borrow().is_empty() {
            "empty"
        } else if store.n_items() == 0 {
            "no-results"
        } else {
            "grid"
        };
        self.imp().view_stack.set_visible_child_name(page);
    }

    fn refresh_engine_availability(&self) {
        self.imp()
            .engine_availability
            .replace(Some(AppService::portal().chromium_availability()));
    }

    pub(crate) fn engine_availability(&self) -> EngineAvailability {
        self.imp()
            .engine_availability
            .borrow()
            .clone()
            .unwrap_or(EngineAvailability::Missing)
    }

    fn show_details(&self, id: &AppId) {
        match AppService::portal().load(id) {
            Ok(config) => self
                .imp()
                .navigation_view
                .push(&AppPage::new(config, self.engine_availability())),
            Err(error) => self.toast(&error.to_string()),
        }
    }

    fn show_repository_warnings(&self) {
        let body = self
            .imp()
            .warnings
            .borrow()
            .iter()
            .map(|warning| format!("{}\n{}", warning.path.display(), warning.message))
            .collect::<Vec<_>>()
            .join("\n\n");
        let dialog =
            adw::AlertDialog::new(Some(&gettext("Application Data Diagnostics")), Some(&body));
        dialog.add_response("close", &gettext("Close"));
        dialog.present(Some(self));
    }

    fn show_capabilities(&self) {
        let window = self.clone();
        glib::spawn_future_local(async move {
            let background_capability = background::capability()
                .await
                .map(|version| format!("{} (v{version})", gettext("Available")))
                .unwrap_or_else(|error| format!("{} ({error})", gettext("Unavailable")));
            let capabilities = portal::probe_capabilities().await;
            let body = format!(
                "{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n\n{}\n{}",
                gettext("Desktop session"),
                capabilities.desktop,
                gettext("Dynamic Launcher"),
                portal_feature(&capabilities.dynamic_launcher.interface),
                gettext("Application launchers"),
                optional_availability(capabilities.dynamic_launcher.application_launchers),
                gettext("Web application launchers"),
                optional_availability(capabilities.dynamic_launcher.web_application_launchers),
                gettext("File Chooser"),
                portal_feature(&capabilities.file_chooser),
                gettext("Documents access"),
                portal_feature(&capabilities.documents),
                gettext("Background activity"),
                background_capability,
                gettext("Creating and repairing applications requires Application launcher support. Backup restore also requires File Chooser and Documents portal access."),
                gettext("Bastle uses portals only and never writes launchers directly to the host."),
            );
            let dialog = adw::AlertDialog::new(Some(&gettext("System Capabilities")), Some(&body));
            dialog.add_responses(&[("close", &gettext("Close")), ("retry", &gettext("Retry"))]);
            dialog.set_response_appearance("retry", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("close"));
            dialog.set_close_response("close");
            if dialog.choose_future(Some(&window)).await == "retry" {
                window.show_capabilities();
            }
        });
    }

    fn confirm_delete(&self, id: AppId) {
        let window = self.clone();
        glib::spawn_future_local(async move {
            let dialog = adw::AlertDialog::new(
                Some(&gettext("Delete this application?")),
                Some(&gettext(
                    "Its launcher, settings, and WebKit or Chromium add-on profile—including cookies and caches—will be removed.",
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
                match AppService::portal().delete(&id).await {
                    Ok(_) => {
                        if window
                            .imp()
                            .navigation_view
                            .visible_page()
                            .and_then(|page| page.tag())
                            .as_deref()
                            != Some("library")
                        {
                            window.imp().navigation_view.pop();
                        }
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
        match AppService::portal().list() {
            Ok(report) => {
                self.imp().apps.replace(report.apps);
                self.imp().warnings.replace(report.warnings);
                self.imp()
                    .diagnostics_banner
                    .set_revealed(!self.imp().warnings.borrow().is_empty());
                self.rebuild_grid();
            }
            Err(error) => self.toast(&error.to_string()),
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

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    use anyhow::ensure;

    let window = BastleWindow::new(application);
    window.set_default_size(360, 640);
    window.imp().apps.replace(Vec::new());
    window.rebuild_grid();
    ensure!(
        window.imp().view_stack.visible_child_name().as_deref() == Some("empty"),
        "empty library state was not shown"
    );

    let first = AppConfigV3::new("Alpha", "https://alpha.example", 0)?;
    let second = AppConfigV3::new("Beta", "https://beta.example", 1)?;
    window.imp().apps.replace(vec![first, second]);
    window.rebuild_grid();
    ensure!(
        window
            .imp()
            .store
            .borrow()
            .as_ref()
            .is_some_and(|store| store.n_items() == 2),
        "filled library did not populate the grid"
    );

    window.set_search_text("beta", false);
    ensure!(
        window
            .imp()
            .store
            .borrow()
            .as_ref()
            .is_some_and(|store| store.n_items() == 1),
        "search did not filter by title"
    );
    window.set_search_text("missing.example", false);
    ensure!(
        window.imp().view_stack.visible_child_name().as_deref() == Some("no-results"),
        "empty search state was not shown"
    );
    window.destroy();
    Ok(())
}

fn matches_search(app: &AppConfigV3, query: &str) -> bool {
    if query.is_empty() || app.title.to_lowercase().contains(query) {
        return true;
    }
    url::Url::parse(&app.start_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_lowercase))
        .is_some_and(|host| host.contains(query))
}

fn optional_availability(available: Option<bool>) -> String {
    match available {
        Some(true) => gettext("Available"),
        Some(false) => gettext("Unsupported"),
        None => gettext("Unknown (interface unavailable)"),
    }
}

fn portal_feature(feature: &PortalFeature) -> String {
    match feature {
        PortalFeature::Available { version } => format!("{} (v{version})", gettext("Available")),
        PortalFeature::Problem(error) => error.to_string(),
    }
}

fn parse_action_id(parameter: Option<&glib::Variant>) -> Option<AppId> {
    parameter
        .and_then(|value| value.get::<String>())
        .and_then(|value| AppId::from_str(&value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(title: &str, url: &str, order: u32) -> AppConfigV3 {
        AppConfigV3::new(title, url, order).unwrap()
    }

    #[test]
    fn library_sort_modes_are_stable() {
        let alpha = app("Alpha", "https://alpha.example", 1);
        let zulu = app("Zulu", "https://zulu.example", 2);
        assert_eq!(
            LibrarySort::TitleAscending.compare(&alpha, &zulu),
            Ordering::Less
        );
        assert_eq!(
            LibrarySort::TitleDescending.compare(&alpha, &zulu),
            Ordering::Greater
        );
        assert_eq!(
            LibrarySort::Newest.compare(&alpha, &zulu),
            Ordering::Greater
        );
        assert_eq!(LibrarySort::Oldest.compare(&alpha, &zulu), Ordering::Less);
    }

    #[test]
    fn search_matches_titles_and_domains() {
        let app = app("Example Workspace", "https://office.example.org/path", 0);
        assert!(matches_search(&app, "workspace"));
        assert!(matches_search(&app, "office.example"));
        assert!(!matches_search(&app, "missing"));
    }
}
