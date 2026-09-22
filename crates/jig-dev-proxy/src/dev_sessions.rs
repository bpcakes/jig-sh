use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};

use self::claim::{ClaimOutcome, claim_session_interruptible};
use self::management::{StopSessionOutcome, stop_session_ids_interruptible};
use self::process_identity::capture_process_identity;
use crate::session_control::SessionControlServer;
use crate::state::{
    DevProcessIdentity, DevSessionApp, DevSessionControl, DevSessionPhase, DevSessionRecord,
    LockOutcome, StateStore, now_ms,
};
use crate::types::{AppRunSpec, Route, RouteMode};

mod assessment;
mod claim;
mod management;
mod process_identity;

pub(crate) use management::{
    OrphanRecoveryNotice, recover_session, status, status_all, status_session, stop, stop_session,
};

const SESSION_ID_RANDOM_BYTES: usize = 16;
// Exact, repository-independent legacy repair is available before enabling this
// writer cutover. T-02 installs the reader and serialized promotion path.
const COMPLETE_EVIDENCE_CUTOVER_ENABLED: bool = true;

pub(crate) struct DevSessionRuntime {
    store: StateStore,
    session_id: String,
    repo_root_identity: String,
    supervisor: DevProcessIdentity,
    control: SessionControlServer,
    pending_cleanup: Arc<AtomicUsize>,
    replacement_recoveries: Vec<OrphanRecoveryNotice>,
}

pub(crate) struct DevCleanupLease {
    pending_cleanup: Arc<AtomicUsize>,
    confirmed: bool,
}

pub(crate) enum DevSessionStartOutcome {
    Claimed(DevSessionRuntime),
    Cancelled(Vec<OrphanRecoveryNotice>),
}

impl DevCleanupLease {
    pub(crate) fn confirm(&mut self) {
        if self.confirmed {
            return;
        }
        let previous = self.pending_cleanup.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "dev cleanup lease counter underflow");
        self.confirmed = true;
    }
}

impl DevSessionRuntime {
    #[cfg(test)]
    pub(crate) fn start(
        store: StateStore,
        repo_name: &str,
        root: &Path,
        specs: &[AppRunSpec],
        replace: bool,
    ) -> Result<Self> {
        match Self::start_interruptible(store, repo_name, root, specs, replace, &|| false)? {
            DevSessionStartOutcome::Claimed(session) => Ok(session),
            DevSessionStartOutcome::Cancelled(_) => {
                bail!("uncancelled Jig dev session startup was cancelled")
            }
        }
    }

