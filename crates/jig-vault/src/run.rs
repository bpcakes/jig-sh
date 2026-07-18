use std::ffi::OsString;
#[cfg(unix)]
use std::fs::{File, Permissions};
use std::io::{self, Read};
#[cfg(unix)]
use std::io::{Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::process::ChildStderr;
use std::process::ChildStdout;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::SecretBytes;
use crate::env_policy::is_preserved_env_var_name;
use crate::redact::Redactor;
use crate::types::{EnvVarName, SecretName};

// Keep this cap aligned with redaction cost: redaction scans the captured text
// once per raw/encoded secret needle.
pub const MAX_CAPTURED_STREAM_BYTES: usize = 1024 * 1024;
const BROKERED_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const BROKERED_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const BROKERED_PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const BROKERED_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_STREAM_READS_PER_POLL: usize = 16;

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredEnv {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredFile {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredRun {
    pub(crate) command: Vec<String>,
    pub(crate) env: Vec<ResolvedBrokeredEnv>,
    pub(crate) files: Vec<ResolvedBrokeredFile>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunOutput {
    pub exit_status: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_brokered(request: ResolvedBrokeredRun) -> AnyResult<RunOutput> {
    run_brokered_with_timeout(request, BROKERED_RUN_TIMEOUT)
}

fn run_brokered_with_timeout(
    request: ResolvedBrokeredRun,
    timeout: Duration,
) -> AnyResult<RunOutput> {
    // Keep this guard for direct crate callers; clap enforces it for the CLI.
    if request.command.is_empty() {
        bail!("vault run requires a command after --");
    }
    let redactor = Redactor::from_secret_slices(
        request
            .env
            .iter()
            .map(|mapping| mapping.value.as_slice())
            .chain(request.files.iter().map(|mapping| mapping.value.as_slice())),
    );
    let file_env = BrokeredSecretFiles::create(&request.files)?;
    let mut env_values = Vec::<(String, Zeroizing<String>)>::new();
    for mapping in request.env {
        let env_value = match mapping.value.into_zeroizing_string() {
            Ok(value) => value,
            Err(_value) => {
                bail!(
                    "vault secret '{}' cannot be injected as env var {} because it is not valid UTF-8",
                    mapping.secret_name.as_str(),
                    mapping.var.as_str()
                );
            }
        };
        env_values.push((mapping.var.as_str().to_string(), env_value));
    }

    let mut command = Command::new(&request.command[0]);
    command.args(&request.command[1..]).env_clear();
    preserve_minimal_environment(&mut command);
    for (name, value) in &env_values {
        // std::process::Command copies env values into OsString storage; keep
        // our source copy zeroized, but the std-owned copy is dropped normally.
        command.env(name, value.as_str());
    }
    if let Some(file_env) = &file_env {
        for (name, path) in file_env.env() {
            command.env(name, path);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let process = BrokeredProcess::spawn(&mut command)
        .with_context(|| format!("failed to run brokered command '{}'", request.command[0]))?;
    let (status, stdout, stderr) = wait_for_capped_output(process, &request.command[0], timeout)?;
    Ok(RunOutput {
        exit_status: status.exit_status,
        exit_signal: status.exit_signal,
        stdout: redactor.redact_bytes_lossy(stdout.as_slice()),
        stderr: redactor.redact_bytes_lossy(stderr.as_slice()),
    })
}

struct BrokeredSecretFiles {
    // Secret files intentionally live on disk while the child runs. TempDir
    // cleanup removes them on normal unwind; hard process kills can leave them
    // behind for OS temp cleanup. Drop runs before field drops, so keep `_dir`
    // before `files`: the explicit Drop wipe uses the retained file handles,
    // then TempDir removes the persisted paths during field drop.
    _dir: tempfile::TempDir,
    env: Vec<(String, OsString)>,
    #[cfg(unix)]
    files: Vec<(OsString, File)>,
}

#[cfg(unix)]
impl Drop for BrokeredSecretFiles {
    fn drop(&mut self) {
        for (path, file) in &mut self.files {
            wipe_secret_file_best_effort(file, std::path::Path::new(path));
        }
    }
}

impl BrokeredSecretFiles {
    fn create(files: &[ResolvedBrokeredFile]) -> AnyResult<Option<Self>> {
        if files.is_empty() {
            return Ok(None);
        }

        #[cfg(not(unix))]
        {
            bail!(
                "vault run --file mapping '{}={}' requires Unix-style owner-only temporary files; use --env on this platform",
                files[0].var.as_str(),
                files[0].secret_name.as_str()
            );
        }

        #[cfg(unix)]
        {
            let dir = tempfile::Builder::new()
                .prefix("jig-vault-run-")
                .permissions(Permissions::from_mode(0o700))
                .tempdir()
                .context("failed to create vault secret file temp dir")?;
            let mut env = Vec::with_capacity(files.len());
            let mut persisted_files = Vec::with_capacity(files.len());
            for mapping in files {
                // tempfile uses mkstemp on Unix and creates owner-only files;
                // keep the random path so the child can read it until TempDir cleanup.
                let mut secret_file = tempfile::Builder::new()
                    .prefix("secret-")
                    .tempfile_in(dir.path())
                    .with_context(|| {
                        format!(
                            "failed to create brokered temp file for vault secret '{}'",
                            mapping.secret_name.as_str()
                        )
                    })?;
                let path = secret_file.path().to_path_buf();
                write_secret_file(secret_file.as_file_mut(), &path, mapping.value.as_slice())
                    .with_context(|| {
                        format!(
                            "failed to write vault secret '{}' to a brokered temp file",
                            mapping.secret_name.as_str()
                        )
                    })?;
                // `keep` gives the child a stable path; the owning TempDir still
                // removes the persisted file tree when the brokered run ends.
                let (file, path) = secret_file.keep().with_context(|| {
                    format!(
                        "failed to persist brokered temp file for vault secret '{}'",
                        mapping.secret_name.as_str(),
                    )
                })?;
                let path = path.into_os_string();
                env.push((mapping.var.as_str().to_string(), path.clone()));
                persisted_files.push((path, file));
            }
            Ok(Some(Self {
                _dir: dir,
                env,
                files: persisted_files,
            }))
        }
    }

    fn env(&self) -> &[(String, OsString)] {
        &self.env
    }
}

#[cfg(unix)]
fn write_secret_file(file: &mut File, path: &std::path::Path, value: &[u8]) -> AnyResult<()> {
    file.write_all(value)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))
}

#[cfg(unix)]
fn wipe_secret_file_best_effort(file: &mut File, path: &std::path::Path) {
    if let Err(error) = wipe_secret_file(file, path) {
        eprintln!(
            "jig vault could not wipe brokered temp secret file {} before cleanup: {error:#}",
            path.display()
        );
    }
}

#[cfg(unix)]
fn wipe_secret_file(file: &mut File, path: &std::path::Path) -> AnyResult<()> {
    let len = file.seek(SeekFrom::End(0)).with_context(|| {
        format!(
            "failed to measure brokered temp secret file {}",
            path.display()
        )
    })?;
    file.rewind().with_context(|| {
        format!(
            "failed to seek brokered temp secret file {}",
            path.display()
        )
    })?;
    let zeros = [0_u8; 8192];
    let mut remaining = len;
    while remaining > 0 {
        let chunk_len = remaining.min(zeros.len() as u64) as usize;
        file.write_all(&zeros[..chunk_len]).with_context(|| {
            format!(
                "failed to wipe brokered temp secret file {}",
                path.display()
            )
        })?;
        remaining -= chunk_len as u64;
    }
    file.sync_all().with_context(|| {
        format!(
            "failed to sync wiped brokered temp secret file {}",
            path.display()
        )
    })
}

enum ProcessPipe {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

impl ProcessPipe {
    #[cfg(unix)]
    fn prepare(&self) -> io::Result<()> {
        let descriptor = match self {
            Self::Stdout(reader) => reader.as_raw_fd(),
            Self::Stderr(reader) => reader.as_raw_fd(),
        };
        // SAFETY: the descriptor is owned by this live pipe. F_GETFL only
        // inspects its current status flags.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the descriptor remains live and F_SETFL preserves all
        // existing flags while adding nonblocking reads.
        if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(windows)]
    fn prepare(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn prepare(&self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "nonblocking brokered process-pipe reads are unavailable on this platform",
        ))
    }

    #[cfg(unix)]
    fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(reader) => reader.read(buffer),
            Self::Stderr(reader) => reader.read(buffer),
        }
    }

    #[cfg(windows)]
    fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, HANDLE};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;

        let handle = match self {
            Self::Stdout(reader) => reader.as_raw_handle(),
            Self::Stderr(reader) => reader.as_raw_handle(),
        } as HANDLE;
        let mut available = 0_u32;
        // SAFETY: handle is a live anonymous-pipe read handle and the only
        // output pointer names a writable u32. This call copies no bytes.
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
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(code) if code == ERROR_BROKEN_PIPE as i32 || code == ERROR_NO_DATA as i32
            ) {
                return Ok(0);
            }
            return Err(error);
        }
        if available == 0 {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        let read_limit = buffer.len().min(available as usize);
        match self {
            Self::Stdout(reader) => reader.read(&mut buffer[..read_limit]),
            Self::Stderr(reader) => reader.read(&mut buffer[..read_limit]),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn read_available(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "nonblocking brokered process-pipe reads are unavailable on this platform",
        ))
    }
}

struct CappedOutputDrain {
    label: &'static str,
    reader: Option<ProcessPipe>,
    output: SecretBytes,
    complete: bool,
}

impl CappedOutputDrain {
    fn start(label: &'static str, reader: ProcessPipe) -> AnyResult<Self> {
        reader
            .prepare()
            .with_context(|| format!("failed to prepare brokered command {label} capture"))?;
        Ok(Self {
            label,
            reader: Some(reader),
            // Allocate the full cap up front so secret-bearing bytes never
            // pass through discarded intermediate Vec allocations.
            output: SecretBytes::with_capacity(MAX_CAPTURED_STREAM_BYTES),
            complete: false,
        })
    }

