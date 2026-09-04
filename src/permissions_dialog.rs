// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::{
    model::AppId,
    policy::{PermissionDecision, PermissionKind},
    service::AppService,
};

pub fn start(parent: &gtk::Window, id: AppId) {
    let policy = match AppService::portal().load_policy(&id) {
        Ok(policy) => policy,
        Err(error) => {
            notify(parent, &error.to_string());
            return;
        }
    };

    let dialog = adw::Dialog::builder()
        .title(gettext("Permissions"))
        .content_width(540)
        .content_height(560)
        .build();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let reset = gtk::Button::with_label(&gettext("Reset All"));
    reset.add_css_class("destructive-action");
    reset.set_sensitive(!policy.permissions.is_empty());
    header.pack_start(&reset);
    let save = gtk::Button::with_label(&gettext("Save"));
    save.add_css_class("suggested-action");
    header.pack_end(&save);
    toolbar.add_top_bar(&header);

    let page = adw::PreferencesPage::new();
    let original = policy.clone();
    let decisions = Rc::new(RefCell::new(policy));
    let reset_all = Rc::new(Cell::new(false));
    let rows = Rc::new(RefCell::new(Vec::new()));
    for (origin, permissions) in decisions.borrow().permissions.clone() {
        let group = adw::PreferencesGroup::builder()
            .title(origin.as_str())
            .build();
        for (kind, decision) in permissions {
            let labels = [gettext("Ask"), gettext("Allow"), gettext("Block")];
            let model =
                gtk::StringList::new(&[labels[0].as_str(), labels[1].as_str(), labels[2].as_str()]);
            let row = adw::ComboRow::builder()
                .title(permission_label(kind))
                .model(&model)
                .selected(decision_index(decision))
                .build();
            row.connect_selected_notify(glib::clone!(
                #[strong]
                decisions,
                #[strong]
                origin,
                #[strong]
                reset_all,
                move |row| {
                    reset_all.set(false);
                    decisions.borrow_mut().set_decision(
                        origin.clone(),
                        kind,
                        selected_decision(row.selected()),
                    );
                }
            ));
            rows.borrow_mut().push(row.clone());
            group.add(&row);
        }
        page.add(&group);
    }

    if decisions.borrow().permissions.is_empty() {
        let status = adw::StatusPage::builder()
            .icon_name("system-lock-screen-symbolic")
            .title(gettext("No Saved Permissions"))
            .description(gettext(
                "Websites will ask before using protected capabilities.",
            ))
            .build();
        toolbar.set_content(Some(&status));
    } else {
        toolbar.set_content(Some(&page));
    }
    dialog.set_child(Some(&toolbar));

    reset.connect_clicked(glib::clone!(
        #[strong]
        decisions,
        #[strong]
        rows,
        #[strong]
        reset_all,
        move |button| {
            decisions.borrow_mut().reset_permissions();
            for row in rows.borrow().iter() {
                row.set_selected(0);
            }
            reset_all.set(true);
            button.set_sensitive(false);
        }
    ));

    let parent_clone = parent.clone();
    save.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        #[strong]
        decisions,
        #[strong]
        reset_all,
        #[strong]
        original,
        move |_| match if reset_all.get() {
            AppService::portal().reset_policy(&id).map(|_| ())
        } else {
            AppService::portal()
                .merge_policy(&id, &original, &decisions.borrow())
                .map(|_| ())
        } {
            Ok(()) => {
                dialog.close();
            }
            Err(error) => notify(&parent_clone, &error.to_string()),
        }
    ));
    dialog.present(Some(parent));
}

fn decision_index(decision: PermissionDecision) -> u32 {
    match decision {
        PermissionDecision::Ask => 0,
        PermissionDecision::Allow => 1,
        PermissionDecision::Block => 2,
    }
}

fn selected_decision(selected: u32) -> PermissionDecision {
    match selected {
        1 => PermissionDecision::Allow,
        2 => PermissionDecision::Block,
        _ => PermissionDecision::Ask,
    }
}

fn permission_label(kind: PermissionKind) -> String {
    match kind {
        PermissionKind::Camera => gettext("Camera"),
        PermissionKind::Microphone => gettext("Microphone"),
        PermissionKind::Geolocation => gettext("Location"),
        PermissionKind::Notifications => gettext("Notifications"),
        PermissionKind::Clipboard => gettext("Clipboard"),
        PermissionKind::PointerLock => gettext("Pointer Lock"),
        PermissionKind::ThirdPartyStorage => gettext("Third-Party Storage"),
    }
}

fn notify(parent: &gtk::Window, message: &str) {
    let _ = parent.activate_action("win.notify", Some(&message.to_variant()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_indices_are_stable() {
        for decision in [
            PermissionDecision::Ask,
            PermissionDecision::Allow,
            PermissionDecision::Block,
        ] {
            assert_eq!(selected_decision(decision_index(decision)), decision);
        }
    }
}
