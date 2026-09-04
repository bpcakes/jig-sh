use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use serde_json::Value;

use crate::{
    ActionEffect, ActionIntent, ActionRunner, NativeFileBudgetConfigV1, ProfileId, ResultParser,
    TargetId,
};

/// Closed arguments accepted by repository action runners.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActionArguments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ActionArguments {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
    }
}

/// The exact Git and worktree state against which a plan was resolved.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub worktree_fingerprint: String,
}

impl SourceIdentity {
    #[must_use]
    pub fn new(commit: Option<String>, worktree_fingerprint: impl Into<String>) -> Self {
        Self {
            commit,
            worktree_fingerprint: worktree_fingerprint.into(),
        }
    }
}

/// A stable explanation for why a target appears in a plan.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectionReason {
    Explicit {
        selector: String,
    },
    Profile {
        profile: ProfileId,
    },
    DirectInput {
        path: String,
    },
    UnclaimedInput {
        path: String,
    },
    ComponentDependency {
        component: crate::ComponentId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    ActionDependency {
        target: TargetId,
    },
    LegacyAlias {
        alias: String,
    },
}

/// One fully resolved target within an immutable run plan.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedTarget {
    pub target: TargetId,
    pub intent: ActionIntent,
    pub effects: Vec<ActionEffect>,
    pub runner: ActionRunner,
    #[serde(default, skip_serializing_if = "ActionArguments::is_empty")]
    pub arguments: ActionArguments,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "is_default_result_parser")]
    pub result_parser: ResultParser,
    pub input_digest: String,
    pub reasons: Vec<SelectionReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_reason_count: Option<usize>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub selection_reasons_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_reasons_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_native_input: Option<PreparedNativeInputV1>,
}

