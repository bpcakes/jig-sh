use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::time::Duration;

use crate::command::{AgentMapCommand, CheckCommand, RuntimeCommand, StateCommand};
use crate::context::RepoContext;
use crate::execution::{ExecutionControl, NoopExecutionObserver};
use crate::policy::{
    AgentMapInput, MigrationImmutabilityInput, PolicyCheckCommand, PolicyDirectCommand,
    SqlxTodoInput,
};
use crate::tool_defs::{self, MemoryTool, tool};

mod agent;
mod file_budget;
mod loops;
mod mcp_repository;
mod migration;
mod prompt;
mod run_execution;
mod sqlx;
mod tool_execution;
mod vault;
mod vault_env;
mod vault_import;
mod work;

pub(crate) use file_budget::{FileBudgetEvaluationMode, run_direct_file_budget};
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

pub(crate) fn wait_for_mcp_repository_runs(ctx: &RepoContext) {
    mcp_repository::wait_for_live_runs(ctx);
}

pub(crate) fn dispatch(ctx: &RepoContext, command: RuntimeCommand) -> Result<Value> {
    dispatch_with_observer(ctx, command, &mut NoopExecutionObserver)
}

pub(crate) fn dispatch_with_observer(
    ctx: &RepoContext,
    command: RuntimeCommand,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    if observer.cancelled() {
        bail!("Execution was cancelled");
    }
    // Each operation owns any cancellation checks after entry so it can stop
    // before its durable commit point. Re-checking here after a successful
    // return would turn an already-committed mutation into an apparent failure.
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
        RuntimeCommand::MigrationAdd(request) => migration::add(ctx, request, observer),
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
        RuntimeCommand::State(command) => dispatch_state(ctx, command, observer),
    }
}

fn dispatch_state(
    ctx: &RepoContext,
    command: StateCommand,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    match command {
        StateCommand::Summary => {
            crate::state::state_summary_with_cancellation(ctx, &|| observer.cancelled()).map(
                |mut value| {
                    value["command"] = json!("state summary");
                    value
                },
            )
        }
        StateCommand::Diagnose(request) => Ok(crate::state::state_diagnose(ctx, request)),
        StateCommand::CompactSessions(request) => crate::state::compact_sessions(ctx, request),
        StateCommand::Restore(request) => crate::state::restore_backup(ctx, request),
        StateCommand::ExportReceipts(request) => {
            crate::state::receipts_export(ctx, &request.before, &request.output)
        }
        StateCommand::Archive(request) => crate::state::state_archive(ctx, request),
    }
}

/// Read-only gate status for `jig ui`; reuses the `work gates` evaluation.
pub(crate) fn work_gates_snapshot(ctx: &RepoContext, plan_id: Option<String>) -> Result<Value> {
    let current = refreshed_repository_context(ctx)?;
    work::gates_snapshot(&current, plan_id)
}

