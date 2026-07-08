use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::command::{AgentMapCommand, CheckCommand, RuntimeCommand, StateCommand};
use crate::context::RepoContext;
use crate::policy::{
    AgentMapInput, MigrationImmutabilityInput, PolicyCheckCommand, PolicyDirectCommand,
    RustFileLocInput, SqlxTodoInput,
};
use crate::tool_defs::{self, MemoryTool, args, tool};

mod agent;
mod loops;
mod prompt;
mod tool_execution;
mod vault;
mod work;
mod worker_runner;

pub(crate) fn dispatch(ctx: &RepoContext, command: RuntimeCommand) -> Result<Value> {
    match command {
        RuntimeCommand::Bootstrap(opts) => {
            tool_execution::execute_manifest_tool_request(ctx, tool::BOOTSTRAP, json!({}), opts)
        }
        RuntimeCommand::Check(command) => dispatch_check(ctx, command),
        RuntimeCommand::SchemaDump(opts) => {
            tool_execution::execute_manifest_tool_request(ctx, tool::SCHEMA_DUMP, json!({}), opts)
        }
        RuntimeCommand::MigrationAdd(opts) => {
            let (plan_id, record_receipt) = opts.tool.into_parts();
            tool_execution::execute_manifest_tool(
                ctx,
                tool::MIGRATION_ADD,
                json!({ args::NAME: opts.name }),
                plan_id,
                record_receipt,
            )
            .map(|value| {
                let name = value["args"][args::NAME].clone();
                json!({
                    "ok": true,
                    "tool": tool::MIGRATION_ADD,
                    args::NAME: name,
                    "result": value["result"],
                    "receipt_id": value["receipt_id"],
                })
            })
        }
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
        RuntimeCommand::Agent(command) => agent::dispatch(ctx, command),
        RuntimeCommand::Work(command) => work::dispatch(ctx, command),
        RuntimeCommand::Loop(command) => loops::dispatch(ctx, command),
        RuntimeCommand::State(command) => dispatch_state(ctx, command),
    }
}

fn dispatch_state(ctx: &RepoContext, command: StateCommand) -> Result<Value> {
    match command {
        StateCommand::Summary => crate::state::state_summary(ctx),
        StateCommand::Archive(request) => crate::state::receipts_archive(
            ctx,
            crate::state::StateArchiveRequest {
                before: request.before,
                dry_run: request.dry_run,
            },
        ),
    }
}

pub(crate) fn dispatch_vault(command: crate::command::VaultCommand) -> Result<Value> {
    vault::dispatch(command)
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

fn dispatch_check(ctx: &RepoContext, command: CheckCommand) -> Result<Value> {
    match command {
        CheckCommand::Fmt(opts) => {
            tool_execution::execute_manifest_tool_request(ctx, tool::FMT_CHECK, json!({}), opts)
        }
        CheckCommand::Clippy(opts) => {
            tool_execution::execute_manifest_tool_request(ctx, tool::CLIPPY, json!({}), opts)
        }
        CheckCommand::Test(opts) => {
            tool_execution::execute_manifest_tool_request(ctx, tool::TEST, json!({}), opts)
        }
        CheckCommand::TestLocked(opts) => {
            tool_execution::execute_manifest_tool_request(ctx, tool::TEST_LOCKED, json!({}), opts)
        }
        CheckCommand::TypeScriptLint(opts) => tool_execution::execute_manifest_tool_request(
            ctx,
            tool::TYPESCRIPT_LINT,
            json!({}),
            opts,
        ),
        CheckCommand::TypeScriptTypecheck(opts) => tool_execution::execute_manifest_tool_request(
            ctx,
            tool::TYPESCRIPT_TYPECHECK,
            json!({}),
            opts,
        ),
        CheckCommand::TypeScriptBuild(opts) => tool_execution::execute_manifest_tool_request(
            ctx,
            tool::TYPESCRIPT_BUILD,
            json!({}),
            opts,
        ),
        CheckCommand::TypeScriptCoverage(opts) => tool_execution::execute_manifest_tool_request(
            ctx,
            tool::TYPESCRIPT_COVERAGE,
            json!({}),
            opts,
        ),
        CheckCommand::Sqlx(opts) => {
            tool_execution::execute_manifest_tool_request(ctx, tool::SQLX_CHECK, json!({}), opts)
        }
        CheckCommand::Schema(opts) => {
            tool_execution::execute_manifest_tool_request(ctx, tool::SCHEMA_CHECK, json!({}), opts)
        }
        CheckCommand::Contract(opts) => tool_execution::execute_manifest_tool_request(
            ctx,
            tool::CONTRACT_CHECK,
            json!({}),
            opts,
        ),
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

pub(crate) fn call_tool(ctx: &RepoContext, name: &str, args: Value) -> Result<Value> {
    let args_obj = args.as_object().cloned().unwrap_or_default();

    match ctx.tool_spec(name) {
        Some(tool) if tool_defs::is_execution_tool(tool) => {
            return tool_execution::call_manifest_tool(ctx, tool, &args_obj);
        }
        _ => {}
    }

    // MCP dispatch is intentionally allowlisted here. CLI-only dev/proxy
    // commands can start processes, install services, or mutate trust stores
    // and must not become agent-callable by adding names to tool_defs.
    match MemoryTool::from_name(name) {
        Some(MemoryTool::AgentDoctor) => agent::doctor(ctx),
        Some(MemoryTool::Goal) => work::goal_from_args(ctx, args),
        Some(MemoryTool::Start) => work::start_from_args(ctx, args),
        Some(MemoryTool::Append) => work::append_from_args(ctx, args),
        Some(MemoryTool::Check) => work::check_from_args(ctx, args),
        Some(MemoryTool::Gates) => work::gates_from_args(ctx, args),
        Some(MemoryTool::Evidence) => work::evidence_from_args(ctx, args),
        Some(MemoryTool::Review) => work::review_from_args(ctx, args),
        Some(MemoryTool::Refine) => work::refine_from_args(ctx, args),
        Some(MemoryTool::Decide) => work::decide_from_args(ctx, args),
        Some(MemoryTool::Receipts) => work::receipts_from_args(ctx, args),
        Some(MemoryTool::Status) => crate::state::state_summary(ctx),
        Some(MemoryTool::Finish) => work::finish_from_args(ctx, args),
        None => bail!("Unsupported tool: {name}"),
    }
}

#[cfg(test)]
mod tests;
