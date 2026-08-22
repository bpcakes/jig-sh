use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::context::RepoContext;
use crate::state::now_ms;

use super::state::{
    LOOP_CACHE_DIR, read_json_or_default_with_cancellation, renewal_interval, with_json_cache_lock,
};

const SCHEDULE_SCHEMA_VERSION: u32 = 1;
const OCCURRENCE_HISTORY_PER_WORKFLOW: usize = 20;
const MAX_ERROR_CHARS: usize = 4_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ScheduleOccurrence {
    pub(super) occurrence_id: String,
    pub(super) workflow_id: String,
    pub(super) scheduled_at_ms: u64,
    pub(super) owner: String,
    pub(super) claim_expires_at_ms: u64,
    pub(super) started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) finished_at_ms: Option<u64>,
    pub(super) status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) worker_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

impl ScheduleOccurrence {
    pub(super) fn is_terminal(&self) -> bool {
        self.status != "running"
    }
}

#[derive(Deserialize, Serialize)]
struct ScheduleFile {
    schema_version: u32,
    occurrences: BTreeMap<String, ScheduleOccurrence>,
}

impl Default for ScheduleFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEDULE_SCHEMA_VERSION,
            occurrences: BTreeMap::new(),
        }
    }
}

pub(super) enum OccurrenceClaim {
    Acquired(ScheduleOccurrence),
    AlreadyRecorded(ScheduleOccurrence),
}

pub(super) struct OccurrenceFinish<'a> {
    pub(super) status: &'a str,
    pub(super) worker_receipt_id: Option<&'a str>,
    pub(super) worktree: Option<&'a str>,
    pub(super) error: Option<&'a str>,
}

#[derive(Clone)]
pub(super) struct OccurrenceStore {
    dir: PathBuf,
    path: PathBuf,
    lock_path: PathBuf,
}

pub(super) struct OccurrenceGuard {
    store: OccurrenceStore,
    occurrence_id: String,
    owner: String,
    stop: Option<Sender<()>>,
    renewal: Option<JoinHandle<Result<()>>>,
    renewal_failed: Arc<AtomicBool>,
}

impl OccurrenceGuard {
    pub(super) fn start(
        store: OccurrenceStore,
        occurrence: &ScheduleOccurrence,
        ttl_seconds: u64,
    ) -> Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let mut renewal_store = store.clone();
        let occurrence_id = occurrence.occurrence_id.clone();
        let renewal_occurrence_id = occurrence_id.clone();
        let owner = occurrence.owner.clone();
        let renewal_owner = owner.clone();
        let interval = renewal_interval(ttl_seconds);
        let renewal_failed = Arc::new(AtomicBool::new(false));
        let renewal_failed_in_thread = Arc::clone(&renewal_failed);
        let renewal = thread::Builder::new()
            .name(format!("jig-loop-occurrence-{owner}"))
            .spawn(move || {
                loop {
                    match receiver.recv_timeout(interval) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(RecvTimeoutError::Timeout) => {
                            if let Err(error) = renewal_store.renew(
                                &renewal_occurrence_id,
                                &renewal_owner,
                                ttl_seconds,
                            ) {
                                renewal_failed_in_thread.store(true, Ordering::Release);
                                return Err(error);
                            }
                        }
                    }
                }
            })
            .map_err(|error| {
                anyhow::anyhow!("Failed to start occurrence renewal thread: {error}")
            })?;
        Ok(Self {
            store,
            occurrence_id,
            owner,
            stop: Some(stop),
            renewal: Some(renewal),
            renewal_failed,
        })
    }

    pub(super) fn renewal_failed(&self) -> bool {
        self.renewal_failed.load(Ordering::Acquire)
    }

    pub(super) fn finish(mut self, finish: OccurrenceFinish<'_>) -> Result<ScheduleOccurrence> {
        self.stop_renewal()?;
        self.store.finish(&self.occurrence_id, &self.owner, finish)
    }

    pub(super) fn abandon(mut self) -> Result<ScheduleOccurrence> {
        self.stop_renewal()?;
        self.store.abandon(&self.occurrence_id, &self.owner)
    }

    fn stop_renewal(&mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(renewal) = self.renewal.take() {
            renewal
                .join()
                .map_err(|_| anyhow::anyhow!("Occurrence renewal thread panicked"))??;
        }
        Ok(())
    }
}

impl Drop for OccurrenceGuard {
    fn drop(&mut self) {
        let _ = self.stop_renewal();
    }
}

impl OccurrenceStore {
    pub(super) fn new(ctx: &RepoContext) -> Self {
        let dir = ctx.root().join(LOOP_CACHE_DIR);
        Self {
            path: dir.join("schedule.json"),
            lock_path: dir.join("schedule.lock"),
            dir,
        }
    }

