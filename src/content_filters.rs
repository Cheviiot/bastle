// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use anyhow::{Context, Result};
use gtk::glib;
use webkit::{UserContentFilterStore, UserContentManager};

use crate::policy::{AppPolicyV2, ContentFilterRuleSet};

pub async fn validate_filter(filter: &ContentFilterRuleSet) -> Result<()> {
    let directory = tempfile::tempdir().context("failed to create filter validation directory")?;
    let store = UserContentFilterStore::new(directory.path().to_string_lossy().as_ref());
    let source = serde_json::to_vec(&filter.source)?;
    store
        .save_future("validation", &glib::Bytes::from(&source))
        .await
        .context("WebKit rejected the content filter")?;
    Ok(())
}

pub async fn apply_filters(
    profile: &Path,
    policy: &AppPolicyV2,
    manager: &UserContentManager,
) -> Vec<String> {
    let enabled = policy
        .content_filters
        .iter()
        .filter(|(_, filter)| filter.enabled)
        .collect::<Vec<_>>();
    if enabled.is_empty() {
        return Vec::new();
    }

    let directory = profile.join("content-filters");
    if let Err(error) = std::fs::create_dir_all(&directory) {
        return vec![format!("failed to create {}: {error}", directory.display())];
    }
    let store = UserContentFilterStore::new(directory.to_string_lossy().as_ref());
    let mut failures = Vec::new();
    for (id, filter) in enabled {
        let result = async {
            let source = serde_json::to_vec(&filter.source)?;
            let compiled = store
                .save_future(id, &glib::Bytes::from(&source))
                .await
                .with_context(|| format!("WebKit rejected {}", filter.name))?;
            manager.add_filter(&compiled);
            anyhow::Ok(())
        }
        .await;
        if let Err(error) = result {
            failures.push(format!("{}: {error:#}", filter.name));
        }
    }
    failures
}
