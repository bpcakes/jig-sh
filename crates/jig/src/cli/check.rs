use clap::{Args, Subcommand};

use crate::tool_defs;

use super::{AgentMapOpts, ToolOpts};

pub(super) const CHECK_AFTER_HELP: &str = "\
Run configured project checks or Jig-owned repository policy checks.

Examples:
  jig check
  jig check fmt
  jig check test
  jig check api:test
  jig check 'web:*'
  jig check --profile ci --explain
  jig check contract
  jig check rust-file-loc --changed-against origin/main";

pub(crate) const CHECK_SUBCOMMAND_NAMES: &[&str] = &[
    tool_defs::cli_command::CHECK_FMT,
    tool_defs::cli_command::CHECK_LINT,
    tool_defs::cli_command::CHECK_CLIPPY,
    tool_defs::cli_command::CHECK_TEST,
    tool_defs::cli_command::CHECK_TEST_LOCKED,
    tool_defs::cli_command::CHECK_TYPESCRIPT_LINT,
    tool_defs::cli_command::CHECK_TYPESCRIPT_TYPECHECK,
    tool_defs::cli_command::CHECK_TYPESCRIPT_BUILD,
    tool_defs::cli_command::CHECK_TYPESCRIPT_COVERAGE,
    tool_defs::cli_command::CHECK_SQLX,
    tool_defs::cli_command::CHECK_SQLC,
    tool_defs::cli_command::CHECK_SCHEMA,
    tool_defs::cli_command::CHECK_CONTRACT,
    tool_defs::cli_command::CHECK_AGENT_MAP,
    tool_defs::cli_command::CHECK_AGENT_GUIDES,
    tool_defs::cli_command::CHECK_RUST_FILE_LOC,
    tool_defs::cli_command::CHECK_NO_MOD_RS,
    tool_defs::cli_command::CHECK_MIGRATION_IMMUTABILITY,
    tool_defs::cli_command::CHECK_SQLX_UNCHECKED_NON_TEST,
];

#[derive(Args, Debug, Default)]
pub(crate) struct CheckOpts {
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
    #[arg(
        long,
        global = true,
        value_name = "PROFILE",
        help = "Select a checked-in target profile"
    )]
    pub(crate) profile: Option<String>,
    #[arg(
        long,
        global = true,
        value_name = "GIT_REF",
        help = "Select targets affected since a Git ref"
    )]
    pub(crate) affected: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Resolve and print the immutable run plan without executing it"
    )]
    pub(crate) explain: bool,
    #[arg(
        long,
        global = true,
        help = "Stop scheduling checks after the first failed target"
    )]
    pub(crate) fail_fast: bool,
    #[command(subcommand)]
    pub(crate) command: Option<CheckCommand>,
}

impl CheckOpts {
    #[cfg(test)]
    pub(crate) fn with_command(command: CheckCommand) -> Self {
        Self {
            command: Some(command),
            ..Self::default()
        }
    }

    pub(crate) fn is_contract_only(&self) -> bool {
        matches!(
            self.command,
            Some(CheckCommand::Contract(CheckTargetOpts {
                ref selectors,
                ..
            })) if selectors.is_empty()
        ) && self.profile.is_none()
            && self.affected.is_none()
            && !self.explain
            && !self.fail_fast
    }
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct CheckTargetOpts {
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
    #[arg(
        value_name = "SELECTOR",
        help = "Additional component action or target selectors"
    )]
    pub(crate) selectors: Vec<String>,
}

