use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[cfg(test)]
use crate::tool_defs::tool;
use crate::{bootstrap, context::RepoContext, doctor, info, mcp, runtime, tool_defs};

mod agent;
mod bootstrap_run;
mod check;
mod loops;
mod prompt;
mod proxy;
mod state;
mod vault;
mod work;

pub(crate) use agent::{AgentBootstrapOpts, AgentCommand};
pub(crate) use check::{CheckCommand, CheckMigrationImmutabilityOpts, CheckRustFileLocOpts};
pub(crate) use loops::{
    LoopClearAttemptOpts, LoopCommand, LoopRunOpts, LoopStatusOpts, LoopTickOpts,
};
pub(crate) use prompt::PromptCommand;
pub(crate) use proxy::{
    DevOpts, ProxyAliasOpts, ProxyCertCommand, ProxyCertGenerateOpts, ProxyCertRuntimeOpts,
    ProxyCertTrustOpts, ProxyCertUntrustOpts, ProxyCommand, ProxyListOpts, ProxyPruneOpts,
    ProxyRunOpts, ProxyRuntimeOpts, ProxyServiceCommand, ProxyServiceInstallOpts,
    ProxyServiceRuntimeOpts, ProxyStartOpts, ProxyStopOpts,
};
pub(crate) use state::{StateArchiveOpts, StateCommand};
pub(crate) use vault::{
    VaultAuditCommand, VaultAuditVerifyOpts, VaultCommand, VaultInitOpts, VaultRunOpts,
    VaultRuntimeOpts, VaultSecretCommand, VaultSecretListOpts, VaultSecretRemoveOpts,
    VaultSecretSetOpts, VaultStatusOpts,
};
pub(crate) use work::{
    WorkAppendOpts, WorkCheckOpts, WorkCommand, WorkDecisionAddOpts, WorkEvidenceOpts,
    WorkFinishOpts, WorkGatesOpts, WorkGoalOpts, WorkReceiptsOpts, WorkRefineOpts, WorkReviewOpts,
    WorkStartOpts,
};

#[derive(Debug, Parser)]
#[command(
    name = "jig",
    version,
    about = "Repo-local agent runtime and bootstrapper for jig.sh"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Print structured JSON output when a command defaults to human-readable output"
    )]
    json: bool,
    #[command(subcommand)]
    command: CommandKind,
}

const TEMPLATE_ERROR_HINT: &str = "\
Templates:
  Omit --template to use the default jig-sh harness template.
  Release builds use the official template:
  https://github.com/bpcakes/jig-sh.git
  Unreleased local builds use templates embedded in the jig binary.

If you passed --template without a value, either omit it to use the default
or provide a path/URL.

Use one of:
  jig adopt .
  jig adopt . --write
  jig init /path/to/new-repo --repo-name new-repo --sqlx-enabled false
  jig adopt . --write --template /path/to/jig-sh

Pass --template only for a local checkout, fork, or private template.";

const MIGRATION_ADD_AFTER_HELP: &str = "\
Use --plan-id to associate the migration with an open structured work plan.

Examples:
  jig migration-add create_users
  jig migration-add add_login_tokens --plan-id plan_abc123";

const DOCTOR_AFTER_HELP: &str = "\
Runs the read-only readiness checks that are otherwise split across bootstrap,
agent doctor, check contract, proxy status, and vault status.

Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig doctor
  jig doctor --json";

const INFO_AFTER_HELP: &str = "\
Summarizes what Jig believes about the current repo from .jig.toml and the
generated contract manifest.

Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig info
  jig info --json
  jig explain --json";

const PRESETS_AFTER_HELP: &str = "\
Use presets with `jig init` when you want Jig to create starter application code
and the repo harness together.

Examples:
  jig presets
  jig init ./my-app --preset rust-react
  jig init ./my-app --preset rust-react --db postgres --frontends web,landing,admin";

const VAULT_AFTER_HELP: &str = "\
Jig Vault stores local secrets outside the repository. Terminal use prompts for
the vault passphrase; scripts can set JIG_VAULT_PASSPHRASE. Command-line
passphrases are not accepted.

Quick start:
  jig vault init
  jig vault secret set api_token --value-prompt
  jig vault run --env TOKEN=api_token -- sh -c 'printf \"%s\" \"$TOKEN\"'
  jig vault run --file TOKEN_FILE=api_token -- sh -c 'cat \"$TOKEN_FILE\"'";