    pub(super) fn claim(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
    ) -> Result<OccurrenceClaim> {
        self.claim_at(workflow_id, scheduled_at_ms, ttl_seconds, now_ms())
    }

    pub(super) fn renew(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        ttl_seconds: u64,
    ) -> Result<ScheduleOccurrence> {
        self.renew_at(occurrence_id, owner, ttl_seconds, now_ms())
    }

    pub(super) fn finish(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        finish: OccurrenceFinish<'_>,
    ) -> Result<ScheduleOccurrence> {
        if !matches!(finish.status, "succeeded" | "failed") {
            bail!(
                "Unsupported scheduled occurrence terminal status '{}'; expected succeeded or failed",
                finish.status
            );
        }
        self.with_locked(|store| {
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            require_running_owner(record, owner)?;
            record.status = finish.status.to_string();
            record.finished_at_ms = Some(now_ms());
            record.worker_receipt_id = finish.worker_receipt_id.map(str::to_string);
            record.worktree = finish.worktree.map(str::to_string);
            record.error = finish.error.map(bounded_error);
            let finished = record.clone();
            prune_history(store);
            Ok(finished)
        })
    }

    fn abandon(&mut self, occurrence_id: &str, owner: &str) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let record = store.occurrences.get(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            require_running_owner(record, owner)?;
            store
                .occurrences
                .remove(occurrence_id)
                .ok_or_else(|| anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}"))
        })
    }

    pub(super) fn reconcile_stale(&mut self) -> Result<Vec<ScheduleOccurrence>> {
        self.reconcile_stale_at(now_ms())
    }

    pub(super) fn snapshot(&mut self) -> Result<Vec<ScheduleOccurrence>> {
        self.with_locked(|store| {
            validate_schema(store)?;
            Ok(sorted_occurrences(store))
        })
    }

    pub(super) fn snapshot_read_only_with_cancellation(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<ScheduleOccurrence>> {
        let store = read_json_or_default_with_cancellation::<ScheduleFile>(&self.path, cancelled)?;
        validate_schema(&store)?;
        Ok(sorted_occurrences(&store))
    }

    pub(super) fn latest_for_workflow(
        occurrences: &[ScheduleOccurrence],
        workflow_id: &str,
    ) -> Option<ScheduleOccurrence> {
        occurrences
            .iter()
            .filter(|record| record.workflow_id == workflow_id)
            .max_by_key(|record| record.scheduled_at_ms)
            .cloned()
    }

    fn claim_at(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
    ) -> Result<OccurrenceClaim> {
        let occurrence_id = occurrence_id(workflow_id, scheduled_at_ms);
        self.with_locked(|store| {
            validate_schema(store)?;
            reconcile_stale_file(store, now);
            if let Some(existing) = store.occurrences.get(&occurrence_id) {
                return Ok(OccurrenceClaim::AlreadyRecorded(existing.clone()));
            }
            let record = ScheduleOccurrence {
                occurrence_id: occurrence_id.clone(),
                workflow_id: workflow_id.to_string(),
                scheduled_at_ms,
                owner: format!("{}-{}", std::process::id(), Ulid::new()),
                claim_expires_at_ms: expiry(now, ttl_seconds),
                started_at_ms: now,
                finished_at_ms: None,
                status: "running".into(),
                worker_receipt_id: None,
                worktree: None,
                error: None,
            };
            store.occurrences.insert(occurrence_id, record.clone());
            prune_history(store);
            Ok(OccurrenceClaim::Acquired(record))
        })
    }

    fn renew_at(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        ttl_seconds: u64,
        now: u64,
    ) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            require_running_owner(record, owner)?;
            if record.claim_expires_at_ms <= now {
                bail!("Scheduled occurrence claim expired before renewal: {occurrence_id}");
            }
            record.claim_expires_at_ms = expiry(now, ttl_seconds);
            Ok(record.clone())
        })
    }

    fn reconcile_stale_at(&mut self, now: u64) -> Result<Vec<ScheduleOccurrence>> {
        self.with_locked(|store| {
            validate_schema(store)?;
            let reconciled = reconcile_stale_file(store, now);
            prune_history(store);
            Ok(reconciled)
        })
    }

    fn with_locked<T>(&mut self, action: impl FnOnce(&mut ScheduleFile) -> Result<T>) -> Result<T> {
        with_json_cache_lock(&self.dir, &self.lock_path, &self.path, action)
    }
}

fn occurrence_id(workflow_id: &str, scheduled_at_ms: u64) -> String {
    format!("{workflow_id}@{scheduled_at_ms}")
}

fn require_running_owner(record: &ScheduleOccurrence, owner: &str) -> Result<()> {
    if record.owner != owner {
        bail!(
            "Scheduled occurrence '{}' is owned by another dispatcher",
            record.occurrence_id
        );
    }
    if record.status != "running" {
        bail!(
            "Scheduled occurrence '{}' is already {}",
            record.occurrence_id,
            record.status
        );
    }
    Ok(())
}