    pub(crate) fn start_interruptible(
        store: StateStore,
        repo_name: &str,
        root: &Path,
        specs: &[AppRunSpec],
        replace: bool,
        cancelled: &impl Fn() -> bool,
    ) -> Result<DevSessionStartOutcome> {
        let repo = CanonicalRepo::resolve(repo_name, root)?;
        let session_id = new_session_id()?;
        let control = SessionControlServer::start(&session_id)?;
        let supervisor = capture_process_identity(std::process::id());
        let timestamp = now_ms();
        let record = DevSessionRecord {
            session_id: session_id.clone(),
            repo_name: repo.name.clone(),
            repo_root_display: repo.root_display.clone(),
            repo_root_identity: repo.root_identity.clone(),
            phase: DevSessionPhase::Starting,
            started_at_ms: timestamp,
            updated_at_ms: timestamp,
            cleanup_required: false,
            preflight_cleanup_pending: Some(false),
            supervisor: supervisor.clone(),
            control: DevSessionControl {
                port: control.port(),
                token: control.token().to_owned(),
            },
            apps: specs
                .iter()
                .map(|spec| DevSessionApp {
                    name: spec.name.clone(),
                    hostname: spec.proxy.then(|| spec.hostname.clone()),
                    target_host: spec.target_host.clone(),
                    target_port: spec.explicit_port,
                    spawn_state_tracked: true,
                    spawn_pending: false,
                    process: None,
                })
                .collect(),
        };
        let mut replacement_recoveries = Vec::new();

        let first_claim = match claim_session_interruptible(&store, &record, cancelled)? {
            LockOutcome::Acquired(claim) => claim,
            LockOutcome::Cancelled => return Ok(DevSessionStartOutcome::Cancelled(Vec::new())),
        };
        match first_claim {
            ClaimOutcome::Claimed(recoveries) => replacement_recoveries.extend(recoveries),
            ClaimOutcome::Conflicted(conflicts) if !replace => {
                return Err(conflicts.launch_error(false, store.root()));
            }
            ClaimOutcome::Conflicted(conflicts) => {
                if conflicts.has_unsafe_replacement() {
                    return Err(conflicts.launch_error(true, store.root()));
                }
                let target_ids = conflicts.same_repo_session_ids();
                let target_session_ids = target_ids.iter().cloned().collect::<Vec<_>>().join(", ");
                if cancelled() {
                    return Ok(DevSessionStartOutcome::Cancelled(replacement_recoveries));
                }
                let stop = match stop_session_ids_interruptible(
                    &store,
                    &repo,
                    &target_ids,
                    cancelled,
                ) {
                    StopSessionOutcome::Complete(stop) => stop,
                    StopSessionOutcome::Cancelled(progress) => {
                        let (recoveries, warnings) = progress.into_parts();
                        replacement_recoveries.extend(recoveries);
                        for warning in warnings {
                            eprintln!(
                                "jig dev --replace stop warning before cancellation: {warning}"
                            );
                        }
                        return Ok(DevSessionStartOutcome::Cancelled(replacement_recoveries));
                    }
                    StopSessionOutcome::Failed { error, progress } => {
                        let (recoveries, warnings) = progress.into_parts();
                        replacement_recoveries.extend(recoveries);
                        let error = attach_replacement_stop_warnings(error, &warnings);
                        let error = error.context(format!(
                                "Could not replace the existing Jig dev session safely (attempted session IDs: {target_session_ids}; final blockers could not be confirmed; state directory {}); inspect with `jig dev status --state-dir PATH`",
                                store.root().display()
                            ));
                        return Err(crate::dev_outcome::with_recovery_notices(
                            error,
                            replacement_recoveries,
                        ));
                    }
                };
                for recovery in &stop.recoveries {
                    eprintln!("jig dev --replace recovery: {}", recovery.message);
                }
                replacement_recoveries.extend(stop.recoveries.iter().cloned());
                if !stop.ok {
                    let blocker_ids = stop.remaining_session_ids().join(", ");
                    let error = anyhow!(
                        "Could not replace the existing Jig dev session safely (blocking session IDs: {blocker_ids}; state directory {}): {}. Inspect with `jig dev status --state-dir PATH`",
                        store.root().display(),
                        stop.warnings.join("; ")
                    );
                    return Err(crate::dev_outcome::with_recovery_notices(
                        error,
                        replacement_recoveries,
                    ));
                }
                let second_claim = match claim_session_interruptible(&store, &record, cancelled) {
                    Ok(claim) => claim,
                    Err(error) => {
                        return Err(crate::dev_outcome::with_recovery_notices(
                            error,
                            replacement_recoveries,
                        ));
                    }
                };
                match second_claim {
                    LockOutcome::Cancelled => {
                        return Ok(DevSessionStartOutcome::Cancelled(replacement_recoveries));
                    }
                    LockOutcome::Acquired(ClaimOutcome::Claimed(recoveries)) => {
                        replacement_recoveries.extend(recoveries);
                    }
                    LockOutcome::Acquired(ClaimOutcome::Conflicted(conflicts)) => {
                        return Err(crate::dev_outcome::with_recovery_notices(
                            conflicts.concurrent_launch_error(store.root()),
                            replacement_recoveries,
                        ));
                    }
                }
            }
        }

        Ok(DevSessionStartOutcome::Claimed(Self {
            store,
            session_id,
            repo_root_identity: repo.root_identity,
            supervisor,
            control,
            pending_cleanup: Arc::new(AtomicUsize::new(0)),
            replacement_recoveries,
        }))
    }

