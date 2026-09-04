// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::{BTreeSet, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use age::secrecy::{ExposeSecret, SecretString};
use anyhow::{anyhow, bail, ensure, Context, Result};
use ashpd::WindowIdentifier;
use serde::{Deserialize, Serialize};
use tempfile::{Builder as TempBuilder, NamedTempFile, TempDir};

use crate::{
    launcher::{LauncherBackend, PortalLauncher},
    model::{AppConfigV2, AppId},
    policy::{AppPolicyV1, Origin, PermissionDecision, PermissionKind},
    repository::{ProfileLock, RUNTIME_LOCK_FILE},
    service::AppService,
};

pub const BACKUP_SCHEMA_VERSION: u32 = 1;
const MANIFEST_PATH: &str = "manifest.json";
const AGE_HEADER: &[u8] = b"age-encryption.org/v1";
const MAX_ARCHIVE_FILES: usize = 100_000;
const MAX_UNCOMPRESSED_SIZE: u64 = 16 * 1024 * 1024 * 1024;
const MAX_MANIFEST_SIZE: u64 = 16 * 1024 * 1024;
const MAX_APP_CONFIG_SIZE: u64 = 1024 * 1024;
const MAX_POLICY_SIZE: u64 = 16 * 1024 * 1024;
const MAX_ICON_SIZE: u64 = 10 * 1024 * 1024;

#[derive(Debug, Default)]
struct ArchiveBudget {
    files: usize,
    uncompressed_size: u64,
}

