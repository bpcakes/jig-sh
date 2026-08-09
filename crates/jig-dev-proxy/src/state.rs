use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

use crate::file_ops;
use crate::host::{RouteHostname, validate_routed_hostname};
use crate::types::{Route, RouteMode};

pub(crate) use dev_sessions::{
    DevProcessIdentity, DevSessionApp, DevSessionControl, DevSessionPhase, DevSessionRecord,
    DevStateSnapshot,
};
use process_identity::route_is_alive;
#[cfg(test)]
use process_identity::windows_tasklist_csv_pid;
pub(crate) use process_identity::{
    pid_is_alive, process_start_token, process_start_tokens_supported,
};
pub(crate) use signature::{FileSignature, file_signature};

mod dev_session_store;
mod dev_sessions;
mod process_identity;
mod resolution;
mod signature;

use resolution::ensure_state_dir_has_no_symlinks;

const ROUTES_VERSION: u32 = 1;
const ROUTES_FILE: &str = "routes.json";
const LOCK_FILE: &str = "routes.lock";
const RUNTIME_LOCK_FILE: &str = "runtime.lock";
const PID_FILE: &str = "proxy.pid";
const EXE_FILE: &str = "proxy-exe.txt";
const HTTP_PORT_FILE: &str = "proxy-http.port";
const HTTPS_PORT_FILE: &str = "proxy-https.port";
const HEALTH_TOKEN_FILE: &str = "proxy-health-token";
const LEAF_HOSTS_FILE: &str = "leaf-hosts.json";
const CERT_LOCK_FILE: &str = "certs.lock";
const STATE_FILE_FALLBACK: &str = "jig-proxy-state";
#[cfg(not(test))]
const REPLACE_BACKUP_RECOVERY_DELAY: Duration = Duration::from_secs(30);
#[cfg(test)]
const REPLACE_BACKUP_RECOVERY_DELAY: Duration = Duration::ZERO;
const MISSING_FILE_READ_RETRY_DELAY: Duration = Duration::from_millis(25);
const MISSING_FILE_READ_ATTEMPTS: usize = 3;
pub const MAX_ROUTES_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const STATE_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const STATE_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(100);

static CLOCK_WARNING_PRINTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static ROUTE_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
    static CERT_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Debug)]
pub(crate) struct StateStore {
    // Clones are cheap handles to the same state directory. They do not share
    // in-memory lock state; every mutating operation reacquires the advisory
    // file lock for the specific route/runtime/cert file it touches.
    root: PathBuf,
    can_chmod_root: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum LockOutcome<T> {
    Acquired(T),
    Cancelled,
}

/// Stable identity for one process-owned route publication.
///
/// Cleanup must never identify a route by hostname alone: once the original
/// owner exits, another session may legitimately publish a replacement before
/// the old session acquires the route lock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessRouteOwnership {
    hostname: RouteHostname,
    owner_pid: u32,
    owner_start_token: String,
}

impl ProcessRouteOwnership {
    pub(crate) const fn new(
        hostname: RouteHostname,
        owner_pid: u32,
        owner_start_token: String,
    ) -> Self {
        Self {
            hostname,
            owner_pid,
            owner_start_token,
        }
    }

    pub(crate) fn hostname(&self) -> &str {
        self.hostname.as_str()
    }

    fn matches(&self, route: &Route) -> bool {
        route.mode == RouteMode::Process
            && route.hostname == self.hostname
            && route.owner_pid == Some(self.owner_pid)
            && route.owner_start_token.as_deref() == Some(self.owner_start_token.as_str())
    }
}

impl StateStore {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn ca_path(&self) -> PathBuf {
        self.root.join("ca.pem")
    }

    pub(crate) fn ca_key_path(&self) -> PathBuf {
        self.root.join("ca-key.pem")
    }

    pub(crate) fn leaf_path(&self) -> PathBuf {
        self.root.join("leaf.pem")
    }

    pub(crate) fn leaf_key_path(&self) -> PathBuf {
        self.root.join("leaf-key.pem")
    }

    pub(crate) fn leaf_hosts_path(&self) -> PathBuf {
        self.root.join(LEAF_HOSTS_FILE)
    }

    pub(crate) fn trusted_ca_path(&self) -> PathBuf {
        self.root.join("trusted-ca.json")
    }

    pub(crate) fn log_path(&self) -> PathBuf {
        self.root.join("proxy.log")
    }

    pub(crate) fn pid_path(&self) -> PathBuf {
        self.root.join(PID_FILE)
    }

    pub(crate) fn proxy_exe_path(&self) -> PathBuf {
        self.root.join(EXE_FILE)
    }

    pub(crate) fn http_port_path(&self) -> PathBuf {
        self.root.join(HTTP_PORT_FILE)
    }

    pub(crate) fn https_port_path(&self) -> PathBuf {
        self.root.join(HTTPS_PORT_FILE)
    }

    pub(crate) fn health_token_path(&self) -> PathBuf {
        self.root.join(HEALTH_TOKEN_FILE)
    }

