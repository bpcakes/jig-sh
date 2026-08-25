use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use jig_contract::{ManifestTool, NativeToolKind, TargetId};
use serde::Serialize;
use serde_json::Value;

use crate::context::RepoContext;
#[cfg(test)]
use crate::execution::NoopExecutionObserver;
use crate::execution::{
    ExecutionCommandError, ExecutionControl, ExecutionPhase, PhasePosition,
    SupervisedExecutionError, run_supervised_execution_command,
};
use crate::policy::NativeToolOutput;
use crate::state::{ReceiptInput, now_ms, record_receipt_with_cancellation};
use crate::tool_defs::{self, JsonObject, args, kind, string_arg, tool};

mod failure;

pub(in crate::runtime) use failure::manifest_tool_result_failure;
use failure::tool_failure_message;

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

pub(super) fn call_manifest_tool_with_observer(
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

pub(super) fn execute_manifest_tool_with_observer(
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
    )?
    .into_value()
}

#[cfg(test)]
pub(super) fn execute_manifest_tool_result_without_worktree_fingerprint(
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
    )?
    .into_value()
}

pub(super) fn execute_manifest_tool_with_options_for_work_check(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
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

pub(super) enum ManifestToolExecutionOutcome {
    Completed(Value),
    Cancelled(Value),
}

impl ManifestToolExecutionOutcome {
    fn into_value(self) -> Result<Value> {
        match self {
            Self::Completed(value) => Ok(value),
            Self::Cancelled(value) => {
                let mut message = manifest_tool_result_failure(&value)?.map_or_else(
                    || "Tool execution was cancelled".to_string(),
                    |(_, message)| message,
                );
                if let Some(receipt_id) = value.get("receipt_id").and_then(Value::as_str) {
                    message.push_str("\nreceipt: ");
                    message.push_str(receipt_id);
                }
                bail!("{message}")
            }
        }
    }
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
) -> Result<ManifestToolExecutionOutcome> {
    if let Some(error) = jig_features::tool_admission_error(ctx, tool_name) {
        bail!(error);
    }
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

pub(super) fn run_native_tool_with_control(
    ctx: &RepoContext,
    tool_name: &str,
    target: Option<&TargetId>,
    args_value: &Value,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeToolOutput> {
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .ok_or(jig_owned_process::OwnedProcessTreeError::TimedOut)?;
    check_native_control(deadline, cancelled)?;
    let output = match jig_features::native_tool_kind(tool_name)
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
        NativeToolKind::SchemaCheck => {
            crate::policy::schema_check_with_control(ctx, target, timeout, cancelled)
        }
        _ => bail!("Unsupported native tool kind for {tool_name}"),
    }?;
    // Once an in-process tool returns, its effects may already be durable.
    // Completion is therefore authoritative; a late timeout observation must
    // not report that a completed mutation did not happen.
    Ok(bound_native_output(output))
}

fn check_native_control(deadline: std::time::Instant, cancelled: &dyn Fn() -> bool) -> Result<()> {
    if cancelled() {
        return Err(jig_owned_process::OwnedProcessTreeError::Cancelled.into());
    }
    if std::time::Instant::now() >= deadline {
        return Err(jig_owned_process::OwnedProcessTreeError::TimedOut.into());
    }
    Ok(())
}

enum NativeToolRun {
    Completed(NativeToolOutput),
    CancelledBeforeStart,
    Cancelled,
}

fn run_native_tool(
    ctx: &RepoContext,
    tool_name: &str,
    args_value: &Value,
    observer: &mut dyn ExecutionControl,
) -> Result<NativeToolRun> {
    let output = match jig_features::native_tool_kind(tool_name)
        .ok_or_else(|| anyhow!("Unsupported native tool: {tool_name}"))?
    {
        NativeToolKind::ContractCheck => {
            Ok(NativeToolRun::Completed(crate::policy::contract_check(ctx)))
        }
        NativeToolKind::MigrationAdd => {
            let name = args_value
                .get(args::NAME)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
            crate::policy::migration_add(ctx, name).map(NativeToolRun::Completed)
        }
        NativeToolKind::SchemaCheck => {
            let target = ctx.action_specs().iter().find_map(|action| {
                action
                    .legacy_aliases
                    .iter()
                    .any(|alias| alias == tool_name)
                    .then_some(&action.target)
            });
            match crate::policy::schema_check_with_observer(ctx, target, observer) {
                Ok(output) => Ok(NativeToolRun::Completed(output)),
                Err(ExecutionCommandError::CancelledBeforeStart) => {
                    Ok(NativeToolRun::CancelledBeforeStart)
                }
                Err(ExecutionCommandError::Cancelled) => Ok(NativeToolRun::Cancelled),
                Err(ExecutionCommandError::Failed(error)) => Err(error),
            }
        }
        _ => bail!("Unsupported native tool kind for {tool_name}"),
    }?;
    Ok(match output {
        NativeToolRun::Completed(output) => NativeToolRun::Completed(bound_native_output(output)),
        NativeToolRun::CancelledBeforeStart => NativeToolRun::CancelledBeforeStart,
        NativeToolRun::Cancelled => NativeToolRun::Cancelled,
    })
}

fn bound_native_output(mut output: NativeToolOutput) -> NativeToolOutput {
    let limits = jig_owned_process::ProcessOutputLimits::default();
    truncate_native_stream(&mut output.stdout, limits.stdout);
    truncate_native_stream(&mut output.stderr, limits.stderr);
    output
}

fn truncate_native_stream(value: &mut String, limit: usize) {
    if value.len() <= limit {
        return;
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.push_str("\n[output truncated by Jig]\n");
}

fn execute_native_tool(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
    let started = now_ms();
    if observer.cancelled() {
        return cancelled_native_tool_outcome(
            ctx,
            CancelledNativeToolRequest {
                tool_name,
                args,
                plan_id,
                options,
                started,
                before_start: true,
            },
            observer,
        );
    }
    let phase = ExecutionPhase::start(observer, tool_name, position);
    let output = run_native_tool(ctx, tool_name, &args, observer);
    phase.finish(
        observer,
        output.as_ref().is_ok_and(
            |output| matches!(output, NativeToolRun::Completed(output) if output.exit_status == 0),
        ),
    );
    let output = match output? {
        NativeToolRun::Completed(output) => output,
        NativeToolRun::CancelledBeforeStart => {
            return cancelled_native_tool_outcome(
                ctx,
                CancelledNativeToolRequest {
                    tool_name,
                    args,
                    plan_id,
                    options,
                    started,
                    before_start: true,
                },
                observer,
            );
        }
        NativeToolRun::Cancelled => {
            return cancelled_native_tool_outcome(
                ctx,
                CancelledNativeToolRequest {
                    tool_name,
                    args,
                    plan_id,
                    options,
                    started,
                    before_start: false,
                },
                observer,
            );
        }
    };
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
        &|| observer.cancelled(),
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
    .map(ManifestToolExecutionOutcome::Completed)
}

struct CancelledNativeToolRequest<'a> {
    tool_name: &'a str,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    started: u64,
    before_start: bool,
}

fn cancelled_native_tool_outcome(
    ctx: &RepoContext,
    request: CancelledNativeToolRequest<'_>,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
    let CancelledNativeToolRequest {
        tool_name,
        args,
        plan_id,
        options,
        started,
        before_start,
    } = request;
    let message = if before_start {
        format!("Native tool {tool_name} was cancelled before it started")
    } else {
        format!("Native tool {tool_name} was cancelled")
    };
    let receipt_id = if before_start {
        None
    } else {
        let evidence = serde_json::json!({
            "kind": "supervised_command",
            "schema_version": 1,
            "status": "cancelled",
            "error": message,
        });
        maybe_record_receipt(
            ctx,
            options.record_receipt,
            ReceiptInput {
                tool_name,
                args: args.clone(),
                invoked_command_key: None,
                plan_id,
                started_at_ms: started,
                ended_at_ms: now_ms(),
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
        )?
    };
    let response = tool_response_value(ToolExecutionResponse {
        ok: true,
        tool: tool_name,
        command_key: None,
        args,
        result: ToolProcessResult {
            exit_status: 1,
            stdout: String::new(),
            stderr: message,
        },
        receipt_id,
    })?;
    Ok(ManifestToolExecutionOutcome::Cancelled(response))
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

mod command_tool;
use command_tool::*;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn native_output_is_bounded_at_the_common_execution_seam() {
        let limit = jig_owned_process::ProcessOutputLimits::default().stdout;
        let output = bound_native_output(NativeToolOutput {
            exit_status: 0,
            stdout: "x".repeat(limit + 1),
            stderr: "y".repeat(limit + 1),
        });

        assert!(output.stdout.starts_with(&"x".repeat(limit)));
        assert!(output.stdout.ends_with("[output truncated by Jig]\n"));
        assert!(output.stderr.ends_with("[output truncated by Jig]\n"));
    }

    #[test]
    fn native_runner_rejects_an_elapsed_timeout_before_start() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let error = run_native_tool_with_control(
            &ctx,
            tool::CONTRACT_CHECK,
            None,
            &serde_json::json!({}),
            Duration::ZERO,
            &|| false,
        )
        .unwrap_err();

        assert!(matches!(
            error.downcast_ref::<jig_owned_process::OwnedProcessTreeError>(),
            Some(jig_owned_process::OwnedProcessTreeError::TimedOut)
        ));
    }

    #[test]
    fn cancellation_after_an_in_process_mutation_does_not_reclassify_completion() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .config(
                r#"
backend_language = "go"
go_database = "postgres"
migration_dir = "migrations"
"#,
            )
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let probes = AtomicUsize::new(0);

        let output = run_native_tool_with_control(
            &ctx,
            tool::MIGRATION_ADD,
            None,
            &serde_json::json!({args::NAME: "create_examples"}),
            Duration::from_secs(30),
            &|| probes.fetch_add(1, Ordering::SeqCst) > 0,
        )
        .unwrap();

        assert_eq!(output.exit_status, 0);
        assert_eq!(probes.load(Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read_dir(temp.path().join("migrations"))
                .unwrap()
                .count(),
            1
        );
    }
}
