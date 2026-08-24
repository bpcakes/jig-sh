use std::process::{Command, Stdio};

use anyhow::Context;

use super::*;

pub(super) struct CommandToolInvocation<'a> {
    pub(super) tool_name: &'a str,
    pub(super) command_key: &'a str,
    pub(super) command_text: &'a str,
}

enum ConfiguredCommandOutcome {
    Completed(std::process::Output),
    CancelledBeforeStart,
    Cancelled,
    OutputLimitExceeded {
        stream: jig_owned_process::OwnedProcessOutputStream,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
}

struct ConfiguredCommandFailure {
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    started: u64,
    ended: u64,
    error: anyhow::Error,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

pub(super) fn execute_command_tool(
    ctx: &RepoContext,
    invocation: CommandToolInvocation<'_>,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
    let started = now_ms();
    let run_result = run_configured_command(
        ctx,
        invocation.tool_name,
        invocation.command_text,
        &args,
        position,
        observer,
    );
    let ended = now_ms();
    let output = match run_result {
        Ok(ConfiguredCommandOutcome::Completed(output)) => output,
        Ok(ConfiguredCommandOutcome::CancelledBeforeStart) => {
            let message = format!(
                "Configured command for {} was cancelled before it started",
                invocation.tool_name
            );
            let response = tool_response_value(ToolExecutionResponse {
                ok: true,
                tool: invocation.tool_name,
                command_key: Some(invocation.command_key),
                args,
                result: ToolProcessResult {
                    exit_status: 1,
                    stdout: String::new(),
                    stderr: message,
                },
                receipt_id: None,
            })?;
            return Ok(ManifestToolExecutionOutcome::Cancelled(response));
        }
        Ok(ConfiguredCommandOutcome::Cancelled) => {
            let message = format!(
                "Configured command for {} was cancelled",
                invocation.tool_name
            );
            let evidence = serde_json::json!({
                "kind": "supervised_command",
                "schema_version": 1,
                "status": "cancelled",
                "error": message,
            });
            let receipt_result = maybe_record_receipt(
                ctx,
                options.record_receipt,
                ReceiptInput {
                    tool_name: invocation.tool_name,
                    args: args.clone(),
                    invoked_command_key: Some(invocation.command_key.to_string()),
                    plan_id,
                    started_at_ms: started,
                    ended_at_ms: ended,
                    exit_status: 1,
                    stdout: "",
                    stderr: &message,
                    evidence: Some(evidence),
                    session_override: None,
                    collect_git_metadata: options.collect_git_metadata,
                    collect_worktree_fingerprint: options.collect_worktree_fingerprint,
                    worktree_fingerprint_override: None,
                },
                &|| observer.cancelled(),
            );
            let receipt_id = match receipt_result {
                Ok(receipt_id) => receipt_id,
                Err(receipt_error) => {
                    bail!("{message}\nreceipt recording also failed:\n{receipt_error:#}")
                }
            };
            let response = tool_response_value(ToolExecutionResponse {
                ok: true,
                tool: invocation.tool_name,
                command_key: Some(invocation.command_key),
                args,
                result: ToolProcessResult {
                    exit_status: 1,
                    stdout: String::new(),
                    stderr: message,
                },
                receipt_id,
            })?;
            return Ok(ManifestToolExecutionOutcome::Cancelled(response));
        }
        Ok(ConfiguredCommandOutcome::OutputLimitExceeded {
            stream,
            stdout,
            stderr,
        }) => {
            let error = anyhow!(
                "Configured command for {} exceeded the {} byte {stream} capture limit",
                invocation.tool_name,
                ctx.command_output_limit().bytes()
            );
            return finish_configured_command_error(
                ctx,
                &invocation,
                observer,
                ConfiguredCommandFailure {
                    args,
                    plan_id,
                    options,
                    started,
                    ended,
                    error,
                    stdout,
                    stderr,
                },
            );
        }
        Err(error) => {
            return finish_configured_command_error(
                ctx,
                &invocation,
                observer,
                ConfiguredCommandFailure {
                    args,
                    plan_id,
                    options,
                    started,
                    ended,
                    error,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
            );
        }
    };
    let exit_status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let receipt_result = maybe_record_receipt(
        ctx,
        options.record_receipt,
        ReceiptInput {
            tool_name: invocation.tool_name,
            args: args.clone(),
            invoked_command_key: Some(invocation.command_key.to_string()),
            plan_id,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status,
            stdout: &stdout,
            stderr: &stderr,
            evidence: None,
            session_override: None,
            collect_git_metadata: options.collect_git_metadata,
            collect_worktree_fingerprint: options.collect_worktree_fingerprint,
            worktree_fingerprint_override: None,
        },
        &|| observer.cancelled(),
    );

    let tool_failure = tool_failure_message(
        invocation.tool_name,
        Some(invocation.command_key),
        exit_status,
        &stdout,
        &stderr,
    );
    let receipt_id =
        receipt_id_for_failure_mode(options.failure_mode, tool_failure, receipt_result)?;

    tool_response_value(ToolExecutionResponse {
        ok: true,
        tool: invocation.tool_name,
        command_key: Some(invocation.command_key),
        args,
        result: ToolProcessResult {
            exit_status,
            stdout,
            stderr,
        },
        receipt_id,
    })
    .map(ManifestToolExecutionOutcome::Completed)
}

fn finish_configured_command_error(
    ctx: &RepoContext,
    invocation: &CommandToolInvocation<'_>,
    observer: &dyn ExecutionControl,
    failure: ConfiguredCommandFailure,
) -> Result<ManifestToolExecutionOutcome> {
    let ConfiguredCommandFailure {
        args,
        plan_id,
        options,
        started,
        ended,
        error,
        stdout,
        stderr,
    } = failure;
    let message = format!("{error:#}");
    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let mut stderr = String::from_utf8_lossy(&stderr).into_owned();
    if !stderr.is_empty() && !stderr.ends_with('\n') {
        stderr.push('\n');
    }
    stderr.push_str(&message);
    let evidence = serde_json::json!({
        "kind": "supervised_command",
        "schema_version": 1,
        "status": "error",
        "error": message,
    });
    let receipt_result = maybe_record_receipt(
        ctx,
        options.record_receipt,
        ReceiptInput {
            tool_name: invocation.tool_name,
            args: args.clone(),
            invoked_command_key: Some(invocation.command_key.to_string()),
            plan_id,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: 1,
            stdout: &stdout,
            stderr: &stderr,
            evidence: Some(evidence),
            session_override: None,
            collect_git_metadata: options.collect_git_metadata,
            collect_worktree_fingerprint: options.collect_worktree_fingerprint,
            worktree_fingerprint_override: None,
        },
        &|| observer.cancelled(),
    );
    let tool_failure = tool_failure_message(
        invocation.tool_name,
        Some(invocation.command_key),
        1,
        &stdout,
        &stderr,
    );
    let receipt_id =
        receipt_id_for_failure_mode(options.failure_mode, tool_failure, receipt_result)?;
    tool_response_value(ToolExecutionResponse {
        ok: true,
        tool: invocation.tool_name,
        command_key: Some(invocation.command_key),
        args,
        result: ToolProcessResult {
            exit_status: 1,
            stdout,
            stderr,
        },
        receipt_id,
    })
    .map(ManifestToolExecutionOutcome::Completed)
}

pub(super) fn maybe_record_receipt(
    ctx: &RepoContext,
    should_record_receipt: bool,
    input: ReceiptInput<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>> {
    if should_record_receipt {
        record_receipt_with_cancellation(ctx, input, cancelled).map(Some)
    } else {
        Ok(None)
    }
}

fn run_configured_command(
    ctx: &RepoContext,
    tool_name: &str,
    command_text: &str,
    args: &Value,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<ConfiguredCommandOutcome> {
    let mut command = Command::new("bash");
    command
        .current_dir(ctx.root())
        .arg("-c")
        .arg(command_text)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if tool_name == tool::MIGRATION_ADD {
        let name = args
            .get(args::NAME)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
        command.env("NAME", name);
    }

    let phase = ExecutionPhase::start(observer, tool_name, position);
    let label = format!("Configured command for {tool_name}");
    let result = match run_supervised_execution_command(
        &mut command,
        ctx.command_timeout().duration(),
        ctx.command_output_limit(),
        &label,
        observer,
    ) {
        Ok(output) => Ok(ConfiguredCommandOutcome::Completed(std::process::Output {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        })),
        Err(SupervisedExecutionError::CancelledBeforeStart) => {
            Ok(ConfiguredCommandOutcome::CancelledBeforeStart)
        }
        Err(SupervisedExecutionError::Cancelled) => Ok(ConfiguredCommandOutcome::Cancelled),
        Err(SupervisedExecutionError::TimedOut) => Err(anyhow!(
            "{label} timed out after {} seconds",
            ctx.command_timeout().as_secs()
        )),
        Err(SupervisedExecutionError::OutputLimitExceeded {
            stream,
            stdout,
            stderr,
        }) => Ok(ConfiguredCommandOutcome::OutputLimitExceeded {
            stream,
            stdout,
            stderr,
        }),
        Err(SupervisedExecutionError::Failed { error, .. }) => Err(error),
    };
    phase.finish(
        observer,
        result.as_ref().is_ok_and(|outcome| {
            matches!(
                outcome,
                ConfiguredCommandOutcome::Completed(output) if output.status.success()
            )
        }),
    );
    result
}

#[derive(Serialize)]
pub(super) struct ToolExecutionResponse<'a> {
    pub(super) ok: bool,
    pub(super) tool: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) command_key: Option<&'a str>,
    pub(super) args: Value,
    pub(super) result: ToolProcessResult,
    pub(super) receipt_id: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ToolProcessResult {
    pub(super) exit_status: i32,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

pub(super) fn tool_response_value(response: ToolExecutionResponse<'_>) -> Result<Value> {
    serde_json::to_value(response).context("Failed to serialize tool execution response")
}