impl ArchiveBudget {
    fn include(&mut self, size: u64) -> Result<()> {
        let files = self
            .files
            .checked_add(1)
            .ok_or_else(|| anyhow!("backup file count overflow"))?;
        ensure!(files <= MAX_ARCHIVE_FILES, "backup contains too many files");
        let uncompressed_size = self
            .uncompressed_size
            .checked_add(size)
            .ok_or_else(|| anyhow!("backup size overflow"))?;
        ensure!(
            uncompressed_size <= MAX_UNCOMPRESSED_SIZE,
            "backup exceeds the uncompressed size limit"
        );
        self.files = files;
        self.uncompressed_size = uncompressed_size;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifestAppV1 {
    pub id: AppId,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifestV1 {
    pub schema_version: u32,
    pub includes_site_data: bool,
    pub apps: Vec<BackupManifestAppV1>,
}

impl BackupManifestV1 {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == BACKUP_SCHEMA_VERSION,
            "unsupported backup manifest version {}",
            self.schema_version
        );
        ensure!(!self.apps.is_empty(), "the backup contains no applications");
        let mut ids = HashSet::new();
        for app in &self.apps {
            AppId::from_str(app.id.as_str())?;
            ensure!(ids.insert(app.id.clone()), "duplicate app id {}", app.id);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct BackupOptions {
    pub include_site_data: bool,
    pub passphrase: Option<SecretString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreDisposition {
    RestoreAsIs,
    RestoreWithNewId,
    SkipIdentical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePreviewEntry {
    pub source_id: AppId,
    pub target_id: AppId,
    pub title: String,
    pub disposition: RestoreDisposition,
}

#[derive(Debug)]
pub struct RestorePlan {
    extracted: TempDir,
    pub manifest: BackupManifestV1,
    pub encrypted: bool,
    pub entries: Vec<RestorePreviewEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreFailure {
    pub source_id: AppId,
    pub message: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub restored: usize,
    pub skipped: usize,
    pub failed: Vec<RestoreFailure>,
}

#[derive(Debug, Clone)]
pub struct BackupService<L> {
    service: AppService<L>,
}

impl<L: LauncherBackend + Clone> BackupService<L> {
    pub fn new(service: AppService<L>) -> Self {
        Self { service }
    }

    pub fn service(&self) -> &AppService<L> {
        &self.service
    }

    pub fn create_backup(
        &self,
        destination: &Path,
        ids: &[AppId],
        options: &BackupOptions,
    ) -> Result<()> {
        ensure!(!ids.is_empty(), "there are no applications to back up");
        if options.include_site_data {
            let passphrase = options
                .passphrase
                .as_ref()
                .context("a passphrase is required when site data is included")?;
            ensure!(
                !passphrase.expose_secret().is_empty(),
                "the backup passphrase cannot be empty"
            );
        }

        let mut apps = Vec::new();
        let mut locks: Vec<ProfileLock> = Vec::new();
        for id in ids {
            let config = self.service.load(id)?;
            if options.include_site_data {
                locks.push(self.service.try_acquire_profile_snapshot_lock(id)?);
            }
            apps.push(config);
        }
        apps.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        let manifest = BackupManifestV1 {
            schema_version: BACKUP_SCHEMA_VERSION,
            includes_site_data: options.include_site_data,
            apps: apps
                .iter()
                .map(|app| BackupManifestAppV1 {
                    id: app.id.clone(),
                    title: app.title.clone(),
                })
                .collect(),
        };
        manifest.validate()?;

        let parent = destination
            .parent()
            .context("backup destination has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        let mut temporary = NamedTempFile::new_in(parent).with_context(|| {
            format!("failed to create a temporary file in {}", parent.display())
        })?;
        if let Some(passphrase) = options.passphrase.clone() {
            let encryptor = age::Encryptor::with_user_passphrase(passphrase);
            let age_writer = encryptor.wrap_output(temporary.as_file_mut())?;
            let mut zstd_writer = zstd::stream::Encoder::new(age_writer, 9)?;
            self.write_tar(&mut zstd_writer, &manifest, &apps)?;
            let age_writer = zstd_writer.finish()?;
            age_writer.finish()?;
        } else {
            let mut zstd_writer = zstd::stream::Encoder::new(temporary.as_file_mut(), 9)?;
            self.write_tar(&mut zstd_writer, &manifest, &apps)?;
            zstd_writer.finish()?;
        }
        temporary.as_file().sync_all()?;
        temporary
            .persist(destination)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to atomically write {}", destination.display()))?;
        sync_parent(destination)?;
        drop(locks);
        Ok(())
    }

    fn write_tar<W: Write>(
        &self,
        writer: W,
        manifest: &BackupManifestV1,
        apps: &[AppConfigV2],
    ) -> Result<()> {
        let mut archive = tar::Builder::new(writer);
        archive.mode(tar::HeaderMode::Deterministic);
        let mut budget = ArchiveBudget::default();
        append_json(
            &mut archive,
            Path::new(MANIFEST_PATH),
            manifest,
            MAX_MANIFEST_SIZE,
            &mut budget,
        )?;
        for app in apps {
            let prefix = PathBuf::from("apps").join(app.id.as_str());
            append_json(
                &mut archive,
                &prefix.join("app.json"),
                app,
                MAX_APP_CONFIG_SIZE,
                &mut budget,
            )?;
            append_bytes(
                &mut archive,
                &prefix.join("icon.png"),
                &self.service.read_icon(&app.id)?,
                MAX_ICON_SIZE,
                &mut budget,
            )?;
            append_json(
                &mut archive,
                &prefix.join("policy.json"),
                &self.service.load_policy(&app.id)?,
                MAX_POLICY_SIZE,
                &mut budget,
            )?;
            if manifest.includes_site_data {
                append_profile(
                    &mut archive,
                    &self.service.profile_dir(&app.id),
                    &PathBuf::from("profiles").join(app.id.as_str()),
                    &mut budget,
                )?;
            }
        }
        archive.finish()?;
        Ok(())
    }

    pub fn prepare_restore(
        &self,
        source: &Path,
        passphrase: Option<&SecretString>,
    ) -> Result<RestorePlan> {
        let (extracted, manifest, encrypted) = extract_backup(source, passphrase)?;
        let mut reserved = HashSet::new();
        let mut entries = Vec::new();
        for manifest_app in &manifest.apps {
            let archived = read_archived_app(extracted.path(), &manifest_app.id)?;
            let (target_id, disposition) = if self.service.contains(&manifest_app.id) {
                if self.existing_matches(&manifest_app.id, &archived)?
                    && !manifest.includes_site_data
                {
                    (manifest_app.id.clone(), RestoreDisposition::SkipIdentical)
                } else {
                    (
                        self.generate_restore_id(&reserved),
                        RestoreDisposition::RestoreWithNewId,
                    )
                }
            } else if self.service.contains_any_data(&manifest_app.id) {
                (
                    self.generate_restore_id(&reserved),
                    RestoreDisposition::RestoreWithNewId,
                )
            } else {
                (manifest_app.id.clone(), RestoreDisposition::RestoreAsIs)
            };
            reserved.insert(target_id.clone());
            entries.push(RestorePreviewEntry {
                source_id: manifest_app.id.clone(),
                target_id,
                title: archived.config.title,
                disposition,
            });
        }
        Ok(RestorePlan {
            extracted,
            manifest,
            encrypted,
            entries,
        })
    }

    fn generate_restore_id(&self, reserved: &HashSet<AppId>) -> AppId {
        loop {
            let id = AppId::generate();
            if !reserved.contains(&id) && !self.service.contains_any_data(&id) {
                return id;
            }
        }
    }

    fn existing_matches(&self, id: &AppId, archived: &ArchivedApp) -> Result<bool> {
        Ok(self.service.load(id)? == archived.config
            && self.service.read_icon(id)? == archived.icon
            && self.service.load_policy(id)? == archived.policy)
    }

    pub async fn restore(
        &self,
        plan: RestorePlan,
        selected: &HashSet<AppId>,
        parent: Option<&WindowIdentifier>,
    ) -> RestoreReport {
        let mut report = RestoreReport::default();
        for entry in plan.entries {
            if entry.disposition == RestoreDisposition::SkipIdentical
                || !selected.contains(&entry.source_id)
            {
                report.skipped += 1;
                continue;
            }
            let result = self
                .restore_one(
                    plan.extracted.path(),
                    plan.manifest.includes_site_data,
                    &entry,
                    parent,
                )
                .await;
            match result {
                Ok(()) => report.restored += 1,
                Err(error) => report.failed.push(RestoreFailure {
                    source_id: entry.source_id,
                    message: format!("{error:#}"),
                }),
            }
        }
        report
    }

    async fn restore_one(
        &self,
        extracted: &Path,
        includes_site_data: bool,
        preview: &RestorePreviewEntry,
        parent: Option<&WindowIdentifier>,
    ) -> Result<()> {
        let archived = read_archived_app(extracted, &preview.source_id)?;
        let mut config = archived.config;
        config.id = preview.target_id.clone();
        let created = self
            .service
            .create(config, &archived.icon, parent)
            .await
            .context("failed to install the restored launcher")?;

        let after_create: Result<()> = async {
            let changes = explicit_policy_decisions(&archived.policy);
            self.service.apply_policy_decisions(&created.id, &changes)?;
            if includes_site_data {
                let profile = extracted.join("profiles").join(preview.source_id.as_str());
                self.service
                    .install_profile_from(&created.id, &profile)
                    .await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = after_create {
            return match self.service.delete(&created.id).await {
                Ok(_) => Err(error).context("the partial restore was rolled back"),
                Err(rollback) => {
                    Err(error).context(format!("restore rollback also failed: {rollback}"))
                }
            };
        }
        Ok(())
    }
}

impl BackupService<PortalLauncher> {
    pub fn portal() -> Self {
        Self::new(AppService::portal())
    }
}

pub fn is_encrypted_backup(path: &Path) -> Result<bool> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    Ok(BufReader::new(file).fill_buf()?.starts_with(AGE_HEADER))
}

#[derive(Debug)]
struct ArchivedApp {
    config: AppConfigV2,
    icon: Vec<u8>,
    policy: AppPolicyV1,
}

fn read_archived_app(root: &Path, id: &AppId) -> Result<ArchivedApp> {
    let directory = root.join("apps").join(id.as_str());
    let mut config: AppConfigV2 =
        read_limited_json(&directory.join("app.json"), MAX_APP_CONFIG_SIZE)?;
    config.normalize_and_validate()?;
    ensure!(config.id == *id, "archive app id does not match its path");
    let icon = read_limited_file(&directory.join("icon.png"), MAX_ICON_SIZE)?;
    ensure!(!icon.is_empty(), "archive icon is empty");
    let policy: AppPolicyV1 = read_limited_json(&directory.join("policy.json"), MAX_POLICY_SIZE)?;
    policy.validate()?;
    Ok(ArchivedApp {
        config,
        icon,
        policy,
    })
}

fn explicit_policy_decisions(
    policy: &AppPolicyV1,
) -> Vec<(Origin, PermissionKind, PermissionDecision)> {
    policy
        .permissions
        .iter()
        .flat_map(|(origin, permissions)| {
            permissions
                .iter()
                .map(move |(kind, decision)| (origin.clone(), *kind, *decision))
        })
        .collect()
}

fn append_json<W: Write>(
    archive: &mut tar::Builder<W>,
    path: &Path,
    value: &impl Serialize,
    limit: u64,
    budget: &mut ArchiveBudget,
) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    append_bytes(archive, path, &bytes, limit, budget)
}

fn append_bytes<W: Write>(
    archive: &mut tar::Builder<W>,
    path: &Path,
    bytes: &[u8],
    limit: u64,
    budget: &mut ArchiveBudget,
) -> Result<()> {
    ensure!(
        bytes.len() as u64 <= limit,
        "{} exceeds the {} byte size limit",
        path.display(),
        limit
    );
    budget.include(bytes.len() as u64)?;
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0o600);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(bytes.len() as u64);
    header.set_cksum();
    archive.append_data(&mut header, path, bytes)?;
    Ok(())
}

fn append_profile<W: Write>(
    archive: &mut tar::Builder<W>,
    source: &Path,
    archive_prefix: &Path,
    budget: &mut ArchiveBudget,
) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    let mut entries = fs::read_dir(source)
        .with_context(|| format!("failed to read {}", source.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry.file_type()?;
        if entry.file_name() == RUNTIME_LOCK_FILE {
            continue;
        }
        let archive_path = archive_prefix.join(entry.file_name());
        if file_type.is_dir() {
            append_profile(archive, &entry.path(), &archive_path, budget)?;
        } else if file_type.is_file() {
            let mut file = File::open(entry.path())?;
            let size = file.metadata()?.len();
            budget.include(size)?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o600);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_size(size);
            header.set_cksum();
            archive.append_data(&mut header, &archive_path, &mut file)?;
        } else {
            bail!(
                "profile contains an unsupported file: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn extract_backup(
    source: &Path,
    passphrase: Option<&SecretString>,
) -> Result<(TempDir, BackupManifestV1, bool)> {
    let file =
        File::open(source).with_context(|| format!("failed to open {}", source.display()))?;
    let mut input = BufReader::new(file);
    let encrypted = input.fill_buf()?.starts_with(AGE_HEADER);
    let extracted = TempBuilder::new().prefix("bastle-restore-").tempdir()?;
    let paths = if encrypted {
        let passphrase =
            passphrase.context("this backup is encrypted and requires a passphrase")?;
        let decryptor = age::Decryptor::new(input).context("invalid age-encrypted backup")?;
        let identity = age::scrypt::Identity::new(passphrase.clone());
        let reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .context("the backup passphrase is incorrect")?;
        let decoder = zstd::stream::Decoder::new(reader).context("invalid zstd backup payload")?;
        extract_tar(decoder, extracted.path())?
    } else {
        let decoder = zstd::stream::Decoder::new(input).context("invalid zstd backup")?;
        extract_tar(decoder, extracted.path())?
    };
    let manifest: BackupManifestV1 =
        read_limited_json(&extracted.path().join(MANIFEST_PATH), MAX_MANIFEST_SIZE)?;
    manifest.validate()?;
    ensure!(
        !manifest.includes_site_data || encrypted,
        "a backup containing site data must be age-encrypted"
    );
    validate_archive_paths(&manifest, &paths)?;
    for app in &manifest.apps {
        read_archived_app(extracted.path(), &app.id)?;
    }
    Ok((extracted, manifest, encrypted))
}

fn extract_tar<R: Read>(reader: R, destination: &Path) -> Result<Vec<PathBuf>> {
    let mut archive = tar::Archive::new(reader);
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut total_size = 0_u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        ensure!(
            paths.len() < MAX_ARCHIVE_FILES,
            "backup contains too many files"
        );
        let path = entry.path()?.into_owned();
        ensure!(
            safe_archive_path(&path),
            "unsafe archive path: {}",
            path.display()
        );
        ensure!(
            seen.insert(path.clone()),
            "duplicate archive path: {}",
            path.display()
        );
        let entry_type = entry.header().entry_type();
        ensure!(
            entry_type.is_file() || entry_type.is_dir(),
            "links and special files are not allowed in backups"
        );
        total_size = total_size
            .checked_add(entry.header().size()?)
            .ok_or_else(|| anyhow!("backup size overflow"))?;
        ensure!(
            total_size <= MAX_UNCOMPRESSED_SIZE,
            "backup exceeds the uncompressed size limit"
        );
        ensure!(
            entry.unpack_in(destination)?,
            "archive path escaped the restore directory"
        );
        paths.push(path);
    }
    Ok(paths)
}

fn safe_archive_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn validate_archive_paths(manifest: &BackupManifestV1, paths: &[PathBuf]) -> Result<()> {
    let ids = manifest
        .apps
        .iter()
        .map(|app| app.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut required = BTreeSet::from([PathBuf::from(MANIFEST_PATH)]);
    for id in &ids {
        let prefix = PathBuf::from("apps").join(id);
        required.insert(prefix.join("app.json"));
        required.insert(prefix.join("icon.png"));
        required.insert(prefix.join("policy.json"));
    }
    for path in paths {
        if required.contains(path) {
            continue;
        }
        let components = path.components().collect::<Vec<_>>();
        let valid_profile = manifest.includes_site_data
            && components.len() >= 3
            && components[0].as_os_str() == "profiles"
            && ids.contains(components[1].as_os_str().to_string_lossy().as_ref())
            && !(components.len() == 3 && components[2].as_os_str() == RUNTIME_LOCK_FILE);
        ensure!(valid_profile, "unexpected backup entry: {}", path.display());
    }
    for path in required {
        ensure!(
            paths.contains(&path),
            "required backup entry is missing: {}",
            path.display()
        );
    }
    Ok(())
}

fn read_limited_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> Result<T> {
    let bytes = read_limited_file(path, limit)?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn read_limited_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let size = file.metadata()?.len();
    ensure!(
        size <= limit,
        "{} exceeds the {} byte size limit",
        path.display(),
        limit
    );
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "{} grew beyond the {} byte size limit while being read",
        path.display(),
        limit
    );
    Ok(bytes)
}

fn sync_parent(path: &Path) -> Result<()> {
    let parent = path.parent().context("path has no parent directory")?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashSet as StdHashSet, rc::Rc};

    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;
    use crate::{launcher::UninstallOutcome, repository::AppRepository};

    #[derive(Debug, Clone, Default)]
    struct FakeLauncher {
        installed: Rc<RefCell<StdHashSet<AppId>>>,
        denied_installs: Rc<RefCell<StdHashSet<AppId>>>,
    }

    #[async_trait(?Send)]
    impl LauncherBackend for FakeLauncher {
        async fn install(
            &self,
            app: &AppConfigV2,
            _icon: &[u8],
            _parent: Option<&WindowIdentifier>,
        ) -> Result<()> {
            if self.denied_installs.borrow().contains(&app.id) {
                bail!("portal denied installation for {}", app.id);
            }
            self.installed.borrow_mut().insert(app.id.clone());
            Ok(())
        }

        async fn uninstall(&self, id: &AppId) -> Result<UninstallOutcome> {
            Ok(if self.installed.borrow_mut().remove(id) {
                UninstallOutcome::Removed
            } else {
                UninstallOutcome::AlreadyMissing
            })
        }
    }

    fn backup_service(root: &Path) -> BackupService<FakeLauncher> {
        let repository = AppRepository::new(root.join("data"), root.join("cache"));
        BackupService::new(AppService::new(repository, FakeLauncher::default()))
    }

    #[test]
    fn metadata_backup_round_trips_and_never_contains_cache() {
        let source = tempfile::tempdir().unwrap();
        let source_service = backup_service(source.path());
        let app = AppConfigV2::new("Example", "https://example.org", 0).unwrap();
        block_on(source_service.service.create(app.clone(), b"icon", None)).unwrap();
        fs::create_dir_all(source_service.service.cache_dir(&app.id)).unwrap();
        fs::write(
            source_service.service.cache_dir(&app.id).join("secret"),
            b"cache",
        )
        .unwrap();
        let backup = source.path().join("example.bastle-backup");
        source_service
            .create_backup(
                &backup,
                std::slice::from_ref(&app.id),
                &BackupOptions::default(),
            )
            .unwrap();

        let target = tempfile::tempdir().unwrap();
        let target_service = backup_service(target.path());
        let plan = target_service.prepare_restore(&backup, None).unwrap();
        assert!(!plan.encrypted);
        assert_eq!(plan.entries[0].disposition, RestoreDisposition::RestoreAsIs);
        let selected = HashSet::from([app.id.clone()]);
        let report = block_on(target_service.restore(plan, &selected, None));
        assert_eq!(report.restored, 1);
        assert!(report.failed.is_empty());
        assert_eq!(target_service.service.load(&app.id).unwrap(), app);
        assert!(!target_service.service.cache_dir(&app.id).exists());
    }

    #[test]
    fn site_data_requires_encryption_and_an_idle_profile() {
        let source = tempfile::tempdir().unwrap();
        let service = backup_service(source.path());
        let app = AppConfigV2::new("Example", "example.org", 0).unwrap();
        block_on(service.service.create(app.clone(), b"icon", None)).unwrap();
        fs::create_dir_all(service.service.profile_dir(&app.id)).unwrap();
        fs::write(
            service.service.profile_dir(&app.id).join("cookies.sqlite"),
            b"cookies",
        )
        .unwrap();
        let backup = source.path().join("site-data.bastle-backup");
        let missing_passphrase = BackupOptions {
            include_site_data: true,
            passphrase: None,
        };
        assert!(service
            .create_backup(&backup, std::slice::from_ref(&app.id), &missing_passphrase)
            .is_err());

        let runtime_lock = service.service.acquire_runtime_lock(&app.id).unwrap();
        let encrypted = BackupOptions {
            include_site_data: true,
            passphrase: Some(SecretString::from(
                "correct horse battery staple".to_owned(),
            )),
        };
        assert!(service
            .create_backup(&backup, std::slice::from_ref(&app.id), &encrypted)
            .is_err());
        drop(runtime_lock);
        service
            .create_backup(&backup, std::slice::from_ref(&app.id), &encrypted)
            .unwrap();
        assert!(service.prepare_restore(&backup, None).is_err());
        let wrong = SecretString::from("wrong passphrase".to_owned());
        assert!(service.prepare_restore(&backup, Some(&wrong)).is_err());
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        let plan = service.prepare_restore(&backup, Some(&passphrase)).unwrap();
        assert!(plan.encrypted);
        assert!(plan.manifest.includes_site_data);
        assert_eq!(
            plan.entries[0].disposition,
            RestoreDisposition::RestoreWithNewId
        );
        assert_ne!(plan.entries[0].target_id, app.id);

        let target = tempfile::tempdir().unwrap();
        let target_service = backup_service(target.path());
        let target_plan = target_service
            .prepare_restore(&backup, Some(&passphrase))
            .unwrap();
        let selected = HashSet::from([app.id.clone()]);
        let report = block_on(target_service.restore(target_plan, &selected, None));
        assert_eq!(report.restored, 1);
        assert!(report.failed.is_empty());
        assert_eq!(
            fs::read(
                target_service
                    .service
                    .profile_dir(&app.id)
                    .join("cookies.sqlite")
            )
            .unwrap(),
            b"cookies"
        );
        assert!(!target_service
            .service
            .profile_dir(&app.id)
            .join(RUNTIME_LOCK_FILE)
            .exists());
    }

    #[test]
    fn conflicting_ids_are_remapped_and_identical_ids_are_skipped() {
        let source = tempfile::tempdir().unwrap();
        let source_service = backup_service(source.path());
        let app = AppConfigV2::new("Source", "example.org", 0).unwrap();
        block_on(source_service.service.create(app.clone(), b"icon", None)).unwrap();
        let backup = source.path().join("conflict.bastle-backup");
        source_service
            .create_backup(
                &backup,
                std::slice::from_ref(&app.id),
                &BackupOptions::default(),
            )
            .unwrap();

        let target = tempfile::tempdir().unwrap();
        let target_service = backup_service(target.path());
        block_on(target_service.service.create(app.clone(), b"icon", None)).unwrap();
        let identical = target_service.prepare_restore(&backup, None).unwrap();
        assert_eq!(
            identical.entries[0].disposition,
            RestoreDisposition::SkipIdentical
        );

        let mut changed = app.clone();
        changed.title = "Existing".to_owned();
        block_on(target_service.service.update(changed, None, None)).unwrap();
        let conflict = target_service.prepare_restore(&backup, None).unwrap();
        assert_eq!(
            conflict.entries[0].disposition,
            RestoreDisposition::RestoreWithNewId
        );
        assert_ne!(conflict.entries[0].target_id, app.id);
    }

    #[test]
    fn archive_paths_must_be_relative_and_cannot_traverse() {
        assert!(safe_archive_path(Path::new("apps/abcdefghijkl/app.json")));
        assert!(!safe_archive_path(Path::new("../app.json")));
        assert!(!safe_archive_path(Path::new("/tmp/app.json")));
        assert!(!safe_archive_path(Path::new("apps/./../app.json")));
    }

    #[test]
    fn limited_file_reads_reject_oversized_input() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("icon.png");
        fs::write(&path, b"12345").unwrap();

        assert_eq!(read_limited_file(&path, 5).unwrap(), b"12345");
        assert!(read_limited_file(&path, 4)
            .unwrap_err()
            .to_string()
            .contains("size limit"));
    }

    #[test]
    fn archive_budget_matches_restore_limits() {
        let mut size_budget = ArchiveBudget {
            files: 0,
            uncompressed_size: MAX_UNCOMPRESSED_SIZE,
        };
        assert!(size_budget.include(1).is_err());

        let mut file_budget = ArchiveBudget {
            files: MAX_ARCHIVE_FILES,
            uncompressed_size: 0,
        };
        assert!(file_budget.include(0).is_err());
    }

    #[test]
    fn archive_links_are_rejected_before_restore() {
        let temp = tempfile::tempdir().unwrap();
        let backup = temp.path().join("link.bastle-backup");
        let file = File::create(&backup).unwrap();
        let encoder = zstd::stream::Encoder::new(file, 1).unwrap();
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_mode(0o777);
        header.set_size(0);
        header.set_link_name("../../outside").unwrap();
        header.set_cksum();
        archive
            .append_data(&mut header, "profiles/abcdefghijkl/link", &[][..])
            .unwrap();
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap();

        let service = backup_service(temp.path());
        let error = service.prepare_restore(&backup, None).unwrap_err();
        assert!(error
            .to_string()
            .contains("links and special files are not allowed"));
    }

    #[test]
    fn future_manifests_and_runtime_lock_entries_are_rejected() {
        let app_id: AppId = "abcdefghijkl".parse().unwrap();
        let future = BackupManifestV1 {
            schema_version: BACKUP_SCHEMA_VERSION + 1,
            includes_site_data: false,
            apps: vec![BackupManifestAppV1 {
                id: app_id.clone(),
                title: "Future".to_owned(),
            }],
        };
        assert!(future.validate().is_err());

        let manifest = BackupManifestV1 {
            schema_version: BACKUP_SCHEMA_VERSION,
            includes_site_data: true,
            apps: vec![BackupManifestAppV1 {
                id: app_id.clone(),
                title: "Example".to_owned(),
            }],
        };
        let prefix = PathBuf::from("apps").join(app_id.as_str());
        let paths = vec![
            PathBuf::from(MANIFEST_PATH),
            prefix.join("app.json"),
            prefix.join("icon.png"),
            prefix.join("policy.json"),
            PathBuf::from("profiles")
                .join(app_id.as_str())
                .join(RUNTIME_LOCK_FILE),
        ];
        assert!(validate_archive_paths(&manifest, &paths).is_err());
    }

    #[test]
    fn restore_continues_after_one_launcher_is_denied() {
        let source = tempfile::tempdir().unwrap();
        let source_service = backup_service(source.path());
        let first = AppConfigV2::new("First", "https://first.example", 0).unwrap();
        let second = AppConfigV2::new("Second", "https://second.example", 1).unwrap();
        block_on(source_service.service.create(first.clone(), b"first", None)).unwrap();
        block_on(
            source_service
                .service
                .create(second.clone(), b"second", None),
        )
        .unwrap();
        let backup = source.path().join("partial.bastle-backup");
        source_service
            .create_backup(
                &backup,
                &[first.id.clone(), second.id.clone()],
                &BackupOptions::default(),
            )
            .unwrap();

        let target = tempfile::tempdir().unwrap();
        let launcher = FakeLauncher::default();
        launcher
            .denied_installs
            .borrow_mut()
            .insert(second.id.clone());
        let repository =
            AppRepository::new(target.path().join("data"), target.path().join("cache"));
        let target_service = BackupService::new(AppService::new(repository, launcher));
        let plan = target_service.prepare_restore(&backup, None).unwrap();
        let selected = HashSet::from([first.id.clone(), second.id.clone()]);
        let report = block_on(target_service.restore(plan, &selected, None));

        assert_eq!(report.restored, 1);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].source_id, second.id);
        assert!(target_service.service.contains(&first.id));
        assert!(!target_service.service.contains(&second.id));
    }
}
