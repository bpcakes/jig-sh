use std::collections::BTreeMap;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::fs::File;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ulid::Ulid;

use crate::cancellation::ensure_status_collection_active;
use crate::context::RepoContext;
use crate::state::now_ms;

use super::renewal::{renewal_interval, run_with_wait};
use super::workflow::ResolvedWorkflow;

mod bounded_json;
mod file_lock;
mod json_cache;
mod json_location;
mod lease_renewal;
mod persistence;

#[cfg(test)]
pub(super) use file_lock::with_exclusive_file_lock;
pub(super) use file_lock::{LOOP_STATE_LOCK_TIMEOUT, loop_state_lock_deadline};
pub(in crate::runtime::loops) use json_cache::StateDirectory;
#[cfg(test)]
pub(in crate::runtime::loops) use json_cache::cache_file_name;
use json_location::{JsonLocation, JsonWriteMode};
use persistence::JsonStatePersistence;

pub(super) const LOOP_CACHE_DIR: &str = ".agent/.cache/loop";
pub(super) const LOOP_RUNTIME_DIR: &str = ".agent/runtime/loop";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct LeaseRecord {
    key: String,
    pub(super) owner: String,
    acquired_at_ms: u64,
    expires_at_ms: u64,
}

impl LeaseRecord {
    pub(super) fn matches_key(&self, key: &str) -> bool {
        self.key == key
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct LeaseFile {
    leases: BTreeMap<String, LeaseRecord>,
}

pub(super) enum LeaseAcquire {
    Acquired(LeaseRecord),
    Held(LeaseRecord),
}

#[derive(Clone)]
pub(super) struct LeaseStore {
    persistence: JsonStatePersistence,
}

impl LeaseStore {
    pub(super) fn new(ctx: &RepoContext) -> Self {
        Self {
            persistence: JsonStatePersistence::new(ctx, "leases"),
        }
    }

    pub(super) fn new_repository(ctx: &RepoContext) -> Self {
        Self {
            persistence: JsonStatePersistence::new_repository(ctx, "branch_leases"),
        }
    }

    pub(super) fn for_workflow_claim(
        ctx: &RepoContext,
        workflow: &ResolvedWorkflow,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        if workflow.requires_repository_branch_authority() {
            validate_repository_branch_authority_with_cancellation(ctx, cancelled)?;
        }
        Ok(Self::new(ctx))
    }

    #[cfg(test)]
    pub(super) fn acquire(&mut self, key: &str, ttl_seconds: u64) -> Result<LeaseAcquire> {
        self.acquire_with_cancellation(key, ttl_seconds, &|| false)
    }

    pub(super) fn acquire_with_cancellation(
        &mut self,
        key: &str,
        ttl_seconds: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<LeaseAcquire> {
        self.persistence
            .with_locked_with_cancellation(cancelled, |store: &mut LeaseFile| {
                let now = now_ms();
                store.prune_expired(now);
                if let Some(existing) = store.leases.get(key) {
                    return Ok(LeaseAcquire::Held(existing.clone()));
                }

                let record = LeaseRecord {
                    key: key.to_string(),
                    owner: format!("{}-{}", std::process::id(), Ulid::new()),
                    acquired_at_ms: now,
                    expires_at_ms: now.saturating_add(ttl_seconds.saturating_mul(1000)),
                };
                store.leases.insert(key.to_string(), record.clone());
                Ok(LeaseAcquire::Acquired(record))
            })
    }

    pub(super) fn release(&mut self, key: &str, owner: &str) -> Result<()> {
        self.release_with_clock(key, owner, now_ms)
    }

    #[cfg(test)]
    pub(super) fn revoke_for_test(&mut self, key: &str) -> Result<()> {
        self.with_locked(|store| {
            store
                .leases
                .remove(key)
                .map(|_| ())
                .ok_or_else(|| anyhow!("Test lease is no longer held: {key}"))
        })
    }

    #[cfg(test)]
    fn release_at(&mut self, key: &str, owner: &str, now: u64) -> Result<()> {
        self.release_with_clock(key, owner, || now)
    }

    fn release_with_clock(
        &mut self,
        key: &str,
        owner: &str,
        now: impl FnOnce() -> u64,
    ) -> Result<()> {
        self.with_locked(|store| {
            let now = now();
            let lease = store
                .leases
                .get(key)
                .ok_or_else(|| anyhow!("Loop lease is no longer held: {key}"))?;
            if lease.owner != owner {
                return Err(anyhow!("Loop lease '{key}' is owned by another worker"));
            }
            if lease.expires_at_ms <= now {
                return Err(anyhow!("Loop lease expired before release: {key}"));
            }
            store.leases.remove(key);
            Ok(())
        })
    }

    #[cfg(test)]
    pub(super) fn active_leases(&mut self) -> Result<Vec<LeaseRecord>> {
        self.with_locked(|store| {
            store.prune_expired(now_ms());
            Ok(store.leases.values().cloned().collect())
        })
    }

    pub(super) fn active_leases_read_only_with_cancellation(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<LeaseRecord>> {
        let mut store = self
            .persistence
            .read_only_with_cancellation::<LeaseFile>(cancelled)?;
        ensure_status_active(cancelled)?;
        store.prune_expired(now_ms());
        Ok(store.leases.into_values().collect())
    }

    fn with_locked<T>(&mut self, action: impl FnOnce(&mut LeaseFile) -> Result<T>) -> Result<T> {
        self.persistence.with_locked(action)
    }

    fn validate_parseable_with_cancellation(&self, cancelled: &dyn Fn() -> bool) -> Result<()> {
        self.persistence
            .read_only_with_cancellation::<LeaseFile>(cancelled)
            .map(|_| ())
    }
}

pub(super) struct LeaseGuard {
    store: LeaseStore,
    key: String,
    owner: String,
    stop: Option<Sender<()>>,
    renewal: Option<JoinHandle<Result<()>>>,
    renewal_failed: Arc<AtomicBool>,
    ttl_seconds: u64,
    release_pending: bool,
}

impl LeaseGuard {
    pub(super) fn start(
        store: LeaseStore,
        key: &str,
        lease: &LeaseRecord,
        ttl_seconds: u64,
    ) -> Result<Self> {
        Self::start_with_interval(
            store,
            key,
            lease,
            ttl_seconds,
            renewal_interval(ttl_seconds),
        )
    }

    #[cfg(test)]
    fn start_for_test(
        store: LeaseStore,
        key: &str,
        lease: &LeaseRecord,
        ttl_seconds: u64,
        interval: Duration,
    ) -> Result<Self> {
        Self::start_with_interval(store, key, lease, ttl_seconds, interval)
    }

    fn start_with_interval(
        store: LeaseStore,
        key: &str,
        lease: &LeaseRecord,
        ttl_seconds: u64,
        interval: Duration,
    ) -> Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let mut renewal_store = store.clone();
        let renewal_key = key.to_string();
        let renewal_owner = lease.owner.clone();
        let renewal_failed = Arc::new(AtomicBool::new(false));
        let renewal_failed_in_thread = Arc::clone(&renewal_failed);
        let lease_expires_at_ms = lease.expires_at_ms;
        let renewal = thread::Builder::new()
            .name(format!("jig-loop-lease-{}", lease.owner))
            .spawn(move || {
                run_with_wait(
                    interval,
                    lease_expires_at_ms,
                    &renewal_failed_in_thread,
                    |deadline| {
                        renewal_store
                            .renew_for_guard(&renewal_key, &renewal_owner, ttl_seconds, deadline)
                            .map(|lease| lease.expires_at_ms)
                    },
                    now_ms,
                    |wait| receiver.recv_timeout(wait),
                )
            });
        let renewal = match renewal {
            Ok(renewal) => renewal,
            Err(error) => {
                let mut store = store;
                let release_error = store.release(key, &lease.owner).err();
                let mut message = format!("Failed to start loop lease renewal thread: {error}");
                if let Some(release_error) = release_error {
                    message.push_str(&format!(
                        "; failed to release acquired lease: {release_error:#}"
                    ));
                }
                return Err(anyhow!(message));
            }
        };
        Ok(Self {
            store,
            key: key.to_string(),
            owner: lease.owner.clone(),
            stop: Some(stop),
            renewal: Some(renewal),
            renewal_failed,
            ttl_seconds,
            release_pending: true,
        })
    }

    pub(super) fn renewal_failed(&self) -> bool {
        self.renewal_failed.load(Ordering::Acquire)
    }

    pub(super) fn refresh(&mut self) -> Result<()> {
        self.store
            .renew(&self.key, &self.owner, self.ttl_seconds)
            .map(|_| ())
    }

    pub(super) fn finish(mut self) -> Result<()> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let renewal_result = self
            .renewal
            .take()
            .map(|renewal| {
                renewal
                    .join()
                    .map_err(|_| anyhow!("Loop lease renewal thread panicked"))?
            })
            .transpose();
        let release_result = if self.release_pending {
            let result = self.store.release(&self.key, &self.owner);
            if result.is_ok() {
                self.release_pending = false;
            }
            result
        } else {
            Ok(())
        };
        renewal_result?;
        release_result
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl LeaseFile {
    fn prune_expired(&mut self, now: u64) {
        self.leases.retain(|_, lease| lease.expires_at_ms > now);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct AttemptRecord {
    key: String,
    workflow_id: String,
    item_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) item_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) observed_item_version: Option<String>,
    attempts: u32,
    max_attempts: u32,
    last_attempt_ms: u64,
    pub(super) next_eligible_ms: u64,
    pub(super) exhausted: bool,
    last_status: String,
}

impl AttemptRecord {
    pub(super) fn belongs_to(&self, workflow_id: &str) -> bool {
        self.workflow_id == workflow_id
    }

    pub(super) const fn in_backoff(&self, now_ms: u64) -> bool {
        !self.exhausted && self.next_eligible_ms > now_ms
    }
}

pub(super) struct AttemptSections {
    pub(super) waiting: Vec<AttemptRecord>,
    pub(super) needs_attention: Vec<AttemptRecord>,
}

impl AttemptSections {
    pub(super) fn new(attempts: &[AttemptRecord], now_ms: u64) -> Self {
        Self::new_with_cancellation(attempts, now_ms, &|| false)
            .expect("an always-false callback cannot cancel attempt classification")
    }

    pub(super) fn new_with_cancellation(
        attempts: &[AttemptRecord],
        now_ms: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let mut waiting = Vec::new();
        let mut needs_attention = Vec::new();
        for attempt in attempts {
            ensure_status_active(cancelled)?;
            if attempt.in_backoff(now_ms) {
                waiting.push(attempt.clone());
            }
            if attempt.exhausted {
                needs_attention.push(attempt.clone());
            }
        }
        Ok(Self {
            waiting,
            needs_attention,
        })
    }

    pub(super) fn blocks_idle(&self) -> bool {
        !self.waiting.is_empty() || !self.needs_attention.is_empty()
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct AttemptFile {
    attempts: BTreeMap<String, AttemptRecord>,
}

pub(super) struct AttemptStore {
    persistence: JsonStatePersistence,
}

impl AttemptStore {
    pub(super) fn new(ctx: &RepoContext) -> Self {
        Self {
            persistence: JsonStatePersistence::new(ctx, "attempts"),
        }
    }

    pub(super) fn record_attempt_for_transition(
        &mut self,
        workflow: &ResolvedWorkflow,
        item_key: &str,
        observed_item_version: Option<&str>,
        resulting_item_version: Option<&str>,
        status: &str,
    ) -> Result<AttemptRecord> {
        let key = format!("{}:{item_key}", workflow.id);
        self.with_locked(|store| {
            let now = now_ms();
            let current = store
                .attempts
                .get(&key)
                .map(|record| record.attempts)
                .unwrap_or(0);
            let attempts = current.saturating_add(1);
            let exhausted = attempts >= workflow.max_attempts && status != "passed";
            let item_version = resulting_item_version
                .or(observed_item_version)
                .filter(|version| !version.is_empty())
                .map(str::to_string);
            let observed_item_version = observed_item_version
                .filter(|version| !version.is_empty())
                .filter(|version| Some(*version) != item_version.as_deref())
                .map(str::to_string);
            let record = AttemptRecord {
                key: key.clone(),
                workflow_id: workflow.id.clone(),
                item_key: item_key.to_string(),
                item_version,
                observed_item_version,
                attempts,
                max_attempts: workflow.max_attempts,
                last_attempt_ms: now,
                next_eligible_ms: if exhausted {
                    u64::MAX
                } else {
                    now.saturating_add(workflow.backoff_seconds.saturating_mul(1000))
                },
                exhausted,
                last_status: status.to_string(),
            };
            if status == "passed" {
                store.attempts.remove(&key);
            } else {
                store.attempts.insert(key, record.clone());
            }
            Ok(record)
        })
    }

    #[cfg(test)]
    pub(super) fn get(&self, workflow_id: &str, item_key: &str) -> Result<Option<AttemptRecord>> {
        self.get_with_cancellation(workflow_id, item_key, &|| false)
    }

    pub(super) fn get_with_cancellation(
        &self,
        workflow_id: &str,
        item_key: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<AttemptRecord>> {
        let key = format!("{workflow_id}:{item_key}");
        Ok(self
            .persistence
            .read_locked_with_cancellation::<AttemptFile>(cancelled)?
            .attempts
            .get(&key)
            .cloned())
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> Result<Vec<AttemptRecord>> {
        self.snapshot_read_only_with_cancellation(&|| false)
    }

    pub(super) fn snapshot_read_only_with_cancellation(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<AttemptRecord>> {
        Ok(self
            .persistence
            .read_only_with_cancellation::<AttemptFile>(cancelled)?
            .attempts
            .into_values()
            .collect())
    }

    pub(super) fn clear_attempt_with_cancellation(
        &mut self,
        workflow_id: &str,
        item_key: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<bool> {
        let key = format!("{workflow_id}:{item_key}");
        self.persistence
            .with_locked_with_cancellation(cancelled, |store: &mut AttemptFile| {
                Ok(store.attempts.remove(&key).is_some())
            })
    }

    pub(super) fn clear_attempt_and_then<T>(
        &mut self,
        workflow_id: &str,
        item_key: &str,
        cancelled: &dyn Fn() -> bool,
        after_commit: impl FnOnce(bool, Instant) -> Result<T>,
    ) -> Result<(bool, T)> {
        let key = format!("{workflow_id}:{item_key}");
        self.persistence.with_locked_compensating(
            cancelled,
            |store: &mut AttemptFile| Ok(store.attempts.remove(&key).is_some()),
            |cleared, deadline| after_commit(*cleared, deadline),
        )
    }

    fn with_locked<T>(&mut self, action: impl FnOnce(&mut AttemptFile) -> Result<T>) -> Result<T> {
        self.persistence.with_locked(action)
    }

    fn recover_unparsable_with_cancellation(&self, cancelled: &dyn Fn() -> bool) -> Result<bool> {
        self.persistence
            .recover_unparsable_with_cancellation::<AttemptFile>(cancelled)
    }
}

#[derive(Debug)]
pub(super) struct CoordinationStateRecovery {
    pub(super) attempt_cache_reset: bool,
}

pub(super) fn prepare_coordination_state_for_dispatch(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<CoordinationStateRecovery> {
    LeaseStore::new(ctx).validate_parseable_with_cancellation(cancelled)?;
    Ok(CoordinationStateRecovery {
        attempt_cache_reset: AttemptStore::new(ctx)
            .recover_unparsable_with_cancellation(cancelled)?,
    })
}

pub(super) fn validate_repository_branch_authority_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    LeaseStore::new_repository(ctx).validate_parseable_with_cancellation(cancelled)
}

#[cfg(test)]
pub(super) fn read_json_or_default<T>(path: &Path) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    read_json_or_default_with_cancellation(path, &|| false)
}

#[cfg(test)]
pub(super) fn read_json_or_default_with_cancellation<T>(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    Ok(read_json_if_exists_with_cancellation(path, cancelled)?.unwrap_or_default())
}

#[cfg(test)]
pub(super) fn read_json_if_exists_with_cancellation<T>(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    ensure_status_active(cancelled)?;
    match File::open(path) {
        Ok(mut file) => bounded_json::read_bounded_json(&mut file, path, cancelled).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Failed to read {}", path.display())),
    }
}

fn ensure_status_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::runtime::loops::workflow::NOOP_STATUS_KIND;

    #[test]
    fn attempt_store_exhausts_after_budget_and_clears_on_success() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = AttemptStore::new(&ctx);
        let workflow = ResolvedWorkflow {
            id: "wf".into(),
            kind: NOOP_STATUS_KIND.into(),
            enabled: true,
            configured: false,
            lease_ttl_seconds: 60,
            max_attempts: 2,
            backoff_seconds: 1,
            codex_home_configured: None,
            schedule: None,
            codex_task: None,
        };

        let first = store
            .record_attempt_for_transition(&workflow, "item-1", None, None, "failed")
            .unwrap();
        assert_eq!(first.attempts, 1);
        assert!(!first.exhausted);

        let second = store
            .record_attempt_for_transition(&workflow, "item-1", None, None, "failed")
            .unwrap();
        assert_eq!(second.attempts, 2);
        assert!(second.exhausted);

        store
            .record_attempt_for_transition(&workflow, "item-1", None, None, "passed")
            .unwrap();
        assert!(store.snapshot().unwrap().is_empty());
    }

    #[test]
    fn lease_guard_renews_until_finished_and_then_releases() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = LeaseStore::new(&ctx);
        let LeaseAcquire::Acquired(lease) = store.acquire("workflow:slow", 3).unwrap() else {
            panic!("expected lease acquisition");
        };
        let guard = LeaseGuard::start(store.clone(), "workflow:slow", &lease, 3).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(3_500));
        let LeaseAcquire::Held(renewed) = store.acquire("workflow:slow", 3).unwrap() else {
            panic!("renewed lease must still be held");
        };
        assert_eq!(renewed.owner, lease.owner);

        guard.finish().unwrap();
        assert!(matches!(
            store.acquire("workflow:slow", 3).unwrap(),
            LeaseAcquire::Acquired(_)
        ));
    }

    #[test]
    fn lease_guard_shutdown_releases_at_most_once() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = LeaseStore::new(&ctx);
        let LeaseAcquire::Acquired(lease) = store.acquire("workflow:once", 60).unwrap() else {
            panic!("expected lease acquisition");
        };
        let mut guard = LeaseGuard::start(store, "workflow:once", &lease, 60).unwrap();

        guard.shutdown().unwrap();
        guard.shutdown().unwrap();
    }

    #[test]
    fn attempt_store_reads_do_not_rewrite_the_cache() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let store = AttemptStore::new(&ctx);
        let path = store.persistence.legacy_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{\n  \"attempts\": {}\n}\n";
        fs::write(path, original).unwrap();

        assert!(store.get("workflow", "item").unwrap().is_none());
        assert!(store.snapshot().unwrap().is_empty());
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn lease_guard_renews_the_persisted_lease_before_expiry() {
        assert!(renewal_interval(1) < Duration::from_secs(1));

        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = LeaseStore::new(&ctx);
        let LeaseAcquire::Acquired(lease) = store.acquire("workflow:short", 60).unwrap() else {
            panic!("expected lease acquisition");
        };
        let guard = LeaseGuard::start_for_test(
            store.clone(),
            "workflow:short",
            &lease,
            60,
            Duration::from_millis(1),
        )
        .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let renewed = loop {
            let LeaseAcquire::Held(renewed) = store.acquire("workflow:short", 60).unwrap() else {
                panic!("renewed lease must remain held by its original owner");
            };
            if renewed.expires_at_ms > lease.expires_at_ms {
                break renewed;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "lease renewal was not persisted before the test deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(renewed.owner, lease.owner);
        guard.finish().unwrap();
    }

    #[test]
    fn lease_guard_reports_renewal_failure() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = LeaseStore::new(&ctx);
        let LeaseAcquire::Acquired(lease) = store.acquire("workflow:lost", 1).unwrap() else {
            panic!("expected lease acquisition");
        };
        let guard = LeaseGuard::start(store.clone(), "workflow:lost", &lease, 1).unwrap();
        store.release("workflow:lost", &lease.owner).unwrap();

        std::thread::sleep(Duration::from_millis(400));
        let error = guard.finish().unwrap_err().to_string();
        assert!(error.contains("Loop lease is no longer held"), "{error}");
    }

    fn write_loop_fixture_repo(root: &Path) {
        crate::test_env::TestRepoBuilder::new(root)
            .required_commands(Vec::<String>::new())
            .write();
    }
}
#[cfg(test)]
#[path = "state/review_tests.rs"]
mod review_tests;