    fn poll(&mut self) -> AnyResult<()> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(());
        };
        let mut buffer = Zeroizing::new([0_u8; 8192]);
        for _ in 0..MAX_STREAM_READS_PER_POLL {
            match reader.read_available(&mut buffer[..]) {
                Ok(0) => {
                    self.complete = true;
                    self.reader = None;
                    return Ok(());
                }
                Ok(read) => {
                    let Some(new_len) = self.output.len().checked_add(read) else {
                        self.reader = None;
                        bail!("brokered command {} capture length overflowed", self.label);
                    };
                    if new_len > MAX_CAPTURED_STREAM_BYTES {
                        let remaining = MAX_CAPTURED_STREAM_BYTES - self.output.len();
                        self.output.extend_from_slice(&buffer[..remaining])?;
                        self.reader = None;
                        bail!(
                            "brokered command {} exceeded the {} byte capture limit",
                            self.label,
                            MAX_CAPTURED_STREAM_BYTES
                        );
                    }
                    self.output.extend_from_slice(&buffer[..read])?;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => {
                    self.reader = None;
                    return Err(error).with_context(|| {
                        format!("failed to read brokered command {}", self.label)
                    });
                }
            }
        }
        Ok(())
    }

    fn is_terminal(&self) -> bool {
        self.reader.is_none()
    }

    fn into_output(self) -> AnyResult<SecretBytes> {
        if !self.complete {
            bail!(
                "brokered command {} capture remained open after process cleanup",
                self.label
            );
        }
        Ok(self.output)
    }
}

struct CappedOutputDrains {
    stdout: CappedOutputDrain,
    stderr: CappedOutputDrain,
}

impl CappedOutputDrains {
    fn start(child: &mut Child) -> AnyResult<Self> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("brokered command stdout pipe was not captured"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("brokered command stderr pipe was not captured"))?;
        Ok(Self {
            stdout: CappedOutputDrain::start("stdout", ProcessPipe::Stdout(stdout))?,
            stderr: CappedOutputDrain::start("stderr", ProcessPipe::Stderr(stderr))?,
        })
    }

    fn poll(&mut self) -> AnyResult<()> {
        self.stdout.poll()?;
        self.stderr.poll()
    }

    fn is_terminal(&self) -> bool {
        self.stdout.is_terminal() && self.stderr.is_terminal()
    }

    fn finish(mut self, timeout: Duration) -> AnyResult<(SecretBytes, SecretBytes)> {
        let deadline = checked_deadline("brokered output drain", timeout)?;
        while !self.is_terminal() {
            self.poll()?;
            if self.is_terminal() {
                break;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                bail!("brokered command output drain exceeded its {timeout:?} deadline");
            };
            thread::sleep(remaining.min(Duration::from_millis(2)));
        }
        Ok((self.stdout.into_output()?, self.stderr.into_output()?))
    }
}

fn checked_deadline(label: &str, timeout: Duration) -> AnyResult<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow!("{label} deadline overflowed"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LeaderObservation {
    Running,
    Exited,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug)]
struct PinnedUnixProcessGroup {
    id: libc::pid_t,
}

struct BrokeredProcess {
    child: Child,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    process_group: Option<PinnedUnixProcessGroup>,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
    reaped_status: Option<ExitStatus>,
    cleanup_deadline: Option<Instant>,
    tree_cleanup_error: Option<String>,
    cleanup_complete: bool,
}

impl BrokeredProcess {
    fn spawn(command: &mut Command) -> io::Result<Self> {
        spawn_brokered_process(command)
    }

    fn observe_leader(&mut self) -> io::Result<LeaderObservation> {
        observe_brokered_leader(self)
    }

    fn terminate_and_reap(&mut self, timeout: Duration) -> AnyResult<ExitStatus> {
        if self.cleanup_complete {
            return self
                .reaped_status
                .ok_or_else(|| anyhow!("brokered process cleanup lost its exit status"));
        }
        let (deadline, first_attempt) =
            fixed_process_cleanup_deadline(&mut self.cleanup_deadline, timeout)?;
        if first_attempt {
            if let Err(error) = terminate_brokered_process_tree(self, deadline) {
                self.tree_cleanup_error = Some(format!("{error:#}"));
            }
        }
        // The owned tree signal has already been attempted while Unix still
        // has a pinned wait status. It is now safe to consume that status;
        // importantly, no later path may signal the numeric group again.
        let reap = self.reap_after_tree_signal(deadline, timeout);
        let status = match reap {
            Ok(status) => status,
            Err(reap_error) => {
                return match &self.tree_cleanup_error {
                    None => Err(reap_error),
                    Some(tree_error) => Err(anyhow!(
                        "failed to terminate brokered process tree: {tree_error}; process reap also failed: {reap_error:#}"
                    )),
                };
            }
        };
        if let Some(error) = &self.tree_cleanup_error {
            bail!("failed to terminate brokered process tree: {error}");
        }
        self.cleanup_complete = true;
        Ok(status)
    }

    fn reap_after_tree_signal(
        &mut self,
        deadline: Instant,
        timeout: Duration,
    ) -> AnyResult<ExitStatus> {
        if let Some(status) = self.reaped_status {
            return Ok(status);
        }
        loop {
            let result = self.child.try_wait();
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if matches!(result, Ok(Some(_)))
                || result
                    .as_ref()
                    .is_err_and(|error| error.raw_os_error() == Some(libc::ECHILD))
            {
                // Some consumes the wait status. ECHILD proves somebody else
                // did. Either way the numeric group identity is recyclable.
                self.process_group = None;
            }
            match result {
                Ok(Some(status)) => {
                    self.reaped_status = Some(status);
                    return Ok(status);
                }
                Ok(None) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to reap brokered command process {}",
                            self.child.id()
                        )
                    });
                }
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                bail!("brokered process cleanup exceeded its {timeout:?} deadline");
            };
            thread::sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
        }
    }
}

