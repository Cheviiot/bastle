// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::model::AppId;

pub const POLICY_SCHEMA_VERSION: u32 = 2;
const LEGACY_POLICY_SCHEMA_VERSION: u32 = 1;
pub const MAX_CONTENT_FILTER_SOURCE_SIZE: usize = 8 * 1024 * 1024;
pub const MAX_CONTENT_FILTERS: usize = 32;
const MAX_TOTAL_CONTENT_FILTER_SOURCE_SIZE: usize = 12 * 1024 * 1024;
pub const MAX_POLICY_SERIALIZED_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Origin(String);

impl Origin {
    pub fn from_url(url: &Url) -> Result<Self> {
        if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
            bail!("origins must use HTTP or HTTPS and include a host");
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allowed_origins: BTreeSet<Origin>,
}

impl NavigationPolicy {
    pub fn allows(&self, origin: &Origin) -> bool {
        !self.enabled || self.allowed_origins.contains(origin)
    }

    fn validate(&self) -> Result<()> {
        for origin in &self.allowed_origins {
            let normalized = Origin::from_str(origin.as_str())?;
            ensure!(
                normalized == *origin,
                "navigation origin is not normalized: {origin}"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    System,
    NoProxy,
    Custom,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyPolicy {
    #[serde(default)]
    pub mode: ProxyMode,
    #[serde(default)]
    pub uri: Option<String>,
}

impl ProxyPolicy {
    fn validate(&self) -> Result<()> {
        match self.mode {
            ProxyMode::System | ProxyMode::NoProxy => {
                ensure!(self.uri.is_none(), "proxy URI is only valid in custom mode");
            }
            ProxyMode::Custom => {
                let uri = self.uri.as_deref().context("custom proxy URI is missing")?;
                ensure!(
                    normalize_proxy_uri(uri)? == uri,
                    "custom proxy URI is not normalized"
                );
            }
        }
        Ok(())
    }
}

pub fn normalize_proxy_uri(value: &str) -> Result<String> {
    let mut url = Url::parse(value.trim()).context("invalid proxy URI")?;
    ensure!(
        matches!(
            url.scheme(),
            "http" | "https" | "socks" | "socks4" | "socks4a" | "socks5"
        ),
        "proxy URI must use HTTP(S) or SOCKS"
    );
    ensure!(url.host().is_some(), "proxy URI must include a host");
    let normalized_host = url
        .host_str()
        .context("proxy URI must include a host")?
        .to_ascii_lowercase();
    url.set_host(Some(&normalized_host))
        .map_err(|_| anyhow!("proxy URI host is invalid"))?;
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "proxy credentials are not stored; remove them from the URI"
    );
    ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "proxy URI cannot contain a query or fragment"
    );
    ensure!(
        matches!(url.path(), "" | "/"),
        "proxy URI cannot contain a path"
    );
    url.set_path("");
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub autostart: bool,
}

impl BackgroundPolicy {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.enabled || !self.autostart,
            "autostart requires background mode"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentFilterRuleSet {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub source: Value,
}

impl ContentFilterRuleSet {
    pub fn new(name: impl Into<String>, source: Value) -> Result<Self> {
        let filter = Self {
            name: sanitize_filter_name(&name.into()),
            enabled: true,
            source,
        };
        filter.validate()?;
        Ok(filter)
    }

    fn validate(&self) -> Result<()> {
        ensure!(!self.name.is_empty(), "content filter name cannot be empty");
        ensure!(
            self.name == sanitize_filter_name(&self.name),
            "content filter name is not normalized"
        );
        ensure!(
            self.name.chars().count() <= 120,
            "content filter name is too long"
        );
        ensure!(
            self.source.is_array(),
            "content filter source must be a JSON array"
        );
        let size = serde_json::to_vec(&self.source)?.len();
        ensure!(
            size <= MAX_CONTENT_FILTER_SOURCE_SIZE,
            "content filter source exceeds the 8 MiB limit"
        );
        Ok(())
    }
}

fn sanitize_filter_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPolicyV2 {
    pub schema_version: u32,
    #[serde(default)]
    pub permissions: BTreeMap<Origin, BTreeMap<PermissionKind, PermissionDecision>>,
    #[serde(default)]
    pub navigation: NavigationPolicy,
    #[serde(default)]
    pub proxy: ProxyPolicy,
    #[serde(default)]
    pub background: BackgroundPolicy,
    #[serde(default)]
    pub content_filters: BTreeMap<String, ContentFilterRuleSet>,
}

impl Default for AppPolicyV2 {
    fn default() -> Self {
        Self {
            schema_version: POLICY_SCHEMA_VERSION,
            permissions: BTreeMap::new(),
            navigation: NavigationPolicy::default(),
            proxy: ProxyPolicy::default(),
            background: BackgroundPolicy::default(),
            content_filters: BTreeMap::new(),
        }
    }
}

impl AppPolicyV2 {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == POLICY_SCHEMA_VERSION,
            "unsupported policy version {}",
            self.schema_version
        );
        for origin in self.permissions.keys() {
            let normalized = Origin::from_str(origin.as_str())?;
            ensure!(
                normalized == *origin,
                "permission origin is not normalized: {origin}"
            );
        }
        self.navigation.validate()?;
        self.proxy.validate()?;
        self.background.validate()?;
        ensure!(
            self.content_filters.len() <= MAX_CONTENT_FILTERS,
            "too many content filters"
        );
        let mut total_source_size = 0usize;
        for (id, filter) in &self.content_filters {
            AppId::from_str(id).with_context(|| format!("invalid content filter id {id}"))?;
            filter.validate()?;
            total_source_size = total_source_size
                .checked_add(serde_json::to_vec(&filter.source)?.len())
                .ok_or_else(|| anyhow!("content filter size overflow"))?;
        }
        ensure!(
            total_source_size <= MAX_TOTAL_CONTENT_FILTER_SOURCE_SIZE,
            "combined content filters exceed the 12 MiB limit"
        );
        let serialized_size = serde_json::to_vec_pretty(self)?
            .len()
            .checked_add(1)
            .ok_or_else(|| anyhow!("policy size overflow"))?;
        ensure!(
            serialized_size <= MAX_POLICY_SERIALIZED_SIZE,
            "serialized policy exceeds the 32 MiB limit"
        );
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

    pub fn add_content_filter(&mut self, filter: ContentFilterRuleSet) -> Result<String> {
        ensure!(
            self.content_filters.len() < MAX_CONTENT_FILTERS,
            "no more than {MAX_CONTENT_FILTERS} content filters are supported"
        );
        let id = AppId::generate().to_string();
        self.content_filters.insert(id.clone(), filter);
        match self.validate() {
            Ok(()) => Ok(id),
            Err(error) => {
                self.content_filters.remove(&id);
                Err(error)
            }
        }
    }

    pub fn reset_permissions(&mut self) {
        self.permissions.clear();
    }

    pub fn for_restore(&self) -> Self {
        let mut restored = self.clone();
        restored.background = BackgroundPolicy::default();
        restored
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPolicyV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub permissions: BTreeMap<Origin, BTreeMap<PermissionKind, PermissionDecision>>,
}

impl AppPolicyV1 {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == LEGACY_POLICY_SCHEMA_VERSION,
            "unsupported legacy policy version {}",
            self.schema_version
        );
        for origin in self.permissions.keys() {
            let normalized = Origin::from_str(origin.as_str())?;
            ensure!(
                normalized == *origin,
                "permission origin is not normalized: {origin}"
            );
        }
        Ok(())
    }
}

impl From<AppPolicyV1> for AppPolicyV2 {
    fn from(legacy: AppPolicyV1) -> Self {
        Self {
            permissions: legacy.permissions,
            ..Self::default()
        }
    }
}

pub fn decode_policy(bytes: &[u8]) -> Result<(AppPolicyV2, bool)> {
    let document: Value = serde_json::from_slice(bytes).context("invalid policy JSON")?;
    let version = document
        .get("schema_version")
        .and_then(Value::as_u64)
        .context("missing or invalid policy schema_version")?;
    match version {
        version if version == u64::from(POLICY_SCHEMA_VERSION) => {
            let policy: AppPolicyV2 = serde_json::from_value(document)?;
            policy.validate()?;
            Ok((policy, false))
        }
        version if version == u64::from(LEGACY_POLICY_SCHEMA_VERSION) => {
            let legacy: AppPolicyV1 = serde_json::from_value(document)?;
            legacy.validate()?;
            let policy = AppPolicyV2::from(legacy);
            policy.validate()?;
            Ok((policy, true))
        }
        version => bail!("unsupported policy version {version}"),
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
        let mut policy = AppPolicyV2::default();
        policy.set_decision(
            origin.clone(),
            PermissionKind::Camera,
            PermissionDecision::Allow,
        );
        policy.set_decision(
            origin.clone(),
            PermissionKind::Camera,
            PermissionDecision::Ask,
        );
        assert!(policy.permissions.is_empty());
    }

    #[test]
    fn version_one_migrates_with_all_new_features_disabled() {
        let bytes = br#"{
            "schema_version": 1,
            "permissions": {
                "https://example.org": {"notifications": "allow"}
            }
        }"#;
        let (policy, migrated) = decode_policy(bytes).unwrap();
        assert!(migrated);
        assert_eq!(policy.schema_version, POLICY_SCHEMA_VERSION);
        assert!(!policy.navigation.enabled);
        assert_eq!(policy.proxy.mode, ProxyMode::System);
        assert!(!policy.background.enabled);
        assert!(policy.content_filters.is_empty());
        assert_eq!(
            policy.decision(
                &Origin::from_str("https://example.org").unwrap(),
                PermissionKind::Notifications
            ),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn policy_v2_round_trips() {
        let mut policy = AppPolicyV2::default();
        policy.navigation.enabled = true;
        policy
            .navigation
            .allowed_origins
            .insert(Origin::from_str("https://example.org").unwrap());
        policy
            .add_content_filter(ContentFilterRuleSet::new("Test", serde_json::json!([])).unwrap())
            .unwrap();
        let bytes = serde_json::to_vec(&policy).unwrap();
        let (decoded, migrated) = decode_policy(&bytes).unwrap();
        assert!(!migrated);
        assert_eq!(decoded, policy);
    }

    #[test]
    fn navigation_allowlist_is_disabled_by_default_and_exact_by_origin() {
        let allowed = Origin::from_str("https://example.org").unwrap();
        let other = Origin::from_str("https://other.example").unwrap();
        let mut navigation = NavigationPolicy::default();
        assert!(navigation.allows(&other));
        navigation.enabled = true;
        navigation.allowed_origins.insert(allowed.clone());
        assert!(navigation.allows(&allowed));
        assert!(!navigation.allows(&other));
    }

    #[test]
    fn content_filters_require_webkit_rule_arrays() {
        assert!(ContentFilterRuleSet::new("Valid", serde_json::json!([])).is_ok());
        assert!(ContentFilterRuleSet::new("Invalid", serde_json::json!({"rules": []})).is_err());
    }

    #[test]
    fn rejected_content_filter_does_not_mutate_policy() {
        let mut policy = AppPolicyV2::default();
        let oversized = ContentFilterRuleSet {
            name: "Oversized".to_owned(),
            enabled: true,
            source: serde_json::json!(["x".repeat(MAX_CONTENT_FILTER_SOURCE_SIZE)]),
        };
        assert!(policy.add_content_filter(oversized).is_err());
        assert!(policy.content_filters.is_empty());
    }

    #[test]
    fn restore_keeps_policy_but_requires_new_background_authorization() {
        let origin = Origin::from_str("https://example.org").unwrap();
        let mut policy = AppPolicyV2::default();
        policy.set_decision(
            origin.clone(),
            PermissionKind::Notifications,
            PermissionDecision::Allow,
        );
        policy.background.enabled = true;
        policy.background.autostart = true;

        let restored = policy.for_restore();
        assert_eq!(
            restored.decision(&origin, PermissionKind::Notifications),
            PermissionDecision::Allow
        );
        assert_eq!(restored.background, BackgroundPolicy::default());
    }

    #[test]
    fn proxy_uris_are_normalized_and_never_store_credentials() {
        assert_eq!(
            normalize_proxy_uri("socks5://Proxy.Example:1080/").unwrap(),
            "socks5://proxy.example:1080"
        );
        assert!(normalize_proxy_uri("http://user:secret@example.org:8080").is_err());
        assert!(normalize_proxy_uri("file:///tmp/proxy").is_err());
    }

    #[test]
    fn future_policy_versions_are_rejected_without_downgrade() {
        assert!(decode_policy(br#"{"schema_version": 99}"#).is_err());
    }
}
