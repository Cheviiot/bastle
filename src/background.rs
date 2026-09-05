// SPDX-License-Identifier: GPL-3.0-only

use anyhow::{ensure, Context, Result};
use ashpd::{desktop::background, WindowIdentifier};
use async_trait::async_trait;
use gettextrs::gettext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundGrant {
    pub background: bool,
    pub autostart: bool,
}

#[async_trait(?Send)]
pub trait BackgroundBackend: Clone {
    async fn request_access(
        &self,
        parent: Option<&WindowIdentifier>,
        reason: &str,
        autostart: bool,
    ) -> Result<BackgroundGrant>;

    async fn update_autostart(
        &self,
        parent: Option<&WindowIdentifier>,
        enabled: bool,
    ) -> Result<bool>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PortalBackground;

#[async_trait(?Send)]
impl BackgroundBackend for PortalBackground {
    async fn request_access(
        &self,
        parent: Option<&WindowIdentifier>,
        reason: &str,
        autostart: bool,
    ) -> Result<BackgroundGrant> {
        let proxy = background::BackgroundProxy::new()
            .await
            .context("Background Portal is unavailable")?;
        let options = background::BackgroundRequestOptions::default()
            .set_reason(reason)
            .set_auto_start(autostart)
            .set_command(["bastle", "--background"])
            .set_dbus_activatable(false);
        let response = proxy
            .request_background(parent, options)
            .await
            .context("Background Portal request failed")?
            .response()
            .context("background access was cancelled or denied")?;
        ensure!(
            response.run_in_background(),
            "the desktop did not grant background access"
        );
        Ok(BackgroundGrant {
            background: response.run_in_background(),
            autostart: response.auto_start(),
        })
    }

    async fn update_autostart(
        &self,
        parent: Option<&WindowIdentifier>,
        enabled: bool,
    ) -> Result<bool> {
        let granted = self
            .request_access(
                parent,
                &gettext("Keep selected Bastle web applications running in the background"),
                enabled,
            )
            .await?
            .autostart;
        ensure!(
            granted == enabled,
            "the desktop did not apply the requested autostart state"
        );
        Ok(granted)
    }
}

pub async fn set_status(message: &str) -> Result<()> {
    let proxy = background::BackgroundProxy::new()
        .await
        .context("Background Portal is unavailable")?;
    let message = message.chars().take(96).collect::<String>();
    proxy
        .set_status(background::SetStatusOptions::default().set_message(&message))
        .await
        .context("Background Portal could not update the status")
}

pub async fn capability() -> Result<u32> {
    Ok(background::BackgroundProxy::new()
        .await
        .context("Background Portal is unavailable")?
        .version())
}