impl Drop for BrokeredProcess {
    fn drop(&mut self) {
        let _ = self.terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        // On Windows the retained kill-on-close Job Object is the final RAII
        // backstop if explicit termination or confirmation failed.
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_brokered_process(command: &mut Command) -> io::Result<BrokeredProcess> {
    unsafe {
        // SAFETY: pre_exec runs after fork and before exec. The closure only
        // calls the async-signal-safe setsid syscall and reads errno on error.
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn()?;
    let Ok(process_group) = libc::pid_t::try_from(child.id()) else {
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(io::Error::other(
            "brokered process identifier is not representable",
        ));
    };
    if process_group <= 0 {
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(io::Error::other(
            "brokered process identifier was not positive",
        ));
    }
    Ok(BrokeredProcess {
        child,
        process_group: Some(PinnedUnixProcessGroup { id: process_group }),
        reaped_status: None,
        cleanup_deadline: None,
        tree_cleanup_error: None,
        cleanup_complete: false,
    })
}

#[cfg(windows)]
fn spawn_brokered_process(command: &mut Command) -> io::Result<BrokeredProcess> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = create_brokered_process_job()?;
    command.creation_flags(windows_brokered_process_creation_flags());
    let mut child = command.spawn()?;
    // SAFETY: both handles are live and the child remains suspended, so it
    // cannot create an unowned descendant before assignment completes.
    let assigned = unsafe {
        AssignProcessToJobObject(
            job.as_raw_handle() as HANDLE,
            child.as_raw_handle() as HANDLE,
        )
    };
    if assigned == 0 {
        let error = io::Error::last_os_error();
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        if let Some(deadline) = deadline {
            let _ = terminate_windows_job(&job, deadline);
        }
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(error);
    }
    if let Err(error) = resume_suspended_windows_process(child.id()) {
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        if let Some(deadline) = deadline {
            let _ = terminate_windows_job(&job, deadline);
        }
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(error);
    }
    Ok(BrokeredProcess {
        child,
        job,
        reaped_status: None,
        cleanup_deadline: None,
        tree_cleanup_error: None,
        cleanup_complete: false,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn spawn_brokered_process(_command: &mut Command) -> io::Result<BrokeredProcess> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "owned brokered process trees are unavailable on this platform",
    ))
}

fn terminate_spawn_failure_child(child: &mut Child, deadline: Option<Instant>) {
    let _ = child.kill();
    let Some(deadline) = deadline else {
        return;
    };
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return;
        };
        thread::sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn observe_brokered_leader(process: &mut BrokeredProcess) -> io::Result<LeaderObservation> {
    let Some(process_group) = process.process_group.as_mut() else {
        return if process.reaped_status.is_some() {
            Ok(LeaderObservation::Exited)
        } else {
            Err(io::Error::other(
                "brokered process-group identity is no longer pinned",
            ))
        };
    };
    let mut information = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: information is writable siginfo_t storage, id names our direct
    // child, and WNOWAIT preserves the wait status so that child continues to
    // pin this process-group generation until cleanup.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            process_group.id as _,
            information.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == 0 {
        // SAFETY: successful waitid initialized information.
        let information = unsafe { information.assume_init() };
        // SAFETY: waitid populated the SIGCHLD union member.
        let observed_pid = unsafe { information.si_pid() };
        if observed_pid == 0 {
            return Ok(LeaderObservation::Running);
        }
        return classify_waitid_leader_observation(
            process_group.id,
            observed_pid,
            information.si_code,
        );
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::Interrupted {
        return Ok(LeaderObservation::Running);
    }
    update_unix_process_group_after_wait_error(&mut process.process_group, &error);
    Err(error)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn classify_waitid_leader_observation(
    expected_pid: libc::pid_t,
    observed_pid: libc::pid_t,
    code: libc::c_int,
) -> io::Result<LeaderObservation> {
    if observed_pid == 0 {
        return Ok(LeaderObservation::Running);
    }
    if observed_pid != expected_pid {
        return Err(io::Error::other(format!(
            "waitid observed unexpected brokered child PID {observed_pid}"
        )));
    }
    match code {
        libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED => Ok(LeaderObservation::Exited),
        libc::CLD_STOPPED | libc::CLD_TRAPPED | libc::CLD_CONTINUED => {
            Ok(LeaderObservation::Running)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("waitid returned unrecognized brokered child state code {code}"),
        )),
    }
}

#[cfg(windows)]
fn observe_brokered_leader(process: &mut BrokeredProcess) -> io::Result<LeaderObservation> {
    if process.reaped_status.is_some() {
        return Ok(LeaderObservation::Exited);
    }
    match process.child.try_wait()? {
        Some(status) => {
            // The Job Object remains a stable tree identity after try_wait
            // consumes the leader status on Windows.
            process.reaped_status = Some(status);
            Ok(LeaderObservation::Exited)
        }
        None => Ok(LeaderObservation::Running),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn observe_brokered_leader(_process: &mut BrokeredProcess) -> io::Result<LeaderObservation> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "non-reaping brokered process observation is unavailable on this platform",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn update_unix_process_group_after_wait_error(
    process_group: &mut Option<PinnedUnixProcessGroup>,
    error: &io::Error,
) {
    if error.raw_os_error() == Some(libc::ECHILD) {
        // ECHILD proves the wait status no longer pins this numeric identity.
        // Clear it before any cleanup path can issue a negative-PID signal.
        *process_group = None;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn with_pinned_unix_process_group<T>(
    process_group: Option<&PinnedUnixProcessGroup>,
    action: impl FnOnce(PinnedUnixProcessGroup) -> io::Result<T>,
) -> io::Result<T> {
    let process_group = process_group.ok_or_else(|| {
        io::Error::other(
            "brokered process-group identity is no longer pinned; refusing to signal it",
        )
    })?;
    action(*process_group)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn signal_pinned_unix_process_group(
    process: &mut BrokeredProcess,
    expected_group: libc::pid_t,
    deadline: Instant,
) -> io::Result<()> {
    signal_pinned_unix_process_group_with(
        process,
        expected_group,
        deadline,
        |process| process.process_group.map(|group| group.id),
        BrokeredProcess::observe_leader,
        Instant::now,
        |_, process_group| {
            // SAFETY: the retained exact-child wait status was freshly
            // observed and revalidated immediately before this call.
            if unsafe { libc::kill(-process_group, libc::SIGKILL) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        },
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn kill_pinned_unix_process_leader(
    process: &mut BrokeredProcess,
    expected_group: libc::pid_t,
    deadline: Instant,
) -> io::Result<()> {
    signal_pinned_unix_process_group_with(
        process,
        expected_group,
        deadline,
        |process| process.process_group.map(|group| group.id),
        BrokeredProcess::observe_leader,
        Instant::now,
        |process, _| process.child.kill(),
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn signal_pinned_unix_process_group_with<T>(
    state: &mut T,
    expected_group: libc::pid_t,
    deadline: Instant,
    mut current_group: impl FnMut(&T) -> Option<libc::pid_t>,
    mut observe_leader: impl FnMut(&mut T) -> io::Result<LeaderObservation>,
    mut now: impl FnMut() -> Instant,
    mut signal_group: impl FnMut(&mut T, libc::pid_t) -> io::Result<()>,
) -> io::Result<()> {
    let validate_group = |current_group: Option<libc::pid_t>| {
        if expected_group <= 0 {
            return Err(io::Error::other(
                "brokered process-group identity was not positive",
            ));
        }
        let current_group = current_group.ok_or_else(|| {
            io::Error::other(
                "brokered process-group identity is no longer pinned; refusing to signal it",
            )
        })?;
        if current_group <= 0 {
            return Err(io::Error::other(
                "brokered process-group identity was not positive",
            ));
        }
        if current_group != expected_group {
            return Err(io::Error::other(format!(
                "brokered process-group identity changed from {expected_group} to {current_group}"
            )));
        }
        Ok(())
    };

    validate_group(current_group(state))?;
    // WNOWAIT keeps the exact child unreaped. ECHILD clears the cached group
    // inside the production observer and must stop before any numeric signal.
    let _ = observe_leader(state)?;
    validate_group(current_group(state))?;
    if deadline
        .checked_duration_since(now())
        .is_none_or(|remaining| remaining.is_zero())
    {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "brokered process group {expected_group} signal observation exceeded its cleanup deadline"
            ),
        ));
    }
    signal_group(state, expected_group)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn terminate_brokered_process_tree(
    process: &mut BrokeredProcess,
    deadline: Instant,
) -> AnyResult<()> {
    let group = with_pinned_unix_process_group(process.process_group.as_ref(), Ok)?;
    let signal_result = match signal_pinned_unix_process_group(process, group.id, deadline) {
        Ok(()) => Ok(()),
        Err(error) => {
            if error.raw_os_error() == Some(libc::ESRCH) {
                // ESRCH is only an inconclusive signal result. The retained leader
                // still pins this generation, and confirmation below re-signals
                // before every independent membership proof.
                Ok(())
            } else {
                #[cfg(target_vendor = "apple")]
                let error = if error.raw_os_error() == Some(libc::EPERM) {
                    // Darwin reports EPERM for a group containing only its zombie
                    // leader, but also when any live member is not signalable. The
                    // exact-PID WNOWAIT observation pins the group; the atomic
                    // membership proof below must still establish that the exited
                    // leader is its sole remaining member. Preserve EPERM whenever
                    // either observation or confirmation fails.
                    let observation = process.observe_leader();
                    match resolve_macos_group_signal_eperm(error, observation, || {
                        confirm_unix_process_group_quiescent(process, group.id, deadline)
                    })? {
                        None => return Ok(()),
                        Some(error) => error,
                    }
                } else {
                    error
                };
                Err(error)
            }
        }
    };
    let signal_error = match signal_result {
        Ok(()) => None,
        Err(group_error) => {
            // A direct kill is safe only while the same unconsumed wait status
            // still pins the PID. It cannot establish descendant cleanup, so
            // retain the group error as context unless the platform-specific
            // confirmation subsequently proves the whole group quiescent.
            if let Err(direct_error) = kill_pinned_unix_process_leader(process, group.id, deadline)
            {
                bail!(
                    "process-group SIGKILL failed: {group_error}; direct child SIGKILL also failed: {direct_error}"
                );
            }
            Some(group_error)
        }
    };
    let confirmation = confirm_unix_process_group_quiescent(process, group.id, deadline);
    finish_unix_process_group_termination(signal_error, confirmation)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn finish_unix_process_group_termination(
    signal_error: Option<io::Error>,
    confirmation: AnyResult<()>,
) -> AnyResult<()> {
    match (signal_error, confirmation) {
        (_, Ok(())) => Ok(()),
        (None, result) => result,
        (Some(signal_error), Err(confirmation_error)) => Err(anyhow!(
            "process-group SIGKILL failed: {signal_error}; group confirmation also failed: {confirmation_error:#}"
        )),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn confirm_unix_process_group_quiescent_with<T>(
    state: &mut T,
    process_group: libc::pid_t,
    deadline: Instant,
    required_quiescent_proofs: u8,
    mut signal_group: impl FnMut(&mut T, libc::pid_t, Instant) -> io::Result<()>,
    mut prove_quiescent: impl FnMut(&mut T, libc::pid_t, Instant) -> AnyResult<bool>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
) -> AnyResult<()> {
    if required_quiescent_proofs == 0 {
        bail!("brokered process-group confirmation requires at least one proof");
    }

    let timeout_error = || {
        anyhow!(
            "brokered process group {process_group} retained additional or unverified members through its cleanup deadline"
        )
    };
    let mut consecutive_quiescent_proofs = 0_u8;
    let mut retained_signal_error = None;
    loop {
        if deadline
            .checked_duration_since(now())
            .is_none_or(|remaining| remaining.is_zero())
        {
            return finish_unix_process_group_termination(
                retained_signal_error,
                Err(timeout_error()),
            );
        }

        // A member can fork or join this still-pinned group after an earlier
        // group signal. Re-signal immediately before every proof; on Linux this
        // also places a fresh SIGKILL between the two required empty scans.
        if let Err(error) = signal_group(state, process_group, deadline) {
            // ESRCH and Darwin EPERM are inconclusive rather than absence. All
            // other errors are retained too: an independent proof may still
            // establish quiescence, while a failed proof must preserve the
            // earliest signal failure as its primary context.
            retained_signal_error.get_or_insert(error);
        }
        if deadline
            .checked_duration_since(now())
            .is_none_or(|remaining| remaining.is_zero())
        {
            return finish_unix_process_group_termination(
                retained_signal_error,
                Err(timeout_error()),
            );
        }

        let quiescent = match prove_quiescent(state, process_group, deadline) {
            Ok(quiescent) => quiescent,
            Err(error) => {
                return finish_unix_process_group_termination(retained_signal_error, Err(error));
            }
        };
        if deadline
            .checked_duration_since(now())
            .is_none_or(|remaining| remaining.is_zero())
        {
            return finish_unix_process_group_termination(
                retained_signal_error,
                Err(timeout_error()),
            );
        }
        if quiescent {
            consecutive_quiescent_proofs += 1;
            if consecutive_quiescent_proofs == required_quiescent_proofs {
                return Ok(());
            }
        } else {
            consecutive_quiescent_proofs = 0;
        }

        let Some(remaining) = deadline.checked_duration_since(now()) else {
            return finish_unix_process_group_termination(
                retained_signal_error,
                Err(timeout_error()),
            );
        };
        sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
    }
}

#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
fn resolve_macos_group_signal_eperm(
    signal_error: io::Error,
    observation: io::Result<LeaderObservation>,
    confirm_quiescence: impl FnOnce() -> AnyResult<()>,
) -> AnyResult<Option<io::Error>> {
    match observation {
        Ok(LeaderObservation::Running) => Ok(Some(signal_error)),
        Ok(LeaderObservation::Exited) => {
            finish_unix_process_group_termination(Some(signal_error), confirm_quiescence())?;
            Ok(None)
        }
        Err(observation_error) => finish_unix_process_group_termination(
            Some(signal_error),
            Err(anyhow!(observation_error).context("failed to observe brokered process leader")),
        )
        .map(|()| None),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn confirm_unix_process_group_quiescent(
    process: &mut BrokeredProcess,
    process_group: libc::pid_t,
    deadline: Instant,
) -> AnyResult<()> {
    #[cfg(target_os = "linux")]
    {
        return confirm_unix_process_group_quiescent_with(
            process,
            process_group,
            deadline,
            2,
            signal_pinned_unix_process_group,
            |process, expected_group, deadline| {
                with_pinned_unix_process_group(process.process_group.as_ref(), |group| {
                    if group.id == expected_group {
                        Ok(())
                    } else {
                        Err(io::Error::other(format!(
                            "brokered process-group identity changed from {expected_group} to {}",
                            group.id
                        )))
                    }
                })?;
                linux_process_group_has_live_members(expected_group, deadline)
                    .map(|live_members| !live_members)
            },
            Instant::now,
            thread::sleep,
        );
    }
    #[cfg(target_vendor = "apple")]
    {
        confirm_unix_process_group_quiescent_with(
            process,
            process_group,
            deadline,
            1,
            signal_pinned_unix_process_group,
            |process, expected_group, deadline| {
                let leader_exited = process.observe_leader()? == LeaderObservation::Exited;
                if deadline.checked_duration_since(Instant::now()).is_none() {
                    bail!(
                        "brokered process group {expected_group} leader observation exceeded its cleanup deadline"
                    );
                }
                if leader_exited {
                    macos_process_group_contains_only_pinned_leader(expected_group)
                } else {
                    Ok(false)
                }
            },
            Instant::now,
            thread::sleep,
        )
    }
}

#[cfg(target_os = "macos")]
fn macos_process_group_contains_only_pinned_leader(process_group: libc::pid_t) -> AnyResult<bool> {
    let mut members = [0 as libc::pid_t; 2];
    let buffer_size = i32::try_from(std::mem::size_of_val(&members))
        .map_err(|_| anyhow!("macOS process-group snapshot buffer size was not representable"))?;
    // SAFETY: members is writable storage for exactly two pid_t values and the
    // byte count describes that complete live buffer. libproc returns a PID
    // count; a count of two may mean two or more members because the kernel
    // deliberately caps collection at the supplied capacity.
    let count =
        unsafe { libc::proc_listpgrppids(process_group, members.as_mut_ptr().cast(), buffer_size) };
    if count <= 0 {
        let error = io::Error::last_os_error();
        bail!("failed to atomically list brokered process group {process_group} members: {error}");
    }
    classify_macos_process_group_snapshot(process_group, count, members)
}

#[cfg(any(target_os = "macos", test))]
fn classify_macos_process_group_snapshot(
    process_group: i32,
    count: i32,
    members: [i32; 2],
) -> AnyResult<bool> {
    if process_group <= 0 {
        bail!("macOS process-group snapshot used a non-positive pinned leader");
    }
    let count = usize::try_from(count)
        .map_err(|_| anyhow!("macOS process-group snapshot returned a negative member count"))?;
    if count == 0 || count > members.len() {
        bail!("macOS process-group snapshot returned an untrusted member count of {count}");
    }
    let observed = &members[..count];
    if observed.iter().any(|pid| *pid <= 0) {
        bail!("macOS process-group snapshot returned a non-positive member identifier");
    }
    if count == members.len() {
        // The kernel scans live allproc entries before zombies and caps its
        // output at this buffer. Two positive PIDs therefore means "at least
        // two members"; the pinned zombie need not be among the returned pair.
        return Ok(false);
    }
    let leader_count = observed.iter().filter(|pid| **pid == process_group).count();
    if leader_count != 1 {
        bail!(
            "macOS process-group snapshot did not contain exactly one pinned leader {process_group}"
        );
    }
    Ok(count == 1)
}

#[cfg(target_os = "linux")]
fn linux_process_group_has_live_members(
    process_group: libc::pid_t,
    deadline: Instant,
) -> AnyResult<bool> {
    let mut within_budget = || {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| !remaining.is_zero())
    };
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    let entries = std::fs::read_dir("/proc");
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    let pids = collect_linux_process_ids_with(
        process_group,
        entries.context("failed to enumerate /proc")?,
        |entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<libc::pid_t>().ok())
        },
        &mut within_budget,
    )?;
    linux_process_group_has_live_members_with(
        process_group,
        pids,
        |pid| std::fs::read_to_string(format!("/proc/{pid}/stat")),
        linux_process_group_for_pid,
        &mut within_budget,
    )
}

#[cfg(any(target_os = "linux", test))]
fn collect_linux_process_ids_with<T>(
    process_group: i32,
    mut entries: impl Iterator<Item = io::Result<T>>,
    mut process_id: impl FnMut(T) -> Option<i32>,
    mut within_budget: impl FnMut() -> bool,
) -> AnyResult<Vec<i32>> {
    let mut pids = Vec::new();
    loop {
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let entry = entries.next();
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let Some(entry) = entry else {
            return Ok(pids);
        };
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("failed to enumerate /proc entry"),
        };
        if let Some(pid) = process_id(entry) {
            pids.push(pid);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn ensure_linux_group_scan_budget(
    process_group: i32,
    within_budget: &mut impl FnMut() -> bool,
) -> AnyResult<()> {
    if within_budget() {
        Ok(())
    } else {
        bail!("brokered process group {process_group} cleanup scan exceeded its deadline")
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_process_group_has_live_members_with(
    process_group: i32,
    pids: impl IntoIterator<Item = i32>,
    mut read_stat: impl FnMut(i32) -> io::Result<String>,
    mut process_group_for_pid: impl FnMut(i32) -> io::Result<Option<i32>>,
    mut within_budget: impl FnMut() -> bool,
) -> AnyResult<bool> {
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    for pid in pids {
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let observation = read_stat(pid).and_then(parse_linux_process_stat);
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let observation = match observation {
            Ok(observation) => observation,
            Err(stat_error) => {
                ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
                let observed_group = process_group_for_pid(pid);
                ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
                match observed_group {
                    Ok(None) => continue,
                    Ok(Some(other_group)) if other_group != process_group => continue,
                    Ok(Some(_)) => {
                        return Err(stat_error).with_context(|| {
                        format!(
                            "could not inspect process {pid}, which belongs to owned process group {process_group}"
                        )
                    });
                    }
                    Err(group_error) => {
                        return Err(stat_error).with_context(|| {
                        format!(
                            "could not inspect process {pid} or prove it is outside owned process group {process_group}: {group_error}"
                        )
                    });
                    }
                }
            }
        };
        if observation.process_group == process_group && observation.live {
            ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
            return Ok(true);
        }
    }
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    Ok(false)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinuxProcessObservation {
    process_group: i32,
    live: bool,
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_process_stat(stat: String) -> io::Result<LinuxProcessObservation> {
    let (_, fields) = stat
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing stat command field"))?;
    let mut fields = fields.split_whitespace();
    let state = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process state"))?;
    let process_group = fields
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process group"))?
        .parse::<i32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid process group"))?;
    Ok(LinuxProcessObservation {
        process_group,
        live: !matches!(state, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "linux")]
fn linux_process_group_for_pid(pid: libc::pid_t) -> io::Result<Option<libc::pid_t>> {
    // SAFETY: pid is a positive process identifier read from /proc. getpgid
    // only observes its current process-group membership.
    let process_group = unsafe { libc::getpgid(pid) };
    if process_group >= 0 {
        return Ok(Some(process_group));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(windows)]
fn terminate_brokered_process_tree(
    process: &mut BrokeredProcess,
    deadline: Instant,
) -> AnyResult<()> {
    let job_result = terminate_windows_job(&process.job, deadline);
    if job_result.is_ok() {
        return Ok(());
    }
    let direct_result = process.child.kill();
    match (job_result, direct_result) {
        (Err(job_error), Err(direct_error)) => Err(anyhow!(
            "Job Object termination failed: {job_error}; direct child termination also failed: {direct_error}"
        )),
        (Err(job_error), Ok(())) => Err(job_error.into()),
        (Ok(()), _) => Ok(()),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn terminate_brokered_process_tree(
    _process: &mut BrokeredProcess,
    _deadline: Instant,
) -> AnyResult<()> {
    bail!("owned brokered process-tree cleanup is unavailable on this platform")
}

fn wait_for_capped_output(
    mut process: BrokeredProcess,
    command_name: &str,
    timeout: Duration,
) -> AnyResult<(PortableRunStatus, SecretBytes, SecretBytes)> {
    let mut drains = match CappedOutputDrains::start(&mut process.child) {
        Ok(drains) => drains,
        Err(primary) => {
            let cleanup = process.terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT);
            return Err(append_secondary_error(
                primary,
                "process cleanup also failed",
                cleanup.err(),
            ));
        }
    };
    let deadline = match checked_deadline("brokered command run", timeout) {
        Ok(deadline) => deadline,
        Err(primary) => {
            let cleanup = process.terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT);
            let drain = drains.finish(BROKERED_OUTPUT_DRAIN_TIMEOUT);
            let primary =
                append_secondary_error(primary, "process cleanup also failed", cleanup.err());
            return Err(append_secondary_error(
                primary,
                "output drain also failed",
                drain.err(),
            ));
        }
    };
    let primary = loop {
        if deadline.checked_duration_since(Instant::now()).is_none() {
            break Some(brokered_run_timeout_error(command_name, timeout));
        }
        if let Err(error) = preserve_poll_result_before_timeout(
            drains.poll(),
            deadline.checked_duration_since(Instant::now()),
            || brokered_run_timeout_error(command_name, timeout),
        ) {
            break Some(error);
        }
        let observation = process
            .observe_leader()
            .map_err(anyhow::Error::from)
            .context(format!("failed to poll brokered command '{command_name}'"));
        let observation = preserve_leader_poll_result_before_timeout(
            observation,
            deadline.checked_duration_since(Instant::now()),
            || brokered_run_timeout_error(command_name, timeout),
        );
        match observation {
            Ok(LeaderObservation::Running) => {}
            Ok(LeaderObservation::Exited) => break None,
            Err(error) => break Some(error),
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break Some(brokered_run_timeout_error(command_name, timeout));
        };
        thread::sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
    };

    // End the entire owned tree on every path before the sole Unix wait. A
    // leader may exit successfully while a descendant still owns a pipe.
    let cleanup = process.terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT);
    let captured = drains.finish(BROKERED_OUTPUT_DRAIN_TIMEOUT);
    if let Some(primary) = primary {
        let primary = append_secondary_error(primary, "process cleanup also failed", cleanup.err());
        return Err(append_secondary_error(
            primary,
            "output drain also failed",
            captured.err(),
        ));
    }
    let status = match cleanup {
        Ok(status) => status,
        Err(primary) => {
            return Err(append_secondary_error(
                primary,
                "output drain also failed",
                captured.err(),
            ));
        }
    };
    let (stdout, stderr) = captured?;
    Ok((run_status(status), stdout, stderr))
}

fn preserve_poll_result_before_timeout(
    result: AnyResult<()>,
    remaining: Option<Duration>,
    timeout_error: impl FnOnce() -> anyhow::Error,
) -> AnyResult<()> {
    result?;
    remaining.map(|_| ()).ok_or_else(timeout_error)
}

fn preserve_leader_poll_result_before_timeout(
    result: AnyResult<LeaderObservation>,
    remaining: Option<Duration>,
    timeout_error: impl FnOnce() -> anyhow::Error,
) -> AnyResult<LeaderObservation> {
    match result {
        Ok(LeaderObservation::Exited) => Ok(LeaderObservation::Exited),
        Err(error) => Err(error),
        Ok(LeaderObservation::Running) => remaining
            .map(|_| LeaderObservation::Running)
            .ok_or_else(timeout_error),
    }
}

fn brokered_run_timeout_error(command_name: &str, timeout: Duration) -> anyhow::Error {
    anyhow!("brokered command '{command_name}' exceeded the {timeout:?} run timeout")
}

fn append_secondary_error(
    primary: anyhow::Error,
    label: &str,
    secondary: Option<anyhow::Error>,
) -> anyhow::Error {
    match secondary {
        Some(secondary) => anyhow!("{primary:#}; {label}: {secondary:#}"),
        None => primary,
    }
}

fn fixed_process_cleanup_deadline(
    deadline: &mut Option<Instant>,
    timeout: Duration,
) -> AnyResult<(Instant, bool)> {
    if let Some(deadline) = *deadline {
        return Ok((deadline, false));
    }
    let new_deadline = checked_deadline("brokered process cleanup", timeout)?;
    *deadline = Some(new_deadline);
    Ok((new_deadline, true))
}

#[cfg(windows)]
const fn windows_brokered_process_creation_flags() -> u32 {
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

    CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP
}

#[cfg(windows)]
fn create_brokered_process_job() -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    // SAFETY: null attributes and name request a private unnamed Job Object.
    let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw_job.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw_job is a newly created owned handle, transferred once.
    let job = unsafe { OwnedHandle::from_raw_handle(raw_job as RawHandle) };
    let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: the pointer and byte length describe the requested live value.
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            (&raw const information).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

#[cfg(windows)]
fn resume_suspended_windows_process(pid: u32) -> io::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // SAFETY: the snapshot call has no borrowed pointer arguments.
    let raw_snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw_snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw_snapshot is a newly created owned handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw_snapshot as RawHandle) };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    // SAFETY: entry has the required size and remains writable and live.
    let mut has_entry = unsafe { Thread32First(raw_snapshot, &mut entry) } != 0;
    while has_entry {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the ID came from the live snapshot; request only resume.
            let raw_thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if raw_thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: raw_thread is a newly opened owned handle.
            let thread = unsafe { OwnedHandle::from_raw_handle(raw_thread as RawHandle) };
            // SAFETY: this thread belongs to the suspended child and the
            // handle carries THREAD_SUSPEND_RESUME.
            let previous_count = unsafe { ResumeThread(raw_thread) };
            drop(thread);
            drop(snapshot);
            if previous_count == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            return Ok(());
        }
        // SAFETY: same live snapshot and initialized entry as above.
        has_entry = unsafe { Thread32Next(raw_snapshot, &mut entry) } != 0;
    }
    Err(io::Error::other(
        "could not find the suspended brokered process thread",
    ))
}

#[cfg(windows)]
fn terminate_windows_job(
    job: &std::os::windows::io::OwnedHandle,
    deadline: Instant,
) -> io::Result<()> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, TerminateJobObject,
    };

    // SAFETY: job is a live Job Object owned by the process supervisor.
    if unsafe { TerminateJobObject(job.as_raw_handle() as HANDLE, 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    loop {
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: output points to a correctly sized accounting structure.
        let queried = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle() as HANDLE,
                JobObjectBasicAccountingInformation,
                (&raw mut information).cast::<c_void>(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        if information.ActiveProcesses == 0 {
            return Ok(());
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "brokered Job Object cleanup timed out",
            ));
        };
        thread::sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PortableRunStatus {
    exit_status: i32,
    exit_signal: Option<i32>,
}

fn run_status(status: ExitStatus) -> PortableRunStatus {
    if let Some(code) = status.code() {
        return PortableRunStatus {
            exit_status: code,
            exit_signal: None,
        };
    }
    #[cfg(unix)]
    {
        let signal = status.signal();
        PortableRunStatus {
            exit_status: signal.map(|signal| 128 + signal).unwrap_or(1),
            exit_signal: signal,
        }
    }
    #[cfg(not(unix))]
    {
        PortableRunStatus {
            exit_status: 1,
            exit_signal: None,
        }
    }
}

fn preserve_minimal_environment(command: &mut Command) {
    // Env forwarding is allowlist-only. Loader/interpreter hooks such as
    // LD_PRELOAD, DYLD_*, PYTHONPATH, NODE_OPTIONS, SSH_AUTH_SOCK, XDG_*,
    // and TZ stay out unless deliberately added to the exact list below.
    for (name, value) in std::env::vars() {
        if is_preserved_env_var_name(&name) {
            command.env(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_MODE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_MODE";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_MARKER_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_MARKER";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_RELEASE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_RELEASE";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_DONE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_DONE";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const PIPE_ESCAPE_HELPER_TEST: &str = "run::tests::brokered_run_pipe_escape_helper";

    #[cfg(windows)]
    const WINDOWS_JOB_MODE_VAR: &str = "JIG_VAULT_WINDOWS_JOB_MODE";
    #[cfg(windows)]
    const WINDOWS_JOB_READY_VAR: &str = "JIG_VAULT_WINDOWS_JOB_READY";
    #[cfg(windows)]
    const WINDOWS_JOB_RELEASE_VAR: &str = "JIG_VAULT_WINDOWS_JOB_RELEASE";
    #[cfg(windows)]
    const WINDOWS_JOB_LEAK_VAR: &str = "JIG_VAULT_WINDOWS_JOB_LEAK";
    #[cfg(windows)]
    const WINDOWS_JOB_HELPER_TEST: &str = "run::tests::windows_brokered_job_descendant_helper";

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn test_env_mapping(var: &str, secret_name: &str, value: &[u8]) -> ResolvedBrokeredEnv {
        ResolvedBrokeredEnv {
            var: EnvVarName::parse(var).unwrap(),
            secret_name: SecretName::parse(secret_name).unwrap(),
            value: SecretBytes::new(value.to_vec()),
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn wait_for_path(path: &std::path::Path, timeout: Duration) {
        let deadline = Instant::now().checked_add(timeout).unwrap();
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for test marker {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn assert_path_stays_absent(path: &std::path::Path, duration: Duration) {
        let deadline = Instant::now().checked_add(duration).unwrap();
        while Instant::now() < deadline {
            assert!(
                !path.exists(),
                "terminated descendant wrote unexpected marker {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn cleanup_deadline_is_fixed_by_the_first_attempt() {
        let mut deadline = None;
        let (first, first_attempt) =
            fixed_process_cleanup_deadline(&mut deadline, Duration::from_secs(1)).unwrap();
        let (second, second_attempt) =
            fixed_process_cleanup_deadline(&mut deadline, Duration::from_secs(30)).unwrap();

        assert!(first_attempt);
        assert!(!second_attempt);
        assert_eq!(first, second);
    }

    #[test]
    fn secondary_cleanup_error_does_not_replace_primary_error() {
        let error = append_secondary_error(
            anyhow!("capture failed first"),
            "process cleanup also failed",
            Some(anyhow!("cleanup failed second")),
        )
        .to_string();

        assert!(error.starts_with("capture failed first"));
        assert!(error.contains("process cleanup also failed: cleanup failed second"));
    }

    #[test]
    fn completed_poll_results_take_precedence_over_a_newly_expired_deadline() {
        let drain_error =
            preserve_poll_result_before_timeout(Err(anyhow!("capture failed first")), None, || {
                anyhow!("timeout second")
            })
            .unwrap_err()
            .to_string();
        assert_eq!(drain_error, "capture failed first");

        assert_eq!(
            preserve_leader_poll_result_before_timeout(
                Ok(LeaderObservation::Exited),
                None,
                || anyhow!("timeout second"),
            )
            .unwrap(),
            LeaderObservation::Exited,
        );
        let observation_error = preserve_leader_poll_result_before_timeout(
            Err(anyhow!("observation failed first")),
            None,
            || anyhow!("timeout second"),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(observation_error, "observation failed first");
        let timeout = preserve_leader_poll_result_before_timeout(
            Ok(LeaderObservation::Running),
            None,
            || anyhow!("timeout after running observation"),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(timeout, "timeout after running observation");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proven_process_group_quiescence_supersedes_a_stale_signal_error() {
        finish_unix_process_group_termination(
            Some(io::Error::from_raw_os_error(libc::EPERM)),
            Ok(()),
        )
        .unwrap();

        let confirmation_only =
            finish_unix_process_group_termination(None, Err(anyhow!("confirmation failed")))
                .unwrap_err()
                .to_string();
        assert_eq!(confirmation_only, "confirmation failed");

        let combined = finish_unix_process_group_termination(
            Some(io::Error::from_raw_os_error(libc::EPERM)),
            Err(anyhow!("confirmation failed")),
        )
        .unwrap_err()
        .to_string();
        assert!(combined.contains("process-group SIGKILL failed"));
        assert!(combined.contains("group confirmation also failed: confirmation failed"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_group_signal_refuses_identity_loss_before_any_signal() {
        struct SignalState {
            process_group: Option<libc::pid_t>,
            signal_attempts: usize,
        }

        let started = Instant::now();
        let mut state = SignalState {
            process_group: Some(73),
            signal_attempts: 0,
        };
        let error = signal_pinned_unix_process_group_with(
            &mut state,
            73,
            started + Duration::from_secs(1),
            |state| state.process_group,
            |state| {
                state.process_group = None;
                Err(io::Error::from_raw_os_error(libc::ECHILD))
            },
            || started,
            |state, _| {
                state.signal_attempts += 1;
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
        assert_eq!(state.process_group, None);
        assert_eq!(state.signal_attempts, 0);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_group_signal_refuses_when_observation_crosses_deadline() {
        struct SignalState {
            process_group: Option<libc::pid_t>,
            signal_attempts: usize,
        }

        let started = Instant::now();
        let deadline = started + Duration::from_millis(10);
        let clock = std::cell::Cell::new(started);
        let mut state = SignalState {
            process_group: Some(73),
            signal_attempts: 0,
        };
        let error = signal_pinned_unix_process_group_with(
            &mut state,
            73,
            deadline,
            |state| state.process_group,
            |_| {
                clock.set(deadline);
                Ok(LeaderObservation::Exited)
            },
            || clock.get(),
            |state, _| {
                state.signal_attempts += 1;
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("cleanup deadline"));
        assert_eq!(state.signal_attempts, 0);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_group_confirmation_resignals_a_late_member_and_between_linux_proofs() {
        struct ConfirmationState {
            late_member_live: bool,
            signal_attempts: usize,
            proof_attempts: usize,
        }

        let started = Instant::now();
        let clock = std::cell::Cell::new(started);
        let mut state = ConfirmationState {
            // Model a member created after the caller's initial SIGKILL. The
            // confirmation loop must kill it before accepting any proof.
            late_member_live: true,
            signal_attempts: 0,
            proof_attempts: 0,
        };
        confirm_unix_process_group_quiescent_with(
            &mut state,
            73,
            started + Duration::from_secs(1),
            2,
            |state, process_group, received_deadline| {
                assert_eq!(process_group, 73);
                assert_eq!(received_deadline, started + Duration::from_secs(1));
                state.signal_attempts += 1;
                state.late_member_live = false;
                Ok(())
            },
            |state, process_group, _| {
                assert_eq!(process_group, 73);
                state.proof_attempts += 1;
                Ok(!state.late_member_live)
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
        )
        .unwrap();

        assert_eq!(state.signal_attempts, 2);
        assert_eq!(state.proof_attempts, 2);
        assert!(!state.late_member_live);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_group_confirmation_treats_esrch_and_eperm_as_inconclusive() {
        struct ConfirmationState {
            signal_attempts: usize,
            proof_attempts: usize,
        }

        let started = Instant::now();
        let clock = std::cell::Cell::new(started);
        let mut state = ConfirmationState {
            signal_attempts: 0,
            proof_attempts: 0,
        };
        confirm_unix_process_group_quiescent_with(
            &mut state,
            73,
            started + Duration::from_secs(1),
            1,
            |state, _, _| {
                state.signal_attempts += 1;
                let errno = if state.signal_attempts == 1 {
                    libc::ESRCH
                } else {
                    libc::EPERM
                };
                Err(io::Error::from_raw_os_error(errno))
            },
            |state, _, _| {
                state.proof_attempts += 1;
                // ESRCH must not finish the first iteration. EPERM may be
                // superseded only by this independent proof on the second.
                Ok(state.proof_attempts == 2)
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
        )
        .unwrap();

        assert_eq!(state.signal_attempts, 2);
        assert_eq!(state.proof_attempts, 2);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_group_confirmation_preserves_signal_error_before_proof_error() {
        let error = confirm_unix_process_group_quiescent_with(
            &mut (),
            73,
            Instant::now() + Duration::from_secs(1),
            1,
            |_, _, _| Err(io::Error::from_raw_os_error(libc::EPERM)),
            |_, _, _| Err(anyhow!("sole-leader snapshot failed")),
            Instant::now,
            |_| {},
        )
        .unwrap_err();
        let error = format!("{error:#}");

        assert!(error.contains("process-group SIGKILL failed"));
        assert!(error.contains(&io::Error::from_raw_os_error(libc::EPERM).to_string()));
        assert!(error.contains("group confirmation also failed"));
        assert!(error.contains("sole-leader snapshot failed"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_group_confirmation_rejects_a_proof_that_crosses_deadline() {
        struct ConfirmationState {
            signal_attempts: usize,
            proof_attempts: usize,
        }

        let started = Instant::now();
        let deadline = started + Duration::from_millis(10);
        let clock = std::cell::Cell::new(started);
        let mut state = ConfirmationState {
            signal_attempts: 0,
            proof_attempts: 0,
        };
        let error = confirm_unix_process_group_quiescent_with(
            &mut state,
            73,
            deadline,
            1,
            |state, _, received_deadline| {
                assert_eq!(received_deadline, deadline);
                state.signal_attempts += 1;
                Ok(())
            },
            |state, _, received_deadline| {
                assert_eq!(received_deadline, deadline);
                state.proof_attempts += 1;
                clock.set(deadline);
                Ok(true)
            },
            || clock.get(),
            |_| panic!("an expired proof must not reach the sleep callback"),
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("cleanup deadline"),
            "unexpected error: {error}"
        );
        assert_eq!(state.signal_attempts, 1);
        assert_eq!(state.proof_attempts, 1);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn macos_eperm_special_case_preserves_signal_failure_until_proven_quiescent() {
        let eperm = || io::Error::from_raw_os_error(libc::EPERM);
        let eperm_text = eperm().to_string();

        let observation_error = resolve_macos_group_signal_eperm(
            eperm(),
            Err(io::Error::other("leader observation failed")),
            || panic!("confirmation must not run after an observation error"),
        )
        .unwrap_err()
        .to_string();
        assert!(observation_error.contains(&eperm_text));
        assert!(observation_error.contains("failed to observe brokered process leader"));
        assert!(observation_error.contains("leader observation failed"));

        let confirmation_error =
            resolve_macos_group_signal_eperm(eperm(), Ok(LeaderObservation::Exited), || {
                Err(anyhow!("sole-leader snapshot failed"))
            })
            .unwrap_err()
            .to_string();
        assert!(confirmation_error.contains(&eperm_text));
        assert!(confirmation_error.contains("sole-leader snapshot failed"));

        assert!(
            resolve_macos_group_signal_eperm(eperm(), Ok(LeaderObservation::Exited), || Ok(()),)
                .unwrap()
                .is_none()
        );

        let mut confirmation_called = false;
        let fallback_error =
            resolve_macos_group_signal_eperm(eperm(), Ok(LeaderObservation::Running), || {
                confirmation_called = true;
                Ok(())
            })
            .unwrap()
            .expect("a running leader must keep EPERM for the fallback path");
        assert!(!confirmation_called);
        assert_eq!(fallback_error.raw_os_error(), Some(libc::EPERM));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_process_group_gate_targets_only_the_pinned_identity() {
        let group = PinnedUnixProcessGroup { id: 4242 };
        let mut target = None;
        with_pinned_unix_process_group(Some(&group), |owned| {
            target = Some(owned.id);
            Ok(())
        })
        .unwrap();
        assert_eq!(target, Some(4242));

        target = None;
        let error = with_pinned_unix_process_group(None, |owned| {
            target = Some(owned.id);
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("refusing to signal"));
        assert_eq!(target, None, "lost identity reached the signal closure");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_wait_error_clears_identity_only_for_echild() {
        let group = PinnedUnixProcessGroup { id: 4242 };
        let mut identity = Some(group);
        update_unix_process_group_after_wait_error(
            &mut identity,
            &io::Error::from_raw_os_error(libc::EINVAL),
        );
        assert!(identity.is_some());

        update_unix_process_group_after_wait_error(
            &mut identity,
            &io::Error::from_raw_os_error(libc::ENOSYS),
        );
        assert!(identity.is_some());

        update_unix_process_group_after_wait_error(
            &mut identity,
            &io::Error::from_raw_os_error(libc::ECHILD),
        );
        assert!(identity.is_none());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn waitid_observer_accepts_only_terminal_child_state_codes() {
        for code in [libc::CLD_EXITED, libc::CLD_KILLED, libc::CLD_DUMPED] {
            assert_eq!(
                classify_waitid_leader_observation(73, 73, code).unwrap(),
                LeaderObservation::Exited
            );
        }
        for code in [libc::CLD_STOPPED, libc::CLD_TRAPPED, libc::CLD_CONTINUED] {
            assert_eq!(
                classify_waitid_leader_observation(73, 73, code).unwrap(),
                LeaderObservation::Running
            );
        }
        assert_eq!(
            classify_waitid_leader_observation(73, 0, i32::MAX).unwrap(),
            LeaderObservation::Running
        );
        assert!(classify_waitid_leader_observation(73, 74, libc::CLD_EXITED).is_err());
        assert!(classify_waitid_leader_observation(73, 73, i32::MAX).is_err());
    }

    #[test]
    fn macos_group_snapshot_requires_the_exact_sole_pinned_leader() {
        assert!(classify_macos_process_group_snapshot(73, 1, [73, 0]).unwrap());
        assert!(!classify_macos_process_group_snapshot(73, 2, [73, 74]).unwrap());
        assert!(!classify_macos_process_group_snapshot(73, 2, [74, 73]).unwrap());
        assert!(!classify_macos_process_group_snapshot(73, 2, [74, 75]).unwrap());
        assert!(!classify_macos_process_group_snapshot(73, 2, [73, 73]).unwrap());

        for (process_group, count, members) in [
            (73, 0, [0, 0]),
            (73, -1, [0, 0]),
            (73, 3, [73, 74]),
            (73, 1, [74, 0]),
            (73, 2, [73, 0]),
            (0, 1, [73, 0]),
        ] {
            assert!(
                classify_macos_process_group_snapshot(process_group, count, members).is_err(),
                "untrusted snapshot was accepted: group={process_group}, count={count}, members={members:?}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_group_confirmation_resignals_an_exited_leader_with_a_live_member() {
        let temp = tempfile::tempdir().unwrap();
        let ready = temp.path().join("macos-group-member-ready");
        let release = temp.path().join("macos-group-member-release");
        let leak = temp.path().join("macos-group-member-leaked");
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "(printf ready > \"$JIG_VAULT_TEST_READY\"; while [ ! -e \"$JIG_VAULT_TEST_RELEASE\" ]; do sleep 0.01; done; printf leaked > \"$JIG_VAULT_TEST_LEAK\") & while [ ! -e \"$JIG_VAULT_TEST_READY\" ]; do sleep 0.01; done; exit 0",
            ])
            .env("JIG_VAULT_TEST_READY", &ready)
            .env("JIG_VAULT_TEST_RELEASE", &release)
            .env("JIG_VAULT_TEST_LEAK", &leak)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = BrokeredProcess::spawn(&mut command).unwrap();
        wait_for_path(&ready, Duration::from_secs(2));

        let observation_deadline = Instant::now().checked_add(Duration::from_secs(2)).unwrap();
        loop {
            if process.observe_leader().unwrap() == LeaderObservation::Exited {
                break;
            }
            assert!(
                Instant::now() < observation_deadline,
                "brokered test leader did not exit"
            );
            thread::sleep(Duration::from_millis(2));
        }
        let process_group = process.process_group.unwrap().id;
        let confirmation_deadline = Instant::now()
            .checked_add(Duration::from_millis(500))
            .unwrap();
        confirm_unix_process_group_quiescent(&mut process, process_group, confirmation_deadline)
            .unwrap();

        let status = process
            .terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT)
            .unwrap();
        assert_eq!(status.code(), Some(0));
        fs::write(release, b"release").unwrap();
        assert_path_stays_absent(&leak, Duration::from_millis(300));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_group_confirmation_resignals_a_running_sole_leader_before_proof() {
        let temp = tempfile::tempdir().unwrap();
        let ready = temp.path().join("macos-running-leader-ready");
        let leak = temp.path().join("macos-running-leader-leaked");
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "printf ready > \"$JIG_VAULT_TEST_READY\"; kill -STOP $$; printf leaked > \"$JIG_VAULT_TEST_LEAK\"",
            ])
            .env("JIG_VAULT_TEST_READY", &ready)
            .env("JIG_VAULT_TEST_LEAK", &leak)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = BrokeredProcess::spawn(&mut command).unwrap();
        wait_for_path(&ready, Duration::from_secs(2));

        let process_group = process.process_group.unwrap().id;
        let confirmation_deadline = Instant::now()
            .checked_add(Duration::from_millis(500))
            .unwrap();
        confirm_unix_process_group_quiescent(&mut process, process_group, confirmation_deadline)
            .unwrap();

        let status = process
            .terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT)
            .unwrap();
        assert_eq!(status.signal(), Some(libc::SIGKILL));
        assert_path_stays_absent(&leak, Duration::from_millis(300));
    }

    #[test]
    fn linux_stat_parser_handles_embedded_closing_delimiter_and_process_states() {
        let live = parse_linux_process_stat("41 (worker ) suffix) S 1 73 0".into()).unwrap();
        let zombie = parse_linux_process_stat("42 (zombie) Z 1 73 0".into()).unwrap();
        let dead = parse_linux_process_stat("43 (dead) X 1 73 0".into()).unwrap();

        assert_eq!(live.process_group, 73);
        assert!(live.live);
        assert!(!zombie.live);
        assert!(!dead.live);
    }

    #[test]
    fn linux_group_classifier_ignores_zombies_but_finds_live_members() {
        let only_zombie = linux_process_group_has_live_members_with(
            73,
            [41],
            |_| Ok("41 (zombie) Z 1 73 0".into()),
            |_| unreachable!(),
            || true,
        )
        .unwrap();
        assert!(!only_zombie);

        let live = linux_process_group_has_live_members_with(
            73,
            [41, 42],
            |pid| {
                Ok(format!(
                    "{pid} (worker) S 1 {} 0",
                    if pid == 42 { 73 } else { 80 }
                ))
            },
            |_| unreachable!(),
            || true,
        )
        .unwrap();
        assert!(live);
    }

    #[test]
    fn linux_group_classifier_fails_closed_for_unreadable_owned_member() {
        let vanished = linux_process_group_has_live_members_with(
            73,
            [41],
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "vanished")),
            |_| Ok(None),
            || true,
        )
        .unwrap();
        assert!(!vanished);

        let unrelated = linux_process_group_has_live_members_with(
            73,
            [42],
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unreadable",
                ))
            },
            |_| Ok(Some(80)),
            || true,
        )
        .unwrap();
        assert!(!unrelated);

        let owned_error = linux_process_group_has_live_members_with(
            73,
            [43],
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unreadable",
                ))
            },
            |_| Ok(Some(73)),
            || true,
        )
        .unwrap_err()
        .to_string();
        assert!(owned_error.contains("belongs to owned process group 73"));
    }

    #[test]
    fn linux_group_classifier_checks_budget_after_stat_read() {
        let within_budget = std::cell::Cell::new(true);
        let membership_checks = std::cell::Cell::new(0usize);
        let error = linux_process_group_has_live_members_with(
            73,
            [41],
            |_| {
                within_budget.set(false);
                Ok("41 (worker) S 1 73 0".into())
            },
            |_| {
                membership_checks.set(membership_checks.get() + 1);
                Ok(Some(73))
            },
            || within_budget.get(),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(membership_checks.get(), 0);
        assert!(error.contains("cleanup scan exceeded its deadline"));
    }

    #[test]
    fn linux_group_enumeration_fails_closed_when_advancing_exhausts_budget() {
        let within_budget = std::cell::Cell::new(true);
        let entries = std::iter::from_fn(|| {
            within_budget.set(false);
            None::<io::Result<i32>>
        });

        let error = collect_linux_process_ids_with(73, entries, Some, || within_budget.get())
            .unwrap_err()
            .to_string();

        assert!(error.contains("cleanup scan exceeded its deadline"));
    }

    #[test]
    fn linux_group_classifier_checks_budget_after_fallback_lookup() {
        let within_budget = std::cell::Cell::new(true);
        let error = linux_process_group_has_live_members_with(
            73,
            [41],
            |_| Err(io::Error::other("injected stat failure")),
            |_| {
                within_budget.set(false);
                Ok(None)
            },
            || within_budget.get(),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("cleanup scan exceeded its deadline"));
    }

    #[test]
    fn linux_group_classifier_checks_budget_before_live_and_empty_results() {
        let live_checks = std::cell::Cell::new(0usize);
        let live_error = linux_process_group_has_live_members_with(
            73,
            [41],
            |_| Ok("41 (worker) S 1 73 0".into()),
            |_| unreachable!(),
            || {
                let check = live_checks.get() + 1;
                live_checks.set(check);
                check <= 3
            },
        )
        .unwrap_err()
        .to_string();
        assert_eq!(live_checks.get(), 4);
        assert!(live_error.contains("cleanup scan exceeded its deadline"));

        let empty_checks = std::cell::Cell::new(0usize);
        let empty_error = linux_process_group_has_live_members_with(
            73,
            std::iter::empty(),
            |_| unreachable!(),
            |_| unreachable!(),
            || {
                let check = empty_checks.get() + 1;
                empty_checks.set(check);
                check == 1
            },
        )
        .unwrap_err()
        .to_string();
        assert_eq!(empty_checks.get(), 2);
        assert!(empty_error.contains("cleanup scan exceeded its deadline"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_brokered_process_is_created_suspended_in_a_new_group() {
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

        let flags = windows_brokered_process_creation_flags();
        assert_eq!(flags, CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
        assert_ne!(flags & CREATE_SUSPENDED, 0);
    }

    #[cfg(windows)]
    #[test]
    fn windows_brokered_job_is_kill_on_close() {
        use std::ffi::c_void;
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::JobObjects::{
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, QueryInformationJobObject,
        };

        let job = create_brokered_process_job().unwrap();
        let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: job and the correctly sized output structure remain live.
        let queried = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle() as HANDLE,
                JobObjectExtendedLimitInformation,
                (&raw mut information).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(queried, 0);
        assert_ne!(
            information.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_brokered_job_terminates_descendant_before_returning_status() {
        let temp = tempfile::tempdir().unwrap();
        let ready = temp.path().join("ready");
        let release = temp.path().join("release");
        let leak = temp.path().join("descendant-survived");
        let output = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec![
                    std::env::current_exe()
                        .unwrap()
                        .into_os_string()
                        .into_string()
                        .unwrap(),
                    "--exact".into(),
                    WINDOWS_JOB_HELPER_TEST.into(),
                    "--nocapture".into(),
                ],
                env: vec![
                    test_env_mapping(WINDOWS_JOB_MODE_VAR, "windows_job_mode", b"parent"),
                    test_env_mapping(
                        WINDOWS_JOB_READY_VAR,
                        "windows_job_ready",
                        ready.as_os_str().as_encoded_bytes(),
                    ),
                    test_env_mapping(
                        WINDOWS_JOB_RELEASE_VAR,
                        "windows_job_release",
                        release.as_os_str().as_encoded_bytes(),
                    ),
                    test_env_mapping(
                        WINDOWS_JOB_LEAK_VAR,
                        "windows_job_leak",
                        leak.as_os_str().as_encoded_bytes(),
                    ),
                ],
                files: Vec::new(),
            },
            Duration::from_secs(2),
        )
        .unwrap();

        assert_eq!(output.exit_status, 7);
        assert!(ready.exists());
        fs::write(release, b"release").unwrap();
        assert_path_stays_absent(&leak, Duration::from_secs(1));
    }

    #[cfg(windows)]
    #[test]
    fn windows_brokered_job_descendant_helper() {
        let Ok(mode) = std::env::var(WINDOWS_JOB_MODE_VAR) else {
            return;
        };
        let ready = std::path::PathBuf::from(std::env::var_os(WINDOWS_JOB_READY_VAR).unwrap());
        let release = std::path::PathBuf::from(std::env::var_os(WINDOWS_JOB_RELEASE_VAR).unwrap());
        let leak = std::path::PathBuf::from(std::env::var_os(WINDOWS_JOB_LEAK_VAR).unwrap());
        match mode.as_str() {
            "parent" => {
                let child = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", WINDOWS_JOB_HELPER_TEST, "--nocapture"])
                    .env(WINDOWS_JOB_MODE_VAR, "descendant")
                    .stdin(Stdio::null())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
                wait_for_path(&ready, Duration::from_secs(2));
                drop(child);
                std::process::exit(7);
            }
            "descendant" => {
                fs::write(ready, b"ready").unwrap();
                let deadline = Instant::now().checked_add(Duration::from_secs(5)).unwrap();
                while !release.exists() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(10));
                }
                if release.exists() {
                    fs::write(leak, b"survived").unwrap();
                }
                std::process::exit(0);
            }
            unexpected => panic!("unexpected Windows Job helper mode {unexpected}"),
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_injects_and_redacts_env_secret() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf '%s' \"$TOKEN\"; printf '%s' \"$TOKEN\" >&2".into(),
            ],
            env: vec![ResolvedBrokeredEnv {
                var: EnvVarName::parse("TOKEN").unwrap(),
                secret_name: SecretName::parse("api_token").unwrap(),
                value: SecretBytes::new(b"secret-value".to_vec()),
            }],
            files: Vec::new(),
        })
        .unwrap();
        assert_eq!(output.exit_status, 0);
        assert_eq!(output.exit_signal, None);
        assert_eq!(output.stdout, "[REDACTED]");
        assert_eq!(output.stderr, "[REDACTED]");
    }

    #[test]
    fn brokered_run_rejects_non_utf8_env_secret() {
        let error = run_brokered(ResolvedBrokeredRun {
            command: vec!["true".into()],
            env: vec![ResolvedBrokeredEnv {
                var: EnvVarName::parse("TOKEN").unwrap(),
                secret_name: SecretName::parse("binary_token").unwrap(),
                value: SecretBytes::new(vec![0xff, 0xfe, 0xfd, 0xfc]),
            }],
            files: Vec::new(),
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("not valid UTF-8"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_rejects_oversized_stdout() {
        let error = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                format!("head -c {} /dev/zero", MAX_CAPTURED_STREAM_BYTES + 1),
            ],
            env: Vec::new(),
            files: Vec::new(),
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("capture limit"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_accepts_exact_capture_limit() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                format!("head -c {MAX_CAPTURED_STREAM_BYTES} /dev/zero"),
            ],
            env: Vec::new(),
            files: Vec::new(),
        })
        .unwrap();

        assert_eq!(output.exit_status, 0);
        assert_eq!(output.stdout.len(), MAX_CAPTURED_STREAM_BYTES);
        assert!(output.stderr.is_empty());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_terminates_other_stream_after_stdout_overflow() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("overflow-descendant-ran");
        let ready = temp.path().join("overflow-descendant-ready");
        let release = temp.path().join("overflow-descendant-release");
        let started = Instant::now();
        let error = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec![
                    "sh".into(),
                    "-c".into(),
                    format!(
                        "(printf ready > \"$JIG_VAULT_TEST_READY\"; while [ ! -e \"$JIG_VAULT_TEST_RELEASE\" ]; do sleep 0.01; done; printf leaked > \"$JIG_VAULT_TEST_MARKER\") >&2 & while [ ! -e \"$JIG_VAULT_TEST_READY\" ]; do sleep 0.01; done; head -c {} /dev/zero",
                        MAX_CAPTURED_STREAM_BYTES + 1
                    ),
                ],
                env: vec![
                    test_env_mapping(
                        "JIG_VAULT_TEST_MARKER",
                        "overflow_marker",
                        marker.as_os_str().as_encoded_bytes(),
                    ),
                    test_env_mapping(
                        "JIG_VAULT_TEST_READY",
                        "overflow_ready",
                        ready.as_os_str().as_encoded_bytes(),
                    ),
                    test_env_mapping(
                        "JIG_VAULT_TEST_RELEASE",
                        "overflow_release",
                        release.as_os_str().as_encoded_bytes(),
                    ),
                ],
                files: Vec::new(),
            },
            Duration::from_secs(2),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("capture limit"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(ready.exists());
        fs::write(release, b"release").unwrap();
        assert_path_stays_absent(&marker, Duration::from_millis(500));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_times_out() {
        let error = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec!["sh".into(), "-c".into(), "sleep 2".into()],
                env: Vec::new(),
                files: Vec::new(),
            },
            Duration::from_millis(20),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("run timeout"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_times_out_after_child_closes_both_pipes() {
        let started = Instant::now();
        let error = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec!["sh".into(), "-c".into(), "exec 1>&- 2>&-; sleep 5".into()],
                env: Vec::new(),
                files: Vec::new(),
            },
            Duration::from_millis(30),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("run timeout"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn zero_run_timeout_wins_even_if_child_can_exit_immediately() {
        let error = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec!["sh".into(), "-c".into(), "exit 0".into()],
                env: Vec::new(),
                files: Vec::new(),
            },
            Duration::ZERO,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("run timeout"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_preserves_nonzero_exit_status() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec!["sh".into(), "-c".into(), "printf ok; exit 7".into()],
            env: Vec::new(),
            files: Vec::new(),
        })
        .unwrap();

        assert_eq!(output.exit_status, 7);
        assert_eq!(output.exit_signal, None);
        assert_eq!(output.stdout, "ok");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_reports_unix_signal_exit_status() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec!["sh".into(), "-c".into(), "kill -TERM $$".into()],
            env: Vec::new(),
            files: Vec::new(),
        })
        .unwrap();
        assert_eq!(output.exit_status, 143);
        assert_eq!(output.exit_signal, Some(15));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_kills_same_group_descendant_before_returning_status() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("same-group-descendant-ran");
        let ready = temp.path().join("same-group-descendant-ready");
        let release = temp.path().join("same-group-descendant-release");
        let started = Instant::now();
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                "(printf ready > \"$JIG_VAULT_TEST_READY\"; while [ ! -e \"$JIG_VAULT_TEST_RELEASE\" ]; do sleep 0.01; done; printf leaked > \"$JIG_VAULT_TEST_MARKER\") & while [ ! -e \"$JIG_VAULT_TEST_READY\" ]; do sleep 0.01; done; printf leader; exit 7".into(),
            ],
            env: vec![
                test_env_mapping(
                    "JIG_VAULT_TEST_MARKER",
                    "descendant_marker",
                    marker.as_os_str().as_encoded_bytes(),
                ),
                test_env_mapping(
                    "JIG_VAULT_TEST_READY",
                    "descendant_ready",
                    ready.as_os_str().as_encoded_bytes(),
                ),
                test_env_mapping(
                    "JIG_VAULT_TEST_RELEASE",
                    "descendant_release",
                    release.as_os_str().as_encoded_bytes(),
                ),
            ],
            files: Vec::new(),
        })
        .unwrap();

        assert_eq!(output.exit_status, 7);
        assert_eq!(output.stdout, "leader");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(ready.exists());
        fs::write(release, b"release").unwrap();
        assert_path_stays_absent(&marker, Duration::from_millis(500));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_repeatedly_cleans_an_immediate_background_wrapper() {
        for iteration in 0..8 {
            let output = run_brokered(ResolvedBrokeredRun {
                command: vec![
                    "sh".into(),
                    "-c".into(),
                    "sleep 5 & printf leader; exit 0".into(),
                ],
                env: Vec::new(),
                files: Vec::new(),
            })
            .unwrap_or_else(|error| panic!("iteration {iteration}: {error:#}"));

            assert_eq!(output.exit_status, 0, "iteration {iteration}");
            assert_eq!(output.stdout, "leader", "iteration {iteration}");
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_bounds_escaped_pipe_holder_and_allows_cooperative_teardown() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("escaped.pid");
        let release = temp.path().join("release");
        let done = temp.path().join("done");
        let started = Instant::now();
        let error = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec![
                    std::env::current_exe()
                        .unwrap()
                        .into_os_string()
                        .into_string()
                        .unwrap(),
                    "--exact".into(),
                    PIPE_ESCAPE_HELPER_TEST.into(),
                    "--nocapture".into(),
                ],
                env: vec![
                    test_env_mapping(PIPE_ESCAPE_MODE_VAR, "escape_mode", b"spawn"),
                    test_env_mapping(
                        PIPE_ESCAPE_MARKER_VAR,
                        "escape_marker",
                        marker.as_os_str().as_encoded_bytes(),
                    ),
                    test_env_mapping(
                        PIPE_ESCAPE_RELEASE_VAR,
                        "escape_release",
                        release.as_os_str().as_encoded_bytes(),
                    ),
                    test_env_mapping(
                        PIPE_ESCAPE_DONE_VAR,
                        "escape_done",
                        done.as_os_str().as_encoded_bytes(),
                    ),
                ],
                files: Vec::new(),
            },
            Duration::from_secs(2),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("output drain"), "unexpected error: {error}");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(marker.exists(), "escaped helper never established itself");
        fs::write(&release, b"release").unwrap();
        wait_for_path(&done, Duration::from_secs(3));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_pipe_escape_helper() {
        let Ok(mode) = std::env::var(PIPE_ESCAPE_MODE_VAR) else {
            return;
        };
        let marker = std::path::PathBuf::from(std::env::var_os(PIPE_ESCAPE_MARKER_VAR).unwrap());
        let release = std::path::PathBuf::from(std::env::var_os(PIPE_ESCAPE_RELEASE_VAR).unwrap());
        let done = std::path::PathBuf::from(std::env::var_os(PIPE_ESCAPE_DONE_VAR).unwrap());
        match mode.as_str() {
            "spawn" => {
                let child = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", PIPE_ESCAPE_HELPER_TEST, "--nocapture"])
                    .env(PIPE_ESCAPE_MODE_VAR, "escaped")
                    .stdin(Stdio::null())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
                wait_for_path(&marker, Duration::from_secs(2));
                drop(child);
                std::process::exit(0);
            }
            "escaped" => {
                // SAFETY: this helper is a non-leader descendant of the
                // brokered setsid leader, so it may deliberately escape into
                // a new session to exercise the bounded-drain boundary.
                assert_ne!(unsafe { libc::setsid() }, -1);
                fs::write(&marker, std::process::id().to_string()).unwrap();
                let deadline = Instant::now().checked_add(Duration::from_secs(5)).unwrap();
                while !release.exists() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(10));
                }
                fs::write(done, b"done").unwrap();
                std::process::exit(0);
            }
            unexpected => panic!("unexpected pipe escape helper mode {unexpected}"),
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn brokered_run_delivers_and_redacts_secret_file() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                "test -f \"$TOKEN_FILE\" && cat \"$TOKEN_FILE\"".into(),
            ],
            env: Vec::new(),
            files: vec![ResolvedBrokeredFile {
                var: EnvVarName::parse("TOKEN_FILE").unwrap(),
                secret_name: SecretName::parse("api_token").unwrap(),
                value: SecretBytes::new(b"secret-value".to_vec()),
            }],
        })
        .unwrap();

        assert_eq!(output.exit_status, 0);
        assert_eq!(output.exit_signal, None);
        assert_eq!(output.stdout, "[REDACTED]");
        assert_eq!(output.stderr, "");
    }

    #[cfg(unix)]
    #[test]
    fn brokered_secret_files_create_owner_only_paths() {
        let files = [ResolvedBrokeredFile {
            var: EnvVarName::parse("TOKEN_FILE").unwrap(),
            secret_name: SecretName::parse("api_token").unwrap(),
            value: SecretBytes::new(b"secret-value".to_vec()),
        }];

        let secret_files = BrokeredSecretFiles::create(&files).unwrap().unwrap();
        let file_path = std::path::PathBuf::from(secret_files.env()[0].1.clone());
        let dir_path = file_path.parent().unwrap();

        assert_eq!(
            fs::metadata(dir_path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(file_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn wipe_secret_file_overwrites_contents_before_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("secret");
        fs::write(&path, b"secret-value").unwrap();
        let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();

        wipe_secret_file(&mut file, &path).unwrap();

        assert_eq!(fs::read(&path).unwrap(), vec![0_u8; "secret-value".len()]);
    }
}
