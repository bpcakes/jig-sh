use std::io::Read;
use std::process::{ChildStderr, ChildStdin, Command, ExitStatus};
use std::time::{Duration, Instant};

use crate::{ChildPipe, NonblockingPipe};

use super::{
    BoundedProcessOutput, OWNED_PROCESS_OUTPUT_LIMIT, OutputDrain, OwnedProcess,
    OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError, ProcessDeadline,
    spawn_owned_process,
};

struct IgnoreProcessActivity;

impl OwnedProcessObserver for IgnoreProcessActivity {}

#[derive(Debug)]
pub enum OwnedProcessTreeInteractionError {
    Process(OwnedProcessTreeError),
    Interaction(String),
    InteractionAndCleanup(String),
}

impl std::fmt::Display for OwnedProcessTreeInteractionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process(error) => error.fmt(formatter),
            Self::Interaction(error) => {
                write!(formatter, "the process interaction failed: {error}")
            }
            Self::InteractionAndCleanup(error) => write!(
                formatter,
                "the process tree could not be cleaned up safely; the process interaction also failed: {error}"
            ),
        }
    }
}

impl std::error::Error for OwnedProcessTreeInteractionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Process(error) => Some(error),
            Self::Interaction(_) | Self::InteractionAndCleanup(_) => None,
        }
    }
}

pub struct ProcessInteractionStdout {
    pipe: NonblockingPipe,
    stderr: Option<OutputDrain>,
}

impl ProcessInteractionStdout {
    fn new(
        stdout: std::process::ChildStdout,
        stderr: Option<ChildStderr>,
    ) -> std::io::Result<Self> {
        let pipe = ChildPipe::Stdout(stdout)
            .prepare("nonblocking process-pipe reads are unavailable on this platform")?;
        let stderr = stderr
            .map(|stderr| OutputDrain::start(ChildPipe::Stderr(stderr), OWNED_PROCESS_OUTPUT_LIMIT))
            .transpose()?;
        Ok(Self { pipe, stderr })
    }

    /// Finishes the bounded stderr preview captured while stdout was polled.
    pub fn take_stderr_output(&mut self) -> Option<BoundedProcessOutput> {
        if let Some(stderr) = &mut self.stderr {
            stderr.poll(OwnedProcessOutputStream::Stderr, &mut IgnoreProcessActivity);
        }
        self.stderr.take().map(OutputDrain::finish)
    }
}

impl Read for ProcessInteractionStdout {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if let Some(stderr) = &mut self.stderr {
            stderr.poll(OwnedProcessOutputStream::Stderr, &mut IgnoreProcessActivity);
        }
        self.pipe.read_available(buffer)
    }
}

/// Runs a cooperatively deadline-bounded exchange against an owned child process.
///
/// The interaction owns the child's stdin and nonblocking stdout and must honor
/// the supplied absolute deadline; this function cannot preempt a synchronous
/// closure that ignores it. Callers should keep writes small because stdin
/// remains a blocking pipe. Returning ends the exchange; the process tree is
/// then terminated and reaped so long-lived protocol servers cannot leak descendants.
pub fn run_owned_process_tree_with_cooperative_interaction<T, F>(
    command: &mut Command,
    timeout: Duration,
    interaction: F,
) -> std::result::Result<T, OwnedProcessTreeInteractionError>
where
    F: FnOnce(
        ChildStdin,
        ProcessInteractionStdout,
        Option<Instant>,
    ) -> std::result::Result<T, String>,
{
    let (mut process, stdin, stdout) = start(command)?;
    let deadline = ProcessDeadline::after(timeout);
    let outcome = interaction(stdin, stdout, deadline.as_optional_instant());
    finish_interaction(outcome, process.terminate_and_reap().map(|_| ()))
}

/// Runs a finite command whose stdout the caller must read with its own buffering.
/// After the exchange succeeds, waits for the direct child's natural exit before
/// tree cleanup. Cancellation and the original deadline apply even after stdout
/// closes. As with cooperative interaction, the callback must poll them while reading.
pub fn run_owned_process_tree_with_cooperative_interaction_and_status<T>(
    command: &mut Command,
    timeout: Duration,
    mut cancelled: impl FnMut() -> bool,
    interaction: impl FnOnce(
        ChildStdin,
        &mut ProcessInteractionStdout,
        Option<Instant>,
    ) -> Result<T, OwnedProcessTreeError>,
) -> Result<(T, ExitStatus), OwnedProcessTreeInteractionError> {
    use super::{
        CancellationObserver, OwnedProcessOutputDrains, ProcessOutputOverflowPolicy,
        finish_owned_process_wait, wait_for_owned_process,
    };

    if cancelled() {
        return Err(OwnedProcessTreeInteractionError::Process(
            OwnedProcessTreeError::CancelledBeforeStart,
        ));
    }
    let (mut process, stdin, mut stdout) = start(command)?;
    let deadline = ProcessDeadline::after(timeout);
    let value = match interaction(stdin, &mut stdout, deadline.as_optional_instant()) {
        Ok(value) => value,
        Err(error) => {
            process
                .terminate_and_reap()
                .map_err(OwnedProcessTreeError::Cleanup)
                .map_err(OwnedProcessTreeInteractionError::Process)?;
            return Err(OwnedProcessTreeInteractionError::Process(error));
        }
    };
    // EOF on stdout can precede the final diagnostic and exit. Keep draining
    // piped stderr and observing the pinned child through the original deadline.
    let wait = wait_for_owned_process(
        &mut process,
        deadline,
        ProcessOutputOverflowPolicy::Truncate,
        &mut CancellationObserver(&mut cancelled),
        &mut OwnedProcessOutputDrains::remaining_stderr(stdout.stderr.take()),
    );
    finish_owned_process_wait(&mut process, wait)
        .map(|status| (value, status))
        .map_err(OwnedProcessTreeInteractionError::Process)
}

