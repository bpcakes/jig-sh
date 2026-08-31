use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::context::RepoContext;

use super::{SCHEDULE_SCHEMA_VERSION, ScheduleFile, migrate_schedule_schema, validate_schema};
use crate::runtime::loops::state::{
    LOOP_CACHE_DIR, LOOP_RUNTIME_DIR, read_json_if_exists_with_cancellation,
    with_exclusive_file_lock,
};

mod authority;
#[cfg(test)]
mod authority_tests;
#[cfg(test)]
mod tests;

use authority::{ProtectedScheduleAuthority, resolve_protected_schedule_authority};

const SCHEDULE_STATE_PATH: &str = ".agent/runtime/loop/schedule.json";
const SCHEDULE_INITIALIZATION_SCHEMA_VERSION: u32 = 1;
const PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION: u32 = 2;
const PROTECTED_SCHEDULE_STATE_PATH: &str = "jig/loop/schedule.json";

#[derive(Deserialize, Serialize)]
struct ScheduleInitializationMarker {
    schema_version: u32,
    state_path: String,
}

#[derive(Clone)]
pub(super) struct SchedulePersistence {
    dir: PathBuf,
    path: PathBuf,
    initialized_path: PathBuf,
    lock_path: PathBuf,
    legacy_dir: PathBuf,
    legacy_path: PathBuf,
    legacy_lock_path: PathBuf,
    protected_authority: std::result::Result<Option<ProtectedScheduleAuthority>, String>,
}

impl SchedulePersistence {
    pub(super) fn new(ctx: &RepoContext) -> Self {
        let dir = ctx.root().join(LOOP_RUNTIME_DIR);
        let legacy_dir = ctx.root().join(LOOP_CACHE_DIR);
        let protected_authority =
            resolve_protected_schedule_authority(ctx.root()).map_err(|error| format!("{error:#}"));
        Self {
            path: dir.join("schedule.json"),
            initialized_path: dir.join("schedule.initialized"),
            lock_path: dir.join("schedule.lock"),
            dir,
            legacy_path: legacy_dir.join("schedule.json"),
            legacy_lock_path: legacy_dir.join("schedule.lock"),
            legacy_dir,
            protected_authority,
        }
    }

    pub(super) fn read_only(&self, cancelled: &dyn Fn() -> bool) -> Result<ScheduleFile> {
        let durable_required = self.durable_state_expected()?;
        let durable = self.read_durable(durable_required, cancelled)?;
        let legacy_expected = path_exists(&self.legacy_path, "legacy loop schedule state")?;
        let legacy = read_expected_schedule(
            &self.legacy_path,
            legacy_expected,
            "Legacy loop schedule state disappeared before it could be read",
            cancelled,
        )?;

        self.resolve_read_only(durable, legacy, cancelled)
    }

