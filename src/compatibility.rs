// SPDX-License-Identifier: GPL-3.0-only

use anyhow::{bail, Context, Result};
use gettextrs::gettext;
use serde::Deserialize;

use crate::model::{parse_web_url, Engine};

const CATALOG_SCHEMA_VERSION: u32 = 1;
const BUNDLED_CATALOG: &str = include_str!("../data/compatibility-catalog-v1.json");

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CompatibilityCatalogV1 {
    schema_version: u32,
    entries: Vec<CompatibilityEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CompatibilityEntry {
    host_pattern: String,
    recommended_engine: Engine,
    reason_code: String,
}

impl CompatibilityCatalogV1 {
    pub fn bundled() -> Result<Self> {
        let catalog: Self = serde_json::from_str(BUNDLED_CATALOG)
            .context("invalid bundled compatibility catalog")?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn recommendation(&self, url: &str) -> Result<Option<&CompatibilityEntry>> {
        let url = parse_web_url(url)?;
        let host = url
            .host_str()
            .context("URL does not contain a normalized host")?;
        Ok(self
            .entries
            .iter()
            .find(|entry| host_matches(&entry.host_pattern, host)))
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            bail!(
                "unsupported compatibility catalog version {}",
                self.schema_version
            );
        }
        for entry in &self.entries {
            validate_pattern(&entry.host_pattern)?;
            if entry.reason_code.is_empty()
                || !entry
                    .reason_code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            {
                bail!("invalid compatibility reason code");
            }
        }
        Ok(())
    }
}

impl CompatibilityEntry {
    pub fn recommended_engine(&self) -> Engine {
        self.recommended_engine
    }

    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }
}

pub fn reason_description(reason_code: &str) -> String {
    match reason_code {
        "webrtc_compatibility" => gettext(
            "This site relies on WebRTC behavior that is usually more compatible with Chromium.",
        ),
        "unsupported_browser" => gettext(
            "This site commonly rejects or limits WebKit browsers. Chromium is recommended.",
        ),
        "optimized_for_chromium" => {
            gettext("This site is optimized for Chromium-specific browser behavior.")
        }
        _ => gettext("Chromium is recommended for this site by the bundled compatibility catalog."),
    }
}

fn validate_pattern(pattern: &str) -> Result<()> {
    let host = pattern.strip_prefix("*.").unwrap_or(pattern);
    if host.is_empty()
        || host != host.to_ascii_lowercase()
        || !host.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
    {
        bail!("invalid compatibility host pattern {pattern}");
    }
    Ok(())
}

fn host_matches(pattern: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    match pattern.strip_prefix("*.") {
        Some(suffix) => host == suffix || host.ends_with(&format!(".{suffix}")),
        None => host == pattern,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_is_valid_and_defaults_unknown_sites_to_webkit() {
        let catalog = CompatibilityCatalogV1::bundled().unwrap();
        assert!(catalog
            .recommendation("https://example.org")
            .unwrap()
            .is_none());
        let recommendation = catalog
            .recommendation("https://meet.google.com/abc-defg-hij")
            .unwrap()
            .unwrap();
        assert_eq!(recommendation.recommended_engine(), Engine::Chromium);
        assert_eq!(recommendation.reason_code(), "webrtc_compatibility");
    }

    #[test]
    fn wildcard_matching_respects_label_boundaries() {
        assert!(host_matches("*.office.com", "office.com"));
        assert!(host_matches("*.office.com", "www.office.com"));
        assert!(!host_matches("*.office.com", "fakeoffice.com"));
        assert!(!host_matches("*.office.com", "office.com.example"));
    }

    #[test]
    fn invalid_catalog_versions_are_rejected() {
        let catalog: CompatibilityCatalogV1 =
            serde_json::from_str(r#"{"schema_version":2,"entries":[]}"#).unwrap();
        assert!(catalog.validate().is_err());
    }
}
