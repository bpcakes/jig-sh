use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use jig_commands::root_commands;
use jig_commands::tool_defs;

use crate::bootstrap;
use crate::command::{self, RuntimeCommand};
use runtime_dispatch::RuntimeDispatch;

mod agent;
mod agent_run;
mod bootstrap_hints;
mod bootstrap_run;
mod check;
mod claude;
mod codex;
mod comparison;
mod doctor;
mod file_budget;
mod home_picker;
mod info;
mod init_wizard;
mod loops;
mod migration;
mod proxy;
mod repository_run;
mod runtime_dispatch;
mod setup_run;
mod sqlx;
mod state;
mod status;
mod ui;
mod vault;

pub(crate) use agent::AgentCommand;
pub(crate) use check::{CheckComparisonOpts, CheckOpts};
pub(crate) use claude::ClaudeCommand;
pub(crate) use codex::CodexCommand;
pub(crate) use comparison::CliExactTreeProvenance;
pub(crate) use file_budget::FileBudgetCommand;
pub(crate) use info::InfoOpts;
pub(crate) use loops::LoopCommand;
pub(crate) use migration::{MigrationAddOpts, MigrationCommand};
pub(crate) use proxy::{DevOpts, ProxyCommand};
pub(crate) use sqlx::SqlxCommand;
pub(crate) use state::StateCommand;
pub(crate) use status::StatusOpts;
pub(crate) use vault::VaultCommand;

// Tests build parsed commands directly, so they also reach the families'
// option and subcommand types.
#[cfg(test)]
pub(crate) use {
    agent::AgentBootstrapOpts,
    check::{CheckCommand, CheckTargetOpts, NamedCheckCommand},
    info::InfoCommand,
    proxy::{
        DevLaunchOpts, DevStatusOpts, DevStopOpts, DevSubcommand, ProxyCertCommand, ProxyListOpts,
        ProxyServiceCommand,
    },
    sqlx::{SqlxMigrationCommand, SqlxSchemaCommand},
    status::StatusCommand,
    vault::{
        VaultAuditCommand, VaultBackupCommand, VaultFieldCommand, VaultImportCommand,
        VaultPassphraseCommand, VaultSecretCommand,
    },
};
// Only tests of the built-in dev proxy construct its runtime options.
#[cfg(all(test, feature = "dev-proxy"))]
pub(crate) use proxy::ProxyRuntimeOpts;

#[derive(Debug, Parser)]
#[command(
    name = "jig",
    version = env!("JIG_DISPLAY_VERSION"),
    propagate_version = true,
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
const LAUNCHER_CHECK_SUBCOMMANDS: &str = "fmt,lint,clippy,test,test-locked,typescript-lint,typescript-typecheck,typescript-build,typescript-coverage,sqlx,sqlc,schema,contract,agent-map,agent-guides,migration-immutability,sqlx-unchecked-non-test";

const ROOT_COMMON_WORKFLOWS: &str = "\
Common workflows:
  jig doctor           Check repository setup and get the next remediation step
  jig info --commands  Show which commands are usable in this repository
  jig dev              Start configured development apps
  jig check test       Run the configured test suite
  jig state summary    Summarize recorded local state";

fn root_after_help() -> String {
    format!(
        "{}\n{ROOT_COMMON_WORKFLOWS}",
        root_commands::categorized_help()
    )
}

const STATUS_AFTER_HELP: &str = "\
Collects local Git state and loop leases and attempts.
The command is read-only and does not fetch remotes.

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

