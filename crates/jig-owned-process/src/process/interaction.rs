use std::io::Read;
use std::process::{ChildStderr, ChildStdin, Command};
use std::time::{Duration, Instant};

use super::{
    BoundedProcessOutput, OWNED_PROCESS_OUTPUT_LIMIT, OutputDrain, OwnedProcess,
    OwnedProcessTreeError, ProcessPipe, spawn_owned_process,
};

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
    pipe: ProcessPipe,
    stderr: Option<OutputDrain>,
}

impl ProcessInteractionStdout {
    fn new(
        stdout: std::process::ChildStdout,
        stderr: Option<ChildStderr>,
    ) -> std::io::Result<Self> {
        let pipe = ProcessPipe::Stdout(stdout);
        pipe.prepare()?;
        let stderr = stderr
            .map(|stderr| {
                OutputDrain::start(ProcessPipe::Stderr(stderr), OWNED_PROCESS_OUTPUT_LIMIT)
            })
            .transpose()?;
        Ok(Self { pipe, stderr })
    }

    /// Finishes the bounded stderr preview captured while stdout was polled.
    pub fn take_stderr_output(&mut self) -> Option<BoundedProcessOutput> {
        if let Some(stderr) = &mut self.stderr {
            stderr.poll();
        }
        self.stderr.take().map(OutputDrain::finish)
    }
}

impl Read for ProcessInteractionStdout {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if let Some(stderr) = &mut self.stderr {
            stderr.poll();
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
    let mut process = spawn_owned_process(command)
        .map_err(OwnedProcessTreeError::Start)
        .map_err(OwnedProcessTreeInteractionError::Process)?;
    let Some(stdin) = process.child.stdin.take() else {
        return cleanup_failed_interaction(
            &mut process,
            "the child stdin was not configured as a pipe",
        );
    };
    let Some(stdout) = process.child.stdout.take() else {
        drop(stdin);
        return cleanup_failed_interaction(
            &mut process,
            "the child stdout was not configured as a pipe",
        );
    };
    let stderr = process.child.stderr.take();
    let stdout = match ProcessInteractionStdout::new(stdout, stderr) {
        Ok(stdout) => stdout,
        Err(error) => {
            drop(stdin);
            return cleanup_failed_interaction(
                &mut process,
                &format!("the child stdout could not be prepared for bounded reads: {error}"),
            );
        }
    };

    let deadline = Instant::now().checked_add(timeout);
    let outcome = interaction(stdin, stdout, deadline);
    finish_interaction(outcome, process.terminate_and_reap().map(|_| ()))
}

fn cleanup_failed_interaction<T>(
    process: &mut OwnedProcess,
    message: &str,
) -> std::result::Result<T, OwnedProcessTreeInteractionError> {
    finish_interaction(
        Err(message.into()),
        process.terminate_and_reap().map(|_| ()),
    )
}

fn finish_interaction<T>(
    outcome: std::result::Result<T, String>,
    cleanup: std::io::Result<()>,
) -> std::result::Result<T, OwnedProcessTreeInteractionError> {
    match (outcome, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(OwnedProcessTreeInteractionError::Interaction(error)),
        (Ok(_), Err(_)) => Err(OwnedProcessTreeInteractionError::Process(
            OwnedProcessTreeError::Cleanup,
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
            Duration::from_millis(100),
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
            started.elapsed() < Duration::from_secs(1),
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
}
