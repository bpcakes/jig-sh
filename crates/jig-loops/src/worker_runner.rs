use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use jig_context::{CommandTimeout, MAX_COMMAND_TIMEOUT_SECONDS, RepoContext};
use jig_owned_process::{
    BoundedProcessOutput, OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError,
    ProcessOutputLimits, ProcessOutputOverflowPolicy,
    run_owned_process_tree_with_output_policy_and_observer,
};
use jig_state::now_ms;
use serde_json::{Value, json};
use tempfile::NamedTempFile;

use jig_execution::{
    EXECUTION_OUTPUT_CAPTURE_LIMIT, ExecutionCommandError, ExecutionControl, ExecutionPhase,
    PhasePosition, ProcessExecutionObserver,
};

const CODEX_TIMEOUT_ENV: &str = "JIG_CODEX_TIMEOUT_SECS";
const WORKER_PROVIDER_PREVIEW_BYTES: usize = 4_000;
// Preserve the supervisor's normal idle responsiveness without repeating a
// metadata syscall for every faster poll while transcript output is flowing.
const WORKER_RESULT_FILE_INSPECTION_INTERVAL: Duration = Duration::from_millis(10);

/// Identifies a worker run in its evidence and process label.
#[derive(Clone, Copy, Debug)]
pub struct WorkerRunLabel<'a> {
    pub purpose: &'a str,
    pub workflow_id: Option<&'a str>,
    pub item_key: Option<&'a str>,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkerPhase<'a> {
    pub label: &'a str,
    pub position: PhasePosition,
}

pub struct CodexExecRequest<'a> {
    pub root: &'a Path,
    pub codex_home: Option<&'a Path>,
    pub model: Option<&'a str>,
    pub approval_policy: Option<&'a str>,
    pub sandbox: Option<&'a str>,
    pub ephemeral: bool,
    pub extra_args: Vec<OsString>,
    pub output_schema: Option<&'a Value>,
    pub transcript_overflow_policy: ProcessOutputOverflowPolicy,
    /// Delivered to `codex exec` on stdin.
    pub prompt: &'a str,
    pub run: WorkerRunLabel<'a>,
    pub phase: Option<WorkerPhase<'a>>,
}

pub struct CodexExecOutput {
    output: Output,
    provider_stdout: String,
    provider_stdout_truncated: bool,
    evidence: Value,
}

impl CodexExecOutput {
    pub fn status(&self) -> &std::process::ExitStatus {
        &self.output.status
    }

    pub fn authoritative_stdout(&self) -> &[u8] {
        &self.output.stdout
    }

    pub fn provider_stdout(&self) -> &str {
        &self.provider_stdout
    }

    pub fn provider_stdout_truncated(&self) -> bool {
        self.provider_stdout_truncated
    }

    /// What ran and how it ended, for the loop occurrence's evidence.
    pub fn evidence(&self) -> &Value {
        &self.evidence
    }
}

#[derive(Debug)]
pub struct CodexExecFailure {
    evidence: Value,
    unexecuted: bool,
    message: String,
}

impl CodexExecFailure {
    pub fn evidence(&self) -> &Value {
        &self.evidence
    }

    pub fn worker_was_unexecuted(&self) -> bool {
        self.unexecuted
    }
}

impl fmt::Display for CodexExecFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CodexExecFailure {}

pub enum CodexExecOutcome {
    Completed(CodexExecOutput),
    Cancelled { before_start: bool, evidence: Value },
}

