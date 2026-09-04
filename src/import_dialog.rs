// SPDX-License-Identifier: GPL-3.0-or-later

use std::rc::Rc;

use adw::prelude::*;
use anyhow::{anyhow, Result};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::glib;

use crate::{
    application::settings,
    legacy::{ImportSummary, LegacyCandidate},
    model::parse_web_url,
    service::AppService,
    util,
};

pub fn start(parent: &gtk::Window, first_run: bool) {
    let parent = parent.clone();
    glib::spawn_future_local(async move {
        let result = choose_and_preview(&parent, first_run).await;
        if first_run {
            let _ = settings().set_boolean("legacy-import-completed", true);
        }
        if let Err(error) = result {
            notify(&parent, &error.to_string());
        }
    });
}

async fn choose_and_preview(parent: &gtk::Window, first_run: bool) -> Result<()> {
    let body = if first_run {
        gettext("Bastle can import Spider settings without copying cookies, sessions, cache, or profiles.")
    } else {
        gettext("Choose the old Spider GSettings keyfile or its configuration directory.")
    };
    let chooser = adw::AlertDialog::new(Some(&gettext("Import from Spider")), Some(&body));
    chooser.add_responses(&[
        ("file", &gettext("Choose Keyfile")),
        ("folder", &gettext("Choose Folder")),
        ("cancel", &gettext("Not Now")),
    ]);
    chooser.set_default_response(Some("file"));
    chooser.set_close_response("cancel");
    let response = chooser.choose_future(Some(parent)).await;
    if response == "cancel" {
        return Ok(());
    }

    let file_dialog = gtk::FileDialog::builder()
        .title(gettext("Select Spider Settings"))
        .modal(true)
        .build();
    let selected = if response == "folder" {
        file_dialog.select_folder_future(Some(parent)).await?
    } else {
        file_dialog.open_future(Some(parent)).await?
    };
    let path = selected
        .path()
        .ok_or_else(|| anyhow!("the selected portal file has no local path"))?;
    let preview = AppService::portal().preview_legacy(&path)?;
    show_preview(parent, preview.candidates, preview.invalid.len());
    Ok(())
}

fn show_preview(parent: &gtk::Window, candidates: Vec<LegacyCandidate>, invalid: usize) {
    let dialog = adw::Dialog::builder()
        .title(gettext("Import Spider Applications"))
        .content_width(520)
        .content_height(480)
        .build();
    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = adw::HeaderBar::new();
    layout.append(&header);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .build();
    let mut choices = Vec::new();
    for candidate in candidates {
        let row = adw::ActionRow::builder()
            .title(&candidate.config.title)
            .subtitle(&candidate.config.start_url)
            .build();
        let check = gtk::CheckButton::builder()
            .active(true)
            .valign(gtk::Align::Center)
            .build();
        row.add_prefix(&check);
        list.append(&row);
        choices.push((check, candidate));
    }
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();
    layout.append(&scroll);
    if invalid > 0 {
        let warning = gtk::Label::builder()
            .label(format!("{}: {invalid}", gettext("Invalid entries")))
            .margin_bottom(6)
            .css_classes(["dim-label"])
            .build();
        layout.append(&warning);
    }
    let import_button = gtk::Button::builder()
        .label(gettext("Import Selected"))
        .halign(gtk::Align::Center)
        .margin_bottom(18)
        .css_classes(["suggested-action", "pill"])
        .build();
    layout.append(&import_button);
    dialog.set_child(Some(&layout));

    let choices = Rc::new(choices);
    let parent_clone = parent.clone();
    import_button.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        import_button,
        move |_| {
            import_button.set_sensitive(false);
            let selected = choices
                .iter()
                .filter(|(check, _)| check.is_active())
                .map(|(_, candidate)| candidate.clone())
                .collect::<Vec<_>>();
            let skipped = choices.len().saturating_sub(selected.len());
            let parent = parent_clone.clone();
            glib::spawn_future_local(glib::clone!(
                #[weak]
                dialog,
                async move {
                    let summary = import_selected(&parent, selected, invalid, skipped).await;
                    let _ = settings().set_boolean("legacy-import-completed", true);
                    dialog.close();
                    match summary {
                        Ok(summary) => {
                            notify(
                                &parent,
                                &format!(
                                    "{}: {}, {}: {}, {}: {}, {}: {}",
                                    gettext("Imported"),
                                    summary.imported,
                                    gettext("Skipped"),
                                    summary.skipped,
                                    gettext("Invalid"),
                                    summary.invalid,
                                    gettext("Failed"),
                                    summary.failed,
                                ),
                            );
                            let _ = parent.activate_action("win.refresh", None);
                        }
                        Err(error) => notify(&parent, &error.to_string()),
                    }
                }
            ));
        }
    ));
    dialog.present(Some(parent));
}

async fn import_selected(
    parent: &gtk::Window,
    candidates: Vec<LegacyCandidate>,
    invalid: usize,
    skipped: usize,
) -> Result<ImportSummary> {
    let parent_id = WindowIdentifier::from_native(parent).await;
    let configs = candidates
        .into_iter()
        .map(|candidate| candidate.config)
        .collect();
    Ok(AppService::portal()
        .import_many(
            configs,
            invalid,
            skipped,
            parent_id.as_ref(),
            |start_url| async move {
                let remote = match parse_web_url(&start_url) {
                    Ok(url) => util::get_website_meta(url)
                        .await
                        .ok()
                        .and_then(|meta| meta.icon),
                    Err(_) => None,
                };
                match remote {
                    Some(icon) => Ok(icon),
                    None => util::default_icon().await,
                }
            },
        )
        .await)
}

fn notify(parent: &gtk::Window, message: &str) {
    let _ = parent.activate_action("win.notify", Some(&message.to_variant()));
}
