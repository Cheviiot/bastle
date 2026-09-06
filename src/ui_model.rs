// SPDX-License-Identifier: GPL-3.0-only

//! Shared, presentation-only models for the adaptive library and forms.
//!
//! These types deliberately do not own repository or portal operations.  The
//! window and pages render them, while `AppService` remains the only place
//! where application data is changed.

use std::cmp::Ordering;

use crate::{
    model::{AppConfigV3, Engine},
    repository::RepositoryWarning,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibrarySortMode {
    #[default]
    TitleAscending,
    TitleDescending,
    Newest,
    Oldest,
}

impl LibrarySortMode {
    pub fn from_key(value: &str) -> Self {
        match value {
            "title-desc" => Self::TitleDescending,
            "newest" => Self::Newest,
            "oldest" => Self::Oldest,
            _ => Self::TitleAscending,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::TitleAscending => "title-asc",
            Self::TitleDescending => "title-desc",
            Self::Newest => "newest",
            Self::Oldest => "oldest",
        }
    }

    pub fn compare(self, left: &AppConfigV3, right: &AppConfigV3) -> Ordering {
        let title = || {
            left.title
                .to_lowercase()
                .cmp(&right.title.to_lowercase())
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        };
        match self {
            Self::TitleAscending => title(),
            Self::TitleDescending => title().reverse(),
            Self::Newest => right.sort_order.cmp(&left.sort_order).then_with(title),
            Self::Oldest => left.sort_order.cmp(&right.sort_order).then_with(title),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    Local,
    Missing,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherState {
    Available,
    Missing,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct LibraryItem {
    pub config: AppConfigV3,
    pub domain: String,
    pub icon_state: IconState,
    pub launcher_state: LauncherState,
}

impl LibraryItem {
    pub fn from_config(config: AppConfigV3) -> Self {
        let domain = url::Url::parse(&config.start_url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| config.start_url.clone());
        Self {
            config,
            domain,
            icon_state: IconState::Local,
            launcher_state: LauncherState::Unknown,
        }
    }
}

#[derive(Debug, Default)]
pub struct LibraryState {
    pub query: String,
    pub sort_mode: LibrarySortMode,
    pub items: Vec<LibraryItem>,
    pub warnings: Vec<RepositoryWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppFormDraft {
    pub title: String,
    pub start_url: String,
    pub user_agent: Option<String>,
    pub use_theme_color: bool,
    pub engine: Engine,
}
