use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::time::Duration;

use crate::command::{AgentMapCommand, CheckCommand, RuntimeCommand, StateCommand};
use crate::context::RepoContext;
use crate::execution::{ExecutionObserver, NoopExecutionObserver};
use crate::policy::{
    AgentMapInput, MigrationImmutabilityInput, PolicyCheckCommand, PolicyDirectCommand,
    RustFileLocInput, SqlxTodoInput,
};
use crate::tool_defs::{self, MemoryTool, tool};

mod agent;
mod loops;
mod prompt;
mod sqlx;
mod tool_execution;
mod vault;
mod vault_env;
mod vault_import;
mod work;
mod worker_runner;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VaultRawOutcome {
    Complete,
    ChildExit(i32),
}

pub(crate) type CodexSupportProbeResult = std::result::Result<bool, String>;

pub(crate) fn agent_doctor_with_codex_support_probe(
    ctx: &RepoContext,
    probe: impl FnMut(&OsStr) -> CodexSupportProbeResult,
) -> Value {
    agent::doctor_with_codex_support_probe(ctx, probe)
}

pub(crate) fn agent_doctor_for_inventory(ctx: &RepoContext, human_progress: bool) -> Value {
    agent::doctor_for_inventory(ctx, human_progress)
}

pub(crate) fn probe_codex_marketplace_support(
    codex_bin: &OsStr,
    timeout: Duration,
    cancelled: impl FnMut() -> bool,
) -> CodexSupportProbeResult {
    agent::codex_supports_plugin_marketplaces_with_timeout_and_cancellation(
        codex_bin, timeout, cancelled,
    )
}

pub(crate) fn dispatch(ctx: &RepoContext, command: RuntimeCommand) -> Result<Value> {
    dispatch_with_observer(ctx, command, &mut NoopExecutionObserver)
}

pub(crate) fn dispatch_with_observer(
    ctx: &RepoContext,
    command: RuntimeCommand,
    observer: &mut dyn ExecutionObserver,
) -> Result<Value> {
    match command {
        RuntimeCommand::Bootstrap(opts) => {
            tool_execution::execute_manifest_tool_request_with_observer(
                ctx,
                tool::BOOTSTRAP,
                json!({}),
                opts,
                observer,
            )
        }
        RuntimeCommand::Check(command) => dispatch_check_with_observer(ctx, command, observer),
        RuntimeCommand::Sqlx(command) => sqlx::dispatch_with_observer(ctx, command, observer),
        RuntimeCommand::AgentMap(AgentMapCommand::Generate(opts)) => crate::policy::run_direct(
            ctx,
            PolicyDirectCommand::AgentMapGenerate(AgentMapInput {
                map_path: opts.map_path,
            }),
        ),
        RuntimeCommand::GenerateSqlxUncheckedQueriesTodo(opts) => crate::policy::run_direct(
            ctx,
            PolicyDirectCommand::GenerateSqlxUncheckedQueriesTodo(SqlxTodoInput {
                output: opts.output,
            }),
        ),
        RuntimeCommand::Dev(opts) => crate::dev_proxy::commands::dev(ctx, opts),
        RuntimeCommand::Proxy(command) => crate::dev_proxy::commands::proxy(ctx, command),
        RuntimeCommand::Agent(command) => agent::dispatch_with_observer(ctx, command, observer),
        RuntimeCommand::Work(command) => work::dispatch_with_observer(ctx, command, observer),
        RuntimeCommand::Loop(command) => loops::dispatch_with_observer(ctx, command, observer),
        RuntimeCommand::State(command) => dispatch_state(ctx, command),
    }
}

fn dispatch_state(ctx: &RepoContext, command: StateCommand) -> Result<Value> {
    match command {
        StateCommand::Summary => crate::state::state_summary(ctx).map(|mut value| {
            value["command"] = json!("state summary");
            value
        }),
        StateCommand::Diagnose(request) => Ok(crate::state::state_diagnose(ctx, request)),
        StateCommand::CompactSessions(request) => crate::state::compact_sessions(ctx, request),
        StateCommand::Restore(request) => crate::state::restore_backup(ctx, request),
        StateCommand::ExportReceipts(request) => {
            crate::state::receipts_export(ctx, &request.before, &request.output)
        }
        StateCommand::Archive(request) => crate::state::receipts_archive(
            ctx,
            crate::state::StateArchiveRequest {
                before: request.before,
                dry_run: request.dry_run,
            },
        ),
    }
}

/// Read-only gate status for `jig ui`; reuses the `work gates` evaluation.
pub(crate) fn work_gates_snapshot(ctx: &RepoContext, plan_id: Option<String>) -> Result<Value> {
    work::gates_snapshot(ctx, plan_id)
}

#[allow(dead_code)]
pub(crate) fn work_gates_snapshot_with_cancellation(
    ctx: &RepoContext,
    plan_id: Option<String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    work::gates_snapshot_with_cancellation(ctx, plan_id, cancelled)
}