    pub(crate) fn requested_stop(&self) -> bool {
        self.control.stop_requested()
    }

    pub(crate) fn replacement_recoveries(&self) -> &[OrphanRecoveryNotice] {
        &self.replacement_recoveries
    }

    pub(crate) fn arm_cleanup(&self) -> DevCleanupLease {
        self.pending_cleanup.fetch_add(1, Ordering::AcqRel);
        DevCleanupLease {
            pending_cleanup: Arc::clone(&self.pending_cleanup),
            confirmed: false,
        }
    }

    pub(crate) fn cleanup_is_confirmed(&self) -> bool {
        self.pending_cleanup.load(Ordering::Acquire) == 0
    }

    #[cfg(test)]
    pub(crate) fn begin_preflight_cleanup(&self) -> Result<DevCleanupLease> {
        match self.begin_preflight_cleanup_interruptible(&|| false)? {
            LockOutcome::Acquired(cleanup) => Ok(cleanup),
            LockOutcome::Cancelled => bail!("uncancelled preflight cleanup setup was cancelled"),
        }
    }

    pub(crate) fn begin_preflight_cleanup_interruptible(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<DevCleanupLease>> {
        let mut cleanup = self.arm_cleanup();
        let outcome = self
            .store
            .mutate_dev_sessions_interruptible(cancelled, |sessions, _| {
                self.begin_preflight_cleanup_in(sessions)
            });
        match outcome {
            Ok(LockOutcome::Acquired(())) => Ok(LockOutcome::Acquired(cleanup)),
            Ok(LockOutcome::Cancelled) => {
                cleanup.confirm();
                Ok(LockOutcome::Cancelled)
            }
            Err(error) => {
                cleanup.confirm();
                Err(error)
            }
        }
    }

    fn begin_preflight_cleanup_in(&self, sessions: &mut [DevSessionRecord]) -> Result<()> {
        let session = exact_session_mut(
            sessions,
            &self.session_id,
            &self.repo_root_identity,
            &self.supervisor,
        )?;
        if session.preflight_cleanup_pending == Some(true) {
            bail!(
                "Jig dev session '{}' already has pending preflight cleanup",
                self.session_id
            );
        }
        session.cleanup_required = true;
        session.preflight_cleanup_pending = Some(true);
        session.updated_at_ms = next_timestamp(session.updated_at_ms);
        Ok(())
    }

    pub(crate) fn confirm_preflight_cleanup_cancelable(
        &self,
        cleanup: &mut DevCleanupLease,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<()>> {
        let outcome =
            self.store
                .mutate_dev_sessions_cleanup_cancelable(cancelled, |sessions, _| {
                    let session = exact_session_mut(
                        sessions,
                        &self.session_id,
                        &self.repo_root_identity,
                        &self.supervisor,
                    )?;
                    if session.preflight_cleanup_pending != Some(true) {
                        bail!(
                            "Jig dev session '{}' has no pending preflight cleanup to confirm",
                            self.session_id
                        );
                    }
                    session.preflight_cleanup_pending = Some(false);
                    session.updated_at_ms = next_timestamp(session.updated_at_ms);
                    Ok(())
                })?;
        if outcome.is_some() {
            cleanup.confirm();
        }
        Ok(outcome)
    }

    #[cfg(test)]
    pub(crate) fn prepare_app_spawn(&self, app_name: &str, target_port: u16) -> Result<()> {
        self.store.mutate_dev_sessions(|sessions, _| {
            self.prepare_app_spawn_in(sessions, app_name, target_port)
        })
    }

    pub(crate) fn prepare_app_spawn_interruptible(
        &self,
        app_name: &str,
        target_port: u16,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<()>> {
        self.store
            .mutate_dev_sessions_interruptible(cancelled, |sessions, _| {
                self.prepare_app_spawn_in(sessions, app_name, target_port)
            })
    }

    fn prepare_app_spawn_in(
        &self,
        sessions: &mut [DevSessionRecord],
        app_name: &str,
        target_port: u16,
    ) -> Result<()> {
        let session = exact_session_mut(
            sessions,
            &self.session_id,
            &self.repo_root_identity,
            &self.supervisor,
        )?;
        let app = session
            .apps
            .iter_mut()
            .find(|app| app.name == app_name)
            .ok_or_else(|| {
                anyhow!(
                    "Jig dev session '{}' did not contain configured app '{}'",
                    self.session_id,
                    app_name
                )
            })?;
        app.prepare_spawn(&self.session_id, target_port)?;
        session.cleanup_required = true;
        session.updated_at_ms = next_timestamp(session.updated_at_ms);
        Ok(())
    }

    pub(crate) fn record_app_process_cleanup_cancelable(
        &self,
        app_name: &str,
        target_port: u16,
        process: DevProcessIdentity,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<()>> {
        self.store
            .mutate_dev_sessions_cleanup_cancelable(cancelled, |sessions, _| {
                self.record_app_process_in(sessions, app_name, target_port, process)
            })
    }

    pub(crate) fn record_app_process_interruptible(
        &self,
        app_name: &str,
        target_port: u16,
        process: DevProcessIdentity,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<()>> {
        self.store
            .mutate_dev_sessions_interruptible(cancelled, |sessions, _| {
                self.record_app_process_in(sessions, app_name, target_port, process)
            })
    }

    fn record_app_process_in(
        &self,
        sessions: &mut [DevSessionRecord],
        app_name: &str,
        target_port: u16,
        process: DevProcessIdentity,
    ) -> Result<()> {
        let session = exact_session_mut(
            sessions,
            &self.session_id,
            &self.repo_root_identity,
            &self.supervisor,
        )?;
        let app = session
            .apps
            .iter_mut()
            .find(|app| app.name == app_name)
            .ok_or_else(|| {
                anyhow!(
                    "Jig dev session '{}' did not contain configured app '{}'",
                    self.session_id,
                    app_name
                )
            })?;
        app.register_process(target_port, process);
        session.cleanup_required = true;
        session.updated_at_ms = next_timestamp(session.updated_at_ms);
        Ok(())
    }

    pub(crate) fn confirm_app_spawn_absent_cleanup_cancelable(
        &self,
        app_name: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<()>> {
        self.store
            .mutate_dev_sessions_cleanup_cancelable(cancelled, |sessions, _| {
                let session = exact_session_mut(
                    sessions,
                    &self.session_id,
                    &self.repo_root_identity,
                    &self.supervisor,
                )?;
                let app = session
                    .apps
                    .iter_mut()
                    .find(|app| app.name == app_name)
                    .ok_or_else(|| {
                        anyhow!(
                            "Jig dev session '{}' did not contain configured app '{}'",
                            self.session_id,
                            app_name
                        )
                    })?;
                app.confirm_spawn_absent(&self.session_id)?;
                session.updated_at_ms = next_timestamp(session.updated_at_ms);
                Ok(())
            })
    }

    pub(crate) fn mark_running_interruptible(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<()>> {
        self.store
            .mutate_dev_sessions_interruptible(cancelled, |sessions, _| {
                self.mark_running_in(sessions)
            })
    }

    fn mark_running_in(&self, sessions: &mut [DevSessionRecord]) -> Result<()> {
        let session = exact_session_mut(
            sessions,
            &self.session_id,
            &self.repo_root_identity,
            &self.supervisor,
        )?;
        session.phase = DevSessionPhase::Running;
        session.updated_at_ms = next_timestamp(session.updated_at_ms);
        Ok(())
    }

    fn retire(&self) -> Result<Option<bool>> {
        self.store.mutate_dev_sessions_cleanup_cancelable(
            &crate::processes::force_cleanup_requested,
            |sessions, _| {
                let Some(index) = sessions.iter().position(|session| {
                    session.session_id == self.session_id
                        && session.repo_root_identity == self.repo_root_identity
                        && session.supervisor == self.supervisor
                }) else {
                    return Ok(true);
                };
                if !self.cleanup_is_confirmed() {
                    sessions[index].phase = DevSessionPhase::Orphaned;
                    sessions[index].cleanup_required = true;
                    sessions[index].updated_at_ms = next_timestamp(sessions[index].updated_at_ms);
                    return Ok(false);
                }
                sessions.remove(index);
                Ok(true)
            },
        )
    }
}

fn attach_replacement_stop_warnings(error: anyhow::Error, warnings: &[String]) -> anyhow::Error {
    if warnings.is_empty() {
        error
    } else {
        error.context(format!(
            "Jig dev replacement stop reported warnings before the failure: {}",
            warnings.join("; ")
        ))
    }
}

#[cfg(test)]
mod stop_progress_tests;

impl Drop for DevSessionRuntime {
    fn drop(&mut self) {
        match self.retire() {
            Ok(Some(true)) => {}
            Ok(Some(false)) => eprintln!(
                "jig dev retained session '{}' because process-tree or route cleanup was not confirmed; inspect `jig dev status`",
                self.session_id
            ),
            Ok(None) => eprintln!(
                "jig dev retained session '{}' because forced cleanup cancelled a contended session-state update; inspect `jig dev status`",
                self.session_id
            ),
            Err(error) => eprintln!(
                "jig dev could not retire session '{}' from private runtime state: {error:#}",
                self.session_id
            ),
        }
    }
}

#[derive(Clone)]
pub(super) struct CanonicalRepo {
    pub(super) name: String,
    pub(super) root_display: String,
    pub(super) root_identity: String,
}

impl CanonicalRepo {
    pub(super) fn resolve(name: &str, root: &Path) -> Result<Self> {
        let root = fs::canonicalize(root)
            .with_context(|| format!("Failed to canonicalize repo root {}", root.display()))?;
        let root_display = root.to_string_lossy().into_owned();
        let root_identity = canonical_root_identity(&root);
        Ok(Self {
            name: name.to_owned(),
            root_display,
            root_identity,
        })
    }

    pub(super) fn from_record(session: &DevSessionRecord) -> Self {
        Self {
            name: session.repo_name.clone(),
            root_display: session.repo_root_display.clone(),
            root_identity: session.repo_root_identity.clone(),
        }
    }
}

fn exact_session_mut<'a>(
    sessions: &'a mut [DevSessionRecord],
    session_id: &str,
    repo_root_identity: &str,
    supervisor: &DevProcessIdentity,
) -> Result<&'a mut DevSessionRecord> {
    sessions
        .iter_mut()
        .find(|session| {
            session.session_id == session_id
                && session.repo_root_identity == repo_root_identity
                && session.supervisor == *supervisor
        })
        .ok_or_else(|| anyhow!("Jig dev session '{session_id}' is no longer registered"))
}

fn session_owns_route(session: &DevSessionRecord, route: &Route) -> bool {
    route.mode == RouteMode::Process
        && session.apps.iter().any(|app| {
            app.hostname.as_deref() == Some(route.hostname.as_str())
                && app.process.as_ref().is_some_and(|identity| {
                    route.owner_pid == Some(identity.pid)
                        && route.owner_start_token == identity.start_token
                })
        })
}

fn next_timestamp(previous: u64) -> u64 {
    previous.max(now_ms())
}

fn new_session_id() -> Result<String> {
    let mut random = [0_u8; SESSION_ID_RANDOM_BYTES];
    getrandom::fill(&mut random)
        .map_err(|error| anyhow!("failed to generate a Jig dev session id: {error}"))?;
    let mut id = String::from("dev_");
    for byte in random {
        write!(&mut id, "{byte:02x}")?;
    }
    Ok(id)
}

fn canonical_root_identity(root: &Path) -> String {
    let mut digest = Sha256::new();
    digest.update(b"jig-dev-repo-root-v1\0");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        digest.update(root.as_os_str().as_bytes());
    }
    #[cfg(not(unix))]
    digest.update(root.to_string_lossy().as_bytes());
    format!("sha256:{:x}", digest.finalize())
}
