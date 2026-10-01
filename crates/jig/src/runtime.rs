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
use crate::tool_defs::tool;

mod agent;
mod file_budget;
mod loops;
mod migration;
mod repository_run;
mod run_cancellation;
mod run_execution;
mod sqlx;
mod tool_execution;
mod vault;
mod vault_env;
mod vault_import;

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
        RuntimeCommand::Bootstrap => tool_execution::execute_manifest_tool_with_observer(
            ctx,
            tool::BOOTSTRAP,
            json!({}),
            observer,
        ),
        RuntimeCommand::Check(command) => dispatch_check_with_observer(ctx, command, observer),
        RuntimeCommand::Run(request) => repository_run::dispatch(ctx, request, observer),
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
        StateCommand::Diagnose => Ok(crate::state::state_diagnose(ctx)),
        StateCommand::Restore(request) => crate::state::restore_backup(ctx, request),
        StateCommand::Archive(request) => crate::state::state_archive(ctx, request),
    }
}

pub(crate) fn refreshed_repository_context(ctx: &RepoContext) -> Result<RepoContext> {
    let current = ctx
        .reload_execution_authority()
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

pub(crate) fn typed_loop_status_snapshot_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<jig_ui::dashboard::StatusLoopObservation> {
    loops::typed_status_with_cancellation(
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
        CheckCommand::Fmt => dispatch_named_check(ctx, "fmt", tool::FMT_CHECK, observer),
        CheckCommand::Lint => dispatch_named_check(ctx, "lint", tool::LINT, observer),
        CheckCommand::Clippy => dispatch_named_check(ctx, "clippy", tool::CLIPPY, observer),
        CheckCommand::Test => dispatch_named_check(ctx, "test", tool::TEST, observer),
        CheckCommand::TestLocked => {
            dispatch_named_check(ctx, "test-locked", tool::TEST_LOCKED, observer)
        }
        CheckCommand::TypeScriptLint => {
            dispatch_named_check(ctx, "typescript-lint", tool::TYPESCRIPT_LINT, observer)
        }
        CheckCommand::TypeScriptTypecheck => dispatch_named_check(
            ctx,
            "typescript-typecheck",
            tool::TYPESCRIPT_TYPECHECK,
            observer,
        ),
        CheckCommand::TypeScriptBuild => {
            dispatch_named_check(ctx, "typescript-build", tool::TYPESCRIPT_BUILD, observer)
        }
        CheckCommand::TypeScriptCoverage => dispatch_named_check(
            ctx,
            "typescript-coverage",
            tool::TYPESCRIPT_COVERAGE,
            observer,
        ),
        CheckCommand::Sqlx => dispatch_named_check(ctx, "sqlx", tool::SQLX_CHECK, observer),
        CheckCommand::Sqlc => dispatch_named_check(ctx, "sqlc", tool::SQLC_CHECK, observer),
        CheckCommand::Schema => dispatch_named_check(ctx, "schema", tool::SCHEMA_CHECK, observer),
        CheckCommand::Contract => {
            dispatch_named_check(ctx, "contract", tool::CONTRACT_CHECK, observer)
        }
        CheckCommand::AgentMap(opts) => crate::policy::run_check(
            ctx,
            PolicyCheckCommand::AgentMap(AgentMapInput {
                map_path: opts.map_path,
            }),
        ),
        CheckCommand::AgentGuides => crate::policy::run_check(ctx, PolicyCheckCommand::AgentGuides),
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
            },
            observer,
        )
    } else {
        tool_execution::execute_manifest_tool_with_observer(ctx, legacy_tool, json!({}), observer)
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
    let plan = crate::repository::plan_run_with_cancellation(
        ctx,
        catalog,
        crate::repository::PlanRunRequest {
            selectors: request.selectors,
            profile: request.profile,
            affected_base: request.affected_base,
            comparison: request.comparison,
        },
        &|| observer.cancelled(),
    )?;
    if request.explain {
        return Ok(json!({
            "ok": true,
            "command": "check plan",
            "executed": false,
            "plan": plan,
        }));
    }

    execute_repository_check_plan(ctx, catalog, plan, request.fail_fast, observer)
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
    fail_fast: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let execution = run_execution::execute_freshly_planned_check_run(
        ctx,
        catalog,
        plan.clone(),
        run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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
mod tests;
