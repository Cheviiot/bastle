// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use gtk::glib;
use tempfile::{Builder, NamedTempFile, TempDir};

use crate::{
    config::DATA_DIR_NAME,
    model::{AppConfigV1, AppId, LegacySource},
};

const CONFIG_FILE: &str = "app.json";
const ICON_FILE: &str = "icon.png";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryWarning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct LoadReport {
    pub apps: Vec<AppConfigV1>,
    pub warnings: Vec<RepositoryWarning>,
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

    pub fn profile_dir(&self, id: &AppId) -> PathBuf {
        self.data_root.join("profiles").join(id.as_str())
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

            match self.load_from_path(&path.join(CONFIG_FILE)) {
                Ok(app) if app.id.as_str() == entry.file_name().to_string_lossy() => {
                    report.apps.push(app)
                }
                Ok(_) => report.warnings.push(RepositoryWarning {
                    path: path.join(CONFIG_FILE),
                    message: "directory name does not match the app id".to_owned(),
                }),
                Err(error) => report.warnings.push(RepositoryWarning {
                    path: path.join(CONFIG_FILE),
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

    pub fn load(&self, id: &AppId) -> Result<AppConfigV1> {
        self.load_from_path(&self.app_dir(id).join(CONFIG_FILE))
    }

    fn load_from_path(&self, path: &Path) -> Result<AppConfigV1> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut app: AppConfigV1 = serde_json::from_str(&contents)
            .with_context(|| format!("invalid JSON in {}", path.display()))?;
        app.normalize_and_validate()
            .with_context(|| format!("invalid app configuration in {}", path.display()))?;
        Ok(app)
    }

    pub fn read_icon(&self, id: &AppId) -> Result<Vec<u8>> {
        let path = self.app_dir(id).join(ICON_FILE);
        fs::read(&path).with_context(|| format!("failed to read {}", path.display()))
    }

    pub fn contains(&self, id: &AppId) -> bool {
        self.app_dir(id).join(CONFIG_FILE).is_file()
    }

    pub fn contains_legacy_source(&self, source: &LegacySource) -> Result<bool> {
        Ok(self.list()?.apps.iter().any(|app| {
            app.imported_from
                .as_ref()
                .is_some_and(|existing| existing == source)
        }))
    }

    #[cfg(test)]
    pub fn create(&self, app: &AppConfigV1, icon: &[u8]) -> Result<()> {
        self.stage_create(app, icon)?.commit()
    }

    pub fn stage_create(&self, app: &AppConfigV1, icon: &[u8]) -> Result<StagedApp> {
        let final_dir = self.app_dir(&app.id);
        if final_dir.exists() {
            bail!("an app with id {} already exists", app.id);
        }

        let apps_root = self.apps_root();
        fs::create_dir_all(&apps_root)
            .with_context(|| format!("failed to create {}", apps_root.display()))?;
        let staging = Builder::new()
            .prefix(".tmp-")
            .tempdir_in(&apps_root)
            .context("failed to create app staging directory")?;
        write_json(&staging.path().join(CONFIG_FILE), app)?;
        write_bytes(&staging.path().join(ICON_FILE), icon)?;
        Ok(StagedApp {
            directory: staging,
            final_path: final_dir,
        })
    }

    pub fn update(&self, app: &AppConfigV1, icon: Option<&[u8]>) -> Result<()> {
        let app_dir = self.app_dir(&app.id);
        if !app_dir.is_dir() {
            bail!("app {} is not stored locally", app.id);
        }

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

    pub fn delete(&self, id: &AppId) -> Result<()> {
        let targets = [self.app_dir(id), self.profile_dir(id), self.cache_dir(id)];
        for target in targets {
            if target.exists() {
                fs::remove_dir_all(&target)
                    .with_context(|| format!("failed to remove {}", target.display()))?;
            }
        }
        Ok(())
    }
}

fn serialize_json(app: &AppConfigV1) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(app)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_json(path: &Path, app: &AppConfigV1) -> Result<()> {
    write_bytes(path, &serialize_json(app)?)
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
    use crate::model::{LegacySource, SCHEMA_VERSION};

    fn repository() -> (tempfile::TempDir, AppRepository) {
        let temp = tempfile::tempdir().unwrap();
        let repository = AppRepository::new(temp.path().join("data"), temp.path().join("cache"));
        (temp, repository)
    }

    #[test]
    fn create_load_update_and_delete() {
        let (_temp, repository) = repository();
        let mut app = AppConfigV1::new("Example", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();
        assert_eq!(repository.load(&app.id).unwrap(), app);
        assert_eq!(repository.read_icon(&app.id).unwrap(), b"icon");

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
            r#"{{"schema_version":{},"id":"mnopqrstuvwx","title":"Future","start_url":"https://example.org","user_agent":null,"use_theme_color":true,"window":{{"width":800,"height":600,"maximized":false}},"sort_order":0,"imported_from":null}}"#,
            SCHEMA_VERSION + 1
        );
        fs::write(future_dir.join(CONFIG_FILE), future).unwrap();

        let report = repository.list().unwrap();
        assert!(report.apps.is_empty());
        assert_eq!(report.warnings.len(), 2);
        assert!(bad_dir.exists());
        assert!(future_dir.exists());
    }

    #[test]
    fn legacy_source_lookup_is_idempotent() {
        let (_temp, repository) = repository();
        let source = LegacySource {
            app_id: "io.github.zaedus.spider".to_owned(),
            legacy_id: "old-app".to_owned(),
        };
        let mut app = AppConfigV1::new("Example", "example.org", 0).unwrap();
        app.imported_from = Some(source.clone());
        repository.create(&app, b"icon").unwrap();
        assert!(repository.contains_legacy_source(&source).unwrap());
    }

    #[test]
    fn missing_icon_is_reported_without_removing_config() {
        let (_temp, repository) = repository();
        let app = AppConfigV1::new("Example", "example.org", 0).unwrap();
        repository.create(&app, b"icon").unwrap();
        fs::remove_file(repository.app_dir(&app.id).join(ICON_FILE)).unwrap();
        assert!(repository.read_icon(&app.id).is_err());
        assert!(repository.contains(&app.id));
    }

    #[test]
    fn rejected_icon_update_does_not_commit_new_config() {
        let (_temp, repository) = repository();
        let mut app = AppConfigV1::new("Before", "example.org", 0).unwrap();
        repository.create(&app, b"old-icon").unwrap();
        let icon_path = repository.app_dir(&app.id).join(ICON_FILE);
        fs::remove_file(&icon_path).unwrap();
        fs::create_dir(&icon_path).unwrap();

        app.title = "After".to_owned();
        assert!(repository.update(&app, Some(b"new-icon")).is_err());
        assert_eq!(repository.load(&app.id).unwrap().title, "Before");
    }
}
