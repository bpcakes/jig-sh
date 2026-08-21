use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use jig_contract::{ManifestTool, NativeToolKind};
use jig_owned_process::{
    ProcessOutputLimits, ProcessOutputOverflowPolicy,
    run_owned_process_tree_with_output_policy_and_observer,
};
use serde::Serialize;
use serde_json::Value;

use crate::context::RepoContext;
#[cfg(test)]
use crate::execution::NoopExecutionObserver;
use crate::execution::{
    EXECUTION_OUTPUT_CAPTURE_LIMIT, ExecutionControl, ExecutionPhase, PhasePosition,
    ProcessExecutionObserver,
};
use crate::policy::NativeToolOutput;
use crate::state::{ReceiptInput, now_ms, record_receipt};
use crate::tool_defs::{self, JsonObject, args, kind, string_arg, tool};

pub(in crate::runtime) fn execute_manifest_tool_request_with_observer(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    request: crate::command::ToolRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let (plan_id, record_receipt) = request.into_parts();
    execute_manifest_tool_with_observer(ctx, tool_name, args, plan_id, record_receipt, observer)
}

pub(in crate::runtime) fn call_manifest_tool_with_observer(
    ctx: &RepoContext,
    tool: &ManifestTool,
    args_obj: &JsonObject,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let plan_id = string_arg(args_obj, args::PLAN_ID);
    let args = tool_defs::execution_tool_args(tool, args_obj)?;

    // MCP execution tools are evidence-producing by design; the CLI-only
    // --no-receipt escape hatch is intentionally not part of the tool schema.
    execute_manifest_tool_with_observer(ctx, &tool.name, args, plan_id, true, observer)
}

pub(in crate::runtime) fn execute_manifest_tool_with_observer(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    record_receipt: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    execute_manifest_tool_with_options(
        ctx,
        tool_name,
        args,
        plan_id,
        ManifestToolExecutionOptions::fail_fast(record_receipt, true, true),
        PhasePosition::single(),
        observer,
    )
}

#[cfg(test)]
pub(in crate::runtime) fn execute_manifest_tool_result_without_worktree_fingerprint(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
) -> Result<Value> {
    execute_manifest_tool_with_options(
        ctx,
        tool_name,
        args,
        plan_id,
        ManifestToolExecutionOptions::collect_result(true, false, false),
        PhasePosition::single(),
        &mut NoopExecutionObserver,
    )
}

pub(in crate::runtime) fn execute_manifest_tool_with_options_for_work_check(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    execute_manifest_tool_with_options(
        ctx,
        tool_name,
        args,
        plan_id,
        ManifestToolExecutionOptions::collect_result(true, false, false),
        position,
        observer,
    )
}

pub(in crate::runtime) fn manifest_tool_result_failure(
    response: &Value,
) -> Result<Option<(i32, String)>> {
    let tool_name = response
        .get("tool")
        .and_then(Value::as_str)
        .context("Tool execution response is missing `tool`")?;
    let result = response
        .get("result")
        .context("Tool execution response is missing `result`")?;
    let exit_status = result
        .get("exit_status")
        .and_then(Value::as_i64)
        .and_then(|status| i32::try_from(status).ok())
        .context("Tool execution response has an invalid `result.exit_status`")?;
    let stdout = result
        .get("stdout")
        .and_then(Value::as_str)
        .context("Tool execution response is missing `result.stdout`")?;
    let stderr = result
        .get("stderr")
        .and_then(Value::as_str)
        .context("Tool execution response is missing `result.stderr`")?;
    let command_key = response.get("command_key").and_then(Value::as_str);

    Ok(
        tool_failure_message(tool_name, command_key, exit_status, stdout, stderr)
            .map(|message| (exit_status, message)),
    )
}

pub(in crate::runtime) fn undeclared_tool_message(ctx: &RepoContext, tool_name: &str) -> String {
    if let Some(message) = jig_features::unavailable_tool_message(ctx, tool_name) {
        message
    } else {
        format!("Tool is not declared in .agent/jig-contract.json: {tool_name}")
    }
}

#[derive(Clone, Copy)]
enum ToolFailureMode {
    FailFast,
    CollectResult,
}

#[derive(Clone, Copy)]
struct ManifestToolExecutionOptions {
    record_receipt: bool,
    collect_git_metadata: bool,
    collect_worktree_fingerprint: bool,
    failure_mode: ToolFailureMode,
}

