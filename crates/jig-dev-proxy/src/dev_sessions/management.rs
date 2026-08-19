use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};

use super::process_identity::{
    ProcessIdentityObservation, observe_process_identity, process_identity_may_be_alive,
};
use super::{CanonicalRepo, next_timestamp, session_owns_route};
use crate::session_control::{ping, request_stop};
use crate::state::{
    DevSessionApp, DevSessionAppSpawnEvidence, DevSessionPhase, DevSessionRecord, LockOutcome,
    StateStore,
};
use crate::types::{DevStatusRequest, DevStopRequest, Route};

const CONTROL_RETIRE_BASE_TIMEOUT: Duration = Duration::from_secs(35);
const CONTROL_RETIRE_PER_APP_TIMEOUT: Duration = Duration::from_secs(15);
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Eq, PartialEq)]
enum OrphanRetentionReason {
    SupervisorAlive,
    SupervisorUncertain,
    PreflightCleanupPending,
    AppAlive(String),
    AppUncertain(String),
    AppSpawnPending(String),
    AppSpawnUntracked(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OrphanRecoveryAssessment {
    Retirable,
    Retain(OrphanRetentionReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RetireDeadOrphanOutcome {
    Retired(OrphanRecoveryNotice),
    AlreadyAbsent,
    Retained(OrphanRetentionReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AmbiguousOrphanPolicy {
    Retain,
    Forget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ForgottenCleanupAmbiguity {
    PreflightCleanup,
    SpawnHistory,
}

impl ForgottenCleanupAmbiguity {
    const fn label(self) -> &'static str {
        match self {
            Self::PreflightCleanup => "preflight-cleanup",
            Self::SpawnHistory => "spawn-history",
        }
    }

    const fn diagnostic(self) -> &'static str {
        match self {
            Self::PreflightCleanup => "unconfirmed preflight cleanup",
            Self::SpawnHistory => "ambiguous spawn history",
        }
    }
}

pub(crate) fn status(request: DevStatusRequest) -> Result<Value> {
    let repo = CanonicalRepo::resolve(&request.repo_name, &request.root)?;
    let Some(store) = StateStore::resolve_existing(request.state_dir.clone())? else {
        return Ok(empty_status(
            &repo,
            configured_state_dir(request.state_dir)?,
        ));
    };
    let snapshot = store.snapshot_dev_state()?;
    let routes = &snapshot.routes;
    let sessions = snapshot
        .sessions
        .iter()
        .filter(|session| session.repo_root_identity == repo.root_identity)
        .map(|session| session_status(session, routes))
        .collect::<Vec<_>>();
    let running = sessions
        .iter()
        .any(|session| !matches!(session["status"].as_str(), Some("stale" | "recoverable")));
    Ok(json!({
        "ok": true,
        "command": "dev status",
        "repo_name": repo.name,
        "repo_root": repo.root_display,
        "state_dir": store.root(),
        "running": running,
        "sessions": sessions,
    }))
}

pub(crate) fn stop(request: DevStopRequest) -> Result<Value> {
    let repo = CanonicalRepo::resolve(&request.repo_name, &request.root)?;
    let Some(store) = StateStore::resolve_existing(request.state_dir.clone())? else {
        return Ok(empty_stop(&repo, configured_state_dir(request.state_dir)?));
    };
    let ids = store
        .snapshot_dev_state()?
        .sessions
        .into_iter()
        .filter(|session| session.repo_root_identity == repo.root_identity)
        .map(|session| session.session_id)
        .collect::<BTreeSet<_>>();
    let policy = if request.forget_ambiguous_orphans {
        AmbiguousOrphanPolicy::Forget
    } else {
        AmbiguousOrphanPolicy::Retain
    };
    let report = stop_session_ids(&store, &repo, &ids, policy)?;
    Ok(report.into_json(&repo, store.root()))
}

#[derive(Default)]
pub(super) struct StopReport {
    pub(super) ok: bool,
    matched_sessions: usize,
    stopped_sessions: usize,
    stopped_apps: usize,
    sessions: Vec<Value>,
    pub(super) recoveries: Vec<OrphanRecoveryNotice>,
    pub(super) warnings: Vec<String>,
}

pub(super) enum StopSessionOutcome {
    Complete(StopReport),
    Cancelled(Vec<OrphanRecoveryNotice>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OrphanRecoveryNotice {
    session_id: String,
    kind: &'static str,
    forgotten_ambiguities: Vec<&'static str>,
    apps: Vec<OrphanRecoveryApp>,
    pub(super) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OrphanRecoveryApp {
    name: String,
    hostname: Option<String>,
    target_host: String,
    target_port: Option<u16>,
    pid: Option<u32>,
    spawn_state: &'static str,
}

impl OrphanRecoveryApp {
    fn from_app(app: &DevSessionApp) -> Self {
        let spawn_state = match app.spawn_evidence() {
            DevSessionAppSpawnEvidence::Untracked => "untracked",
            DevSessionAppSpawnEvidence::NotStarted => "not-started",
            DevSessionAppSpawnEvidence::Pending => "pending",
            DevSessionAppSpawnEvidence::Registered(_) => "registered",
        };
        Self {
            name: app.name.clone(),
            hostname: app.hostname.clone(),
            target_host: app.target_host.clone(),
            target_port: app.target_port,
            pid: app.process.as_ref().map(|process| process.pid),
            spawn_state,
        }
    }

    fn diagnostic(&self) -> String {
        let target = self.target_port.map_or_else(
            || format!("{}:<unknown-port>", self.target_host),
            |port| format!("{}:{port}", self.target_host),
        );
        let pid = self
            .pid
            .map_or_else(|| "unknown PID".to_owned(), |pid| format!("last PID {pid}"));
        format!(
            "{} (target {target}, {pid}, spawn {})",
            self.name, self.spawn_state
        )
    }
}

impl OrphanRecoveryNotice {
    fn from_session(
        session: &DevSessionRecord,
        forgotten_ambiguities: &[ForgottenCleanupAmbiguity],
    ) -> Self {
        let apps = session
            .apps
            .iter()
            .map(OrphanRecoveryApp::from_app)
            .collect::<Vec<_>>();
        let app_diagnostics = apps
            .iter()
            .map(OrphanRecoveryApp::diagnostic)
            .collect::<Vec<_>>()
            .join(", ");
        let diagnostic_suffix = if app_diagnostics.is_empty() {
            String::new()
        } else {
            format!("; retired app diagnostics: {app_diagnostics}")
        };
        let forgotten_diagnostic = forgotten_ambiguities
            .iter()
            .map(|ambiguity| ambiguity.diagnostic())
            .collect::<Vec<_>>()
            .join(" and ");
        let (kind, message) = if !forgotten_ambiguities.is_empty() {
            (
                "ambiguous-orphan-forgotten",
                format!(
                    "session '{}': explicitly forgot a dead-supervisor orphan with {forgotten_diagnostic} and retired its exact-owned stale routes without signaling persisted PIDs; an unrecorded process may still be running{diagnostic_suffix}",
                    session.session_id
                ),
            )
        } else {
            (
                "dead-orphan-retired",
                format!(
                    "session '{}': retired a dead orphan and its exact-owned stale routes without signaling persisted PIDs; unrecorded descendants could not be ruled out{diagnostic_suffix}",
                    session.session_id
                ),
            )
        };
        Self {
            session_id: session.session_id.clone(),
            kind,
            forgotten_ambiguities: forgotten_ambiguities
                .iter()
                .map(|ambiguity| ambiguity.label())
                .collect(),
            apps,
            message,
        }
    }
}

impl StopReport {
    fn into_json(self, repo: &CanonicalRepo, state_dir: &Path) -> Value {
        json!({
            "ok": self.ok,
            "command": "dev stop",
            "repo_name": repo.name,
            "repo_root": repo.root_display,
            "state_dir": state_dir,
            "matched_sessions": self.matched_sessions,
            "stopped_sessions": self.stopped_sessions,
            "stopped_apps": self.stopped_apps,
            "sessions": self.sessions,
            "recoveries": self.recoveries,
            "warnings": self.warnings,
        })
    }
}

fn stop_session_ids(
    store: &StateStore,
    repo: &CanonicalRepo,
    target_ids: &BTreeSet<String>,
    policy: AmbiguousOrphanPolicy,
) -> Result<StopReport> {
    match stop_session_ids_interruptible_with_policy(store, repo, target_ids, policy, &|| false)? {
        StopSessionOutcome::Complete(report) => Ok(report),
        StopSessionOutcome::Cancelled(_) => bail!("uncancelled Jig dev stop was cancelled"),
    }
}

pub(super) fn stop_session_ids_interruptible(
    store: &StateStore,
    repo: &CanonicalRepo,
    target_ids: &BTreeSet<String>,
    cancelled: &impl Fn() -> bool,
) -> Result<StopSessionOutcome> {
    stop_session_ids_interruptible_with_policy(
        store,
        repo,
        target_ids,
        AmbiguousOrphanPolicy::Retain,
        cancelled,
    )
}

fn stop_session_ids_interruptible_with_policy(
    store: &StateStore,
    repo: &CanonicalRepo,
    target_ids: &BTreeSet<String>,
    policy: AmbiguousOrphanPolicy,
    cancelled: &impl Fn() -> bool,
) -> Result<StopSessionOutcome> {
    let mut recoveries = Vec::new();
    match stop_session_ids_interruptible_inner(
        store,
        repo,
        target_ids,
        policy,
        cancelled,
        &mut recoveries,
    ) {
        Ok(outcome) => Ok(outcome),
        Err(error) => Err(crate::dev_outcome::with_recovery_notices(error, recoveries)),
    }
}

fn stop_session_ids_interruptible_inner(
    store: &StateStore,
    repo: &CanonicalRepo,
    target_ids: &BTreeSet<String>,
    policy: AmbiguousOrphanPolicy,
    cancelled: &impl Fn() -> bool,
    recoveries: &mut Vec<OrphanRecoveryNotice>,
) -> Result<StopSessionOutcome> {
    if target_ids.is_empty() {
        return Ok(StopSessionOutcome::Complete(StopReport {
            ok: true,
            ..StopReport::default()
        }));
    }
    let targets = match store.mutate_dev_sessions_interruptible(cancelled, |sessions, _| {
        let mut targets = Vec::new();
        for session in sessions.iter_mut().filter(|session| {
            session.repo_root_identity == repo.root_identity
                && target_ids.contains(&session.session_id)
        }) {
            session.phase = DevSessionPhase::Stopping;
            session.updated_at_ms = next_timestamp(session.updated_at_ms);
            targets.push(session.clone());
        }
        Ok(targets)
    })? {
        LockOutcome::Acquired(targets) => targets,
        LockOutcome::Cancelled => {
            return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
        }
    };
    let matched_sessions = targets.len();
    let initially_maybe_live_apps = targets
        .iter()
        .map(|session| {
            (
                session.session_id.clone(),
                session
                    .apps
                    .iter()
                    .filter_map(|app| app.process.as_ref())
                    .filter(|identity| process_identity_may_be_alive(identity))
                    .count(),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut pending = BTreeSet::new();
    let mut control_warnings = Vec::new();
    for session in &targets {
        if cancelled() {
            return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
        }
        match request_stop(
            session.control.port,
            &session.session_id,
            &session.control.token,
        ) {
            Ok(()) => {
                pending.insert(session.session_id.clone());
            }
            Err(error) => {
                if error.delivery_uncertain() {
                    pending.insert(session.session_id.clone());
                }
                control_warnings.push((
                    session.session_id.clone(),
                    format!(
                        "session '{}': authenticated supervisor stop was unavailable ({error})",
                        session.session_id
                    ),
                ));
            }
        }
    }
    if !wait_for_session_removal(
        store,
        &mut pending,
        control_retire_timeout(&targets),
        cancelled,
    )? {
        return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
    }

    let mut lifecycle_warnings = Vec::new();
    let remaining = match current_target_sessions(store, repo, target_ids, cancelled)? {
        LockOutcome::Acquired(remaining) => remaining,
        LockOutcome::Cancelled => {
            return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
        }
    };
    for session in remaining {
        if cancelled() {
            return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
        }
        match (
            session.cleanup_required,
            orphan_recovery_assessment(&session, policy),
        ) {
            (true, OrphanRecoveryAssessment::Retirable) => {
                match retire_orphan(store, &session, policy, cancelled)? {
                    LockOutcome::Cancelled => {
                        return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
                    }
                    LockOutcome::Acquired(RetireDeadOrphanOutcome::Retired(recovery)) => {
                        recoveries.push(recovery);
                    }
                    LockOutcome::Acquired(RetireDeadOrphanOutcome::AlreadyAbsent) => {}
                    LockOutcome::Acquired(RetireDeadOrphanOutcome::Retained(reason)) => {
                        lifecycle_warnings.push(retention_warning(&session, &reason));
                        if !mark_orphaned(store, &session, cancelled)? {
                            return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
                        }
                    }
                }
            }
            (_, OrphanRecoveryAssessment::Retain(reason)) => {
                lifecycle_warnings.push(retention_warning(&session, &reason));
                if !mark_orphaned(store, &session, cancelled)? {
                    return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
                }
            }
            (false, OrphanRecoveryAssessment::Retirable) => {
                if !remove_exact_session(store, &session, cancelled)? {
                    return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
                }
            }
        }
    }
    let snapshot = match store.snapshot_dev_state_interruptible(cancelled)? {
        LockOutcome::Acquired(snapshot) => snapshot,
        LockOutcome::Cancelled => {
            return Ok(StopSessionOutcome::Cancelled(std::mem::take(recoveries)));
        }
    };
    let sessions = snapshot
        .sessions
        .iter()
        .filter(|session| {
            session.repo_root_identity == repo.root_identity
                && target_ids.contains(&session.session_id)
        })
        .map(|session| session_status(session, &snapshot.routes))
        .collect::<Vec<_>>();
    let remaining_ids = sessions
        .iter()
        .filter_map(|session| session["session_id"].as_str())
        .collect::<HashSet<_>>();
    let blocking_warnings = lifecycle_warnings
        .into_iter()
        .chain(control_warnings)
        .filter(|(session_id, _)| remaining_ids.contains(session_id.as_str()))
        .map(|(_, warning)| warning)
        .collect::<Vec<_>>();
    let ok = blocking_warnings.is_empty() && sessions.is_empty();
    let warnings = blocking_warnings;
    let stopped_sessions = matched_sessions.saturating_sub(sessions.len());
    let stopped_apps = count_stopped_apps(initially_maybe_live_apps, &remaining_ids);
    Ok(StopSessionOutcome::Complete(StopReport {
        ok,
        matched_sessions,
        stopped_sessions,
        stopped_apps,
        sessions,
        recoveries: std::mem::take(recoveries),
        warnings,
    }))
}

fn count_stopped_apps(
    initially_maybe_live_apps: HashMap<String, usize>,
    remaining_ids: &HashSet<&str>,
) -> usize {
    initially_maybe_live_apps
        .into_iter()
        .filter(|(session_id, _)| !remaining_ids.contains(session_id.as_str()))
        .map(|(_, app_count)| app_count)
        .sum()
}

fn retire_orphan(
    store: &StateStore,
    session: &DevSessionRecord,
    policy: AmbiguousOrphanPolicy,
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<RetireDeadOrphanOutcome>> {
    store.mutate_dev_state_interruptible(cancelled, |sessions, routes| {
        let Some(index) = sessions.iter().position(|current| {
            current.session_id == session.session_id
                && current.repo_root_identity == session.repo_root_identity
                && current.supervisor == session.supervisor
        }) else {
            return Ok(RetireDeadOrphanOutcome::AlreadyAbsent);
        };
        let current = &sessions[index];
        let forgotten_ambiguities = forgotten_cleanup_ambiguities(current, policy);
        if let OrphanRecoveryAssessment::Retain(reason) =
            orphan_recovery_assessment(current, policy)
        {
            return Ok(RetireDeadOrphanOutcome::Retained(reason));
        }

        let recovery = OrphanRecoveryNotice::from_session(current, &forgotten_ambiguities);
        routes.retain(|route| !session_owns_route(current, route));
        sessions.remove(index);
        Ok(RetireDeadOrphanOutcome::Retired(recovery))
    })
}

fn orphan_recovery_assessment(
    session: &DevSessionRecord,
    policy: AmbiguousOrphanPolicy,
) -> OrphanRecoveryAssessment {
    orphan_recovery_assessment_with_observations(
        session,
        policy,
        observe_process_identity(&session.supervisor),
        |_, app| app.process.as_ref().map(observe_process_identity),
    )
}

fn orphan_recovery_assessment_with_observations(
    session: &DevSessionRecord,
    policy: AmbiguousOrphanPolicy,
    supervisor_observation: ProcessIdentityObservation,
    mut app_observation: impl FnMut(usize, &DevSessionApp) -> Option<ProcessIdentityObservation>,
) -> OrphanRecoveryAssessment {
    match supervisor_observation {
        ProcessIdentityObservation::Alive => {
            return OrphanRecoveryAssessment::Retain(OrphanRetentionReason::SupervisorAlive);
        }
        ProcessIdentityObservation::Uncertain => {
            return OrphanRecoveryAssessment::Retain(OrphanRetentionReason::SupervisorUncertain);
        }
        ProcessIdentityObservation::Absent => {}
    }
    if !session.cleanup_required {
        return OrphanRecoveryAssessment::Retirable;
    }
    if session.preflight_cleanup_pending && policy == AmbiguousOrphanPolicy::Retain {
        return OrphanRecoveryAssessment::Retain(OrphanRetentionReason::PreflightCleanupPending);
    }
    for (index, app) in session.apps.iter().enumerate() {
        match app.spawn_evidence() {
            DevSessionAppSpawnEvidence::Untracked => {
                if policy == AmbiguousOrphanPolicy::Retain {
                    return OrphanRecoveryAssessment::Retain(
                        OrphanRetentionReason::AppSpawnUntracked(app.name.clone()),
                    );
                }
                continue;
            }
            DevSessionAppSpawnEvidence::Pending => {
                if policy == AmbiguousOrphanPolicy::Retain {
                    return OrphanRecoveryAssessment::Retain(
                        OrphanRetentionReason::AppSpawnPending(app.name.clone()),
                    );
                }
                continue;
            }
            DevSessionAppSpawnEvidence::NotStarted => continue,
            DevSessionAppSpawnEvidence::Registered(_) => {}
        }
        match app_observation(index, app)
            .expect("registered development app must have a process observation")
        {
            ProcessIdentityObservation::Alive => {
                return OrphanRecoveryAssessment::Retain(OrphanRetentionReason::AppAlive(
                    app.name.clone(),
                ));
            }
            ProcessIdentityObservation::Uncertain => {
                return OrphanRecoveryAssessment::Retain(OrphanRetentionReason::AppUncertain(
                    app.name.clone(),
                ));
            }
            ProcessIdentityObservation::Absent => {}
        }
    }
    OrphanRecoveryAssessment::Retirable
}

fn retention_warning(
    session: &DevSessionRecord,
    reason: &OrphanRetentionReason,
) -> (String, String) {
    let detail = match reason {
        OrphanRetentionReason::SupervisorAlive => format!(
            "supervisor PID {} remained live after the authenticated stop request",
            session.supervisor.pid
        ),
        OrphanRetentionReason::SupervisorUncertain => format!(
            "supervisor PID {} could not be classified safely",
            session.supervisor.pid
        ),
        OrphanRetentionReason::PreflightCleanupPending => {
            "development preflight cleanup was not confirmed".to_owned()
        }
        OrphanRetentionReason::AppAlive(app) => {
            format!("registered app '{app}' is still live")
        }
        OrphanRetentionReason::AppUncertain(app) => {
            format!("registered app '{app}' could not be classified safely")
        }
        OrphanRetentionReason::AppSpawnPending(app) => {
            format!("app '{app}' may have spawned before its process identity was durably recorded")
        }
        OrphanRetentionReason::AppSpawnUntracked(app) => format!(
            "legacy app '{app}' has no process identity and predates durable spawn-state tracking"
        ),
    };
    let repair = matches!(
        reason,
        OrphanRetentionReason::PreflightCleanupPending
            | OrphanRetentionReason::AppSpawnPending(_)
            | OrphanRetentionReason::AppSpawnUntracked(_)
    )
    .then_some(
        "; after independently confirming that no unrecorded process remains, retry with `jig dev stop --forget-ambiguous-orphans`",
    )
    .unwrap_or_default();
    (
        session.session_id.clone(),
        format!(
            "session '{}': {detail}; the registry entry was retained without signaling numeric PIDs{repair}",
            session.session_id
        ),
    )
}

fn forgotten_cleanup_ambiguities(
    session: &DevSessionRecord,
    policy: AmbiguousOrphanPolicy,
) -> Vec<ForgottenCleanupAmbiguity> {
    if policy == AmbiguousOrphanPolicy::Retain || !session.cleanup_required {
        return Vec::new();
    }
    let mut ambiguities = Vec::new();
    if session.preflight_cleanup_pending {
        ambiguities.push(ForgottenCleanupAmbiguity::PreflightCleanup);
    }
    if session.apps.iter().any(|app| {
        matches!(
            app.spawn_evidence(),
            DevSessionAppSpawnEvidence::Pending | DevSessionAppSpawnEvidence::Untracked
        )
    }) {
        ambiguities.push(ForgottenCleanupAmbiguity::SpawnHistory);
    }
    ambiguities
}

fn control_retire_timeout(targets: &[DevSessionRecord]) -> Duration {
    let max_apps = targets
        .iter()
        .map(|session| session.apps.len())
        .max()
        .unwrap_or(0);
    let app_count = u32::try_from(max_apps).unwrap_or(u32::MAX);
    CONTROL_RETIRE_BASE_TIMEOUT
        .saturating_add(CONTROL_RETIRE_PER_APP_TIMEOUT.saturating_mul(app_count))
}

fn current_target_sessions(
    store: &StateStore,
    repo: &CanonicalRepo,
    target_ids: &BTreeSet<String>,
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<Vec<DevSessionRecord>>> {
    match store.snapshot_dev_state_interruptible(cancelled)? {
        LockOutcome::Acquired(snapshot) => Ok(LockOutcome::Acquired(
            snapshot
                .sessions
                .into_iter()
                .filter(|session| {
                    session.repo_root_identity == repo.root_identity
                        && target_ids.contains(&session.session_id)
                })
                .collect(),
        )),
        LockOutcome::Cancelled => Ok(LockOutcome::Cancelled),
    }
}

fn wait_for_session_removal(
    store: &StateStore,
    pending: &mut BTreeSet<String>,
    timeout: Duration,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    while !pending.is_empty() && Instant::now() < deadline {
        let live = match store.snapshot_dev_state_interruptible(cancelled)? {
            LockOutcome::Acquired(snapshot) => snapshot
                .sessions
                .into_iter()
                .map(|session| session.session_id)
                .collect::<HashSet<_>>(),
            LockOutcome::Cancelled => return Ok(false),
        };
        pending.retain(|session_id| live.contains(session_id));
        if !pending.is_empty() {
            thread::sleep(SESSION_POLL_INTERVAL);
        }
    }
    Ok(!cancelled())
}

fn mark_orphaned(
    store: &StateStore,
    session: &DevSessionRecord,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    match store.mutate_dev_sessions_interruptible(cancelled, |sessions, _| {
        if let Some(current) = sessions
            .iter_mut()
            .find(|current| current.session_id == session.session_id)
        {
            current.phase = DevSessionPhase::Orphaned;
            current.updated_at_ms = next_timestamp(current.updated_at_ms);
        }
        Ok(())
    })? {
        LockOutcome::Acquired(()) => Ok(true),
        LockOutcome::Cancelled => Ok(false),
    }
}

fn remove_exact_session(
    store: &StateStore,
    session: &DevSessionRecord,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    match store.mutate_dev_sessions_interruptible(cancelled, |sessions, _| {
        sessions.retain(|current| {
            current.session_id != session.session_id
                || current.repo_root_identity != session.repo_root_identity
                || current.supervisor != session.supervisor
        });
        Ok(())
    })? {
        LockOutcome::Acquired(()) => Ok(true),
        LockOutcome::Cancelled => Ok(false),
    }
}

fn session_status(session: &DevSessionRecord, routes: &[Route]) -> Value {
    let control_alive = ping(
        session.control.port,
        &session.session_id,
        &session.control.token,
    )
    .unwrap_or(false);
    let supervisor_observation = observe_process_identity(&session.supervisor);
    let app_observations = session
        .apps
        .iter()
        .map(|app| app.process.as_ref().map(observe_process_identity))
        .collect::<Vec<_>>();
    session_status_from_observations(
        session,
        routes,
        control_alive,
        supervisor_observation,
        &app_observations,
    )
}

fn session_status_from_observations(
    session: &DevSessionRecord,
    routes: &[Route],
    control_alive: bool,
    supervisor_observation: ProcessIdentityObservation,
    app_observations: &[Option<ProcessIdentityObservation>],
) -> Value {
    assert_eq!(
        session.apps.len(),
        app_observations.len(),
        "every development app must have one status observation"
    );
    let supervisor_verified = supervisor_observation.is_verified_alive();
    let supervisor_alive = supervisor_observation.may_be_alive();
    let apps = session
        .apps
        .iter()
        .zip(app_observations.iter().copied())
        .map(|(app, process_observation)| {
            let process_alive =
                process_observation.is_some_and(|observation| observation.may_be_alive());
            let process_identity_verified =
                process_observation.is_some_and(|observation| observation.is_verified_alive());
            let route_present = app.hostname.as_deref().is_some_and(|hostname| {
                routes.iter().any(|route| {
                    route.hostname.as_str() == hostname && session_owns_route(session, route)
                })
            });
            json!({
                "name": app.name,
                "hostname": app.hostname,
                "target_host": app.target_host,
                "target_port": app.target_port,
                "spawn_state_tracked": app.spawn_state_tracked,
                "spawn_pending": app.spawn_pending,
                "pid": app.process.as_ref().map(|process| process.pid),
                "alive": process_alive,
                "identity_verified": process_identity_verified,
                "identity_observation": process_observation.map(ProcessIdentityObservation::label),
                "route_present": route_present,
            })
        })
        .collect::<Vec<_>>();
    let recovery_assessment = orphan_recovery_assessment_with_observations(
        session,
        AmbiguousOrphanPolicy::Retain,
        supervisor_observation,
        |index, _| app_observations[index],
    );
    let supervisor_active = control_alive || supervisor_observation.may_be_alive();
    let recoverable = !supervisor_active
        && session.cleanup_required
        && recovery_assessment == OrphanRecoveryAssessment::Retirable;
    let status = if session.phase != DevSessionPhase::Orphaned && supervisor_active {
        match session.phase {
            DevSessionPhase::Starting => "starting",
            DevSessionPhase::Stopping => "stopping",
            DevSessionPhase::Running => "running",
            DevSessionPhase::Orphaned => unreachable!("orphaned phase handled above"),
        }
    } else if recoverable {
        "recoverable"
    } else if matches!(recovery_assessment, OrphanRecoveryAssessment::Retain(_)) {
        "orphaned"
    } else {
        "stale"
    };
    json!({
        "session_id": session.session_id,
        "status": status,
        "phase": session.phase,
        "started_at_ms": session.started_at_ms,
        "updated_at_ms": session.updated_at_ms,
        "cleanup_required": session.cleanup_required,
        "preflight_cleanup_pending": session.preflight_cleanup_pending,
        "recoverable": recoverable,
        "supervisor_pid": session.supervisor.pid,
        "supervisor_alive": supervisor_alive,
        "supervisor_identity_verified": supervisor_verified,
        "supervisor_observation": supervisor_observation.label(),
        "control_alive": control_alive,
        "apps": apps,
    })
}

fn empty_status(repo: &CanonicalRepo, state_dir: PathBuf) -> Value {
    json!({
        "ok": true,
        "command": "dev status",
        "repo_name": repo.name,
        "repo_root": repo.root_display,
        "state_dir": state_dir,
        "running": false,
        "sessions": [],
    })
}

fn empty_stop(repo: &CanonicalRepo, state_dir: PathBuf) -> Value {
    json!({
        "ok": true,
        "command": "dev stop",
        "repo_name": repo.name,
        "repo_root": repo.root_display,
        "state_dir": state_dir,
        "matched_sessions": 0,
        "stopped_sessions": 0,
        "stopped_apps": 0,
        "sessions": [],
        "recoveries": [],
        "warnings": [],
    })
}

fn configured_state_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Ok(path) = std::env::var("JIG_PROXY_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }
    Ok(dirs::home_dir()
        .context("Could not resolve home directory for Jig proxy state")?
        .join(".jig/proxy"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{DevProcessIdentity, DevSessionControl};

    fn cleanup_required_session() -> DevSessionRecord {
        DevSessionRecord {
            session_id: "dev_example".into(),
            repo_name: "ExampleProject".into(),
            repo_root_display: "/tmp/example-project".into(),
            repo_root_identity: "/tmp/example-project".into(),
            phase: DevSessionPhase::Running,
            started_at_ms: 1,
            updated_at_ms: 1,
            cleanup_required: true,
            preflight_cleanup_pending: false,
            supervisor: DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("example-supervisor".into()),
            },
            control: DevSessionControl {
                port: 1,
                token: "example-control-token".into(),
            },
            apps: Vec::new(),
        }
    }

    #[test]
    fn active_status_evidence_cannot_also_be_recoverable() {
        let session = cleanup_required_session();

        let control_active = session_status_from_observations(
            &session,
            &[],
            true,
            ProcessIdentityObservation::Absent,
            &[],
        );
        assert_eq!(control_active["status"], "running");
        assert_eq!(control_active["recoverable"], false);

        let supervisor_active = session_status_from_observations(
            &session,
            &[],
            false,
            ProcessIdentityObservation::Alive,
            &[],
        );
        assert_eq!(supervisor_active["status"], "running");
        assert_eq!(supervisor_active["recoverable"], false);
    }

    #[test]
    fn inactive_recovery_snapshot_is_reported_consistently() {
        let session = cleanup_required_session();

        let status = session_status_from_observations(
            &session,
            &[],
            false,
            ProcessIdentityObservation::Absent,
            &[],
        );

        assert_eq!(status["status"], "recoverable");
        assert_eq!(status["recoverable"], true);
        assert_eq!(status["supervisor_alive"], false);
        assert_eq!(status["control_alive"], false);
    }

    #[test]
    fn live_app_observation_cannot_also_be_recoverable() {
        let mut session = cleanup_required_session();
        session.apps.push(DevSessionApp {
            name: "web".into(),
            hostname: None,
            target_host: "127.0.0.1".into(),
            target_port: Some(4000),
            spawn_state_tracked: true,
            spawn_pending: false,
            process: Some(DevProcessIdentity {
                pid: u32::MAX - 1,
                start_token: Some("example-app".into()),
            }),
        });

        let status = session_status_from_observations(
            &session,
            &[],
            false,
            ProcessIdentityObservation::Absent,
            &[Some(ProcessIdentityObservation::Alive)],
        );

        assert_eq!(status["status"], "orphaned");
        assert_eq!(status["recoverable"], false);
        assert_eq!(status["apps"][0]["alive"], true);
        assert_eq!(status["apps"][0]["identity_observation"], "alive");
    }

    #[test]
    fn stopped_app_count_includes_removed_targets_and_excludes_remaining_targets() {
        let initially_maybe_live_apps =
            HashMap::from([("stopped".to_owned(), 2), ("remaining".to_owned(), 3)]);
        let remaining_ids = HashSet::from(["remaining"]);

        assert_eq!(
            count_stopped_apps(initially_maybe_live_apps, &remaining_ids),
            2
        );
    }
}
