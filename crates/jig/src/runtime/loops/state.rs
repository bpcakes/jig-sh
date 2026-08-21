use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ulid::Ulid;

use crate::cancellation::ensure_status_collection_active;
use crate::context::RepoContext;
use crate::state::now_ms;

use super::workflow::ResolvedWorkflow;

pub(super) const LOOP_CACHE_DIR: &str = ".agent/.cache/loop";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct LeaseRecord {
    key: String,
    pub(super) owner: String,
    acquired_at_ms: u64,
    expires_at_ms: u64,
}

#[derive(Default, Deserialize, Serialize)]
struct LeaseFile {
    leases: BTreeMap<String, LeaseRecord>,
}

pub(super) enum LeaseAcquire {
    Acquired(LeaseRecord),
    Held(LeaseRecord),
}

#[derive(Clone)]
pub(super) struct LeaseStore {
    dir: PathBuf,
    path: PathBuf,
    lock_path: PathBuf,
}

impl LeaseStore {
    pub(super) fn new(ctx: &RepoContext) -> Self {
        let dir = ctx.root().join(LOOP_CACHE_DIR);
        Self {
            path: dir.join("leases.json"),
            lock_path: dir.join("leases.lock"),
            dir,
        }
    }

    pub(super) fn acquire(&mut self, key: &str, ttl_seconds: u64) -> Result<LeaseAcquire> {
        self.with_locked(|store| {
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
        self.with_locked(|store| {
            if store
                .leases
                .get(key)
                .is_some_and(|lease| lease.owner == owner)
            {
                store.leases.remove(key);
            }
            Ok(())
        })
    }

    pub(super) fn renew(
        &mut self,
        key: &str,
        owner: &str,
        ttl_seconds: u64,
    ) -> Result<LeaseRecord> {
        self.renew_at(key, owner, ttl_seconds, now_ms())
    }

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
        let mut store = read_json_or_default_with_cancellation::<LeaseFile>(&self.path, cancelled)?;
        ensure_status_active(cancelled)?;
        store.prune_expired(now_ms());
        Ok(store.leases.into_values().collect())
    }

    fn with_locked<T>(&mut self, action: impl FnOnce(&mut LeaseFile) -> Result<T>) -> Result<T> {
        with_json_cache_lock(&self.dir, &self.lock_path, &self.path, action)
    }

    fn renew_at(
        &mut self,
        key: &str,
        owner: &str,
        ttl_seconds: u64,
        now: u64,
    ) -> Result<LeaseRecord> {
        self.with_locked(|store| {
            let lease = store
                .leases
                .get_mut(key)
                .ok_or_else(|| anyhow!("Loop lease is no longer held: {key}"))?;
            if lease.owner != owner {
                return Err(anyhow!("Loop lease '{key}' is owned by another worker"));
            }
            if lease.expires_at_ms <= now {
                return Err(anyhow!("Loop lease expired before renewal: {key}"));
            }
            lease.expires_at_ms = now.saturating_add(ttl_seconds.saturating_mul(1_000));
            Ok(lease.clone())
        })
    }
}

pub(super) struct LeaseGuard {
    store: LeaseStore,
    key: String,
    owner: String,
    stop: Option<Sender<()>>,
    renewal: Option<JoinHandle<Result<()>>>,
}

impl LeaseGuard {
    pub(super) fn start(
        store: LeaseStore,
        key: &str,
        lease: &LeaseRecord,
        ttl_seconds: u64,
    ) -> Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let mut renewal_store = store.clone();
        let renewal_key = key.to_string();
        let renewal_owner = lease.owner.clone();
        let interval = Duration::from_secs((ttl_seconds / 3).max(1));
        let renewal = thread::Builder::new()
            .name(format!("jig-loop-lease-{}", lease.owner))
            .spawn(move || {
                loop {
                    match receiver.recv_timeout(interval) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(RecvTimeoutError::Timeout) => {
                            renewal_store.renew(&renewal_key, &renewal_owner, ttl_seconds)?;
                        }
                    }
                }
            })
            .context("Failed to start loop lease renewal thread")?;
        Ok(Self {
            store,
            key: key.to_string(),
            owner: lease.owner.clone(),
            stop: Some(stop),
            renewal: Some(renewal),
        })
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
        let release_result = self.store.release(&self.key, &self.owner);
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
    attempts: u32,
    max_attempts: u32,
    last_attempt_ms: u64,
    pub(super) next_eligible_ms: u64,
    pub(super) exhausted: bool,
    last_status: String,
}

impl AttemptRecord {
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

#[derive(Default, Deserialize, Serialize)]
struct AttemptFile {
    attempts: BTreeMap<String, AttemptRecord>,
}

pub(super) struct AttemptStore {
    dir: PathBuf,
    path: PathBuf,
    lock_path: PathBuf,
}

impl AttemptStore {
    pub(super) fn new(ctx: &RepoContext) -> Self {
        let dir = ctx.root().join(LOOP_CACHE_DIR);
        Self {
            path: dir.join("attempts.json"),
            lock_path: dir.join("attempts.lock"),
            dir,
        }
    }

