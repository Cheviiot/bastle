// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fmt, str::FromStr};

use anyhow::{bail, Context, Result};
use rand::{distributions::Uniform, Rng};
use serde::{Deserialize, Serialize};
use url::Url;

pub const SCHEMA_VERSION: u32 = 2;
pub const APP_ID_LENGTH: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AppId(String);

impl AppId {
    pub fn generate() -> Self {
        let value = rand::thread_rng()
            .sample_iter(Uniform::from('a'..='z'))
            .take(APP_ID_LENGTH)
            .collect();
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for AppId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.len() != APP_ID_LENGTH
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            bail!("app id must contain exactly {APP_ID_LENGTH} lowercase ASCII letters or digits");
        }
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    #[serde(default = "default_window_width")]
    pub width: i32,
    #[serde(default = "default_window_height")]
    pub height: i32,
    #[serde(default)]
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: default_window_width(),
            height: default_window_height(),
            maximized: false,
        }
    }
}

fn default_window_width() -> i32 {
    1200
}

fn default_window_height() -> i32 {
    800
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfigV2 {
    pub schema_version: u32,
    pub id: AppId,
    pub title: String,
    pub start_url: String,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default = "default_true")]
    pub use_theme_color: bool,
    #[serde(default)]
    pub window: WindowState,
    #[serde(default)]
    pub sort_order: u32,
}

fn default_true() -> bool {
    true
}

impl AppConfigV2 {
    pub fn new(
        title: impl Into<String>,
        start_url: impl Into<String>,
        sort_order: u32,
    ) -> Result<Self> {
        let mut config = Self {
            schema_version: SCHEMA_VERSION,
            id: AppId::generate(),
            title: sanitize_title(&title.into()),
            start_url: start_url.into(),
            user_agent: None,
            use_theme_color: true,
            window: WindowState::default(),
            sort_order,
        };
        config.normalize_and_validate()?;
        Ok(config)
    }

    pub fn normalize_and_validate(&mut self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported app configuration version {}",
                self.schema_version
            );
        }

        self.title = sanitize_title(&self.title);
        if self.title.is_empty() {
            bail!("application title cannot be empty");
        }

        let url = parse_web_url(&self.start_url)?;
        self.start_url = url.to_string();
        self.window.width = self.window.width.clamp(320, 8192);
        self.window.height = self.window.height.clamp(200, 8192);
        if self.user_agent.as_deref().is_some_and(str::is_empty) {
            self.user_agent = None;
        }
        Ok(())
    }
}

pub fn parse_web_url(value: &str) -> Result<Url> {
    let value = value.trim();
    let candidate = Url::parse(value)
        .or_else(|_| Url::parse(&format!("https://{value}")))
        .with_context(|| format!("invalid URL: {value}"))?;
    if !matches!(candidate.scheme(), "http" | "https") {
        bail!("only HTTP and HTTPS URLs are supported");
    }
    if candidate.host().is_none() {
        bail!("URL must include a host");
    }
    Ok(candidate)
}

pub fn sanitize_title(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '\n' | '\r') {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_validation_is_strict() {
        assert!("abcdefghijkl".parse::<AppId>().is_ok());
        assert!("abc-defghijk".parse::<AppId>().is_err());
        assert!("ABCDEFGHIJKL".parse::<AppId>().is_err());
        assert!("short".parse::<AppId>().is_err());
    }

    #[test]
    fn web_url_is_normalized_and_restricted() {
        assert_eq!(
            parse_web_url("example.org").unwrap().as_str(),
            "https://example.org/"
        );
        assert!(parse_web_url("file:///etc/passwd").is_err());
        assert!(parse_web_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn titles_cannot_inject_desktop_keys() {
        assert_eq!(
            sanitize_title("Example\nExec=malware\r\n"),
            "Example Exec=malware"
        );
    }

    #[test]
    fn config_round_trips_as_version_two() {
        let config = AppConfigV2::new("Example", "https://example.org", 2).unwrap();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"user_agent\":null"));
        assert!(!json.contains("imported_from"));
        let decoded: AppConfigV2 = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, config);
    }
}
