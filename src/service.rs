// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use ashpd::WindowIdentifier;
use futures::channel::oneshot;

use crate::{
    background::{BackgroundBackend, PortalBackground},
    launcher::{LauncherBackend, PortalLauncher, UninstallOutcome},
    model::{AppConfigV2, AppId, WindowState},
    policy::{AppPolicyV2, Origin, PermissionDecision, PermissionKind},
    repository::{
        AppRepository, AppSnapshot, BackgroundLock, LoadReport, ProfileLock, StagedProfile,
    },
};

#[derive(Debug, Clone)]
pub struct AppService<L, B = PortalBackground> {
    repository: AppRepository,
    launcher: L,
    background: B,
}

impl<L: LauncherBackend> AppService<L, PortalBackground> {
    pub fn new(repository: AppRepository, launcher: L) -> Self {
        Self::with_background(repository, launcher, PortalBackground)
    }
}

impl<L: LauncherBackend, B: BackgroundBackend> AppService<L, B> {
    pub fn with_background(repository: AppRepository, launcher: L, background: B) -> Self {
        Self {
            repository,
            launcher,
            background,
        }
    }

    #[cfg(test)]
    pub fn repository(&self) -> &AppRepository {
        &self.repository
    }

    pub fn list(&self) -> Result<LoadReport> {
        self.repository.list()
    }

    pub fn load(&self, id: &AppId) -> Result<AppConfigV2> {
        self.repository.load(id)
    }

    pub fn read_icon(&self, id: &AppId) -> Result<Vec<u8>> {
        self.repository.read_icon(id)
    }

    pub fn snapshot(&self, id: &AppId) -> Result<AppSnapshot> {
        self.repository.snapshot(id)
    }

    pub fn contains(&self, id: &AppId) -> bool {
        self.repository.contains(id)
    }

    pub fn profile_dir(&self, id: &AppId) -> PathBuf {
        self.repository.profile_dir(id)
    }

    pub fn cache_dir(&self, id: &AppId) -> PathBuf {
        self.repository.cache_dir(id)
    }

    pub fn contains_any_data(&self, id: &AppId) -> bool {
        self.repository.contains_any_data(id)
    }

    pub fn acquire_runtime_lock(&self, id: &AppId) -> Result<ProfileLock> {
        self.repository.acquire_runtime_lock(id)
    }

    pub fn try_acquire_profile_snapshot_lock(&self, id: &AppId) -> Result<ProfileLock> {
        self.repository.try_acquire_profile_snapshot_lock(id)
    }

    async fn stage_profile_from(
        &self,
        id: &AppId,
        source: &std::path::Path,
    ) -> Result<StagedProfile> {
        let repository = self.repository.clone();
        let id = id.clone();
        let source = source.to_path_buf();
        let (sender, receiver) = oneshot::channel();
        std::thread::Builder::new()
            .name("bastle-profile-restore".to_owned())
            .spawn(move || {
                let _ = sender.send(repository.stage_profile_from(&id, &source));
            })
            .context("failed to start the profile restore worker")?;
        receiver
            .await
            .map_err(|_| anyhow!("the profile restore worker stopped unexpectedly"))?
    }

    async fn lock_background(&self) -> Result<BackgroundLock> {
        let repository = self.repository.clone();
        let (sender, receiver) = oneshot::channel();
        std::thread::Builder::new()
            .name("bastle-background-lock".to_owned())
            .spawn(move || {
                let _ = sender.send(repository.lock_background());
            })
            .context("failed to start the background lock worker")?;
        receiver
            .await
            .map_err(|_| anyhow!("the background lock worker stopped unexpectedly"))?
    }

    pub fn save_runtime_state(&self, id: &AppId, window: WindowState) -> Result<()> {
        let mut current = self.repository.load(id)?;
        current.window = window;
        self.repository.update(&current, None)
    }

    pub fn load_policy(&self, id: &AppId) -> Result<AppPolicyV2> {
        self.repository.load_policy(id)
    }

