use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[cfg(test)]
use crate::tool_defs::tool;
use crate::{bootstrap, context::RepoContext, doctor, info, mcp, runtime, status, tool_defs, ui};

mod agent;
mod bootstrap_run;
mod check;
mod init_wizard;
mod loops;
mod prompt;
mod proxy;
mod setup_run;
mod state;
mod status_opts;
mod vault;
mod work;

pub(crate) use agent::{AgentBootstrapOpts, AgentCommand};
pub(crate) use check::{CheckCommand, CheckMigrationImmutabilityOpts, CheckRustFileLocOpts};
pub(crate) use loops::{
    LoopClearAttemptOpts, LoopCommand, LoopRunOpts, LoopStatusOpts, LoopTickOpts,
};
pub(crate) use prompt::PromptCommand;
pub(crate) use proxy::{
    DevLaunchOpts, DevOpts, DevStatusOpts, DevStopOpts, DevSubcommand, ProxyAliasOpts,
    ProxyCertCommand, ProxyCertGenerateOpts, ProxyCertRuntimeOpts, ProxyCertTrustOpts,
    ProxyCertUntrustOpts, ProxyCommand, ProxyListOpts, ProxyPruneOpts, ProxyRunOpts,
    ProxyRuntimeOpts, ProxyServiceCommand, ProxyServiceInstallOpts, ProxyServiceRuntimeOpts,
    ProxyStartOpts, ProxyStopOpts,
};
pub(crate) use state::{
    StateArchiveOpts, StateCommand, StateCompactCommand, StateCompactSessionsOpts,
    StateDiagnoseOpts, StateExportCommand, StateExportReceiptsOpts, StateRestoreOpts,
};
pub(crate) use status_opts::StatusOpts;
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
    about = "Repo-local agent runtime and bootstrapper for jig.sh",
    after_help = ROOT_AFTER_HELP
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Print structured JSON results and errors; does not disable interactive prompts"
    )]
    json: bool,
    #[command(subcommand)]
    command: CommandKind,
}

const ROOT_AFTER_HELP: &str = "\
Common workflows:
  jig doctor       Check repository setup and get the next remediation step
  jig dev          Start configured development apps
  jig check test   Run the configured test suite
  jig work status  Inspect structured work and required gates";

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
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
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

const STATUS_AFTER_HELP: &str = "\
Runs configured jig.status-provider/v1 inspectors and combines their validated
reports with local Git freshness, structured work and gate state, and loop
leases and attempts. The command is read-only, does not fetch remotes, and
records no receipt.

Provider failures are included as partial status so an operator can inspect the
remaining snapshot. Human-readable output is the default. Pass --json for the
versioned aggregate or --tui for the interactive dashboard.

Examples:
  jig status
  jig status --json
  jig status --tui";

const PRESETS_AFTER_HELP: &str = "\
Use presets with `jig init` when you want Jig to create starter application code
and the repo harness together.

Examples:
  jig presets
  jig init ./my-repo --preset harness-only --no-input --no-vault
  jig init ./my-app --preset rust-react
  jig init ./my-app --preset rust-react --db postgres --frontends web,landing,admin";

const UI_AFTER_HELP: &str = "\
Serves a read-only loopback dashboard over .agent/state: open plans with gate
status, loop workflows, and a merged timeline of sessions, plans, receipts, and
decisions. The page auto-refreshes; the printed namespaced snapshot path returns the same data as JSON
after the browser establishes a session from the printed one-time URL.

The server binds 127.0.0.1 only, validates its exact Host and Origin, and records
no receipts. Proxy aliases are intentionally rejected to prevent DNS rebinding.