pub fn run_codex_exec(
    ctx: &RepoContext,
    request: CodexExecRequest<'_>,
    observer: &mut dyn ExecutionControl,
) -> Result<CodexExecOutcome> {
    let phase = request
        .phase
        .map(|phase| ExecutionPhase::start(observer, phase.label, phase.position));
    let started = now_ms();
    let result = run_codex_exec_inner(ctx, &request, observer);
    let ended = now_ms();
    if let Some(phase) = phase {
        phase.finish(
            observer,
            result.as_ref().is_ok_and(|run| run.output.status.success()),
        );
    }

    match result {
        Ok(run) => {
            let exit_status = run.output.status.code().unwrap_or(1);
            let evidence = worker_evidence(
                &request,
                WorkerRunOutcome {
                    started_at_ms: started,
                    ended_at_ms: ended,
                    exit_status,
                    stderr: &run.provider_stderr,
                    provider_stdout: Some(&run.provider_stdout),
                    provider_stdout_truncated: run.provider_stdout_truncated,
                    provider_stderr_truncated: run.provider_stderr_truncated,
                    error: None,
                    status: "completed",
                },
            );
            Ok(CodexExecOutcome::Completed(CodexExecOutput {
                output: run.output,
                provider_stdout: run.provider_stdout,
                provider_stdout_truncated: run.provider_stdout_truncated,
                evidence,
            }))
        }
        Err(
            error
            @ (ExecutionCommandError::CancelledBeforeStart | ExecutionCommandError::Cancelled),
        ) => {
            let before_start = matches!(error, ExecutionCommandError::CancelledBeforeStart);
            let message = format!("{error:#}");
            let evidence = worker_evidence(
                &request,
                WorkerRunOutcome {
                    started_at_ms: started,
                    ended_at_ms: ended,
                    exit_status: 1,
                    stderr: &message,
                    provider_stdout: None,
                    provider_stdout_truncated: false,
                    provider_stderr_truncated: false,
                    error: Some(&message),
                    status: "cancelled",
                },
            );
            Ok(CodexExecOutcome::Cancelled {
                before_start,
                evidence,
            })
        }
        Err(ExecutionCommandError::Failed {
            error,
            process_started,
        }) => {
            let message = format!("{error:#}");
            let evidence = worker_evidence(
                &request,
                WorkerRunOutcome {
                    started_at_ms: started,
                    ended_at_ms: ended,
                    exit_status: 1,
                    stderr: &message,
                    provider_stdout: None,
                    provider_stdout_truncated: false,
                    provider_stderr_truncated: false,
                    error: Some(&message),
                    status: "error",
                },
            );
            Err(CodexExecFailure {
                evidence,
                unexecuted: !process_started,
                message: format!("Codex worker invocation failed: {message}"),
            }
            .into())
        }
    }
}

struct CodexRunOutput {
    output: Output,
    provider_stdout: String,
    provider_stderr: String,
    provider_stdout_truncated: bool,
    provider_stderr_truncated: bool,
}

fn run_codex_exec_inner(
    ctx: &RepoContext,
    request: &CodexExecRequest<'_>,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<CodexRunOutput, ExecutionCommandError> {
    let schema_file = if let Some(schema) = request.output_schema {
        let schema_file = NamedTempFile::new().context("Failed to create Codex schema file")?;
        fs::write(
            schema_file.path(),
            serde_json::to_vec_pretty(schema).context("Failed to encode Codex schema JSON")?,
        )
        .context("Failed to write Codex schema file")?;
        Some(schema_file)
    } else {
        None
    };
    // The last-message file is the authoritative result channel for every
    // worker. Schema validation is optional and must not decide whether noisy
    // provider transcripts are allowed to truncate.
    let output_file = NamedTempFile::new().context("Failed to create Codex output file")?;

    let mut command = build_codex_command(
        jig_agents::codex::codex_bin(),
        request,
        schema_file.as_ref().map(NamedTempFile::path),
        output_file.path(),
    );
    let output = run_worker_command(
        &mut command,
        Some(request.prompt),
        codex_timeout(ctx)?,
        request.run.purpose,
        request.transcript_overflow_policy,
        Some(output_file.path()),
        observer,
    )?;
    let provider_stdout = String::from_utf8_lossy(&output.output.stdout).into_owned();
    let provider_stderr = String::from_utf8_lossy(&output.output.stderr).into_owned();
    let provider_stdout_truncated = output.provider_stdout_truncated;
    let provider_stderr_truncated = output.provider_stderr_truncated;
    let mut output = output.output;

    output.stdout = read_worker_output_file(output_file.path())
        .map_err(ExecutionCommandError::failed_after_start)?
        .unwrap_or_default();

    Ok(CodexRunOutput {
        output,
        provider_stdout,
        provider_stderr,
        provider_stdout_truncated,
        provider_stderr_truncated,
    })
}

