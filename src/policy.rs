// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::BTreeMap, fmt, str::FromStr};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use url::Url;

pub const POLICY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Origin(String);

impl Origin {
    pub fn from_url(url: &Url) -> Result<Self> {
        if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
            bail!("permission origins must use HTTP or HTTPS and include a host");
        }

        let value = url.origin().ascii_serialization();
        if value == "null" {
            bail!("URL does not have a tuple origin");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for Origin {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let url = Url::parse(value).with_context(|| format!("invalid origin URL: {value}"))?;
        Self::from_url(&url)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    Clipboard,
    PointerLock,
    ThirdPartyStorage,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    #[default]
    Ask,
    Allow,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPolicyV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub permissions: BTreeMap<Origin, BTreeMap<PermissionKind, PermissionDecision>>,
}

impl Default for AppPolicyV1 {
    fn default() -> Self {
        Self {
            schema_version: POLICY_SCHEMA_VERSION,
            permissions: BTreeMap::new(),
        }
    }
}

impl AppPolicyV1 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != POLICY_SCHEMA_VERSION {
            bail!("unsupported policy version {}", self.schema_version);
        }
        for origin in self.permissions.keys() {
            let normalized = Origin::from_str(origin.as_str())?;
            if normalized != *origin {
                bail!("permission origin is not normalized: {origin}");
            }
        }
        Ok(())
    }

    pub fn decision(&self, origin: &Origin, kind: PermissionKind) -> PermissionDecision {
        self.permissions
            .get(origin)
            .and_then(|decisions| decisions.get(&kind))
            .copied()
            .unwrap_or_default()
    }

    pub fn set_decision(
        &mut self,
        origin: Origin,
        kind: PermissionKind,
        decision: PermissionDecision,
    ) {
        if decision == PermissionDecision::Ask {
            if let Some(decisions) = self.permissions.get_mut(&origin) {
                decisions.remove(&kind);
                if decisions.is_empty() {
                    self.permissions.remove(&origin);
                }
            }
            return;
        }

        self.permissions
            .entry(origin)
            .or_default()
            .insert(kind, decision);
    }

    pub fn reset(&mut self) {
        self.permissions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_normalized() {
        let origin = Origin::from_str("HTTPS://Example.COM:443/path?q=1").unwrap();
        assert_eq!(origin.as_str(), "https://example.com");

        let non_default_port = Origin::from_str("http://example.com:8080/path").unwrap();
        assert_eq!(non_default_port.as_str(), "http://example.com:8080");
        assert!(Origin::from_str("file:///tmp/example").is_err());
    }

    #[test]
    fn ask_is_implicit_and_removes_empty_origin_entries() {
        let origin = Origin::from_str("https://example.org").unwrap();
        let mut policy = AppPolicyV1::default();
        assert_eq!(
            policy.decision(&origin, PermissionKind::Camera),
            PermissionDecision::Ask
        );

        policy.set_decision(
            origin.clone(),
            PermissionKind::Camera,
            PermissionDecision::Allow,
        );
        assert_eq!(
            policy.decision(&origin, PermissionKind::Camera),
            PermissionDecision::Allow
        );

        policy.set_decision(
            origin.clone(),
            PermissionKind::Camera,
            PermissionDecision::Ask,
        );
        assert!(policy.permissions.is_empty());
    }

    #[test]
    fn policy_round_trips_as_version_one() {
        let origin = Origin::from_str("https://example.org/path").unwrap();
        let mut policy = AppPolicyV1::default();
        policy.set_decision(
            origin,
            PermissionKind::Notifications,
            PermissionDecision::Block,
        );

        let json = serde_json::to_string(&policy).unwrap();
        let decoded: AppPolicyV1 = serde_json::from_str(&json).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, policy);
    }
}