fn start(
    command: &mut Command,
) -> Result<(OwnedProcess, ChildStdin, ProcessInteractionStdout), OwnedProcessTreeInteractionError>
{
    let mut process = spawn_owned_process(command)
        .map_err(OwnedProcessTreeError::Start)
        .map_err(OwnedProcessTreeInteractionError::Process)?;
    match prepare_pipes(&mut process) {
        Ok((stdin, stdout)) => Ok((process, stdin, stdout)),
        Err(error) => finish_interaction(Err(error), process.terminate_and_reap().map(|_| ())),
    }
}

fn prepare_pipes(
    process: &mut OwnedProcess,
) -> Result<(ChildStdin, ProcessInteractionStdout), String> {
    let stdin = process
        .child
        .stdin
        .take()
        .ok_or("the child stdin was not configured as a pipe")?;
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or("the child stdout was not configured as a pipe")?;
    let stdout =
        ProcessInteractionStdout::new(stdout, process.child.stderr.take()).map_err(|error| {
            format!("the child stdout could not be prepared for bounded reads: {error}")
        })?;
    Ok((stdin, stdout))
}

fn finish_interaction<T>(
    outcome: std::result::Result<T, String>,
    cleanup: std::io::Result<()>,
) -> std::result::Result<T, OwnedProcessTreeInteractionError> {
    match (outcome, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(_)) => Err(OwnedProcessTreeInteractionError::Interaction(error)),
        (Ok(_), Err(error)) => Err(OwnedProcessTreeInteractionError::Process(
            OwnedProcessTreeError::Cleanup(error),
        )),
        (Err(error), Err(_)) => Err(OwnedProcessTreeInteractionError::InteractionAndCleanup(
            error,
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use super::{
        OwnedProcessTreeInteractionError, finish_interaction,
        run_owned_process_tree_with_cooperative_interaction,
        run_owned_process_tree_with_cooperative_interaction_and_status,
    };
    use crate::test_process::{read_test_process_identity, terminate_and_confirm_test_process};

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn owned_process_interaction_deadline_survives_an_escaped_stdout_owner() {
        let temp = tempdir().unwrap();
        let marker = temp.path().join("escaped-interaction-output-owner");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "process::tests::owned_process_output_escape_helper",
                "--nocapture",
            ])
            .env("JIG_OWNED_OUTPUT_ESCAPE_HELPER", "spawn")
            .env("JIG_OWNED_OUTPUT_ESCAPE_MARKER", &marker)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let started = Instant::now();

        let error = run_owned_process_tree_with_cooperative_interaction(
            &mut command,
            Duration::from_millis(500),
            |_stdin, mut stdout, deadline| {
                let mut buffer = [0_u8; 4096];
                loop {
                    match stdout.read(&mut buffer) {
                        Ok(0) => return Ok(()),
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                                return Err("interaction timed out".into());
                            }
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => return Err(error.to_string()),
                    }
                }
            },
        )
        .unwrap_err();

        let escaped = read_test_process_identity(&marker);
        terminate_and_confirm_test_process(&escaped);
        assert!(error.to_string().contains("timed out"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "escaped stdout ownership exceeded the interaction deadline"
        );
    }

    #[test]
    fn cleanup_failure_retains_the_interaction_error() {
        let error = finish_interaction::<()>(
            Err("specific protocol failure".into()),
            Err(std::io::Error::other("cleanup failure")),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            OwnedProcessTreeInteractionError::InteractionAndCleanup(_)
        ));
        let rendered = error.to_string();
        assert!(rendered.contains("could not be cleaned up safely"));
        assert!(rendered.contains("specific protocol failure"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn interaction_status_reports_the_child_exit_after_end_of_output() {
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf done; exec 1>&-; sleep 0.1; echo finished >&2; exit 7",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let (output, status) = run_owned_process_tree_with_cooperative_interaction_and_status(
            &mut command,
            Duration::from_secs(10),
            || false,
            |stdin, stdout, deadline| {
                drop(stdin);
                let mut output = Vec::new();
                let mut buffer = [0_u8; 64];
                loop {
                    match stdout.read(&mut buffer) {
                        Ok(0) => return Ok(output),
                        Ok(count) => output.extend_from_slice(&buffer[..count]),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                                return Err(crate::OwnedProcessTreeError::TimedOut);
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => return Err(crate::OwnedProcessTreeError::Await),
                    }
                }
            },
        )
        .unwrap();

        assert_eq!(output, b"done");
        assert_eq!(status.code(), Some(7));
    }
}
