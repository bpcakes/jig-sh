use std::ffi::OsString;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use zeroize::{Zeroize, Zeroizing};

use crate::exec::{VAULT_NEW_PASSPHRASE_ENV, VAULT_PASSPHRASE_ENV};
use crate::exec_output::StreamingRedactor;
use crate::{EnvVarName, ExecOutcome};

const EXEC_POLL_INTERVAL: Duration = Duration::from_millis(5);
const EXEC_FINAL_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_READS_PER_POLL: usize = 64;
const EXEC_READ_CHUNK_LEN: usize = 8 * 1024;

pub(crate) struct ResolvedExecEnv {
    var: EnvVarName,
    value: Zeroizing<String>,
}

impl ResolvedExecEnv {
    pub(crate) fn new(var: EnvVarName, value: Zeroizing<String>) -> Self {
        Self { var, value }
    }
}

impl std::fmt::Debug for ResolvedExecEnv {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedExecEnv")
            .field("var", &self.var)
            .field("value_len", &self.value.len())
            .field("value", &"[REDACTED]")
            .finish()
    }
}

pub(crate) struct ResolvedExecProcess {
    command: Vec<OsString>,
    env: Vec<ResolvedExecEnv>,
    redactor: StreamingRedactor,
}

impl ResolvedExecProcess {
    pub(crate) fn new(
        command: Vec<OsString>,
        env: Vec<ResolvedExecEnv>,
        redactor: StreamingRedactor,
    ) -> Self {
        Self {
            command,
            env,
            redactor,
        }
    }
}

impl std::fmt::Debug for ResolvedExecProcess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedExecProcess")
            .field("argument_count", &self.command.len())
            .field("arguments", &"[REDACTED]")
            .field("environment_count", &self.env.len())
            .field("environment_values", &"[REDACTED]")
            .field("redactor", &self.redactor)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) struct ExecProcessFailure {
    stage: &'static str,
    error: io::Error,
}

impl ExecProcessFailure {
    pub(crate) const fn stage(&self) -> &'static str {
        self.stage
    }

    pub(crate) fn into_error(self) -> anyhow::Error {
        self.error.into()
    }
}

pub(crate) fn run_exec_process(
    request: ResolvedExecProcess,
) -> std::result::Result<ExecOutcome, ExecProcessFailure> {
    let stdout = io::stdout();
    let stderr = io::stderr();
    run_exec_process_with_writers(request, &mut stdout.lock(), &mut stderr.lock())
}

fn run_exec_process_with_writers(
    request: ResolvedExecProcess,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::result::Result<ExecOutcome, ExecProcessFailure> {
    let ResolvedExecProcess {
        command,
        env,
        redactor,
    } = request;
    let mut process = Command::new(&command[0]);
    process
        .args(&command[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    strip_passphrase_environment(&mut process);
    for binding in &env {
        process.env(binding.var.as_str(), binding.value.as_str());
    }

    let mut child = process
        .spawn()
        .map_err(|error| sanitized_failure("spawn", error))?;
    // `Command` retains owned environment strings until it is dropped. Clear
    // those copies as soon as spawn has transferred the environment to the OS.
    drop(process);
    drop(env);

    let stdout_pipe = child.stdout.take().ok_or_else(|| {
        cleanup_spawned_child(
            &mut child,
            sanitized_failure(
                "pipe",
                io::Error::other("vault exec stdout pipe was unavailable"),
            ),
        )
    })?;
    let stderr_pipe = child.stderr.take().ok_or_else(|| {
        cleanup_spawned_child(
            &mut child,
            sanitized_failure(
                "pipe",
                io::Error::other("vault exec stderr pipe was unavailable"),
            ),
        )
    })?;

    let mut stdout_pump = StreamPump::new(
        ProcessPipe::Stdout(stdout_pipe),
        redactor.independent_stream(),
    )
    .map_err(|error| cleanup_spawned_child(&mut child, error))?;
    let mut stderr_pump = StreamPump::new(ProcessPipe::Stderr(stderr_pipe), redactor)
        .map_err(|error| cleanup_spawned_child(&mut child, error))?;

    let status = loop {
        stdout_pump.poll(stdout);
        stderr_pump.poll(stderr);
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(EXEC_POLL_INTERVAL),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                let failure = sanitized_failure("wait", error);
                let _ = child.wait();
                return Err(failure);
            }
        }
    };

    let deadline = Instant::now()
        .checked_add(EXEC_FINAL_DRAIN_TIMEOUT)
        .unwrap_or_else(Instant::now);
    while !(stdout_pump.is_terminal() && stderr_pump.is_terminal()) && Instant::now() < deadline {
        stdout_pump.poll(stdout);
        stderr_pump.poll(stderr);
        if !(stdout_pump.is_terminal() && stderr_pump.is_terminal()) {
            thread::sleep(EXEC_POLL_INTERVAL);
        }
    }
    // A descendant may retain a pipe after the direct child exits. Abandoning
    // a nonterminal pump drops both the reader and its withheld overlap bytes;
    // it never emits an unverified partial secret prefix.
    stdout_pump.abandon_if_open();
    stderr_pump.abandon_if_open();

    if let Some(failure) = stdout_pump
        .take_failure()
        .or_else(|| stderr_pump.take_failure())
    {
        return Err(failure);
    }
    Ok(portable_outcome(status))
}

fn strip_passphrase_environment(command: &mut Command) {
    #[cfg(windows)]
    {
        for (name, _) in std::env::vars_os() {
            let spelling = name.to_string_lossy();
            if spelling.eq_ignore_ascii_case(VAULT_PASSPHRASE_ENV)
                || spelling.eq_ignore_ascii_case(VAULT_NEW_PASSPHRASE_ENV)
            {
                command.env_remove(name);
            }
        }
    }
    #[cfg(not(windows))]
    {
        command.env_remove(VAULT_PASSPHRASE_ENV);
        command.env_remove(VAULT_NEW_PASSPHRASE_ENV);
    }
}

fn cleanup_spawned_child(child: &mut Child, failure: ExecProcessFailure) -> ExecProcessFailure {
    let _ = child.kill();
    let _ = child.wait();
    failure
}

fn sanitized_failure(stage: &'static str, error: io::Error) -> ExecProcessFailure {
    ExecProcessFailure {
        stage,
        error: io::Error::new(
            error.kind(),
            format!("vault exec process {stage} failed ({:?})", error.kind()),
        ),
    }
}

fn portable_outcome(status: ExitStatus) -> ExecOutcome {
    #[cfg(unix)]
    {
        let signal = status.signal();
        let exit_status = status.code().unwrap_or_else(|| {
            128_i32
                .saturating_add(signal.unwrap_or(0))
                .min(u8::MAX as i32)
        });
        ExecOutcome {
            exit_status,
            exit_signal: signal,
        }
    }
    #[cfg(not(unix))]
    {
        ExecOutcome {
            exit_status: status.code().unwrap_or(1),
            exit_signal: None,
        }
    }
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
        // SAFETY: the descriptor is owned by this live pipe; F_GETFL only
        // reads its flags and F_SETFL preserves them while adding O_NONBLOCK.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }
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
            "transparent vault process pipes are unsupported on this platform",
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
        // SAFETY: `handle` is a live anonymous-pipe read handle and the only
        // output pointer names writable `u32` storage.
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
            "transparent vault process pipes are unsupported on this platform",
        ))
    }
}

