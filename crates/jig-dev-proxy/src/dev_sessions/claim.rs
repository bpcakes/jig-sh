use std::collections::{BTreeSet, HashSet};

use super::assessment::{AmbiguousOrphanPolicy, OrphanRecoveryAssessment, assess_session};
use super::process_identity::process_identity_may_be_alive;
use super::*;
use crate::session_control::ping;
use crate::state::{observe_pid, process_start_token};

#[derive(Default)]
pub(super) struct ClaimConflicts {
    same_repo: Vec<DevSessionRecord>,
    other_repos: Vec<(String, DevSessionRecord)>,
    unmanaged_routes: Vec<Route>,
}

impl ClaimConflicts {
    fn is_empty(&self) -> bool {
        self.same_repo.is_empty() && self.other_repos.is_empty() && self.unmanaged_routes.is_empty()
    }

    pub(super) fn has_unsafe_replacement(&self) -> bool {
        !self.other_repos.is_empty() || !self.unmanaged_routes.is_empty()
    }

    pub(super) fn same_repo_session_ids(&self) -> BTreeSet<String> {
        self.same_repo
            .iter()
            .map(|session| session.session_id.clone())
            .collect()
    }

    pub(super) fn launch_error(&self, replacing: bool, state_dir: &Path) -> anyhow::Error {
        if let Some((hostname, session)) = self.other_repos.first() {
            let activity = claim_activity(session);
            return anyhow!(
                "Development hostname '{hostname}' is claimed by Jig dev session '{}' from repository {} ({activity}). Cross-repository ownership remains reserved until that exact session is explicitly cleaned up; `jig dev --replace` will not take it over. Inspect the session from its repository with `jig dev status --state-dir PATH`, using state directory {}, or change the duplicate hostname.",
                session.session_id,
                session.repo_root_display,
                state_dir.display(),
            );
        }
        if let Some(route) = self.unmanaged_routes.first() {
            let owner = route
                .owner_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "<unknown>".into());
            return anyhow!(
                "Proxy route '{}' would replace a live process route owned by PID {} and targeting {}:{}, but that route is not attributable to a registered Jig dev session. `jig dev --replace` will not terminate an unregistered or ad-hoc process. Stop that process, run `jig proxy prune --state-dir PATH` using state directory {}, or change the duplicate hostname.",
                route.hostname,
                owner,
                route.target_host,
                route.target_port,
                state_dir.display()
            );
        }
        let hosts = conflict_hostnames(&self.same_repo);
        let claim_details = self
            .same_repo
            .iter()
            .take(8)
            .map(|session| format!("'{}' ({})", session.session_id, claim_activity(session)))
            .collect::<Vec<_>>()
            .join(", ");
        let omitted = self.same_repo.len().saturating_sub(8);
        let more = if omitted == 0 {
            String::new()
        } else {
            format!("; {omitted} more claim(s) remain")
        };
        if replacing {
            anyhow!(
                "The registered Jig dev session for {} could not be replaced safely. Blocking claim(s): {claim_details}{more}. Inspect with `jig dev status --state-dir PATH` using state directory {}.",
                hosts.join(", "),
                state_dir.display(),
            )
        } else {
            anyhow!(
                "A registered Jig dev session from this repository already claims {}. Blocking claim(s): {claim_details}{more}. Inspect with `jig dev status --state-dir PATH` or explicitly clean up with `jig dev stop --state-dir PATH`, using state directory {}; otherwise retry with `jig dev --replace`.",
                hosts.join(", "),
                state_dir.display(),
            )
        }
    }

    pub(super) fn concurrent_launch_error(&self, state_dir: &Path) -> anyhow::Error {
        anyhow!(
            "A concurrent Jig dev launch claimed the requested app or hostname while replacement was completing. No newly observed session was stopped. {}",
            self.launch_error(false, state_dir)
        )
    }
}

pub(super) enum ClaimOutcome {
    Claimed,
    Conflicted(ClaimConflicts),
}

