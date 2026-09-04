// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::RefCell, collections::BTreeSet, fs::File, io::Read, path::Path, rc::Rc, str::FromStr,
};

use adw::prelude::*;
use anyhow::{anyhow, ensure, Context, Result};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::{
    background, content_filters,
    model::AppId,
    policy::{
        normalize_proxy_uri, AppPolicyV2, ContentFilterRuleSet, Origin, ProxyMode,
        MAX_CONTENT_FILTER_SOURCE_SIZE,
    },
    service::AppService,
    BastleWindow,
};

pub fn start(parent: &BastleWindow, id: AppId) {
    let service = AppService::portal();
    let original = match service.load_policy(&id) {
        Ok(policy) => policy,
        Err(error) => {
            parent.toast(&error.to_string());
            return;
        }
    };
    let config = match service.load(&id) {
        Ok(config) => config,
        Err(error) => {
            parent.toast(&error.to_string());
            return;
        }
    };
    present_editor(parent, id, original, config);
}

fn present_editor(
    parent: &BastleWindow,
    id: AppId,
    original: AppPolicyV2,
    config: crate::model::AppConfigV2,
) -> adw::Dialog {
    let dialog = adw::Dialog::builder()
        .title(gettext("Privacy & Power"))
        .content_width(600)
        .content_height(720)
        .build();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let save = gtk::Button::with_label(&gettext("Save"));
    save.add_css_class("suggested-action");
    header.pack_end(&save);
    toolbar.add_top_bar(&header);

    let page = adw::PreferencesPage::new();

    let navigation_group = adw::PreferencesGroup::builder()
        .title(gettext("Top-Level Navigation"))
        .description(gettext(
            "Restrict which origins may replace the application's main page. Subresources are not blocked.",
        ))
        .build();
    let navigation_enabled = adw::SwitchRow::builder()
        .title(gettext("Restrict Navigation"))
        .subtitle(gettext("Ask before opening an origin outside this list"))
        .active(original.navigation.enabled)
        .build();
    navigation_group.add(&navigation_enabled);
    let origins = adw::EntryRow::builder()
        .title(gettext("Allowed Origins"))
        .text(
            original
                .navigation
                .allowed_origins
                .iter()
                .map(Origin::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        )
        .build();
    origins.set_sensitive(navigation_enabled.is_active());
    navigation_group.add(&origins);
    navigation_enabled.connect_active_notify(glib::clone!(
        #[weak]
        origins,
        move |row| origins.set_sensitive(row.is_active())
    ));
    page.add(&navigation_group);

    let proxy_group = adw::PreferencesGroup::builder()
        .title(gettext("Proxy"))
        .description(gettext(
            "Applies only to this application's WebKit traffic. Credentials are never stored.",
        ))
        .build();
    let proxy_labels = [
        gettext("System Settings"),
        gettext("Direct Connection"),
        gettext("Custom HTTP(S) or SOCKS"),
    ];
    let proxy_model = gtk::StringList::new(&[
        proxy_labels[0].as_str(),
        proxy_labels[1].as_str(),
        proxy_labels[2].as_str(),
    ]);
    let proxy_mode = adw::ComboRow::builder()
        .title(gettext("Proxy Mode"))
        .model(&proxy_model)
        .selected(proxy_mode_index(original.proxy.mode))
        .build();
    proxy_group.add(&proxy_mode);
    let proxy_uri = adw::EntryRow::builder()
        .title(gettext("Proxy URI"))
        .text(original.proxy.uri.as_deref().unwrap_or_default())
        .build();
    proxy_uri.set_sensitive(proxy_mode.selected() == 2);
    proxy_group.add(&proxy_uri);
    proxy_mode.connect_selected_notify(glib::clone!(
        #[weak]
        proxy_uri,
        move |row| proxy_uri.set_sensitive(row.selected() == 2)
    ));
    page.add(&proxy_group);

    let background_group = adw::PreferencesGroup::builder()
        .title(gettext("Background"))
        .description(gettext(
            "Background access is requested through the desktop portal. Normal launch keeps working if access is denied.",
        ))
        .build();
    let background_enabled = adw::SwitchRow::builder()
        .title(gettext("Keep Running in Background"))
        .subtitle(gettext(
            "Closing the window keeps this web application active",
        ))
        .active(original.background.enabled)
        .build();
    background_group.add(&background_enabled);
    let autostart = adw::SwitchRow::builder()
        .title(gettext("Start at Login"))
        .subtitle(gettext(
            "Start all opted-in Bastle applications without opening windows",
        ))
        .active(original.background.autostart)
        .build();
    autostart.set_sensitive(background_enabled.is_active());
    background_group.add(&autostart);
    background_enabled.connect_active_notify(glib::clone!(
        #[weak]
        autostart,
        move |row| {
            autostart.set_sensitive(row.is_active());
            if !row.is_active() {
                autostart.set_active(false);
            }
        }
    ));
    page.add(&background_group);

    let filter_group = adw::PreferencesGroup::builder()
        .title(gettext("Content Filters"))
        .description(gettext(
            "Import WebKit content-extension JSON. Filters affect only this application.",
        ))
        .build();
    let edited = Rc::new(RefCell::new(original.clone()));
    for (filter_id, filter) in &original.content_filters {
        add_filter_row(&filter_group, &edited, filter_id.clone(), filter);
    }
    let import_row = adw::ActionRow::builder()
        .title(gettext("Import Filter List…"))
        .subtitle(gettext(
            "The list is validated by WebKit before it is added",
        ))
        .activatable(true)
        .build();
    import_row.add_suffix(&gtk::Image::from_icon_name("document-open-symbolic"));
    filter_group.add(&import_row);
    let filter_imported_message = gettext("Content filter imported");
    import_row.connect_activated(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        filter_group,
        #[strong]
        edited,
        #[weak]
        parent,
        #[strong]
        filter_imported_message,
        move |_| {
            glib::spawn_future_local(glib::clone!(
                #[weak]
                dialog,
                #[weak]
                filter_group,
                #[strong]
                edited,
                #[weak]
                parent,
                #[strong]
                filter_imported_message,
                async move {
                    match import_filter(dialog.root().and_downcast::<gtk::Window>().as_ref()).await
                    {
                        Ok(filter) => {
                            match edited.borrow_mut().add_content_filter(filter.clone()) {
                                Ok(filter_id) => {
                                    add_filter_row(&filter_group, &edited, filter_id, &filter);
                                    parent.toast(&filter_imported_message);
                                }
                                Err(error) => parent.toast(&error.to_string()),
                            }
                        }
                        Err(error) if !is_cancelled(&error) => parent.toast(&error.to_string()),
                        Err(_) => {}
                    }
                }
            ));
        }
    ));
    page.add(&filter_group);

    toolbar.set_content(Some(&page));
    dialog.set_child(Some(&toolbar));

    let background_request_reason =
        gettext("Keep this Bastle application running in the background");
    let background_denied_message = gettext("Background access was not enabled");
    let privacy_saved_message =
        gettext("Privacy settings saved; restart the web application to apply runtime changes");
    let autostart_failed_message = gettext("Autostart could not be updated by the portal");
    save.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        parent,
        #[strong]
        original,
        #[strong]
        edited,
        #[strong]
        background_request_reason,
        #[strong]
        background_denied_message,
        #[strong]
        privacy_saved_message,
        #[strong]
        autostart_failed_message,
        move |_| {
            let origins_text = origins.text();
            let proxy_uri_text = proxy_uri.text();
            let input = PolicyEditorInput {
                navigation_enabled: navigation_enabled.is_active(),
                origins: origins_text.as_str(),
                proxy_mode: selected_proxy_mode(proxy_mode.selected()),
                proxy_uri: proxy_uri_text.as_str(),
                background_enabled: background_enabled.is_active(),
                autostart: autostart.is_active(),
                start_url: &config.start_url,
            };
            let result = build_edited_policy(&edited.borrow(), &input);
            let mut policy = match result {
                Ok(policy) => policy,
                Err(error) => {
                    parent.toast(&error.to_string());
                    return;
                }
            };
            let parent_window = parent.clone();
            let app_id = id.clone();
            let original = original.clone();
            glib::spawn_future_local(glib::clone!(
                #[weak]
                dialog,
                #[strong]
                background_request_reason,
                #[strong]
                background_denied_message,
                #[strong]
                privacy_saved_message,
                #[strong]
                autostart_failed_message,
                async move {
                    let other_autostart = if needs_other_autostart(&original, &policy) {
                        match another_app_uses_autostart(&app_id) {
                            Ok(enabled) => enabled,
                            Err(error) => {
                                parent_window.toast(&error.to_string());
                                return;
                            }
                        }
                    } else {
                        false
                    };
                    let identifier = WindowIdentifier::from_native(&parent_window).await;
                    let mut portal_warning = None;
                    if policy.background.enabled && policy.background != original.background {
                        let requested_for_this_app = policy.background.autostart;
                        match background::request_access(
                            identifier.as_ref(),
                            &background_request_reason,
                            requested_for_this_app || other_autostart,
                        )
                        .await
                        {
                            Ok(grant) => {
                                policy.background.enabled = grant.background;
                                policy.background.autostart =
                                    requested_for_this_app && grant.autostart;
                                if (requested_for_this_app || other_autostart) && !grant.autostart {
                                    portal_warning = Some(anyhow!(
                                        "the desktop allowed background activity but not autostart"
                                    ));
                                }
                            }
                            Err(error) => {
                                parent_window
                                    .toast(&format!("{}: {error:#}", background_denied_message));
                                return;
                            }
                        }
                    } else if !policy.background.enabled && original.background.autostart {
                        if let Err(error) =
                            background::update_autostart(identifier.as_ref(), other_autostart).await
                        {
                            portal_warning = Some(error);
                        }
                    }

                    match AppService::portal().merge_policy(&app_id, &original, &policy) {
                        Ok(_) => {
                            dialog.close();
                            parent_window.toast(&privacy_saved_message);
                            if let Some(error) = portal_warning {
                                parent_window
                                    .toast(&format!("{}: {error:#}", autostart_failed_message));
                            }
                        }
                        Err(error) => parent_window.toast(&error.to_string()),
                    }
                }
            ));
        }
    ));

    dialog.present(Some(parent));
    dialog
}

