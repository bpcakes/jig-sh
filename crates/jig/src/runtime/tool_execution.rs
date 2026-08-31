use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use jig_contract::{
    ActionRunner, ActionSpec, ComparisonPreparationV1, Finding, FindingLocation, FindingSeverity,
    ManifestTool, NativeActionResult, NativeToolKind, PolicyPreparationV1, PreparedNativeInputV1,
    RunConclusion, TargetId,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
#[cfg(test)]
use crate::execution::NoopExecutionObserver;
use crate::execution::{
    ExecutionCommandError, ExecutionControl, ExecutionPhase, PhasePosition,
    SupervisedExecutionError, run_supervised_execution_command,
};
use crate::policy::NativeToolOutput;

pub(super) struct NativeActionContext<'a> {
    pub(super) repository: &'a RepoContext,
    pub(super) prepared_input: &'a PreparedNativeInputV1,
    pub(super) deadline: std::time::Instant,
    pub(super) cancelled: &'a dyn Fn() -> bool,
    pub(super) run_id: &'a str,
    pub(super) target: &'a TargetId,
    pub(super) work_plan_id: Option<&'a str>,
}

pub(super) fn run_prepared_native_action(
    context: NativeActionContext<'_>,
) -> Result<NativeActionResult> {
    if (context.cancelled)() {
        return Ok(native_terminal_result(
            RunConclusion::Cancelled,
            "file_budget.cancelled",
            "file-budget evaluation was cancelled before it started",
            None,
        ));
    }
    if std::time::Instant::now() >= context.deadline {
        return Ok(native_terminal_result(
            RunConclusion::TimedOut,
            "file_budget.timed_out",
            "file-budget evaluation reached its deadline before it started",
            None,
        ));
    }
    let prepared = context.prepared_input;
    let evidence = json!({
        "schema": "jig.file_budget/prepared-v1",
        "prepared_input_schema_version": prepared.schema_version,
        "policy_source": prepared.policy_source,
        "view": prepared.view,
        "request": prepared.request,
        "configuration": prepared.configuration,
        "policy": prepared.policy,
        "comparison": prepared.comparison,
        "run_id": context.run_id,
        "target": context.target,
        "work_plan_id": context.work_plan_id,
        "repository_contract_version": context.repository.contract_version(),
    });
    if let PolicyPreparationV1::InvalidPolicy {
        diagnostics_count,
        diagnostics_digest,
        diagnostics_preview,
        ..
    } = &prepared.policy
    {
        let findings = diagnostics_preview
            .iter()
            .map(|diagnostic| Finding {
                severity: diagnostic.severity,
                message: diagnostic.message.clone(),
                code: Some(diagnostic.code.clone()),
                source: Some(jig_contract::tool::FILE_BUDGET.into()),
                location: diagnostic.path.as_ref().map(|path| FindingLocation {
                    path: path.clone(),
                    line: None,
                    column: None,
                }),
            })
            .collect::<Vec<_>>();
        let mut result = native_result(
            RunConclusion::Failure,
            findings,
            *diagnostics_count,
            Some(evidence),
        );
        // Preparation authenticated this digest over the complete diagnostic
        // set before bounding the persisted preview. Preserve that complete
        // identity when the normalized finding preview is truncated.
        result.findings_digest.clone_from(diagnostics_digest);
        return Ok(result);
    }
    if let ComparisonPreparationV1::ComparisonUnavailable { reason, .. } = &prepared.comparison {
        return Ok(native_terminal_result(
            RunConclusion::Blocked,
            "file_budget.baseline_unavailable",
            &reason.message,
            Some(evidence),
        ));
    }
    super::file_budget::execute_prepared_file_budget(super::file_budget::FileBudgetEngineContext {
        repository: context.repository,
        prepared_input: context.prepared_input,
        deadline: context.deadline,
        cancelled: context.cancelled,
        mode: super::file_budget::FileBudgetEvaluationMode::Check,
    })
}