pub(super) fn claim_session_interruptible(
    store: &StateStore,
    proposed: &DevSessionRecord,
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<ClaimOutcome>> {
    store.mutate_dev_sessions_for_claim_interruptible(
        cancelled,
        COMPLETE_EVIDENCE_CUTOVER_ENABLED,
        |sessions, routes| {
            sessions.retain(|session| session.cleanup_required || session_observed_alive(session));
            let mut conflicts = ClaimConflicts::default();
            let mut seen_session_ids = HashSet::new();

            for session in sessions.iter() {
                let same_repo = session.repo_root_identity == proposed.repo_root_identity;
                let overlap = if same_repo {
                    sessions_overlap(session, proposed)
                } else {
                    overlapping_hostname(session, proposed).is_some()
                };
                if !overlap {
                    continue;
                }
                seen_session_ids.insert(session.session_id.clone());
                if same_repo {
                    conflicts.same_repo.push(session.clone());
                } else {
                    let hostname = overlapping_hostname(session, proposed)
                        .expect("cross-repository overlap is hostname-based");
                    conflicts.other_repos.push((hostname, session.clone()));
                }
            }

            let proposed_hostnames = proposed
                .apps
                .iter()
                .filter_map(|app| app.hostname.as_deref())
                .collect::<HashSet<_>>();
            for route in routes.iter().filter(|route| {
                route.mode == RouteMode::Process
                    && proposed_hostnames.contains(route.hostname.as_str())
                    && route_is_live(route)
            }) {
                let attributed = sessions
                    .iter()
                    .find(|session| session_owns_route(session, route));
                match attributed {
                    Some(session) if seen_session_ids.contains(&session.session_id) => {}
                    Some(session) if session.repo_root_identity == proposed.repo_root_identity => {
                        seen_session_ids.insert(session.session_id.clone());
                        conflicts.same_repo.push(session.clone());
                    }
                    Some(session) => conflicts
                        .other_repos
                        .push((route.hostname.to_string(), session.clone())),
                    None => conflicts.unmanaged_routes.push(route.clone()),
                }
            }

            deduplicate_conflicts(&mut conflicts);
            if conflicts.is_empty() {
                sessions.push(proposed.clone());
                Ok(ClaimOutcome::Claimed)
            } else {
                Ok(ClaimOutcome::Conflicted(conflicts))
            }
        },
    )
}

fn sessions_overlap(left: &DevSessionRecord, right: &DevSessionRecord) -> bool {
    left.apps.iter().any(|left_app| {
        right.apps.iter().any(|right_app| {
            left_app.name == right_app.name
                || left_app
                    .hostname
                    .as_ref()
                    .zip(right_app.hostname.as_ref())
                    .is_some_and(|(left, right)| left == right)
        })
    })
}

fn overlapping_hostname(left: &DevSessionRecord, right: &DevSessionRecord) -> Option<String> {
    left.apps.iter().find_map(|left_app| {
        right.apps.iter().find_map(|right_app| {
            left_app
                .hostname
                .as_ref()
                .zip(right_app.hostname.as_ref())
                .filter(|(left, right)| left == right)
                .map(|(hostname, _)| hostname.clone())
        })
    })
}

fn session_observed_alive(session: &DevSessionRecord) -> bool {
    process_identity_may_be_alive(&session.supervisor)
        || session
            .apps
            .iter()
            .filter_map(|app| app.process.as_ref())
            .any(process_identity_may_be_alive)
}

fn route_is_live(route: &Route) -> bool {
    match route.mode {
        RouteMode::Alias => true,
        RouteMode::Process => route
            .owner_pid
            .zip(route.owner_start_token.as_deref())
            .is_some_and(|(pid, token)| {
                observe_pid(pid).may_be_alive()
                    && process_start_token(pid)
                        .as_deref()
                        .is_none_or(|current| current == token)
            }),
    }
}

fn deduplicate_conflicts(conflicts: &mut ClaimConflicts) {
    let mut session_ids = HashSet::new();
    conflicts
        .same_repo
        .retain(|session| session_ids.insert(session.session_id.clone()));
    let mut other = HashSet::new();
    conflicts
        .other_repos
        .retain(|(hostname, session)| other.insert((hostname.clone(), session.session_id.clone())));
    let mut routes = HashSet::new();
    conflicts
        .unmanaged_routes
        .retain(|route| routes.insert(route.hostname.to_string()));
}

fn claim_activity(session: &DevSessionRecord) -> String {
    // Conflict formatting runs after the shared state lock is released.
    let control_alive = ping(
        session.control.port,
        &session.session_id,
        &session.control.token,
    )
    .unwrap_or(false);
    let assessment = assess_session(session, AmbiguousOrphanPolicy::Retain, control_alive);
    let reason = match assessment.recovery {
        OrphanRecoveryAssessment::Retain(reason) => match reason.app() {
            Some(app) => format!("{}: '{app}'", reason.code()),
            None => reason.code().to_owned(),
        },
        OrphanRecoveryAssessment::Retirable if session.cleanup_required => {
            "eligible for explicit metadata cleanup".to_owned()
        }
        OrphanRecoveryAssessment::Retirable => "no cleanup obligation observed".to_owned(),
    };
    format!(
        "activity {}; cleanup required {}; {reason}",
        assessment.activity.label(),
        session.cleanup_required
    )
}

fn conflict_hostnames(sessions: &[DevSessionRecord]) -> Vec<String> {
    let hosts = sessions
        .iter()
        .flat_map(|session| &session.apps)
        .filter_map(|app| app.hostname.clone().or_else(|| Some(app.name.clone())))
        .collect::<BTreeSet<_>>();
    hosts.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{DevSessionControl, DevSessionPhase};

    #[test]
    fn concurrent_claim_error_identifies_the_new_exact_owner() {
        let session = DevSessionRecord {
            session_id: "dev_example_winner".into(),
            repo_name: "ExampleProject".into(),
            repo_root_display: "/tmp/ExampleProject".into(),
            repo_root_identity: "/tmp/ExampleProject".into(),
            phase: DevSessionPhase::Orphaned,
            started_at_ms: 1,
            updated_at_ms: 1,
            cleanup_required: true,
            preflight_cleanup_pending: Some(false),
            supervisor: DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-supervisor".into()),
            },
            control: DevSessionControl {
                port: 1,
                token: "example-control-token".into(),
            },
            apps: vec![DevSessionApp {
                name: "web".into(),
                hostname: Some("web.example.localhost".into()),
                target_host: "127.0.0.1".into(),
                target_port: None,
                spawn_state_tracked: true,
                spawn_pending: false,
                process: None,
            }],
        };
        let conflicts = ClaimConflicts {
            same_repo: vec![session],
            ..ClaimConflicts::default()
        };
        let error = conflicts
            .concurrent_launch_error(Path::new("/tmp/ExampleProject-proxy-state"))
            .to_string();

        assert!(error.contains("No newly observed session was stopped"));
        assert!(error.contains("dev_example_winner"));
        assert!(error.contains("activity none"));
        assert!(error.contains("cleanup required true"));
        assert!(error.contains("/tmp/ExampleProject-proxy-state"));
        assert!(!error.contains("example-control-token"));
    }
}
