use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[cfg(test)]
use crate::tool_defs::tool;
use crate::{
    bootstrap, context::RepoContext, doctor, info, mcp, root_commands, runtime, status, tool_defs,
    ui,
};

mod agent;
mod bootstrap_run;
mod check;
mod codex;
mod codex_run;
mod file_budget;
mod init_wizard;
mod loops;
mod migration;
mod prompt;
mod proxy;
mod setup_run;
mod sqlx;
mod state;
mod status_opts;
mod vault;
mod work;

pub(crate) use agent::{AgentBootstrapOpts, AgentCommand};
pub(crate) use check::{
    CHECK_SUBCOMMAND_NAMES, CheckCommand, CheckComparisonOpts, CheckExactTreeProvenance,
    CheckMigrationImmutabilityOpts, CheckOpts, CheckTargetOpts,
};
pub(crate) use codex::CodexCommand;
pub(crate) use file_budget::FileBudgetCommand;
pub(crate) use loops::{
    LoopAcknowledgeOccurrenceOpts, LoopClearAttemptOpts, LoopCommand, LoopDispatchOpts,
    LoopRunOpts, LoopStatusOpts, LoopTickOpts,
};
pub(crate) use migration::{MigrationAddOpts, MigrationCommand};
pub(crate) use prompt::PromptCommand;
pub(crate) use proxy::{
    DevLaunchOpts, DevOpts, DevStatusOpts, DevStopOpts, DevSubcommand, ProxyAliasOpts,
    ProxyCertCommand, ProxyCertGenerateOpts, ProxyCertRuntimeOpts, ProxyCertTrustOpts,
    ProxyCertUntrustOpts, ProxyCommand, ProxyListOpts, ProxyPruneOpts, ProxyRunOpts,
    ProxyRuntimeOpts, ProxyServiceCommand, ProxyServiceInstallOpts, ProxyServiceRuntimeOpts,
    ProxyStartOpts, ProxyStopOpts,
};
pub(crate) use sqlx::{SqlxCommand, SqlxMigrationCommand, SqlxSchemaCommand};
pub(crate) use state::{
    StateArchiveOpts, StateCommand, StateCompactCommand, StateCompactSessionsOpts,
    StateDiagnoseOpts, StateExportCommand, StateExportReceiptsOpts, StateRestoreOpts,
};
pub(crate) use status_opts::{StatusCommand, StatusOpts};
pub(crate) use vault::{
    VaultAuditCommand, VaultAuditVerifyOpts, VaultBackupCommand, VaultBackupCreateOpts,
    VaultBackupRestoreOpts, VaultCommand, VaultExecOpts, VaultFieldCommand, VaultFieldListOpts,
    VaultFieldRemoveOpts, VaultFieldSetOpts, VaultImportCommand, VaultImportOnePasswordOpts,
    VaultInitOpts, VaultInjectOpts, VaultMigrateOpts, VaultPassphraseChangeOpts,
    VaultPassphraseCommand, VaultReadOpts, VaultRunOpts, VaultRuntimeOpts, VaultSecretCommand,
    VaultSecretListOpts, VaultSecretRemoveOpts, VaultSecretSetOpts, VaultStatusOpts, VaultTuiOpts,
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
    after_help = root_after_help()
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Print structured JSON results and errors; does not disable interactive prompts"
    )]
    json: bool,
    #[arg(long = "__launcher-contract-version", hide = true)]
    launcher_contract_version: Option<u32>,
    #[arg(long = "__launcher-profile", value_enum, hide = true)]
    launcher_profile: Option<RuntimeCompatibilityProfile>,
    #[arg(long = "__launcher-repo-root", hide = true)]
    launcher_repo_root: Option<PathBuf>,
    #[command(subcommand)]
    command: CommandKind,
}

#[cfg(test)]
const LAUNCHER_GLOBAL_FLAGS: &str = "--json";
#[cfg(test)]
const LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS: &str = "adopt,codex,doctor,init,presets,update";
#[cfg(test)]
const LAUNCHER_REPOSITORY_SCOPE_SUBCOMMANDS: &str = "agent,agent-map,bootstrap,check,dev,file-budget,generate-sqlx-unchecked-queries-todo,info,loop,mcp,migration,migration-add,prompt,proxy,schema-dump,setup,sqlx,state,status,ui,vault,work";
#[cfg(test)]
const LAUNCHER_CHECK_SUBCOMMANDS: &str = "fmt,lint,clippy,test,test-locked,typescript-lint,typescript-typecheck,typescript-build,typescript-coverage,sqlx,sqlc,schema,contract,agent-map,agent-guides,migration-immutability,sqlx-unchecked-non-test";

