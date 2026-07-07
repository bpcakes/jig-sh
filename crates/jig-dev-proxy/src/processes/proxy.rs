use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt;

use crate::ports::{is_any_jig_proxy_http, is_jig_proxy_http, is_port_free, is_tcp_listening};
use crate::state::StateStore;
use crate::types::ProxySettings;

use super::cleanup::{terminate_child, wait_after_terminate};

pub(super) const MAX_PROXY_LOG_BYTES: u64 = 2 * 1024 * 1024;
const PROXY_START_TIMEOUT: Duration = Duration::from_secs(10);
const PROXY_START_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const PROXY_HEALTH_MISSES_BEFORE_STOP: u8 = 3;
const DEFAULT_HTTPS_PORT: u16 = 1443;

pub(crate) fn ensure_proxy_running(settings: &ProxySettings, current_exe: &Path) -> Result<()> {
    let store = StateStore::resolve(settings.state_dir.clone())?;
    if proxy_ready(&store, settings)? {
        ensure_requested_https(&store, settings)?;
        return Ok(());
    }

    with_proxy_start_lock(&store, || {
        ensure_proxy_running_after_lock(&store, settings, current_exe)
    })
}

fn ensure_proxy_running_after_lock(
    store: &StateStore,
    settings: &ProxySettings,
    current_exe: &Path,
) -> Result<()> {
    if proxy_ready(store, settings)? {
        ensure_requested_https(store, settings)?;
        return Ok(());
    }
    ensure_no_unregistered_proxy_on_requested_port(store, settings)?;

    let log = open_proxy_log(store)?;
    let log2 = log.try_clone()?;
    let mut command = Command::new(current_exe);
    // Keep the background proxy environment small and explicit. State is
    // passed directly and via JIG_PROXY_STATE_DIR so the child does not need HOME.
    command
        .env_clear()
        .arg("proxy")
        .arg("start")
        .arg("--foreground")
        .arg("--state-dir")
        .arg(store.root())
        .arg("--http-port")
        .arg(settings.http_port.to_string())
        .arg("--tld")
        .arg(&settings.tld)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log2));
    preserve_proxy_child_env(&mut command);
    command.env("JIG_PROXY_STATE_DIR", store.root());
    if settings.https {
        command.arg("--https");
        if let Some(port) = settings.https_port {
            command.arg("--https-port").arg(port.to_string());
        }
    }
    if !settings.http2 {
        command.arg("--no-http2");
    }
    if settings.lan {
        command.arg("--lan");
    }
    detach_background_proxy(&mut command);
    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to spawn proxy from {}", current_exe.display()))?;

    let deadline = Instant::now() + PROXY_START_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            bail!("Proxy process exited before listening with status {status}");
        }
        if proxy_ready(store, settings)? {
            // The foreground command only supervises startup. After the proxy
            // has published its own PID and ports, lifecycle checks go through
            // the state files and health endpoint. The proxy was
            // session-detached at spawn, so normal orphan reparenting is the
            // intended long-running background behavior.
            drop(child);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    terminate_child(&mut child);
    // Keep the startup timeout as the primary error. Waiting here only reaps
    // the child after terminate_child has performed best-effort cleanup.
    wait_after_terminate(&mut child);
    bail!(
        "Timed out waiting for Jig proxy to listen. Logs: {}. Likely fix: inspect the proxy log for bind or certificate errors, stop any process using the requested proxy port, or run `scripts/jig proxy cert generate --force` for HTTPS certificate issues.",
        store.log_path().display()
    )
}

struct ProxyStartLock {
    file: File,
    unlocked: bool,
}

impl ProxyStartLock {
    fn lock(store: &StateStore) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .private_mode()
            .open(store.root().join("proxy-start.lock"))?;
        #[cfg(unix)]
        {
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            let mode = file.metadata()?.permissions().mode() & 0o777;
            if mode != 0o600 {
                bail!("proxy start lock permissions are {:o}; expected 600", mode);
            }
        }
        lock_proxy_start_file(&file)?;
        Ok(Self {
            file,
            unlocked: false,
        })
    }

    fn unlock(mut self) -> std::io::Result<()> {
        let result = FileExt::unlock(&self.file);
        if result.is_ok() {
            self.unlocked = true;
        }
        result
    }
}

