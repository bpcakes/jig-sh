use std::collections::BTreeMap;
use std::path::Path;
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

use super::state::renewal_interval;

mod persistence;

use persistence::SchedulePersistence;

const SCHEDULE_SCHEMA_VERSION: u32 = 2;
const LEGACY_SCHEDULE_SCHEMA_VERSION: u32 = 1;
const OCCURRENCE_HISTORY_PER_WORKFLOW: usize = 20;
const MAX_ERROR_CHARS: usize = 4_000;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct ScheduleOccurrence {
    pub(super) occurrence_id: String,
    pub(super) workflow_id: String,
    pub(super) scheduled_at_ms: u64,
    pub(super) owner: String,
    pub(super) claim_expires_at_ms: u64,
    pub(super) started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) acknowledged_at_ms: Option<u64>,
    pub(super) status: OccurrenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) worker_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

impl ScheduleOccurrence {
    fn is_prunable_history(&self) -> bool {
        matches!(
            self.status,
            OccurrenceStatus::Succeeded | OccurrenceStatus::Failed | OccurrenceStatus::Acknowledged
        )
    }

    fn has_retained_worktree(&self) -> bool {
        self.worktree
            .as_deref()
            .is_some_and(|worktree| Path::new(worktree).exists())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OccurrenceStatus {
    Running,
    Succeeded,
    Failed,
    NeedsAttention,
    Acknowledged,
}

impl OccurrenceStatus {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::NeedsAttention => "needs_attention",
            Self::Acknowledged => "acknowledged",
        }
    }
}

impl std::fmt::Display for OccurrenceStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OccurrenceOutcome {
    Succeeded,
    Failed,
    NeedsAttention,
}

impl OccurrenceOutcome {
    pub(super) const fn status(self) -> OccurrenceStatus {
        match self {
            Self::Succeeded => OccurrenceStatus::Succeeded,
            Self::Failed => OccurrenceStatus::Failed,
            Self::NeedsAttention => OccurrenceStatus::NeedsAttention,
        }
    }
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
struct ScheduleFile {
    schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    migrated_to: Option<String>,
    occurrences: BTreeMap<String, ScheduleOccurrence>,
}

impl Default for ScheduleFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEDULE_SCHEMA_VERSION,
            migrated_to: None,
            occurrences: BTreeMap::new(),
        }
    }
}

pub(super) enum OccurrenceClaim {
    Acquired(ScheduleOccurrence),
    AlreadyRecorded(ScheduleOccurrence),
}

pub(super) enum OccurrenceAcknowledgement {
    Acknowledged(ScheduleOccurrence),
    AlreadyAcknowledged(ScheduleOccurrence),
}

pub(super) struct OccurrenceFinish<'a> {
    pub(super) outcome: OccurrenceOutcome,
    pub(super) worker_receipt_id: Option<&'a str>,
    pub(super) worktree: Option<&'a str>,
    pub(super) error: Option<&'a str>,
}

#[derive(Clone)]
pub(super) struct OccurrenceStore {
    persistence: SchedulePersistence,
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
        Self {
            persistence: SchedulePersistence::new(ctx),
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
        self.with_locked(|store| {
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            require_running_owner(record, owner)?;
            record.status = finish.outcome.status();
            record.finished_at_ms = Some(now_ms());
            record.worker_receipt_id = finish.worker_receipt_id.map(str::to_string);
            record.worktree = finish.worktree.map(str::to_string);
            record.error = finish.error.map(bounded_error);
            let finished = record.clone();
            prune_history(store);
            Ok(finished)
        })
    }

