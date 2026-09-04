// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    future::Future,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ashpd::WindowIdentifier;

use crate::{
    launcher::{LauncherBackend, PortalLauncher, UninstallOutcome},
    legacy::{self, ImportSummary, LegacyPreview},
    model::{AppConfigV1, AppId},
    repository::{AppRepository, LoadReport},
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

    pub fn load(&self, id: &AppId) -> Result<AppConfigV1> {
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

    pub fn save_runtime_state(&self, app: &AppConfigV1) -> Result<()> {
        self.repository.update(app, None)
    }

    pub fn preview_legacy(&self, selected: &Path) -> Result<LegacyPreview> {
        legacy::parse_keyfile(selected)
    }

    pub async fn create(
        &self,
        mut app: AppConfigV1,
        icon: &[u8],
        parent: Option<&WindowIdentifier>,
    ) -> Result<AppConfigV1> {
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
        mut app: AppConfigV1,
        icon: Option<&[u8]>,
        parent: Option<&WindowIdentifier>,
    ) -> Result<AppConfigV1> {
        app.normalize_and_validate()?;
        let previous = self.repository.load(&app.id)?;
        let previous_icon = self.repository.read_icon(&app.id)?;
        let current_icon = icon.unwrap_or(&previous_icon);
        self.launcher.install(&app, current_icon, parent).await?;
        if let Err(error) = self.repository.update(&app, icon) {
            let _ = self
                .launcher
                .install(&previous, &previous_icon, parent)
                .await;
            return Err(error).context("launcher was restored after a failed local update");
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

    pub async fn import_many<F, Fut>(
        &self,
        candidates: Vec<AppConfigV1>,
        invalid: usize,
        skipped: usize,
        parent: Option<&WindowIdentifier>,
        mut icon_for_url: F,
    ) -> ImportSummary
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<Vec<u8>>>,
    {
        let mut summary = ImportSummary {
            invalid,
            skipped,
            ..Default::default()
        };
        for mut app in candidates {
            let Some(source) = app.imported_from.as_ref() else {
                summary.invalid += 1;
                continue;
            };
            match self.repository.contains_legacy_source(source) {
                Ok(true) => {
                    summary.skipped += 1;
                    continue;
                }
                Ok(false) => {}
                Err(_) => {
                    summary.failed += 1;
                    continue;
                }
            }
            while self.contains(&app.id) {
                app.id = AppId::generate();
            }
            let icon = match icon_for_url(app.start_url.clone()).await {
                Ok(icon) => icon,
                Err(_) => {
                    summary.failed += 1;
                    continue;
                }
            };
            match self.create(app, &icon, parent).await {
                Ok(_) => summary.imported += 1,
                Err(_) => summary.failed += 1,
            }
        }
        summary
    }
}

impl AppService<PortalLauncher> {
    pub fn portal() -> Self {
        Self::new(AppRepository::for_current_user(), PortalLauncher)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashSet, rc::Rc};

    use anyhow::{bail, Result};
    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;

    #[derive(Debug, Clone, Default)]
    struct FakeLauncher {
        installed: Rc<RefCell<HashSet<AppId>>>,
        deny_install: Rc<RefCell<bool>>,
        deny_uninstall: Rc<RefCell<bool>>,
        fail_title: Rc<RefCell<Option<String>>>,
    }

    #[async_trait(?Send)]
    impl LauncherBackend for FakeLauncher {
        async fn install(
            &self,
            app: &AppConfigV1,
            _icon: &[u8],
            _parent: Option<&WindowIdentifier>,
        ) -> Result<()> {
            if *self.deny_install.borrow() {
                bail!("portal denied installation");
            }
            if self
                .fail_title
                .borrow()
                .as_deref()
                .is_some_and(|title| title == app.title)
            {
                bail!("selective portal failure");
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
        let app = AppConfigV1::new("Example", "example.org", 0).unwrap();
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
        let app = AppConfigV1::new("Example", "example.org", 0).unwrap();
        assert!(block_on(service.create(app.clone(), b"icon", None)).is_err());
        assert!(!service.repository().contains(&app.id));
        assert!(!launcher.installed.borrow().contains(&app.id));
    }

    #[test]
    fn portal_error_preserves_local_data_on_delete() {
        let (_temp, service, launcher) = service();
        let app = AppConfigV1::new("Example", "example.org", 0).unwrap();
        block_on(service.create(app.clone(), b"icon", None)).unwrap();
        *launcher.deny_uninstall.borrow_mut() = true;
        assert!(block_on(service.delete(&app.id)).is_err());
        assert!(service.repository().contains(&app.id));
    }

    #[test]
    fn partial_import_reports_every_outcome() {
        let (_temp, service, launcher) = service();
        let source = crate::model::LegacySource {
            app_id: "io.github.zaedus.spider".to_owned(),
            legacy_id: "existing".to_owned(),
        };
        let mut existing = AppConfigV1::new("Existing", "example.org", 0).unwrap();
        existing.imported_from = Some(source.clone());
        block_on(service.create(existing, b"icon", None)).unwrap();

        let mut duplicate = AppConfigV1::new("Duplicate", "example.org", 1).unwrap();
        duplicate.imported_from = Some(source);
        let mut success = AppConfigV1::new("Success", "example.net", 2).unwrap();
        success.imported_from = Some(crate::model::LegacySource {
            app_id: "io.github.zaedus.spider".to_owned(),
            legacy_id: "success".to_owned(),
        });
        let mut failure = AppConfigV1::new("Failure", "example.com", 3).unwrap();
        failure.imported_from = Some(crate::model::LegacySource {
            app_id: "io.github.zaedus.spider".to_owned(),
            legacy_id: "failure".to_owned(),
        });
        *launcher.fail_title.borrow_mut() = Some("Failure".to_owned());

        let summary = block_on(service.import_many(
            vec![duplicate, success, failure],
            1,
            0,
            None,
            |_| async { Ok(b"icon".to_vec()) },
        ));
        assert_eq!(
            summary,
            ImportSummary {
                imported: 1,
                skipped: 1,
                invalid: 1,
                failed: 1,
            }
        );
    }
}
