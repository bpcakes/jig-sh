//! One-time 1Password dotenv import helpers.

use std::io::{ErrorKind, Read};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use jig_vault::{FieldKind, FieldMutation, MAX_SECRET_VALUE_LEN, SecretBytes};
use zeroize::Zeroizing;

use crate::command::{VaultImportEnvironment, VaultImportValueSource};

const MAX_OP_STDERR_LEN: usize = 64 * 1024;
const PASSPHRASE_ENV: &str = "JIG_VAULT_PASSPHRASE";
const NEW_PASSPHRASE_ENV: &str = "JIG_VAULT_NEW_PASSPHRASE";
const MAX_IMPORT_TOTAL_VALUE_LEN: usize = 16 * 1024 * 1024;
const OP_READ_TIMEOUT: Duration = Duration::from_secs(30);
const OP_FINAL_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const OP_POLL_INTERVAL: Duration = Duration::from_millis(5);
const MAX_READS_PER_POLL: usize = 64;
const OP_READ_CHUNK_LEN: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImportEntry {
    pub(crate) name: String,
    pub(crate) reference: jig_vault::VaultReference,
    pub(crate) kind: FieldKind,
}

pub(crate) struct ResolvedImport {
    pub(crate) entries: Vec<ImportEntry>,
    pub(crate) mutations: Vec<FieldMutation>,
    pub(crate) destination: SecretBytes,
}

impl std::fmt::Debug for ResolvedImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedImport")
            .field("entries", &self.entries)
            .field("mutation_count", &self.mutations.len())
            .field("mutations", &"[REDACTED]")
            .field("destination_len", &self.destination.len())
            .field("destination", &"[REDACTED]")
            .finish()
    }
}

pub(crate) fn import_entries(environment: &VaultImportEnvironment) -> Vec<ImportEntry> {
    environment
        .assignments
        .iter()
        .map(|assignment| ImportEntry {
            name: assignment.name.clone(),
            reference: assignment.reference.clone(),
            kind: match assignment.source {
                VaultImportValueSource::Literal(_) => FieldKind::Text,
                VaultImportValueSource::OnePassword(_) => FieldKind::Concealed,
            },
        })
        .collect()
}

pub(crate) fn resolve_import(environment: VaultImportEnvironment) -> Result<ResolvedImport> {
    let entries = import_entries(&environment);
    let destination = destination_bytes(&entries)?;
    let mut mutations = Vec::with_capacity(environment.assignments.len());
    let mut total_value_len = 0_usize;
    for assignment in environment.assignments {
        let (kind, value) = match assignment.source {
            VaultImportValueSource::Literal(value) => (FieldKind::Text, value),
            VaultImportValueSource::OnePassword(reference) => (
                FieldKind::Concealed,
                resolve_onepassword_value(&assignment.name, reference)?,
            ),
        };
        total_value_len = total_value_len
            .checked_add(value.len())
            .ok_or_else(|| anyhow!("vault import decoded value total exceeds supported bounds"))?;
        if total_value_len > MAX_IMPORT_TOTAL_VALUE_LEN {
            bail!(
                "vault import decoded values exceed the {MAX_IMPORT_TOTAL_VALUE_LEN} byte total limit"
            );
        }
        mutations.push(FieldMutation::set(assignment.reference, kind, value));
    }
    Ok(ResolvedImport {
        entries,
        mutations,
        destination,
    })
}

pub(crate) fn preflight_destination(path: &Path) -> Result<bool> {
    if path == Path::new("-") {
        bail!("vault import rejects --out-env -; choose an atomic private file destination");
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("vault import destination must not be a symbolic link")
        }
        Ok(metadata) if !metadata.is_file() => {
            bail!("vault import destination exists and is not a regular file")
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to inspect vault import destination {}",
                path.display()
            )
        }),
    }
}

fn destination_bytes(entries: &[ImportEntry]) -> Result<SecretBytes> {
    let total_len = entries.iter().try_fold(0_usize, |total, entry| {
        total
            .checked_add(entry.name.len())
            .and_then(|value| value.checked_add(1))
            .and_then(|value| value.checked_add(entry.reference.to_string().len()))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| anyhow!("vault import destination length exceeds supported bounds"))
    })?;
    if total_len > super::vault_env::MAX_VAULT_ENV_FILE_LEN {
        bail!(
            "vault import destination exceeds the {} byte dotenv limit",
            super::vault_env::MAX_VAULT_ENV_FILE_LEN
        );
    }
    let mut bytes = Vec::with_capacity(total_len);
    for entry in entries {
        bytes.extend_from_slice(entry.name.as_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(entry.reference.to_string().as_bytes());
        bytes.push(b'\n');
    }
    Ok(SecretBytes::new(bytes))
}