Examples:
  jig ui
  jig ui --port 0        # pick any free port
  jig ui --json          # print the URL as JSON, then serve";

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
    #[command(name = tool_defs::cli_command::INIT, display_order = 10)]
    Init(bootstrap::InitOpts),
    /// Show available project scaffolds for `jig init`.
    #[command(
        name = tool_defs::cli_command::PRESETS,
        display_order = 20,
        after_help = PRESETS_AFTER_HELP
    )]
    Presets,
    /// Adopt Jig harness files into an existing repository.
    #[command(name = tool_defs::cli_command::ADOPT, display_order = 30)]
    Adopt(bootstrap::AdoptOpts),
    /// Refresh managed Jig harness files from the configured template source.
    #[command(name = tool_defs::cli_command::UPDATE, display_order = 40)]
    Update(bootstrap::UpdateOpts),
    /// Run the configured project bootstrap command.
    #[command(name = tool_defs::cli_command::BOOTSTRAP, display_order = 50)]
    Bootstrap(ToolOpts),
    /// Prepare a generated repo for first use and verify its minimum contract.
    #[command(name = tool_defs::cli_command::SETUP, display_order = 55)]
    Setup,
    /// Report repo harness readiness and the next command to fix setup.
    #[command(
        name = tool_defs::cli_command::DOCTOR,
        display_order = 60,
        after_help = DOCTOR_AFTER_HELP
    )]
    Doctor,
    /// Summarize repo Jig configuration, capabilities, gates, and dev apps.
    #[command(
        name = tool_defs::cli_command::INFO,
        display_order = 70,
        visible_alias = "explain",
        after_help = INFO_AFTER_HELP
    )]
    Info,
    /// Run and manage configured development app sessions.
    #[command(name = tool_defs::cli_command::DEV, display_order = 100)]
    Dev(DevOpts),
    /// Run configured project checks and Jig-owned repository policy checks.
    #[command(
        name = tool_defs::cli_command::CHECK,
        display_order = 110,
        subcommand,
        after_help = check::CHECK_AFTER_HELP
    )]
    Check(CheckCommand),
    /// Aggregate local repo, work, loop, and configured status-provider observations.
    #[command(
        name = tool_defs::cli_command::STATUS,
        display_order = 120,
        after_help = STATUS_AFTER_HELP
    )]
    Status(StatusOpts),
    /// Serve the local flight-recorder dashboard for plans, gates, receipts, and loops.
    #[command(
        name = tool_defs::cli_command::UI,
        display_order = 130,
        after_help = UI_AFTER_HELP
    )]
    Ui(UiOpts),
    /// Manage structured work plans, receipts, gates, and decisions.
    #[command(
        name = tool_defs::cli_command::WORK,
        display_order = 200,
        subcommand
    )]
    Work(WorkCommand),
    /// Run and inspect automated orchestration workflows.
    #[command(
        name = tool_defs::cli_command::LOOP,
        display_order = 210,
        subcommand,
        after_help = loops::LOOP_AFTER_HELP
    )]
    Loop(LoopCommand),
    /// Add a forward-only SQLx migration file when SQLx is enabled.
    #[command(
        name = tool_defs::cli_command::MIGRATION_ADD,
        display_order = 300
    )]
    MigrationAdd(MigrationAddOpts),
    /// Regenerate schema documentation when schema dumps are enabled.
    #[command(
        name = tool_defs::cli_command::SCHEMA_DUMP,
        display_order = 310
    )]
    SchemaDump(ToolOpts),
    /// Manage the local encrypted Jig vault.
    #[command(
        name = tool_defs::cli_command::VAULT,
        display_order = 320,
        subcommand,
        after_help = VAULT_AFTER_HELP
    )]
    Vault(VaultCommand),
    /// Generate a TODO report for unchecked SQLx queries.
    #[command(
        name = tool_defs::cli_command::GENERATE_SQLX_UNCHECKED_QUERIES_TODO,
        hide = true
    )]
    GenerateSqlxUncheckedQueriesTodo(GenerateSqlxUncheckedQueriesTodoOpts),
    /// Manage the local development proxy.
    #[command(
        name = tool_defs::cli_command::PROXY,
        display_order = 400,
        subcommand
    )]
    Proxy(ProxyCommand),
    /// Manage user, repo, and prompt-pack prompt libraries.
    #[command(name = "prompt", display_order = 500, subcommand)]
    Prompt(PromptCommand),
    /// Inspect or bootstrap local agent tooling.
    #[command(
        name = tool_defs::cli_command::AGENT,
        display_order = 510,
        subcommand,
        after_help = agent::AGENT_AFTER_HELP
    )]
    Agent(AgentCommand),
    /// Generate the repository agent guide map.
    #[command(
        name = tool_defs::cli_command::AGENT_MAP,
        display_order = 520,
        subcommand
    )]
    AgentMap(AgentMapCommand),
    /// Inspect and archive runtime-owned Jig state.
    #[command(
        name = tool_defs::cli_command::STATE,
        display_order = 530,
        subcommand
    )]
    State(StateCommand),
    /// Serve the Jig MCP server over stdio.
    #[command(name = tool_defs::cli_command::MCP, display_order = 540)]
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

#[derive(Args, Debug)]
pub(crate) struct UiOpts {
    #[arg(
        long,
        default_value_t = ui::DEFAULT_UI_PORT,
        help = "Loopback port to serve on; 0 selects any free port"
    )]
    pub(crate) port: u16,
}

mod command_conversion;

mod output;
mod prompt_run;
mod run;
mod structured_error;
mod vault_run;

pub(crate) use run::{is_structured_json_failure, run, structured_error_exit_code};
pub(crate) use structured_error::json_output_already_emitted;

#[cfg(test)]
mod dev_tests;
#[cfg(test)]
mod help_tests;
#[cfg(test)]
mod preset_tests;
#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod tests;