fn another_app_uses_autostart(excluded: &AppId) -> Result<bool> {
    let service = AppService::portal();
    for app in service.list()?.apps {
        if app.id != *excluded && service.load_policy(&app.id)?.background.autostart {
            return Ok(true);
        }
    }
    Ok(false)
}

fn needs_other_autostart(original: &AppPolicyV2, edited: &AppPolicyV2) -> bool {
    (edited.background.enabled
        && edited.background != original.background
        && !edited.background.autostart)
        || (!edited.background.enabled && original.background.autostart)
}

struct PolicyEditorInput<'a> {
    navigation_enabled: bool,
    origins: &'a str,
    proxy_mode: ProxyMode,
    proxy_uri: &'a str,
    background_enabled: bool,
    autostart: bool,
    start_url: &'a str,
}

fn build_edited_policy(base: &AppPolicyV2, input: &PolicyEditorInput<'_>) -> Result<AppPolicyV2> {
    let mut policy = base.clone();
    policy.navigation.enabled = input.navigation_enabled;
    policy.navigation.allowed_origins = parse_origins(input.origins)?;
    if input.navigation_enabled {
        policy
            .navigation
            .allowed_origins
            .insert(Origin::from_url(&url::Url::parse(input.start_url)?)?);
    }
    policy.proxy.mode = input.proxy_mode;
    policy.proxy.uri = match input.proxy_mode {
        ProxyMode::Custom => Some(normalize_proxy_uri(input.proxy_uri)?),
        ProxyMode::System | ProxyMode::NoProxy => None,
    };
    policy.background.enabled = input.background_enabled;
    policy.background.autostart = input.background_enabled && input.autostart;
    policy.validate()?;
    Ok(policy)
}