    pub(crate) fn read_routes(&self, prune_dead: bool) -> Result<Vec<Route>> {
        let routes = self.with_route_lock(read_routes_from_path)?;
        if !prune_dead {
            return Ok(routes);
        }
        // Prune-on-read is intentionally in-memory only. Use `prune` when the
        // caller needs the persisted routes file rewritten.
        Ok(routes.into_iter().filter(route_is_alive).collect())
    }

    pub(crate) fn read_routes_interruptible(
        &self,
        prune_dead: bool,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<Vec<Route>>> {
        match self.with_route_lock_interruptible(cancelled, read_routes_from_path)? {
            LockOutcome::Acquired(routes) if prune_dead => Ok(LockOutcome::Acquired(
                routes.into_iter().filter(route_is_alive).collect(),
            )),
            outcome => Ok(outcome),
        }
    }

    #[cfg(test)]
    pub(crate) fn add_route(&self, route: Route) -> Result<()> {
        if route.mode == RouteMode::Process && !process_start_tokens_supported() {
            anyhow::bail!(
                "Process routes require process start-token verification on this platform. Use `scripts/jig proxy alias` for an already-running app, or run with --no-proxy."
            );
        }
        self.with_route_lock(|path| add_route_to_path(path, route))
    }

    #[cfg(test)]
    pub(crate) fn add_verified_route<F>(&self, route: Route, mut verify: F) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        if route.mode == RouteMode::Process && !process_start_tokens_supported() {
            anyhow::bail!(
                "Process routes require process start-token verification on this platform. Use `scripts/jig proxy alias` for an already-running app, or run with --no-proxy."
            );
        }
        self.with_route_lock(|path| add_route_to_path_verified(path, route, &mut verify))
    }

    #[cfg(test)]
    pub(crate) fn add_route_then_fail_after_write(&self, route: Route) -> Result<()> {
        self.with_route_lock(|path| {
            add_route_to_path(path, route)?;
            anyhow::bail!("injected route publication failure after durable write")
        })
    }

    pub(crate) fn add_verified_route_interruptible<F>(
        &self,
        route: Route,
        cancelled: &impl Fn() -> bool,
        mut verify: F,
    ) -> Result<LockOutcome<()>>
    where
        F: FnMut() -> Result<()>,
    {
        if route.mode == RouteMode::Process && !process_start_tokens_supported() {
            anyhow::bail!(
                "Process routes require process start-token verification on this platform. Use `scripts/jig proxy alias` for an already-running app, or run with --no-proxy."
            );
        }
        self.with_route_lock_interruptible(cancelled, |path| {
            add_route_to_path_verified(path, route, &mut verify)
        })
    }

    pub(crate) fn add_alias_route(&self, route: Route) -> Result<()> {
        if route.mode != RouteMode::Alias {
            anyhow::bail!("add_alias_route requires RouteMode::Alias");
        }
        self.with_route_lock(|path| add_route_to_path(path, route))
    }