impl PlannedTarget {
    #[must_use]
    pub fn new(
        target: TargetId,
        intent: ActionIntent,
        runner: ActionRunner,
        input_digest: impl Into<String>,
    ) -> Self {
        Self {
            target,
            intent,
            effects: Vec::new(),
            runner,
            arguments: ActionArguments::default(),
            inputs: Vec::new(),
            depends_on: Vec::new(),
            timeout_seconds: None,
            result_parser: ResultParser::default(),
            input_digest: input_digest.into(),
            reasons: Vec::new(),
            selection_reason_count: None,
            selection_reasons_truncated: false,
            selection_reasons_digest: None,
            prepared_native_input: None,
        }
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CurrentViewV1 {
    Worktree,
    Index,
    Inventory,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ExactTreeProvenanceV1 {
    Explicit,
    WorkPlan,
    PushBefore,
    UnbornWorktree,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum StrictInventoryReasonV1 {
    ExplicitAudit,
    ExplicitCheck,
    MissingComparisonFallback,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComparisonRequestV1 {
    MergeBaseRef {
        requested_ref: String,
    },
    ExactTree {
        requested_oid: String,
        provenance: ExactTreeProvenanceV1,
    },
    IndexAgainstHead,
    StrictInventory {
        reason: StrictInventoryReasonV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonPreparationFailureV1 {
    pub code: String,
    pub message: String,
    pub failure_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StrictInventoryFallbackV1 {
    pub original_request: ComparisonRequestV1,
    pub failure: ComparisonPreparationFailureV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempted_object_ids: Vec<String>,
    pub failure_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedComparisonV1 {
    MergeBase {
        requested_ref: String,
        resolved_ref_oid: String,
        head_oid: String,
        merge_base_oid: String,
    },
    ExactTree {
        requested_oid: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peeled_commit_oid: Option<String>,
        tree_oid: String,
        provenance: ExactTreeProvenanceV1,
    },
    IndexAgainstHead {
        head_or_empty_oid: String,
    },
    StrictInventory {
        reason: StrictInventoryReasonV1,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fallback_from: Option<StrictInventoryFallbackV1>,
    },
}

impl ResolvedComparisonV1 {
    #[must_use]
    pub fn baseline_oid(&self) -> Option<&str> {
        match self {
            Self::MergeBase { merge_base_oid, .. } => Some(merge_base_oid),
            Self::ExactTree { tree_oid, .. } => Some(tree_oid),
            Self::IndexAgainstHead { head_or_empty_oid } => Some(head_or_empty_oid),
            Self::StrictInventory { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySourceV1 {
    pub path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyPreparationFailureV1 {
    Missing,
    Unreadable,
    Invalid,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedDiagnosticV1 {
    pub severity: FindingSeverity,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyPreparationV1 {
    Ready {
        policy_raw_digest: String,
        policy_semantic_digest: String,
    },
    InvalidPolicy {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        policy_raw_digest: Option<String>,
        reason: PolicyPreparationFailureV1,
        diagnostics_count: u64,
        diagnostics_digest: String,
        diagnostics_preview: Vec<PreparedDiagnosticV1>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComparisonPreparationV1 {
    Ready {
        comparison: ResolvedComparisonV1,
    },
    ComparisonUnavailable {
        reason: ComparisonPreparationFailureV1,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attempted_object_ids: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedNativeInputV1 {
    pub schema_version: u32,
    pub view: CurrentViewV1,
    pub request: ComparisonRequestV1,
    pub configuration: NativeFileBudgetConfigV1,
    pub policy_source: PolicySourceV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_plan_id: Option<String>,
    pub policy: PolicyPreparationV1,
    pub comparison: ComparisonPreparationV1,
}

impl PreparedNativeInputV1 {
    pub const SCHEMA_VERSION: u32 = 1;
}

/// An immutable resolution of a selection request into exact executable targets.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunPlan {
    pub schema_version: u32,
    pub id: String,
    pub config_digest: String,
    pub source: SourceIdentity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selectors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_base: Option<String>,
    pub targets: Vec<PlannedTarget>,
    pub execution_layers: Vec<Vec<TargetId>>,
    pub effects: Vec<ActionEffect>,
}

impl RunPlan {
    pub const SCHEMA_VERSION: u32 = 3;

    #[must_use]
    pub fn new(
        id: impl Into<String>,
        config_digest: impl Into<String>,
        source: SourceIdentity,
        targets: Vec<PlannedTarget>,
        execution_layers: Vec<Vec<TargetId>>,
    ) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            id: id.into(),
            config_digest: config_digest.into(),
            source,
            selectors: Vec::new(),
            profile: None,
            affected_base: None,
            targets,
            execution_layers,
            effects: Vec::new(),
        }
    }
}

/// Lifecycle status, independent of a terminal conclusion.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Completed,
}

/// The terminal meaning of a completed run or target run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunConclusion {
    Success,
    Failure,
    Cancelled,
    TimedOut,
    Blocked,
    Skipped,
}

/// Severity of a normalized finding.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Notice,
    Warning,
    Error,
}

/// Optional source location for a normalized finding.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FindingLocation {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
}

/// A structured diagnostic emitted by a target runner or result parser.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub severity: FindingSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<FindingLocation>,
}

impl Finding {
    #[must_use]
    pub fn new(severity: FindingSeverity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            code: None,
            source: None,
            location: None,
        }
    }
}

/// A durable log or artifact associated with evidence.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReference {
    pub kind: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// The normalized result of one target within a run.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRunResult {
    pub target: TargetId,
    pub status: RunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<RunConclusion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    pub config_digest: String,
    pub input_digest: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finding_count: Option<u64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub findings_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_evidence: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluated_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until_ms: Option<u64>,
}

impl TargetRunResult {
    #[must_use]
    pub fn queued(
        target: TargetId,
        config_digest: impl Into<String>,
        input_digest: impl Into<String>,
    ) -> Self {
        Self {
            target,
            status: RunStatus::Queued,
            conclusion: None,
            started_at_ms: None,
            ended_at_ms: None,
            exit_code: None,
            receipt_id: None,
            config_digest: config_digest.into(),
            input_digest: input_digest.into(),
            findings: Vec::new(),
            evidence: Vec::new(),
            finding_count: None,
            findings_truncated: false,
            findings_digest: None,
            native_evidence: None,
            evaluated_at_ms: None,
            valid_until_ms: None,
        }
    }
}

/// A typed, bounded result returned directly by an in-process native action.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeActionResult {
    pub conclusion: RunConclusion,
    #[serde(default)]
    pub findings: Vec<Finding>,
    pub finding_count: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub findings_truncated: bool,
    pub findings_digest: String,
    #[serde(default)]
    pub human_output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Value>,
    pub evaluated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until_ms: Option<u64>,
}

/// The current folded state of one durable run.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunResult {
    pub run_id: String,
    pub plan_id: String,
    pub status: RunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<RunConclusion>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub targets: Vec<TargetRunResult>,
}

impl RunResult {
    #[must_use]
    pub fn queued(
        run_id: impl Into<String>,
        plan_id: impl Into<String>,
        created_at_ms: u64,
        targets: Vec<TargetRunResult>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            plan_id: plan_id.into(),
            status: RunStatus::Queued,
            conclusion: None,
            created_at_ms,
            updated_at_ms: created_at_ms,
            targets,
        }
    }
}

fn is_default_result_parser(value: &ResultParser) -> bool {
    *value == ResultParser::default()
}

const fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use crate::{
        ActionEffect, ActionIntent, ActionRunner, Finding, FindingSeverity, PlannedTarget,
        RunConclusion, RunPlan, RunResult, RunStatus, SourceIdentity, TargetId, TargetRunResult,
    };

    #[test]
    fn plan_json_is_structured_and_stable() {
        let target: TargetId = "api:test".parse().unwrap();
        let mut planned = PlannedTarget::new(
            target.clone(),
            ActionIntent::Check,
            ActionRunner::command("go_test_command"),
            "sha256:input",
        );
        planned.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        let plan = RunPlan::new(
            "run-plan_sha256",
            "sha256:config",
            SourceIdentity::new(Some("abc123".to_owned()), "worktree-1"),
            vec![planned],
            vec![vec![target]],
        );

        assert_eq!(
            serde_json::to_value(plan).unwrap(),
            serde_json::json!({
                "schema_version": 3,
                "id": "run-plan_sha256",
                "config_digest": "sha256:config",
                "source": {"commit": "abc123", "worktree_fingerprint": "worktree-1"},
                "targets": [{
                    "target": {"component": "api", "action": "test"},
                    "intent": "check",
                    "effects": ["read_only", "process"],
                    "runner": {"kind": "command", "command": "go_test_command"},
                    "input_digest": "sha256:input",
                    "reasons": []
                }],
                "execution_layers": [[{"component": "api", "action": "test"}]],
                "effects": []
            })
        );
    }

    #[test]
    fn status_and_conclusion_are_independent() {
        let target = "api:test".parse().unwrap();
        let mut target_result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
        assert_eq!(target_result.status, RunStatus::Queued);
        assert_eq!(target_result.conclusion, None);

        target_result.status = RunStatus::Completed;
        target_result.conclusion = Some(RunConclusion::Failure);
        target_result
            .findings
            .push(Finding::new(FindingSeverity::Error, "one test failed"));
        let mut result = RunResult::queued("run_1", "plan_1", 10, vec![target_result]);
        result.status = RunStatus::Completed;
        result.conclusion = Some(RunConclusion::Failure);
        result.updated_at_ms = 20;

        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["status"], "completed");
        assert_eq!(value["conclusion"], "failure");
        assert_eq!(value["targets"][0]["findings"][0]["severity"], "error");
    }

    #[test]
    fn nonterminal_results_omit_conclusion() {
        let target =
            TargetRunResult::queued("web:test".parse().unwrap(), "sha256:config", "sha256:input");
        let value = serde_json::to_value(target).unwrap();
        assert_eq!(value["status"], "queued");
        assert_eq!(value.get("conclusion"), None);
    }

    #[test]
    fn schema_two_plans_and_pre_native_results_remain_readable() {
        let old_plan = serde_json::json!({
            "schema_version": 2,
            "id": "run-plan_old",
            "config_digest": "sha256:config",
            "source": {"commit": "abc123", "worktree_fingerprint": "worktree-1"},
            "targets": [{
                "target": {"component": "api", "action": "test"},
                "intent": "check",
                "effects": ["read_only"],
                "runner": {"kind": "native", "operation": "jig.schema_check"},
                "input_digest": "sha256:input",
                "reasons": []
            }],
            "execution_layers": [[{"component": "api", "action": "test"}]],
            "effects": []
        });
        let plan: RunPlan = serde_json::from_value(old_plan).unwrap();
        assert_eq!(plan.schema_version, 2);
        assert!(plan.targets[0].prepared_native_input.is_none());
        assert!(matches!(
            &plan.targets[0].runner,
            ActionRunner::Native {
                configuration: None,
                ..
            }
        ));

        let old_result = serde_json::json!({
            "target": {"component": "api", "action": "test"},
            "status": "completed",
            "conclusion": "success",
            "config_digest": "sha256:config",
            "input_digest": "sha256:input",
            "findings": [],
            "evidence": []
        });
        let result: TargetRunResult = serde_json::from_value(old_result).unwrap();
        assert_eq!(result.finding_count, None);
        assert_eq!(result.valid_until_ms, None);
    }
}