fn read_worker_output_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let output_metadata = path
        .metadata()
        .context("Failed to inspect Codex output file")?;
    if output_metadata.len() == 0 {
        return Ok(None);
    }
    if output_metadata.len() > EXECUTION_OUTPUT_CAPTURE_LIMIT as u64 {
        bail!(
            "Codex last-message output exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
        );
    }
    fs::read(path)
        .map(Some)
        .context("Failed to read Codex output file")
}

fn build_codex_command(
    bin: impl AsRef<OsStr>,
    request: &CodexExecRequest<'_>,
    schema_path: Option<&Path>,
    output_path: &Path,
) -> Command {
    let mut command = Command::new(bin);
    command.current_dir(request.root);
    if let Some(codex_home) = request.codex_home {
        command.env(jig_agents::codex::CODEX_HOME_ENV, codex_home);
    }
    if let Some(approval_policy) = request.approval_policy {
        command.arg("--ask-for-approval").arg(approval_policy);
    }
    command.arg("exec");
    if let Some(sandbox) = request.sandbox {
        command.arg("--sandbox").arg(sandbox);
    }
    if request.ephemeral {
        command.arg("--ephemeral");
    }
    command.args(&request.extra_args);
    if let Some(model) = request.model {
        command.arg("--model").arg(model);
    }
    if let Some(schema_path) = schema_path {
        command.arg("--output-schema").arg(schema_path);
    }
    command.arg("-o").arg(output_path);
    // The prompt is written to the worker's stdin.
    command.arg("-");
    command
}

fn run_worker_command(
    command: &mut Command,
    stdin_prompt: Option<&str>,
    timeout: CommandTimeout,
    label: &str,
    transcript_overflow_policy: ProcessOutputOverflowPolicy,
    authoritative_output_path: Option<&Path>,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<WorkerCommandOutput, ExecutionCommandError> {
    let prompt_file = stdin_prompt
        .map(|prompt| -> Result<NamedTempFile> {
            let file = NamedTempFile::new().context("Failed to create worker stdin file")?;
            fs::write(file.path(), prompt).context("Failed to write worker prompt")?;
            command.stdin(file.reopen().context("Failed to open worker stdin file")?);
            Ok(file)
        })
        .transpose()?;
    let _prompt_file = prompt_file;
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let mut process_observer = WorkerProcessObserver::new(
        ProcessExecutionObserver::new(observer, label),
        authoritative_output_path,
    );
    let process_result = run_owned_process_tree_with_output_policy_and_observer(
        command,
        timeout.duration(),
        ProcessOutputLimits {
            stdout: EXECUTION_OUTPUT_CAPTURE_LIMIT,
            stderr: EXECUTION_OUTPUT_CAPTURE_LIMIT,
        },
        transcript_overflow_policy,
        &mut process_observer,
    );
    let result_file_failure = process_observer.take_result_file_failure();
    let output = match (process_result, result_file_failure) {
        (Ok(_), Some(failure)) => return Err(failure.into_execution_error(true)),
        (Err(OwnedProcessTreeError::CancelledBeforeStart), Some(failure)) => {
            return Err(failure.into_execution_error(false));
        }
        (Err(OwnedProcessTreeError::Cancelled), Some(failure)) => {
            return Err(failure.into_execution_error(true));
        }
        (Ok(output), None) => output,
        (Err(error), _) => return Err(worker_process_error(error, timeout)),
    };

    let stdout = complete_worker_output(output.stdout, "stdout")
        .map_err(ExecutionCommandError::failed_after_start)?;
    let stderr = complete_worker_output(output.stderr, "stderr")
        .map_err(ExecutionCommandError::failed_after_start)?;
    Ok(WorkerCommandOutput {
        output: Output {
            status: output.status,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
        },
        provider_stdout_truncated: stdout.truncated,
        provider_stderr_truncated: stderr.truncated,
    })
}

struct WorkerProcessObserver<'a> {
    execution: ProcessExecutionObserver<'a>,
    authoritative_output_path: Option<&'a Path>,
    last_result_file_inspection: Option<Instant>,
    result_file_failure: Option<WorkerResultFileFailure>,
}

