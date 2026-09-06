// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use gtk::{gio, glib, prelude::*};

use crate::{
    model::{AppConfigV3, AppId},
    policy::AppPolicyV2,
};

pub const BUS_NAME: &str = "io.github.cheviiot.bastle.Chromium";
pub const OBJECT_PATH: &str = "/io/github/cheviiot/bastle/Chromium/Engine1";
pub const INTERFACE_NAME: &str = "io.github.cheviiot.bastle.Chromium.Engine1";
pub const PROTOCOL_VERSION: u32 = 1;
pub const RUNTIME_SHELL_FEATURE: &str = "runtime-shell-v1";
pub const EXTENSION_ROOT: &str = "/app/extensions/chromium";
const CALL_TIMEOUT_MSEC: i32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromiumCapabilities {
    pub protocol_version: u32,
    pub features: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineAvailability {
    Missing,
    Available(ChromiumCapabilities),
    Incompatible(String),
    Broken(String),
}

impl EngineAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available(_))
    }

    pub fn diagnostic(&self) -> Option<&str> {
        match self {
            Self::Incompatible(message) | Self::Broken(message) => Some(message),
            Self::Missing | Self::Available(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChromiumClient;

pub trait ChromiumBackend: Clone {
    fn installed(&self) -> bool;
    fn capabilities(&self) -> Result<ChromiumCapabilities>;
    fn open_app(
        &self,
        app: &AppConfigV3,
        policy: &AppPolicyV2,
        token: &str,
        start_in_background: bool,
    ) -> Result<()>;
    fn delete_profile(&self, id: &AppId, token: &str) -> Result<()>;
}

impl ChromiumBackend for ChromiumClient {
    fn installed(&self) -> bool {
        ["electron", "main.js", "bin/zypak-wrapper"]
            .iter()
            .all(|entry| std::path::Path::new(EXTENSION_ROOT).join(entry).is_file())
    }

    fn capabilities(&self) -> Result<ChromiumCapabilities> {
        let response = self.call("GetCapabilities", None)?;
        let (protocol_version, features) = response
            .get::<(u32, Vec<String>)>()
            .context("the Chromium add-on returned malformed capabilities")?;
        let capabilities = ChromiumCapabilities {
            protocol_version,
            features: features.into_iter().collect(),
        };
        validate_capabilities(&capabilities)?;
        Ok(capabilities)
    }

    fn open_app(
        &self,
        app: &AppConfigV3,
        policy: &AppPolicyV2,
        token: &str,
        start_in_background: bool,
    ) -> Result<()> {
        let mut chromium_policy = policy.clone();
        chromium_policy.content_filters.clear();
        let policy_json = serde_json::to_string(&chromium_policy)
            .context("failed to serialize the Chromium runtime policy")?;
        let parameters = (
            app.id.as_str(),
            app.start_url.as_str(),
            app.title.as_str(),
            app.user_agent.as_deref().unwrap_or_default(),
            app.window.width,
            app.window.height,
            app.window.maximized,
            start_in_background,
            token,
            policy_json.as_str(),
        )
            .to_variant();
        self.call("OpenApp", Some(&parameters))?;
        Ok(())
    }

    fn delete_profile(&self, id: &AppId, token: &str) -> Result<()> {
        let parameters = (id.as_str(), token).to_variant();
        self.call("DeleteProfile", Some(&parameters))?;
        Ok(())
    }
}

impl ChromiumCapabilities {
    pub fn require(&self, feature: &str) -> Result<()> {
        if !self.features.contains(feature) {
            bail!("the Chromium add-on does not support required feature {feature}");
        }
        Ok(())
    }
}

impl ChromiumClient {
    fn call(&self, method: &str, parameters: Option<&glib::Variant>) -> Result<glib::Variant> {
        let proxy = gio::DBusProxy::for_bus_sync(
            gio::BusType::Session,
            gio::DBusProxyFlags::NONE,
            None,
            BUS_NAME,
            OBJECT_PATH,
            INTERFACE_NAME,
            gio::Cancellable::NONE,
        )
        .context("the Chromium add-on service could not be activated")?;
        proxy
            .call_sync(
                method,
                parameters,
                gio::DBusCallFlags::NONE,
                CALL_TIMEOUT_MSEC,
                gio::Cancellable::NONE,
            )
            .with_context(|| format!("Chromium add-on {method} call failed"))
    }
}

fn validate_capabilities(capabilities: &ChromiumCapabilities) -> Result<()> {
    if capabilities.protocol_version != PROTOCOL_VERSION {
        bail!(
            "incompatible Chromium add-on protocol {}; Bastle requires {}",
            capabilities.protocol_version,
            PROTOCOL_VERSION
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_mismatch_is_rejected_before_opening_an_app() {
        let capabilities = ChromiumCapabilities {
            protocol_version: PROTOCOL_VERSION + 1,
            features: BTreeSet::new(),
        };
        assert!(validate_capabilities(&capabilities)
            .unwrap_err()
            .to_string()
            .contains("incompatible"));
    }

    #[test]
    fn availability_distinguishes_user_visible_states() {
        assert!(!EngineAvailability::Missing.is_available());
        assert_eq!(EngineAvailability::Missing.diagnostic(), None);
        let broken = EngineAvailability::Broken("failed".to_owned());
        assert_eq!(broken.diagnostic(), Some("failed"));
    }
}