    pub(super) fn record_attempt_for_version(
        &mut self,
        workflow: &ResolvedWorkflow,
        item_key: &str,
        item_version: Option<&str>,
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
            let record = AttemptRecord {
                key: key.clone(),
                workflow_id: workflow.id.clone(),
                item_key: item_key.to_string(),
                item_version: item_version
                    .filter(|version| !version.is_empty())
                    .map(str::to_string),
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

    pub(super) fn get(
        &mut self,
        workflow_id: &str,
        item_key: &str,
    ) -> Result<Option<AttemptRecord>> {
        let key = format!("{workflow_id}:{item_key}");
        self.with_locked(|store| Ok(store.attempts.get(&key).cloned()))
    }

    pub(super) fn snapshot(&mut self) -> Result<Vec<AttemptRecord>> {
        self.with_locked(|store| Ok(store.attempts.values().cloned().collect()))
    }

    pub(super) fn snapshot_read_only_with_cancellation(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<AttemptRecord>> {
        Ok(
            read_json_or_default_with_cancellation::<AttemptFile>(&self.path, cancelled)?
                .attempts
                .into_values()
                .collect(),
        )
    }

    pub(super) fn clear_attempt(&mut self, workflow_id: &str, item_key: &str) -> Result<bool> {
        let key = format!("{workflow_id}:{item_key}");
        self.with_locked(|store| Ok(store.attempts.remove(&key).is_some()))
    }

    fn with_locked<T>(&mut self, action: impl FnOnce(&mut AttemptFile) -> Result<T>) -> Result<T> {
        with_json_cache_lock(&self.dir, &self.lock_path, &self.path, action)
    }
}

pub(super) fn with_json_cache_lock<T, S>(
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
    action: impl FnOnce(&mut S) -> Result<T>,
) -> Result<T>
where
    S: Default + DeserializeOwned + Serialize,
{
    fs::create_dir_all(dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .with_context(|| format!("Failed to open loop cache lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("Failed to lock {}", lock_path.display()))?;

    let mut store = read_json_or_default(data_path)?;
    let result = action(&mut store)?;
    write_json(data_path, &store)?;
    drop(lock);
    Ok(result)
}

fn read_json_or_default<T>(path: &Path) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    read_json_or_default_with_cancellation(path, &|| false)
}

pub(super) fn read_json_or_default_with_cancellation<T>(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    ensure_status_active(cancelled)?;
    match File::open(path) {
        Ok(mut file) => {
            let mut bytes = Vec::new();
            let mut chunk = vec![0_u8; 64 * 1024].into_boxed_slice();
            loop {
                ensure_status_active(cancelled)?;
                let read = file
                    .read(&mut chunk)
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..read]);
            }
            ensure_status_active(cancelled)?;
            serde_json::from_slice(&bytes)
                .with_context(|| format!("Failed to parse {}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error).with_context(|| format!("Failed to read {}", path.display())),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(anyhow!("Loop cache path has no parent: {}", path.display()));
    };
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    let tmp = path.with_extension(format!("tmp-{}", Ulid::new()));
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(value).context("Failed to encode loop cache JSON")?,
    )
    .with_context(|| format!("Failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "Failed to replace loop cache file {} with {}",
            path.display(),
            tmp.display()
        )
    })
}

fn ensure_status_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::Value;
    use tempfile::tempdir;

    use super::*;
    use crate::runtime::loops::workflow::NOOP_STATUS_KIND;

    #[test]
    fn read_only_loop_cache_scan_observes_cancellation_between_chunks() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("large.json");
        fs::write(
            &path,
            format!("{{\"padding\":\"{}\"}}", "x".repeat(256 * 1024)),
        )
        .unwrap();
        let checks = AtomicUsize::new(0);

        let error = read_json_or_default_with_cancellation::<Value>(&path, &|| {
            checks.fetch_add(1, Ordering::SeqCst) >= 2
        })
        .unwrap_err();

        assert_eq!(error.to_string(), "status collection was cancelled");
    }

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
            .record_attempt_for_version(&workflow, "item-1", None, "failed")
            .unwrap();
        assert_eq!(first.attempts, 1);
        assert!(!first.exhausted);

        let second = store
            .record_attempt_for_version(&workflow, "item-1", None, "failed")
            .unwrap();
        assert_eq!(second.attempts, 2);
        assert!(second.exhausted);

        store
            .record_attempt_for_version(&workflow, "item-1", None, "passed")
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
    fn lease_renewal_is_owner_checked_and_cannot_revive_expired_lease() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = LeaseStore::new(&ctx);
        let LeaseAcquire::Acquired(lease) = store.acquire("workflow:owner", 60).unwrap() else {
            panic!("expected lease acquisition");
        };

        assert!(
            store
                .renew_at(
                    "workflow:owner",
                    "another-owner",
                    60,
                    lease.acquired_at_ms + 1
                )
                .unwrap_err()
                .to_string()
                .contains("owned by another worker")
        );
        assert!(
            store
                .renew_at("workflow:owner", &lease.owner, 60, lease.expires_at_ms)
                .unwrap_err()
                .to_string()
                .contains("expired before renewal")
        );
    }

    fn write_loop_fixture_repo(root: &Path) {
        crate::test_env::TestRepoBuilder::new(root)
            .required_commands(Vec::<String>::new())
            .write();
    }
}
