use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use jig_owned_process::{
    BoundedProcessOutput, OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError,
    ProcessOutputLimits, ProcessOutputOverflowPolicy,
    run_owned_process_tree_with_output_policy_and_observer,
};
use serde_json::{Value, json};
use tempfile::NamedTempFile;

use crate::context::{CommandTimeout, MAX_COMMAND_TIMEOUT_SECONDS, RepoContext};
use crate::execution::{
    EXECUTION_OUTPUT_CAPTURE_LIMIT, ExecutionCommandError, ExecutionControl, ExecutionPhase,
    PhasePosition, ProcessExecutionObserver,
};
use crate::state::{ReceiptInput, now_ms, record_receipt_with_cancellation};
use crate::tool_defs::WORKER_RUN_TOOL;

const CODEX_TIMEOUT_ENV: &str = "JIG_CODEX_TIMEOUT_SECS";
const WORKER_PROVIDER_PREVIEW_BYTES: usize = 4_000;
// Preserve the supervisor's normal idle responsiveness without repeating a
// metadata syscall for every faster poll while transcript output is flowing.
const WORKER_RESULT_FILE_INSPECTION_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug)]
pub(crate) enum CodexExecMode {
    Exec,
    Review,
}

impl CodexExecMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Exec => "exec",
            Self::Review => "review",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CodexPrompt<'a> {
    Argument(&'a str),
    Stdin(&'a str),
}

impl<'a> CodexPrompt<'a> {
    const fn delivery(self) -> &'static str {
        match self {
            Self::Argument(_) => "argument",
            Self::Stdin(_) => "stdin",
        }
    }

    fn stdin_prompt(self) -> Option<&'a str> {
        match self {
            Self::Argument(_) => None,
            Self::Stdin(prompt) => Some(prompt),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkerReceiptRequest<'a> {
    pub(crate) purpose: &'a str,
    pub(crate) plan_id: Option<&'a str>,
    pub(crate) workflow_id: Option<&'a str>,
    pub(crate) item_key: Option<&'a str>,
    pub(crate) collect_git_metadata: bool,
    pub(crate) collect_worktree_fingerprint: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkerPhase<'a> {
    pub(crate) label: &'a str,
    pub(crate) position: PhasePosition,
}

pub(crate) struct CodexExecRequest<'a> {
    pub(crate) root: &'a Path,
    pub(crate) codex_home: Option<&'a Path>,
    pub(crate) mode: CodexExecMode,
    pub(crate) model: Option<&'a str>,
    pub(crate) approval_policy: Option<&'a str>,
    pub(crate) sandbox: Option<&'a str>,
    pub(crate) ephemeral: bool,
    pub(crate) extra_args: Vec<OsString>,
    pub(crate) output_schema: Option<&'a Value>,
    pub(crate) transcript_overflow_policy: ProcessOutputOverflowPolicy,
    pub(crate) prompt: CodexPrompt<'a>,
    pub(crate) receipt: WorkerReceiptRequest<'a>,
    pub(crate) phase: Option<WorkerPhase<'a>>,
}

pub(crate) struct CodexExecOutput {
    output: Output,
    provider_stdout: String,
    provider_stdout_truncated: bool,
    worker_receipt_id: String,
}

impl CodexExecOutput {
    pub(crate) fn status(&self) -> &std::process::ExitStatus {
        &self.output.status
    }

    pub(crate) fn authoritative_stdout(&self) -> &[u8] {
        &self.output.stdout
    }

    pub(crate) fn provider_stdout(&self) -> &str {
        &self.provider_stdout
    }

    pub(crate) fn provider_stdout_truncated(&self) -> bool {
        self.provider_stdout_truncated
    }

    pub(crate) fn worker_receipt_id(&self) -> &str {
        &self.worker_receipt_id
    }

    pub(crate) fn into_process_output(self) -> Output {
        self.output
    }
}

#[derive(Debug)]
pub(crate) struct CodexExecFailure {
    worker_receipt_id: Option<String>,
    message: String,
}

impl CodexExecFailure {
    pub(crate) fn worker_receipt_id(&self) -> Option<&str> {
        self.worker_receipt_id.as_deref()
    }
}

impl fmt::Display for CodexExecFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CodexExecFailure {}

pub(crate) enum CodexExecOutcome {
    Completed(CodexExecOutput),
    Cancelled {
        before_start: bool,
        worker_receipt_id: String,
    },
}

