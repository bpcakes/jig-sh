use anyhow::{Result, bail};
use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use jig_contract::{ComparisonRequestV1, ExactTreeProvenanceV1, StrictInventoryReasonV1};

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
    #[command(flatten)]
    pub(crate) comparison: CheckComparisonOpts,
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
    pub(crate) comparison_provenance: Option<CheckExactTreeProvenance>,
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

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum CheckExactTreeProvenance {
    #[value(name = "explicit")]
    Explicit,
    #[value(name = "push_before")]
    PushBefore,
}

impl CheckComparisonOpts {
    pub(crate) fn request(&self) -> Result<Option<ComparisonRequestV1>> {
        let selector_count = usize::from(self.comparison_base.is_some())
            + usize::from(self.comparison_exact_tree.is_some())
            + usize::from(self.comparison_staged)
            + usize::from(self.comparison_strict_inventory);
        if selector_count > 1 {
            bail!(
                "--comparison-base, --comparison-exact-tree, --comparison-staged, and --comparison-strict-inventory are mutually exclusive"
            );
        }
        if self.comparison_exact_tree.is_some() != self.comparison_provenance.is_some() {
            bail!("--comparison-exact-tree and --comparison-provenance must be supplied together");
        }
        if let Some(requested_ref) = &self.comparison_base {
            return Ok(Some(ComparisonRequestV1::MergeBaseRef {
                requested_ref: requested_ref.clone(),
            }));
        }
        if let Some(requested_oid) = &self.comparison_exact_tree {
            let provenance = match self
                .comparison_provenance
                .expect("comparison provenance was validated above")
            {
                CheckExactTreeProvenance::Explicit => ExactTreeProvenanceV1::Explicit,
                CheckExactTreeProvenance::PushBefore => ExactTreeProvenanceV1::PushBefore,
            };
            return Ok(Some(ComparisonRequestV1::ExactTree {
                requested_oid: requested_oid.clone(),
                provenance,
            }));
        }
        if self.comparison_staged {
            return Ok(Some(ComparisonRequestV1::IndexAgainstHead));
        }
        Ok(self
            .comparison_strict_inventory
            .then_some(ComparisonRequestV1::StrictInventory {
                reason: StrictInventoryReasonV1::ExplicitCheck,
            }))
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
pub(crate) struct CheckMigrationImmutabilityOpts {
    #[arg(long = "changed-against", help = "Git ref to compare against")]
    pub(crate) changed_against: String,
}
