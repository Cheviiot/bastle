// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use ashpd::{
    desktop::{
        dynamic_launcher::{DynamicLauncherProxy, LauncherType, PrepareInstallOptions},
        Icon,
    },
    WindowIdentifier,
};
use async_trait::async_trait;
use gettextrs::gettext;
use gtk::glib;

use crate::{
    config,
    model::AppConfigV3,
    model::AppId,
    portal::{current_desktop, PortalFailureKind, PortalOperationError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallOutcome {
    Removed,
    AlreadyMissing,
}

#[async_trait(?Send)]
pub trait LauncherBackend {
    async fn install(
        &self,
        app: &AppConfigV3,
        icon: &[u8],
        parent: Option<&WindowIdentifier>,
    ) -> Result<()>;

    async fn uninstall(&self, id: &AppId) -> Result<UninstallOutcome>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PortalLauncher;

impl PortalLauncher {
    pub fn desktop_id(id: &AppId) -> String {
        format!("{}.desktop", config::managed_app_id(id))
    }

    fn desktop_entry(app: &AppConfigV3) -> Result<String> {
        let key_file = glib::KeyFile::new();
        key_file.set_string("Desktop Entry", "Type", "Application");
        key_file.set_string("Desktop Entry", "Name", &app.title);
        key_file.set_string("Desktop Entry", "Exec", &format!("bastle {}", app.id));
        key_file.set_string("Desktop Entry", "Terminal", "false");
        key_file.set_string("Desktop Entry", "Categories", "Network;");
        key_file.set_string("Desktop Entry", "StartupNotify", "true");
        Ok(key_file.to_data().to_string())
    }

    async fn supported_proxy() -> Result<DynamicLauncherProxy> {
        let proxy = DynamicLauncherProxy::new().await.map_err(|error| {
            PortalOperationError::from_ashpd(gettext("Dynamic Launcher"), error)
        })?;
        let supported = proxy.supported_launcher_types().await.map_err(|error| {
            PortalOperationError::from_ashpd(gettext("Dynamic Launcher capabilities"), error)
        })?;
        if !supported.contains(LauncherType::Application) {
            return Err(PortalOperationError::new(
                PortalFailureKind::Unsupported,
                gettext("Dynamic Launcher"),
                format!(
                    "{} {}: {} ({}: {})",
                    gettext("Version"),
                    proxy.version(),
                    gettext("Application launcher type is not supported"),
                    gettext("desktop session"),
                    current_desktop()
                ),
            )
            .into());
        }
        Ok(proxy)
    }
}

#[async_trait(?Send)]
impl LauncherBackend for PortalLauncher {
    async fn install(
        &self,
        app: &AppConfigV3,
        icon: &[u8],
        parent: Option<&WindowIdentifier>,
    ) -> Result<()> {
        let proxy = Self::supported_proxy().await?;
        let options = PrepareInstallOptions::default()
            .set_modal(true)
            .set_editable_icon(false)
            .set_editable_name(false)
            .set_launcher_type(LauncherType::Application);
        let response = proxy
            .prepare_install(parent, &app.title, Icon::Bytes(icon.to_vec()), options)
            .await
            .map_err(|error| {
                PortalOperationError::from_ashpd(gettext("Open launcher confirmation"), error)
            })?
            .response()
            .map_err(|error| {
                PortalOperationError::from_ashpd(gettext("Confirm launcher installation"), error)
            })?;
        proxy
            .install(
                response.token(),
                &Self::desktop_id(&app.id),
                &Self::desktop_entry(app)?,
                Default::default(),
            )
            .await
            .map_err(|error| {
                PortalOperationError::from_ashpd(gettext("Install launcher"), error)
            })?;
        Ok(())
    }

    async fn uninstall(&self, id: &AppId) -> Result<UninstallOutcome> {
        let proxy = Self::supported_proxy().await?;
        match proxy
            .uninstall(&Self::desktop_id(id), Default::default())
            .await
        {
            Ok(()) => Ok(UninstallOutcome::Removed),
            Err(error) if launcher_is_missing(&error.to_string()) => {
                Ok(UninstallOutcome::AlreadyMissing)
            }
            Err(error) => {
                Err(PortalOperationError::from_ashpd(gettext("Remove launcher"), error).into())
            }
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
        let app = AppConfigV3::new("Example\nExec=bad", "example.org", 0).unwrap();
        let desktop = PortalLauncher::desktop_entry(&app).unwrap();
        assert!(desktop.contains("Name=Example Exec=bad"));
        assert_eq!(desktop.matches("Exec=").count(), 2);
        assert!(desktop.contains(&format!("Exec=bastle {}", app.id)));
    }

    #[test]
    fn desktop_and_application_ids_share_the_same_valid_component() {
        let ordinary: AppId = "abcdefghijkl".parse().unwrap();
        assert_eq!(
            PortalLauncher::desktop_id(&ordinary),
            "io.github.cheviiot.bastle.abcdefghijkl.desktop"
        );
        let leading_digit: AppId = "1bcdefghijkl".parse().unwrap();
        assert_eq!(
            PortalLauncher::desktop_id(&leading_digit),
            "io.github.cheviiot.bastle.app1bcdefghijkl.desktop"
        );
    }
}
