use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::context::RepoContext;
use crate::state::now_ms;

#[cfg(test)]
use super::renewal::retry_delay as renewal_retry_delay;
use super::renewal::{RenewalAttemptError, RenewalOwnershipLost, renewal_interval, run_with_wait};

mod attention;
mod claim;
mod guard_renewal;
mod history;
mod manual;
mod persistence;
mod transition;
mod worktree;

pub(super) use claim::OccurrenceAttentionScope;
use guard_renewal::run_occurrence_renewal;
#[cfg(test)]
use guard_renewal::run_occurrence_renewal_with_wait;
use history::prune_history;
use manual::MANUAL_OCCURRENCE_SCHEDULED_AT_MS;
use persistence::SchedulePersistence;
use transition::{
    OwnedOccurrenceState, require_owned_occurrence_state, require_running_owner,
    require_unexecuted_owner,
};
pub(super) use worktree::{OccurrenceWorktreeReservation, encode_worktree_path};

const SCHEDULE_SCHEMA_VERSION: u32 = 4;
const PREVIOUS_SCHEDULE_SCHEMA_VERSION: u32 = 3;
const LEGACY_SCHEDULE_SCHEMA_VERSION: u32 = 1;
const OCCURRENCE_HISTORY_PER_WORKFLOW: usize = 20;
const MAX_ERROR_CHARS: usize = 4_000;
const STALE_RECONCILIATION_ERROR: &str = "scheduled task stopped without a terminal result";
const STALE_RECONCILIATION_STAGED_ERROR_PREFIX: &str =
    "scheduled task stopped without a terminal result; staged occurrence evidence: ";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct ScheduleOccurrence {
    pub(super) occurrence_id: String,
    pub(super) workflow_id: String,
    pub(super) scheduled_at_ms: u64,
    pub(super) owner: String,
    pub(super) claim_expires_at_ms: u64,
    pub(super) started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) uses_shared_checkout: Option<bool>,
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
    BlockedByAttention(ScheduleOccurrence),
    BlockedByRunning(ScheduleOccurrence),
    BlockedByRetainedWorktree(ScheduleOccurrence),
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

pub(super) struct OccurrenceFinalization {
    pub(super) occurrence: ScheduleOccurrence,
    pub(super) renewal_error: Option<String>,
    pub(super) renewal_ownership_lost: bool,
}

impl OccurrenceGuard {
    pub(super) fn start(
        store: OccurrenceStore,
        occurrence: &ScheduleOccurrence,
        ttl_seconds: u64,
    ) -> Result<Self> {
        Self::start_with_interval(
            store,
            occurrence,
            ttl_seconds,
            renewal_interval(ttl_seconds),
        )
    }

    #[cfg(test)]
    fn start_for_test(
        store: OccurrenceStore,
        occurrence: &ScheduleOccurrence,
        ttl_seconds: u64,
        interval: Duration,
    ) -> Result<Self> {
        Self::start_with_interval(store, occurrence, ttl_seconds, interval)
    }