struct StreamPump {
    reader: Option<ProcessPipe>,
    redactor: Option<StreamingRedactor>,
    failure: Option<ExecProcessFailure>,
}

impl StreamPump {
    fn new(reader: ProcessPipe, redactor: StreamingRedactor) -> Result<Self, ExecProcessFailure> {
        reader
            .prepare()
            .map_err(|error| sanitized_failure("pipe", error))?;
        Ok(Self {
            reader: Some(reader),
            redactor: Some(redactor),
            failure: None,
        })
    }

    fn poll(&mut self, writer: &mut dyn Write) {
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        let mut buffer = Zeroizing::new([0_u8; EXEC_READ_CHUNK_LEN]);
        for _ in 0..MAX_READS_PER_POLL {
            match reader.read_available(&mut buffer[..]) {
                Ok(0) => {
                    self.reader = None;
                    if self.failure.is_none() {
                        if let Some(redactor) = self.redactor.take() {
                            if let Err(error) =
                                redactor.finish(writer).and_then(|()| writer.flush())
                            {
                                self.failure = Some(sanitized_failure("output", error));
                            }
                        }
                    } else {
                        self.redactor = None;
                    }
                    buffer.zeroize();
                    return;
                }
                Ok(read) => {
                    if self.failure.is_none() {
                        let result = self
                            .redactor
                            .as_mut()
                            .expect("active stream retains its redactor")
                            .push_chunk(&buffer[..read], writer)
                            .and_then(|()| writer.flush());
                        if let Err(error) = result {
                            self.failure = Some(sanitized_failure("output", error));
                            self.redactor = None;
                        }
                    }
                    buffer[..read].zeroize();
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) => {
                    self.reader = None;
                    self.redactor = None;
                    self.failure
                        .get_or_insert_with(|| sanitized_failure("read", error));
                    buffer.zeroize();
                    return;
                }
            }
        }
    }

    fn is_terminal(&self) -> bool {
        self.reader.is_none()
    }

    fn abandon_if_open(&mut self) {
        if self.reader.is_some() {
            self.reader = None;
            self.redactor = None;
        }
    }

    fn take_failure(&mut self) -> Option<ExecProcessFailure> {
        self.failure.take()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn request(command: Vec<OsString>) -> ResolvedExecProcess {
        ResolvedExecProcess::new(
            command,
            Vec::new(),
            StreamingRedactor::new(Vec::new()).unwrap(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_and_signal_statuses_are_portable() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let nonzero = run_exec_process_with_writers(
            request(vec!["sh".into(), "-c".into(), "exit 37".into()]),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(nonzero.exit_status, 37);
        assert_eq!(nonzero.exit_signal, None);

        let signal = run_exec_process_with_writers(
            request(vec!["sh".into(), "-c".into(), "kill -TERM $$".into()]),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(signal.exit_status, 143);
        assert_eq!(signal.exit_signal, Some(libc::SIGTERM));
    }

    #[cfg(unix)]
    #[test]
    fn descendant_holding_pipe_is_bounded_after_leader_exit() {
        let started = Instant::now();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let outcome = run_exec_process_with_writers(
            request(vec![
                "sh".into(),
                "-c".into(),
                "(sleep 2) & printf done".into(),
            ]),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(outcome.exit_status, 0);
        assert_eq!(stdout, b"done");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn output_failure_still_reaps_a_large_writer() {
        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut broken = BrokenWriter;
        let mut stderr = Vec::new();
        let started = Instant::now();
        let error = run_exec_process_with_writers(
            request(vec![
                "sh".into(),
                "-c".into(),
                "head -c 2097152 /dev/zero".into(),
            ]),
            &mut broken,
            &mut stderr,
        )
        .unwrap_err();
        assert_eq!(error.stage(), "output");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
