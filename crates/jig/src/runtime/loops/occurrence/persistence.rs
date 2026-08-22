use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::context::RepoContext;

use super::{SCHEDULE_SCHEMA_VERSION, ScheduleFile, migrate_schedule_schema, validate_schema};
use crate::runtime::loops::state::{
    LOOP_CACHE_DIR, LOOP_RUNTIME_DIR, read_json_or_default, read_json_or_default_with_cancellation,
    with_exclusive_file_lock, write_json,
};

const SCHEDULE_STATE_PATH: &str = ".agent/runtime/loop/schedule.json";

#[derive(Clone)]
pub(super) struct SchedulePersistence {
    dir: PathBuf,
    path: PathBuf,
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
            lock_path: dir.join("schedule.lock"),
            dir,
            legacy_path: legacy_dir.join("schedule.json"),
            legacy_lock_path: legacy_dir.join("schedule.lock"),
            legacy_dir,
        }
    }

    pub(super) fn read_only(&self, cancelled: &dyn Fn() -> bool) -> Result<ScheduleFile> {
        let mut store = if self.path.try_exists().with_context(|| {
            format!(
                "Failed to inspect loop schedule state {}",
                self.path.display()
            )
        })? {
            read_json_or_default_with_cancellation::<ScheduleFile>(&self.path, cancelled)?
        } else {
            read_json_or_default_with_cancellation::<ScheduleFile>(&self.legacy_path, cancelled)?
        };
        migrate_schedule_schema(&mut store)?;
        if store.migrated_to.is_some() {
            bail!(
                "Loop schedule migration marker exists without durable state at {}",
                self.path.display()
            );
        }
        Ok(store)
    }

    pub(super) fn with_locked<T>(
        &self,
        action: impl FnOnce(&mut ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.ensure_legacy_migrated()?;
        with_exclusive_file_lock(&self.dir, &self.lock_path, || {
            let mut store = read_json_or_default::<ScheduleFile>(&self.path)?;
            migrate_schedule_schema(&mut store)?;
            let result = action(&mut store)?;
            write_json(&self.path, &store)?;
            Ok(result)
        })
    }

    fn ensure_legacy_migrated(&self) -> Result<()> {
        with_exclusive_file_lock(&self.legacy_dir, &self.legacy_lock_path, || {
            if !self.legacy_path.try_exists().with_context(|| {
                format!(
                    "Failed to inspect legacy loop schedule state {}",
                    self.legacy_path.display()
                )
            })? {
                return Ok(());
            }
            let mut legacy = read_json_or_default::<ScheduleFile>(&self.legacy_path)?;
            if legacy.migrated_to.as_deref() == Some(SCHEDULE_STATE_PATH) {
                validate_schema(&legacy)?;
                if !legacy.occurrences.is_empty() {
                    bail!(
                        "Invalid legacy loop schedule migration marker at {}: occurrences must be empty",
                        self.legacy_path.display()
                    );
                }
                return Ok(());
            }
            migrate_schedule_schema(&mut legacy)?;

            if self.path.try_exists().with_context(|| {
                format!(
                    "Failed to inspect loop schedule state {}",
                    self.path.display()
                )
            })? {
                let mut current = read_json_or_default::<ScheduleFile>(&self.path)?;
                migrate_schedule_schema(&mut current)?;
                if current != legacy {
                    bail!(
                        "Loop schedule state exists at both {} and {}; stop older Jig runtimes and reconcile the files before dispatching",
                        self.path.display(),
                        self.legacy_path.display()
                    );
                }
            } else {
                write_json(&self.path, &legacy)?;
            }

            write_json(
                &self.legacy_path,
                &ScheduleFile {
                    schema_version: SCHEDULE_SCHEMA_VERSION,
                    migrated_to: Some(SCHEDULE_STATE_PATH.to_string()),
                    occurrences: BTreeMap::new(),
                },
            )
        })
    }
}