const ROOT_COMMON_WORKFLOWS: &str = "\
Common workflows:
  jig doctor           Check repository setup and get the next remediation step
  jig info --commands  Show which commands are usable in this repository
  jig dev              Start configured development apps
  jig check test       Run the configured test suite
  jig work status      Inspect structured work and required gates";

fn root_after_help() -> String {
    format!(
        "{}\n{ROOT_COMMON_WORKFLOWS}",
        root_commands::categorized_help()
    )
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
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
  jig adopt . --write --template /path/to/jig-sh

Pass --template only for a local checkout, fork, or private template.";

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

Use --commands for repository-specific command availability. Status describes
each root command's primary workflow; setup and diagnostic subcommands or flags
may still work when that status is not ready. Invocation still runs
command-specific preflight.
Stable JSON status codes are ready, not_configured, needs_setup, and unavailable.
When Codex marketplaces are configured, this view checks machine-local Codex
readiness and may wait up to five seconds for that probe.

Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig info
  jig info --json
  jig info components
  jig info target api:test --json
  jig info --commands
  jig info --commands --json  # also works before adoption
  jig explain --json";

const STATUS_AFTER_HELP: &str = "\
Collects local Git, structured work and gate state, and loop leases and attempts.
The command is read-only, does not fetch remotes, and records no receipt.

Collection failures are included as partial status so an operator can inspect
the remaining snapshot. Human-readable output is the default. Pass --json for
the versioned aggregate or --tui for the interactive dashboard.

Examples:
  jig status
  jig status run RUN_ID
  jig status --json
  jig status --tui";

const PRESETS_AFTER_HELP: &str = "\
Use presets with `jig init` when you want Jig to create starter project code
and the repo harness together.

Examples:
  jig presets
  jig init ./my-repo --preset harness-only --no-input --no-vault
  jig init ./my-library --preset rust-library --no-input --no-vault
  jig init ./my-cli --preset rust-cli --no-input --no-vault
  jig init ./my-app --preset rust-react
  jig init ./my-app --preset rust-react --db postgres --frontends web,landing,admin";

const UI_AFTER_HELP: &str = "\
Opens a read-only terminal dashboard over repository status and .agent/state:
plans, gates, receipts, loops, repository state, and activity.
Interactive mode requires terminal stdin and stdout and records no receipts.

Pass --json for one local recorder snapshot.
Combine --plan with --json for one plan-detail snapshot.

Examples:
  jig ui
  jig ui --plan PLAN_ID
  jig ui --timeline-limit 120
  jig ui --json
  jig ui --plan PLAN_ID --json";

const VAULT_AFTER_HELP: &str = "\
Jig Vault stores encrypted project fields outside the repository. References
are project-relative: jig://Production/TOKEN selects the current repo-scoped,
global, or explicit-home vault; the project name is never a reference segment.
Both concealed and text fields are encrypted. Concealed fields are redaction
needles, while text fields remain visible when deliberately passed to a command.
Terminal use prompts for the vault passphrase; scripts can set
JIG_VAULT_PASSPHRASE. Command-line passphrases are not accepted.

Quick start:
  jig vault init
  jig vault tui
  jig vault migrate --to 2
  jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt
  printf '%s' 'local' | jig vault field set jig://Production/MODE --text --value-stdin
  jig vault read jig://Production/RESTIC_PASSWORD | command
  jig vault inject --in config.template > config
  jig vault exec --env-file .env.jig -- command
  jig vault import onepassword --env-file .env.op --item Production --out-env .env.jig
  jig vault passphrase change
  jig vault backup create --out ../ExampleProject-vault.backup
  jig vault backup restore --in ../ExampleProject-vault.backup

Compatibility commands (concealed fields and constrained execution):
  jig vault secret set api_token --value-prompt
  jig vault run --env TOKEN=api_token -- sh -c 'printf \"%s\" \"$TOKEN\"'
  jig vault run --file TOKEN_FILE=api_token -- sh -c 'cat \"$TOKEN_FILE\"'";

#[derive(Debug, Subcommand)]
pub(crate) enum CommandKind {
    /// Create a new repository and render Jig harness files into it.
    #[command(
        name = root_commands::INIT.name,
        display_order = root_commands::INIT.display_order
    )]
    Init(bootstrap::InitOpts),
    /// Show available project scaffolds for `jig init`.
    #[command(
        name = root_commands::PRESETS.name,
        display_order = root_commands::PRESETS.display_order,
        after_help = PRESETS_AFTER_HELP
    )]
    Presets,
    /// Adopt Jig harness files into an existing repository.
    #[command(
        name = root_commands::ADOPT.name,
        display_order = root_commands::ADOPT.display_order
    )]
    Adopt(bootstrap::AdoptOpts),
    /// Refresh managed Jig harness files from the configured template source.
    #[command(
        name = root_commands::UPDATE.name,
        display_order = root_commands::UPDATE.display_order
    )]
    Update(bootstrap::UpdateOpts),
    /// Run the configured project bootstrap command.
    #[command(
        name = root_commands::BOOTSTRAP.name,
        display_order = root_commands::BOOTSTRAP.display_order
    )]
    Bootstrap(ToolOpts),
    /// Prepare a generated repo for first use and verify its minimum contract.
    #[command(
        name = root_commands::SETUP.name,
        display_order = root_commands::SETUP.display_order
    )]
    Setup,
    /// Report repo harness readiness and the next command to fix setup.
    #[command(
        name = root_commands::DOCTOR.name,
        display_order = root_commands::DOCTOR.display_order,
        after_help = DOCTOR_AFTER_HELP
    )]
    Doctor,
    /// Summarize repo Jig configuration, capabilities, gates, and dev apps.
    #[command(
        name = root_commands::INFO.name,
        display_order = root_commands::INFO.display_order,
        visible_alias = "explain",
        after_help = INFO_AFTER_HELP
    )]
    Info(InfoOpts),
    /// Run and manage configured development app sessions.
    #[command(
        name = root_commands::DEV.name,
        display_order = root_commands::DEV.display_order
    )]
    Dev(DevOpts),
    /// Run configured project checks and Jig-owned repository policy checks.
    #[command(
        name = root_commands::CHECK.name,
        display_order = root_commands::CHECK.display_order,
        after_help = check::CHECK_AFTER_HELP
    )]
    Check(CheckOpts),
    /// Run built-in file-budget diagnostics without creating a run or receipt.
    #[command(
        name = root_commands::FILE_BUDGET.name,
        display_order = root_commands::FILE_BUDGET.display_order,
        subcommand
    )]
    FileBudget(FileBudgetCommand),
    /// Aggregate local repository, work, and loop observations.
    #[command(
        name = root_commands::STATUS.name,
        display_order = root_commands::STATUS.display_order,
        after_help = STATUS_AFTER_HELP
    )]
    Status(StatusOpts),
    /// Open the unified terminal dashboard for status and local recorder state.
    #[command(
        name = root_commands::UI.name,
        display_order = root_commands::UI.display_order,
        after_help = UI_AFTER_HELP
    )]
    Ui(UiOpts),
    /// Manage structured work plans, receipts, gates, and decisions.
    #[command(
        name = root_commands::WORK.name,
        display_order = root_commands::WORK.display_order,
        subcommand
    )]
    Work(WorkCommand),
    /// Run and inspect automated orchestration workflows.
    #[command(
        name = root_commands::LOOP.name,
        display_order = root_commands::LOOP.display_order,
        subcommand,
        after_help = loops::LOOP_AFTER_HELP
    )]
    Loop(LoopCommand),
    /// Create migrations in the configured backend format.
    #[command(
        name = root_commands::MIGRATION.name,
        display_order = root_commands::MIGRATION.display_order,
        subcommand,
        after_help = migration::MIGRATION_AFTER_HELP
    )]
    Migration(MigrationCommand),
    /// Manage SQLx migrations and schema documentation.
    #[command(
        name = root_commands::SQLX.name,
        display_order = root_commands::SQLX.display_order,
        subcommand,
        after_help = sqlx::SQLX_AFTER_HELP
    )]
    Sqlx(SqlxCommand),
    /// Add a forward-only migration through the legacy flattened command.
    #[command(name = tool_defs::cli_command::MIGRATION_ADD, hide = true)]
    MigrationAdd(MigrationAddOpts),
    /// Regenerate schema documentation when schema dumps are enabled.
    #[command(name = tool_defs::cli_command::SCHEMA_DUMP, hide = true)]
    SchemaDump(ToolOpts),
    /// Manage the local encrypted Jig vault.
    #[command(
        name = root_commands::VAULT.name,
        display_order = root_commands::VAULT.display_order,
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
        name = root_commands::PROXY.name,
        display_order = root_commands::PROXY.display_order,
        subcommand
    )]
    Proxy(ProxyCommand),
    /// Manage user, repo, and prompt-pack prompt libraries.
    #[command(
        name = root_commands::PROMPT.name,
        display_order = root_commands::PROMPT.display_order,
        subcommand
    )]
    Prompt(PromptCommand),
    /// Inspect or bootstrap local agent tooling.
    #[command(
        name = root_commands::AGENT.name,
        display_order = root_commands::AGENT.display_order,
        subcommand,
        after_help = agent::AGENT_AFTER_HELP
    )]
    Agent(AgentCommand),
    /// Inspect Codex homes, launch Codex, or resume a session from its owning home.
    #[command(
        name = root_commands::CODEX.name,
        display_order = root_commands::CODEX.display_order,
        subcommand,
        after_help = codex::CODEX_AFTER_HELP
    )]
    Codex(CodexCommand),
    /// Generate the repository agent guide map.
    #[command(
        name = root_commands::AGENT_MAP.name,
        display_order = root_commands::AGENT_MAP.display_order,
        subcommand
    )]
    AgentMap(AgentMapCommand),
    /// Inspect and archive runtime-owned Jig state.
    #[command(
        name = root_commands::STATE.name,
        display_order = root_commands::STATE.display_order,
        subcommand
    )]
    State(StateCommand),
    /// Serve the Jig MCP server over stdio.
    #[command(
        name = root_commands::MCP.name,
        display_order = root_commands::MCP.display_order
    )]
    Mcp,
    /// Validate this binary against a generated repository launcher contract.
    #[command(name = "__runtime-compatible", hide = true)]
    RuntimeCompatible(RuntimeCompatibleOpts),
}