impl Drop for ProxyStartLock {
    fn drop(&mut self) {
        if !self.unlocked {
            if let Err(error) = FileExt::unlock(&self.file) {
                eprintln!(
                    "jig proxy failed to unlock proxy start lock while dropping guard: {error}"
                );
            }
        }
    }
}

fn with_proxy_start_lock<T>(store: &StateStore, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock = ProxyStartLock::lock(store)?;
    let result = f();
    let unlock_result = lock.unlock();
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(unlock_error)) => {
            eprintln!(
                "jig proxy failed to unlock proxy start lock after an earlier error: {unlock_error}"
            );
            Err(error)
        }
    }
}

pub(super) fn open_proxy_log(store: &StateStore) -> Result<File> {
    let path = store.log_path();
    if path.exists() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            bail!(
                "Refusing to open proxy log {} because it is a symlink",
                path.display()
            );
        }
        #[cfg(unix)]
        if metadata.nlink() != 1 {
            bail!(
                "Refusing to open proxy log {} because it has {} hardlinks",
                path.display(),
                metadata.nlink()
            );
        }
        if path.metadata()?.len() > MAX_PROXY_LOG_BYTES {
            // The proxy-start lock serializes background starters before this
            // rotation path, so at most one launcher renames proxy.log.
            // Keep one bounded backup. The previous backup is intentionally
            // removed before rename so repeated restarts cannot grow the set of
            // retained proxy logs.
            let rotated = path.with_file_name("proxy.log.1");
            match fs::remove_file(&rotated) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to remove rotated proxy log {}", rotated.display())
                    });
                }
            }
            // If rename fails after the stale rotated file is removed, the
            // active log remains in place and the contextual error tells the
            // user which paths need inspection.
            fs::rename(&path, &rotated).with_context(|| {
                format!(
                    "Failed to rotate proxy log {} to {}",
                    path.display(),
                    rotated.display()
                )
            })?;
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .private_mode()
        .open(&path)?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

