use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::process_identity::{process_identity_matches, process_identity_observed_alive};
use super::{CanonicalRepo, next_timestamp};
use crate::session_control::{ping, request_stop};
use crate::state::{DevSessionPhase, DevSessionRecord, StateStore};
use crate::types::{DevStatusRequest, DevStopRequest, Route};

const CONTROL_RETIRE_BASE_TIMEOUT: Duration = Duration::from_secs(35);
const CONTROL_RETIRE_PER_APP_TIMEOUT: Duration = Duration::from_secs(15);
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(50);

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
    let running = sessions.iter().any(|session| session["status"] != "stale");
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
    if target_ids.is_empty() {
        return Ok(StopReport {
            ok: true,
            ..StopReport::default()
        });
    }
    let targets = store.mutate_dev_sessions(|sessions, _| {
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
    })?;
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
                    .filter(|identity| process_identity_observed_alive(identity))
                    .count(),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut pending = BTreeSet::new();
    let mut control_warnings = Vec::new();
    for session in &targets {
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
    wait_for_session_removal(store, &mut pending, control_retire_timeout(&targets))?;

    let mut lifecycle_warnings = Vec::new();
    let remaining = current_target_sessions(store, repo, target_ids)?;
    for session in remaining {
        if process_identity_observed_alive(&session.supervisor) {
            let detail = if process_identity_matches(&session.supervisor) {
                "remained live after the authenticated stop request"
            } else {
                "is live but its start identity cannot be verified safely"
            };
            lifecycle_warnings.push((
                session.session_id.clone(),
                format!(
                    "session '{}': supervisor PID {} {detail}",
                    session.session_id, session.supervisor.pid,
                ),
            ));
            mark_orphaned(store, &session)?;
            continue;
        }

        if session.cleanup_required {
            lifecycle_warnings.push((
                session.session_id.clone(),
                format!(
                    "session '{}': its supervisor is unavailable and prior process-tree or route cleanup was not confirmed; the registry entry was retained without signaling numeric PIDs",
                    session.session_id
                ),
            ));
            mark_orphaned(store, &session)?;
        } else {
            remove_exact_session(store, &session)?;
        }
    }
    let snapshot = store.snapshot_dev_state()?;
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
    let warnings = lifecycle_warnings
        .into_iter()
        .chain(control_warnings)
        .filter(|(session_id, _)| remaining_ids.contains(session_id.as_str()))
        .map(|(_, warning)| warning)
        .collect::<Vec<_>>();
    let stopped_sessions = matched_sessions.saturating_sub(sessions.len());
    let stopped_apps = initially_live_apps
        .into_iter()
        .filter(|(session_id, _)| !remaining_ids.contains(session_id.as_str()))
        .map(|(_, app_count)| app_count)
        .sum();
    Ok(StopReport {
        ok: warnings.is_empty() && sessions.is_empty(),
        matched_sessions,
        stopped_sessions,
        stopped_apps,
        sessions,
        warnings,
    })
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
) -> Result<Vec<DevSessionRecord>> {
    Ok(store
        .snapshot_dev_state()?
        .sessions
        .into_iter()
        .filter(|session| {
            session.repo_root_identity == repo.root_identity
                && target_ids.contains(&session.session_id)
        })
        .collect())
}

fn wait_for_session_removal(
    store: &StateStore,
    pending: &mut BTreeSet<String>,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while !pending.is_empty() && Instant::now() < deadline {
        let live = store
            .snapshot_dev_state()?
            .sessions
            .into_iter()
            .map(|session| session.session_id)
            .collect::<HashSet<_>>();
        pending.retain(|session_id| live.contains(session_id));
        if !pending.is_empty() {
            thread::sleep(SESSION_POLL_INTERVAL);
        }
    }
    Ok(())
}

fn mark_orphaned(store: &StateStore, session: &DevSessionRecord) -> Result<()> {
    store.mutate_dev_sessions(|sessions, _| {
        if let Some(current) = sessions
            .iter_mut()
            .find(|current| current.session_id == session.session_id)
        {
            current.phase = DevSessionPhase::Orphaned;
            current.updated_at_ms = next_timestamp(current.updated_at_ms);
        }
        Ok(())
    })
}

fn remove_exact_session(store: &StateStore, session: &DevSessionRecord) -> Result<()> {
    store.mutate_dev_sessions(|sessions, _| {
        sessions.retain(|current| {
            current.session_id != session.session_id
                || current.repo_root_identity != session.repo_root_identity
                || current.supervisor != session.supervisor
        });
        Ok(())
    })
}

fn session_status(session: &DevSessionRecord, routes: &[Route]) -> Value {
    let control_alive = ping(
        session.control.port,
        &session.session_id,
        &session.control.token,
    )
    .unwrap_or(false);
    let supervisor_verified = process_identity_matches(&session.supervisor);
    let supervisor_alive = process_identity_observed_alive(&session.supervisor);
    let apps = session
        .apps
        .iter()
        .map(|app| {
            let process_alive = app
                .process
                .as_ref()
                .is_some_and(process_identity_observed_alive);
            let process_identity_verified =
                app.process.as_ref().is_some_and(process_identity_matches);
            let route_present = app.hostname.as_deref().is_some_and(|hostname| {
                routes.iter().any(|route| {
                    route.hostname.as_str() == hostname
                        && app.process.as_ref().is_some_and(|identity| {
                            route.owner_pid == Some(identity.pid)
                                && route.owner_start_token == identity.start_token
                        })
                })
            });
            json!({
                "name": app.name,
                "hostname": app.hostname,
                "target_host": app.target_host,
                "target_port": app.target_port,
                "pid": app.process.as_ref().map(|process| process.pid),
                "alive": process_alive,
                "identity_verified": process_identity_verified,
                "route_present": route_present,
            })
        })
        .collect::<Vec<_>>();
    let any_app_alive = apps.iter().any(|app| app["alive"].as_bool() == Some(true));
    let supervisor_active = control_alive || supervisor_alive;
    let status = if session.phase == DevSessionPhase::Orphaned {
        "orphaned"
    } else if supervisor_active {
        match session.phase {
            DevSessionPhase::Starting => "starting",
            DevSessionPhase::Stopping => "stopping",
            DevSessionPhase::Running => "running",
            DevSessionPhase::Orphaned => unreachable!("orphaned phase handled above"),
        }
    } else if any_app_alive || session.cleanup_required {
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
        "supervisor_pid": session.supervisor.pid,
        "supervisor_alive": supervisor_alive,
        "supervisor_identity_verified": supervisor_verified,
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