    #[cfg(test)]
    pub(crate) fn ensure_no_live_process_routes_for_hostnames<'a>(
        &self,
        hostnames: impl IntoIterator<Item = &'a str>,
    ) -> Result<()> {
        let hostnames = hostnames
            .into_iter()
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        self.with_route_lock(|path| {
            let routes = read_routes_from_path(path)?;
            for hostname in &hostnames {
                ensure_no_live_process_route_for_hostname(&routes, hostname)?;
            }
            Ok(())
        })
    }

    pub(crate) fn ensure_no_live_process_routes_for_hostnames_interruptible<'a>(
        &self,
        hostnames: impl IntoIterator<Item = &'a str>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<()>> {
        let hostnames = hostnames
            .into_iter()
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        self.with_route_lock_interruptible(cancelled, |path| {
            let routes = read_routes_from_path(path)?;
            for hostname in &hostnames {
                ensure_no_live_process_route_for_hostname(&routes, hostname)?;
            }
            Ok(())
        })
    }

    /// Removes a route while allowing forced foreground cleanup to abandon a
    /// contended lock wait. `Ok(false)` means no mutation occurred.
    #[cfg(test)]
    pub(crate) fn remove_route_cancelable(
        &self,
        hostname: &str,
        cancelled: impl Fn() -> bool,
    ) -> Result<bool> {
        let hostname = hostname.to_ascii_lowercase();
        Ok(self
            .with_route_lock_cancelable(&cancelled, |path| {
                let mut routes = read_routes_from_path(path)?;
                routes.retain(|existing| existing.hostname.as_str() != hostname);
                routes.retain(route_is_alive);
                write_routes_to_path(path, &routes)
            })?
            .is_some())
    }

    /// Removes only the route publication owned by `ownership`, while allowing
    /// forced cleanup to abandon an actually contended lock wait. A missing or
    /// replaced route is already clean and therefore still returns `Ok(true)`
    /// once the lock-protected check completes.
    pub(crate) fn remove_process_route_if_owned_cancelable(
        &self,
        ownership: &ProcessRouteOwnership,
        cancelled: impl Fn() -> bool,
    ) -> Result<bool> {
        self.remove_process_route_if_owned_cancelable_until(
            ownership,
            Instant::now() + STATE_LOCK_TIMEOUT,
            cancelled,
        )
    }

    /// Removes only the route publication owned by `ownership` without waiting
    /// past the cleanup operation's shared absolute deadline.
    pub(crate) fn remove_process_route_if_owned_cancelable_until(
        &self,
        ownership: &ProcessRouteOwnership,
        deadline: Instant,
        cancelled: impl Fn() -> bool,
    ) -> Result<bool> {
        Ok(self
            .with_route_lock_cancelable_until(deadline, &cancelled, |path| {
                let mut routes = read_routes_from_path(path)?;
                let route_count = routes.len();
                routes.retain(|existing| !ownership.matches(existing));
                if routes.len() == route_count {
                    return Ok(());
                }
                write_routes_to_path(path, &routes)
            })?
            .is_some())
    }

    pub(crate) fn prune(&self) -> Result<Vec<Route>> {
        self.with_route_lock(|path| {
            let routes = read_routes_from_path(path)?;
            let original_len = routes.len();
            let pruned: Vec<_> = routes.into_iter().filter(route_is_alive).collect();
            if pruned.len() != original_len {
                write_routes_to_path(path, &pruned)?;
            }
            Ok(pruned)
        })
    }

    #[cfg(test)]
    pub(crate) fn write_pid(&self, pid: u32) -> Result<()> {
        self.with_runtime_lock(|| {
            file_ops::write_atomic_text(self.pid_path(), &pid.to_string(), STATE_FILE_FALLBACK)
        })
    }

    pub(crate) fn read_pid(&self) -> Result<Option<u32>> {
        self.with_runtime_lock(|| self.read_pid_unlocked())
    }

    pub(crate) fn read_pid_interruptible(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<Option<u32>>> {
        self.with_runtime_lock_interruptible(cancelled, || self.read_pid_unlocked())
    }

    fn read_pid_unlocked(&self) -> Result<Option<u32>> {
        // PID files are only identity hints; callers that need process identity
        // should pair this with the health-token handshake or a start token.
        let path = self.pid_path();
        let Some(text) = file_ops::read_text_no_follow(&path)? else {
            return Ok(None);
        };
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        match text.parse() {
            Ok(pid) => Ok(Some(pid)),
            Err(error) => anyhow::bail!(
                "Invalid Jig proxy PID file {}: {error}. Remove the file or run with a clean JIG_PROXY_STATE_DIR.",
                self.pid_path().display()
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn write_proxy_exe(&self, path: &Path) -> Result<()> {
        self.with_runtime_lock(|| {
            file_ops::write_atomic_text(
                self.proxy_exe_path(),
                &path.to_string_lossy(),
                STATE_FILE_FALLBACK,
            )
        })
    }

    pub(crate) fn read_proxy_exe_status(&self) -> Result<ProxyExeStatus> {
        self.with_runtime_lock(|| Ok(self.read_proxy_exe_status_unlocked()))
    }

    fn read_proxy_exe_status_unlocked(&self) -> ProxyExeStatus {
        let text = match file_ops::read_text_no_follow(&self.proxy_exe_path()) {
            Ok(Some(text)) => text,
            Ok(None) => return ProxyExeStatus::default(),
            Err(error) => {
                return ProxyExeStatus {
                    path: None,
                    warning: Some(format!(
                        "Could not read recorded proxy executable path {}: {error}",
                        self.proxy_exe_path().display()
                    )),
                };
            }
        };
        let text = text.trim();
        if text.is_empty() {
            return ProxyExeStatus::default();
        }
        let recorded = PathBuf::from(text);
        let path = match recorded.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                return ProxyExeStatus {
                    path: None,
                    warning: Some(format!(
                        "Recorded proxy executable {} is not available: {error}",
                        recorded.display()
                    )),
                };
            }
        };
        if proxy_exe_is_usable(&path) {
            ProxyExeStatus {
                path: Some(path),
                warning: None,
            }
        } else {
            ProxyExeStatus {
                path: None,
                warning: Some(format!(
                    "Recorded proxy executable {} is not a usable executable file",
                    path.display()
                )),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn write_http_port(&self, port: u16) -> Result<()> {
        self.with_runtime_lock(|| {
            file_ops::write_atomic_text(
                self.http_port_path(),
                &port.to_string(),
                STATE_FILE_FALLBACK,
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn write_https_port(&self, port: u16) -> Result<()> {
        self.with_runtime_lock(|| {
            file_ops::write_atomic_text(
                self.https_port_path(),
                &port.to_string(),
                STATE_FILE_FALLBACK,
            )
        })
    }

    pub(crate) fn read_http_port(&self) -> Result<Option<u16>> {
        self.with_runtime_lock(|| Ok(read_port_file(&self.http_port_path())))
    }

    pub(crate) fn read_http_port_interruptible(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<Option<u16>>> {
        self.with_runtime_lock_interruptible(cancelled, || {
            Ok(read_port_file(&self.http_port_path()))
        })
    }

    pub(crate) fn read_https_port(&self) -> Result<Option<u16>> {
        self.with_runtime_lock(|| Ok(read_port_file(&self.https_port_path())))
    }

    pub(crate) fn read_https_port_interruptible(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<Option<u16>>> {
        self.with_runtime_lock_interruptible(cancelled, || {
            Ok(read_port_file(&self.https_port_path()))
        })
    }

    #[cfg(test)]
    pub(crate) fn ensure_health_token(&self) -> Result<String> {
        self.with_runtime_lock(|| {
            if let Some(token) = read_health_token_file(&self.health_token_path())? {
                return Ok(token);
            }
            let token = random_health_token()?;
            file_ops::write_atomic_text(self.health_token_path(), &token, STATE_FILE_FALLBACK)?;
            Ok(token)
        })
    }

    pub(crate) fn read_health_token(&self) -> Result<Option<String>> {
        self.with_runtime_lock(|| read_health_token_file(&self.health_token_path()))
    }

    pub(crate) fn read_health_token_interruptible(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<Option<String>>> {
        self.with_runtime_lock_interruptible(cancelled, || {
            read_health_token_file(&self.health_token_path())
        })
    }

    pub(crate) fn clear_runtime_files(&self) {
        if let Err(error) = self.try_clear_runtime_files() {
            eprintln!(
                "jig proxy could not clear runtime files in {}: {error}",
                self.root.display()
            );
        }
    }

    pub(crate) fn try_clear_runtime_files(&self) -> Result<()> {
        self.with_runtime_lock(|| self.remove_runtime_files_unlocked())
    }

    pub(crate) fn replace_runtime_files(
        &self,
        current_exe: &Path,
        http_port: u16,
        https_port: Option<u16>,
    ) -> Result<String> {
        self.with_runtime_lock(|| {
            self.remove_runtime_files_unlocked()?;
            let health_token = random_health_token()?;
            file_ops::write_atomic_text(
                self.health_token_path(),
                &health_token,
                STATE_FILE_FALLBACK,
            )?;
            file_ops::write_atomic_text(
                self.pid_path(),
                &std::process::id().to_string(),
                STATE_FILE_FALLBACK,
            )?;
            file_ops::write_atomic_text(
                self.proxy_exe_path(),
                &current_exe.to_string_lossy(),
                STATE_FILE_FALLBACK,
            )?;
            file_ops::write_atomic_text(
                self.http_port_path(),
                &http_port.to_string(),
                STATE_FILE_FALLBACK,
            )?;
            if let Some(port) = https_port {
                file_ops::write_atomic_text(
                    self.https_port_path(),
                    &port.to_string(),
                    STATE_FILE_FALLBACK,
                )?;
            }
            Ok(health_token)
        })
    }

    pub(crate) fn with_cert_lock<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        // Keep the post-hardening scan close to every recursive ACL/chmod pass.
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let _lock_order_guard = enter_lock_order_guard(LockKind::Cert)?;
        let lock = open_lock_file(self.root.join(CERT_LOCK_FILE))?;
        lock_state_file(&lock, "cert lock")?;
        let lock = LockedFile::new(lock, "cert lock");
        let result = f();
        let unlock_result = lock.unlock();
        finish_with_unlock("cert lock", result, unlock_result)
    }

    pub(crate) fn with_cert_lock_interruptible<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        f: impl FnOnce() -> Result<T>,
    ) -> Result<LockOutcome<T>> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let _lock_order_guard = enter_lock_order_guard(LockKind::Cert)?;
        let lock = open_lock_file(self.root.join(CERT_LOCK_FILE))?;
        if !lock_state_file_interruptible(&lock, "cert lock", cancelled)? {
            return Ok(LockOutcome::Cancelled);
        }
        let lock = LockedFile::new(lock, "cert lock");
        if cancelled() {
            lock.unlock()?;
            return Ok(LockOutcome::Cancelled);
        }
        let result = f();
        let unlock_result = lock.unlock();
        finish_with_unlock("cert lock", result, unlock_result).map(LockOutcome::Acquired)
    }

    pub(crate) fn routes_signature(&self) -> FileSignature {
        file_signature(&self.routes_path())
    }

    fn routes_path(&self) -> PathBuf {
        self.root.join(ROUTES_FILE)
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    fn runtime_lock_path(&self) -> PathBuf {
        self.root.join(RUNTIME_LOCK_FILE)
    }

    fn with_route_lock<T>(&self, f: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
        self.with_route_lock_cancelable(&|| false, f)?
            .ok_or_else(|| anyhow::anyhow!("uncancelled route lock acquisition was cancelled"))
    }

    fn with_route_lock_cancelable<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        f: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<Option<T>> {
        self.with_route_lock_cancelable_until(Instant::now() + STATE_LOCK_TIMEOUT, cancelled, f)
    }

    fn with_route_lock_cancelable_until<T>(
        &self,
        deadline: Instant,
        cancelled: &impl Fn() -> bool,
        f: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<Option<T>> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        // Keep the post-hardening scan close to every recursive ACL/chmod pass.
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let lock = open_lock_file(self.lock_path())?;
        if !lock_state_file_cleanup_cancelable_until(&lock, "route lock", deadline, cancelled)? {
            return Ok(None);
        }
        let lock = LockedFile::new(lock, "route lock");
        let _lock_order_guard = enter_lock_order_guard(LockKind::Route)?;
        recover_replace_backups(&self.root)?;

        let routes_path = self.routes_path();
        let result = f(&routes_path);
        let unlock_result = lock.unlock();
        finish_with_unlock("route lock", result, unlock_result).map(Some)
    }

    fn with_route_lock_interruptible<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        f: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<LockOutcome<T>> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let lock = open_lock_file(self.lock_path())?;
        if !lock_state_file_interruptible(&lock, "route lock", cancelled)? {
            return Ok(LockOutcome::Cancelled);
        }
        let lock = LockedFile::new(lock, "route lock");
        let _lock_order_guard = enter_lock_order_guard(LockKind::Route)?;
        if cancelled() {
            lock.unlock()?;
            return Ok(LockOutcome::Cancelled);
        }
        recover_replace_backups(&self.root)?;
        if cancelled() {
            lock.unlock()?;
            return Ok(LockOutcome::Cancelled);
        }
        let result = f(&self.routes_path());
        let unlock_result = lock.unlock();
        finish_with_unlock("route lock", result, unlock_result).map(LockOutcome::Acquired)
    }

    fn with_runtime_lock<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        // Keep the post-hardening scan close to every recursive ACL/chmod pass.
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let lock = open_lock_file(self.runtime_lock_path())?;
        lock_state_file(&lock, "runtime lock")?;
        let lock = LockedFile::new(lock, "runtime lock");

        let result = f();
        let unlock_result = lock.unlock();
        finish_with_unlock("runtime lock", result, unlock_result)
    }

    fn with_runtime_lock_interruptible<T>(
        &self,
        cancelled: &impl Fn() -> bool,
        f: impl FnOnce() -> Result<T>,
    ) -> Result<LockOutcome<T>> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let lock = open_lock_file(self.runtime_lock_path())?;
        // Runtime operations used here are read-only. Try an immediately free
        // lock even after cancellation so a concurrently completed proxy can
        // still be recognized as shared/ready; abandon only an actual wait.
        if !lock_state_file_cleanup_cancelable(&lock, "runtime lock", cancelled)? {
            return Ok(LockOutcome::Cancelled);
        }
        let lock = LockedFile::new(lock, "runtime lock");
        let result = f();
        let unlock_result = lock.unlock();
        finish_with_unlock("runtime lock", result, unlock_result).map(LockOutcome::Acquired)
    }

    fn remove_runtime_files_unlocked(&self) -> Result<()> {
        remove_runtime_file(self.pid_path())?;
        remove_runtime_file(self.proxy_exe_path())?;
        remove_runtime_file(self.http_port_path())?;
        remove_runtime_file(self.https_port_path())?;
        remove_runtime_file(self.health_token_path())?;
        Ok(())
    }
}

fn remove_runtime_file(path: PathBuf) -> Result<()> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to remove runtime file {}", path.display()))
        }
    }
}

#[derive(Default)]
pub(crate) struct ProxyExeStatus {
    pub(crate) path: Option<PathBuf>,
    pub(crate) warning: Option<String>,
}

struct LockedFile {
    file: File,
    label: &'static str,
    unlocked: bool,
}

#[derive(Clone, Copy)]
enum LockKind {
    Route,
    Cert,
}

struct LockOrderGuard {
    kind: LockKind,
}

fn enter_lock_order_guard(kind: LockKind) -> Result<LockOrderGuard> {
    match kind {
        LockKind::Route => {
            ROUTE_LOCK_DEPTH.with(|depth| depth.set(depth.get() + 1));
        }
        LockKind::Cert => {
            if ROUTE_LOCK_DEPTH.with(std::cell::Cell::get) != 0 {
                anyhow::bail!("cert lock cannot be acquired while a route lock is held");
            }
            CERT_LOCK_DEPTH.with(|depth| depth.set(depth.get() + 1));
        }
    }
    Ok(LockOrderGuard { kind })
}

impl Drop for LockOrderGuard {
    fn drop(&mut self) {
        match self.kind {
            LockKind::Route => {
                ROUTE_LOCK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
            }
            LockKind::Cert => {
                CERT_LOCK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
            }
        }
    }
}

impl LockedFile {
    const fn new(file: File, label: &'static str) -> Self {
        Self {
            file,
            label,
            unlocked: false,
        }
    }

    fn unlock(mut self) -> std::io::Result<()> {
        let result = FileExt::unlock(&self.file);
        if result.is_ok() {
            self.unlocked = true;
        }
        result
    }
}

impl Drop for LockedFile {
    fn drop(&mut self) {
        if !self.unlocked {
            if let Err(error) = FileExt::unlock(&self.file) {
                let _ = writeln!(
                    std::io::stderr(),
                    "jig proxy failed to unlock {} while dropping lock guard: {error}",
                    self.label
                );
            }
        }
    }
}

fn add_route_to_path(path: &Path, route: Route) -> Result<()> {
    validate_route_for_write(&route)?;
    let mut routes = read_routes_from_path(path)?;
    ensure_no_live_process_route_replacement(&routes, &route)?;
    routes.retain(|existing| existing.hostname != route.hostname);
    routes.retain(route_is_alive);
    routes.push(route);
    write_routes_to_path(path, &routes)
}

fn add_route_to_path_verified<F>(path: &Path, route: Route, verify: &mut F) -> Result<()>
where
    F: FnMut() -> Result<()>,
{
    validate_route_for_write(&route)?;
    let mut routes = read_routes_from_path(path)?;
    ensure_no_live_process_route_replacement(&routes, &route)?;
    verify()?;
    let rollback_routes = routes.clone();
    routes.retain(|existing| existing.hostname != route.hostname);
    routes.retain(route_is_alive);
    routes.push(route.clone());
    write_routes_to_path(path, &routes)?;
    if let Err(error) = verify() {
        if let Err(cleanup_error) = write_routes_to_path(path, &rollback_routes) {
            return Err(error).context(format!(
                "verification failed for route '{}', and rollback also failed: {cleanup_error}",
                route.hostname
            ));
        }
        return Err(error);
    }
    Ok(())
}

fn finish_with_unlock<T>(
    label: &str,
    result: Result<T>,
    unlock_result: std::io::Result<()>,
) -> Result<T> {
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(unlock_error)) => {
            eprintln!("jig proxy failed to unlock {label} after an earlier error: {unlock_error}");
            Err(error)
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        Err(error) => {
            if !CLOCK_WARNING_PRINTED.swap(true, Ordering::Relaxed) {
                eprintln!(
                    "jig proxy system clock is before the Unix epoch; route timestamps will use 0: {error}"
                );
            }
            0
        }
    }
}

fn read_routes_from_file(file: &mut File) -> Result<Vec<Route>> {
    file.seek(SeekFrom::Start(0))?;
    let len = file.metadata()?.len();
    if len > MAX_ROUTES_FILE_BYTES {
        anyhow::bail!(
            "Jig proxy routes file is {len} bytes, above the {MAX_ROUTES_FILE_BYTES} byte limit"
        );
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let routes = match serde_json::from_str::<RoutesFile>(&text)
        .context("Failed to parse Jig proxy routes")?
    {
        RoutesFile::Versioned(document) if document.version == ROUTES_VERSION => document.routes,
        RoutesFile::Versioned(document) => {
            let version = document.version;
            anyhow::bail!("Unsupported Jig proxy routes version {version}");
        }
        RoutesFile::Legacy(routes) => routes,
    };
    for route in &routes {
        validate_route_for_read(route)?;
    }
    Ok(routes)
}

fn read_routes_from_path(path: &Path) -> Result<Vec<Route>> {
    let Some(mut file) = open_read_no_follow_maybe_missing(path)? else {
        return Ok(Vec::new());
    };
    ensure_private_state_file_permissions(path, &file)?;
    read_routes_from_file(&mut file)
}

fn ensure_private_state_file_permissions(path: &Path, file: &File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o7777;
        if mode != 0o600 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "private state file {} must have mode 600, found {:o}",
                    path.display(),
                    mode
                ),
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = (path, file);
    Ok(())
}

fn open_read_no_follow_maybe_missing(path: &Path) -> std::io::Result<Option<File>> {
    for attempt in 0..MISSING_FILE_READ_ATTEMPTS {
        match file_ops::open_read_no_follow(path) {
            Ok(file) => return Ok(Some(file)),
            Err(error)
                if cfg!(windows)
                    && error.kind() == std::io::ErrorKind::NotFound
                    && attempt + 1 < MISSING_FILE_READ_ATTEMPTS =>
            {
                std::thread::sleep(MISSING_FILE_READ_RETRY_DELAY);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return missing_file_read_result(path, cfg!(windows));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn missing_file_read_result(
    path: &Path,
    fail_on_replace_backup: bool,
) -> std::io::Result<Option<File>> {
    if fail_on_replace_backup && file_ops::replace_backup_for_path_exists(path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "state file {} is temporarily unavailable during replacement",
                path.display()
            ),
        ));
    }
    Ok(None)
}

fn validate_route(route: &Route) -> Result<()> {
    validate_routed_hostname(&route.hostname)?;
    crate::host::validate_route_target_host(&route.target_host)?;
    if route.target_port == 0 {
        anyhow::bail!("Jig proxy route '{}' has target port 0", route.hostname);
    }
    Ok(())
}

fn validate_route_for_write(route: &Route) -> Result<()> {
    validate_route(route)?;
    match route.mode {
        RouteMode::Alias => {}
        RouteMode::Process => {
            if route.owner_pid.is_none() || route.owner_start_token.is_none() {
                anyhow::bail!(
                    "Process route '{}' must include owner PID and start token before it can be persisted",
                    route.hostname
                );
            }
        }
    }
    Ok(())
}

fn validate_route_for_read(route: &Route) -> Result<()> {
    validate_route_for_write(route)?;
    if route.mode == RouteMode::Process && !process_start_tokens_supported() {
        anyhow::bail!(
            "Process route '{}' cannot be trusted on this platform because process start-token verification is unavailable",
            route.hostname
        );
    }
    Ok(())
}

fn ensure_no_live_process_route_replacement(routes: &[Route], route: &Route) -> Result<()> {
    ensure_no_live_process_route_for_hostname(routes, route.hostname.as_str())
}

fn ensure_no_live_process_route_for_hostname(routes: &[Route], hostname: &str) -> Result<()> {
    if let Some(existing) = live_process_route_for_hostname(routes, hostname) {
        let owner = existing
            .owner_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "<unknown>".into());
        anyhow::bail!(
            "Proxy route '{}' would replace a live process route owned by PID {} and targeting {}:{}. Stop that app process, then run `jig proxy prune`, or change one of the duplicate route hostnames before retrying.",
            existing.hostname,
            owner,
            existing.target_host,
            existing.target_port
        );
    }
    Ok(())
}

fn live_process_route_for_hostname<'a>(routes: &'a [Route], hostname: &str) -> Option<&'a Route> {
    routes.iter().find(|existing| {
        existing.hostname.as_str() == hostname
            && existing.mode == RouteMode::Process
            && route_is_alive(existing)
    })
}

