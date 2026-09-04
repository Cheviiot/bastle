// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use gtk::glib;

use crate::model::{AppConfigV1, LegacySource, WindowState};

const LEGACY_APP_ID: &str = "io.github.zaedus.spider";
const LEGACY_GROUP: &str = "io/github/zaedus/spider";

type LegacySettings = HashMap<String, HashMap<String, String>>;

#[derive(Debug, Clone)]
pub struct LegacyCandidate {
    pub config: AppConfigV1,
}

#[derive(Debug, Default)]
pub struct LegacyPreview {
    pub candidates: Vec<LegacyCandidate>,
    pub invalid: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped: usize,
    pub invalid: usize,
    pub failed: usize,
}

pub fn locate_keyfile(selected: &Path) -> Result<PathBuf> {
    if selected.is_file() {
        return Ok(selected.to_path_buf());
    }
    if !selected.is_dir() {
        bail!("the selected legacy path does not exist");
    }
    let candidates = [
        selected.join("keyfile"),
        selected.join("glib-2.0/settings/keyfile"),
        selected.join("config/glib-2.0/settings/keyfile"),
    ];
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| anyhow!("no Spider GSettings keyfile was found in the selected directory"))
}

pub fn parse_keyfile(selected: &Path) -> Result<LegacyPreview> {
    let path = locate_keyfile(selected)?;
    let key_file = glib::KeyFile::new();
    key_file
        .load_from_file(&path, glib::KeyFileFlags::NONE)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let serialized = key_file
        .value(LEGACY_GROUP, "apps-settings")
        .context("Spider keyfile does not contain apps-settings")?;
    let variant = glib::Variant::parse(None, serialized.as_str())
        .context("Spider apps-settings is not valid GVariant data")?;
    let apps = variant
        .get::<LegacySettings>()
        .ok_or_else(|| anyhow!("Spider apps-settings has an unexpected GVariant type"))?;

    let mut preview = LegacyPreview::default();
    for (legacy_id, values) in apps {
        match candidate_from_values(&legacy_id, &values, preview.candidates.len() as u32) {
            Ok(candidate) => preview.candidates.push(candidate),
            Err(error) => preview.invalid.push(format!("{legacy_id}: {error}")),
        }
    }
    preview
        .candidates
        .sort_by(|left, right| left.config.title.cmp(&right.config.title));
    Ok(preview)
}

fn candidate_from_values(
    legacy_id: &str,
    values: &HashMap<String, String>,
    sort_order: u32,
) -> Result<LegacyCandidate> {
    let title = values
        .get("title")
        .ok_or_else(|| anyhow!("missing title"))?;
    let url = values.get("url").ok_or_else(|| anyhow!("missing URL"))?;
    let mut config = AppConfigV1::new(title, url, sort_order)?;
    config.user_agent = values
        .get("useragent")
        .filter(|value| !value.is_empty())
        .cloned();
    config.use_theme_color = values
        .get("hastitlebarcolor")
        .is_none_or(|value| value != "false");
    config.window = WindowState {
        width: parse_i32(values.get("windowwidth"), 1200),
        height: parse_i32(values.get("windowheight"), 800),
        maximized: values
            .get("windowmaximize")
            .and_then(|value| value.parse().ok())
            .unwrap_or(false),
    };
    config.imported_from = Some(LegacySource {
        app_id: LEGACY_APP_ID.to_owned(),
        legacy_id: legacy_id.to_owned(),
    });
    config.normalize_and_validate()?;
    Ok(LegacyCandidate { config })
}

fn parse_i32(value: Option<&String>, fallback: i32) -> i32 {
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_keyfile(contents: &str) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), contents).unwrap();
        file
    }

    #[test]
    fn parses_realistic_legacy_gvariant() {
        let file = write_keyfile(
            "[io/github/zaedus/spider]\napps-settings={'legacy': {'url': 'https://example.org/', 'title': 'Example', 'hastitlebarcolor': 'false', 'windowwidth': '900', 'windowheight': '700', 'windowmaximize': 'true'}}\n",
        );
        let preview = parse_keyfile(file.path()).unwrap();
        assert_eq!(preview.candidates.len(), 1);
        let app = &preview.candidates[0].config;
        assert_eq!(app.window.width, 900);
        assert!(app.window.maximized);
        assert!(!app.use_theme_color);
        assert_eq!(app.imported_from.as_ref().unwrap().legacy_id, "legacy");
    }

    #[test]
    fn malformed_keyfile_is_an_error() {
        let file = write_keyfile(include_str!("../tests/fixtures/malformed-keyfile"));
        assert!(parse_keyfile(file.path()).is_err());
    }

    #[test]
    fn invalid_rows_are_reported_individually() {
        let file = write_keyfile(
            "[io/github/zaedus/spider]\napps-settings={'bad': {'url': 'file:///tmp/nope', 'title': 'Bad'}}\n",
        );
        let preview = parse_keyfile(file.path()).unwrap();
        assert!(preview.candidates.is_empty());
        assert_eq!(preview.invalid.len(), 1);
    }
}
