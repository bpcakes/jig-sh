use std::ffi::OsString;

use anyhow::Result;
use clap::{ArgGroup, Args, Subcommand};
use jig_contract::ComparisonRequestV1;

use crate::command::{NamedCheck, RuntimeCommand};
use crate::{root_commands, tool_defs};

use super::AgentMapOpts;
use super::comparison::{CliExactTreeProvenance, comparison_request};
use super::output;
use super::runtime_dispatch::RuntimeDispatch;

mod convert;

pub(super) const CHECK_AFTER_HELP: &str = "\
Run configured project checks or Jig-owned repository policy checks.

Examples:
  jig check
  jig check fmt
  jig check test
  jig check api:test
  jig check 'web:*'
  jig check --profile ci --explain
  jig check contract";

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
    tool_defs::cli_command::CHECK_MIGRATION_IMMUTABILITY,
    tool_defs::cli_command::CHECK_SQLX_UNCHECKED_NON_TEST,
];

/// `jig check` options that take a value, so a value is never read as a
/// selector.
pub(in crate::cli) const CHECK_VALUE_OPTIONS: &[&str] = &[
    "--profile",
    "--affected",
    "--comparison-base",
    "--comparison-exact-tree",
    "--comparison-provenance",
];

/// Retired top-level spellings and the `jig check` command that replaced each.
const MOVED_COMMANDS: &[(&str, &str)] = &[
    ("fmt-check", "jig check fmt"),
    ("clippy", "jig check clippy"),
    ("test", "jig check test"),
    ("test-locked", "jig check test-locked"),
    ("sqlx-check", "jig check sqlx"),
    ("schema-check", "jig check schema"),
    ("contract-check", "jig check contract"),
    ("check-agent-guides", "jig check agent-guides"),
    (
        "check-migration-immutability",
        "jig check migration-immutability",
    ),
    (
        "check-sqlx-unchecked-non-test",
        "jig check sqlx-unchecked-non-test",
    ),
];

/// Moves global flags that follow the `check` command at `check_index` ahead
/// of its selectors.
///
/// Clap gives everything after an external selector such as `api:test` to the
/// selector list, so a trailing `--json` would never reach the root parser,
/// and `--help` after an external selector would be read as a selector.
pub(in crate::cli) fn normalize_external_global_flags(
    mut args: Vec<OsString>,
    check_index: usize,
) -> Vec<OsString> {
    let separator_index = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    if check_index >= separator_index {
        return args;
    }

    let external_selector = names_external_selector(&args[check_index + 1..separator_index]);
    let mut moved_json = Vec::new();
    let mut moved_help = Vec::new();
    let mut index = check_index + 1;
    let mut option_value = false;
    while index < args.len() && args[index] != "--" {
        if option_value {
            option_value = false;
            index += 1;
        } else if args[index]
            .to_str()
            .is_some_and(|arg| CHECK_VALUE_OPTIONS.contains(&arg))
        {
            option_value = true;
            index += 1;
        } else if args[index] == "--json" {
            moved_json.push(args.remove(index));
        } else if external_selector && matches!(args[index].to_str(), Some("--help" | "-h")) {
            moved_help.push(args.remove(index));
        } else {
            index += 1;
        }
    }
    // Each `--json` moves to the front, which shifts `check` right by one.
    let check_index = check_index + moved_json.len();
    for flag in moved_json.into_iter().rev() {
        args.insert(1, flag);
    }
    for flag in moved_help.into_iter().rev() {
        args.insert(check_index + 1, flag);
    }
    args
}

/// Whether the first positional after `check` is a target selector rather than
/// a named subcommand.
fn names_external_selector(args: &[OsString]) -> bool {
    let mut skip_value = false;
    for arg in args {
        let arg = arg.to_string_lossy();
        if skip_value {
            skip_value = false;
            continue;
        }
        if CHECK_VALUE_OPTIONS.contains(&arg.as_ref()) {
            skip_value = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        return !CHECK_SUBCOMMAND_NAMES.contains(&arg.as_ref());
    }
    false
}

/// Points a retired check spelling at its `jig check` replacement.
///
/// `root_command` is the first command token and `invalid_subcommand` is the
/// name Clap rejected; they are equal when the root command itself is unknown.
pub(in crate::cli) fn moved_command_hint(
    root_command: &str,
    invalid_subcommand: &str,
) -> Option<String> {
    let replacement = if root_command == invalid_subcommand {
        MOVED_COMMANDS
            .iter()
            .find(|(retired, _)| *retired == invalid_subcommand)?
            .1
    } else if root_command == root_commands::AGENT_MAP.name
        && invalid_subcommand == root_commands::CHECK.name
    {
        "jig check agent-map"
    } else {
        return None;
    };
    Some(format!("This check command moved. Use:\n  {replacement}"))
}

#[derive(Args, Debug, Default)]
pub(crate) struct CheckOpts {
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
    #[command(flatten)]
    pub(crate) comparison: CheckComparisonOpts,
    #[command(subcommand)]
    pub(crate) command: Option<CheckCommand>,
}

impl CheckOpts {
    pub(super) fn into_dispatch(self) -> Result<RuntimeDispatch> {
        Ok(RuntimeDispatch::new(
            RuntimeCommand::Check(self.try_into()?),
            output::format_check_output,
        )
        .failing_on_ok_false())
    }

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
            Some(CheckCommand::Named(NamedCheckCommand::Contract(CheckTargetOpts {
                ref selectors,
                ..
            }))) if selectors.is_empty()
        ) && self.profile.is_none()
            && self.affected.is_none()
            && !self.explain
            && !self.fail_fast
    }
}