fn reconcile_stale_file(store: &mut ScheduleFile, now: u64) -> Vec<ScheduleOccurrence> {
    let mut reconciled = Vec::new();
    for record in store.occurrences.values_mut() {
        if record.status == "running" && record.claim_expires_at_ms <= now {
            record.status = "needs_attention".into();
            record.finished_at_ms = Some(now);
            record.error = Some("scheduled task stopped without a terminal result".into());
            reconciled.push(record.clone());
        }
    }
    reconciled
}

fn sorted_occurrences(store: &ScheduleFile) -> Vec<ScheduleOccurrence> {
    let mut records = store.occurrences.values().cloned().collect::<Vec<_>>();
    records.sort_by_key(|record| (record.workflow_id.clone(), record.scheduled_at_ms));
    records
}

fn validate_schema(store: &ScheduleFile) -> Result<()> {
    if store.schema_version != SCHEDULE_SCHEMA_VERSION {
        bail!(
            "Unsupported loop schedule cache schema version {}; expected {}",
            store.schema_version,
            SCHEDULE_SCHEMA_VERSION
        );
    }
    Ok(())
}

fn prune_history(store: &mut ScheduleFile) {
    let workflow_ids = store
        .occurrences
        .values()
        .map(|record| record.workflow_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for workflow_id in workflow_ids {
        let mut terminal = store
            .occurrences
            .values()
            .filter(|record| record.workflow_id == workflow_id && record.is_terminal())
            .map(|record| (record.scheduled_at_ms, record.occurrence_id.clone()))
            .collect::<Vec<_>>();
        terminal.sort_by_key(|(scheduled_at_ms, _)| std::cmp::Reverse(*scheduled_at_ms));
        for (_, occurrence_id) in terminal.into_iter().skip(OCCURRENCE_HISTORY_PER_WORKFLOW) {
            store.occurrences.remove(&occurrence_id);
        }
    }
}

fn expiry(now: u64, ttl_seconds: u64) -> u64 {
    now.saturating_add(ttl_seconds.saturating_mul(1_000))
}

fn bounded_error(error: &str) -> String {
    error.chars().take(MAX_ERROR_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn occurrence_claim_is_single_use_and_owner_checked() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = OccurrenceStore::new(&ctx);

        let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 60, 1_000).unwrap()
        else {
            panic!("expected occurrence claim");
        };
        let OccurrenceClaim::AlreadyRecorded(existing) =
            store.claim_at("nightly", 100, 60, 1_001).unwrap()
        else {
            panic!("expected duplicate occurrence");
        };
        assert_eq!(existing.owner, claim.owner);
        assert!(
            store
                .finish(
                    &claim.occurrence_id,
                    "another-owner",
                    OccurrenceFinish {
                        status: "succeeded",
                        worker_receipt_id: None,
                        worktree: None,
                        error: None,
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("owned by another dispatcher")
        );
    }

    #[test]
    fn occurrence_renewal_and_stale_reconciliation_are_terminal() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = OccurrenceStore::new(&ctx);
        let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap()
        else {
            panic!("expected occurrence claim");
        };

        let renewed = store
            .renew_at(&claim.occurrence_id, &claim.owner, 2, 2_000)
            .unwrap();
        assert_eq!(renewed.claim_expires_at_ms, 4_000);
        assert!(store.reconcile_stale_at(3_999).unwrap().is_empty());
        let reconciled = store.reconcile_stale_at(4_000).unwrap();
        assert_eq!(reconciled.len(), 1);
        assert_eq!(reconciled[0].status, "needs_attention");

        let OccurrenceClaim::AlreadyRecorded(existing) =
            store.claim_at("nightly", 100, 2, 5_000).unwrap()
        else {
            panic!("stale occurrence must not be reclaimed");
        };
        assert_eq!(existing.status, "needs_attention");
    }

    #[test]
    fn one_second_occurrence_claim_renews_before_expiry() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = OccurrenceStore::new(&ctx);
        let OccurrenceClaim::Acquired(claim) = store.claim("nightly", 100, 1).unwrap() else {
            panic!("expected occurrence claim");
        };
        let guard = OccurrenceGuard::start(store.clone(), &claim, 1).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1_200));
        let finished = guard
            .finish(OccurrenceFinish {
                status: "succeeded",
                worker_receipt_id: None,
                worktree: None,
                error: None,
            })
            .unwrap();
        assert_eq!(finished.status, "succeeded");
    }

    fn write_loop_fixture_repo(root: &std::path::Path) {
        crate::test_env::TestRepoBuilder::new(root)
            .required_commands(Vec::<String>::new())
            .write();
    }
}