    fn resolve_read_only(
        &self,
        durable: Option<ScheduleFile>,
        legacy: Option<ScheduleFile>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ScheduleFile> {
        match (durable, legacy) {
            (Some(durable), Some(mut legacy)) => {
                if legacy_is_migration_marker(&mut legacy, &self.legacy_path)? {
                    return Ok(durable);
                }
                migrate_schedule_schema(&mut legacy)?;
                if legacy != durable {
                    bail!(
                        "Loop schedule state exists at both {} and {}; stop older Jig runtimes and reconcile the files before dispatching",
                        self.path.display(),
                        self.legacy_path.display()
                    );
                }
                Ok(durable)
            }
            (Some(durable), None) => Ok(durable),
            (None, Some(mut legacy)) => {
                if legacy_is_migration_marker(&mut legacy, &self.legacy_path)? {
                    let durable_required = self.durable_state_expected()?;
                    if let Some(durable) = self.read_durable(durable_required, cancelled)? {
                        return Ok(durable);
                    }
                    bail!(
                        "Loop schedule migration marker exists without durable state at {}",
                        self.path.display()
                    );
                }
                migrate_schedule_schema(&mut legacy)?;
                Ok(legacy)
            }
            (None, None) => Ok(ScheduleFile::default()),
        }
    }

    pub(super) fn with_locked<T>(
        &self,
        action: impl FnOnce(&mut ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.with_legacy_migration_lock(|| {
            let durable_required = self.durable_state_expected()?;
            let (authority_dir, authority_lock) = self.authority_lock()?;
            ensure_durable_directory(authority_dir)?;
            ensure_durable_directory(&self.dir)?;
            let result = with_exclusive_file_lock(authority_dir, authority_lock, || {
                remove_orphaned_schedule_temps(authority_dir)?;
                remove_orphaned_schedule_temps(&self.dir)?;
                let durable = self.read_durable(durable_required, &|| false)?;
                let mut store = durable.unwrap_or_default();
                if !durable_required || self.protected_authority_needs_state()? {
                    validate_durable_schedule(&store, &self.path)?;
                    self.write_durable_schedule(&store)?;
                }
                self.ensure_initialization_markers()?;
                self.write_legacy_marker()?;
                let result = action(&mut store)?;
                validate_durable_schedule(&store, &self.path)?;
                self.write_durable_schedule(&store)?;
                Ok(result)
            })?;
            Ok(result)
        })
    }

    pub(super) fn read_locked<T>(
        &self,
        action: impl FnOnce(&ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.with_legacy_migration_lock(|| {
            let durable_required = self.durable_state_expected()?;
            let (authority_dir, authority_lock) = self.authority_lock()?;
            ensure_durable_directory(authority_dir)?;
            ensure_durable_directory(&self.dir)?;
            let (result, durable_exists) =
                with_exclusive_file_lock(authority_dir, authority_lock, || {
                    remove_orphaned_schedule_temps(authority_dir)?;
                    remove_orphaned_schedule_temps(&self.dir)?;
                    let store = self.read_durable(durable_required, &|| false)?;
                    let durable_exists = store.is_some();
                    if let Some(store) = store.as_ref() {
                        if self.protected_authority_needs_state()? {
                            self.write_durable_schedule(store)?;
                        }
                        self.ensure_initialization_markers()?;
                    }
                    let store = store.unwrap_or_default();
                    Ok((action(&store)?, durable_exists))
                })?;
            if durable_exists {
                self.write_legacy_marker()?;
            }
            Ok(result)
        })
    }

    fn with_legacy_migration_lock<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        ensure_durable_directory(&self.legacy_dir)?;
        with_exclusive_file_lock(&self.legacy_dir, &self.legacy_lock_path, || {
            remove_orphaned_schedule_temps(&self.legacy_dir)?;
            self.ensure_legacy_migrated_locked()?;
            action()
        })
    }

    fn ensure_legacy_migrated_locked(&self) -> Result<()> {
        let legacy_expected = path_exists(&self.legacy_path, "legacy loop schedule state")?;
        let Some(mut legacy) = read_expected_schedule(
            &self.legacy_path,
            legacy_expected,
            "Legacy loop schedule state disappeared before migration",
            &|| false,
        )?
        else {
            if let Some(store) = self.read_durable(false, &|| false)? {
                if self.protected_authority_needs_state()? {
                    self.write_durable_schedule(&store)?;
                }
                self.ensure_initialization_markers()?;
                self.write_legacy_marker()?;
            }
            return Ok(());
        };
        let marker_schema = legacy.schema_version;
        if legacy_is_migration_marker(&mut legacy, &self.legacy_path)? {
            let durable_required = self.durable_state_expected()?;
            let Some(store) = self.read_durable(durable_required, &|| false)? else {
                bail!(
                    "Loop schedule migration marker exists without durable state at {}",
                    self.path.display()
                );
            };
            if self.protected_authority_needs_state()? {
                self.write_durable_schedule(&store)?;
            }
            self.ensure_initialization_markers()?;
            if marker_schema != legacy.schema_version {
                write_json_durable(&self.legacy_path, &legacy)?;
            }
            return Ok(());
        }
        migrate_schedule_schema(&mut legacy)?;

        let durable_required = self.durable_state_expected()?;
        if let Some(current) = self.read_durable(durable_required, &|| false)? {
            if current != legacy {
                bail!(
                    "Loop schedule state exists at both {} and {}; stop older Jig runtimes and reconcile the files before dispatching",
                    self.path.display(),
                    self.legacy_path.display()
                );
            }
            if self.protected_authority_needs_state()? {
                self.write_durable_schedule(&current)?;
            }
        } else {
            self.write_durable_schedule(&legacy)?;
        }
        self.ensure_initialization_markers()?;
        self.write_legacy_marker()
    }

    fn write_legacy_marker(&self) -> Result<()> {
        let marker = ScheduleFile {
            schema_version: SCHEDULE_SCHEMA_VERSION,
            migrated_to: Some(SCHEDULE_STATE_PATH.to_string()),
            occurrences: BTreeMap::new(),
        };
        if let Some(mut existing) =
            read_json_if_exists_with_cancellation::<ScheduleFile>(&self.legacy_path, &|| false)?
            && legacy_is_migration_marker(&mut existing, &self.legacy_path)?
            && existing == marker
        {
            return Ok(());
        }
        write_json_durable(&self.legacy_path, &marker)
    }

    fn durable_state_expected(&self) -> Result<bool> {
        let public_marker = path_exists(
            &self.initialized_path,
            "loop schedule initialization marker",
        )?;
        let durable_state = path_exists(&self.path, "loop schedule state")?;
        let (protected_state, protected_marker) = match self.protected_authority()? {
            Some(authority) => (
                path_exists(&authority.path, "protected loop schedule state")?,
                path_exists(
                    &authority.initialized_path,
                    "protected loop schedule initialization marker",
                )?,
            ),
            None => (false, false),
        };
        Ok(public_marker || durable_state || protected_state || protected_marker)
    }

    fn read_durable(
        &self,
        required: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<ScheduleFile>> {
        let required = required || self.durable_state_expected()?;
        let path = self.durable_read_path()?;
        let store = read_expected_schedule(
            path,
            required,
            "Initialized loop schedule state is missing",
            cancelled,
        )?;
        store
            .map(|mut store| {
                migrate_schedule_schema(&mut store)?;
                validate_durable_schedule(&store, path)?;
                Ok(store)
            })
            .transpose()
    }

    fn ensure_initialization_markers(&self) -> Result<()> {
        let protected = match self.protected_authority()? {
            Some(authority) if path_exists(&authority.path, "protected loop schedule state")? => {
                Some(authority)
            }
            _ => None,
        };
        if let Some(authority) = protected {
            self.ensure_initialization_marker_at(
                &authority.initialized_path,
                "protected loop schedule initialization marker",
                PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
                PROTECTED_SCHEDULE_STATE_PATH,
                false,
            )?;
        }
        self.ensure_initialization_marker_at(
            &self.initialized_path,
            "loop schedule initialization marker",
            SCHEDULE_INITIALIZATION_SCHEMA_VERSION,
            SCHEDULE_STATE_PATH,
            protected.is_some(),
        )?;
        Ok(())
    }

    fn ensure_initialization_marker_at(
        &self,
        path: &Path,
        description: &str,
        schema_version: u32,
        state_path: &str,
        replace_existing: bool,
    ) -> Result<()> {
        let expected = ScheduleInitializationMarker {
            schema_version,
            state_path: state_path.to_string(),
        };
        let current =
            read_json_if_exists_with_cancellation::<ScheduleInitializationMarker>(path, &|| false);
        if replace_existing {
            if current
                .as_ref()
                .ok()
                .and_then(Option::as_ref)
                .is_some_and(|marker| {
                    marker.schema_version == expected.schema_version
                        && marker.state_path == expected.state_path
                })
            {
                return Ok(());
            }
            return write_json_durable(path, &expected);
        }
        let current = current?;
        if current.as_ref().is_some_and(|marker| {
            marker.schema_version == expected.schema_version
                && marker.state_path == expected.state_path
        }) {
            return Ok(());
        }
        let upgrading_protected_witness = current.as_ref().is_some_and(|marker| {
            schema_version == PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION
                && marker.schema_version == SCHEDULE_INITIALIZATION_SCHEMA_VERSION
                && marker.state_path == SCHEDULE_STATE_PATH
        });
        if current.is_some() && !upgrading_protected_witness {
            bail!("Invalid {description} at {}", path.display());
        }
        write_json_durable(path, &expected)
    }

    fn protected_authority(&self) -> Result<Option<&ProtectedScheduleAuthority>> {
        self.protected_authority
            .as_ref()
            .map(Option::as_ref)
            .map_err(|error| anyhow::anyhow!(error.clone()))
    }

    fn protected_marker_requires_authority(&self) -> Result<bool> {
        let Some(authority) = self.protected_authority()? else {
            return Ok(false);
        };
        let Some(marker) = read_json_if_exists_with_cancellation::<ScheduleInitializationMarker>(
            &authority.initialized_path,
            &|| false,
        )?
        else {
            return Ok(false);
        };
        match (marker.schema_version, marker.state_path.as_str()) {
            (SCHEDULE_INITIALIZATION_SCHEMA_VERSION, SCHEDULE_STATE_PATH) => Ok(false),
            (PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION, PROTECTED_SCHEDULE_STATE_PATH) => {
                Ok(true)
            }
            _ => bail!(
                "Invalid protected loop schedule initialization marker at {}",
                authority.initialized_path.display()
            ),
        }
    }

    fn durable_read_path(&self) -> Result<&Path> {
        if let Some(authority) = self.protected_authority()?
            && (path_exists(&authority.path, "protected loop schedule state")?
                || self.protected_marker_requires_authority()?)
        {
            return Ok(&authority.path);
        }
        Ok(&self.path)
    }

    fn protected_authority_needs_state(&self) -> Result<bool> {
        self.protected_authority()?
            .map(|authority| path_exists(&authority.path, "protected loop schedule state"))
            .transpose()
            .map(|exists| exists == Some(false))
    }

    fn authority_lock(&self) -> Result<(&Path, &Path)> {
        Ok(match self.protected_authority()? {
            Some(authority) => (&authority.dir, &authority.lock_path),
            None => (&self.dir, &self.lock_path),
        })
    }

    fn write_durable_schedule(&self, store: &ScheduleFile) -> Result<()> {
        write_json_durable(&self.path, store)?;
        if let Some(authority) = self.protected_authority()? {
            write_json_durable(&authority.path, store)?;
            self.ensure_initialization_marker_at(
                &authority.initialized_path,
                "protected loop schedule initialization marker",
                PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
                PROTECTED_SCHEDULE_STATE_PATH,
                false,
            )?;
        }
        Ok(())
    }
}

fn path_exists(path: &Path, description: &str) -> Result<bool> {
    path.try_exists()
        .with_context(|| format!("Failed to inspect {description} {}", path.display()))
}

fn read_expected_schedule(
    path: &Path,
    expected: bool,
    missing_message: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<ScheduleFile>> {
    let schedule = read_json_if_exists_with_cancellation::<ScheduleFile>(path, cancelled)?;
    if expected && schedule.is_none() {
        bail!("{missing_message} at {}", path.display());
    }
    Ok(schedule)
}

fn validate_durable_schedule(store: &ScheduleFile, path: &Path) -> Result<()> {
    validate_schema(store)?;
    if store.migrated_to.is_some() {
        bail!(
            "Invalid durable loop schedule migration marker at {}: migrated_to is reserved for the legacy cache path",
            path.display()
        );
    }
    Ok(())
}

fn remove_orphaned_schedule_temps(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| {
        format!(
            "Failed to inspect loop schedule directory {}",
            dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "Failed to inspect an entry in loop schedule directory {}",
                dir.display()
            )
        })?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("schedule.tmp-"))
        {
            continue;
        }
        let file_type = entry.file_type().with_context(|| {
            format!(
                "Failed to inspect orphan schedule temp {}",
                entry.path().display()
            )
        })?;
        if file_type.is_file() || file_type.is_symlink() {
            fs::remove_file(entry.path()).with_context(|| {
                format!(
                    "Failed to remove orphan schedule temp {}",
                    entry.path().display()
                )
            })?;
        }
    }
    Ok(())
}

fn legacy_is_migration_marker(store: &mut ScheduleFile, path: &Path) -> Result<bool> {
    let Some(target) = store.migrated_to.as_deref() else {
        return Ok(false);
    };
    if target != SCHEDULE_STATE_PATH {
        bail!(
            "Invalid legacy loop schedule migration marker at {}: unexpected target {target}",
            path.display()
        );
    }
    migrate_schedule_schema(store)?;
    validate_schema(store)?;
    if !store.occurrences.is_empty() {
        bail!(
            "Invalid legacy loop schedule migration marker at {}: occurrences must be empty",
            path.display()
        );
    }
    Ok(true)
}

fn write_json_durable<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let Some(parent) = path.parent() else {
        bail!("Loop schedule state path has no parent: {}", path.display());
    };
    ensure_durable_directory(parent)?;
    let tmp = path.with_extension(format!("tmp-{}", Ulid::new()));
    let bytes = serde_json::to_vec_pretty(value).context("Failed to encode loop schedule JSON")?;
    let result = publish_durable(
        || {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .with_context(|| format!("Failed to create {}", tmp.display()))?;
            file.write_all(&bytes)
                .with_context(|| format!("Failed to write {}", tmp.display()))?;
            file.sync_all()
                .with_context(|| format!("Failed to sync {}", tmp.display()))
        },
        || {
            fs::rename(&tmp, path).with_context(|| {
                format!(
                    "Failed to replace loop schedule state {} with {}",
                    path.display(),
                    tmp.display()
                )
            })
        },
        || sync_parent_directory(parent),
    );
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn ensure_durable_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => return Ok(()),
        Ok(_) => bail!(
            "Loop schedule directory path is not a directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect loop schedule directory {}",
                    path.display()
                )
            });
        }
    }

    let Some(parent) = path.parent() else {
        bail!("Loop schedule directory has no parent: {}", path.display());
    };
    ensure_durable_directory(parent)?;
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(path)
                .with_context(|| {
                    format!(
                        "Failed to inspect loop schedule directory {}",
                        path.display()
                    )
                })?
                .is_dir()
            {
                bail!(
                    "Loop schedule directory path is not a directory: {}",
                    path.display()
                );
            }
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to create loop schedule directory {}",
                    path.display()
                )
            });
        }
    }
    sync_parent_directory(parent)
}

fn publish_durable(
    write_and_sync_temp: impl FnOnce() -> Result<()>,
    replace: impl FnOnce() -> Result<()>,
    sync_parent: impl FnOnce() -> Result<()>,
) -> Result<()> {
    write_and_sync_temp()?;
    replace()?;
    sync_parent()
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> Result<()> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| {
            format!(
                "Failed to sync loop schedule directory {}",
                parent.display()
            )
        })
}

#[cfg(not(unix))]
fn sync_parent_directory(_: &Path) -> Result<()> {
    Ok(())
}