impl ManifestToolExecutionOptions {
    const fn fail_fast(
        record_receipt: bool,
        collect_git_metadata: bool,
        collect_worktree_fingerprint: bool,
    ) -> Self {
        Self {
            record_receipt,
            collect_git_metadata,
            collect_worktree_fingerprint,
            failure_mode: ToolFailureMode::FailFast,
        }
    }

    const fn collect_result(
        record_receipt: bool,
        collect_git_metadata: bool,
        collect_worktree_fingerprint: bool,
    ) -> Self {
        Self {
            record_receipt,
            collect_git_metadata,
            collect_worktree_fingerprint,
            failure_mode: ToolFailureMode::CollectResult,
        }
    }
}

fn execute_manifest_tool_with_options(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let tool = ctx
        .tool_spec(tool_name)
        .ok_or_else(|| anyhow!("{}", undeclared_tool_message(ctx, tool_name)))?;
    match tool.kind.as_str() {
        kind::NATIVE => {
            execute_native_tool(ctx, &tool.name, args, plan_id, options, position, observer)
        }
        kind::COMMAND => {
            let command_key = tool
                .command
                .as_deref()
                .ok_or_else(|| anyhow!("Command-backed tool is missing command: {tool_name}"))?;
            let command = ctx.command_for_key(command_key)?;
            execute_command_tool(
                ctx,
                CommandToolInvocation {
                    tool_name: &tool.name,
                    command_key,
                    command_text: command,
                },
                args,
                plan_id,
                options,
                position,
                observer,
            )
        }
        _ => bail!("Unsupported tool kind '{}' for {tool_name}", tool.kind),
    }
}

fn run_native_tool(
    ctx: &RepoContext,
    tool_name: &str,
    args_value: &Value,
) -> Result<NativeToolOutput> {
    match jig_features::native_tool_kind(tool_name)
        .ok_or_else(|| anyhow!("Unsupported native tool: {tool_name}"))?
    {
        NativeToolKind::ContractCheck => Ok(crate::policy::contract_check(ctx)),
        NativeToolKind::MigrationAdd => {
            let name = args_value
                .get(args::NAME)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
            crate::policy::migration_add(ctx, name)
        }
        NativeToolKind::SchemaCheck => crate::policy::schema_check(ctx),
        _ => bail!("Unsupported native tool kind for {tool_name}"),
    }
}

fn execute_native_tool(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let phase = ExecutionPhase::start(observer, tool_name, position);
    let output = run_native_tool(ctx, tool_name, &args);
    phase.finish(
        observer,
        output.as_ref().is_ok_and(|output| output.exit_status == 0),
    );
    let output = output?;
    let ended = now_ms();

    let receipt_result = maybe_record_receipt(
        ctx,
        options.record_receipt,
        ReceiptInput {
            tool_name,
            args: args.clone(),
            invoked_command_key: None,
            plan_id,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: output.exit_status,
            stdout: &output.stdout,
            stderr: &output.stderr,
            evidence: None,
            session_override: None,
            collect_git_metadata: options.collect_git_metadata,
            collect_worktree_fingerprint: options.collect_worktree_fingerprint,
            worktree_fingerprint_override: None,
        },
    );

    let tool_failure = tool_failure_message(
        tool_name,
        None,
        output.exit_status,
        &output.stdout,
        &output.stderr,
    );
    let receipt_id =
        receipt_id_for_failure_mode(options.failure_mode, tool_failure, receipt_result)?;

    tool_response_value(ToolExecutionResponse {
        ok: true,
        tool: tool_name,
        command_key: None,
        args,
        result: ToolProcessResult {
            exit_status: output.exit_status,
            stdout: output.stdout,
            stderr: output.stderr,
        },
        receipt_id,
    })
}

fn receipt_id_or_preserve_tool_error(
    tool_failure: Option<String>,
    receipt_result: Result<Option<String>>,
) -> Result<Option<String>> {
    if let Some(tool_failure) = tool_failure {
        match receipt_result {
            Ok(Some(receipt_id)) => bail!("{tool_failure}\nreceipt: {receipt_id}"),
            Ok(None) => bail!("{tool_failure}"),
            Err(receipt_error) => {
                bail!("{tool_failure}\nreceipt recording also failed:\n{receipt_error:#}")
            }
        }
    } else {
        receipt_result
    }
}

fn receipt_id_or_preserve_receipt_recording_context(
    tool_failure: Option<String>,
    receipt_result: Result<Option<String>>,
) -> Result<Option<String>> {
    match (tool_failure, receipt_result) {
        (Some(tool_failure), Err(receipt_error)) => {
            bail!("{tool_failure}\nreceipt recording also failed:\n{receipt_error:#}")
        }
        (_, receipt_result) => receipt_result,
    }
}

