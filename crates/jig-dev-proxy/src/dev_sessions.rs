use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};

use self::management::stop_session_ids;
use self::process_identity::{capture_process_identity, process_identity_observed_alive};
use crate::session_control::SessionControlServer;
use crate::state::{
    DevProcessIdentity, DevSessionApp, DevSessionControl, DevSessionPhase, DevSessionRecord,
    StateStore, now_ms, pid_is_alive, process_start_token,
};
use crate::types::{AppRunSpec, Route, RouteMode};

mod management;
mod process_identity;

pub(crate) use management::{status, stop};

const SESSION_ID_RANDOM_BYTES: usize = 16;

pub(crate) struct DevSessionRuntime {
    store: StateStore,
    session_id: String,
    repo_root_identity: String,
    supervisor: DevProcessIdentity,
    control: SessionControlServer,
    pending_cleanup: Arc<AtomicUsize>,
}

pub(crate) struct DevCleanupLease {
    pending_cleanup: Arc<AtomicUsize>,
    confirmed: bool,
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
    pub(crate) fn start(
        store: StateStore,
        repo_name: &str,
        root: &Path,
        specs: &[AppRunSpec],
        replace: bool,
    ) -> Result<Self> {
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
                    process: None,
                })
                .collect(),
        };

        let first_claim = claim_session(&store, &record)?;
        match first_claim {
            ClaimOutcome::Claimed => {}
            ClaimOutcome::Conflicted(conflicts) if !replace => {
                return Err(conflicts.launch_error(false));
            }
            ClaimOutcome::Conflicted(conflicts) => {
                if conflicts.has_unsafe_replacement() {
                    return Err(conflicts.launch_error(true));
                }
                let target_ids = conflicts.same_repo_session_ids();
                let stop = stop_session_ids(&store, &repo, &target_ids)?;
                if !stop.ok {
                    bail!(
                        "Could not replace the existing Jig dev session safely: {}",
                        stop.warnings.join("; ")
                    );
                }
                match claim_session(&store, &record)? {
                    ClaimOutcome::Claimed => {}
                    ClaimOutcome::Conflicted(conflicts) => {
                        return Err(conflicts.concurrent_launch_error());
                    }
                }
            }
        }

        Ok(Self {
            store,
            session_id,
            repo_root_identity: repo.root_identity,
            supervisor,
            control,
            pending_cleanup: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub(crate) fn requested_stop(&self) -> bool {
        self.control.stop_requested()
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

    pub(crate) fn prepare_cleanup_scope(&self) -> Result<()> {
        self.store.mutate_dev_sessions(|sessions, _| {
            let session = exact_session_mut(
                sessions,
                &self.session_id,
                &self.repo_root_identity,
                &self.supervisor,
            )?;
            if !session.cleanup_required {
                session.cleanup_required = true;
                session.updated_at_ms = next_timestamp(session.updated_at_ms);
            }
            Ok(())
        })
    }

    pub(crate) fn record_app_process(
        &self,
        app_name: &str,
        target_port: u16,
        process: DevProcessIdentity,
    ) -> Result<()> {
        self.store.mutate_dev_sessions(|sessions, _| {
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
            app.target_port = Some(target_port);
            app.process = Some(process);
            session.cleanup_required = true;
            session.updated_at_ms = next_timestamp(session.updated_at_ms);
            Ok(())
        })
    }

    pub(crate) fn mark_running(&self) -> Result<()> {
        self.store.mutate_dev_sessions(|sessions, _| {
            let session = exact_session_mut(
                sessions,
                &self.session_id,
                &self.repo_root_identity,
                &self.supervisor,
            )?;
            session.phase = DevSessionPhase::Running;
            session.updated_at_ms = next_timestamp(session.updated_at_ms);
            Ok(())
        })
    }

    fn retire(&self) -> Result<bool> {
        self.store.mutate_dev_sessions(|sessions, _| {
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
        })
    }
}

impl Drop for DevSessionRuntime {
    fn drop(&mut self) {
        match self.retire() {
            Ok(true) => {}
            Ok(false) => eprintln!(
                "jig dev retained session '{}' because process-tree or route cleanup was not confirmed; inspect `jig dev status`",
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
}

#[derive(Default)]
struct ClaimConflicts {
    same_repo: Vec<DevSessionRecord>,
    other_repos: Vec<(String, String)>,
    unmanaged_routes: Vec<Route>,
}

impl ClaimConflicts {
    fn is_empty(&self) -> bool {
        self.same_repo.is_empty() && self.other_repos.is_empty() && self.unmanaged_routes.is_empty()
    }

    fn has_unsafe_replacement(&self) -> bool {
        !self.other_repos.is_empty() || !self.unmanaged_routes.is_empty()
    }

    fn same_repo_session_ids(&self) -> BTreeSet<String> {
        self.same_repo
            .iter()
            .map(|session| session.session_id.clone())
            .collect()
    }

    fn launch_error(&self, replacing: bool) -> anyhow::Error {
        if let Some((hostname, root)) = self.other_repos.first() {
            return anyhow!(
                "Development route '{hostname}' belongs to a live Jig dev session from repository {root}. `jig dev --replace` refuses cross-repository ownership; stop that repository's session or change the duplicate hostname."
            );
        }
        if let Some(route) = self.unmanaged_routes.first() {
            let owner = route
                .owner_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "<unknown>".into());
            return anyhow!(
                "Proxy route '{}' would replace a live process route owned by PID {} and targeting {}:{}, but that route is not attributable to a registered Jig dev session. `jig dev --replace` will not terminate an unregistered or ad-hoc process. Stop that process, run `jig proxy prune`, or change the duplicate hostname.",
                route.hostname,
                owner,
                route.target_host,
                route.target_port
            );
        }
        let hosts = conflict_hostnames(&self.same_repo);
        if replacing {
            anyhow!(
                "The registered Jig dev session for {} could not be replaced safely.",
                hosts.join(", ")
            )
        } else {
            anyhow!(
                "A live Jig dev session from this repository already claims {}. Run `jig dev stop` to stop all repository sessions, or retry this launch with `jig dev --replace`.",
                hosts.join(", ")
            )
        }
    }

    fn concurrent_launch_error(&self) -> anyhow::Error {
        anyhow!(
            "A concurrent Jig dev launch claimed the requested app or route while replacement was completing. No newly observed session was stopped; inspect `jig dev status` and retry."
        )
    }
}

enum ClaimOutcome {
    Claimed,
    Conflicted(ClaimConflicts),
}

fn claim_session(store: &StateStore, proposed: &DevSessionRecord) -> Result<ClaimOutcome> {
    store.mutate_dev_sessions(|sessions, routes| {
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
                conflicts
                    .other_repos
                    .push((hostname, session.repo_root_display.clone()));
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
                Some(session) => conflicts.other_repos.push((
                    route.hostname.to_string(),
                    session.repo_root_display.clone(),
                )),
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
    })
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

fn session_owns_route(session: &DevSessionRecord, route: &Route) -> bool {
    session.apps.iter().any(|app| {
        app.hostname.as_deref() == Some(route.hostname.as_str())
            && app.process.as_ref().is_some_and(|identity| {
                route.owner_pid == Some(identity.pid)
                    && route.owner_start_token == identity.start_token
            })
    })
}

fn session_observed_alive(session: &DevSessionRecord) -> bool {
    process_identity_observed_alive(&session.supervisor)
        || session
            .apps
            .iter()
            .filter_map(|app| app.process.as_ref())
            .any(process_identity_observed_alive)
}

fn route_is_live(route: &Route) -> bool {
    match route.mode {
        RouteMode::Alias => true,
        RouteMode::Process => route
            .owner_pid
            .zip(route.owner_start_token.as_deref())
            .is_some_and(|(pid, token)| {
                pid_is_alive(pid)
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
        .retain(|entry| other.insert(entry.clone()));
    let mut routes = HashSet::new();
    conflicts
        .unmanaged_routes
        .retain(|route| routes.insert(route.hostname.to_string()));
}

fn conflict_hostnames(sessions: &[DevSessionRecord]) -> Vec<String> {
    let hosts = sessions
        .iter()
        .flat_map(|session| &session.apps)
        .filter_map(|app| app.hostname.clone().or_else(|| Some(app.name.clone())))
        .collect::<BTreeSet<_>>();
    hosts.into_iter().collect()
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
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in root.as_os_str().encode_wide() {
            digest.update(unit.to_le_bytes());
        }
    }
    #[cfg(not(any(unix, windows)))]
    digest.update(root.to_string_lossy().as_bytes());
    format!("sha256:{:x}", digest.finalize())
}
