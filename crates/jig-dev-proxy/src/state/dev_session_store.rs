use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::types::Route;

use super::{
    DevSessionRecord, DevStateSnapshot, LockOutcome, StateStore, dev_sessions,
    read_routes_from_path, write_routes_to_path,
};

impl StateStore {
    /// Returns one lock-consistent snapshot of persisted development sessions
    /// and proxy routes.
    ///
    /// Callers must release this snapshot before performing process or network
    /// work; the state lock is held only while the files are read.
    pub(crate) fn snapshot_dev_state(&self) -> Result<DevStateSnapshot> {
        self.with_route_lock(|routes_path| self.snapshot_dev_state_unlocked(routes_path))
    }

    pub(crate) fn snapshot_dev_state_interruptible(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<DevStateSnapshot>> {
        self.with_route_lock_interruptible(cancelled, |routes_path| {
            self.snapshot_dev_state_unlocked(routes_path)
        })
    }

    fn snapshot_dev_state_unlocked(&self, routes_path: &Path) -> Result<DevStateSnapshot> {
        Ok(DevStateSnapshot {
            sessions: dev_sessions::read_from_path(&self.dev_sessions_path())?,
            routes: read_routes_from_path(routes_path)?,
        })
    }

    /// Mutates development sessions while observing the routes protected by
    /// the same state lock.
    ///
    /// Route state is read-only at this boundary. The closure must return
    /// before callers perform process or network work. If the closure fails,
    /// or leaves the session collection unchanged, no session file is written.
    #[cfg(test)]
    pub(crate) fn mutate_dev_sessions<T>(
        &self,
        mutate: impl FnOnce(&mut Vec<DevSessionRecord>, &[Route]) -> Result<T>,
    ) -> Result<T> {
        self.with_route_lock(|routes_path| {
            self.mutate_dev_sessions_unlocked(routes_path, false, mutate)
        })
    }

    pub(crate) fn mutate_dev_sessions_interruptible<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        mutate: impl FnOnce(&mut Vec<DevSessionRecord>, &[Route]) -> Result<T>,
    ) -> Result<LockOutcome<T>> {
        self.with_route_lock_interruptible(cancelled, |routes_path| {
            self.mutate_dev_sessions_unlocked(routes_path, false, mutate)
        })
    }

    /// The version transition and first claim share the route lock. Until
    /// contextless legacy recovery is available, callers keep the cutover
    /// disabled; tests exercise the future transition explicitly.
    pub(crate) fn mutate_dev_sessions_for_claim_interruptible<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        enable_v2_cutover: bool,
        mutate: impl FnOnce(&mut Vec<DevSessionRecord>, &[Route]) -> Result<T>,
    ) -> Result<LockOutcome<T>> {
        self.with_route_lock_interruptible(cancelled, |routes_path| {
            self.mutate_dev_sessions_unlocked(routes_path, enable_v2_cutover, mutate)
        })
    }

    /// Mutates development sessions and routes together under the shared route
    /// lock. Route state is persisted first, so a partial write retains the
    /// conservative session record and the whole operation can be retried.
    ///
    /// The closure must return before callers perform network work or signal
    /// processes. This boundary exists for coordinated metadata cleanup, not
    /// for turning persisted process observations into signaling authority.
    pub(crate) fn mutate_dev_state_interruptible<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        mutate: impl FnOnce(&mut Vec<DevSessionRecord>, &mut Vec<Route>) -> Result<T>,
    ) -> Result<LockOutcome<T>> {
        self.with_route_lock_interruptible(cancelled, |routes_path| {
            let mut routes = read_routes_from_path(routes_path)?;
            let original_routes = routes.clone();
            let sessions_path = self.dev_sessions_path();
            let mut state = dev_sessions::read_document_from_path(&sessions_path)?;
            let original_sessions = state.sessions.clone();
            let result = mutate(&mut state.sessions, &mut routes)?;
            dev_sessions::validate_records(&state.sessions)?;

            if routes != original_routes {
                write_routes_to_path(routes_path, &routes)?;
            }
            if state.sessions != original_sessions {
                dev_sessions::write_to_path(&sessions_path, state.version, &state.sessions)?;
            }
            Ok(result)
        })
    }

    pub(crate) fn mutate_dev_sessions_cleanup_cancelable<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        mutate: impl FnOnce(&mut Vec<DevSessionRecord>, &[Route]) -> Result<T>,
    ) -> Result<Option<T>> {
        self.with_route_lock_cancelable(cancelled, |routes_path| {
            self.mutate_dev_sessions_unlocked(routes_path, false, mutate)
        })
    }

    fn mutate_dev_sessions_unlocked<T>(
        &self,
        routes_path: &Path,
        enable_v2_cutover: bool,
        mutate: impl FnOnce(&mut Vec<DevSessionRecord>, &[Route]) -> Result<T>,
    ) -> Result<T> {
        let routes = read_routes_from_path(routes_path)?;
        let sessions_path = self.dev_sessions_path();
        let mut state = dev_sessions::read_document_from_path(&sessions_path)?;
        if enable_v2_cutover && state.version != dev_sessions::COMPLETE_EVIDENCE_VERSION {
            if !state.sessions.is_empty() {
                let blockers = state
                    .sessions
                    .iter()
                    .take(8)
                    .map(|session| {
                        format!("'{}' ({})", session.session_id, session.repo_root_display)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                bail!(
                    "Jig dev cannot upgrade the nonempty legacy sessions store at {}: {} session(s) require explicit cleanup or repair; first blockers: {}. Run `jig dev status --all --state-dir PATH` with this state directory to inspect every session, including sessions without proxy routes. No session was stopped or changed.",
                    self.root.display(),
                    state.sessions.len(),
                    blockers,
                );
            }
            state.version = dev_sessions::COMPLETE_EVIDENCE_VERSION;
        }
        let original = state.sessions.clone();
        let result = mutate(&mut state.sessions, &routes)?;
        dev_sessions::validate_records(&state.sessions)?;
        if state.sessions != original {
            dev_sessions::write_to_path(&sessions_path, state.version, &state.sessions)?;
        }
        Ok(result)
    }

    pub(super) fn dev_sessions_path(&self) -> PathBuf {
        self.root.join(dev_sessions::FILE_NAME)
    }
}
