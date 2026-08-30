use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use ulid::Ulid;

use crate::context::RepoContext;

use super::{SCHEDULE_SCHEMA_VERSION, ScheduleFile, migrate_schedule_schema, validate_schema};
use crate::runtime::loops::state::{
    LOOP_CACHE_DIR, LOOP_RUNTIME_DIR, read_json_if_exists_with_cancellation,
    with_exclusive_file_lock,
};

const SCHEDULE_STATE_PATH: &str = ".agent/runtime/loop/schedule.json";
const SCHEDULE_INITIALIZATION_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct ScheduleInitializationMarker<'a> {
    schema_version: u32,
    state_path: &'a str,
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
}

impl SchedulePersistence {
    pub(super) fn new(ctx: &RepoContext) -> Self {
        let dir = ctx.root().join(LOOP_RUNTIME_DIR);
        let legacy_dir = ctx.root().join(LOOP_CACHE_DIR);
        Self {
            path: dir.join("schedule.json"),
            initialized_path: dir.join("schedule.initialized"),
            lock_path: dir.join("schedule.lock"),
            dir,
            legacy_path: legacy_dir.join("schedule.json"),
            legacy_lock_path: legacy_dir.join("schedule.lock"),
            legacy_dir,
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
            ensure_durable_directory(&self.dir)?;
            let result = with_exclusive_file_lock(&self.dir, &self.lock_path, || {
                remove_orphaned_schedule_temps(&self.dir)?;
                let mut store = self
                    .read_durable(durable_required, &|| false)?
                    .unwrap_or_default();
                let result = action(&mut store)?;
                validate_durable_schedule(&store, &self.path)?;
                write_json_durable(&self.path, &store)?;
                self.ensure_initialization_marker()?;
                Ok(result)
            })?;
            self.write_legacy_marker()?;
            Ok(result)
        })
    }

    pub(super) fn read_locked<T>(
        &self,
        action: impl FnOnce(&ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.with_legacy_migration_lock(|| {
            let durable_required = self.durable_state_expected()?;
            ensure_durable_directory(&self.dir)?;
            let (result, durable_exists) =
                with_exclusive_file_lock(&self.dir, &self.lock_path, || {
                    remove_orphaned_schedule_temps(&self.dir)?;
                    let store = self.read_durable(durable_required, &|| false)?;
                    let durable_exists = store.is_some();
                    if durable_exists {
                        self.ensure_initialization_marker()?;
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
            if self.read_durable(false, &|| false)?.is_some() {
                self.ensure_initialization_marker()?;
                self.write_legacy_marker()?;
            }
            return Ok(());
        };
        let marker_schema = legacy.schema_version;
        if legacy_is_migration_marker(&mut legacy, &self.legacy_path)? {
            let durable_required = self.durable_state_expected()?;
            let Some(_) = self.read_durable(durable_required, &|| false)? else {
                bail!(
                    "Loop schedule migration marker exists without durable state at {}",
                    self.path.display()
                );
            };
            self.ensure_initialization_marker()?;
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
        } else {
            write_json_durable(&self.path, &legacy)?;
        }
        self.ensure_initialization_marker()?;
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
        Ok(path_exists(
            &self.initialized_path,
            "loop schedule initialization marker",
        )? || path_exists(&self.path, "loop schedule state")?)
    }

    fn read_durable(
        &self,
        required: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<ScheduleFile>> {
        let required = required || self.durable_state_expected()?;
        let store = read_expected_schedule(
            &self.path,
            required,
            "Initialized loop schedule state is missing",
            cancelled,
        )?;
        store
            .map(|mut store| {
                migrate_schedule_schema(&mut store)?;
                validate_durable_schedule(&store, &self.path)?;
                Ok(store)
            })
            .transpose()
    }

    fn ensure_initialization_marker(&self) -> Result<()> {
        if path_exists(
            &self.initialized_path,
            "loop schedule initialization marker",
        )? {
            return Ok(());
        }
        write_json_durable(
            &self.initialized_path,
            &ScheduleInitializationMarker {
                schema_version: SCHEDULE_INITIALIZATION_SCHEMA_VERSION,
                state_path: SCHEDULE_STATE_PATH,
            },
        )
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
    match fs::metadata(path) {
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
            if !fs::metadata(path)
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::context::RepoContext;
    use crate::runtime::loops::state::read_json_or_default;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn durable_publish_orders_sync_replace_and_directory_sync() {
        let steps = RefCell::new(Vec::new());

        publish_durable(
            || {
                steps.borrow_mut().push("sync_file");
                Ok(())
            },
            || {
                steps.borrow_mut().push("rename");
                Ok(())
            },
            || {
                steps.borrow_mut().push("sync_directory");
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            steps.into_inner(),
            ["sync_file", "rename", "sync_directory"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_legacy_marker_is_not_rewritten() {
        use std::os::unix::fs::MetadataExt;

        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let persistence = SchedulePersistence::new(&ctx);
        persistence.with_locked(|_| Ok(())).unwrap();
        let original_inode = fs::metadata(&persistence.legacy_path).unwrap().ino();

        persistence.read_locked(|_| Ok(())).unwrap();

        assert_eq!(
            fs::metadata(&persistence.legacy_path).unwrap().ino(),
            original_inode
        );
    }

    #[test]
    fn durable_publish_stops_before_replace_when_file_sync_fails() {
        let steps = RefCell::new(Vec::new());

        let error = publish_durable(
            || {
                steps.borrow_mut().push("sync_file");
                anyhow::bail!("injected sync failure")
            },
            || {
                steps.borrow_mut().push("rename");
                Ok(())
            },
            || {
                steps.borrow_mut().push("sync_directory");
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("injected sync failure"));
        assert_eq!(steps.into_inner(), ["sync_file"]);
    }

    #[test]
    fn durable_directory_creation_builds_missing_parent_chain() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join(".agent/runtime/loop");

        ensure_durable_directory(&nested).unwrap();

        assert!(nested.is_dir());
    }

    #[test]
    fn durable_write_publishes_a_readable_schedule_without_a_temp_leftover() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("schedule.json");

        write_json_durable(&path, &ScheduleFile::default()).unwrap();

        let published: ScheduleFile = read_json_or_default(&path).unwrap();
        assert_eq!(published.schema_version, SCHEDULE_SCHEMA_VERSION);
        assert!(published.migrated_to.is_none());
        assert!(published.occurrences.is_empty());
        assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("schedule.tmp-")
        }));
    }

    #[test]
    fn durable_directory_rejects_a_non_directory_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("loop");
        fs::write(&path, "not a directory").unwrap();

        let error = ensure_durable_directory(&path).unwrap_err().to_string();

        assert!(error.contains("is not a directory"), "{error}");
    }

    #[test]
    fn durable_write_removes_temporary_file_after_rename_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("schedule.json");
        fs::create_dir(&path).unwrap();

        let error = write_json_durable(&path, &ScheduleFile::default())
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("Failed to replace loop schedule state"),
            "{error}"
        );
        let leftovers = fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with("schedule.tmp-"))
            .collect::<Vec<_>>();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn read_only_rechecks_durable_state_when_marker_follows_initial_miss() {
        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let persistence = SchedulePersistence::new(&ctx);
        let durable = ScheduleFile::default();
        write_json_durable(&persistence.path, &durable).unwrap();
        let marker = ScheduleFile {
            schema_version: SCHEDULE_SCHEMA_VERSION,
            migrated_to: Some(SCHEDULE_STATE_PATH.into()),
            occurrences: BTreeMap::new(),
        };

        let resolved = persistence
            .resolve_read_only(None, Some(marker), &|| false)
            .unwrap();

        assert!(resolved == durable);
    }
}
