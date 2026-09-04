// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use gtk::glib;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tempfile::{Builder, NamedTempFile, TempDir};

use crate::{
    config::DATA_DIR_NAME,
    model::{decode_app_config, AppConfigV3, AppId},
    policy::{decode_policy, AppPolicyV2, Origin, PermissionDecision, PermissionKind},
};

const CONFIG_FILE: &str = "app.json";
const ICON_FILE: &str = "icon.png";
const POLICY_FILE: &str = "policy.json";
const COMPANION_TOKEN_FILE: &str = "chromium.token";
const METADATA_LOCK_FILE: &str = ".metadata.lock";
const POLICY_LOCK_FILE: &str = ".policy.lock";
const BACKGROUND_LOCK_FILE: &str = ".background.lock";
const COMPANION_QUEUE_FILE: &str = "pending-chromium-deletions.json";
const COMPANION_QUEUE_LOCK_FILE: &str = ".companion-deletions.lock";
pub const RUNTIME_LOCK_FILE: &str = ".runtime.lock";
const COMPANION_QUEUE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingCompanionDeletion {
    pub id: AppId,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PendingCompanionDeletionQueue {
    schema_version: u32,
    entries: Vec<PendingCompanionDeletion>,
}

impl Default for PendingCompanionDeletionQueue {
    fn default() -> Self {
        Self {
            schema_version: COMPANION_QUEUE_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct ProfileLock {
    _file: fs::File,
}

#[derive(Debug)]
pub struct BackgroundLock {
    _file: fs::File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryWarning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct LoadReport {
    pub apps: Vec<AppConfigV3>,
    pub warnings: Vec<RepositoryWarning>,
}

#[derive(Debug)]
pub struct AppSnapshot {
    pub config: AppConfigV3,
    pub icon: Vec<u8>,
    pub policy: AppPolicyV2,
}

#[derive(Debug, Clone)]
pub struct AppRepository {
    data_root: PathBuf,
    cache_root: PathBuf,
}

#[derive(Debug)]
pub struct StagedApp {
    directory: TempDir,
    final_path: PathBuf,
}

#[derive(Debug)]
pub struct StagedProfile {
    directory: TempDir,
    final_path: PathBuf,
}

impl StagedApp {
    pub fn commit(self) -> Result<()> {
        let staging_path = self.directory.keep();
        if let Err(error) = fs::rename(&staging_path, &self.final_path) {
            let _ = fs::remove_dir_all(&staging_path);
            return Err(error).with_context(|| {
                format!(
                    "failed to commit app directory {}",
                    self.final_path.display()
                )
            });
        }
        sync_parent(&self.final_path)?;
        Ok(())
    }
}

impl StagedProfile {
    pub fn commit(self) -> Result<()> {
        let staging_path = self.directory.keep();
        if let Err(error) = fs::rename(&staging_path, &self.final_path) {
            let _ = fs::remove_dir_all(&staging_path);
            return Err(error).with_context(|| {
                format!(
                    "failed to commit profile directory {}",
                    self.final_path.display()
                )
            });
        }
        sync_parent(&self.final_path)
    }
}

impl AppRepository {
    pub fn for_current_user() -> Self {
        Self::new(
            glib::user_data_dir().join(DATA_DIR_NAME),
            glib::user_cache_dir().join(DATA_DIR_NAME),
        )
    }

    pub fn new(data_root: impl Into<PathBuf>, cache_root: impl Into<PathBuf>) -> Self {
        Self {
            data_root: data_root.into(),
            cache_root: cache_root.into(),
        }
    }

    pub fn apps_root(&self) -> PathBuf {
        self.data_root.join("apps")
    }

    pub fn lock_background(&self) -> Result<BackgroundLock> {
        fs::create_dir_all(&self.data_root)
            .with_context(|| format!("failed to create {}", self.data_root.display()))?;
        let path = self.data_root.join(BACKGROUND_LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)
            .with_context(|| format!("failed to lock {}", path.display()))?;
        Ok(BackgroundLock { _file: file })
    }

    pub fn profile_dir(&self, id: &AppId) -> PathBuf {
        self.data_root.join("profiles").join(id.as_str())
    }

    pub fn contains_any_data(&self, id: &AppId) -> bool {
        self.app_dir(id).exists() || self.profile_dir(id).exists() || self.cache_dir(id).exists()
    }

    pub fn acquire_runtime_lock(&self, id: &AppId) -> Result<ProfileLock> {
        self.lock_profile(id, rustix::fs::FlockOperation::NonBlockingLockShared)
            .context("the WebKit profile is temporarily unavailable")
    }

    pub fn try_acquire_profile_snapshot_lock(&self, id: &AppId) -> Result<ProfileLock> {
        self.lock_profile(id, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .context("the WebKit profile is in use by a running application")
    }

    fn lock_profile(
        &self,
        id: &AppId,
        operation: rustix::fs::FlockOperation,
    ) -> Result<ProfileLock> {
        let profile = self.profile_dir(id);
        fs::create_dir_all(&profile)
            .with_context(|| format!("failed to create {}", profile.display()))?;
        let path = profile.join(RUNTIME_LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        rustix::fs::flock(&file, operation)
            .with_context(|| format!("failed to lock {}", path.display()))?;
        Ok(ProfileLock { _file: file })
    }

    pub fn cache_dir(&self, id: &AppId) -> PathBuf {
        self.cache_root.join(id.as_str())
    }

    pub fn app_dir(&self, id: &AppId) -> PathBuf {
        self.apps_root().join(id.as_str())
    }

    pub fn list(&self) -> Result<LoadReport> {
        let apps_root = self.apps_root();
        if !apps_root.exists() {
            return Ok(LoadReport::default());
        }

        let mut report = LoadReport::default();
        for entry in fs::read_dir(&apps_root)
            .with_context(|| format!("failed to read {}", apps_root.display()))?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    report.warnings.push(RepositoryWarning {
                        path: apps_root.clone(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            let path = entry.path();
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
                || entry.file_name().to_string_lossy().starts_with(".tmp-")
            {
                continue;
            }

            let config_path = path.join(CONFIG_FILE);
            match self.load_from_path(&config_path) {
                Ok(app) if app.id.as_str() == entry.file_name().to_string_lossy() => {
                    let policy_path = path.join(POLICY_FILE);
                    if policy_path.exists() {
                        if let Err(error) = self.load_policy(&app.id) {
                            report.warnings.push(RepositoryWarning {
                                path: policy_path,
                                message: error.to_string(),
                            });
                        }
                    }
                    report.apps.push(app);
                }
                Ok(_) => report.warnings.push(RepositoryWarning {
                    path: config_path,
                    message: "directory name does not match the app id".to_owned(),
                }),
                Err(error) => report.warnings.push(RepositoryWarning {
                    path: config_path,
                    message: error.to_string(),
                }),
            }
        }

        report.apps.sort_by(|left, right| {
            left.sort_order
                .cmp(&right.sort_order)
                .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
        });
        Ok(report)
    }

    pub fn load(&self, id: &AppId) -> Result<AppConfigV3> {
        self.load_from_path(&self.app_dir(id).join(CONFIG_FILE))
    }

    pub fn snapshot(&self, id: &AppId) -> Result<AppSnapshot> {
        let _metadata_lock = self.lock_app_file(id, METADATA_LOCK_FILE)?;
        let _policy_lock = self.lock_app_file(id, POLICY_LOCK_FILE)?;
        let policy_path = self.app_dir(id).join(POLICY_FILE);
        let policy = if policy_path.exists() {
            self.load_policy_from_locked_path(&policy_path)?
        } else {
            AppPolicyV2::default()
        };
        Ok(AppSnapshot {
            config: self.load(id)?,
            icon: self.read_icon(id)?,
            policy,
        })
    }

    fn load_from_path(&self, path: &Path) -> Result<AppConfigV3> {
        let contents =
            fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let (app, migrated) = decode_app_config(&contents)
            .with_context(|| format!("invalid app configuration in {}", path.display()))?;
        if migrated {
            replace_json(path, &app)
                .with_context(|| format!("failed to migrate {} to schema v3", path.display()))?;
        }
        Ok(app)
    }

    pub fn read_icon(&self, id: &AppId) -> Result<Vec<u8>> {
        let path = self.app_dir(id).join(ICON_FILE);
        fs::read(&path).with_context(|| format!("failed to read {}", path.display()))
    }

    pub fn contains(&self, id: &AppId) -> bool {
        self.app_dir(id).join(CONFIG_FILE).is_file()
    }

    pub fn companion_token(&self, id: &AppId) -> Result<String> {
        let app_dir = self.app_dir(id);
        if !app_dir.is_dir() {
            bail!("app {id} is not stored locally");
        }
        let _metadata_lock = self.lock_app_file(id, METADATA_LOCK_FILE)?;
        let path = app_dir.join(COMPANION_TOKEN_FILE);
        if path.exists() {
            let token = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            validate_companion_token(&token)?;
            return Ok(token);
        }

        let mut bytes = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let temporary = stage_bytes(&path, token.as_bytes())?;
        persist_staged(temporary, &path)?;
        sync_parent(&path)?;
        Ok(token)
    }

    pub fn pending_companion_deletions(&self) -> Result<Vec<PendingCompanionDeletion>> {
        fs::create_dir_all(&self.data_root)
            .with_context(|| format!("failed to create {}", self.data_root.display()))?;
        let _lock = self.lock_companion_queue()?;
        Ok(self.load_companion_queue()?.entries)
    }

    pub fn enqueue_companion_deletion(&self, id: &AppId, token: &str) -> Result<()> {
        validate_companion_token(token)?;
        fs::create_dir_all(&self.data_root)
            .with_context(|| format!("failed to create {}", self.data_root.display()))?;
        let _lock = self.lock_companion_queue()?;
        let mut queue = self.load_companion_queue()?;
        if let Some(entry) = queue.entries.iter_mut().find(|entry| entry.id == *id) {
            entry.token = token.to_owned();
        } else {
            queue.entries.push(PendingCompanionDeletion {
                id: id.clone(),
                token: token.to_owned(),
            });
        }
        replace_json(&self.data_root.join(COMPANION_QUEUE_FILE), &queue)
    }

    pub fn complete_companion_deletion(&self, id: &AppId) -> Result<()> {
        fs::create_dir_all(&self.data_root)
            .with_context(|| format!("failed to create {}", self.data_root.display()))?;
        let _lock = self.lock_companion_queue()?;
        let mut queue = self.load_companion_queue()?;
        queue.entries.retain(|entry| entry.id != *id);
        replace_json(&self.data_root.join(COMPANION_QUEUE_FILE), &queue)
    }

    fn load_companion_queue(&self) -> Result<PendingCompanionDeletionQueue> {
        let path = self.data_root.join(COMPANION_QUEUE_FILE);
        if !path.exists() {
            return Ok(PendingCompanionDeletionQueue::default());
        }
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let queue: PendingCompanionDeletionQueue = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid JSON in {}", path.display()))?;
        if queue.schema_version != COMPANION_QUEUE_SCHEMA_VERSION {
            bail!(
                "unsupported pending Chromium deletion version {}",
                queue.schema_version
            );
        }
        for entry in &queue.entries {
            validate_companion_token(&entry.token)?;
        }
        Ok(queue)
    }

    fn lock_companion_queue(&self) -> Result<fs::File> {
        let path = self.data_root.join(COMPANION_QUEUE_LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)
            .with_context(|| format!("failed to lock {}", path.display()))?;
        Ok(file)
    }

    pub fn load_policy(&self, id: &AppId) -> Result<AppPolicyV2> {
        let path = self.app_dir(id).join(POLICY_FILE);
        if !path.exists() {
            return Ok(AppPolicyV2::default());
        }
        let _policy_lock = self.lock_app_file(id, POLICY_LOCK_FILE)?;
        if !path.exists() {
            return Ok(AppPolicyV2::default());
        }
        self.load_policy_from_locked_path(&path)
    }

    fn load_policy_from_locked_path(&self, path: &Path) -> Result<AppPolicyV2> {
        let contents =
            fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let (policy, migrated) = decode_policy(&contents)
            .with_context(|| format!("invalid policy in {}", path.display()))?;
        if migrated {
            replace_json(path, &policy).with_context(|| {
                format!("failed to migrate {} to policy schema v2", path.display())
            })?;
        }
        Ok(policy)
    }

    #[cfg(test)]
    pub fn save_policy(&self, id: &AppId, policy: &AppPolicyV2) -> Result<()> {
        if !self.app_dir(id).is_dir() {
            bail!("app {id} is not stored locally");
        }
        policy.validate()?;
        replace_json(&self.app_dir(id).join(POLICY_FILE), policy)
    }

    pub fn apply_policy_decisions(
        &self,
        id: &AppId,
        decisions: &[(Origin, PermissionKind, PermissionDecision)],
    ) -> Result<AppPolicyV2> {
        self.mutate_policy(id, |policy| {
            for (origin, kind, decision) in decisions {
                policy.set_decision(origin.clone(), *kind, *decision);
            }
        })
    }

    pub fn allow_navigation_origin(&self, id: &AppId, origin: Origin) -> Result<AppPolicyV2> {
        self.mutate_policy(id, |policy| {
            policy.navigation.allowed_origins.insert(origin);
        })
    }

    pub fn merge_policy(
        &self,
        id: &AppId,
        original: &AppPolicyV2,
        edited: &AppPolicyV2,
    ) -> Result<AppPolicyV2> {
        original.validate()?;
        edited.validate()?;
        let mut changes = Vec::new();
        for (origin, permissions) in &original.permissions {
            for kind in permissions.keys() {
                let before = original.decision(origin, *kind);
                let after = edited.decision(origin, *kind);
                if before != after {
                    changes.push((origin.clone(), *kind, after));
                }
            }
        }
        for (origin, permissions) in &edited.permissions {
            for kind in permissions.keys() {
                if !original
                    .permissions
                    .get(origin)
                    .is_some_and(|original| original.contains_key(kind))
                {
                    changes.push((origin.clone(), *kind, edited.decision(origin, *kind)));
                }
            }
        }
        self.mutate_policy(id, |current| {
            for (origin, kind, decision) in changes {
                current.set_decision(origin, kind, decision);
            }
            if original.navigation.enabled != edited.navigation.enabled {
                current.navigation.enabled = edited.navigation.enabled;
            }
            for origin in edited
                .navigation
                .allowed_origins
                .difference(&original.navigation.allowed_origins)
            {
                current.navigation.allowed_origins.insert(origin.clone());
            }
            for origin in original
                .navigation
                .allowed_origins
                .difference(&edited.navigation.allowed_origins)
            {
                current.navigation.allowed_origins.remove(origin);
            }
            if original.proxy != edited.proxy {
                current.proxy = edited.proxy.clone();
            }
            if original.background != edited.background {
                current.background = edited.background.clone();
            }
            for filter_id in original.content_filters.keys() {
                if !edited.content_filters.contains_key(filter_id) {
                    current.content_filters.remove(filter_id);
                }
            }
            for (filter_id, filter) in &edited.content_filters {
                if original.content_filters.get(filter_id) != Some(filter) {
                    current
                        .content_filters
                        .insert(filter_id.clone(), filter.clone());
                }
            }
        })
    }

    pub fn reset_policy(&self, id: &AppId) -> Result<AppPolicyV2> {
        self.mutate_policy(id, AppPolicyV2::reset_permissions)
    }

    fn mutate_policy(
        &self,
        id: &AppId,
        mutate: impl FnOnce(&mut AppPolicyV2),
    ) -> Result<AppPolicyV2> {
        let app_dir = self.app_dir(id);
        if !app_dir.is_dir() {
            bail!("app {id} is not stored locally");
        }
        let _metadata_lock = self.lock_app_file(id, METADATA_LOCK_FILE)?;
        let _policy_lock = self.lock_app_file(id, POLICY_LOCK_FILE)?;

        let policy_path = app_dir.join(POLICY_FILE);
        let mut policy = if policy_path.exists() {
            self.load_policy_from_locked_path(&policy_path)?
        } else {
            AppPolicyV2::default()
        };
        mutate(&mut policy);
        policy.validate()?;
        replace_json(&policy_path, &policy)?;
        Ok(policy)
    }

    #[cfg(test)]
    pub fn create(&self, app: &AppConfigV3, icon: &[u8]) -> Result<()> {
        self.stage_create(app, icon)?.commit()
    }

    pub fn stage_create(&self, app: &AppConfigV3, icon: &[u8]) -> Result<StagedApp> {
        self.stage_create_with_policy(app, icon, &AppPolicyV2::default())
    }

    pub fn stage_create_with_policy(
        &self,
        app: &AppConfigV3,
        icon: &[u8],
        policy: &AppPolicyV2,
    ) -> Result<StagedApp> {
        let final_dir = self.app_dir(&app.id);
        if final_dir.exists() {
            bail!("an app with id {} already exists", app.id);
        }
        policy.validate()?;

        let apps_root = self.apps_root();
        fs::create_dir_all(&apps_root)
            .with_context(|| format!("failed to create {}", apps_root.display()))?;
        let staging = Builder::new()
            .prefix(".tmp-")
            .tempdir_in(&apps_root)
            .context("failed to create app staging directory")?;
        write_json(&staging.path().join(CONFIG_FILE), app)?;
        write_bytes(&staging.path().join(ICON_FILE), icon)?;
        write_json(&staging.path().join(POLICY_FILE), policy)?;
        Ok(StagedApp {
            directory: staging,
            final_path: final_dir,
        })
    }

    pub fn update(&self, app: &AppConfigV3, icon: Option<&[u8]>) -> Result<()> {
        let app_dir = self.app_dir(&app.id);
        if !app_dir.is_dir() {
            bail!("app {} is not stored locally", app.id);
        }
        let _metadata_lock = self.lock_app_file(&app.id, METADATA_LOCK_FILE)?;

        let config_path = app_dir.join(CONFIG_FILE);
        let staged_config = stage_bytes(&config_path, &serialize_json(app)?)?;
        let staged_icon = icon
            .map(|bytes| {
                let path = app_dir.join(ICON_FILE);
                stage_bytes(&path, bytes).map(|temporary| (temporary, path))
            })
            .transpose()?;

        // Commit the icon first so a rejected icon target cannot leave newer
        // metadata pointing at an older icon. AppService restores both files
        // if the second atomic replacement ever fails.
        if let Some((temporary, path)) = staged_icon {
            persist_staged(temporary, &path)?;
        }
        persist_staged(staged_config, &config_path)?;
        sync_parent(&config_path)
    }

    #[cfg(test)]
    pub fn delete(&self, id: &AppId) -> Result<()> {
        let profile_lock = self.acquire_delete_profile_lock(id)?;
        self.delete_with_profile_lock(id, profile_lock)
    }

    pub fn acquire_delete_profile_lock(&self, id: &AppId) -> Result<ProfileLock> {
        self.try_acquire_profile_snapshot_lock(id)
    }

    pub fn delete_with_profile_lock(&self, id: &AppId, profile_lock: ProfileLock) -> Result<()> {
        let _metadata_lock = self
            .app_dir(id)
            .is_dir()
            .then(|| self.lock_app_file(id, METADATA_LOCK_FILE))
            .transpose()?;
        let _policy_lock = self
            .app_dir(id)
            .is_dir()
            .then(|| self.lock_app_file(id, POLICY_LOCK_FILE))
            .transpose()?;
        let targets = [self.app_dir(id), self.profile_dir(id), self.cache_dir(id)];
        for target in targets {
            if target.exists() {
                fs::remove_dir_all(&target)
                    .with_context(|| format!("failed to remove {}", target.display()))?;
            }
        }
        drop(profile_lock);
        Ok(())
    }

    pub fn stage_profile_from(&self, id: &AppId, source: &Path) -> Result<StagedProfile> {
        ensure_profile_source(source)?;
        let destination = self.profile_dir(id);
        if destination.exists() {
            bail!("profile target {} already exists", destination.display());
        }
        let profiles_root = destination
            .parent()
            .context("profile target has no parent")?;
        fs::create_dir_all(profiles_root)
            .with_context(|| format!("failed to create {}", profiles_root.display()))?;
        let staging = Builder::new()
            .prefix(".tmp-profile-")
            .tempdir_in(profiles_root)
            .context("failed to create profile staging directory")?;
        if source.is_dir() {
            copy_regular_tree(source, staging.path())?;
        } else {
            fs::File::open(staging.path())?.sync_all()?;
        }
        Ok(StagedProfile {
            directory: staging,
            final_path: destination,
        })
    }

    pub fn remove_profile(&self, id: &AppId) -> Result<()> {
        let profile = self.profile_dir(id);
        if profile.exists() {
            fs::remove_dir_all(&profile)
                .with_context(|| format!("failed to remove {}", profile.display()))?;
            sync_parent(&profile)?;
        }
        Ok(())
    }

    pub fn remove_profile_with_lock(&self, id: &AppId, profile_lock: ProfileLock) -> Result<()> {
        let result = self.remove_profile(id);
        drop(profile_lock);
        result
    }

    fn lock_app_file(&self, id: &AppId, name: &str) -> Result<fs::File> {
        let path = self.app_dir(id).join(name);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)
            .with_context(|| format!("failed to lock {}", path.display()))?;
        Ok(file)
    }
}

fn ensure_profile_source(source: &Path) -> Result<()> {
    if source.exists() && !source.is_dir() {
        bail!("profile source {} is not a directory", source.display());
    }
    Ok(())
}

fn validate_companion_token(token: &str) -> Result<()> {
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("invalid Chromium companion capability token");
    }
    Ok(())
}

fn copy_regular_tree(source: &Path, destination: &Path) -> Result<()> {
    let mut entries = fs::read_dir(source)
        .with_context(|| format!("failed to read {}", source.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        if name == RUNTIME_LOCK_FILE {
            continue;
        }
        let target = destination.join(&name);
        if file_type.is_dir() {
            fs::create_dir(&target)
                .with_context(|| format!("failed to create {}", target.display()))?;
            copy_regular_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)
                .with_context(|| format!("failed to copy {}", entry.path().display()))?;
            fs::File::open(&target)
                .with_context(|| format!("failed to open {} for syncing", target.display()))?
                .sync_all()
                .with_context(|| format!("failed to sync {}", target.display()))?;
        } else {
            bail!(
                "profile contains an unsupported file: {}",
                entry.path().display()
            );
        }
    }
    fs::File::open(destination)
        .with_context(|| format!("failed to open {} for syncing", destination.display()))?
        .sync_all()
        .with_context(|| format!("failed to sync {}", destination.display()))
}

fn serialize_json(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write_bytes(path, &serialize_json(value)?)
}

fn replace_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = stage_bytes(path, &serialize_json(value)?)?;
    persist_staged(temporary, path)?;
    sync_parent(path)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file =
        fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))
}

fn stage_bytes(path: &Path, bytes: &[u8]) -> Result<NamedTempFile> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create a temporary file in {}", parent.display()))?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    Ok(temporary)
}

fn persist_staged(temporary: NamedTempFile, path: &Path) -> Result<()> {
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to atomically replace {}", path.display()))?;
    Ok(())
}

fn sync_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("failed to sync {}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::SCHEMA_VERSION,
        policy::{Origin, PermissionDecision, PermissionKind, POLICY_SCHEMA_VERSION},
    };
    use std::str::FromStr;

    fn repository() -> (tempfile::TempDir, AppRepository) {
        let temp = tempfile::tempdir().unwrap();
        let repository = AppRepository::new(temp.path().join("data"), temp.path().join("cache"));
        (temp, repository)
    }

    #[test]
    fn create_load_update_and_delete() {
        let (_temp, repository) = repository();
        let mut app = AppConfigV3::new("Example", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();
        assert_eq!(repository.load(&app.id).unwrap(), app);
        assert_eq!(repository.read_icon(&app.id).unwrap(), b"icon");
        assert_eq!(
            repository.load_policy(&app.id).unwrap(),
            AppPolicyV2::default()
        );

        app.title = "Updated".to_owned();
        repository.update(&app, Some(b"new-icon")).unwrap();
        assert_eq!(repository.load(&app.id).unwrap().title, "Updated");
        assert_eq!(repository.read_icon(&app.id).unwrap(), b"new-icon");

        repository.delete(&app.id).unwrap();
        assert!(!repository.contains(&app.id));
    }

    #[test]
    fn corrupt_and_future_configs_are_reported_not_deleted() {
        let (_temp, repository) = repository();
        let bad_dir = repository.apps_root().join("abcdefghijkl");
        fs::create_dir_all(&bad_dir).unwrap();
        fs::write(
            bad_dir.join(CONFIG_FILE),
            include_str!("../tests/fixtures/malformed-app.json"),
        )
        .unwrap();

        let future_dir = repository.apps_root().join("mnopqrstuvwx");
        fs::create_dir_all(&future_dir).unwrap();
        let future = format!(
            r#"{{"schema_version":{},"id":"mnopqrstuvwx","title":"Future","start_url":"https://example.org","user_agent":null,"use_theme_color":true,"window":{{"width":800,"height":600,"maximized":false}},"sort_order":0}}"#,
            SCHEMA_VERSION + 1
        );
        fs::write(future_dir.join(CONFIG_FILE), &future).unwrap();

        let report = repository.list().unwrap();
        assert!(report.apps.is_empty());
        assert_eq!(report.warnings.len(), 2);
        assert!(bad_dir.exists());
        assert!(future_dir.exists());
        assert_eq!(
            fs::read_to_string(future_dir.join(CONFIG_FILE)).unwrap(),
            future
        );
    }

    #[test]
    fn version_one_migration_removes_provenance_and_preserves_app_data() {
        let (_temp, repository) = repository();
        let id = AppId::from_str("abcdefghijkl").unwrap();
        let app_dir = repository.app_dir(&id);
        let profile_dir = repository.profile_dir(&id);
        let cache_dir = repository.cache_dir(&id);
        fs::create_dir_all(&app_dir).unwrap();
        fs::create_dir_all(&profile_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(app_dir.join(ICON_FILE), b"icon").unwrap();
        fs::write(profile_dir.join("profile-state"), b"profile").unwrap();
        fs::write(cache_dir.join("cache-state"), b"cache").unwrap();
        fs::write(
            app_dir.join(CONFIG_FILE),
            r#"{
  "schema_version": 1,
  "id": "abcdefghijkl",
  "title": "Imported before v0.2",
  "start_url": "https://example.org/",
  "user_agent": null,
  "use_theme_color": true,
  "window": { "width": 900, "height": 700, "maximized": false },
  "sort_order": 1,
  "imported_from": {
    "app_id": "io.github.zaedus.spider",
    "legacy_id": "old-app"
  }
}"#,
        )
        .unwrap();

        let app = repository.load(&id).unwrap();
        assert_eq!(app.schema_version, SCHEMA_VERSION);
        assert_eq!(app.title, "Imported before v0.2");
        let migrated = fs::read_to_string(app_dir.join(CONFIG_FILE)).unwrap();
        assert!(!migrated.contains("imported_from"));
        assert!(migrated.contains("\"schema_version\": 3"));
        assert!(migrated.contains("\"engine\": \"webkit\""));
        assert_eq!(fs::read(app_dir.join(ICON_FILE)).unwrap(), b"icon");
        assert_eq!(
            fs::read(profile_dir.join("profile-state")).unwrap(),
            b"profile"
        );
        assert_eq!(fs::read(cache_dir.join("cache-state")).unwrap(), b"cache");
        assert_eq!(repository.load_policy(&id).unwrap(), AppPolicyV2::default());
    }

    #[test]
    fn version_two_migration_adds_webkit_without_touching_profile_data() {
        let (_temp, repository) = repository();
        let id = AppId::from_str("abcdefghijkl").unwrap();
        let app_dir = repository.app_dir(&id);
        let profile_dir = repository.profile_dir(&id);
        fs::create_dir_all(&app_dir).unwrap();
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(app_dir.join(ICON_FILE), b"icon").unwrap();
        fs::write(profile_dir.join("profile-state"), b"webkit profile").unwrap();
        fs::write(
            app_dir.join(CONFIG_FILE),
            r#"{
  "schema_version": 2,
  "id": "abcdefghijkl",
  "title": "Bastle v2",
  "start_url": "https://example.org/",
  "user_agent": null,
  "use_theme_color": true,
  "window": { "width": 900, "height": 700, "maximized": false },
  "sort_order": 1
}"#,
        )
        .unwrap();

        let app = repository.load(&id).unwrap();
        assert_eq!(app.schema_version, SCHEMA_VERSION);
        assert_eq!(app.engine, crate::model::Engine::WebKit);
        assert_eq!(
            fs::read(profile_dir.join("profile-state")).unwrap(),
            b"webkit profile"
        );
        let migrated = fs::read_to_string(app_dir.join(CONFIG_FILE)).unwrap();
        assert!(migrated.contains("\"schema_version\": 3"));
        assert!(migrated.contains("\"engine\": \"webkit\""));
    }

    #[test]
    fn companion_token_is_stable_and_invalid_tokens_are_rejected() {
        let (_temp, repository) = repository();
        let app = AppConfigV3::new("Chromium", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();

        let token = repository.companion_token(&app.id).unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(repository.companion_token(&app.id).unwrap(), token);

        fs::write(
            repository.app_dir(&app.id).join(COMPANION_TOKEN_FILE),
            "../invalid",
        )
        .unwrap();
        assert!(repository.companion_token(&app.id).is_err());
    }

    #[test]
    fn policy_writes_are_atomic_and_invalid_policy_does_not_hide_app() {
        let (_temp, repository) = repository();
        let app = AppConfigV3::new("Example", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();

        let origin = Origin::from_str("https://example.org/path").unwrap();
        let mut policy = AppPolicyV2::default();
        policy.set_decision(
            origin.clone(),
            PermissionKind::Camera,
            PermissionDecision::Allow,
        );
        repository.save_policy(&app.id, &policy).unwrap();
        assert_eq!(repository.load_policy(&app.id).unwrap(), policy);

        let policy_path = repository.app_dir(&app.id).join(POLICY_FILE);
        let future = format!(
            r#"{{"schema_version":{},"permissions":{{}}}}"#,
            POLICY_SCHEMA_VERSION + 1
        );
        fs::write(&policy_path, &future).unwrap();
        let report = repository.list().unwrap();
        assert_eq!(report.apps, vec![app]);
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(fs::read_to_string(policy_path).unwrap(), future);
    }

    #[test]
    fn policy_v1_is_atomically_migrated_to_v2() {
        let (_temp, repository) = repository();
        let app = AppConfigV3::new("Example", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();
        let policy_path = repository.app_dir(&app.id).join(POLICY_FILE);
        fs::write(
            &policy_path,
            r#"{
  "schema_version": 1,
  "permissions": {
    "https://example.org": { "camera": "block" }
  }
}"#,
        )
        .unwrap();

        let policy = repository.load_policy(&app.id).unwrap();
        assert_eq!(policy.schema_version, POLICY_SCHEMA_VERSION);
        assert_eq!(
            policy.decision(
                &Origin::from_str("https://example.org").unwrap(),
                PermissionKind::Camera
            ),
            PermissionDecision::Block
        );
        assert!(!policy.navigation.enabled);
        assert!(!policy.background.enabled);
        let migrated = fs::read_to_string(policy_path).unwrap();
        assert!(migrated.contains("\"schema_version\": 2"));
        assert!(!migrated.contains(".tmp-"));
    }

    #[test]
    fn policy_migration_waits_for_the_policy_lock() {
        let (_temp, repository) = repository();
        let app = AppConfigV3::new("Example", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();
        fs::write(
            repository.app_dir(&app.id).join(POLICY_FILE),
            r#"{"schema_version":1,"permissions":{}}"#,
        )
        .unwrap();

        let policy_lock = repository.lock_app_file(&app.id, POLICY_LOCK_FILE).unwrap();
        let repository_for_thread = repository.clone();
        let id = app.id.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = sender.send(repository_for_thread.load_policy(&id));
        });
        assert!(matches!(
            receiver.recv_timeout(std::time::Duration::from_millis(100)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(policy_lock);
        let migrated = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap();
        thread.join().unwrap();
        assert_eq!(migrated.schema_version, POLICY_SCHEMA_VERSION);
    }

    #[test]
    fn background_reconciliation_lock_serializes_processes() {
        let (_temp, repository) = repository();
        let first_lock = repository.lock_background().unwrap();
        let repository_for_thread = repository.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = sender.send(repository_for_thread.lock_background());
        });
        assert!(matches!(
            receiver.recv_timeout(std::time::Duration::from_millis(100)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(first_lock);
        receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn policy_edits_merge_with_decisions_saved_by_another_process() {
        let (_temp, repository) = repository();
        let app = AppConfigV3::new("Example", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();
        let origin = Origin::from_str("https://example.org").unwrap();
        let removed_origin = Origin::from_str("https://removed.example").unwrap();
        repository
            .allow_navigation_origin(&app.id, removed_origin.clone())
            .unwrap();
        repository
            .mutate_policy(&app.id, |policy| {
                policy.content_filters.insert(
                    "removed00000".to_owned(),
                    crate::policy::ContentFilterRuleSet::new(
                        "Removed by editor",
                        serde_json::json!([]),
                    )
                    .unwrap(),
                );
            })
            .unwrap();
        let editor_snapshot = repository.load_policy(&app.id).unwrap();
        let mut editor_changes = editor_snapshot.clone();
        editor_changes.set_decision(
            origin.clone(),
            PermissionKind::Notifications,
            PermissionDecision::Allow,
        );
        editor_changes.navigation.enabled = true;
        editor_changes
            .navigation
            .allowed_origins
            .insert(origin.clone());
        editor_changes
            .navigation
            .allowed_origins
            .remove(&removed_origin);
        editor_changes.content_filters.remove("removed00000");
        editor_changes.content_filters.insert(
            "editor000000".to_owned(),
            crate::policy::ContentFilterRuleSet::new("Added by editor", serde_json::json!([]))
                .unwrap(),
        );

        let concurrent_origin = Origin::from_str("https://concurrent.example").unwrap();

        repository
            .apply_policy_decisions(
                &app.id,
                &[(
                    origin.clone(),
                    PermissionKind::Camera,
                    PermissionDecision::Block,
                )],
            )
            .unwrap();
        repository
            .allow_navigation_origin(&app.id, concurrent_origin.clone())
            .unwrap();
        repository
            .mutate_policy(&app.id, |policy| {
                policy.content_filters.insert(
                    "parallel0000".to_owned(),
                    crate::policy::ContentFilterRuleSet::new(
                        "Added concurrently",
                        serde_json::json!([]),
                    )
                    .unwrap(),
                );
            })
            .unwrap();
        let merged = repository
            .merge_policy(&app.id, &editor_snapshot, &editor_changes)
            .unwrap();

        assert_eq!(
            merged.decision(&origin, PermissionKind::Camera),
            PermissionDecision::Block
        );
        assert_eq!(
            merged.decision(&origin, PermissionKind::Notifications),
            PermissionDecision::Allow
        );
        assert!(merged.navigation.enabled);
        assert!(merged.navigation.allowed_origins.contains(&origin));
        assert!(merged
            .navigation
            .allowed_origins
            .contains(&concurrent_origin));
        assert!(!merged.navigation.allowed_origins.contains(&removed_origin));
        assert!(!merged.content_filters.contains_key("removed00000"));
        assert!(merged.content_filters.contains_key("editor000000"));
        assert!(merged.content_filters.contains_key("parallel0000"));
        assert!(repository.app_dir(&app.id).join(POLICY_LOCK_FILE).is_file());
    }

    #[test]
    fn metadata_snapshot_captures_config_icon_and_policy_together() {
        let (_temp, repository) = repository();
        let app = AppConfigV3::new("Snapshot", "example.org", 0).unwrap();
        repository.create(&app, b"snapshot-icon").unwrap();
        let origin = Origin::from_str("https://example.org").unwrap();
        repository
            .apply_policy_decisions(
                &app.id,
                &[(
                    origin.clone(),
                    PermissionKind::Camera,
                    PermissionDecision::Block,
                )],
            )
            .unwrap();

        let snapshot = repository.snapshot(&app.id).unwrap();
        assert_eq!(snapshot.config, app);
        assert_eq!(snapshot.icon, b"snapshot-icon");
        assert_eq!(
            snapshot.policy.decision(&origin, PermissionKind::Camera),
            PermissionDecision::Block
        );
        assert!(repository
            .app_dir(&app.id)
            .join(METADATA_LOCK_FILE)
            .is_file());
    }

    #[test]
    fn missing_icon_is_reported_without_removing_config() {
        let (_temp, repository) = repository();
        let app = AppConfigV3::new("Example", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();
        fs::remove_file(repository.app_dir(&app.id).join(ICON_FILE)).unwrap();
        assert!(repository.read_icon(&app.id).is_err());
        assert!(repository.contains(&app.id));
    }

    #[test]
    fn rejected_icon_update_does_not_commit_new_config() {
        let (_temp, repository) = repository();
        let mut app = AppConfigV3::new("Before", "example.org", 0).unwrap();
        repository.create(&app, b"old-icon").unwrap();
        let icon_path = repository.app_dir(&app.id).join(ICON_FILE);
        fs::remove_file(&icon_path).unwrap();
        fs::create_dir(&icon_path).unwrap();

        app.title = "After".to_owned();
        assert!(repository.update(&app, Some(b"new-icon")).is_err());
        assert_eq!(repository.load(&app.id).unwrap().title, "Before");
    }
}
