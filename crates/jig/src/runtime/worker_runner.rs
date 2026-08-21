use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use jig_owned_process::{
    BoundedProcessOutput, OwnedProcessTreeError, ProcessOutputLimits, ProcessOutputOverflowPolicy,
    run_owned_process_tree_with_output_policy_and_observer,
};
use serde_json::{Value, json};
use tempfile::NamedTempFile;

use crate::context::{CommandTimeout, MAX_COMMAND_TIMEOUT_SECONDS, RepoContext};
use crate::execution::{
    EXECUTION_OUTPUT_CAPTURE_LIMIT, ExecutionControl, ExecutionPhase, PhasePosition,
    ProcessExecutionObserver,
};
use crate::state::{ReceiptInput, now_ms, record_receipt};
use crate::tool_defs::WORKER_RUN_TOOL;

const CODEX_TIMEOUT_ENV: &str = "JIG_CODEX_TIMEOUT_SECS";

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
    pub(crate) prompt: CodexPrompt<'a>,
    pub(crate) receipt: WorkerReceiptRequest<'a>,
    pub(crate) phase: Option<WorkerPhase<'a>>,
}

pub(crate) struct CodexExecOutput {
    pub(crate) output: Output,
    pub(crate) provider_stdout: String,
    pub(crate) worker_receipt_id: String,
}