    pub fn apply_policy_decisions(
        &self,
        id: &AppId,
        decisions: &[(Origin, PermissionKind, PermissionDecision)],
    ) -> Result<AppPolicyV2> {
        self.repository.apply_policy_decisions(id, decisions)
    }

    pub fn allow_navigation_origin(&self, id: &AppId, origin: Origin) -> Result<AppPolicyV2> {
        self.repository.allow_navigation_origin(id, origin)
    }

    pub fn merge_policy(
        &self,
        id: &AppId,
        original: &AppPolicyV2,
        edited: &AppPolicyV2,
    ) -> Result<AppPolicyV2> {
        self.repository.merge_policy(id, original, edited)
    }

    pub fn reset_policy(&self, id: &AppId) -> Result<AppPolicyV2> {
        self.repository.reset_policy(id)
    }

    pub async fn merge_policy_with_background(
        &self,
        id: &AppId,
        original: &AppPolicyV2,
        edited: &AppPolicyV2,
        parent: Option<&WindowIdentifier>,
        reason: &str,
    ) -> Result<()> {
        if edited.background == original.background {
            self.repository.merge_policy(id, original, edited)?;
            return Ok(());
        }
        let _background_lock = self.lock_background().await?;
        let current = self.repository.load_policy(id)?;
        let other_autostart = self.another_app_uses_autostart(id)?;
        let previous_global_autostart = current.background.autostart || other_autostart;
        let mut effective = edited.clone();
        let mut portal_changed = false;

        if effective.background.enabled {
            let requested_for_this_app = effective.background.autostart;
            let requested_global_autostart = requested_for_this_app || other_autostart;
            let grant = self
                .background
                .request_access(parent, reason, requested_global_autostart)
                .await?;
            portal_changed = true;
            if requested_global_autostart && !grant.autostart {
                let error = anyhow!("the desktop did not grant required global autostart");
                return match self
                    .background
                    .update_autostart(parent, previous_global_autostart)
                    .await
                {
                    Ok(_) => Err(error).context("the previous portal autostart state was restored"),
                    Err(rollback) => Err(error).context(format!(
                        "portal autostart rollback also failed: {rollback:#}"
                    )),
                };
            }
            effective.background.enabled = grant.background;
            effective.background.autostart = requested_for_this_app && grant.autostart;
        } else if current.background.autostart {
            self.background
                .update_autostart(parent, other_autostart)
                .await?;
            portal_changed = true;
        }

        match self.repository.merge_policy(id, original, &effective) {
            Ok(_) => Ok(()),
            Err(error) if portal_changed => {
                match self
                    .background
                    .update_autostart(parent, previous_global_autostart)
                    .await
                {
                    Ok(_) => Err(error).context("the previous portal autostart state was restored"),
                    Err(rollback) => Err(error).context(format!(
                        "portal autostart rollback also failed: {rollback:#}"
                    )),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn another_app_uses_autostart(&self, excluded: &AppId) -> Result<bool> {
        for app in self.repository.list()?.apps {
            if app.id != *excluded && self.repository.load_policy(&app.id)?.background.autostart {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn another_app_may_use_autostart(&self, excluded: &AppId) -> Result<bool> {
        for app in self.repository.list()?.apps {
            if app.id == *excluded {
                continue;
            }
            match self.repository.load_policy(&app.id) {
                Ok(policy) if policy.background.autostart => return Ok(true),
                Ok(_) => {}
                Err(_) => return Ok(true),
            }
        }
        Ok(false)
    }

    pub async fn create(
        &self,
        mut app: AppConfigV2,
        icon: &[u8],
        parent: Option<&WindowIdentifier>,
    ) -> Result<AppConfigV2> {
        app.normalize_and_validate()?;
        let staged = self.repository.stage_create(&app, icon)?;
        self.launcher
            .install(&app, icon, parent)
            .await
            .context("local files were not committed")?;
        if let Err(error) = staged.commit() {
            let rollback = self.launcher.uninstall(&app.id).await;
            return match rollback {
                Ok(_) => Err(error),
                Err(rollback_error) => {
                    Err(error.context(format!("launcher rollback also failed: {rollback_error}")))
                }
            };
        }
        Ok(app)
    }

    pub async fn create_from_backup(
        &self,
        mut app: AppConfigV2,
        icon: &[u8],
        policy: &AppPolicyV2,
        profile_source: Option<&std::path::Path>,
        parent: Option<&WindowIdentifier>,
    ) -> Result<AppConfigV2> {
        app.normalize_and_validate()?;
        let policy = policy.for_restore();
        policy.validate()?;
        let staged_app = self
            .repository
            .stage_create_with_policy(&app, icon, &policy)?;
        let staged_profile = match profile_source {
            Some(source) => Some(self.stage_profile_from(&app.id, source).await?),
            None => None,
        };

        let profile_committed = if let Some(profile) = staged_profile {
            profile.commit()?;
            true
        } else {
            false
        };

        if let Err(error) = self.launcher.install(&app, icon, parent).await {
            return if profile_committed {
                match self.repository.remove_profile(&app.id) {
                    Ok(()) => Err(error).context("the staged profile was rolled back"),
                    Err(cleanup) => {
                        Err(error).context(format!("profile rollback also failed: {cleanup}"))
                    }
                }
            } else {
                Err(error)
            };
        }

        if let Err(error) = staged_app.commit() {
            let mut rollback_failures = Vec::new();
            if let Err(rollback) = self.launcher.uninstall(&app.id).await {
                rollback_failures.push(format!("launcher rollback failed: {rollback}"));
            }
            if profile_committed {
                if let Err(rollback) = self.repository.remove_profile(&app.id) {
                    rollback_failures.push(format!("profile rollback failed: {rollback}"));
                }
            }
            return if rollback_failures.is_empty() {
                Err(error).context("the launcher and profile were rolled back")
            } else {
                Err(error).context(rollback_failures.join("; "))
            };
        }
        Ok(app)
    }

    pub async fn update(
        &self,
        mut app: AppConfigV2,
        icon: Option<&[u8]>,
        parent: Option<&WindowIdentifier>,
    ) -> Result<AppConfigV2> {
        app.normalize_and_validate()?;
        let previous = self.repository.load(&app.id)?;
        let previous_icon = self.repository.read_icon(&app.id)?;
        let current_icon = icon.unwrap_or(&previous_icon);
        self.launcher.install(&app, current_icon, parent).await?;
        if let Err(error) = self.repository.update(&app, icon) {
            let mut rollback_failures = Vec::new();
            if let Err(rollback_error) = self.repository.update(&previous, Some(&previous_icon)) {
                rollback_failures.push(format!("local rollback failed: {rollback_error}"));
            }
            if let Err(rollback_error) = self
                .launcher
                .install(&previous, &previous_icon, parent)
                .await
            {
                rollback_failures.push(format!("launcher rollback failed: {rollback_error}"));
            }
            let context = if rollback_failures.is_empty() {
                "the previous local data and launcher were restored".to_owned()
            } else {
                rollback_failures.join("; ")
            };
            return Err(error).context(context);
        }
        Ok(app)
    }

    pub async fn delete(&self, id: &AppId) -> Result<UninstallOutcome> {
        let profile_existed = self.repository.profile_dir(id).exists();
        let profile_lock = self.repository.acquire_delete_profile_lock(id)?;
        let _background_lock = self.lock_background().await?;
        let target_may_use_autostart = self
            .repository
            .load_policy(id)
            .map(|policy| policy.background.autostart)
            .unwrap_or(true);
        let disable_portal_autostart =
            target_may_use_autostart && !self.another_app_may_use_autostart(id)?;
        if disable_portal_autostart {
            self.background.update_autostart(None, false).await?;
        }

        let result = async {
            let outcome = match self.launcher.uninstall(id).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    if !profile_existed {
                        return match self.repository.remove_profile_with_lock(id, profile_lock) {
                            Ok(()) => Err(error),
                            Err(cleanup) => Err(error).context(format!(
                                "temporary profile-lock cleanup also failed: {cleanup}"
                            )),
                        };
                    }
                    drop(profile_lock);
                    return Err(error);
                }
            };
            self.repository
                .delete_with_profile_lock(id, profile_lock)
                .context("launcher was removed but local data cleanup failed")?;
            Ok(outcome)
        }
        .await;

        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) if disable_portal_autostart => {
                match self.background.update_autostart(None, true).await {
                    Ok(_) => Err(error).context("the previous portal autostart state was restored"),
                    Err(rollback) => Err(error).context(format!(
                        "portal autostart rollback also failed: {rollback:#}"
                    )),
                }
            }
            Err(error) => Err(error),
        }
    }

    pub async fn repair(&self, id: &AppId, parent: Option<&WindowIdentifier>) -> Result<()> {
        let app = self.repository.load(id)?;
        let icon = self.repository.read_icon(id)?;
        self.launcher.install(&app, &icon, parent).await
    }
}

impl AppService<PortalLauncher, PortalBackground> {
    pub fn portal() -> Self {
        Self::new(AppRepository::for_current_user(), PortalLauncher)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::{HashMap, HashSet},
        rc::Rc,
        str::FromStr,
    };

    use anyhow::{bail, Result};
    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;
    use crate::{
        background::BackgroundGrant,
        policy::{Origin, PermissionDecision, PermissionKind},
    };

    #[derive(Debug, Clone, Default)]
    struct FakeLauncher {
        installed: Rc<RefCell<HashSet<AppId>>>,
        deny_install: Rc<RefCell<bool>>,
        deny_uninstall: Rc<RefCell<bool>>,
        required_profiles: Rc<RefCell<HashMap<AppId, PathBuf>>>,
    }

    #[async_trait(?Send)]
    impl LauncherBackend for FakeLauncher {
        async fn install(
            &self,
            app: &AppConfigV2,
            _icon: &[u8],
            _parent: Option<&WindowIdentifier>,
        ) -> Result<()> {
            if *self.deny_install.borrow() {
                bail!("portal denied installation");
            }
            if let Some(profile) = self.required_profiles.borrow().get(&app.id) {
                if !profile.join("profile-state").is_file() {
                    bail!("profile was not committed before launcher installation");
                }
            }
            self.installed.borrow_mut().insert(app.id.clone());
            Ok(())
        }

        async fn uninstall(&self, id: &AppId) -> Result<UninstallOutcome> {
            if *self.deny_uninstall.borrow() {
                bail!("portal denied removal");
            }
            Ok(if self.installed.borrow_mut().remove(id) {
                UninstallOutcome::Removed
            } else {
                UninstallOutcome::AlreadyMissing
            })
        }
    }

    #[derive(Debug, Clone, Default)]
    struct FakeBackground {
        autostart: Rc<Cell<bool>>,
        updates: Rc<RefCell<Vec<bool>>>,
        deny_autostart: Rc<Cell<bool>>,
        remove_on_request: Rc<RefCell<Option<(AppRepository, AppId)>>>,
    }

    impl FakeBackground {
        fn apply(&self, enabled: bool) -> Result<()> {
            self.updates.borrow_mut().push(enabled);
            self.autostart.set(enabled);
            if let Some((repository, id)) = self.remove_on_request.borrow_mut().take() {
                repository.delete(&id)?;
            }
            Ok(())
        }
    }

    #[async_trait(?Send)]
    impl BackgroundBackend for FakeBackground {
        async fn request_access(
            &self,
            _parent: Option<&WindowIdentifier>,
            _reason: &str,
            autostart: bool,
        ) -> Result<BackgroundGrant> {
            let autostart = autostart && !self.deny_autostart.get();
            self.apply(autostart)?;
            Ok(BackgroundGrant {
                background: true,
                autostart,
            })
        }

        async fn update_autostart(
            &self,
            _parent: Option<&WindowIdentifier>,
            enabled: bool,
        ) -> Result<bool> {
            self.apply(enabled)?;
            Ok(enabled)
        }
    }

    fn service() -> (tempfile::TempDir, AppService<FakeLauncher>, FakeLauncher) {
        let temp = tempfile::tempdir().unwrap();
        let repository = AppRepository::new(temp.path().join("data"), temp.path().join("cache"));
        let launcher = FakeLauncher::default();
        let service = AppService::new(repository, launcher.clone());
        (temp, service, launcher)
    }

    fn service_with_background() -> (
        tempfile::TempDir,
        AppService<FakeLauncher, FakeBackground>,
        FakeLauncher,
        FakeBackground,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let repository = AppRepository::new(temp.path().join("data"), temp.path().join("cache"));
        let launcher = FakeLauncher::default();
        let background = FakeBackground::default();
        let service = AppService::with_background(repository, launcher.clone(), background.clone());
        (temp, service, launcher, background)
    }

    #[test]
    fn full_crud_and_missing_launcher() {
        let (_temp, service, launcher) = service();
        let app = AppConfigV2::new("Example", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        assert!(service.repository().contains(&app.id));
        assert!(launcher.installed.borrow().contains(&app.id));

        let mut edited = app.clone();
        edited.title = "Edited".to_owned();
        block_on(service.update(edited.clone(), None, None)).unwrap();
        assert_eq!(service.repository().load(&app.id).unwrap(), edited);

        launcher.installed.borrow_mut().remove(&app.id);
        let result = block_on(service.delete(&app.id)).unwrap();
        assert_eq!(result, UninstallOutcome::AlreadyMissing);
        assert!(!service.repository().contains(&app.id));
    }

    #[test]
    fn portal_denial_rolls_back_staged_create() {
        let (_temp, service, launcher) = service();
        *launcher.deny_install.borrow_mut() = true;
        let app = AppConfigV2::new("Example", "example.org", 0).unwrap();
        assert!(block_on(service.create(app.clone(), b"icon", None)).is_err());
        assert!(!service.repository().contains(&app.id));
        assert!(!launcher.installed.borrow().contains(&app.id));
    }

    #[test]
    fn portal_error_preserves_local_data_on_delete() {
        let (_temp, service, launcher) = service();
        let app = AppConfigV2::new("Example", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        *launcher.deny_uninstall.borrow_mut() = true;
        assert!(block_on(service.delete(&app.id)).is_err());
        assert!(service.repository().contains(&app.id));
    }

    #[test]
    fn active_profile_blocks_delete_before_launcher_removal() {
        let (_temp, service, launcher) = service();
        let app = AppConfigV2::new("Running", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        let runtime_lock = service.acquire_runtime_lock(&app.id).unwrap();

        assert!(block_on(service.delete(&app.id)).is_err());
        assert!(launcher.installed.borrow().contains(&app.id));
        assert!(service.contains(&app.id));

        drop(runtime_lock);
        assert!(block_on(service.delete(&app.id)).is_ok());
        assert!(!launcher.installed.borrow().contains(&app.id));
    }

    #[test]
    fn deletion_lock_rejects_new_runtime_without_queueing() {
        let (_temp, service, _launcher) = service();
        let app = AppConfigV2::new("Deleting", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        let deletion_lock = service
            .repository()
            .acquire_delete_profile_lock(&app.id)
            .unwrap();

        let error = service.acquire_runtime_lock(&app.id).unwrap_err();
        assert!(error
            .to_string()
            .contains("profile is temporarily unavailable"));

        drop(deletion_lock);
        assert!(service.acquire_runtime_lock(&app.id).is_ok());
    }

    #[test]
    fn runtime_state_is_merged_into_the_latest_config() {
        let (_temp, service, _launcher) = service();
        let app = AppConfigV2::new("Before", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();

        let mut edited = app.clone();
        edited.title = "After".to_owned();
        service.repository().update(&edited, None).unwrap();
        let window = WindowState {
            width: 1440,
            height: 900,
            maximized: true,
        };
        service.save_runtime_state(&app.id, window.clone()).unwrap();

        let stored = service.repository().load(&app.id).unwrap();
        assert_eq!(stored.title, "After");
        assert_eq!(stored.window, window);
    }

    #[test]
    fn permission_policy_is_persisted_through_the_service() {
        let (_temp, service, _launcher) = service();
        let app = AppConfigV2::new("Example", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();

        let origin = Origin::from_str("https://example.org/path").unwrap();
        let mut policy = service.load_policy(&app.id).unwrap();
        policy.set_decision(
            origin.clone(),
            PermissionKind::Notifications,
            PermissionDecision::Allow,
        );
        service
            .merge_policy(&app.id, &AppPolicyV2::default(), &policy)
            .unwrap();

        assert_eq!(
            service
                .load_policy(&app.id)
                .unwrap()
                .decision(&origin, PermissionKind::Notifications),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn background_portal_changes_roll_back_when_policy_commit_fails() {
        let (_temp, service, _launcher, background) = service_with_background();
        let app = AppConfigV2::new("Transactional", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        let original = service.load_policy(&app.id).unwrap();
        let mut edited = original.clone();
        edited.background.enabled = true;
        edited.background.autostart = true;
        background
            .remove_on_request
            .borrow_mut()
            .replace((service.repository().clone(), app.id.clone()));

        assert!(block_on(
            service.merge_policy_with_background(&app.id, &original, &edited, None, "test",)
        )
        .is_err());
        assert_eq!(*background.updates.borrow(), vec![true, false]);
        assert!(!background.autostart.get());
    }

    #[test]
    fn background_rollback_uses_policy_reloaded_under_global_lock() {
        let (_temp, service, _launcher, background) = service_with_background();
        let app = AppConfigV2::new("Concurrent", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();

        let mut stale_original = AppPolicyV2::default();
        stale_original.background.enabled = true;
        stale_original.background.autostart = true;
        service
            .merge_policy(&app.id, &AppPolicyV2::default(), &stale_original)
            .unwrap();
        service
            .merge_policy(&app.id, &stale_original, &AppPolicyV2::default())
            .unwrap();

        let mut edited = stale_original.clone();
        edited.background.autostart = false;
        background.autostart.set(false);
        background
            .remove_on_request
            .borrow_mut()
            .replace((service.repository().clone(), app.id.clone()));

        assert!(block_on(service.merge_policy_with_background(
            &app.id,
            &stale_original,
            &edited,
            None,
            "test",
        ))
        .is_err());
        assert_eq!(*background.updates.borrow(), vec![false, false]);
        assert!(!background.autostart.get());
    }

    #[test]
    fn unrelated_policy_edits_skip_the_global_autostart_scan() {
        let (_temp, service, _launcher, background) = service_with_background();
        let app = AppConfigV2::new("Healthy", "example.org", 0).unwrap();
        let corrupt = AppConfigV2::new("Corrupt", "corrupt.example", 1).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        block_on(service.create(corrupt.clone(), b"icon", None)).unwrap();
        std::fs::write(
            service
                .repository()
                .app_dir(&corrupt.id)
                .join("policy.json"),
            b"not json",
        )
        .unwrap();
        let original = service.load_policy(&app.id).unwrap();
        let mut edited = original.clone();
        edited.navigation.enabled = true;
        edited
            .navigation
            .allowed_origins
            .insert(Origin::from_str("https://example.org").unwrap());

        block_on(service.merge_policy_with_background(&app.id, &original, &edited, None, "test"))
            .unwrap();
        assert!(background.updates.borrow().is_empty());
    }

    #[test]
    fn denied_global_autostart_does_not_commit_an_inconsistent_policy() {
        let (_temp, service, _launcher, background) = service_with_background();
        let opted_in = AppConfigV2::new("Opted in", "one.example", 0).unwrap();
        let edited_app = AppConfigV2::new("Edited", "two.example", 1).unwrap();
        block_on(service.create(opted_in.clone(), b"icon", None)).unwrap();
        block_on(service.create(edited_app.clone(), b"icon", None)).unwrap();
        let mut opted_in_policy = AppPolicyV2::default();
        opted_in_policy.background.enabled = true;
        opted_in_policy.background.autostart = true;
        service
            .merge_policy(&opted_in.id, &AppPolicyV2::default(), &opted_in_policy)
            .unwrap();

        let original = service.load_policy(&edited_app.id).unwrap();
        let mut edited = original.clone();
        edited.background.enabled = true;
        background.autostart.set(true);
        background.deny_autostart.set(true);

        assert!(block_on(service.merge_policy_with_background(
            &edited_app.id,
            &original,
            &edited,
            None,
            "test",
        ))
        .is_err());
        assert_eq!(service.load_policy(&edited_app.id).unwrap(), original);
        assert!(background.autostart.get());
        assert_eq!(*background.updates.borrow(), vec![false, true]);
    }

    #[test]
    fn deleting_last_autostart_app_updates_portal_and_rolls_back_failure() {
        let (_temp, service, launcher, background) = service_with_background();
        let app = AppConfigV2::new("Autostart", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        let mut policy = AppPolicyV2::default();
        policy.background.enabled = true;
        policy.background.autostart = true;
        service
            .merge_policy(&app.id, &AppPolicyV2::default(), &policy)
            .unwrap();
        background.autostart.set(true);
        *launcher.deny_uninstall.borrow_mut() = true;

        assert!(block_on(service.delete(&app.id)).is_err());
        assert!(service.contains(&app.id));
        assert!(background.autostart.get());
        assert_eq!(*background.updates.borrow(), vec![false, true]);

        *launcher.deny_uninstall.borrow_mut() = false;
        block_on(service.delete(&app.id)).unwrap();
        assert!(!service.contains(&app.id));
        assert!(!background.autostart.get());
        assert_eq!(*background.updates.borrow(), vec![false, true, false]);
    }

    #[test]
    fn corrupt_policy_does_not_prevent_explicit_deletion() {
        let (_temp, service, _launcher, background) = service_with_background();
        let app = AppConfigV2::new("Corrupt policy", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        std::fs::write(
            service.repository().app_dir(&app.id).join("policy.json"),
            b"not json",
        )
        .unwrap();

        block_on(service.delete(&app.id)).unwrap();
        assert!(!service.contains_any_data(&app.id));
        assert_eq!(*background.updates.borrow(), vec![false]);
    }

    #[test]
    fn backup_restore_commits_profile_before_launcher_and_rolls_back_denial() {
        let (_temp, service, launcher) = service();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("profile-state"), b"session").unwrap();
        let app = AppConfigV2::new("Example", "example.org", 0).unwrap();
        let origin = Origin::from_str("https://example.org").unwrap();
        let mut policy = AppPolicyV2::default();
        policy.set_decision(
            origin,
            PermissionKind::Notifications,
            PermissionDecision::Allow,
        );
        policy.background.enabled = true;
        policy.background.autostart = true;
        launcher
            .required_profiles
            .borrow_mut()
            .insert(app.id.clone(), service.profile_dir(&app.id));

        block_on(service.create_from_backup(
            app.clone(),
            b"icon",
            &policy,
            Some(source.path()),
            None,
        ))
        .unwrap();
        let restored_policy = service.load_policy(&app.id).unwrap();
        assert_eq!(
            restored_policy.decision(
                &Origin::from_str("https://example.org").unwrap(),
                PermissionKind::Notifications
            ),
            PermissionDecision::Allow
        );
        assert!(!restored_policy.background.enabled);
        assert!(!restored_policy.background.autostart);
        assert_eq!(
            std::fs::read(service.profile_dir(&app.id).join("profile-state")).unwrap(),
            b"session"
        );

        let denied = AppConfigV2::new("Denied", "denied.example", 1).unwrap();
        *launcher.deny_install.borrow_mut() = true;
        assert!(block_on(service.create_from_backup(
            denied.clone(),
            b"icon",
            &AppPolicyV2::default(),
            Some(source.path()),
            None,
        ))
        .is_err());
        assert!(!service.contains(&denied.id));
        assert!(!service.profile_dir(&denied.id).exists());
    }
}
