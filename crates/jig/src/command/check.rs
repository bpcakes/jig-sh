//! Check and policy command DTOs.

use std::path::PathBuf;

use super::ToolRequest;

#[derive(Debug)]
pub(crate) enum CheckCommand {
    Repository(RepositoryCheckRequest),
    Fmt(ToolRequest),
    Lint(ToolRequest),
    Clippy(ToolRequest),
    Test(ToolRequest),
    TestLocked(ToolRequest),
    TypeScriptLint(ToolRequest),
    TypeScriptTypecheck(ToolRequest),
    TypeScriptBuild(ToolRequest),
    TypeScriptCoverage(ToolRequest),
    Sqlx(ToolRequest),
    Sqlc(ToolRequest),
    Schema(ToolRequest),
    Contract(ToolRequest),
    AgentMap(AgentMapRequest),
    AgentGuides,
    NoModRs,
    MigrationImmutability(MigrationImmutabilityRequest),
    SqlxUncheckedNonTest,
}

#[derive(Clone, Debug)]
pub(crate) struct RepositoryCheckRequest {
    pub(crate) selectors: Vec<String>,
    pub(crate) profile: Option<String>,
    pub(crate) affected_base: Option<String>,
    pub(crate) explain: bool,
    pub(crate) fail_fast: bool,
    pub(crate) tool: ToolRequest,
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