    pub(super) fn acknowledge(&mut self, occurrence_id: &str) -> Result<OccurrenceAcknowledgement> {
        self.with_locked(|store| {
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            match record.status {
                OccurrenceStatus::NeedsAttention => {
                    record.status = OccurrenceStatus::Acknowledged;
                    record.acknowledged_at_ms = Some(now_ms());
                    let acknowledged = record.clone();
                    prune_history(store);
                    Ok(OccurrenceAcknowledgement::Acknowledged(acknowledged))
                }
                OccurrenceStatus::Acknowledged => Ok(
                    OccurrenceAcknowledgement::AlreadyAcknowledged(record.clone()),
                ),
                status => bail!(
                    "Scheduled occurrence '{occurrence_id}' is {status}; only needs_attention occurrences can be acknowledged"
                ),
            }
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
        let store = self.persistence.read_only(cancelled)?;
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
                acknowledged_at_ms: None,
                status: OccurrenceStatus::Running,
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
        self.persistence.with_locked(action)
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
    if record.status != OccurrenceStatus::Running {
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
        if record.status == OccurrenceStatus::Running && record.claim_expires_at_ms <= now {
            record.status = OccurrenceStatus::NeedsAttention;
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
            "Unsupported loop schedule state schema version {}; expected {}",
            store.schema_version,
            SCHEDULE_SCHEMA_VERSION
        );
    }
    Ok(())
}

fn migrate_schedule_schema(store: &mut ScheduleFile) -> Result<()> {
    match store.schema_version {
        SCHEDULE_SCHEMA_VERSION => Ok(()),
        LEGACY_SCHEDULE_SCHEMA_VERSION => {
            store.schema_version = SCHEDULE_SCHEMA_VERSION;
            Ok(())
        }
        _ => validate_schema(store),
    }
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
            .filter(|record| {
                record.workflow_id == workflow_id
                    && record.is_prunable_history()
                    && !record.has_retained_worktree()
            })
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
    use crate::runtime::loops::state::{LOOP_CACHE_DIR, LOOP_RUNTIME_DIR};

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
                        outcome: OccurrenceOutcome::Succeeded,
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
        assert_eq!(reconciled[0].status, OccurrenceStatus::NeedsAttention);

        let OccurrenceClaim::AlreadyRecorded(existing) =
            store.claim_at("nightly", 100, 2, 5_000).unwrap()
        else {
            panic!("stale occurrence must not be reclaimed");
        };
        assert_eq!(existing.status, OccurrenceStatus::NeedsAttention);
    }

    #[test]
    fn acknowledging_attention_is_idempotent_and_preserves_the_claim() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = OccurrenceStore::new(&ctx);
        let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap()
        else {
            panic!("expected occurrence claim");
        };
        store.reconcile_stale_at(3_000).unwrap();

        let OccurrenceAcknowledgement::Acknowledged(acknowledged) =
            store.acknowledge(&claim.occurrence_id).unwrap()
        else {
            panic!("expected first acknowledgement to change state");
        };
        assert_eq!(acknowledged.status, OccurrenceStatus::Acknowledged);
        assert!(acknowledged.acknowledged_at_ms.is_some());

        let OccurrenceAcknowledgement::AlreadyAcknowledged(existing) =
            store.acknowledge(&claim.occurrence_id).unwrap()
        else {
            panic!("expected repeated acknowledgement to be idempotent");
        };
        assert_eq!(existing.acknowledged_at_ms, acknowledged.acknowledged_at_ms);

        let OccurrenceClaim::AlreadyRecorded(existing) =
            store.claim_at("nightly", 100, 2, 4_000).unwrap()
        else {
            panic!("acknowledgement must not make the occurrence runnable again");
        };
        assert_eq!(existing.status, OccurrenceStatus::Acknowledged);
    }

    #[test]
    fn schedule_state_migrates_out_of_disposable_cache_and_survives_cache_removal() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let legacy_dir = temp.path().join(LOOP_CACHE_DIR);
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            legacy_dir.join("schedule.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": LEGACY_SCHEDULE_SCHEMA_VERSION,
                "occurrences": {
                    "nightly@100": {
                        "occurrence_id": "nightly@100",
                        "workflow_id": "nightly",
                        "scheduled_at_ms": 100,
                        "owner": "owner",
                        "claim_expires_at_ms": 200,
                        "started_at_ms": 100,
                        "finished_at_ms": 150,
                        "status": "succeeded"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let migrated = OccurrenceStore::new(&ctx).snapshot().unwrap();
        assert_eq!(migrated.len(), 1);
        assert_eq!(migrated[0].status, OccurrenceStatus::Succeeded);
        let durable_path = temp.path().join(LOOP_RUNTIME_DIR).join("schedule.json");
        assert!(durable_path.is_file());
        let marker: serde_json::Value =
            serde_json::from_slice(&std::fs::read(legacy_dir.join("schedule.json")).unwrap())
                .unwrap();
        assert_eq!(marker["schema_version"], SCHEDULE_SCHEMA_VERSION);
        assert_eq!(marker["migrated_to"], ".agent/runtime/loop/schedule.json");

        std::fs::remove_dir_all(&legacy_dir).unwrap();
        let after_cache_removal = OccurrenceStore::new(&ctx).snapshot().unwrap();
        assert_eq!(after_cache_removal, migrated);
    }

    #[test]
    fn read_only_snapshot_can_inspect_legacy_state_without_migrating_it() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let legacy_dir = temp.path().join(LOOP_CACHE_DIR);
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            legacy_dir.join("schedule.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": LEGACY_SCHEDULE_SCHEMA_VERSION,
                "occurrences": {}
            }))
            .unwrap(),
        )
        .unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let snapshot = OccurrenceStore::new(&ctx)
            .snapshot_read_only_with_cancellation(&|| false)
            .unwrap();

