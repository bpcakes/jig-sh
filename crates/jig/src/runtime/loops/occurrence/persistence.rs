use std::collections::BTreeMap;
use std::ffi::OsStr;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::context::RepoContext;
#[cfg(test)]
use crate::runtime::loops::managed_path::ensure_managed_directory;

use super::{SCHEDULE_SCHEMA_VERSION, ScheduleFile, migrate_schedule_schema, validate_schema};
use crate::runtime::loops::state::{
    LOOP_CACHE_DIR, LOOP_RUNTIME_DIR, StateDirectory, loop_state_lock_deadline,
};
#[cfg(test)]
use crate::runtime::loops::state::{cache_file_name, read_json_if_exists_with_cancellation};

mod authority;
#[cfg(test)]
mod authority_tests;
mod directory;
#[cfg(test)]
mod tests;

use authority::{ProtectedScheduleAuthority, resolve_protected_schedule_authority};
use directory::ScheduleDirectories;

const SCHEDULE_STATE_PATH: &str = ".agent/runtime/loop/schedule.json";
const SCHEDULE_INITIALIZATION_SCHEMA_VERSION: u32 = 1;
const PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION: u32 = 2;
const PROTECTED_SCHEDULE_STATE_PATH: &str = "jig/loop/schedule.json";
const SCHEDULE_FILE_NAME: &str = "schedule.json";
const SCHEDULE_INITIALIZED_FILE_NAME: &str = "schedule.initialized";
const SCHEDULE_LOCK_FILE_NAME: &str = "schedule.lock";

#[derive(Deserialize, Serialize)]
struct ScheduleInitializationMarker {
    schema_version: u32,
    state_path: String,
}

#[derive(Clone)]
pub(super) struct SchedulePersistence {
    root: PathBuf,
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
            root: ctx.root().to_path_buf(),
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
        let directories = ScheduleDirectories::open(self, false)?;
        let durable_required = self.durable_state_expected(&directories)?;
        let durable = self.read_durable(&directories, durable_required, cancelled)?;
        let legacy_expected = location_exists(
            directories.legacy.as_ref(),
            &self.legacy_path,
            SCHEDULE_FILE_NAME,
        )?;
        let legacy = read_expected_schedule(
            directories.legacy.as_ref(),
            &self.legacy_path,
            SCHEDULE_FILE_NAME,
            legacy_expected,
            "Legacy loop schedule state disappeared before it could be read",
            cancelled,
        )?;