pub(crate) fn open_plan_gate_snapshots_with_cancellation(
    ctx: &RepoContext,
    plan_ids: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Result<std::collections::BTreeMap<String, Value>> {
    crate::cancellation::ensure_status_collection_active(cancelled)?;
    if plan_ids.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    let current = refreshed_repository_context(ctx)?;
    crate::cancellation::ensure_status_collection_active(cancelled)?;
    work::open_plan_gate_snapshots_with_cancellation(&current, plan_ids, cancelled)
}

pub(crate) fn refreshed_repository_context(ctx: &RepoContext) -> Result<RepoContext> {
    let current = RepoContext::load_from_root(ctx.root().to_path_buf())
        .context("Failed to refresh repository authority")?;
    if current.contract_version() != ctx.contract_version() {
        bail!(
            "repository contract changed from version {} to {}; restart the process",
            ctx.contract_version(),
            current.contract_version()
        );
    }
    Ok(current)
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
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    match command {
        CheckCommand::Repository(request) => dispatch_repository_check(ctx, request, observer),
        CheckCommand::Fmt(opts) => {
            dispatch_named_check(ctx, "fmt", tool::FMT_CHECK, opts, observer)
        }
        CheckCommand::Lint(opts) => dispatch_named_check(ctx, "lint", tool::LINT, opts, observer),
        CheckCommand::Clippy(opts) => {
            dispatch_named_check(ctx, "clippy", tool::CLIPPY, opts, observer)
        }
        CheckCommand::Test(opts) => dispatch_named_check(ctx, "test", tool::TEST, opts, observer),
        CheckCommand::TestLocked(opts) => {
            dispatch_named_check(ctx, "test-locked", tool::TEST_LOCKED, opts, observer)
        }
        CheckCommand::TypeScriptLint(opts) => dispatch_named_check(
            ctx,
            "typescript-lint",
            tool::TYPESCRIPT_LINT,
            opts,
            observer,
        ),
        CheckCommand::TypeScriptTypecheck(opts) => dispatch_named_check(
            ctx,
            "typescript-typecheck",
            tool::TYPESCRIPT_TYPECHECK,
            opts,
            observer,
        ),
        CheckCommand::TypeScriptBuild(opts) => dispatch_named_check(
            ctx,
            "typescript-build",
            tool::TYPESCRIPT_BUILD,
            opts,
            observer,
        ),
        CheckCommand::TypeScriptCoverage(opts) => dispatch_named_check(
            ctx,
            "typescript-coverage",
            tool::TYPESCRIPT_COVERAGE,
            opts,
            observer,
        ),
        CheckCommand::Sqlx(opts) => {
            dispatch_named_check(ctx, "sqlx", tool::SQLX_CHECK, opts, observer)
        }
        CheckCommand::Sqlc(opts) => {
            dispatch_named_check(ctx, "sqlc", tool::SQLC_CHECK, opts, observer)
        }
        CheckCommand::Schema(opts) => {
            dispatch_named_check(ctx, "schema", tool::SCHEMA_CHECK, opts, observer)
        }
        CheckCommand::Contract(opts) => {
            dispatch_named_check(ctx, "contract", tool::CONTRACT_CHECK, opts, observer)
        }
        CheckCommand::AgentMap(opts) => crate::policy::run_check(
            ctx,
            PolicyCheckCommand::AgentMap(AgentMapInput {
                map_path: opts.map_path,
            }),
        ),
        CheckCommand::AgentGuides => crate::policy::run_check(ctx, PolicyCheckCommand::AgentGuides),
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

fn dispatch_named_check(
    ctx: &RepoContext,
    selector: &str,
    legacy_tool: &str,
    tool: crate::command::ToolRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    if ctx.contract_version() >= 6 {
        let catalog = crate::repository::RepositoryCatalog::from_context(ctx)?;
        dispatch_repository_check_with_catalog(
            ctx,
            &catalog,
            crate::command::RepositoryCheckRequest {
                selectors: vec![selector.into()],
                profile: None,
                affected_base: None,
                comparison: None,
                explain: false,
                fail_fast: false,
                tool,
            },
            observer,
        )
    } else {
        tool_execution::execute_manifest_tool_request_with_observer(
            ctx,
            legacy_tool,
            json!({}),
            tool,
            observer,
        )
    }
}

fn dispatch_repository_check(
    ctx: &RepoContext,
    request: crate::command::RepositoryCheckRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let catalog = crate::repository::RepositoryCatalog::from_context(ctx)?;
    dispatch_repository_check_with_catalog(ctx, &catalog, request, observer)
}

fn dispatch_repository_check_with_catalog(
    ctx: &RepoContext,
    catalog: &crate::repository::RepositoryCatalog,
    request: crate::command::RepositoryCheckRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    preserve_named_check_availability_diagnostic(ctx, catalog, &request.selectors)?;
    if request.comparison.is_some()
        && catalog.contract_version() < crate::repository::FILE_BUDGET_CONTRACT_VERSION
    {
        anyhow::bail!(
            "explicit check comparison authority requires repository contract version 7 or later"
        );
    }
    let plan = crate::repository::plan_run(
        ctx,
        catalog,
        crate::repository::PlanRunRequest {
            selectors: request.selectors,
            profile: request.profile,
            affected_base: request.affected_base,
            comparison: request.comparison,
            work_plan_id: None,
        },
    )?;
    if request.explain {
        return Ok(json!({
            "ok": true,
            "command": "check plan",
            "executed": false,
            "plan": plan,
        }));
    }

    execute_repository_check_plan(
        ctx,
        catalog,
        plan,
        request.tool,
        request.fail_fast,
        observer,
    )
}

fn preserve_named_check_availability_diagnostic(
    ctx: &RepoContext,
    catalog: &crate::repository::RepositoryCatalog,
    selectors: &[String],
) -> Result<()> {
    let [selector] = selectors else {
        return Ok(());
    };
    if catalog
        .actions()
        .any(|action| action.target.action.as_str() == selector)
    {
        return Ok(());
    }
    let legacy_tool = match selector.as_str() {
        "fmt" => tool::FMT_CHECK,
        "lint" => tool::LINT,
        "clippy" => tool::CLIPPY,
        "test" => tool::TEST,
        "test-locked" => tool::TEST_LOCKED,
        "typescript-lint" => tool::TYPESCRIPT_LINT,
        "typescript-typecheck" => tool::TYPESCRIPT_TYPECHECK,
        "typescript-build" => tool::TYPESCRIPT_BUILD,
        "typescript-coverage" => tool::TYPESCRIPT_COVERAGE,
        "sqlx" => tool::SQLX_CHECK,
        "sqlc" => tool::SQLC_CHECK,
        "schema" => tool::SCHEMA_CHECK,
        "contract" => tool::CONTRACT_CHECK,
        _ => return Ok(()),
    };
    if let Some(message) = jig_features::unavailable_tool_message(ctx, legacy_tool) {
        bail!(message);
    }
    Ok(())
}

fn execute_repository_check_plan(
    ctx: &RepoContext,
    catalog: &crate::repository::RepositoryCatalog,
    plan: jig_contract::RunPlan,
    tool: crate::command::ToolRequest,
    fail_fast: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let (work_plan_id, record_receipts) = tool.into_parts();
    let execution = run_execution::execute_freshly_planned_check_run(
        ctx,
        catalog,
        plan.clone(),
        run_execution::ExecuteCheckRunRequest {
            work_plan_id,
            record_receipts,
            fail_fast,
        },
        observer,
    )?;
    let ok = execution.run.result.conclusion == Some(jig_contract::RunConclusion::Success);

    Ok(json!({
        "ok": ok,
        "command": "check",
        "executed": true,
        "plan": plan,
        "run": execution.run.result,
        "results": execution.results,
        "failed_targets": execution.failed_targets,
        "source_observations": execution.source_observations,
    }))
}

#[cfg(test)]
pub(crate) fn call_tool(ctx: &RepoContext, name: &str, args: Value) -> Result<Value> {
    call_tool_with_observer(ctx, name, args, &mut NoopExecutionObserver)
}

pub(crate) fn call_tool_with_observer(
    ctx: &RepoContext,
    name: &str,
    args: Value,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let args_obj = args.as_object().cloned().unwrap_or_default();
    let memory_tool = MemoryTool::from_name(name);

    if observer.cancelled() {
        bail!("Execution was cancelled");
    }
    if ctx.contract_version() >= 6 {
        if let Some(tool) = tool_defs::RepositoryTool::from_name(name) {
            return mcp_repository::call(ctx, tool, args);
        }
    } else if memory_tool.is_none() {
        let current = refreshed_repository_context(ctx)?;
        match current.tool_spec(name) {
            Some(tool) if tool_defs::is_execution_tool(tool) => {
                return tool_execution::call_manifest_tool_with_observer(
                    &current, tool, &args_obj, observer,
                );
            }
            _ => {}
        }
    }

    let current_ctx = memory_tool
        .filter(|tool| tool.uses_repository_authority())
        .map(|_| refreshed_repository_context(ctx))
        .transpose()?;
    let memory_ctx = current_ctx.as_ref().unwrap_or(ctx);

    // MCP dispatch is intentionally allowlisted here. CLI-only dev/proxy
    // commands can start processes, install services, or mutate trust stores
    // and must not become agent-callable by adding names to tool_defs.
    match memory_tool {
        Some(MemoryTool::AgentDoctor) => Ok(agent::doctor(memory_ctx)),
        Some(MemoryTool::Goal) => work::goal_from_args(memory_ctx, args),
        Some(MemoryTool::Start) => work::start_from_args(memory_ctx, args),
        Some(MemoryTool::Append) => work::append_from_args(ctx, args),
        Some(MemoryTool::Check) => work::check_from_args_with_observer(memory_ctx, args, observer),
        Some(MemoryTool::Gates) => work::gates_from_args(memory_ctx, args),
        Some(MemoryTool::Evidence) => work::evidence_from_args(memory_ctx, args),
        Some(MemoryTool::Review) => {
            work::review_from_args_with_observer(memory_ctx, args, observer)
        }
        Some(MemoryTool::Refine) => {
            work::refine_from_args_with_observer(memory_ctx, args, observer)
        }
        Some(MemoryTool::Decide) => work::decide_from_args(ctx, args),
        Some(MemoryTool::Receipts) => work::receipts_from_args(ctx, args),
        Some(MemoryTool::Status) => crate::state::state_summary(memory_ctx),
        Some(MemoryTool::Finish) => work::finish_from_args(memory_ctx, args),
        None => bail!("Unsupported tool: {name}"),
    }
}

#[cfg(test)]
mod tests;