fn native_terminal_result(
    conclusion: RunConclusion,
    code: &str,
    message: &str,
    evidence: Option<Value>,
) -> NativeActionResult {
    let mut finding = Finding::new(FindingSeverity::Error, message);
    finding.code = Some(code.into());
    finding.source = Some(jig_contract::tool::FILE_BUDGET.into());
    native_result(conclusion, vec![finding], 1, evidence)
}

fn native_result(
    conclusion: RunConclusion,
    findings: Vec<Finding>,
    finding_count: u64,
    evidence: Option<Value>,
) -> NativeActionResult {
    use sha2::{Digest, Sha256};

    let encoded = serde_json::to_vec(&findings).expect("native findings are serializable");
    let mut hasher = Sha256::new();
    hasher.update(b"jig-native-findings-v1\0");
    hasher.update(finding_count.to_be_bytes());
    hasher.update((encoded.len() as u64).to_be_bytes());
    hasher.update(encoded);
    NativeActionResult {
        conclusion,
        human_output: findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        findings_truncated: finding_count > findings.len() as u64,
        findings_digest: format!("sha256:{:x}", hasher.finalize()),
        findings,
        finding_count,
        evidence,
        evaluated_at_ms: crate::state::now_ms(),
        valid_until_ms: None,
    }
}
use crate::repository::RepositoryCatalog;
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

pub(super) fn execute_manifest_tool_without_lease_wait_for_work_check(
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
        ManifestToolExecutionOptions::collect_result_without_lease_wait(true, false, false),
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
    lease_contention: LeaseContention,
}

#[derive(Clone, Copy)]
enum LeaseContention {
    Wait,
    Reject,
}