impl<'a> WorkerProcessObserver<'a> {
    fn new(
        execution: ProcessExecutionObserver<'a>,
        authoritative_output_path: Option<&'a Path>,
    ) -> Self {
        Self {
            execution,
            authoritative_output_path,
            last_result_file_inspection: None,
            result_file_failure: None,
        }
    }

    fn take_result_file_failure(&mut self) -> Option<WorkerResultFileFailure> {
        self.result_file_failure.take()
    }

    fn inspect_authoritative_output_if_due(&mut self) -> bool {
        if self.authoritative_output_path.is_none() {
            return false;
        }
        let now = Instant::now();
        if self.last_result_file_inspection.is_some_and(|last| {
            now.saturating_duration_since(last) < WORKER_RESULT_FILE_INSPECTION_INTERVAL
        }) {
            return false;
        }
        self.last_result_file_inspection = Some(now);
        self.inspect_authoritative_output()
    }

    fn inspect_authoritative_output(&mut self) -> bool {
        let Some(path) = self.authoritative_output_path else {
            return false;
        };
        let failure = match path.metadata() {
            Ok(metadata) if !metadata.is_file() => Some(WorkerResultFileFailure::Inspection(
                "Codex output path is not a regular file".into(),
            )),
            Ok(metadata) if metadata.len() > EXECUTION_OUTPUT_CAPTURE_LIMIT as u64 => {
                Some(WorkerResultFileFailure::CaptureLimitExceeded)
            }
            Ok(_) => None,
            Err(error) => Some(WorkerResultFileFailure::Inspection(format!(
                "Failed to inspect Codex output file: {error}"
            ))),
        };
        if let Some(failure) = failure {
            self.result_file_failure = Some(failure);
            true
        } else {
            false
        }
    }
}

impl OwnedProcessObserver for WorkerProcessObserver<'_> {
    fn cancelled(&mut self) -> bool {
        self.execution.cancelled()
            || self.result_file_failure.is_some()
            || self.inspect_authoritative_output_if_due()
    }

    fn output(&mut self, stream: OwnedProcessOutputStream, bytes: &[u8]) {
        self.execution.output(stream, bytes);
    }

    fn poll(&mut self, elapsed: Duration) {
        self.execution.poll(elapsed);
    }
}

#[derive(Debug)]
enum WorkerResultFileFailure {
    CaptureLimitExceeded,
    Inspection(String),
}