pub(crate) fn run_codex_exec(
    ctx: &RepoContext,
    request: CodexExecRequest<'_>,
    observer: &mut dyn ExecutionControl,
) -> Result<CodexExecOutput> {
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
            let receipt_id = record_worker_receipt(
                ctx,
                &request,
                WorkerReceiptOutcome {
                    started_at_ms: started,
                    ended_at_ms: ended,
                    exit_status,
                    stdout: &run.provider_stdout,
                    stderr: &run.provider_stderr,
                    stdout_truncated: run.provider_stdout_truncated,
                    stderr_truncated: run.provider_stderr_truncated,
                    error: None,
                },
            )?;
            Ok(CodexExecOutput {
                output: run.output,
                provider_stdout: run.provider_stdout,
                worker_receipt_id: receipt_id,
            })
        }
        Err(error) => {
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
                    stdout_truncated: false,
                    stderr_truncated: false,
                    error: Some(&message),
                },
            )?;
            bail!("Codex worker invocation failed; receipt {receipt_id}: {message}");
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
) -> Result<CodexRunOutput> {
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
    let output_file = if request.output_schema.is_some() {
        Some(NamedTempFile::new().context("Failed to create Codex output file")?)
    } else {
        None
    };

    let mut command = build_codex_command(
        crate::codex::codex_bin(),
        request,
        schema_file.as_ref().map(NamedTempFile::path),
        output_file.as_ref().map(NamedTempFile::path),
    );
    let output = run_worker_command(
        &mut command,
        request.prompt.stdin_prompt(),
        codex_timeout(ctx)?,
        request.receipt.purpose,
        request.output_schema.is_some(),
        observer,
    )?;
    let provider_stdout = String::from_utf8_lossy(&output.output.stdout).into_owned();
    let provider_stderr = String::from_utf8_lossy(&output.output.stderr).into_owned();
    let provider_stdout_truncated = output.stdout_truncated;
    let provider_stderr_truncated = output.stderr_truncated;
    let mut output = output.output;

    if let Some(output_file) = output_file {
        if let Some(structured_output) = read_worker_output_file(output_file.path())? {
            output.stdout = structured_output;
        }
    }

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
            "Codex structured output exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
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
    output_path: Option<&Path>,
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
    if let (Some(schema_path), Some(output_path)) = (schema_path, output_path) {
        command
            .arg("--output-schema")
            .arg(schema_path)
            .arg("-o")
            .arg(output_path);
    }
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
    allow_transcript_truncation: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<WorkerCommandOutput> {
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

    let output = run_owned_process_tree_with_output_policy_and_observer(
        command,
        timeout.duration(),
        ProcessOutputLimits {
            stdout: EXECUTION_OUTPUT_CAPTURE_LIMIT,
            stderr: EXECUTION_OUTPUT_CAPTURE_LIMIT,
        },
        if allow_transcript_truncation {
            ProcessOutputOverflowPolicy::Truncate
        } else {
            ProcessOutputOverflowPolicy::Error
        },
        &mut ProcessExecutionObserver::new(observer, label),
    )
    .map_err(|error| worker_process_error(error, timeout))?;

    let stdout = complete_worker_output(output.stdout, "stdout")?;
    let stderr = complete_worker_output(output.stderr, "stderr")?;
    Ok(WorkerCommandOutput {
        output: Output {
            status: output.status,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
        },
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

#[derive(Debug)]
struct WorkerCommandOutput {
    output: Output,
    stdout_truncated: bool,
    stderr_truncated: bool,
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

fn worker_process_error(error: OwnedProcessTreeError, timeout: CommandTimeout) -> anyhow::Error {
    match error {
        OwnedProcessTreeError::Start(error) => {
            anyhow!(error).context("Failed to start worker process")
        }
        OwnedProcessTreeError::TimedOut => anyhow!(
            "Worker process timed out after {} seconds",
            timeout.as_secs()
        ),
        OwnedProcessTreeError::CancelledBeforeStart => {
            anyhow!("Worker process was cancelled before it started")
        }
        OwnedProcessTreeError::Cancelled => anyhow!("Worker process was cancelled"),
        OwnedProcessTreeError::OutputLimitExceeded(stream) => anyhow!(
            "Worker {stream} exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
        ),
        OwnedProcessTreeError::Await => anyhow!("Failed to wait for worker process"),
        OwnedProcessTreeError::Cleanup => {
            anyhow!("Worker process tree could not be cleaned up safely")
        }
    }
}

struct WorkerReceiptOutcome<'a> {
    started_at_ms: u64,
    ended_at_ms: u64,
    exit_status: i32,
    stdout: &'a str,
    stderr: &'a str,
    stdout_truncated: bool,
    stderr_truncated: bool,
    error: Option<&'a str>,
}

fn record_worker_receipt(
    ctx: &RepoContext,
    request: &CodexExecRequest<'_>,
    outcome: WorkerReceiptOutcome<'_>,
) -> Result<String> {
    let status = if outcome.error.is_some() {
        "error"
    } else if outcome.exit_status == 0 {
        "passed"
    } else {
        "failed"
    };
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
        "stdout_truncated": outcome.stdout_truncated,
        "stderr_truncated": outcome.stderr_truncated,
    });
    record_receipt(
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
    )
    .context("Failed to record worker receipt")
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
mod tests {
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::process::Command;
    #[cfg(unix)]
    use std::thread;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    use crate::test_env::{EnvVarGuard, lock_env};

    use std::path::Path;

    use super::*;

    #[derive(Default)]
    struct RecordingControl {
        output: Vec<u8>,
    }

    impl crate::execution::ExecutionObserver for RecordingControl {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            if let crate::execution::ExecutionEvent::Output { bytes, .. } = event {
                self.output.extend_from_slice(bytes);
            }
        }
    }

    impl crate::execution::ExecutionCancellation for RecordingControl {}

    #[test]
    fn codex_refine_approval_policy_is_a_top_level_codex_arg() {
        let mut request = CodexExecRequest {
            root: Path::new("/tmp/repo"),
            codex_home: Some(Path::new("/tmp/codex-home")),
            mode: CodexExecMode::Exec,
            model: Some("gpt-x"),
            approval_policy: Some("never"),
            sandbox: Some("workspace-write"),
            ephemeral: true,
            extra_args: Vec::new(),
            output_schema: None,
            prompt: CodexPrompt::Stdin("fix this"),
            receipt: WorkerReceiptRequest {
                purpose: "work_refine",
                plan_id: Some("plan_1"),
                workflow_id: None,
                item_key: None,
                collect_git_metadata: true,
                collect_worktree_fingerprint: true,
            },
            phase: Some(WorkerPhase {
                label: "test worker",
                position: PhasePosition::single(),
            }),
        };
        let command = build_codex_command("codex", &request, None, None);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            [
                "--ask-for-approval",
                "never",
                "exec",
                "--sandbox",
                "workspace-write",
                "--ephemeral",
                "--model",
                "gpt-x",
                "-",
            ]
        );
        assert!(command.get_envs().any(|(key, value)| {
            key == crate::codex::CODEX_HOME_ENV && value == Some(OsStr::new("/tmp/codex-home"))
        }));

        request.codex_home = None;
        let inherited_command = build_codex_command("codex", &request, None, None);
        assert!(
            inherited_command
                .get_envs()
                .all(|(key, _)| key != crate::codex::CODEX_HOME_ENV)
        );
    }

    #[test]
    fn codex_timeout_override_uses_the_validated_command_timeout_range() {
        assert_eq!(parse_codex_timeout("1").unwrap().as_secs(), 1);
        assert_eq!(
            parse_codex_timeout(&MAX_COMMAND_TIMEOUT_SECONDS.to_string())
                .unwrap()
                .as_secs(),
            MAX_COMMAND_TIMEOUT_SECONDS
        );
        for value in [
            "0".to_string(),
            (MAX_COMMAND_TIMEOUT_SECONDS + 1).to_string(),
        ] {
            let error = parse_codex_timeout(&value).unwrap_err().to_string();
            assert!(error.contains("must be between 1 and 86400"), "{error}");
        }
    }

    #[test]
    fn structured_worker_output_file_is_size_bounded() {
        let output = NamedTempFile::new().unwrap();
        output
            .as_file()
            .set_len((EXECUTION_OUTPUT_CAPTURE_LIMIT + 1) as u64)
            .unwrap();

        let error = read_worker_output_file(output.path())
            .unwrap_err()
            .to_string();

        assert!(
            error.contains(&format!(
                "exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            )),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn worker_supervision_delivers_stdin_and_observes_output() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "cat"]);
        let mut control = RecordingControl::default();

        let output = run_worker_command(
            &mut command,
            Some("prompt through a file"),
            CommandTimeout::from_seconds(1).unwrap(),
            "test worker",
            false,
            &mut control,
        )
        .unwrap();

        assert!(output.output.status.success());
        assert_eq!(output.output.stdout, b"prompt through a file");
        assert_eq!(control.output, b"prompt through a file");
    }

    #[cfg(unix)]
    #[test]
    fn worker_supervision_rejects_output_beyond_the_capture_limit() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            &format!("head -c {} /dev/zero", EXECUTION_OUTPUT_CAPTURE_LIMIT + 1),
        ]);

        let error = run_worker_command(
            &mut command,
            None,
            CommandTimeout::from_seconds(5).unwrap(),
            "test worker",
            false,
            &mut crate::execution::NoopExecutionObserver,
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains(&format!(
                "exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte capture limit"
            )),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn schema_backed_worker_allows_truncated_provider_transcript() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            &format!("head -c {} /dev/zero", EXECUTION_OUTPUT_CAPTURE_LIMIT + 1),
        ]);

        let output = run_worker_command(
            &mut command,
            None,
            CommandTimeout::from_seconds(5).unwrap(),
            "test worker",
            true,
            &mut crate::execution::NoopExecutionObserver,
        )
        .unwrap();

        assert!(output.output.status.success());
        assert_eq!(output.output.stdout.len(), EXECUTION_OUTPUT_CAPTURE_LIMIT);
        assert!(output.stdout_truncated);
        assert!(!output.stderr_truncated);
    }

    #[cfg(unix)]
    #[test]
    fn worker_timeout_kills_process_group() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = lock_env();
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("escaped-grandchild");
        let script = temp.path().join("worker.sh");
        fs::write(
            &script,
            r#"#!/bin/sh
marker="$1"
(sh -c 'sleep 3; printf leaked > "$1"' sh "$marker") &
wait
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
        let _timeout = EnvVarGuard::set(CODEX_TIMEOUT_ENV, "1");

        let mut command = Command::new(&script);
        command.arg(&marker);
        let error = run_worker_command(
            &mut command,
            None,
            CommandTimeout::from_seconds(1).unwrap(),
            "test worker",
            false,
            &mut crate::execution::NoopExecutionObserver,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("Worker process timed out after 1 seconds"));
        thread::sleep(Duration::from_millis(3500));
        assert!(
            !marker.exists(),
            "worker timeout killed the child process but left its process group running"
        );
    }
}