impl LeaseContention {
    fn acquire(
        self,
        ctx: &RepoContext,
        effects: &[jig_contract::ActionEffect],
    ) -> Result<crate::state::RepositoryExecutionLease> {
        match self {
            Self::Wait => crate::state::acquire_repository_execution_lease(ctx, effects),
            Self::Reject => {
                crate::state::acquire_repository_execution_lease_without_wait(ctx, effects)
            }
        }
    }
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
            lease_contention: LeaseContention::Wait,
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
            lease_contention: LeaseContention::Wait,
        }
    }

    const fn collect_result_without_lease_wait(
        record_receipt: bool,
        collect_git_metadata: bool,
        collect_worktree_fingerprint: bool,
    ) -> Self {
        Self {
            record_receipt,
            collect_git_metadata,
            collect_worktree_fingerprint,
            failure_mode: ToolFailureMode::CollectResult,
            lease_contention: LeaseContention::Reject,
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
    let current = if ctx.contract_version() >= 6 {
        super::refreshed_repository_context(ctx)?
    } else {
        ctx.clone()
    };
    let tool = current
        .tool_spec(tool_name)
        .cloned()
        .ok_or_else(|| anyhow!("{}", undeclared_tool_message(&current, tool_name)))?;
    if current.contract_version() >= 6 {
        let catalog = RepositoryCatalog::from_context(&current)?;
        if catalog.action_for_alias(tool_name).is_none() {
            bail!(
                "Contract-v6 tool '{tool_name}' does not resolve to a repository action through legacy_aliases"
            );
        }
        return execute_v6_action_alias(
            current, tool_name, args, plan_id, options, position, observer,
        );
    }
    if let Some(error) = jig_features::tool_admission_error(&current, tool_name) {
        bail!(error);
    }
    match tool.kind.as_str() {
        kind::NATIVE => execute_native_tool(
            &current,
            NativeToolInvocation {
                tool_name: &tool.name,
                operation: &tool.name,
                target: None,
                timeout_seconds: None,
            },
            args,
            plan_id,
            options,
            position,
            observer,
        ),
        kind::COMMAND => {
            let command_key = tool
                .command
                .as_deref()
                .ok_or_else(|| anyhow!("Command-backed tool is missing command: {tool_name}"))?;
            let command = current.command_for_key(command_key)?;
            execute_command_tool(
                &current,
                CommandToolInvocation {
                    tool_name: &tool.name,
                    command_key,
                    command_text: command,
                    working_directory: None,
                    environment: None,
                    timeout: current.command_timeout().duration(),
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

fn execute_v6_action_alias(
    mut current: RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
    loop {
        let action = resolve_action_alias(&current, tool_name)?;
        validate_action_admission(&current, tool_name, &action)?;
        // Contract-v6 actions are the authority for both dispatch and checkout
        // effects. The authority can change while this blocks, so this lease is
        // only admission to a second resolution below, not permission to run
        // the action value resolved above.
        let repository_execution = options
            .lease_contention
            .acquire(&current, &action.effects)?;

        let refreshed = super::refreshed_repository_context(&current)?;
        let tool = refreshed
            .tool_spec(tool_name)
            .cloned()
            .ok_or_else(|| anyhow!("{}", undeclared_tool_message(&refreshed, tool_name)))?;
        let action = resolve_action_alias(&refreshed, tool_name)?;
        validate_action_admission(&refreshed, tool_name, &action)?;
        if !repository_execution.permits(&action.effects) {
            // Authority became more effectful while a shared lease was being
            // acquired. Drop it and repeat from that newly refreshed authority
            // so dispatch never runs beneath a weaker isolation mode.
            drop(repository_execution);
            current = refreshed;
            continue;
        }

        return execute_action_alias(
            &refreshed,
            &tool,
            action,
            args,
            plan_id,
            options,
            position,
            observer,
            repository_execution,
        );
    }
}

fn resolve_action_alias(ctx: &RepoContext, tool_name: &str) -> Result<ActionSpec> {
    RepositoryCatalog::from_context(ctx)?
        .action_for_alias(tool_name)
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "Contract-v6 compatibility alias '{tool_name}' no longer resolves to a repository action"
            )
        })
}

fn validate_action_admission(
    ctx: &RepoContext,
    tool_name: &str,
    action: &ActionSpec,
) -> Result<()> {
    let admission_name = match &action.runner {
        ActionRunner::Native { operation, .. } => operation.as_str(),
        ActionRunner::Command { .. } => tool_name,
    };
    if let Some(error) = jig_features::tool_admission_error(ctx, admission_name) {
        bail!(error);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_action_alias(
    ctx: &RepoContext,
    tool: &ManifestTool,
    action: ActionSpec,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
    repository_execution: crate::state::RepositoryExecutionLease,
) -> Result<ManifestToolExecutionOutcome> {
    let outcome = match action.runner {
        ActionRunner::Native { operation, .. } => execute_native_tool(
            ctx,
            NativeToolInvocation {
                tool_name: &tool.name,
                operation: &operation,
                target: Some(&action.target),
                timeout_seconds: action.timeout_seconds,
            },
            args,
            plan_id,
            options,
            position,
            observer,
        ),
        ActionRunner::Command {
            command,
            working_directory,
            environment,
        } => {
            let command_text = ctx.command_for_key(&command)?;
            execute_command_tool(
                ctx,
                CommandToolInvocation {
                    tool_name: &tool.name,
                    command_key: &command,
                    command_text,
                    working_directory: working_directory.as_deref(),
                    environment: Some(&environment),
                    timeout: action
                        .timeout_seconds
                        .map(Duration::from_secs)
                        .unwrap_or_else(|| ctx.command_timeout().duration()),
                },
                args,
                plan_id,
                options,
                position,
                observer,
            )
        }
    };
    drop(repository_execution);
    outcome
}

mod native;
pub(super) use native::run_native_tool_with_control;
use native::*;

mod command_tool;
use command_tool::*;

#[cfg(test)]
mod tests;
