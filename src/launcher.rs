// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{ensure, Context, Result};
use ashpd::{
    desktop::{
        dynamic_launcher::{DynamicLauncherProxy, LauncherType, PrepareInstallOptions},
        Icon,
    },
    WindowIdentifier,
};
use async_trait::async_trait;
use gtk::glib;

use crate::{config, model::AppConfigV3, model::AppId};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherCapabilities {
    pub desktop: String,
    pub portal_version: u32,
    pub application_launchers: bool,
    pub web_application_launchers: bool,
}

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

    pub async fn capabilities() -> Result<LauncherCapabilities> {
        let proxy = DynamicLauncherProxy::new()
            .await
            .context("Dynamic Launcher Portal is unavailable")?;
        let supported = proxy
            .supported_launcher_types()
            .await
            .context("Dynamic Launcher Portal did not report its supported launcher types")?;
        Ok(LauncherCapabilities {
            desktop: current_desktop(),
            portal_version: proxy.version(),
            application_launchers: supported.contains(LauncherType::Application),
            web_application_launchers: supported.contains(LauncherType::WebApplication),
        })
    }

    async fn supported_proxy() -> Result<DynamicLauncherProxy> {
        let proxy = DynamicLauncherProxy::new()
            .await
            .context("Dynamic Launcher Portal is unavailable")?;
        let supported = proxy
            .supported_launcher_types()
            .await
            .context("Dynamic Launcher Portal did not report its supported launcher types")?;
        ensure!(
            supported.contains(LauncherType::Application),
            "Dynamic Launcher Portal version {} does not support application launchers (desktop session: {})",
            proxy.version(),
            current_desktop()
        );
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
            .context("Dynamic Launcher Portal failed to open its confirmation dialog")?
            .response()
            .context("launcher installation was cancelled or denied")?;
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
        let proxy = Self::supported_proxy().await?;
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

fn current_desktop() -> String {
    desktop_name(std::env::var_os("XDG_CURRENT_DESKTOP"))
}

fn desktop_name(value: Option<std::ffi::OsString>) -> String {
    value
        .map(|desktop| desktop.to_string_lossy().into_owned())
        .filter(|desktop| !desktop.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
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
    fn missing_desktop_session_has_a_stable_diagnostic() {
        assert_eq!(desktop_name(None), "unknown");
        assert_eq!(desktop_name(Some("KDE".into())), "KDE");
        assert_eq!(desktop_name(Some("  ".into())), "unknown");
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