fn write_routes_to_path(path: &Path, routes: &[Route]) -> Result<()> {
    let tmp = file_ops::temp_path(path, "jig-proxy-state");
    let mut file = file_ops::create_new_file(&tmp, 0o600)?;
    serde_json::to_writer_pretty(
        &mut file,
        &RoutesDocument {
            version: ROUTES_VERSION,
            routes,
        },
    )?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    drop(file);
    file_ops::replace_file(&tmp, path, "jig-proxy-state")?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RoutesFile {
    Versioned(RoutesDocumentOwned),
    Legacy(Vec<Route>),
}

#[derive(Deserialize)]
struct RoutesDocumentOwned {
    version: u32,
    routes: Vec<Route>,
}

#[derive(Serialize)]
struct RoutesDocument<'a> {
    version: u32,
    routes: &'a [Route],
}

fn read_port_file(path: &Path) -> Option<u16> {
    file_ops::read_text_no_follow(path)
        .ok()
        .flatten()?
        .trim()
        .parse()
        .ok()
}

fn read_health_token_file(path: &Path) -> Result<Option<String>> {
    let Some(text) = read_private_text_no_follow(path)? else {
        return Ok(None);
    };
    let token = text.trim();
    if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(Some(token.to_string()))
    } else {
        eprintln!(
            "jig proxy ignored corrupt health token file {}; a new token will be written when the proxy starts",
            path.display()
        );
        Ok(None)
    }
}