        self.resolve_read_only(&directories, durable, legacy, cancelled)
    }

    fn resolve_read_only(
        &self,
        directories: &ScheduleDirectories,
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
                    let durable_required = self.durable_state_expected(directories)?;
                    if let Some(durable) =
                        self.read_durable(directories, durable_required, cancelled)?
                    {
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
        self.with_locked_until(loop_state_lock_deadline(), action)
    }

    pub(super) fn with_locked_with_cancellation<T>(
        &self,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&mut ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.with_locked_until_with_cancellation(loop_state_lock_deadline(), cancelled, action)
    }

    pub(super) fn with_locked_until<T>(
        &self,
        deadline: Instant,
        action: impl FnOnce(&mut ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.with_locked_until_with_cancellation(deadline, &|| false, action)
    }

    fn with_locked_until_with_cancellation<T>(
        &self,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&mut ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.with_schedule_locks_until(deadline, cancelled, |directories| {
            let durable_required = self.durable_state_expected(directories)?;
            let durable = self.read_durable(directories, durable_required, &|| false)?;
            let mut store = durable.unwrap_or_default();
            if !durable_required || self.protected_authority_needs_state(directories)? {
                validate_durable_schedule(&store, &self.path)?;
                self.write_durable_schedule(directories, &store)?;
            }
            self.ensure_initialization_markers(directories)?;
            self.write_legacy_marker(directories)?;
            let result = action(&mut store)?;
            validate_durable_schedule(&store, &self.path)?;
            self.write_durable_schedule(directories, &store)?;
            Ok(result)
        })
    }

    pub(super) fn with_locked_compensating<T, U>(
        &self,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&mut ScheduleFile) -> Result<T>,
        after_commit: impl FnOnce(&T, Instant) -> Result<U>,
    ) -> Result<(T, U)> {
        let deadline = loop_state_lock_deadline();
        self.with_locked_compensating_until(deadline, cancelled, action, |result| {
            after_commit(result, deadline)
        })
    }

    fn with_locked_compensating_until<T, U>(
        &self,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&mut ScheduleFile) -> Result<T>,
        after_commit: impl FnOnce(&T) -> Result<U>,
    ) -> Result<(T, U)> {
        self.with_schedule_locks_until(deadline, cancelled, |directories| {
            let durable_required = self.durable_state_expected(directories)?;
            let durable = self.read_durable(directories, durable_required, &|| false)?;
            let mut store = durable.unwrap_or_default();
            if !durable_required || self.protected_authority_needs_state(directories)? {
                validate_durable_schedule(&store, &self.path)?;
                self.write_durable_schedule(directories, &store)?;
            }
            self.ensure_initialization_markers(directories)?;
            self.write_legacy_marker(directories)?;
            let rollback = store.clone();
            let result = action(&mut store)?;
            validate_durable_schedule(&store, &self.path)?;
            self.write_durable_schedule(directories, &store)?;
            compensate_after_commit(result, after_commit, || {
                self.write_durable_schedule(directories, &rollback)
            })
        })
    }

    pub(super) fn read_locked<T>(
        &self,
        action: impl FnOnce(&ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.with_schedule_locks_until(loop_state_lock_deadline(), &|| false, |directories| {
            let durable_required = self.durable_state_expected(directories)?;
            let store = self.read_durable(directories, durable_required, &|| false)?;
            let durable_exists = store.is_some();
            if let Some(store) = store.as_ref() {
                if self.protected_authority_needs_state(directories)? {
                    self.write_durable_schedule(directories, store)?;
                }
                self.ensure_initialization_markers(directories)?;
            }
            let store = store.unwrap_or_default();
            let result = action(&store)?;
            if durable_exists {
                self.write_legacy_marker(directories)?;
            }
            Ok(result)
        })
    }

    fn with_schedule_locks_until<T>(
        &self,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&ScheduleDirectories) -> Result<T>,
    ) -> Result<T> {
        let directories = ScheduleDirectories::open(self, true)?;
        let legacy = directories.legacy()?;
        let authority = directories.authority().ok_or_else(|| {
            anyhow::anyhow!("Authoritative loop schedule directory is unavailable")
        })?;
        legacy.with_lock_until(
            OsStr::new(SCHEDULE_LOCK_FILE_NAME),
            &self.legacy_lock_path,
            deadline,
            cancelled,
            || {
                legacy.reclaim_orphaned_temps(OsStr::new(SCHEDULE_FILE_NAME), &self.legacy_path)?;
                authority.with_lock_until(
                    OsStr::new(SCHEDULE_LOCK_FILE_NAME),
                    self.authority_lock_path()?,
                    deadline,
                    cancelled,
                    || {
                        authority.reclaim_orphaned_temps(
                            OsStr::new(SCHEDULE_FILE_NAME),
                            self.authority_state_path()?,
                        )?;
                        directories
                            .public()?
                            .reclaim_orphaned_temps(OsStr::new(SCHEDULE_FILE_NAME), &self.path)?;
                        // Keep one lock order for every mutating access: the legacy cache lock
                        // first, then the protected authority lock. Migration may publish the
                        // authoritative ledger, so it belongs inside both locks.
                        self.ensure_legacy_migrated_locked(&directories)?;
                        action(&directories)
                    },
                )
            },
        )
    }

    fn ensure_legacy_migrated_locked(&self, directories: &ScheduleDirectories) -> Result<()> {
        let legacy_expected = location_exists(
            directories.legacy.as_ref(),
            &self.legacy_path,
            SCHEDULE_FILE_NAME,
        )?;
        let Some(mut legacy) = read_expected_schedule(
            directories.legacy.as_ref(),
            &self.legacy_path,
            SCHEDULE_FILE_NAME,
            legacy_expected,
            "Legacy loop schedule state disappeared before migration",
            &|| false,
        )?
        else {
            if let Some(store) = self.read_durable(directories, false, &|| false)? {
                if self.protected_authority_needs_state(directories)? {
                    self.write_durable_schedule(directories, &store)?;
                }
                self.ensure_initialization_markers(directories)?;
                self.write_legacy_marker(directories)?;
            }
            return Ok(());
        };
        let marker_schema = legacy.schema_version;
        if legacy_is_migration_marker(&mut legacy, &self.legacy_path)? {
            let durable_required = self.durable_state_expected(directories)?;
            let Some(store) = self.read_durable(directories, durable_required, &|| false)? else {
                bail!(
                    "Loop schedule migration marker exists without durable state at {}",
                    self.path.display()
                );
            };
            if self.protected_authority_needs_state(directories)? {
                self.write_durable_schedule(directories, &store)?;
            }
            self.ensure_initialization_markers(directories)?;
            if marker_schema != legacy.schema_version {
                write_location(
                    directories.legacy()?,
                    &self.legacy_path,
                    SCHEDULE_FILE_NAME,
                    &legacy,
                )?;
            }
            return Ok(());
        }
        migrate_schedule_schema(&mut legacy)?;

        let durable_required = self.durable_state_expected(directories)?;
        if let Some(current) = self.read_durable(directories, durable_required, &|| false)? {
            if current != legacy {
                bail!(
                    "Loop schedule state exists at both {} and {}; stop older Jig runtimes and reconcile the files before dispatching",
                    self.path.display(),
                    self.legacy_path.display()
                );
            }
            if self.protected_authority_needs_state(directories)? {
                self.write_durable_schedule(directories, &current)?;
            }
        } else {
            self.write_durable_schedule(directories, &legacy)?;
        }
        self.ensure_initialization_markers(directories)?;
        self.write_legacy_marker(directories)
    }

    fn write_legacy_marker(&self, directories: &ScheduleDirectories) -> Result<()> {
        let marker = ScheduleFile {
            schema_version: SCHEDULE_SCHEMA_VERSION,
            migrated_to: Some(SCHEDULE_STATE_PATH.to_string()),
            occurrences: BTreeMap::new(),
        };
        let existing = read_location::<ScheduleFile>(
            directories.legacy.as_ref(),
            &self.legacy_path,
            SCHEDULE_FILE_NAME,
            &|| false,
        )?;
        if let Some(mut existing) = existing
            && legacy_is_migration_marker(&mut existing, &self.legacy_path)?
            && existing == marker
        {
            return Ok(());
        }
        write_location(
            directories.legacy()?,
            &self.legacy_path,
            SCHEDULE_FILE_NAME,
            &marker,
        )
    }

    fn durable_state_expected(&self, directories: &ScheduleDirectories) -> Result<bool> {
        let (protected_state, protected_marker) = match self.protected_authority()? {
            Some(authority) => (
                location_exists(
                    directories.protected.as_ref(),
                    &authority.path,
                    SCHEDULE_FILE_NAME,
                )?,
                location_exists(
                    directories.protected.as_ref(),
                    &authority.initialized_path,
                    SCHEDULE_INITIALIZED_FILE_NAME,
                )?,
            ),
            None => (false, false),
        };
        if protected_state || protected_marker {
            return Ok(true);
        }
        let public_marker = location_exists(
            directories.public.as_ref(),
            &self.initialized_path,
            SCHEDULE_INITIALIZED_FILE_NAME,
        )?;
        let durable_state =
            location_exists(directories.public.as_ref(), &self.path, SCHEDULE_FILE_NAME)?;
        Ok(public_marker || durable_state)
    }

    fn read_durable(
        &self,
        directories: &ScheduleDirectories,
        required: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<ScheduleFile>> {
        let required = required || self.durable_state_expected(directories)?;
        let (directory, path) = self.durable_read_location(directories)?;
        let store = read_expected_schedule(
            directory,
            path,
            SCHEDULE_FILE_NAME,
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

    fn ensure_initialization_markers(&self, directories: &ScheduleDirectories) -> Result<()> {
        let protected = match self.protected_authority()? {
            Some(authority)
                if location_exists(
                    directories.protected.as_ref(),
                    &authority.path,
                    SCHEDULE_FILE_NAME,
                )? =>
            {
                Some(authority)
            }
            _ => None,
        };
        if let Some(authority) = protected {
            self.ensure_initialization_marker_at(
                directories.protected.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Protected loop schedule directory is unavailable")
                })?,
                &authority.initialized_path,
                "protected loop schedule initialization marker",
                PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
                PROTECTED_SCHEDULE_STATE_PATH,
                false,
            )?;
        }
        let public_marker = self.ensure_initialization_marker_at(
            directories.public()?,
            &self.initialized_path,
            "loop schedule initialization marker",
            SCHEDULE_INITIALIZATION_SCHEMA_VERSION,
            SCHEDULE_STATE_PATH,
            protected.is_some(),
        );
        if protected.is_some() {
            // The checkout marker is a repairable replica once protected authority exists.
            // A later authoritative access retries this publication.
            return Ok(());
        }
        public_marker
    }

    fn ensure_initialization_marker_at(
        &self,
        directory: &StateDirectory,
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
        let current = directory.read_json::<ScheduleInitializationMarker>(
            OsStr::new(SCHEDULE_INITIALIZED_FILE_NAME),
            path,
            &|| false,
        );
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
            return write_location(directory, path, SCHEDULE_INITIALIZED_FILE_NAME, &expected);
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
        write_location(directory, path, SCHEDULE_INITIALIZED_FILE_NAME, &expected)
    }

    fn protected_authority(&self) -> Result<Option<&ProtectedScheduleAuthority>> {
        self.protected_authority
            .as_ref()
            .map(Option::as_ref)
            .map_err(|error| anyhow::anyhow!(error.clone()))
    }

    fn protected_marker_requires_authority(
        &self,
        directories: &ScheduleDirectories,
    ) -> Result<bool> {
        let Some(authority) = self.protected_authority()? else {
            return Ok(false);
        };
        let Some(marker) = read_location::<ScheduleInitializationMarker>(
            directories.protected.as_ref(),
            &authority.initialized_path,
            SCHEDULE_INITIALIZED_FILE_NAME,
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

    fn durable_read_location<'a>(
        &'a self,
        directories: &'a ScheduleDirectories,
    ) -> Result<(Option<&'a StateDirectory>, &'a Path)> {
        if let Some(authority) = self.protected_authority()?
            && (location_exists(
                directories.protected.as_ref(),
                &authority.path,
                SCHEDULE_FILE_NAME,
            )? || self.protected_marker_requires_authority(directories)?)
        {
            return Ok((directories.protected.as_ref(), &authority.path));
        }
        Ok((directories.public.as_ref(), &self.path))
    }

    fn protected_authority_needs_state(&self, directories: &ScheduleDirectories) -> Result<bool> {
        self.protected_authority()?
            .map(|authority| {
                location_exists(
                    directories.protected.as_ref(),
                    &authority.path,
                    SCHEDULE_FILE_NAME,
                )
            })
            .transpose()
            .map(|exists| exists == Some(false))
    }

    fn authority_lock_path(&self) -> Result<&Path> {
        Ok(self
            .protected_authority()?
            .map_or(self.lock_path.as_path(), |authority| {
                authority.lock_path.as_path()
            }))
    }

    fn authority_state_path(&self) -> Result<&Path> {
        Ok(self
            .protected_authority()?
            .map_or(self.path.as_path(), |authority| authority.path.as_path()))
    }

    fn write_durable_schedule(
        &self,
        directories: &ScheduleDirectories,
        store: &ScheduleFile,
    ) -> Result<()> {
        if let Some(authority) = self.protected_authority()? {
            let protected = directories.protected.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Protected loop schedule directory is unavailable")
            })?;
            write_location(protected, &authority.path, SCHEDULE_FILE_NAME, store)?;
            self.ensure_initialization_marker_at(
                protected,
                &authority.initialized_path,
                "protected loop schedule initialization marker",
                PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
                PROTECTED_SCHEDULE_STATE_PATH,
                false,
            )?;
            // Protected Git metadata is the commit point. The checkout copy is a
            // compatibility/diagnostic replica and is repaired on later writes.
            let _ = write_location(directories.public()?, &self.path, SCHEDULE_FILE_NAME, store);
            return Ok(());
        }
        write_location(directories.public()?, &self.path, SCHEDULE_FILE_NAME, store)
    }
}