fn resolve_onepassword_value(variable: &str, reference: SecretBytes) -> Result<SecretBytes> {
    let reference_text = std::str::from_utf8(reference.as_slice())
        .expect("the restricted dotenv parser validated UTF-8");
    let mut command = Command::new("op");
    command
        .args(["read", "--no-newline"])
        .arg(reference_text)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove(PASSPHRASE_ENV)
        .env_remove(NEW_PASSPHRASE_ENV);
    isolate_op_process(&mut command);
    let mut child = command.spawn().map_err(|error| {
        anyhow!(
            "failed to start 1Password CLI for variable '{}' ({:?}); ensure `op` is installed and authenticated",
            variable,
            error.kind()
        )
    })?;
    let process_id = child.id();
    drop(command);

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_and_reap(&mut child, process_id);
            bail!("1Password CLI stdout capture for variable '{variable}' was unavailable");
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            drop(stdout);
            terminate_and_reap(&mut child, process_id);
            bail!("1Password CLI stderr capture for variable '{variable}' was unavailable");
        }
    };
    let mut stdout_pump = match CapturePump::new(OpPipe::Stdout(stdout), MAX_SECRET_VALUE_LEN) {
        Ok(pump) => pump,
        Err(error) => {
            drop(stderr);
            terminate_and_reap(&mut child, process_id);
            bail!(
                "failed to prepare bounded 1Password stdout capture for variable '{}' ({:?})",
                variable,
                error.kind()
            );
        }
    };
    let mut stderr_pump = match CapturePump::new(OpPipe::Stderr(stderr), MAX_OP_STDERR_LEN) {
        Ok(pump) => pump,
        Err(error) => {
            stdout_pump.abandon();
            terminate_and_reap(&mut child, process_id);
            bail!(
                "failed to prepare bounded 1Password stderr capture for variable '{}' ({:?})",
                variable,
                error.kind()
            );
        }
    };

    let wait_outcome =
        wait_child_bounded(&mut child, process_id, &mut stdout_pump, &mut stderr_pump);
    finish_pipe_drain(process_id, &mut stdout_pump, &mut stderr_pump);

    if stdout_pump.overflowed {
        bail!(
            "1Password value for variable '{}' exceeds the {MAX_SECRET_VALUE_LEN} byte limit",
            variable
        );
    }
    if stderr_pump.overflowed {
        bail!(
            "1Password CLI diagnostic for variable '{}' exceeded the {MAX_OP_STDERR_LEN} byte safety limit; diagnostic text was suppressed",
            variable
        );
    }
    if let Some(kind) = stdout_pump.failure {
        bail!(
            "failed to read bounded 1Password stdout for variable '{}' ({kind:?}); captured bytes were discarded",
            variable
        );
    }
    if let Some(kind) = stderr_pump.failure {
        bail!(
            "failed to read bounded 1Password stderr for variable '{}' ({kind:?}); captured bytes were discarded",
            variable
        );
    }
    let status = match wait_outcome {
        OpWaitOutcome::Exited(status) => status,
        OpWaitOutcome::TimedOut => {
            bail!(
                "1Password CLI for variable '{variable}' exceeded the {OP_READ_TIMEOUT:?} resolution deadline and was terminated"
            )
        }
        OpWaitOutcome::ObservationFailed(kind) => {
            bail!("failed to observe 1Password CLI for variable '{variable}' ({kind:?})")
        }
        OpWaitOutcome::OutputOverflow => {
            bail!(
                "1Password CLI output for variable '{variable}' exceeded a bounded safety limit and was terminated"
            )
        }
        OpWaitOutcome::CaptureFailed => {
            bail!(
                "failed to capture bounded 1Password output for variable '{variable}'; captured bytes were discarded"
            )
        }
    };
    if !status.success() {
        bail!(
            "1Password CLI failed for variable '{}' ({}); diagnostic text was suppressed",
            variable,
            status_label(status)
        );
    }
    drop(stderr_pump.bytes);
    if std::str::from_utf8(stdout_pump.bytes.as_slice()).is_err() {
        bail!(
            "1Password value for variable '{}' is not valid UTF-8",
            variable
        );
    }
    if stdout_pump.bytes.as_slice().contains(&0) {
        bail!("1Password value for variable '{}' contains NUL", variable);
    }
    Ok(stdout_pump.bytes)
}