    fn start_with_interval(
        store: OccurrenceStore,
        occurrence: &ScheduleOccurrence,
        ttl_seconds: u64,
        interval: Duration,
    ) -> Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let mut renewal_store = store.clone();
        let occurrence_id = occurrence.occurrence_id.clone();
        let renewal_occurrence_id = occurrence_id.clone();
        let owner = occurrence.owner.clone();
        let renewal_owner = owner.clone();
        let renewal_failed = Arc::new(AtomicBool::new(false));
        let renewal_failed_in_thread = Arc::clone(&renewal_failed);
        let claim_expires_at_ms = occurrence.claim_expires_at_ms;
        let renewal = thread::Builder::new()
            .name(format!("jig-loop-occurrence-{owner}"))
            .spawn(move || {
                run_occurrence_renewal(
                    &receiver,
                    interval,
                    claim_expires_at_ms,
                    &renewal_failed_in_thread,
                    |deadline| {
                        renewal_store
                            .renew_for_guard(
                                &renewal_occurrence_id,
                                &renewal_owner,
                                ttl_seconds,
                                deadline,
                            )
                            .map(|renewed| renewed.claim_expires_at_ms)
                    },
                    now_ms,
                )
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

    pub(super) fn finish(self, finish: OccurrenceFinish<'_>) -> Result<OccurrenceFinalization> {
        self.finalize(|store, occurrence_id, owner| store.finish(occurrence_id, owner, finish))
    }

    #[cfg(test)]
    pub(super) fn abandon(self) -> Result<OccurrenceFinalization> {
        self.finalize(OccurrenceStore::abandon)
    }

    pub(super) fn abandon_unexecuted(self) -> Result<OccurrenceFinalization> {
        self.finalize(OccurrenceStore::abandon_unexecuted)
    }

    fn finalize(
        mut self,
        transition: impl FnOnce(&mut OccurrenceStore, &str, &str) -> Result<ScheduleOccurrence>,
    ) -> Result<OccurrenceFinalization> {
        let renewal_error = self.stop_renewal().err();
        let renewal_ownership_lost = renewal_error
            .as_ref()
            .is_some_and(|error| error.downcast_ref::<RenewalOwnershipLost>().is_some());
        let renewal_error = renewal_error.map(|error| format!("{error:#}"));
        let transition = transition(&mut self.store, &self.occurrence_id, &self.owner);
        match (transition, renewal_error) {
            (Ok(occurrence), renewal_error) => Ok(OccurrenceFinalization {
                occurrence,
                renewal_error,
                renewal_ownership_lost,
            }),
            (Err(error), Some(renewal_error)) if format!("{error:#}") == renewal_error => {
                Err(error)
            }
            (Err(error), Some(renewal_error)) => Err(error.context(format!(
                "Occurrence renewal shutdown also failed: {renewal_error}"
            ))),
            (Err(error), None) => Err(error),
        }
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

    #[cfg(test)]
    pub(super) fn claim(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
    ) -> Result<OccurrenceClaim> {
        self.claim_at(workflow_id, scheduled_at_ms, ttl_seconds, now_ms())
    }

    #[cfg(test)]
    pub(super) fn claim_scheduled(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        attention_scope: claim::OccurrenceAttentionScope,
        block_retained_worktree: bool,
    ) -> Result<OccurrenceClaim> {
        self.claim_scheduled_with_cancellation(
            workflow_id,
            scheduled_at_ms,
            ttl_seconds,
            attention_scope,
            block_retained_worktree,
            &|| false,
        )
    }

    pub(super) fn claim_scheduled_with_cancellation(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        attention_scope: claim::OccurrenceAttentionScope,
        block_retained_worktree: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<OccurrenceClaim> {
        self.claim_with_execution_at(
            workflow_id,
            scheduled_at_ms,
            ttl_seconds,
            now_ms(),
            claim::OccurrenceClaimExecution::scheduled(
                attention_scope,
                block_retained_worktree,
                cancelled,
            ),
        )
    }

    fn renew_for_guard(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        ttl_seconds: u64,
        deadline: Instant,
    ) -> std::result::Result<ScheduleOccurrence, RenewalAttemptError> {
        self.renew_with_lock_deadline(occurrence_id, owner, ttl_seconds, deadline, now_ms)
            .map_err(|error| {
                if error.downcast_ref::<RenewalOwnershipLost>().is_some() {
                    RenewalAttemptError::Terminal(error)
                } else {
                    RenewalAttemptError::Retryable(error)
                }
            })
    }

    pub(super) fn finish(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        finish: OccurrenceFinish<'_>,
    ) -> Result<ScheduleOccurrence> {
        self.finish_with_clock(occurrence_id, owner, finish, now_ms)
    }

    #[cfg(test)]
    fn finish_at(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        finish: OccurrenceFinish<'_>,
        now: u64,
    ) -> Result<ScheduleOccurrence> {
        self.finish_with_clock(occurrence_id, owner, finish, || now)
    }

    fn finish_with_clock(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        finish: OccurrenceFinish<'_>,
        now: impl FnOnce() -> u64,
    ) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let now = now();
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            match require_owned_occurrence_state(record, owner)? {
                OwnedOccurrenceState::Running => {}
                OwnedOccurrenceState::StaleReconciled => {
                    record_expired_finish_evidence(record, &finish);
                    let finished = record.clone();
                    prune_history(store);
                    return Ok(finished);
                }
            }
            if record.claim_expires_at_ms <= now {
                mark_expired_claim(record, now, Some(&finish));
                let finished = record.clone();
                prune_history(store);
                return Ok(finished);
            }
            record.status = finish.outcome.status();
            record.finished_at_ms = Some(now);
            record.worker_receipt_id = finish.worker_receipt_id.map(str::to_string);
            record.worktree = finish.worktree.map(str::to_string);
            record.error = finish.error.map(bounded_error);
            let finished = record.clone();
            prune_history(store);
            Ok(finished)
        })
    }

    #[cfg(test)]
    fn abandon(&mut self, occurrence_id: &str, owner: &str) -> Result<ScheduleOccurrence> {
        self.abandon_with_clock(occurrence_id, owner, now_ms)
    }

    #[cfg(test)]
    fn abandon_at(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        now: u64,
    ) -> Result<ScheduleOccurrence> {
        self.abandon_with_clock(occurrence_id, owner, || now)
    }

    #[cfg(test)]
    fn abandon_with_clock(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        now: impl FnOnce() -> u64,
    ) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let now = now();
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            require_running_owner(record, owner)?;
            if record.claim_expires_at_ms <= now {
                mark_expired_claim(record, now, None);
                return Ok(record.clone());
            }
            store
                .occurrences
                .remove(occurrence_id)
                .ok_or_else(|| anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}"))
        })
    }

    pub(super) fn abandon_unexecuted(
        &mut self,
        occurrence_id: &str,
        owner: &str,
    ) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let record = store.occurrences.get(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            require_unexecuted_owner(record, owner)?;
            let removed = store.occurrences.remove(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            Ok(removed)
        })
    }

    pub(super) fn reconcile_stale_with_cancellation(
        &mut self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<ScheduleOccurrence>> {
        self.reconcile_stale_at_with_cancellation(now_ms(), cancelled)
    }

    #[cfg(test)]
    pub(super) fn reconcile_stale_for_test(&mut self, now: u64) -> Result<Vec<ScheduleOccurrence>> {
        self.reconcile_stale_at(now)
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> Result<Vec<ScheduleOccurrence>> {
        self.persistence.read_locked(|store| {
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
            .filter(|record| {
                record.workflow_id == workflow_id
                    && record.scheduled_at_ms != MANUAL_OCCURRENCE_SCHEDULED_AT_MS
            })
            .max_by_key(|record| record.scheduled_at_ms)
            .cloned()
    }

    #[cfg(test)]
    fn claim_at(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
    ) -> Result<OccurrenceClaim> {
        self.claim_with_constraints_at(
            workflow_id,
            scheduled_at_ms,
            ttl_seconds,
            now,
            claim::OccurrenceAttentionScope::None,
            false,
        )
    }

    #[cfg(test)]
    fn renew_at(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        ttl_seconds: u64,
        now: u64,
    ) -> Result<ScheduleOccurrence> {
        self.renew_with_lock_deadline(
            occurrence_id,
            owner,
            ttl_seconds,
            super::state::loop_state_lock_deadline(),
            || now,
        )
    }

    fn renew_with_lock_deadline(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        ttl_seconds: u64,
        deadline: Instant,
        now: impl FnOnce() -> u64,
    ) -> Result<ScheduleOccurrence> {
        self.persistence.with_locked_until(deadline, |store| {
            let now = now();
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                RenewalOwnershipLost::new(format!(
                    "Loop occurrence is no longer held: {occurrence_id}"
                ))
            })?;
            if record.owner != owner {
                return Err(RenewalOwnershipLost::new(format!(
                    "Loop occurrence '{occurrence_id}' is owned by another dispatcher"
                ))
                .into());
            }
            if record.status != OccurrenceStatus::Running {
                return Err(RenewalOwnershipLost::new(format!(
                    "Loop occurrence '{occurrence_id}' is already {}",
                    record.status
                ))
                .into());
            }
            if record.claim_expires_at_ms <= now {
                return Err(RenewalOwnershipLost::new(format!(
                    "Loop occurrence claim expired before renewal: {occurrence_id}"
                ))
                .into());
            }
            record.claim_expires_at_ms = expiry(now, ttl_seconds);
            Ok(record.clone())
        })
    }

    #[cfg(test)]
    fn reconcile_stale_at(&mut self, now: u64) -> Result<Vec<ScheduleOccurrence>> {
        self.reconcile_stale_at_with_cancellation(now, &|| false)
    }

    fn reconcile_stale_at_with_cancellation(
        &mut self,
        now: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<ScheduleOccurrence>> {
        self.with_locked_with_cancellation(cancelled, |store| {
            validate_schema(store)?;
            let reconciled = reconcile_stale_file(store, now);
            prune_history(store);
            Ok(reconciled)
        })
    }

    fn with_locked<T>(&mut self, action: impl FnOnce(&mut ScheduleFile) -> Result<T>) -> Result<T> {
        self.persistence.with_locked(action)
    }

    fn with_locked_with_cancellation<T>(
        &mut self,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&mut ScheduleFile) -> Result<T>,
    ) -> Result<T> {
        self.persistence
            .with_locked_with_cancellation(cancelled, action)
    }
}

fn occurrence_id(workflow_id: &str, scheduled_at_ms: u64) -> String {
    format!("{workflow_id}@{scheduled_at_ms}")
}

fn reconcile_stale_file(store: &mut ScheduleFile, now: u64) -> Vec<ScheduleOccurrence> {
    let mut reconciled = Vec::new();
    for record in store.occurrences.values_mut() {
        if record.status == OccurrenceStatus::Running && record.claim_expires_at_ms <= now {
            mark_expired_claim(record, now, None);
            reconciled.push(record.clone());
        }
    }
    reconciled
}

fn mark_expired_claim(
    record: &mut ScheduleOccurrence,
    now: u64,
    finish: Option<&OccurrenceFinish<'_>>,
) {
    record.status = OccurrenceStatus::NeedsAttention;
    record.finished_at_ms = Some(now);
    if let Some(finish) = finish {
        record_expired_finish_evidence(record, finish);
    } else {
        record.error = Some(match record.error.take() {
            Some(staged_error) => bounded_error(&format!(
                "{STALE_RECONCILIATION_STAGED_ERROR_PREFIX}{staged_error}"
            )),
            None => STALE_RECONCILIATION_ERROR.into(),
        });
    }
}

fn record_expired_finish_evidence(record: &mut ScheduleOccurrence, finish: &OccurrenceFinish<'_>) {
    record.worker_receipt_id = finish.worker_receipt_id.map(str::to_string);
    record.worktree = finish.worktree.map(str::to_string);
    record.error = Some(bounded_error(&match finish.error {
        Some(error) => format!(
            "scheduled task claim expired before its terminal result was recorded; worker result is ambiguous: {error}"
        ),
        None => "scheduled task claim expired before its terminal result was recorded; worker result is ambiguous".into(),
    }));
}

fn sorted_occurrences(store: &ScheduleFile) -> Vec<ScheduleOccurrence> {
    let mut records = store.occurrences.values().cloned().collect::<Vec<_>>();
    records.sort_by_key(|record| (record.workflow_id.clone(), record.scheduled_at_ms));
    records
}

fn validate_schema(store: &ScheduleFile) -> Result<()> {
    validate_schema_version(store.schema_version, SCHEDULE_SCHEMA_VERSION)
}

fn validate_schema_version(actual: u32, expected: u32) -> Result<()> {
    if actual != expected {
        bail!(
            "Unsupported loop schedule state schema version {}; expected {}",
            actual,
            expected
        );
    }
    Ok(())
}

fn migrate_schedule_schema(store: &mut ScheduleFile) -> Result<()> {
    match store.schema_version {
        SCHEDULE_SCHEMA_VERSION => Ok(()),
        version
            if (LEGACY_SCHEDULE_SCHEMA_VERSION..=PREVIOUS_SCHEDULE_SCHEMA_VERSION)
                .contains(&version) =>
        {
            store.schema_version = SCHEDULE_SCHEMA_VERSION;
            Ok(())
        }
        _ => validate_schema(store),
    }
}

fn expiry(now: u64, ttl_seconds: u64) -> u64 {
    now.saturating_add(ttl_seconds.saturating_mul(1_000))
}

fn bounded_error(error: &str) -> String {
    error.chars().take(MAX_ERROR_CHARS).collect()
}

#[cfg(test)]
#[path = "occurrence/tests.rs"]
mod tests;