        assert!(snapshot.is_empty());
        assert!(!temp.path().join(LOOP_RUNTIME_DIR).exists());
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
                outcome: OccurrenceOutcome::Succeeded,
                worker_receipt_id: None,
                worktree: None,
                error: None,
            })
            .unwrap();
        assert_eq!(finished.status, OccurrenceStatus::Succeeded);
    }

    #[test]
    fn pruning_preserves_discoverability_until_retained_worktree_is_removed() {
        let temp = tempdir().unwrap();
        let retained = temp.path().join("retained-worktree");
        std::fs::create_dir(&retained).unwrap();
        let mut store = ScheduleFile::default();
        for scheduled_at_ms in 0..=21 {
            let occurrence_id = occurrence_id("nightly", scheduled_at_ms);
            store.occurrences.insert(
                occurrence_id.clone(),
                ScheduleOccurrence {
                    occurrence_id,
                    workflow_id: "nightly".into(),
                    scheduled_at_ms,
                    owner: "owner".into(),
                    claim_expires_at_ms: 0,
                    started_at_ms: 0,
                    finished_at_ms: Some(1),
                    acknowledged_at_ms: None,
                    status: OccurrenceStatus::Succeeded,
                    worker_receipt_id: None,
                    worktree: (scheduled_at_ms == 0).then(|| retained.display().to_string()),
                    error: None,
                },
            );
        }

        prune_history(&mut store);

        assert_eq!(store.occurrences.len(), 21);
        assert!(store.occurrences.contains_key("nightly@0"));
        assert!(!store.occurrences.contains_key("nightly@1"));

        std::fs::remove_dir(&retained).unwrap();
        prune_history(&mut store);

        assert_eq!(store.occurrences.len(), OCCURRENCE_HISTORY_PER_WORKFLOW);
        assert!(!store.occurrences.contains_key("nightly@0"));
    }

    #[test]
    fn pruning_never_discards_occurrences_that_need_attention() {
        let mut store = ScheduleFile::default();
        for scheduled_at_ms in 0..=OCCURRENCE_HISTORY_PER_WORKFLOW as u64 {
            let occurrence_id = occurrence_id("nightly", scheduled_at_ms);
            store.occurrences.insert(
                occurrence_id.clone(),
                ScheduleOccurrence {
                    occurrence_id,
                    workflow_id: "nightly".into(),
                    scheduled_at_ms,
                    owner: "owner".into(),
                    claim_expires_at_ms: 0,
                    started_at_ms: 0,
                    finished_at_ms: Some(1),
                    acknowledged_at_ms: None,
                    status: if scheduled_at_ms == 0 {
                        OccurrenceStatus::NeedsAttention
                    } else {
                        OccurrenceStatus::Succeeded
                    },
                    worker_receipt_id: None,
                    worktree: None,
                    error: None,
                },
            );
        }

        prune_history(&mut store);

        assert_eq!(store.occurrences.len(), OCCURRENCE_HISTORY_PER_WORKFLOW + 1);
        assert!(store.occurrences.contains_key("nightly@0"));
    }

    fn write_loop_fixture_repo(root: &std::path::Path) {
        crate::test_env::TestRepoBuilder::new(root)
            .required_commands(Vec::<String>::new())
            .write();
    }
}
