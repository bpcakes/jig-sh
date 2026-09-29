use std::process::{Command, Stdio};
use std::{collections::BTreeMap, time::Duration};

use anyhow::Context;

use crate::repository_path::{resolve_repository_working_directory, validate_runner_environment};

use super::*;

pub(super) struct CommandToolInvocation<'a> {
    pub(super) tool_name: &'a str,
    pub(super) command_key: Option<&'a str>,
    pub(super) argv: Option<(&'a str, &'a [jig_contract::ArgvValue])>,
    pub(super) command_text: &'a str,
    pub(super) working_directory: Option<&'a str>,
    pub(super) environment: Option<&'a BTreeMap<String, String>>,
    pub(super) timeout: Duration,
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
    error: anyhow::Error,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

pub(super) fn execute_command_tool(
    ctx: &RepoContext,
    invocation: CommandToolInvocation<'_>,
    args: Value,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
    let run_result = run_configured_command(ctx, &invocation, &args, position, observer);
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
                command_key: invocation.command_key,
                args,
                result: ToolProcessResult {
                    exit_status: 1,
                    stdout: String::new(),
                    stderr: message,
                },
            })?;
            return Ok(ManifestToolExecutionOutcome::Cancelled(response));
        }
        Ok(ConfiguredCommandOutcome::Cancelled) => {
            let message = format!(
                "Configured command for {} was cancelled",
                invocation.tool_name
            );
            let response = tool_response_value(ToolExecutionResponse {
                ok: true,
                tool: invocation.tool_name,
                command_key: invocation.command_key,
                args,
                result: ToolProcessResult {
                    exit_status: 1,
                    stdout: String::new(),
                    stderr: message,
                },
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
                &invocation,
                ConfiguredCommandFailure {
                    args,
                    error,
                    stdout,
                    stderr,
                },
            );
        }
        Err(error) => {
            return finish_configured_command_error(
                &invocation,
                ConfiguredCommandFailure {
                    args,
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

    let tool_failure = tool_failure_message(
        invocation.tool_name,
        invocation.command_key,
        exit_status,
        &stdout,
        &stderr,
    );
    fail_on_tool_failure(tool_failure)?;

    tool_response_value(ToolExecutionResponse {
        ok: true,
        tool: invocation.tool_name,
        command_key: invocation.command_key,
        args,
        result: ToolProcessResult {
            exit_status,
            stdout,
            stderr,
        },
    })
    .map(ManifestToolExecutionOutcome::Completed)
}

fn finish_configured_command_error(
    invocation: &CommandToolInvocation<'_>,
    failure: ConfiguredCommandFailure,
) -> Result<ManifestToolExecutionOutcome> {
    let ConfiguredCommandFailure {
        args,
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
    let tool_failure = tool_failure_message(
        invocation.tool_name,
        invocation.command_key,
        1,
        &stdout,
        &stderr,
    );
    fail_on_tool_failure(tool_failure)?;
    tool_response_value(ToolExecutionResponse {
        ok: true,
        tool: invocation.tool_name,
        command_key: invocation.command_key,
        args,
        result: ToolProcessResult {
            exit_status: 1,
            stdout,
            stderr,
        },
    })
    .map(ManifestToolExecutionOutcome::Completed)
}

fn run_configured_command(
    ctx: &RepoContext,
    invocation: &CommandToolInvocation<'_>,
    args: &Value,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<ConfiguredCommandOutcome> {
    let working_directory =
        resolve_repository_working_directory(ctx.root(), invocation.working_directory)?;
    if let Some(environment) = invocation.environment {
        validate_runner_environment(environment)?;
    }
    let mut command = if let Some((program, positions)) = invocation.argv {
        crate::repository::runners::argv_command(
            program,
            positions,
            &serde_json::from_value(args.clone())?,
        )
    } else {
        let mut command = Command::new("bash");
        command.arg("-c").arg(invocation.command_text);
        command
    };
    command
        .current_dir(working_directory)
        .envs(
            invocation
                .environment
                .into_iter()
                .flat_map(|environment| environment.iter()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if invocation.argv.is_none() && invocation.tool_name == tool::MIGRATION_ADD {
        let name = args
            .get(args::NAME)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
        command.env("NAME", name);
    }

    if invocation.argv.is_some() {
        crate::repository::runners::prepare_literal_exec(&mut command)?;
    }
    let phase = ExecutionPhase::start(observer, invocation.tool_name, position);
    let label = format!("Configured command for {}", invocation.tool_name);
    let result = match run_supervised_execution_command(
        &mut command,
        invocation.timeout,
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
            invocation.timeout.as_secs()
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
