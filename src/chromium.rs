// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use gtk::{gio, glib, prelude::*};

use crate::{
    model::{AppConfigV3, AppId},
    policy::AppPolicyV2,
};

pub const BUS_NAME: &str = "io.github.cheviiot.bastle_chromium";
pub const OBJECT_PATH: &str = "/io/github/cheviiot/bastle_chromium/Engine1";
pub const INTERFACE_NAME: &str = "io.github.cheviiot.bastle_chromium.Engine1";
pub const PROTOCOL_VERSION: u32 = 1;
const CALL_TIMEOUT_MSEC: i32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionCapabilities {
    pub protocol_version: u32,
    pub features: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChromiumClient;

pub trait ChromiumBackend: Clone {
    fn capabilities(&self) -> Result<CompanionCapabilities>;
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
    fn capabilities(&self) -> Result<CompanionCapabilities> {
        let response = self.call("GetCapabilities", None)?;
        let (protocol_version, features) = response
            .get::<(u32, Vec<String>)>()
            .context("Chromium companion returned malformed capabilities")?;
        let capabilities = CompanionCapabilities {
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
        let mut companion_policy = policy.clone();
        companion_policy.content_filters.clear();
        let policy_json = serde_json::to_string(&companion_policy)
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

impl CompanionCapabilities {
    pub fn require(&self, feature: &str) -> Result<()> {
        if !self.features.contains(feature) {
            bail!("Chromium companion does not support required feature {feature}");
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
        .context("Chromium companion is not installed or could not be activated")?;
        proxy
            .call_sync(
                method,
                parameters,
                gio::DBusCallFlags::NONE,
                CALL_TIMEOUT_MSEC,
                gio::Cancellable::NONE,
            )
            .with_context(|| format!("Chromium companion {method} call failed"))
    }
}

fn validate_capabilities(capabilities: &CompanionCapabilities) -> Result<()> {
    if capabilities.protocol_version != PROTOCOL_VERSION {
        bail!(
            "incompatible Chromium companion protocol {}; Bastle requires {}",
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
        let capabilities = CompanionCapabilities {
            protocol_version: PROTOCOL_VERSION + 1,
            features: BTreeSet::new(),
        };
        assert!(validate_capabilities(&capabilities)
            .unwrap_err()
            .to_string()
            .contains("incompatible"));
    }
}