pub(crate) fn open_plan_gate_snapshots_with_cancellation(
    ctx: &RepoContext,
    plan_ids: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Result<std::collections::BTreeMap<String, Value>> {
    work::open_plan_gate_snapshots_with_cancellation(ctx, plan_ids, cancelled)
}

/// Read-only loop workflow status for `jig ui`; reuses `loop status`.
pub(crate) fn loop_status_snapshot(ctx: &RepoContext) -> Result<Value> {
    loop_status_snapshot_with_cancellation(ctx, &|| false)
}

pub(crate) fn loop_status_snapshot_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    loops::status_with_cancellation(
        ctx,
        crate::command::LoopStatusRequest { workflow: None },
        cancelled,
    )
}

pub(crate) fn dispatch_vault(command: crate::command::VaultCommand) -> Result<Value> {
    #[cfg(test)]
    return vault::dispatch_for_test(command);

    #[cfg(not(test))]
    vault::dispatch(command)
}

pub(crate) fn dispatch_vault_raw(command: crate::command::VaultCommand) -> Result<VaultRawOutcome> {
    vault::dispatch_raw(command)
}

pub(crate) fn prepare_vault_raw_input(command: &mut crate::command::VaultCommand) -> Result<()> {
    vault::prepare_raw_input(command)
}

pub(crate) fn preflight_scoped_vault_command(
    command: &mut crate::command::VaultCommand,
) -> Result<()> {
    vault::preflight_scoped_command(command)
}

pub(crate) fn dispatch_prompt(
    ctx: Option<&RepoContext>,
    command: crate::command::PromptCommand,
) -> Result<Value> {
    prompt::dispatch(ctx, command)
}

pub(crate) fn capture_vault_passphrase() -> Result<()> {
    // SAFETY: Callers must invoke this before starting background threads in the
    // process; `runtime::vault` clears the captured environment variable.
    vault::capture_passphrase()
}

pub(crate) fn capture_new_vault_passphrase() -> Result<()> {
    // SAFETY: Callers must invoke this before starting background threads in the
    // process; `runtime::vault` clears the captured environment variable.
    vault::capture_new_passphrase()
}

pub(crate) fn capture_vault_passphrase_change() -> Result<()> {
    // SAFETY: Callers must invoke this before starting background threads in the
    // process; `runtime::vault` clears both captured environment variables.
    vault::capture_passphrase_change()
}

pub(crate) fn strip_vault_passphrase_environment() {
    vault::strip_passphrase_environment();
}

pub(crate) fn take_optional_vault_tui_passphrase() -> Result<Option<jig_vault::SecretBytes>> {
    vault::take_optional_tui_passphrase()
}

pub(crate) fn run_vault_tui(
    request: crate::command::VaultTuiRequest,
    initial_passphrase: Option<jig_vault::SecretBytes>,
) -> Result<()> {
    vault::tui::run(request, initial_passphrase)
}

pub(crate) fn vault_passphrase_prompt_available() -> bool {
    vault::passphrase_prompt_available()
}

pub(crate) fn vault_passphrase_env_present() -> bool {
    vault::passphrase_env_present()
}

pub(crate) fn repo_vault_options_for_context(
    ctx: &RepoContext,
) -> Option<crate::command::VaultRuntimeOptions> {
    let scope_id = ctx.vault_config().repo_scope_id()?;
    Some(crate::command::VaultRuntimeOptions::repo(
        scope_id,
        ctx.repo_name(),
        ctx.root(),
    ))
}

pub(crate) fn vault_options_for_context(
    ctx: Option<&RepoContext>,
) -> crate::command::VaultRuntimeOptions {
    ctx.and_then(repo_vault_options_for_context)
        .unwrap_or_default()
}