const VAULT_AFTER_HELP: &str = "\
Jig Vault stores encrypted project fields outside the repository. References
are project-relative: jig://Production/TOKEN selects the current repo-scoped,
global, or explicit-home vault; the project name is never a reference segment.
Both concealed and text fields are encrypted. vault exec redacts only concealed
fields, so text stays visible when deliberately passed to a command; the
compatible vault run broker redacts every injected value of at least 4 bytes.
Terminal use prompts for the vault passphrase; automation the operator runs
outside any agent session can provide JIG_VAULT_PASSPHRASE. Agents must ask the
operator to run passphrase-requiring commands in a terminal and must never
request, choose, or set the passphrase. Command-line passphrases are not accepted.

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
  jig vault run --env TOKEN=jig://Production/TOKEN -- command
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
    Bootstrap,
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
        after_help = doctor::DOCTOR_AFTER_HELP
    )]
    Doctor,
    /// Summarize repo Jig configuration, capabilities, gates, and dev apps.
    #[command(
        name = root_commands::INFO.name,
        display_order = root_commands::INFO.display_order,
        visible_alias = "explain",
        after_help = info::INFO_AFTER_HELP
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
    /// Run declared repository actions in the foreground
    #[command(name = root_commands::RUN.name, display_order = root_commands::RUN.display_order)]
    Run(repository_run::RepositoryRunOpts),
    /// Run built-in file-budget diagnostics without creating a run.
    #[command(
        name = root_commands::FILE_BUDGET.name,
        display_order = root_commands::FILE_BUDGET.display_order,
        subcommand
    )]
    FileBudget(FileBudgetCommand),
    /// Aggregate local repository and loop observations.
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
        after_help = ui::UI_AFTER_HELP
    )]
    Ui(ui::UiOpts),
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
    #[command(name = root_commands::MIGRATION_ADD.name, hide = true)]
    MigrationAdd(MigrationAddOpts),
    /// Regenerate schema documentation when schema dumps are enabled.
    #[command(name = root_commands::SCHEMA_DUMP.name, hide = true)]
    SchemaDump,
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
        name = root_commands::GENERATE_SQLX_UNCHECKED_QUERIES_TODO.name,
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
    /// Inspect Claude configuration homes or launch Claude Code with a selected home.
    #[command(name = root_commands::CLAUDE.name, display_order = root_commands::CLAUDE.display_order, subcommand, after_help = claude::AFTER_HELP)]
    Claude(ClaudeCommand),
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
    /// Validate this binary against a generated repository launcher contract.
    #[command(name = root_commands::RUNTIME_COMPATIBLE.name, hide = true)]
    RuntimeCompatible(RuntimeCompatibleOpts),
}

#[derive(Args, Debug)]
pub(crate) struct RuntimeCompatibleOpts {
    #[arg(long, value_enum)]
    pub(crate) profile: RuntimeCompatibilityProfile,
    // The alias lets pin-aware launchers distinguish this runtime from older
    // releases before an update can replace their generated scripts.
    #[arg(long, alias = "require-runtime-pin-update", hide = true)]
    pub(crate) capability_only: bool,
    #[arg(long, hide = true)]
    pub(crate) contract_version: Option<u32>,
    pub(crate) repo_root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum RuntimeCompatibilityProfile {
    Default,
    Runtime,
}

impl RuntimeCompatibilityProfile {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentMapCommand {
    /// Rewrite agent-map.md from tracked AGENTS.md files.
    #[command(name = tool_defs::cli_command::AGENT_MAP_GENERATE)]
    Generate(AgentMapOpts),
}

impl AgentMapCommand {
    fn into_dispatch(self) -> RuntimeDispatch {
        RuntimeDispatch::new(
            RuntimeCommand::AgentMap(self.into()),
            output::format_agent_map_generate_summary,
        )
    }
}

impl From<AgentMapCommand> for command::AgentMapCommand {
    fn from(command: AgentMapCommand) -> Self {
        match command {
            AgentMapCommand::Generate(opts) => Self::Generate(opts.into()),
        }
    }
}

impl From<AgentMapOpts> for command::AgentMapRequest {
    fn from(opts: AgentMapOpts) -> Self {
        Self {
            map_path: opts.map_path,
        }
    }
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

#[derive(Args, Debug)]
pub(crate) struct GenerateSqlxUncheckedQueriesTodoOpts {
    /// Optional output path for the generated TODO report.
    pub(crate) output: Option<PathBuf>,
}

impl From<GenerateSqlxUncheckedQueriesTodoOpts> for command::SqlxTodoRequest {
    fn from(opts: GenerateSqlxUncheckedQueriesTodoOpts) -> Self {
        Self {
            output: opts.output,
        }
    }
}

impl GenerateSqlxUncheckedQueriesTodoOpts {
    fn into_dispatch(self) -> RuntimeDispatch {
        RuntimeDispatch::tool(RuntimeCommand::GenerateSqlxUncheckedQueriesTodo(
            self.into(),
        ))
    }
}

mod output;
mod run;
mod structured_error;

#[cfg(test)]
pub(crate) fn format_doctor_summary_for_test(value: &serde_json::Value) -> String {
    doctor::render::format_doctor_summary(value)
}

#[cfg(test)]
pub(crate) fn format_info_summary_for_test(value: &serde_json::Value) -> String {
    info::render::format_info_summary(value)
}

pub(crate) use run::run;

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
