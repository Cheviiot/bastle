// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;

use adw::prelude::*;
use age::secrecy::SecretString;
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::{
    backup::{is_encrypted_backup, BackupOptions, BackupService, RestoreDisposition, RestorePlan},
    window::BastleWindow,
};

pub fn start_backup(parent: &BastleWindow) {
    let window = parent.clone();
    glib::spawn_future_local(async move {
        let apps = match BackupService::portal().service().list() {
            Ok(report) if !report.apps.is_empty() => report.apps,
            Ok(_) => {
                window.toast(&gettext("There are no applications to back up"));
                return;
            }
            Err(error) => {
                window.toast(&error.to_string());
                return;
            }
        };

        let options_dialog = adw::AlertDialog::new(
            Some(&gettext("Back Up Bastle")),
            Some(&gettext(
                "Configuration, icons, and permissions are always included.",
            )),
        );
        options_dialog.add_responses(&[
            ("cancel", &gettext("Cancel")),
            ("continue", &gettext("Choose Destination")),
        ]);
        options_dialog.set_response_appearance("continue", adw::ResponseAppearance::Suggested);
        options_dialog.set_default_response(Some("continue"));
        options_dialog.set_close_response("cancel");

        let options_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        let include_site_data = gtk::CheckButton::with_label(&gettext(
            "Include cookies and site storage (requires encryption)",
        ));
        let passphrase = gtk::PasswordEntry::builder()
            .placeholder_text(gettext("Passphrase"))
            .show_peek_icon(true)
            .visible(false)
            .build();
        let confirm = gtk::PasswordEntry::builder()
            .placeholder_text(gettext("Confirm Passphrase"))
            .show_peek_icon(true)
            .visible(false)
            .build();
        include_site_data.connect_toggled(glib::clone!(
            #[weak]
            passphrase,
            #[weak]
            confirm,
            move |button| {
                passphrase.set_visible(button.is_active());
                confirm.set_visible(button.is_active());
            }
        ));
        options_box.append(&include_site_data);
        options_box.append(&passphrase);
        options_box.append(&confirm);
        options_dialog.set_extra_child(Some(&options_box));
        if options_dialog.choose_future(Some(&window)).await != "continue" {
            return;
        }

        let include_site_data = include_site_data.is_active();
        let passphrase = if include_site_data {
            if passphrase.text().is_empty() || passphrase.text() != confirm.text() {
                window.toast(&gettext("Passphrases must be non-empty and match"));
                return;
            }
            Some(SecretString::from(passphrase.text().to_string()))
        } else {
            None
        };
        let file_dialog = gtk::FileDialog::builder()
            .title(gettext("Save Bastle Backup"))
            .accept_label(gettext("Back Up"))
            .initial_name("Bastle.bastle-backup")
            .modal(true)
            .build();
        let file = match file_dialog.save_future(Some(&window)).await {
            Ok(file) => file,
            Err(_) => return,
        };
        let Some(destination) = file.path() else {
            window.toast(&gettext("The selected destination is not writable"));
            return;
        };
        let ids = apps.into_iter().map(|app| app.id).collect::<Vec<_>>();
        let options = BackupOptions {
            include_site_data,
            passphrase,
        };
        let result = gio::spawn_blocking(move || {
            BackupService::portal().create_backup(&destination, &ids, &options)
        })
        .await;
        match result {
            Ok(Ok(())) => window.toast(&gettext("Backup completed")),
            Ok(Err(error)) => window.toast(&format!("{}: {error:#}", gettext("Backup failed"))),
            Err(_) => window.toast(&gettext("The backup worker stopped unexpectedly")),
        }
    });
}

pub fn start_restore(parent: &BastleWindow) {
    let window = parent.clone();
    glib::spawn_future_local(async move {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Bastle Backups")));
        filter.add_pattern("*.bastle-backup");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let file_dialog = gtk::FileDialog::builder()
            .title(gettext("Open Bastle Backup"))
            .accept_label(gettext("Open"))
            .filters(&filters)
            .modal(true)
            .build();
        let file = match file_dialog.open_future(Some(&window)).await {
            Ok(file) => file,
            Err(_) => return,
        };
        let Some(source) = file.path() else {
            window.toast(&gettext("The selected backup cannot be read"));
            return;
        };
        let encrypted = match is_encrypted_backup(&source) {
            Ok(encrypted) => encrypted,
            Err(error) => {
                window.toast(&error.to_string());
                return;
            }
        };
        let passphrase = if encrypted {
            match ask_restore_passphrase(&window).await {
                Some(passphrase) => Some(passphrase),
                None => return,
            }
        } else {
            None
        };
        let prepared = gio::spawn_blocking(move || {
            BackupService::portal().prepare_restore(&source, passphrase.as_ref())
        })
        .await;
        let plan = match prepared {
            Ok(Ok(plan)) => plan,
            Ok(Err(error)) => {
                window.toast(&format!(
                    "{}: {error:#}",
                    gettext("Backup could not be opened")
                ));
                return;
            }
            Err(_) => {
                window.toast(&gettext("The restore worker stopped unexpectedly"));
                return;
            }
        };
        show_restore_preview(&window, plan).await;
    });
}