fn parse_origins(value: &str) -> Result<BTreeSet<Origin>> {
    value
        .split(|character: char| character.is_whitespace() || character == ',' || character == ';')
        .filter(|origin| !origin.is_empty())
        .map(Origin::from_str)
        .collect()
}

fn proxy_mode_index(mode: ProxyMode) -> u32 {
    match mode {
        ProxyMode::System => 0,
        ProxyMode::NoProxy => 1,
        ProxyMode::Custom => 2,
    }
}

fn selected_proxy_mode(index: u32) -> ProxyMode {
    match index {
        1 => ProxyMode::NoProxy,
        2 => ProxyMode::Custom,
        _ => ProxyMode::System,
    }
}

fn add_filter_row(
    group: &adw::PreferencesGroup,
    policy: &Rc<RefCell<AppPolicyV2>>,
    filter_id: String,
    filter: &ContentFilterRuleSet,
) {
    let row = adw::SwitchRow::builder()
        .title(&filter.name)
        .subtitle(gettext("WebKit content-extension rules"))
        .active(filter.enabled)
        .build();
    let remove = gtk::Button::builder()
        .icon_name("edit-delete-symbolic")
        .tooltip_text(gettext("Remove Filter"))
        .valign(gtk::Align::Center)
        .build();
    remove.add_css_class("flat");
    row.add_suffix(&remove);
    row.connect_active_notify(glib::clone!(
        #[strong]
        policy,
        #[strong]
        filter_id,
        move |row| {
            if let Some(filter) = policy.borrow_mut().content_filters.get_mut(&filter_id) {
                filter.enabled = row.is_active();
            }
        }
    ));
    remove.connect_clicked(glib::clone!(
        #[weak]
        group,
        #[weak]
        row,
        #[strong]
        policy,
        move |_| {
            policy.borrow_mut().content_filters.remove(&filter_id);
            group.remove(&row);
        }
    ));
    group.add(&row);
}