fn compensate_after_commit<T, U>(
    result: T,
    after_commit: impl FnOnce(&T) -> Result<U>,
    rollback: impl FnOnce() -> Result<()>,
) -> Result<(T, U)> {
    match after_commit(&result) {
        Ok(effect) => Ok((result, effect)),
        Err(error) if crate::state::receipt_append_may_have_landed(&error) => Err(error.context(
            "Committed loop schedule state was retained because its receipt append may have landed",
        )),
        Err(error) => match rollback() {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "Failed to roll back committed loop schedule state: {rollback_error:#}"
            ))),
        },
    }
}

fn location_exists(directory: Option<&StateDirectory>, path: &Path, name: &str) -> Result<bool> {
    directory
        .map(|directory| directory.exists(OsStr::new(name), path))
        .transpose()
        .map(|exists| exists.unwrap_or(false))
}

fn read_location<T: serde::de::DeserializeOwned>(
    directory: Option<&StateDirectory>,
    path: &Path,
    name: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<T>> {
    directory
        .map(|directory| directory.read_json(OsStr::new(name), path, cancelled))
        .transpose()
        .map(Option::flatten)
}

fn write_location<T: Serialize>(
    directory: &StateDirectory,
    path: &Path,
    name: &str,
    value: &T,
) -> Result<()> {
    directory.write_json_durable(OsStr::new(name), path, value)
}

fn read_expected_schedule(
    directory: Option<&StateDirectory>,
    path: &Path,
    name: &str,
    expected: bool,
    missing_message: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<ScheduleFile>> {
    let schedule = read_location(directory, path, name, cancelled)?;
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

#[cfg(test)]
fn write_json_durable<T: Serialize>(root: &Path, path: &Path, value: &T) -> Result<()> {
    let Some(parent) = path.parent() else {
        bail!("Loop schedule state path has no parent: {}", path.display());
    };
    let directory = StateDirectory::open(root, parent)?;
    let name = cache_file_name(parent, path)?;
    directory.write_json_durable(&name, path, value)
}

#[cfg(test)]
fn publish_durable(
    write_and_sync_temp: impl FnOnce() -> Result<()>,
    replace: impl FnOnce() -> Result<()>,
    sync_parent: impl FnOnce() -> Result<()>,
) -> Result<()> {
    write_and_sync_temp()?;
    replace()?;
    sync_parent()
}
