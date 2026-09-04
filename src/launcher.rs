// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use ashpd::{
    desktop::{
        dynamic_launcher::{DynamicLauncherProxy, LauncherType, PrepareInstallOptions},
        Icon,
    },
    WindowIdentifier,
};
use async_trait::async_trait;
use gtk::glib;

use crate::{config, model::AppConfigV2, model::AppId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallOutcome {
    Removed,
    AlreadyMissing,
}

#[async_trait(?Send)]
pub trait LauncherBackend {
    async fn install(
        &self,
        app: &AppConfigV2,
        icon: &[u8],
        parent: Option<&WindowIdentifier>,
    ) -> Result<()>;

    async fn uninstall(&self, id: &AppId) -> Result<UninstallOutcome>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PortalLauncher;

impl PortalLauncher {
    pub fn desktop_id(id: &AppId) -> String {
        format!("{}.{}.desktop", config::APP_ID, id)
    }

    fn desktop_entry(app: &AppConfigV2) -> Result<String> {
        let key_file = glib::KeyFile::new();
        key_file.set_string("Desktop Entry", "Type", "Application");
        key_file.set_string("Desktop Entry", "Name", &app.title);
        key_file.set_string("Desktop Entry", "Exec", &format!("bastle {}", app.id));
        key_file.set_string("Desktop Entry", "Terminal", "false");
        key_file.set_string("Desktop Entry", "Categories", "Network;");
        key_file.set_string("Desktop Entry", "StartupNotify", "true");
        Ok(key_file.to_data().to_string())
    }
}

#[async_trait(?Send)]
impl LauncherBackend for PortalLauncher {
    async fn install(
        &self,
        app: &AppConfigV2,
        icon: &[u8],
        parent: Option<&WindowIdentifier>,
    ) -> Result<()> {
        let proxy = DynamicLauncherProxy::new()
            .await
            .context("Dynamic Launcher Portal is unavailable")?;
        let options = PrepareInstallOptions::default()
            .set_modal(true)
            .set_editable_icon(false)
            .set_editable_name(false)
            .set_launcher_type(LauncherType::Application);
        let response = proxy
            .prepare_install(parent, &app.title, Icon::Bytes(icon.to_vec()), options)
            .await
            .context("launcher installation was denied or cancelled")?
            .response()
            .context("launcher installation was cancelled")?;
        proxy
            .install(
                response.token(),
                &Self::desktop_id(&app.id),
                &Self::desktop_entry(app)?,
                Default::default(),
            )
            .await
            .context("failed to install the launcher through the portal")?;
        Ok(())
    }

    async fn uninstall(&self, id: &AppId) -> Result<UninstallOutcome> {
        let proxy = DynamicLauncherProxy::new()
            .await
            .context("Dynamic Launcher Portal is unavailable")?;
        match proxy
            .uninstall(&Self::desktop_id(id), Default::default())
            .await
        {
            Ok(()) => Ok(UninstallOutcome::Removed),
            Err(error) if launcher_is_missing(&error.to_string()) => {
                Ok(UninstallOutcome::AlreadyMissing)
            }
            Err(error) => Err(error).context("failed to remove the launcher through the portal"),
        }
    }
}

fn launcher_is_missing(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("not found")
        || message.contains("does not exist")
        || message.contains("unknown launcher")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_is_generated_by_key_file() {
        let app = AppConfigV2::new("Example\nExec=bad", "example.org", 0).unwrap();
        let desktop = PortalLauncher::desktop_entry(&app).unwrap();
        assert!(desktop.contains("Name=Example Exec=bad"));
        assert_eq!(desktop.matches("Exec=").count(), 2);
        assert!(desktop.contains(&format!("Exec=bastle {}", app.id)));
    }
}
