//! Check and policy command DTOs.

use std::path::PathBuf;

use jig_contract::ComparisonRequestV1;

#[derive(Debug)]
pub(crate) enum CheckCommand {
    Repository(RepositoryCheckRequest),
    Fmt,
    Lint,
    Clippy,
    Test,
    TestLocked,
    TypeScriptLint,
    TypeScriptTypecheck,
    TypeScriptBuild,
    TypeScriptCoverage,
    Sqlx,
    Sqlc,
    Schema,
    Contract,
    AgentMap(AgentMapRequest),
    AgentGuides,
    MigrationImmutability(MigrationImmutabilityRequest),
    SqlxUncheckedNonTest,
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