#[derive(Debug, Subcommand)]
pub(crate) enum CommandKind {
    /// Create a new repository and render Jig harness files into it.
    #[command(name = tool_defs::cli_command::INIT)]
    Init(bootstrap::InitOpts),
    /// Show available project scaffolds for `jig init`.
    #[command(name = tool_defs::cli_command::PRESETS, after_help = PRESETS_AFTER_HELP)]
    Presets,
    /// Adopt Jig harness files into an existing repository.
    #[command(name = tool_defs::cli_command::ADOPT)]
    Adopt(bootstrap::AdoptOpts),
    /// Refresh managed Jig harness files from the configured template source.
    #[command(name = tool_defs::cli_command::UPDATE)]
    Update(bootstrap::UpdateOpts),
    /// Run the configured project bootstrap command.
    #[command(name = tool_defs::cli_command::BOOTSTRAP)]
    Bootstrap(ToolOpts),
    /// Run configured project checks and Jig-owned repository policy checks.
    #[command(
        name = tool_defs::cli_command::CHECK,
        subcommand,
        after_help = check::CHECK_AFTER_HELP
    )]
    Check(CheckCommand),
    /// Report repo harness readiness and the next command to fix setup.
    #[command(name = tool_defs::cli_command::DOCTOR, after_help = DOCTOR_AFTER_HELP)]
    Doctor,
    /// Summarize repo Jig configuration, capabilities, gates, and dev apps.
    #[command(
        name = tool_defs::cli_command::INFO,
        visible_alias = "explain",
        after_help = INFO_AFTER_HELP
    )]
    Info,
    /// Regenerate schema documentation when schema dumps are enabled.
    #[command(name = tool_defs::cli_command::SCHEMA_DUMP)]
    SchemaDump(ToolOpts),
    /// Add a forward-only SQLx migration file when SQLx is enabled.
    #[command(name = tool_defs::cli_command::MIGRATION_ADD)]
    MigrationAdd(MigrationAddOpts),
    /// Generate the repository agent guide map.
    #[command(name = tool_defs::cli_command::AGENT_MAP, subcommand)]
    AgentMap(AgentMapCommand),
    /// Generate a TODO report for unchecked SQLx queries.
    #[command(
        name = tool_defs::cli_command::GENERATE_SQLX_UNCHECKED_QUERIES_TODO,
        hide = true
    )]
    GenerateSqlxUncheckedQueriesTodo(GenerateSqlxUncheckedQueriesTodoOpts),
    /// Run configured development apps through the local dev proxy.
    #[command(name = tool_defs::cli_command::DEV)]
    Dev(DevOpts),
    /// Manage the local development proxy.
    #[command(name = tool_defs::cli_command::PROXY, subcommand)]
    Proxy(ProxyCommand),
    /// Manage the local encrypted Jig vault.
    #[command(
        name = tool_defs::cli_command::VAULT,
        subcommand,
        after_help = VAULT_AFTER_HELP
    )]
    Vault(VaultCommand),
    /// Manage user, repo, and prompt-pack prompt libraries.
    #[command(name = "prompt", subcommand)]
    Prompt(PromptCommand),
    /// Inspect or bootstrap local agent tooling.
    #[command(
        name = tool_defs::cli_command::AGENT,
        subcommand,
        after_help = agent::AGENT_AFTER_HELP
    )]
    Agent(AgentCommand),
    /// Manage structured work plans, receipts, gates, and decisions.
    #[command(name = tool_defs::cli_command::WORK, subcommand)]
    Work(WorkCommand),
    /// Run runtime-owned orchestration workflows as idempotent ticks.
    #[command(
        name = tool_defs::cli_command::LOOP,
        subcommand,
        after_help = loops::LOOP_AFTER_HELP
    )]
    Loop(LoopCommand),
    /// Inspect and archive runtime-owned Jig state.
    #[command(name = tool_defs::cli_command::STATE, subcommand)]
    State(StateCommand),
    /// Serve the Jig MCP server over stdio.
    #[command(name = tool_defs::cli_command::MCP)]
    Mcp,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentMapCommand {
    /// Rewrite agent-map.md from tracked AGENTS.md files.
    #[command(name = tool_defs::cli_command::AGENT_MAP_GENERATE)]
    Generate(AgentMapOpts),
}

#[derive(Args, Debug)]
pub(crate) struct AgentMapOpts {
    #[arg(
        long = "map",
        default_value = "agent-map.md",
        help = "Agent map file to generate or check"
    )]
    pub(crate) map_path: PathBuf,
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct ToolOpts {
    #[arg(long, help = "Structured work plan id to attach the receipt to")]
    pub(crate) plan_id: Option<String>,
    #[arg(
        long,
        conflicts_with = "plan_id",
        help = "Run without appending a receipt to .agent/state"
    )]
    pub(crate) no_receipt: bool,
}

#[derive(Args, Debug)]
#[command(after_help = MIGRATION_ADD_AFTER_HELP)]
pub(crate) struct MigrationAddOpts {
    /// Migration name, for example create_users.
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

#[derive(Args, Debug)]
pub(crate) struct GenerateSqlxUncheckedQueriesTodoOpts {
    /// Optional output path for the generated TODO report.
    pub(crate) output: Option<PathBuf>,
}

mod command_conversion;

mod output;
mod prompt_run;
mod run;
mod structured_error;
mod vault_run;

pub(crate) use run::{is_structured_json_failure, run, structured_error_exit_code};

#[cfg(test)]
mod help_tests;
#[cfg(test)]
mod preset_tests;
#[cfg(test)]
mod tests;
