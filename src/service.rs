// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use anyhow::{Context, Result};
use ashpd::WindowIdentifier;

use crate::{
    launcher::{LauncherBackend, PortalLauncher, UninstallOutcome},
    model::{AppConfigV2, AppId, WindowState},
    policy::{AppPolicyV1, Origin, PermissionDecision, PermissionKind},
    repository::{AppRepository, LoadReport, ProfileLock},
};

#[derive(Debug, Clone)]
pub struct AppService<L> {
    repository: AppRepository,
    launcher: L,
}

impl<L: LauncherBackend> AppService<L> {
    pub fn new(repository: AppRepository, launcher: L) -> Self {
        Self {
            repository,
            launcher,
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

    pub fn install_profile_from(&self, id: &AppId, source: &std::path::Path) -> Result<()> {
        self.repository.install_profile_from(id, source)
    }

    pub fn save_runtime_state(&self, id: &AppId, window: WindowState) -> Result<()> {
        let mut current = self.repository.load(id)?;
        current.window = window;
        self.repository.update(&current, None)
    }

    pub fn load_policy(&self, id: &AppId) -> Result<AppPolicyV1> {
        self.repository.load_policy(id)
    }

    pub fn apply_policy_decisions(
        &self,
        id: &AppId,
        decisions: &[(Origin, PermissionKind, PermissionDecision)],
    ) -> Result<AppPolicyV1> {
        self.repository.apply_policy_decisions(id, decisions)
    }

    pub fn merge_policy(
        &self,
        id: &AppId,
        original: &AppPolicyV1,
        edited: &AppPolicyV1,
    ) -> Result<AppPolicyV1> {
        self.repository.merge_policy(id, original, edited)
    }

    pub fn reset_policy(&self, id: &AppId) -> Result<AppPolicyV1> {
        self.repository.reset_policy(id)
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
        let outcome = self.launcher.uninstall(id).await?;
        self.repository
            .delete(id)
            .context("launcher was removed but local data cleanup failed")?;
        Ok(outcome)
    }

    pub async fn repair(&self, id: &AppId, parent: Option<&WindowIdentifier>) -> Result<()> {
        let app = self.repository.load(id)?;
        let icon = self.repository.read_icon(id)?;
        self.launcher.install(&app, &icon, parent).await
    }
}

impl AppService<PortalLauncher> {
    pub fn portal() -> Self {
        Self::new(AppRepository::for_current_user(), PortalLauncher)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashSet, rc::Rc, str::FromStr};

    use anyhow::{bail, Result};
    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;
    use crate::policy::{Origin, PermissionDecision, PermissionKind};

    #[derive(Debug, Clone, Default)]
    struct FakeLauncher {
        installed: Rc<RefCell<HashSet<AppId>>>,
        deny_install: Rc<RefCell<bool>>,
        deny_uninstall: Rc<RefCell<bool>>,
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

    fn service() -> (tempfile::TempDir, AppService<FakeLauncher>, FakeLauncher) {
        let temp = tempfile::tempdir().unwrap();
        let repository = AppRepository::new(temp.path().join("data"), temp.path().join("cache"));
        let launcher = FakeLauncher::default();
        let service = AppService::new(repository, launcher.clone());
        (temp, service, launcher)
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
            .merge_policy(&app.id, &AppPolicyV1::default(), &policy)
            .unwrap();

        assert_eq!(
            service
                .load_policy(&app.id)
                .unwrap()
                .decision(&origin, PermissionKind::Notifications),
            PermissionDecision::Allow
        );
    }
}
