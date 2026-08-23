use std::path::{Path, PathBuf};

use anyhow::Result;

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
        self.with_route_lock(|routes_path| self.mutate_dev_sessions_unlocked(routes_path, mutate))
    }

    pub(crate) fn mutate_dev_sessions_interruptible<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        mutate: impl FnOnce(&mut Vec<DevSessionRecord>, &[Route]) -> Result<T>,
    ) -> Result<LockOutcome<T>> {
        self.with_route_lock_interruptible(cancelled, |routes_path| {
            self.mutate_dev_sessions_unlocked(routes_path, mutate)
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
            let mut sessions = dev_sessions::read_from_path(&sessions_path)?;
            let original_sessions = sessions.clone();
            let result = mutate(&mut sessions, &mut routes)?;
            dev_sessions::validate_records(&sessions)?;

            if routes != original_routes {
                write_routes_to_path(routes_path, &routes)?;
            }
            if sessions != original_sessions {
                dev_sessions::write_to_path(&sessions_path, &sessions)?;
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
            self.mutate_dev_sessions_unlocked(routes_path, mutate)
        })
    }

    fn mutate_dev_sessions_unlocked<T>(
        &self,
        routes_path: &Path,
        mutate: impl FnOnce(&mut Vec<DevSessionRecord>, &[Route]) -> Result<T>,
    ) -> Result<T> {
        let routes = read_routes_from_path(routes_path)?;
        let sessions_path = self.dev_sessions_path();
        let mut sessions = dev_sessions::read_from_path(&sessions_path)?;
        let original = sessions.clone();
        let result = mutate(&mut sessions, &routes)?;
        dev_sessions::validate_records(&sessions)?;
        if sessions != original {
            dev_sessions::write_to_path(&sessions_path, &sessions)?;
        }
        Ok(result)
    }

    pub(super) fn dev_sessions_path(&self) -> PathBuf {
        self.root.join(dev_sessions::FILE_NAME)
    }
}