enum OpPipe {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

impl OpPipe {
    #[cfg(unix)]
    fn prepare(&self) -> std::io::Result<()> {
        let descriptor = match self {
            Self::Stdout(reader) => reader.as_raw_fd(),
            Self::Stderr(reader) => reader.as_raw_fd(),
        };
        // SAFETY: the descriptor is owned by this live pipe. F_GETFL only
        // reads its flags, and F_SETFL preserves them while adding O_NONBLOCK.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags == -1 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(windows)]
    fn prepare(&self) -> std::io::Result<()> {
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn prepare(&self) -> std::io::Result<()> {
        Err(std::io::Error::new(
            ErrorKind::Unsupported,
            "bounded 1Password pipe capture is unsupported on this platform",
        ))
    }

    #[cfg(unix)]
    fn read_available(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Stdout(reader) => reader.read(buffer),
            Self::Stderr(reader) => reader.read(buffer),
        }
    }

    #[cfg(windows)]
    fn read_available(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, HANDLE};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;

        let handle = match self {
            Self::Stdout(reader) => reader.as_raw_handle(),
            Self::Stderr(reader) => reader.as_raw_handle(),
        } as HANDLE;
        let mut available = 0_u32;
        // SAFETY: handle is a live anonymous-pipe read handle and `available`
        // is writable for the duration of this call.
        let peeked = unsafe {
            PeekNamedPipe(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(code) if code == ERROR_BROKEN_PIPE as i32 || code == ERROR_NO_DATA as i32
            ) {
                return Ok(0);
            }
            return Err(error);
        }
        if available == 0 {
            return Err(std::io::Error::from(ErrorKind::WouldBlock));
        }
        let read_limit = buffer.len().min(available as usize);
        match self {
            Self::Stdout(reader) => reader.read(&mut buffer[..read_limit]),
            Self::Stderr(reader) => reader.read(&mut buffer[..read_limit]),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn read_available(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            ErrorKind::Unsupported,
            "bounded 1Password pipe capture is unsupported on this platform",
        ))
    }
}

struct CapturePump {
    reader: Option<OpPipe>,
    bytes: SecretBytes,
    cap: usize,
    overflowed: bool,
    failure: Option<ErrorKind>,
}

impl CapturePump {
    fn new(reader: OpPipe, cap: usize) -> std::io::Result<Self> {
        reader.prepare()?;
        Ok(Self {
            reader: Some(reader),
            bytes: SecretBytes::with_capacity(cap),
            cap,
            overflowed: false,
            failure: None,
        })
    }

    fn poll(&mut self) {
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        let mut buffer = Zeroizing::new([0_u8; OP_READ_CHUNK_LEN]);
        for _ in 0..MAX_READS_PER_POLL {
            match reader.read_available(&mut buffer[..]) {
                Ok(0) => {
                    self.reader = None;
                    return;
                }
                Ok(read) => {
                    let remaining = self.cap - self.bytes.len();
                    let retained = remaining.min(read);
                    if retained > 0 {
                        self.bytes
                            .extend_from_slice(&buffer[..retained])
                            .expect("bounded 1Password capture was preallocated to its exact cap");
                    }
                    if retained != read {
                        self.overflowed = true;
                        self.reader = None;
                        return;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => return,
                Err(error) => {
                    self.failure = Some(error.kind());
                    self.reader = None;
                    return;
                }
            }
        }
    }

    fn is_terminal(&self) -> bool {
        self.reader.is_none()
    }

    fn abandon(&mut self) {
        self.reader = None;
    }
}

fn wait_child_bounded(
    child: &mut Child,
    process_id: u32,
    stdout: &mut CapturePump,
    stderr: &mut CapturePump,
) -> OpWaitOutcome {
    let Some(deadline) = Instant::now().checked_add(OP_READ_TIMEOUT) else {
        terminate_and_reap(child, process_id);
        return OpWaitOutcome::TimedOut;
    };
    loop {
        stdout.poll();
        stderr.poll();
        if stdout.overflowed || stderr.overflowed {
            terminate_and_reap(child, process_id);
            return OpWaitOutcome::OutputOverflow;
        }
        if stdout.failure.is_some() || stderr.failure.is_some() {
            terminate_and_reap(child, process_id);
            return OpWaitOutcome::CaptureFailed;
        }
        match child.try_wait() {
            Ok(Some(status)) => return OpWaitOutcome::Exited(status),
            Ok(None) => {}
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                terminate_and_reap(child, process_id);
                return OpWaitOutcome::ObservationFailed(error.kind());
            }
        }
        if Instant::now() >= deadline {
            terminate_and_reap(child, process_id);
            return OpWaitOutcome::TimedOut;
        }
        std::thread::sleep(OP_POLL_INTERVAL);
    }
}

fn finish_pipe_drain(process_id: u32, stdout: &mut CapturePump, stderr: &mut CapturePump) {
    let deadline = Instant::now()
        .checked_add(OP_FINAL_DRAIN_TIMEOUT)
        .unwrap_or_else(Instant::now);
    while !(stdout.is_terminal() && stderr.is_terminal()) && Instant::now() < deadline {
        stdout.poll();
        stderr.poll();
        if !(stdout.is_terminal() && stderr.is_terminal()) {
            std::thread::sleep(OP_POLL_INTERVAL);
        }
    }
    // The direct `op` leader is already reaped (or was terminated), but a
    // background descendant may have closed every inherited pipe and remain
    // otherwise invisible. Always end the fresh Unix process group after the
    // bounded drain; this is harmless when the group is already empty.
    terminate_process_group(process_id);
    // No worker owns another reader copy. Dropping any pipe still held by an
    // escaped writer is immediate and cannot block this import operation.
    stdout.abandon();
    stderr.abandon();
}

#[cfg(unix)]
fn isolate_op_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(windows)]
fn isolate_op_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