#[derive(Args, Clone, Debug, Default)]
#[command(group(
    ArgGroup::new("check_comparison_selector")
        .args(["comparison_base", "comparison_exact_tree", "comparison_staged", "comparison_strict_inventory"])
        .multiple(false)
))]
pub(crate) struct CheckComparisonOpts {
    #[arg(
        long = "comparison-base",
        global = true,
        value_name = "GIT_REF",
        help = "Compare native repository checks with the merge base of this ref"
    )]
    pub(crate) comparison_base: Option<String>,
    #[arg(
        long = "comparison-exact-tree",
        global = true,
        value_name = "OID",
        requires = "comparison_provenance",
        help = "Compare native repository checks directly with this commit or tree"
    )]
    pub(crate) comparison_exact_tree: Option<String>,
    #[arg(
        long = "comparison-provenance",
        global = true,
        value_name = "KIND",
        requires = "comparison_exact_tree",
        help = "State the authority carried by --comparison-exact-tree"
    )]
    pub(crate) comparison_provenance: Option<CliExactTreeProvenance>,
    #[arg(
        long = "comparison-staged",
        global = true,
        help = "Use index-against-HEAD authority for native repository checks"
    )]
    pub(crate) comparison_staged: bool,
    #[arg(
        long = "comparison-strict-inventory",
        global = true,
        help = "Use explicit exhaustive inventory authority for native repository checks"
    )]
    pub(crate) comparison_strict_inventory: bool,
}

impl CheckComparisonOpts {
    pub(crate) fn request(&self) -> Result<Option<ComparisonRequestV1>> {
        comparison_request(
            self.comparison_base.as_deref(),
            self.comparison_exact_tree.as_deref(),
            self.comparison_provenance,
            self.comparison_staged,
            self.comparison_strict_inventory,
            "comparison-",
        )
    }
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct CheckTargetOpts {
    #[arg(
        value_name = "SELECTOR",
        help = "Additional component action or target selectors"
    )]
    pub(crate) selectors: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CheckCommand {
    #[command(flatten)]
    Named(NamedCheckCommand),
    /// Check agent-map.md coverage and links.
    #[command(name = tool_defs::cli_command::CHECK_AGENT_MAP)]
    AgentMap(AgentMapOpts),
    /// Validate guide links and owner guides (v9+); check guide structure on older contracts.
    #[command(name = tool_defs::cli_command::CHECK_AGENT_GUIDES)]
    AgentGuides,
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

impl CheckCommand {
    pub(crate) fn has_additional_selectors(&self) -> bool {
        match self {
            Self::Named(named) => !named.target_opts().selectors.is_empty(),
            Self::AgentMap(_)
            | Self::AgentGuides
            | Self::MigrationImmutability(_)
            | Self::SqlxUncheckedNonTest
            | Self::Selectors(_) => false,
        }
    }
}

/// The checks that name a repository action. Each variant is tied to its
/// selector and legacy manifest tool once, in [`Self::into_parts`].
#[derive(Debug, Subcommand)]
pub(crate) enum NamedCheckCommand {
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
}

impl NamedCheckCommand {
    /// The check this subcommand names and its additional selectors.
    pub(crate) fn into_parts(self) -> (NamedCheck, CheckTargetOpts) {
        match self {
            Self::Fmt(opts) => (NamedCheck::FMT, opts),
            Self::Lint(opts) => (NamedCheck::LINT, opts),
            Self::Clippy(opts) => (NamedCheck::CLIPPY, opts),
            Self::Test(opts) => (NamedCheck::TEST, opts),
            Self::TestLocked(opts) => (NamedCheck::TEST_LOCKED, opts),
            Self::TypeScriptLint(opts) => (NamedCheck::TYPESCRIPT_LINT, opts),
            Self::TypeScriptTypecheck(opts) => (NamedCheck::TYPESCRIPT_TYPECHECK, opts),
            Self::TypeScriptBuild(opts) => (NamedCheck::TYPESCRIPT_BUILD, opts),
            Self::TypeScriptCoverage(opts) => (NamedCheck::TYPESCRIPT_COVERAGE, opts),
            Self::Sqlx(opts) => (NamedCheck::SQLX, opts),
            Self::Sqlc(opts) => (NamedCheck::SQLC, opts),
            Self::Schema(opts) => (NamedCheck::SCHEMA, opts),
            Self::Contract(opts) => (NamedCheck::CONTRACT, opts),
        }
    }

    fn target_opts(&self) -> &CheckTargetOpts {
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
            | Self::Contract(opts) => opts,
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct CheckMigrationImmutabilityOpts {
    #[arg(long = "changed-against", help = "Git ref to compare against")]
    pub(crate) changed_against: String,
}