fn lock_proxy_start_file(file: &File) -> Result<()> {
    let deadline = Instant::now() + PROXY_START_LOCK_TIMEOUT;
    loop {
        match file.try_lock_exclusive() {
            Ok(true) => return Ok(()),
            Ok(false) => {
                if Instant::now() >= deadline {
                    bail!(
                        "Timed out waiting for proxy start lock after {:?}",
                        PROXY_START_LOCK_TIMEOUT
                    );
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

trait PrivateOpenOptions {
    fn private_mode(&mut self) -> &mut Self;
}

impl PrivateOpenOptions for OpenOptions {
    fn private_mode(&mut self) -> &mut Self {
        #[cfg(unix)]
        {
            self.mode(0o600).custom_flags(libc::O_NOFOLLOW)
        }
        #[cfg(not(unix))]
        {
            self
        }
    }
}

#[cfg(unix)]
fn detach_background_proxy(command: &mut Command) {
    command.current_dir("/");
    unsafe {
        // SAFETY: pre_exec runs in the child after fork and before exec. The
        // closure only calls async-signal-safe libc functions and reads errno
        // for the setsid return value.
        command.pre_exec(|| {
            // `setsid` detaches the proxy from the caller's process group. It
            // intentionally does not double-fork; users that need login-session
            // independent lifetime should install the user service.
            libc::umask(0o077);
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(windows)]
fn detach_background_proxy(command: &mut Command) {
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let system_root = std::env::var_os("SystemRoot")
        .filter(|value| Path::new(value).is_absolute())
        .unwrap_or_else(|| "C:\\".into());
    command.current_dir(system_root);
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach_background_proxy(_command: &mut Command) {}

pub(super) fn proxy_ready(store: &StateStore, settings: &ProxySettings) -> Result<bool> {
    let Some(http_port) = store.read_http_port()? else {
        return Ok(false);
    };
    let Some(health_token) = store.read_health_token()? else {
        return Ok(false);
    };
    let Some(health_pid) =
        crate::ports::jig_proxy_http_pid("127.0.0.1", http_port, Some(&health_token))
    else {
        return Ok(false);
    };
    if store.read_pid()? != Some(health_pid) {
        return Ok(false);
    }
    ensure_requested_http_port(store, settings, http_port)?;
    ensure_requested_https(store, settings)?;
    Ok(true)
}

fn ensure_requested_http_port(
    store: &StateStore,
    settings: &ProxySettings,
    actual_port: u16,
) -> Result<()> {
    if settings.http_port == 0 || settings.http_port == actual_port {
        return Ok(());
    }
    bail!(
        "A Jig proxy is already running in state dir {} on HTTP port {}, but this command requested HTTP port {}. Likely fix: run `scripts/jig proxy stop && scripts/jig proxy start --http-port {}` with the same JIG_PROXY_STATE_DIR, or retry with --http-port {}.",
        store.root().display(),
        actual_port,
        settings.http_port,
        settings.http_port,
        actual_port
    )
}

pub(super) fn ensure_requested_https(store: &StateStore, settings: &ProxySettings) -> Result<()> {
    if !settings.https {
        return Ok(());
    }
    let requested_port = requested_https_port(settings);
    let Some(actual_port) = store.read_https_port()? else {
        // Runtime files are replaced under one runtime lock after every
        // requested listener has bound. If an authenticated proxy has published
        // HTTP state but no HTTPS port, it is a live HTTP-only proxy rather than
        // a transient startup snapshot.
        bail!(
            "A Jig proxy is already running without the requested HTTPS listener in state dir {}. Likely fix: run `scripts/jig proxy stop && scripts/jig proxy start --https --https-port {}` with the same JIG_PROXY_STATE_DIR, then retry the dev command.",
            store.root().display(),
            requested_port
        )
    };
    if actual_port != requested_port {
        bail!(
            "A Jig proxy is already running in state dir {} on HTTPS port {}, but this command requested HTTPS port {}. Likely fix: run `scripts/jig proxy stop && scripts/jig proxy start --https --https-port {}` with the same JIG_PROXY_STATE_DIR, or retry with --https-port {}.",
            store.root().display(),
            actual_port,
            requested_port,
            requested_port,
            actual_port
        )
    }
    if !is_tcp_listening("127.0.0.1", actual_port) {
        bail!(
            "A Jig proxy is already running without the requested HTTPS listener in state dir {}. Likely fix: run `scripts/jig proxy stop && scripts/jig proxy start --https --https-port {}` with the same JIG_PROXY_STATE_DIR, then retry the dev command.",
            store.root().display(),
            requested_port
        )
    }
    Ok(())
}

fn requested_https_port(settings: &ProxySettings) -> u16 {
    settings.https_port.unwrap_or(DEFAULT_HTTPS_PORT)
}

fn ensure_no_unregistered_proxy_on_requested_port(
    store: &StateStore,
    settings: &ProxySettings,
) -> Result<()> {
    let health_token = store.read_health_token()?;
    if settings.http_port != 0 && !is_port_free("127.0.0.1", settings.http_port) {
        if health_token
            .as_deref()
            .is_some_and(|token| is_jig_proxy_http("127.0.0.1", settings.http_port, Some(token)))
        {
            bail!(
                "A Jig proxy is already running on HTTP port {} but it is not registered in state dir {}. Likely fix: use the same JIG_PROXY_STATE_DIR as the running proxy, choose a different --http-port, or stop the other proxy first.",
                settings.http_port,
                store.root().display()
            );
        }
        // The unauthenticated probe is only a collision detector. It never
        // trusts the returned PID or mutates state; authenticated health-token
        // checks above are required for identity-sensitive operations.
        if is_any_jig_proxy_http("127.0.0.1", settings.http_port) {
            bail!(
                "A Jig proxy is already running on HTTP port {} but this state dir {} cannot authenticate to it. Likely fix: use the same JIG_PROXY_STATE_DIR as the running proxy, choose a different --http-port, or stop the other proxy first.",
                settings.http_port,
                store.root().display()
            );
        }
        bail!(
            "HTTP port {} is already in use. Likely fix: choose a different --http-port or stop the process currently using that port.",
            settings.http_port
        );
    }
    Ok(())
}

pub(super) fn proxy_health_failed(misses: &mut u8, ready: bool) -> bool {
    if ready {
        *misses = 0;
        return false;
    }
    *misses = misses.saturating_add(1);
    *misses >= PROXY_HEALTH_MISSES_BEFORE_STOP
}

fn preserve_proxy_child_env(command: &mut Command) {
    for key in [
        "TMPDIR",
        "TEMP",
        "TMP",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "LC_ALL",
        "LC_CTYPE",
        "LANG",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}