impl WorkerResultFileFailure {
    fn into_execution_error(self, process_started: bool) -> ExecutionCommandError {
        let error = match self {
            Self::CaptureLimitExceeded => anyhow!(
                "Codex last-message output exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            ),
            Self::Inspection(message) => anyhow!(message),
        };
        if process_started {
            ExecutionCommandError::failed_after_start(error)
        } else {
            ExecutionCommandError::failed(error)
        }
    }
}

#[derive(Debug)]
struct WorkerCommandOutput {
    output: Output,
    provider_stdout_truncated: bool,
    provider_stderr_truncated: bool,
}

struct CapturedWorkerOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn complete_worker_output(
    output: Option<BoundedProcessOutput>,
    stream: &str,
) -> Result<CapturedWorkerOutput> {
    let output = output.with_context(|| format!("Failed to capture worker {stream}"))?;
    if !output.complete {
        bail!("Failed to capture complete worker {stream}");
    }
    Ok(CapturedWorkerOutput {
        bytes: output.bytes,
        truncated: output.truncated,
    })
}

fn worker_process_error(
    error: OwnedProcessTreeError,
    timeout: CommandTimeout,
) -> ExecutionCommandError {
    match error {
        OwnedProcessTreeError::Start(error) => {
            ExecutionCommandError::failed(anyhow!(error).context("Failed to start worker process"))
        }
        OwnedProcessTreeError::TimedOut => ExecutionCommandError::failed_after_start(anyhow!(
            "Worker process timed out after {} seconds",
            timeout.as_secs()
        )),
        OwnedProcessTreeError::CancelledBeforeStart => ExecutionCommandError::CancelledBeforeStart,
        OwnedProcessTreeError::Cancelled => ExecutionCommandError::Cancelled,
        OwnedProcessTreeError::OutputLimitExceeded(stream) => {
            ExecutionCommandError::failed_after_start(anyhow!(
                "Worker {stream} exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            ))
        }
        OwnedProcessTreeError::Await => {
            ExecutionCommandError::failed_after_start(anyhow!("Failed to wait for worker process"))
        }
        OwnedProcessTreeError::Cleanup => ExecutionCommandError::failed_after_start(anyhow!(
            "Worker process tree could not be cleaned up safely"
        )),
    }
}

struct WorkerRunOutcome<'a> {
    started_at_ms: u64,
    ended_at_ms: u64,
    exit_status: i32,
    stderr: &'a str,
    provider_stdout: Option<&'a str>,
    provider_stdout_truncated: bool,
    provider_stderr_truncated: bool,
    error: Option<&'a str>,
    status: &'static str,
}

/// Describes one worker run for the loop occurrence that started it. The
/// authoritative final message stays with the caller's action.
fn worker_evidence(request: &CodexExecRequest<'_>, outcome: WorkerRunOutcome<'_>) -> Value {
    let status = if outcome.status == "completed" && outcome.exit_status == 0 {
        "passed"
    } else if outcome.status == "completed" {
        "failed"
    } else {
        outcome.status
    };
    let (provider_stdout_preview, provider_stdout_preview_truncated) = outcome
        .provider_stdout
        .map(bounded_provider_preview)
        .map_or((None, false), |(preview, truncated)| {
            (Some(preview), truncated)
        });
    let (provider_stderr_preview, provider_stderr_preview_truncated) =
        bounded_provider_preview(outcome.stderr);
    json!({
        "kind": "worker_run",
        "schema_version": 2,
        "provider": "codex",
        "runner": "codex_exec",
        "mode": "exec",
        "purpose": request.run.purpose,
        "status": status,
        "started_at_ms": outcome.started_at_ms,
        "ended_at_ms": outcome.ended_at_ms,
        "exit_status": outcome.exit_status,
        "model": request.model,
        "approval_policy": request.approval_policy,
        "sandbox": request.sandbox,
        "ephemeral": request.ephemeral,
        "output_schema": request.output_schema.is_some(),
        "prompt_delivery": "stdin",
        "extra_args": request
            .extra_args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        "codex_home_resolved": request
            .codex_home
            .map(|home| home.display().to_string()),
        "workflow_id": request.run.workflow_id,
        "item_key": request.run.item_key,
        "error": outcome.error,
        "stdout_truncated": outcome.provider_stdout_truncated,
        "stderr_truncated": outcome.provider_stderr_truncated,
        "provider_stdout_preview": provider_stdout_preview,
        "provider_stdout_preview_truncated": provider_stdout_preview_truncated,
        "provider_stdout_truncated": outcome.provider_stdout_truncated,
        "provider_stderr_preview": provider_stderr_preview,
        "provider_stderr_preview_truncated": provider_stderr_preview_truncated,
    })
}

fn bounded_provider_preview(text: &str) -> (String, bool) {
    if text.len() <= WORKER_PROVIDER_PREVIEW_BYTES {
        return (text.to_owned(), false);
    }
    let mut end = WORKER_PROVIDER_PREVIEW_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

fn codex_timeout(ctx: &RepoContext) -> Result<CommandTimeout> {
    let Ok(value) = env::var(CODEX_TIMEOUT_ENV) else {
        return Ok(ctx.command_timeout());
    };
    parse_codex_timeout(&value)
}

fn parse_codex_timeout(value: &str) -> Result<CommandTimeout> {
    let seconds = value
        .parse::<u64>()
        .with_context(|| format!("Invalid {CODEX_TIMEOUT_ENV} value '{value}'"))?;
    CommandTimeout::from_seconds(seconds).ok_or_else(|| {
        anyhow!("{CODEX_TIMEOUT_ENV} must be between 1 and {MAX_COMMAND_TIMEOUT_SECONDS}")
    })
}

#[cfg(test)]
mod tests;