fn read_private_text_no_follow(path: &Path) -> std::io::Result<Option<String>> {
    let mut file = match file_ops::open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o7777;
        if mode != 0o600 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "private state file {} must have mode 600, found {:o}",
                    path.display(),
                    mode
                ),
            ));
        }
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(Some(text))
}

fn random_health_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| {
        anyhow::anyhow!("Failed to generate proxy health token with getrandom: {error}")
    })?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut token, "{byte:02x}")?;
    }
    Ok(token)
}

fn open_lock_file(path: PathBuf) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(&path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "state lock is not a regular file",
        )
        .into());
    }
    #[cfg(unix)]
    {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn lock_state_file(file: &File, label: &str) -> Result<()> {
    let acquired = lock_state_file_interruptible(file, label, &|| false)?;
    debug_assert!(acquired, "an uncancelled state lock must be acquired");
    Ok(())
}

fn lock_state_file_interruptible(
    file: &File,
    label: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    let deadline = Instant::now() + STATE_LOCK_TIMEOUT;
    loop {
        if cancelled() {
            return Ok(false);
        }
        match file.try_lock_exclusive() {
            Ok(true) => return Ok(true),
            Ok(false) => {
                if Instant::now() >= deadline {
                    anyhow::bail!(
                        "Timed out waiting for Jig proxy {label} after {STATE_LOCK_TIMEOUT:?}"
                    );
                }
                std::thread::sleep(STATE_LOCK_POLL_INTERVAL);
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn lock_state_file_cleanup_cancelable(
    file: &File,
    label: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    lock_state_file_cleanup_cancelable_until(
        file,
        label,
        Instant::now() + STATE_LOCK_TIMEOUT,
        cancelled,
    )
}

fn lock_state_file_cleanup_cancelable_until(
    file: &File,
    label: &str,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    loop {
        match file.try_lock_exclusive() {
            Ok(true) => return Ok(true),
            Ok(false) => {
                // Cancellation may abandon an actual lock wait, but it must not
                // skip an immediately available cleanup mutation.
                if cancelled() {
                    return Ok(false);
                }
                let now = Instant::now();
                if now >= deadline {
                    anyhow::bail!(
                        "Timed out waiting for Jig proxy {label} before the shared cleanup deadline"
                    );
                }
                std::thread::sleep(std::cmp::min(
                    STATE_LOCK_POLL_INTERVAL,
                    deadline.saturating_duration_since(now),
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn ensure_state_dir_permissions(path: &Path, can_chmod: bool) -> Result<()> {
    #[cfg(unix)]
    {
        if can_chmod {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            return Ok(());
        }
        let mode = fs::metadata(path)?.permissions().mode() & 0o7777;
        if mode != 0o700 {
            anyhow::bail!(
                "Proxy state dir {} already exists with permissions {:o}. Use a dedicated directory with mode 700 or let Jig create it.",
                path.display(),
                mode
            );
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = can_chmod;
        harden_windows_state_dir(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, can_chmod);
        Ok(())
    }
}

#[cfg(windows)]
fn harden_windows_state_dir(path: &Path) -> Result<()> {
    let account = current_windows_account()?;
    let grant = format!("{account}:(OI)(CI)F");
    let icacls = crate::windows_system::native_system_executable("icacls.exe")
        .context("Failed to resolve the native Windows icacls executable")?;
    let output = Command::new(icacls)
        .arg(path)
        .args([
            "/inheritance:r",
            "/grant:r",
            &grant,
            "/remove:g",
            "*S-1-1-0",
            "*S-1-5-11",
            "*S-1-5-32-545",
            "/T",
        ])
        .output()
        .with_context(|| format!("Failed to run icacls for {}", path.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "Failed to apply owner-only ACL to proxy state dir {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn current_windows_account() -> Result<String> {
    let user = std::env::var("USERNAME").context("USERNAME is not set")?;
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    if domain.is_empty() {
        Ok(user)
    } else {
        Ok(format!("{domain}\\{user}"))
    }
}

fn ensure_default_state_parent_permissions(path: &Path, existed: bool) -> Result<()> {
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            if existed {
                let mode = fs::metadata(parent)?.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    anyhow::bail!(
                        "Default proxy state parent {} already exists with permissions {:o}. Tighten it to mode 700 before using the default state dir, or set JIG_PROXY_STATE_DIR to a dedicated private directory.",
                        parent.display(),
                        mode
                    );
                }
            } else {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (path, existed);
    Ok(())
}

fn ensure_state_create_ancestor_is_not_shared_writable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut ancestor = path;
        while !ancestor.try_exists()? {
            ancestor = ancestor
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
        }
        let metadata = fs::symlink_metadata(ancestor)?;
        let metadata = if metadata.file_type().is_symlink() {
            let parent = ancestor
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            ensure_existing_state_ancestor_is_not_shared_writable(path, parent)?;
            fs::metadata(ancestor)?
        } else {
            metadata
        };
        if !metadata.is_dir() {
            anyhow::bail!(
                "Proxy state dir {} would be created under non-directory ancestor {}.",
                path.display(),
                ancestor.display()
            );
        }
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o022 != 0 {
            anyhow::bail!(
                "Proxy state dir {} would be created under shared-writable ancestor {} with permissions {:o}. Use a dedicated private directory.",
                path.display(),
                ancestor.display(),
                mode
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(unix)]
fn ensure_existing_state_ancestor_is_not_shared_writable(
    path: &Path,
    ancestor: &Path,
) -> Result<()> {
    let metadata = fs::metadata(ancestor)?;
    if !metadata.is_dir() {
        anyhow::bail!(
            "Proxy state dir {} would be created under non-directory ancestor {}.",
            path.display(),
            ancestor.display()
        );
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o022 != 0 {
        anyhow::bail!(
            "Proxy state dir {} would be created under shared-writable ancestor {} with permissions {:o}. Use a dedicated private directory.",
            path.display(),
            ancestor.display(),
            mode
        );
    }
    Ok(())
}

fn recover_replace_backups(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some((original_name, backup_pid)) = file_ops::replace_backup_parts(file_name) else {
            continue;
        };
        let original = root.join(original_name);
        if original.exists() {
            if replace_backup_is_stale(&entry.path()) {
                let _ = fs::remove_file(entry.path());
            }
        } else if replace_backup_can_be_promoted(&entry.path(), backup_pid) {
            match fs::rename(entry.path(), original) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

fn recover_replace_backups_with_lock_interruptible(
    root: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    let lock = open_lock_file(root.join(LOCK_FILE))?;
    if !lock_state_file_interruptible(&lock, "route recovery lock", cancelled)? {
        return Ok(false);
    }
    let lock = LockedFile::new(lock, "route recovery lock");
    if cancelled() {
        lock.unlock()?;
        return Ok(false);
    }
    let result = recover_replace_backups(root);
    let unlock_result = lock.unlock();
    finish_with_unlock("route recovery lock", result, unlock_result).map(|()| true)
}

fn replace_backup_can_be_promoted(path: &Path, backup_pid: &str) -> bool {
    if !process_start_tokens_supported() {
        return false;
    }
    let stale = replace_backup_is_stale(path);
    // A fresh backup from this process can only be from the currently executing
    // recovery/write path. Older same-pid backups are treated as stale PID reuse.
    if backup_pid == std::process::id().to_string() && !stale {
        return false;
    }
    stale
        || backup_pid
            .parse()
            .ok()
            .is_some_and(|pid| !pid_is_alive(pid))
}

fn replace_backup_is_stale(path: &Path) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    match modified.elapsed() {
        Ok(age) => age >= REPLACE_BACKUP_RECOVERY_DELAY,
        Err(_) => true,
    }
}

fn existing_dir_is_empty(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

fn path_is_symlink(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_symlink()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn proxy_exe_is_usable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests;