    // M4 private destination preflight rejects the import before resolution on
    // Windows, so this runner is currently unreachable there. Keep direct
    // process creation explicit without claiming Job Object containment; any
    // future Windows private-file support must add that boundary first.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn isolate_op_process(_command: &mut Command) {}

fn terminate_and_reap(child: &mut Child, process_id: u32) {
    terminate_process_group(process_id);
    let _ = child.kill();
    loop {
        match child.wait() {
            Ok(_) => return,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }
}

#[cfg(unix)]
fn terminate_process_group(process_id: u32) {
    let Ok(process_group) = libc::pid_t::try_from(process_id) else {
        return;
    };
    // SAFETY: `op` is spawned as leader of a fresh process group whose id is
    // its pinned child pid. A negative id targets only that owned group.
    let _ = unsafe { libc::kill(-process_group, libc::SIGKILL) };
}

#[cfg(not(unix))]
fn terminate_process_group(_process_id: u32) {}

enum OpWaitOutcome {
    Exited(ExitStatus),
    OutputOverflow,
    CaptureFailed,
    TimedOut,
    ObservationFailed(ErrorKind),
}

fn status_label(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal {signal}");
        }
    }
    "unknown process status".to_owned()
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use crate::test_env::{EnvVarGuard, lock_env};

    use super::*;

    #[test]
    fn op_spawn_strips_both_reserved_passphrase_variables() {
        let _env = lock_env();
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let op = bin.join("op");
        std::fs::write(
            &op,
            r#"#!/bin/sh
set -eu
if [ "${JIG_VAULT_PASSPHRASE+set}" = set ] || [ "${JIG_VAULT_NEW_PASSPHRASE+set}" = set ]; then
  exit 87
fi
printf '%s' 'resolved-with-clean-environment'
"#,
        )
        .unwrap();
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut path_parts = vec![bin];
        if let Some(path) = std::env::var_os("PATH") {
            path_parts.extend(std::env::split_paths(&path));
        }
        let _path = EnvVarGuard::set("PATH", std::env::join_paths(path_parts).unwrap());
        let _current = EnvVarGuard::set(PASSPHRASE_ENV, "current-must-not-reach-op");
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "new-must-not-reach-op");

        let resolved =
            resolve_onepassword_value("TOKEN", SecretBytes::new(b"op://Test/Login/TOKEN".to_vec()))
                .unwrap();

        assert_eq!(resolved.as_slice(), b"resolved-with-clean-environment");
        assert_eq!(
            std::env::var(PASSPHRASE_ENV).as_deref(),
            Ok("current-must-not-reach-op")
        );
        assert_eq!(
            std::env::var(NEW_PASSPHRASE_ENV).as_deref(),
            Ok("new-must-not-reach-op")
        );
    }
}