impl CodexExecOutcome {
    pub(crate) fn into_completed(self) -> Result<CodexExecOutput> {
        match self {
            Self::Completed(output) => Ok(output),
            Self::Cancelled {
                before_start,
                worker_receipt_id,
            } => {
                let timing = if before_start {
                    " before it started"
                } else {
                    ""
                };
                bail!("Codex worker was cancelled{timing}; receipt {worker_receipt_id}")
            }
        }
    }
}

pub(crate) fn run_codex_exec(
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
            let authoritative_stdout = String::from_utf8_lossy(&run.output.stdout).into_owned();
            let receipt_id = record_worker_receipt(
                ctx,
                &request,
                WorkerReceiptOutcome {
                    started_at_ms: started,
                    ended_at_ms: ended,
                    exit_status,
                    stdout: &authoritative_stdout,
                    stderr: &run.provider_stderr,
                    provider_stdout: Some(&run.provider_stdout),
                    provider_stdout_truncated: run.provider_stdout_truncated,
                    provider_stderr_truncated: run.provider_stderr_truncated,
                    error: None,
                    status: "completed",
                },
                observer,
            )?;
            Ok(CodexExecOutcome::Completed(CodexExecOutput {
                output: run.output,
                provider_stdout: run.provider_stdout,
                provider_stdout_truncated: run.provider_stdout_truncated,
                worker_receipt_id: receipt_id,
            }))
        }
        Err(
            error
            @ (ExecutionCommandError::CancelledBeforeStart | ExecutionCommandError::Cancelled),
        ) => {
            let before_start = matches!(error, ExecutionCommandError::CancelledBeforeStart);
            let message = format!("{error:#}");
            let receipt_id = record_worker_receipt(
                ctx,
                &request,
                WorkerReceiptOutcome {
                    started_at_ms: started,
                    ended_at_ms: ended,
                    exit_status: 1,
                    stdout: "",
                    stderr: &message,
                    provider_stdout: None,
                    provider_stdout_truncated: false,
                    provider_stderr_truncated: false,
                    error: Some(&message),
                    status: "cancelled",
                },
                observer,
            )?;
            Ok(CodexExecOutcome::Cancelled {
                before_start,
                worker_receipt_id: receipt_id,
            })
        }
        Err(ExecutionCommandError::Failed(error)) => {
            let message = format!("{error:#}");
            let receipt_id = record_worker_receipt(
                ctx,
                &request,
                WorkerReceiptOutcome {
                    started_at_ms: started,
                    ended_at_ms: ended,
                    exit_status: 1,
                    stdout: "",
                    stderr: &message,
                    provider_stdout: None,
                    provider_stdout_truncated: false,
                    provider_stderr_truncated: false,
                    error: Some(&message),
                    status: "error",
                },
                observer,
            )?;
            Err(CodexExecFailure {
                worker_receipt_id: Some(receipt_id.clone()),
                message: format!("Codex worker invocation failed; receipt {receipt_id}: {message}"),
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
        crate::codex::codex_bin(),
        request,
        schema_file.as_ref().map(NamedTempFile::path),
        output_file.path(),
    );
    let output = run_worker_command(
        &mut command,
        request.prompt.stdin_prompt(),
        codex_timeout(ctx)?,
        request.receipt.purpose,
        request.transcript_overflow_policy,
        Some(output_file.path()),
        observer,
    )?;
    let provider_stdout = String::from_utf8_lossy(&output.output.stdout).into_owned();
    let provider_stderr = String::from_utf8_lossy(&output.output.stderr).into_owned();
    let provider_stdout_truncated = output.provider_stdout_truncated;
    let provider_stderr_truncated = output.provider_stderr_truncated;
    let mut output = output.output;

    output.stdout = read_worker_output_file(output_file.path())?.unwrap_or_default();

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
        command.env(crate::codex::CODEX_HOME_ENV, codex_home);
    }
    if let Some(approval_policy) = request.approval_policy {
        command.arg("--ask-for-approval").arg(approval_policy);
    }
    command.arg("exec");
    if matches!(request.mode, CodexExecMode::Review) {
        command.arg("review");
    }
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
    match request.prompt {
        CodexPrompt::Argument(prompt) => {
            command.arg(prompt);
        }
        CodexPrompt::Stdin(_) => {
            command.arg("-");
        }
    }
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
        (Ok(_), Some(failure))
        | (
            Err(OwnedProcessTreeError::CancelledBeforeStart | OwnedProcessTreeError::Cancelled),
            Some(failure),
        ) => return Err(failure.into_execution_error()),
        (Ok(output), None) => output,
        (Err(error), _) => return Err(worker_process_error(error, timeout)),
    };

    let stdout = complete_worker_output(output.stdout, "stdout")?;
    let stderr = complete_worker_output(output.stderr, "stderr")?;
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
    fn into_execution_error(self) -> ExecutionCommandError {
        let error = match self {
            Self::CaptureLimitExceeded => anyhow!(
                "Codex last-message output exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            ),
            Self::Inspection(message) => anyhow!(message),
        };
        ExecutionCommandError::failed(error)
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
        OwnedProcessTreeError::TimedOut => ExecutionCommandError::failed(anyhow!(
            "Worker process timed out after {} seconds",
            timeout.as_secs()
        )),
        OwnedProcessTreeError::CancelledBeforeStart => ExecutionCommandError::CancelledBeforeStart,
        OwnedProcessTreeError::Cancelled => ExecutionCommandError::Cancelled,
        OwnedProcessTreeError::OutputLimitExceeded(stream) => {
            ExecutionCommandError::failed(anyhow!(
                "Worker {stream} exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            ))
        }
        OwnedProcessTreeError::Await => {
            ExecutionCommandError::failed(anyhow!("Failed to wait for worker process"))
        }
        OwnedProcessTreeError::Cleanup => ExecutionCommandError::failed(anyhow!(
            "Worker process tree could not be cleaned up safely"
        )),
    }
}

struct WorkerReceiptOutcome<'a> {
    started_at_ms: u64,
    ended_at_ms: u64,
    exit_status: i32,
    stdout: &'a str,
    stderr: &'a str,
    provider_stdout: Option<&'a str>,
    provider_stdout_truncated: bool,
    provider_stderr_truncated: bool,
    error: Option<&'a str>,
    status: &'static str,
}

fn record_worker_receipt(
    ctx: &RepoContext,
    request: &CodexExecRequest<'_>,
    outcome: WorkerReceiptOutcome<'_>,
    observer: &mut dyn ExecutionControl,
) -> Result<String> {
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
    let evidence = json!({
        "kind": "worker_run",
        "schema_version": 1,
        "provider": "codex",
        "runner": "codex_exec",
        "mode": request.mode.as_str(),
        "purpose": request.receipt.purpose,
        "status": status,
        "model": request.model,
        "approval_policy": request.approval_policy,
        "sandbox": request.sandbox,
        "ephemeral": request.ephemeral,
        "output_schema": request.output_schema.is_some(),
        "prompt_delivery": request.prompt.delivery(),
        "extra_args": request
            .extra_args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        "codex_home_resolved": request
            .codex_home
            .map(|home| home.display().to_string()),
        "plan_id": request.receipt.plan_id,
        "workflow_id": request.receipt.workflow_id,
        "item_key": request.receipt.item_key,
        "error": outcome.error,
        "stdout_truncated": outcome.provider_stdout_truncated,
        "stderr_truncated": outcome.provider_stderr_truncated,
        "provider_stdout_preview": provider_stdout_preview,
        "provider_stdout_preview_truncated": provider_stdout_preview_truncated,
        "provider_stdout_truncated": outcome.provider_stdout_truncated,
    });
    record_receipt_with_cancellation(
        ctx,
        ReceiptInput {
            tool_name: WORKER_RUN_TOOL,
            args: json!({
                "provider": "codex",
                "runner": "codex_exec",
                "mode": request.mode.as_str(),
                "purpose": request.receipt.purpose,
                "plan_id": request.receipt.plan_id,
                "workflow_id": request.receipt.workflow_id,
                "item_key": request.receipt.item_key,
            }),
            invoked_command_key: None,
            plan_id: request.receipt.plan_id.map(ToOwned::to_owned),
            started_at_ms: outcome.started_at_ms,
            ended_at_ms: outcome.ended_at_ms,
            exit_status: outcome.exit_status,
            stdout: outcome.stdout,
            stderr: outcome.stderr,
            evidence: Some(evidence),
            session_override: None,
            collect_git_metadata: request.receipt.collect_git_metadata,
            collect_worktree_fingerprint: request.receipt.collect_worktree_fingerprint,
            worktree_fingerprint_override: None,
        },
        &|| observer.cancelled(),
    )
    .context("Failed to record worker receipt")
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

include!("worker_runner/tests.rs");