fn receipt_id_for_failure_mode(
    failure_mode: ToolFailureMode,
    tool_failure: Option<String>,
    receipt_result: Result<Option<String>>,
) -> Result<Option<String>> {
    match failure_mode {
        ToolFailureMode::FailFast => {
            receipt_id_or_preserve_tool_error(tool_failure, receipt_result)
        }
        ToolFailureMode::CollectResult => {
            receipt_id_or_preserve_receipt_recording_context(tool_failure, receipt_result)
        }
    }
}

struct CommandToolInvocation<'a> {
    tool_name: &'a str,
    command_key: &'a str,
    command_text: &'a str,
}

fn execute_command_tool(
    ctx: &RepoContext,
    invocation: CommandToolInvocation<'_>,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
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
        Ok(output) => output,
        Err(error) => {
            let message = format!("{error:#}");
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
                    stdout: "",
                    stderr: &message,
                    evidence: Some(evidence),
                    session_override: None,
                    collect_git_metadata: options.collect_git_metadata,
                    collect_worktree_fingerprint: options.collect_worktree_fingerprint,
                    worktree_fingerprint_override: None,
                },
            );
            let receipt_id = receipt_id_for_failure_mode(
                options.failure_mode,
                Some(message.clone()),
                receipt_result,
            )?;
            return tool_response_value(ToolExecutionResponse {
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
            });
        }
    };
    let exit_status = output.status.code().unwrap_or(1);
    let stdout = output
        .stdout
        .as_ref()
        .map_or_else(String::new, |output| output.to_string_lossy());
    let stderr = output
        .stderr
        .as_ref()
        .map_or_else(String::new, |output| output.to_string_lossy());

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
}

fn maybe_record_receipt(
    ctx: &RepoContext,
    should_record_receipt: bool,
    input: ReceiptInput<'_>,
) -> Result<Option<String>> {
    if should_record_receipt {
        record_receipt(ctx, input).map(Some)
    } else {
        Ok(None)
    }
}

fn tool_failure_message(
    tool_name: &str,
    command_key: Option<&str>,
    exit_status: i32,
    stdout: &str,
    stderr: &str,
) -> Option<String> {
    if exit_status == 0 {
        return None;
    }
    Some(match command_key {
        Some(command_key) => format!(
            "{tool_name} failed with status {exit_status}\ncommand key: {command_key}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
        None => format!(
            "{tool_name} failed with status {exit_status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
    })
}

fn run_configured_command(
    ctx: &RepoContext,
    tool_name: &str,
    command_text: &str,
    args: &Value,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<jig_owned_process::OwnedProcessTreeOutput> {
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
    let result = run_owned_process_tree_with_output_policy_and_observer(
        &mut command,
        ctx.command_timeout().duration(),
        ProcessOutputLimits {
            stdout: EXECUTION_OUTPUT_CAPTURE_LIMIT,
            stderr: EXECUTION_OUTPUT_CAPTURE_LIMIT,
        },
        ProcessOutputOverflowPolicy::Error,
        &mut ProcessExecutionObserver::new(observer, tool_name),
    )
    .map_err(|error| {
        anyhow!(
            "Configured command for {tool_name} failed under process supervision (timeout: {}s): {error}",
            ctx.command_timeout().as_secs()
        )
    })
    .and_then(|output| require_complete_command_output(output, tool_name));
    phase.finish(
        observer,
        result.as_ref().is_ok_and(|output| output.status.success()),
    );
    result
}

fn require_complete_command_output(
    output: jig_owned_process::OwnedProcessTreeOutput,
    tool_name: &str,
) -> Result<jig_owned_process::OwnedProcessTreeOutput> {
    for (stream, capture) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
        let capture = capture.as_ref().with_context(|| {
            format!("Configured command for {tool_name} did not capture {stream}")
        })?;
        if capture.truncated {
            bail!(
                "Configured command for {tool_name} exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte {stream} capture limit"
            );
        }
        if !capture.complete {
            bail!("Configured command for {tool_name} did not finish capturing {stream}");
        }
    }
    Ok(output)
}

#[derive(Serialize)]
struct ToolExecutionResponse<'a> {
    ok: bool,
    tool: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_key: Option<&'a str>,
    args: Value,
    result: ToolProcessResult,
    receipt_id: Option<String>,
}

#[derive(Serialize)]
struct ToolProcessResult {
    exit_status: i32,
    stdout: String,
    stderr: String,
}

fn tool_response_value(response: ToolExecutionResponse<'_>) -> Result<Value> {
    serde_json::to_value(response).context("Failed to serialize tool execution response")
}