async fn import_filter(parent: Option<&gtk::Window>) -> Result<ContentFilterRuleSet> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("WebKit Content Filters")));
    filter.add_mime_type("application/json");
    filter.add_pattern("*.json");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let file = gtk::FileDialog::builder()
        .title(gettext("Import Content Filter"))
        .accept_label(gettext("Import"))
        .filters(&filters)
        .modal(true)
        .build()
        .open_future(parent)
        .await
        .context("content filter selection was cancelled")?;
    let path = file
        .path()
        .context("the selected content filter is not a local file")?;
    let name = file
        .basename()
        .and_then(|name| Path::new(&name).file_stem().map(|stem| stem.to_owned()))
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| gettext("Imported Filter"));
    let bytes = gio::spawn_blocking(move || read_limited(&path))
        .await
        .map_err(|_| anyhow!("the content filter reader stopped unexpectedly"))??;
    let source = serde_json::from_slice(&bytes).context("invalid content filter JSON")?;
    let filter = ContentFilterRuleSet::new(name, source)?;
    content_filters::validate_filter(&filter).await?;
    Ok(filter)
}

fn read_limited(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take((MAX_CONTENT_FILTER_SOURCE_SIZE + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_CONTENT_FILTER_SOURCE_SIZE,
        "content filter exceeds the 8 MiB limit"
    );
    Ok(bytes)
}

fn is_cancelled(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("cancelled"))
}

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    let parent = BastleWindow::new(application);
    let config =
        crate::model::AppConfigV2::new("Privacy UI smoke test", "https://example.org/", 0)?;
    let dialog = present_editor(&parent, config.id.clone(), AppPolicyV2::default(), config);
    ensure!(
        dialog.title().as_str() == gettext("Privacy & Power"),
        "privacy dialog title was not initialized"
    );
    dialog.force_close();
    parent.destroy();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_editor_normalizes_and_deduplicates() {
        let origins = parse_origins(
            "https://example.org/path, HTTPS://EXAMPLE.ORG:443; http://other.example:8080/x",
        )
        .unwrap();
        assert_eq!(origins.len(), 2);
        assert!(origins.contains(&Origin::from_str("https://example.org").unwrap()));
    }

    #[test]
    fn policy_editor_keeps_new_features_opt_in() {
        let policy = build_edited_policy(
            &AppPolicyV2::default(),
            &PolicyEditorInput {
                navigation_enabled: false,
                origins: "",
                proxy_mode: ProxyMode::System,
                proxy_uri: "",
                background_enabled: false,
                autostart: false,
                start_url: "https://example.org/",
            },
        )
        .unwrap();
        assert!(!policy.navigation.enabled);
        assert!(!policy.background.enabled);
        assert!(policy.content_filters.is_empty());
        assert!(!needs_other_autostart(&policy, &policy));

        let mut enabling_background = policy.clone();
        enabling_background.background.enabled = true;
        assert!(needs_other_autostart(&policy, &enabling_background));

        enabling_background.background.autostart = true;
        assert!(!needs_other_autostart(&policy, &enabling_background));
    }
}
