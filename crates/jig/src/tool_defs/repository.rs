use jig_contract::{RunPlan, RunResult, RunStatus};
use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::repository::CatalogInspection;
use crate::state::DurableRun;

use super::tool;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepositoryTool {
    Inspect,
    PlanRun,
    ExecuteRun,
    CancelRun,
}

impl RepositoryTool {
    pub(crate) const ALL: &'static [Self] = &[
        Self::Inspect,
        Self::PlanRun,
        Self::ExecuteRun,
        Self::CancelRun,
    ];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            tool::INSPECT => Some(Self::Inspect),
            tool::PLAN_RUN => Some(Self::PlanRun),
            tool::EXECUTE_RUN => Some(Self::ExecuteRun),
            tool::CANCEL_RUN => Some(Self::CancelRun),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Inspect => tool::INSPECT,
            Self::PlanRun => tool::PLAN_RUN,
            Self::ExecuteRun => tool::EXECUTE_RUN,
            Self::CancelRun => tool::CANCEL_RUN,
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Inspect => {
                "Inspect the repository catalog or the durable state of one repository run."
            }
            Self::PlanRun => {
                "Resolve selectors or a profile into an immutable, explainable repository run plan."
            }
            Self::ExecuteRun => {
                "Validate an exact run plan, start it in the background, and return its durable run handle immediately."
            }
            Self::CancelRun => {
                "Request cooperative cancellation of a run and return its current durable state."
            }
        }
    }

    pub(crate) fn descriptor(self) -> Value {
        let (input, output) = match self {
            Self::Inspect => (
                schema_value::<RepositoryInspectArgs>(),
                schema_value::<RepositoryInspectOutput>(),
            ),
            Self::PlanRun => (
                schema_value::<PlanRunArgs>(),
                schema_value::<PlanRunOutput>(),
            ),
            Self::ExecuteRun => (
                schema_value::<ExecuteRunArgs>(),
                schema_value::<ExecuteRunOutput>(),
            ),
            Self::CancelRun => (
                schema_value::<CancelRunArgs>(),
                schema_value::<CancelRunOutput>(),
            ),
        };
        json!({
            "name": self.name(),
            "description": self.description(),
            "inputSchema": input,
            "outputSchema": output,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryInspectKind {
    Workspace,
    Components,
    Component,
    Targets,
    Target,
    Profiles,
    Profile,
    Run,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RepositoryInspectArgs {
    Workspace,
    Components,
    Component { id: String },
    Targets,
    Target { id: String },
    Profiles,
    Profile { id: String },
    Run { run_id: String },
}

impl RepositoryInspectArgs {
    pub(crate) const fn kind(&self) -> RepositoryInspectKind {
        match self {
            Self::Workspace => RepositoryInspectKind::Workspace,
            Self::Components => RepositoryInspectKind::Components,
            Self::Component { .. } => RepositoryInspectKind::Component,
            Self::Targets => RepositoryInspectKind::Targets,
            Self::Target { .. } => RepositoryInspectKind::Target,
            Self::Profiles => RepositoryInspectKind::Profiles,
            Self::Profile { .. } => RepositoryInspectKind::Profile,
            Self::Run { .. } => RepositoryInspectKind::Run,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanRunArgs {
    #[serde(default)]
    pub(crate) selectors: Vec<String>,
    #[serde(default)]
    pub(crate) profile: Option<String>,
    #[serde(default)]
    pub(crate) affected_base: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecuteRunArgs {
    pub(crate) plan: RunPlan,
    #[serde(default)]
    pub(crate) work_plan_id: Option<String>,
    #[serde(default = "default_true")]
    pub(crate) record_receipts: bool,
    #[serde(default)]
    pub(crate) fail_fast: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancelRunArgs {
    pub(crate) run_id: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryInspectOutput {
    pub(crate) ok: bool,
    pub(crate) schema_version: u32,
    pub(crate) kind: RepositoryInspectKind,
    pub(crate) result: RepositoryInspectResult,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(untagged)]
pub(crate) enum RepositoryInspectResult {
    Catalog(CatalogInspection),
    Run(RunInspection),
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunInspection {
    pub(crate) run: DurableRun,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanRunOutput {
    pub(crate) ok: bool,
    pub(crate) schema_version: u32,
    pub(crate) plan: RunPlan,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecuteRunOutput {
    pub(crate) ok: bool,
    pub(crate) schema_version: u32,
    pub(crate) accepted: bool,
    pub(crate) run_id: String,
    pub(crate) status: RunStatus,
    pub(crate) run: RunResult,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancelRunOutput {
    pub(crate) ok: bool,
    pub(crate) schema_version: u32,
    pub(crate) run_id: String,
    pub(crate) cancellation_requested: bool,
    pub(crate) worker_signalled: bool,
    pub(crate) run: RunResult,
}

fn schema_value<T: JsonSchema>() -> Value {
    let schema = SchemaGenerator::new(SchemaSettings::draft2020_12()).into_root_schema_for::<T>();
    serde_json::to_value(schema).expect("repository MCP schemas must serialize")
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_tools_have_closed_input_and_output_schemas() {
        for tool in RepositoryTool::ALL {
            let descriptor = tool.descriptor();
            jsonschema::validator_for(&descriptor["inputSchema"])
                .expect("repository MCP input schema must compile");
            jsonschema::validator_for(&descriptor["outputSchema"])
                .expect("repository MCP output schema must compile");
            assert_eq!(descriptor["outputSchema"]["type"], "object");
            assert_eq!(descriptor["outputSchema"]["additionalProperties"], false);
        }
    }

    #[test]
    fn execute_defaults_to_receipts_without_defaulting_fail_fast() {
        let plan = RunPlan::new(
            "plan",
            "config",
            jig_contract::SourceIdentity::new(None, "worktree"),
            Vec::new(),
            Vec::new(),
        );
        let parsed = serde_json::from_value::<ExecuteRunArgs>(json!({ "plan": plan })).unwrap();

        assert!(parsed.record_receipts);
        assert!(!parsed.fail_fast);
    }

    #[test]
    fn repository_input_schemas_reject_unknown_fields() {
        let plan = RunPlan::new(
            "plan",
            "config",
            jig_contract::SourceIdentity::new(None, "worktree"),
            Vec::new(),
            Vec::new(),
        );
        let cases = [
            (
                RepositoryTool::Inspect,
                json!({"kind": "workspace"}),
                json!({"kind": "workspace", "unexpected": true}),
            ),
            (
                RepositoryTool::PlanRun,
                json!({}),
                json!({"unexpected": true}),
            ),
            (
                RepositoryTool::ExecuteRun,
                json!({"plan": plan}),
                json!({"plan": plan, "unexpected": true}),
            ),
            (
                RepositoryTool::CancelRun,
                json!({"run_id": "run_1"}),
                json!({"run_id": "run_1", "unexpected": true}),
            ),
        ];

        for (tool, valid, unknown) in cases {
            let schema = tool.descriptor()["inputSchema"].clone();
            let validator = jsonschema::validator_for(&schema).unwrap();
            assert!(
                validator.is_valid(&valid),
                "{} rejected valid input",
                tool.name()
            );
            assert!(
                !validator.is_valid(&unknown),
                "{} schema accepted an unknown field",
                tool.name()
            );
        }
    }
}