fn dispatch_check_with_observer(
    ctx: &RepoContext,
    command: CheckCommand,
    observer: &mut dyn ExecutionObserver,
) -> Result<Value> {
    match command {
        CheckCommand::Fmt(opts) => tool_execution::execute_manifest_tool_request_with_observer(
            ctx,
            tool::FMT_CHECK,
            json!({}),
            opts,
            observer,
        ),
        CheckCommand::Clippy(opts) => tool_execution::execute_manifest_tool_request_with_observer(
            ctx,
            tool::CLIPPY,
            json!({}),
            opts,
            observer,
        ),
        CheckCommand::Test(opts) => tool_execution::execute_manifest_tool_request_with_observer(
            ctx,
            tool::TEST,
            json!({}),
            opts,
            observer,
        ),
        CheckCommand::TestLocked(opts) => {
            tool_execution::execute_manifest_tool_request_with_observer(
                ctx,
                tool::TEST_LOCKED,
                json!({}),
                opts,
                observer,
            )
        }
        CheckCommand::TypeScriptLint(opts) => {
            tool_execution::execute_manifest_tool_request_with_observer(
                ctx,
                tool::TYPESCRIPT_LINT,
                json!({}),
                opts,
                observer,
            )
        }
        CheckCommand::TypeScriptTypecheck(opts) => {
            tool_execution::execute_manifest_tool_request_with_observer(
                ctx,
                tool::TYPESCRIPT_TYPECHECK,
                json!({}),
                opts,
                observer,
            )
        }
        CheckCommand::TypeScriptBuild(opts) => {
            tool_execution::execute_manifest_tool_request_with_observer(
                ctx,
                tool::TYPESCRIPT_BUILD,
                json!({}),
                opts,
                observer,
            )
        }
        CheckCommand::TypeScriptCoverage(opts) => {
            tool_execution::execute_manifest_tool_request_with_observer(
                ctx,
                tool::TYPESCRIPT_COVERAGE,
                json!({}),
                opts,
                observer,
            )
        }
        CheckCommand::Sqlx(opts) => tool_execution::execute_manifest_tool_request_with_observer(
            ctx,
            tool::SQLX_CHECK,
            json!({}),
            opts,
            observer,
        ),
        CheckCommand::Schema(opts) => tool_execution::execute_manifest_tool_request_with_observer(
            ctx,
            tool::SCHEMA_CHECK,
            json!({}),
            opts,
            observer,
        ),
        CheckCommand::Contract(opts) => {
            tool_execution::execute_manifest_tool_request_with_observer(
                ctx,
                tool::CONTRACT_CHECK,
                json!({}),
                opts,
                observer,
            )
        }
        CheckCommand::AgentMap(opts) => crate::policy::run_check(
            ctx,
            PolicyCheckCommand::AgentMap(AgentMapInput {
                map_path: opts.map_path,
            }),
        ),
        CheckCommand::AgentGuides => crate::policy::run_check(ctx, PolicyCheckCommand::AgentGuides),
        CheckCommand::RustFileLoc(opts) => crate::policy::run_check(
            ctx,
            PolicyCheckCommand::RustFileLoc(RustFileLocInput {
                staged: opts.staged,
                changed_against: opts.changed_against,
                all: opts.all,
            }),
        ),
        CheckCommand::NoModRs => crate::policy::run_check(ctx, PolicyCheckCommand::NoModRs),
        CheckCommand::MigrationImmutability(opts) => crate::policy::run_check(
            ctx,
            PolicyCheckCommand::MigrationImmutability(MigrationImmutabilityInput {
                changed_against: opts.changed_against,
            }),
        ),
        CheckCommand::SqlxUncheckedNonTest => {
            crate::policy::run_check(ctx, PolicyCheckCommand::SqlxUncheckedNonTest)
        }
    }
}

#[cfg(test)]
pub(crate) fn call_tool(ctx: &RepoContext, name: &str, args: Value) -> Result<Value> {
    call_tool_with_observer(ctx, name, args, &mut NoopExecutionObserver)
}

pub(crate) fn call_tool_with_observer(
    ctx: &RepoContext,
    name: &str,
    args: Value,
    observer: &mut dyn ExecutionObserver,
) -> Result<Value> {
    let args_obj = args.as_object().cloned().unwrap_or_default();

    match ctx.tool_spec(name) {
        Some(tool) if tool_defs::is_execution_tool(tool) => {
            return tool_execution::call_manifest_tool_with_observer(
                ctx, tool, &args_obj, observer,
            );
        }
        _ => {}
    }

    // MCP dispatch is intentionally allowlisted here. CLI-only dev/proxy
    // commands can start processes, install services, or mutate trust stores
    // and must not become agent-callable by adding names to tool_defs.
    match MemoryTool::from_name(name) {
        Some(MemoryTool::AgentDoctor) => Ok(agent::doctor(ctx)),
        Some(MemoryTool::Goal) => work::goal_from_args(ctx, args),
        Some(MemoryTool::Start) => work::start_from_args(ctx, args),
        Some(MemoryTool::Append) => work::append_from_args(ctx, args),
        Some(MemoryTool::Check) => work::check_from_args_with_observer(ctx, args, observer),
        Some(MemoryTool::Gates) => work::gates_from_args(ctx, args),
        Some(MemoryTool::Evidence) => work::evidence_from_args(ctx, args),
        Some(MemoryTool::Review) => work::review_from_args_with_observer(ctx, args, observer),
        Some(MemoryTool::Refine) => work::refine_from_args_with_observer(ctx, args, observer),
        Some(MemoryTool::Decide) => work::decide_from_args(ctx, args),
        Some(MemoryTool::Receipts) => work::receipts_from_args(ctx, args),
        Some(MemoryTool::Status) => crate::state::state_summary(ctx),
        Some(MemoryTool::Finish) => work::finish_from_args(ctx, args),
        None => bail!("Unsupported tool: {name}"),
    }
}

#[cfg(test)]
mod tests;