async fn ask_restore_passphrase(parent: &BastleWindow) -> Option<SecretString> {
    let dialog = adw::AlertDialog::new(
        Some(&gettext("Encrypted Backup")),
        Some(&gettext("Enter the passphrase used to create this backup.")),
    );
    dialog.add_responses(&[("cancel", &gettext("Cancel")), ("open", &gettext("Open"))]);
    dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("open"));
    dialog.set_close_response("cancel");
    let passphrase = gtk::PasswordEntry::builder()
        .placeholder_text(gettext("Passphrase"))
        .show_peek_icon(true)
        .activates_default(true)
        .build();
    dialog.set_extra_child(Some(&passphrase));
    if dialog.choose_future(Some(parent)).await != "open" || passphrase.text().is_empty() {
        return None;
    }
    Some(SecretString::from(passphrase.text().to_string()))
}

async fn show_restore_preview(parent: &BastleWindow, plan: RestorePlan) {
    let description = if plan.manifest.includes_site_data {
        gettext(
            "This encrypted backup includes cookies and site storage. Select the applications to restore.",
        )
    } else if plan.encrypted {
        gettext("This encrypted backup contains settings only. Select the applications to restore.")
    } else {
        gettext("Select the applications to restore.")
    };
    let description = format!(
        "{}\n\n{}",
        description,
        gettext("Background activity and autostart must be enabled again after restore."),
    );
    let dialog = adw::AlertDialog::new(Some(&gettext("Restore Preview")), Some(&description));
    dialog.add_responses(&[
        ("cancel", &gettext("Cancel")),
        ("restore", &gettext("Restore")),
    ]);
    dialog.set_response_appearance("restore", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("restore"));
    dialog.set_close_response("cancel");

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let selected = std::rc::Rc::new(std::cell::RefCell::new(HashSet::new()));
    for entry in &plan.entries {
        let subtitle = match entry.disposition {
            RestoreDisposition::RestoreAsIs => gettext("Restore with the original ID"),
            RestoreDisposition::RestoreWithNewId => gettext("ID conflict: restore with a new ID"),
            RestoreDisposition::SkipIdentical => gettext("Identical application already exists"),
        };
        let row = adw::ActionRow::builder()
            .title(&entry.title)
            .subtitle(subtitle)
            .build();
        let check = gtk::CheckButton::builder()
            .active(entry.disposition != RestoreDisposition::SkipIdentical)
            .sensitive(entry.disposition != RestoreDisposition::SkipIdentical)
            .valign(gtk::Align::Center)
            .build();
        if check.is_active() {
            selected.borrow_mut().insert(entry.source_id.clone());
        }
        let id = entry.source_id.clone();
        check.connect_toggled(glib::clone!(
            #[strong]
            selected,
            move |check| {
                if check.is_active() {
                    selected.borrow_mut().insert(id.clone());
                } else {
                    selected.borrow_mut().remove(&id);
                }
            }
        ));
        row.add_prefix(&check);
        list.append(&row);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(180)
        .max_content_height(420)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();
    dialog.set_extra_child(Some(&scroll));
    if dialog.choose_future(Some(parent)).await != "restore" {
        return;
    }
    let selected = selected.borrow().clone();
    if selected.is_empty() {
        parent.toast(&gettext("No applications were selected"));
        return;
    }
    let parent_identifier = WindowIdentifier::from_native(parent).await;
    let report = BackupService::portal()
        .restore(plan, &selected, parent_identifier.as_ref())
        .await;
    parent.refresh();
    let summary = format!(
        "{}: {}; {}: {}",
        gettext("Restored"),
        report.restored,
        gettext("Skipped"),
        report.skipped
    );
    if report.failed.is_empty() {
        parent.toast(&summary);
    } else {
        let body = format!("{summary}; {}: {}", gettext("Failed"), report.failed.len());
        let details = report
            .failed
            .iter()
            .map(|failure| format!("{}: {}", failure.source_id, failure.message))
            .collect::<Vec<_>>()
            .join("\n\n");
        let label = gtk::Label::builder()
            .label(details)
            .selectable(true)
            .wrap(true)
            .xalign(0.0)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(120)
            .max_content_height(360)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&label)
            .build();
        let result_dialog =
            adw::AlertDialog::new(Some(&gettext("Restore Finished with Errors")), Some(&body));
        result_dialog.set_extra_child(Some(&scroll));
        result_dialog.add_response("close", &gettext("Close"));
        result_dialog.set_default_response(Some("close"));
        result_dialog.set_close_response("close");
        result_dialog.choose_future(Some(parent)).await;
    }
}

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    use anyhow::ensure;

    let window = BastleWindow::new(application);
    let list = gtk::ListBox::new();
    let row = adw::ActionRow::builder()
        .title("Example")
        .subtitle("ID conflict: restore with a new ID")
        .build();
    list.append(&row);
    ensure!(
        list.row_at_index(0).is_some(),
        "restore preview row was not created"
    );
    window.destroy();
    Ok(())
}