#[derive(Args, Debug)]
pub(crate) struct RuntimeCompatibleOpts {
    #[arg(long, value_enum)]
    pub(crate) profile: RuntimeCompatibilityProfile,
    #[arg(long, hide = true)]
    pub(crate) capability_only: bool,
    #[arg(long, hide = true)]
    pub(crate) contract_version: Option<u32>,
    pub(crate) repo_root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum RuntimeCompatibilityProfile {
    Default,
    Runtime,
    Mcp,
}

impl RuntimeCompatibilityProfile {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Runtime => "runtime",
            Self::Mcp => "mcp",
        }
    }
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

#[derive(Args, Debug, Default)]
pub(crate) struct InfoOpts {
    #[arg(
        long,
        help = "Show root commands with repository-specific availability and remediation"
    )]
    pub(crate) commands: bool,
    #[command(subcommand)]
    pub(crate) subject: Option<InfoCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum InfoCommand {
    /// Print the highest Go module toolchain selector used by managed CI.
    #[command(name = "go-version", hide = true)]
    GoVersion,
    /// Inspect the normalized workspace catalog.
    Workspace,
    /// List addressable repository components.
    Components,
    /// Inspect one component and its targets.
    Component { id: String },
    /// List executable component/action targets.
    Targets,
    /// Inspect one target by its component:action address.
    Target { id: String },
    /// List checked-in target profiles.
    Profiles,
    /// Inspect one checked-in profile.
    Profile { id: String },
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
        value_name = "SECONDS",
        value_parser = clap::value_parser!(u64).range(1..=3600),
        help = "Read-only dashboard refresh interval; defaults to 10 seconds"
    )]
    pub(crate) refresh_seconds: Option<u64>,
    #[arg(
        long,
        value_name = "ROWS",
        value_parser = clap::value_parser!(u64).range(1..=1000),
        help = "Initial activity rows for the TUI or recorder JSON; defaults to 120 (not valid with plan JSON)"
    )]
    pub(crate) timeline_limit: Option<u64>,
    #[arg(
        long,
        value_name = "PLAN_ID",
        value_parser = parse_ui_plan_id,
        help = "Open this plan's detail view; with --json emit one plan snapshot"
    )]
    pub(crate) plan: Option<String>,
    #[arg(long = "port", hide = true)]
    pub(crate) retired_port: Option<u16>,
}

impl UiOpts {
    pub(crate) fn effective_refresh_seconds(&self) -> u64 {
        self.refresh_seconds.unwrap_or(10)
    }

    pub(crate) fn effective_timeline_limit(&self) -> u64 {
        self.timeline_limit.unwrap_or(120)
    }
}

fn parse_ui_plan_id(value: &str) -> Result<String, String> {
    crate::state::validate_plan_id(value)
        .map(|()| value.to_string())
        .map_err(|error| error.to_string())
}

mod command_conversion;

mod output;
mod prompt_run;
mod run;
mod structured_error;
mod vault_run;

#[cfg(test)]
pub(crate) fn format_doctor_summary_for_test(value: &serde_json::Value) -> String {
    output::format_doctor_summary(value)
}

#[cfg(test)]
pub(crate) fn format_info_summary_for_test(value: &serde_json::Value) -> String {
    output::format_info_summary(value)
}

pub(crate) use run::{is_structured_json_failure, run, structured_error_exit_code};
#[cfg(test)]
pub(crate) use structured_error::is_json_output_already_emitted;
pub(crate) use structured_error::json_command_error;
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
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
#[path = "cli/tests/vault_lifecycle.rs"]
mod vault_lifecycle_tests;