impl CheckCommand {
    pub(crate) fn has_additional_selectors(&self) -> bool {
        match self {
            Self::Fmt(opts)
            | Self::Lint(opts)
            | Self::Clippy(opts)
            | Self::Test(opts)
            | Self::TestLocked(opts)
            | Self::TypeScriptLint(opts)
            | Self::TypeScriptTypecheck(opts)
            | Self::TypeScriptBuild(opts)
            | Self::TypeScriptCoverage(opts)
            | Self::Sqlx(opts)
            | Self::Sqlc(opts)
            | Self::Schema(opts)
            | Self::Contract(opts) => !opts.selectors.is_empty(),
            Self::AgentMap(_)
            | Self::AgentGuides
            | Self::RustFileLoc(_)
            | Self::NoModRs
            | Self::MigrationImmutability(_)
            | Self::SqlxUncheckedNonTest
            | Self::Selectors(_) => false,
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum CheckCommand {
    /// Run the configured Rust format check.
    #[command(name = tool_defs::cli_command::CHECK_FMT)]
    Fmt(CheckTargetOpts),
    /// Run the configured language lint check.
    #[command(name = tool_defs::cli_command::CHECK_LINT)]
    Lint(CheckTargetOpts),
    /// Run the configured Rust clippy check.
    #[command(name = tool_defs::cli_command::CHECK_CLIPPY)]
    Clippy(CheckTargetOpts),
    /// Run the configured default test command.
    #[command(name = tool_defs::cli_command::CHECK_TEST)]
    Test(CheckTargetOpts),
    /// Run the configured locked test command.
    #[command(name = tool_defs::cli_command::CHECK_TEST_LOCKED)]
    TestLocked(CheckTargetOpts),
    /// Run the configured TypeScript lint command.
    #[command(name = tool_defs::cli_command::CHECK_TYPESCRIPT_LINT)]
    TypeScriptLint(CheckTargetOpts),
    /// Run the configured TypeScript typecheck command.
    #[command(name = tool_defs::cli_command::CHECK_TYPESCRIPT_TYPECHECK)]
    TypeScriptTypecheck(CheckTargetOpts),
    /// Run the configured TypeScript build command.
    #[command(name = tool_defs::cli_command::CHECK_TYPESCRIPT_BUILD)]
    TypeScriptBuild(CheckTargetOpts),
    /// Run the configured TypeScript coverage command.
    #[command(name = tool_defs::cli_command::CHECK_TYPESCRIPT_COVERAGE)]
    TypeScriptCoverage(CheckTargetOpts),
    /// Verify committed SQLx metadata when SQLx is enabled.
    #[command(name = tool_defs::cli_command::CHECK_SQLX)]
    Sqlx(CheckTargetOpts),
    /// Verify sqlc queries and checked-in generated output.
    #[command(name = tool_defs::cli_command::CHECK_SQLC)]
    Sqlc(CheckTargetOpts),
    /// Verify generated schema documentation when schema dumps are enabled.
    #[command(name = tool_defs::cli_command::CHECK_SCHEMA)]
    Schema(CheckTargetOpts),
    /// Validate the generated Jig command contract and runtime wiring.
    #[command(name = tool_defs::cli_command::CHECK_CONTRACT)]
    Contract(CheckTargetOpts),
    /// Check agent-map.md coverage and links.
    #[command(name = tool_defs::cli_command::CHECK_AGENT_MAP)]
    AgentMap(AgentMapOpts),
    /// Verify crate-level AGENTS.md guide coverage and required sections.
    #[command(name = tool_defs::cli_command::CHECK_AGENT_GUIDES)]
    AgentGuides,
    /// Enforce Rust file-size policy for changed or tracked files.
    #[command(name = tool_defs::cli_command::CHECK_RUST_FILE_LOC)]
    RustFileLoc(CheckRustFileLocOpts),
    /// Fail if disallowed mod.rs files exist under configured crate roots.
    #[command(name = tool_defs::cli_command::CHECK_NO_MOD_RS)]
    NoModRs,
    /// Verify existing migrations were not mutated.
    #[command(name = tool_defs::cli_command::CHECK_MIGRATION_IMMUTABILITY)]
    MigrationImmutability(CheckMigrationImmutabilityOpts),
    /// Verify non-test SQLx queries use compile-time checked macros.
    #[command(name = tool_defs::cli_command::CHECK_SQLX_UNCHECKED_NON_TEST)]
    SqlxUncheckedNonTest,
    /// Select one or more component actions using target syntax.
    #[command(external_subcommand)]
    Selectors(Vec<String>),
}

#[derive(Args, Debug)]
pub(crate) struct CheckRustFileLocOpts {
    #[arg(long, help = "Check staged Rust files against HEAD.")]
    pub(crate) staged: bool,
    #[arg(
        long = "changed-against",
        help = "Check Rust files changed between the given git ref and HEAD."
    )]
    pub(crate) changed_against: Option<String>,
    #[arg(
        long,
        help = "Check all tracked Rust files against a zero baseline; existing oversized legacy files fail unless annotated."
    )]
    pub(crate) all: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CheckMigrationImmutabilityOpts {
    #[arg(long = "changed-against", help = "Git ref to compare against")]
    pub(crate) changed_against: String,
}
