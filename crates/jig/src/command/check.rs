//! Check and policy command DTOs.

use std::path::PathBuf;

use jig_contract::ComparisonRequestV1;

use jig_commands::tool_defs::{cli_command, tool};

#[derive(Debug)]
pub(crate) enum CheckCommand {
    Repository(RepositoryCheckRequest),
    Named(NamedCheck),
    AgentMap(AgentMapRequest),
    AgentGuides,
    MigrationImmutability(MigrationImmutabilityRequest),
    SqlxUncheckedNonTest,
}

/// A named repository check: its `jig check <selector>` spelling and the
/// manifest tool that backs it on contracts older than version 6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NamedCheck {
    pub(crate) selector: &'static str,
    pub(crate) legacy_tool: &'static str,
}

impl NamedCheck {
    pub(crate) const FMT: Self = Self::new(cli_command::CHECK_FMT, tool::FMT_CHECK);
    pub(crate) const LINT: Self = Self::new(cli_command::CHECK_LINT, tool::LINT);
    pub(crate) const CLIPPY: Self = Self::new(cli_command::CHECK_CLIPPY, tool::CLIPPY);
    pub(crate) const TEST: Self = Self::new(cli_command::CHECK_TEST, tool::TEST);
    pub(crate) const TEST_LOCKED: Self =
        Self::new(cli_command::CHECK_TEST_LOCKED, tool::TEST_LOCKED);
    pub(crate) const TYPESCRIPT_LINT: Self =
        Self::new(cli_command::CHECK_TYPESCRIPT_LINT, tool::TYPESCRIPT_LINT);
    pub(crate) const TYPESCRIPT_TYPECHECK: Self = Self::new(
        cli_command::CHECK_TYPESCRIPT_TYPECHECK,
        tool::TYPESCRIPT_TYPECHECK,
    );
    pub(crate) const TYPESCRIPT_BUILD: Self =
        Self::new(cli_command::CHECK_TYPESCRIPT_BUILD, tool::TYPESCRIPT_BUILD);
    pub(crate) const TYPESCRIPT_COVERAGE: Self = Self::new(
        cli_command::CHECK_TYPESCRIPT_COVERAGE,
        tool::TYPESCRIPT_COVERAGE,
    );
    pub(crate) const SQLX: Self = Self::new(cli_command::CHECK_SQLX, tool::SQLX_CHECK);
    pub(crate) const SQLC: Self = Self::new(cli_command::CHECK_SQLC, tool::SQLC_CHECK);
    pub(crate) const SCHEMA: Self = Self::new(cli_command::CHECK_SCHEMA, tool::SCHEMA_CHECK);
    pub(crate) const CONTRACT: Self = Self::new(cli_command::CHECK_CONTRACT, tool::CONTRACT_CHECK);

    pub(crate) const ALL: &[Self] = &[
        Self::FMT,
        Self::LINT,
        Self::CLIPPY,
        Self::TEST,
        Self::TEST_LOCKED,
        Self::TYPESCRIPT_LINT,
        Self::TYPESCRIPT_TYPECHECK,
        Self::TYPESCRIPT_BUILD,
        Self::TYPESCRIPT_COVERAGE,
        Self::SQLX,
        Self::SQLC,
        Self::SCHEMA,
        Self::CONTRACT,
    ];

    const fn new(selector: &'static str, legacy_tool: &'static str) -> Self {
        Self {
            selector,
            legacy_tool,
        }
    }

    pub(crate) fn from_selector(selector: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|check| check.selector == selector)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RepositoryCheckRequest {
    pub(crate) selectors: Vec<String>,
    pub(crate) profile: Option<String>,
    pub(crate) affected_base: Option<String>,
    pub(crate) comparison: Option<ComparisonRequestV1>,
    pub(crate) explain: bool,
    pub(crate) fail_fast: bool,
}

// Top-level `jig agent-map generate` and `jig check agent-map` share the same
// request shape, even though they run through different policy paths.
#[derive(Debug)]
pub(crate) enum AgentMapCommand {
    Generate(AgentMapRequest),
}

#[derive(Debug)]
pub(crate) struct AgentMapRequest {
    pub(crate) map_path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct MigrationImmutabilityRequest {
    pub(crate) changed_against: String,
}

#[derive(Debug)]
pub(crate) struct SqlxTodoRequest {
    pub(crate) output: Option<PathBuf>,
}
