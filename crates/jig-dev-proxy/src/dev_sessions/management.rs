use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use super::process_identity::{
    ProcessIdentityObservation, observe_process_identity, process_identity_may_be_alive,
};
use super::{CanonicalRepo, next_timestamp, session_owns_route};
use crate::session_control::{ping, request_stop};
use crate::state::{DevSessionPhase, DevSessionRecord, LockOutcome, StateStore};
use crate::types::{DevStatusRequest, DevStopRequest, Route};

const CONTROL_RETIRE_BASE_TIMEOUT: Duration = Duration::from_secs(35);
const CONTROL_RETIRE_PER_APP_TIMEOUT: Duration = Duration::from_secs(15);
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Eq, PartialEq)]
enum OrphanRetentionReason {
    SupervisorAlive,
    SupervisorUncertain,
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
    Retired,
    AlreadyAbsent,
    Retained(OrphanRetentionReason),
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
    let report = stop_session_ids(&store, &repo, &ids)?;
    Ok(report.into_json(&repo, store.root()))
}

#[derive(Default)]
pub(super) struct StopReport {
    pub(super) ok: bool,
    matched_sessions: usize,
    stopped_sessions: usize,
    stopped_apps: usize,
    sessions: Vec<Value>,
    pub(super) warnings: Vec<String>,
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
            "warnings": self.warnings,
        })
    }
}

pub(super) fn stop_session_ids(
    store: &StateStore,
    repo: &CanonicalRepo,
    target_ids: &BTreeSet<String>,
) -> Result<StopReport> {
    match stop_session_ids_interruptible(store, repo, target_ids, &|| false)? {
        LockOutcome::Acquired(report) => Ok(report),
        LockOutcome::Cancelled => bail!("uncancelled Jig dev stop was cancelled"),
    }
}

pub(super) fn stop_session_ids_interruptible(
    store: &StateStore,
    repo: &CanonicalRepo,
    target_ids: &BTreeSet<String>,
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<StopReport>> {
    if target_ids.is_empty() {
        return Ok(LockOutcome::Acquired(StopReport {
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
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    let matched_sessions = targets.len();
    let initially_live_apps = targets
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
            return Ok(LockOutcome::Cancelled);
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
        return Ok(LockOutcome::Cancelled);
    }

    let mut lifecycle_warnings = Vec::new();
    let mut recovery_warnings = Vec::new();
    let remaining = match current_target_sessions(store, repo, target_ids, cancelled)? {
        LockOutcome::Acquired(remaining) => remaining,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    for session in remaining {
        if cancelled() {
            return Ok(LockOutcome::Cancelled);
        }
        match (
            session.cleanup_required,
            orphan_recovery_assessment(&session),
        ) {
            (true, OrphanRecoveryAssessment::Retirable) => {
                match retire_dead_orphan(store, &session, cancelled)? {
                    LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
                    LockOutcome::Acquired(RetireDeadOrphanOutcome::Retired) => {
                        recovery_warnings.push(format!(
                            "session '{}': retired a dead orphan and its exact-owned stale routes without signaling persisted PIDs; unrecorded descendants could not be ruled out",
                            session.session_id
                        ));
                    }
                    LockOutcome::Acquired(RetireDeadOrphanOutcome::AlreadyAbsent) => {}
                    LockOutcome::Acquired(RetireDeadOrphanOutcome::Retained(reason)) => {
                        lifecycle_warnings.push(retention_warning(&session, &reason));
                        if !mark_orphaned(store, &session, cancelled)? {
                            return Ok(LockOutcome::Cancelled);
                        }
                    }
                }
            }
            (_, OrphanRecoveryAssessment::Retain(reason)) => {
                lifecycle_warnings.push(retention_warning(&session, &reason));
                if !mark_orphaned(store, &session, cancelled)? {
                    return Ok(LockOutcome::Cancelled);
                }
            }
            (false, OrphanRecoveryAssessment::Retirable) => {
                if !remove_exact_session(store, &session, cancelled)? {
                    return Ok(LockOutcome::Cancelled);
                }
            }
        }
    }
    let snapshot = match store.snapshot_dev_state_interruptible(cancelled)? {
        LockOutcome::Acquired(snapshot) => snapshot,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
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
    let warnings = blocking_warnings
        .into_iter()
        .chain(recovery_warnings)
        .collect::<Vec<_>>();
    let stopped_sessions = matched_sessions.saturating_sub(sessions.len());
    let stopped_apps = initially_live_apps
        .into_iter()
        .filter(|(session_id, _)| !remaining_ids.contains(session_id.as_str()))
        .map(|(_, app_count)| app_count)
        .sum();
    Ok(LockOutcome::Acquired(StopReport {
        ok,
        matched_sessions,
        stopped_sessions,
        stopped_apps,
        sessions,
        warnings,
    }))
}

fn retire_dead_orphan(
    store: &StateStore,
    session: &DevSessionRecord,
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
        if let OrphanRecoveryAssessment::Retain(reason) = orphan_recovery_assessment(current) {
            return Ok(RetireDeadOrphanOutcome::Retained(reason));
        }

        routes.retain(|route| !session_owns_route(current, route));
        sessions.remove(index);
        Ok(RetireDeadOrphanOutcome::Retired)
    })
}

fn orphan_recovery_assessment(session: &DevSessionRecord) -> OrphanRecoveryAssessment {
    match observe_process_identity(&session.supervisor) {
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
    for app in &session.apps {
        if app.spawn_pending {
            return OrphanRecoveryAssessment::Retain(OrphanRetentionReason::AppSpawnPending(
                app.name.clone(),
            ));
        }
        if app.process.is_none() && !app.spawn_state_tracked {
            return OrphanRecoveryAssessment::Retain(OrphanRetentionReason::AppSpawnUntracked(
                app.name.clone(),
            ));
        }
        let Some(process) = app.process.as_ref() else {
            continue;
        };
        match observe_process_identity(process) {
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
    (
        session.session_id.clone(),
        format!(
            "session '{}': {detail}; the registry entry was retained without signaling numeric PIDs",
            session.session_id
        ),
    )
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
    let supervisor_verified = supervisor_observation == ProcessIdentityObservation::Alive;
    let supervisor_alive = supervisor_observation == ProcessIdentityObservation::Alive;
    let apps = session
        .apps
        .iter()
        .map(|app| {
            let process_observation = app.process.as_ref().map(observe_process_identity);
            let process_alive = process_observation == Some(ProcessIdentityObservation::Alive);
            let process_identity_verified =
                process_observation == Some(ProcessIdentityObservation::Alive);
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
                "identity_observation": process_observation.map(process_observation_label),
                "route_present": route_present,
            })
        })
        .collect::<Vec<_>>();
    let recovery_assessment = orphan_recovery_assessment(session);
    let recoverable =
        session.cleanup_required && recovery_assessment == OrphanRecoveryAssessment::Retirable;
    let supervisor_active =
        control_alive || supervisor_observation == ProcessIdentityObservation::Alive;
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
        "recoverable": recoverable,
        "supervisor_pid": session.supervisor.pid,
        "supervisor_alive": supervisor_alive,
        "supervisor_identity_verified": supervisor_verified,
        "supervisor_observation": process_observation_label(supervisor_observation),
        "control_alive": control_alive,
        "apps": apps,
    })
}

const fn process_observation_label(observation: ProcessIdentityObservation) -> &'static str {
    match observation {
        ProcessIdentityObservation::Alive => "alive",
        ProcessIdentityObservation::Absent => "absent",
        ProcessIdentityObservation::Uncertain => "uncertain",
    }
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
