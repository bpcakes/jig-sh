use super::ToolRequest;
use jig_contract::{ActionEffect, ComparisonRequestV1};

#[derive(Clone, Debug)]
pub(crate) struct RepositoryRunRequest {
    pub(crate) arguments:
        std::collections::BTreeMap<jig_contract::TargetId, jig_contract::ActionArguments>,
    pub(crate) selectors: Vec<String>,
    pub(crate) profile: Option<String>,
    pub(crate) affected_base: Option<String>,
    pub(crate) comparison: Option<ComparisonRequestV1>,
    pub(crate) explain: bool,
    pub(crate) fail_fast: bool,
    pub(crate) approved_effects: Vec<ActionEffect>,
    pub(crate) tool: ToolRequest,
}
