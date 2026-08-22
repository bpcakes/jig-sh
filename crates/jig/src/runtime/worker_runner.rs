use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use wait_timeout::ChildExt;

use crate::context::RepoContext;
use crate::state::{ReceiptInput, now_ms, record_receipt};
use crate::tool_defs::WORKER_RUN_TOOL;

const CODEX_TIMEOUT_ENV: &str = "JIG_CODEX_TIMEOUT_SECS";
const DEFAULT_CODEX_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(100);

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

impl CodexPrompt<'_> {
    const fn delivery(self) -> &'static str {
        match self {
            Self::Argument(_) => "argument",
            Self::Stdin(_) => "stdin",
        }
    }

    fn stdin_prompt(self) -> Option<String> {
        match self {
            Self::Argument(_) => None,
            Self::Stdin(prompt) => Some(prompt.to_string()),
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
    pub(crate) cancelled: Option<&'a dyn Fn() -> bool>,
    pub(crate) receipt: WorkerReceiptRequest<'a>,
}

pub(crate) struct CodexExecOutput {
    pub(crate) output: Output,
    pub(crate) provider_stdout: String,
    pub(crate) worker_receipt_id: String,
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

pub(crate) fn run_codex_exec(
    ctx: &RepoContext,
    request: CodexExecRequest<'_>,
) -> std::result::Result<CodexExecOutput, CodexExecFailure> {
    let started = now_ms();
    let result = run_codex_exec_inner(&request);
    let ended = now_ms();

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
                    error: None,
                },
            )
            .map_err(|error| CodexExecFailure {
                worker_receipt_id: None,
                message: format!("Failed to record completed Codex worker receipt: {error:#}"),
            })?;
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
                    error: Some(&message),
                },
            )
            .map_err(|receipt_error| CodexExecFailure {
                worker_receipt_id: None,
                message: format!(
                    "Codex worker invocation failed: {message}; failed to record worker receipt: {receipt_error:#}"
                ),
            })?;
            Err(CodexExecFailure {
                worker_receipt_id: Some(receipt_id.clone()),
                message: format!("Codex worker invocation failed; receipt {receipt_id}: {message}"),
            })
        }
    }
}

struct CodexRunOutput {
    output: Output,
    provider_stdout: String,
    provider_stderr: String,
}

fn run_codex_exec_inner(request: &CodexExecRequest<'_>) -> Result<CodexRunOutput> {
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
        request.cancelled,
    )?;
    let provider_stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let provider_stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let mut output = output;

    if let Some(output_file) = output_file {
        let output_metadata = output_file
            .path()
            .metadata()
            .context("Failed to inspect Codex output file")?;
        if output_metadata.len() > 0 {
            output.stdout =
                fs::read(output_file.path()).context("Failed to read Codex output file")?;
        }
    }

    Ok(CodexRunOutput {
        output,
        provider_stdout,
        provider_stderr,
    })
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
    stdin_prompt: Option<String>,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Result<Output> {
    let stdout_file = NamedTempFile::new().context("Failed to create worker stdout file")?;
    let stderr_file = NamedTempFile::new().context("Failed to create worker stderr file")?;
    command
        .stdout(
            stdout_file
                .reopen()
                .context("Failed to open worker stdout file")?,
        )
        .stderr(
            stderr_file
                .reopen()
                .context("Failed to open worker stderr file")?,
        );
    if stdin_prompt.is_some() {
        command.stdin(Stdio::piped());
    }
    configure_worker_process(command);

    let mut child = command.spawn().context("Failed to start worker process")?;
    let writer = if let Some(prompt) = stdin_prompt {
        let mut stdin = child.stdin.take().context("Failed to open worker stdin")?;
        Some(thread::spawn(move || -> Result<()> {
            stdin
                .write_all(prompt.as_bytes())
                .context("Failed to write worker prompt")?;
            Ok(())
        }))
    } else {
        None
    };

    let status = wait_for_worker(&mut child, codex_timeout()?, cancelled)?;

    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|_| anyhow!("Worker stdin writer thread panicked"))??;
    }

    Ok(Output {
        status,
        stdout: fs::read(stdout_file.path()).context("Failed to read worker stdout")?,
        stderr: fs::read(stderr_file.path()).context("Failed to read worker stderr")?,
    })
}

fn wait_for_worker(
    child: &mut Child,
    timeout: Duration,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Result<ExitStatus> {
    let started = Instant::now();
    loop {
        if cancelled.is_some_and(|cancelled| cancelled()) {
            terminate_worker_process(child);
            let _ = child.wait();
            bail!("Worker process cancelled because its execution lease was lost");
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            terminate_worker_process(child);
            let _ = child.wait();
            bail!(
                "Worker process timed out after {} seconds",
                timeout.as_secs()
            );
        }
        if let Some(status) = child
            .wait_timeout(worker_wait_interval(remaining, cancelled.is_some()))
            .context("Failed to wait for worker process")?
        {
            return Ok(status);
        }
    }
}

fn worker_wait_interval(remaining: Duration, cancellable: bool) -> Duration {
    if cancellable {
        remaining.min(CANCELLATION_POLL_INTERVAL)
    } else {
        remaining
    }
}

#[cfg(unix)]
fn configure_worker_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_worker_process(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_worker_process(child: &mut Child) {
    let pgid = child.id() as libc::pid_t;
    if pgid > 0 {
        let _ = unsafe { libc::kill(-pgid, libc::SIGKILL) };
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn terminate_worker_process(child: &mut Child) {
    let _ = child.kill();
}

struct WorkerReceiptOutcome<'a> {
    started_at_ms: u64,
    ended_at_ms: u64,
    exit_status: i32,
    stdout: &'a str,
    stderr: &'a str,
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

fn codex_timeout() -> Result<Duration> {
    let Ok(value) = env::var(CODEX_TIMEOUT_ENV) else {
        return Ok(DEFAULT_CODEX_TIMEOUT);
    };
    let seconds = value
        .parse::<u64>()
        .with_context(|| format!("Invalid {CODEX_TIMEOUT_ENV} value '{value}'"))?;
    if seconds == 0 {
        bail!("{CODEX_TIMEOUT_ENV} must be greater than zero");
    }
    Ok(Duration::from_secs(seconds))
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
            cancelled: None,
            receipt: WorkerReceiptRequest {
                purpose: "work_refine",
                plan_id: Some("plan_1"),
                workflow_id: None,
                item_key: None,
                collect_git_metadata: true,
                collect_worktree_fingerprint: true,
            },
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
    fn worker_wait_interval_only_polls_cancellable_workers() {
        let remaining = Duration::from_secs(30 * 60);

        assert_eq!(worker_wait_interval(remaining, false), remaining);
        assert_eq!(
            worker_wait_interval(remaining, true),
            CANCELLATION_POLL_INTERVAL
        );
    }

    #[cfg(unix)]
    #[test]
    fn maximum_worker_timeout_does_not_overflow_deadline() {
        let _guard = lock_env();
        let _timeout = EnvVarGuard::set(CODEX_TIMEOUT_ENV, u64::MAX.to_string());
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);

        let output = run_worker_command(&mut command, None, None).unwrap();

        assert!(output.status.success());
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
        let error = run_worker_command(&mut command, None, None)
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
