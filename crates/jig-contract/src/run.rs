use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ActionEffect, ActionIntent, ActionRunner, ProfileId, ResultParser, TargetId};

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
            inputs: Vec::new(),
            depends_on: Vec::new(),
            timeout_seconds: None,
            result_parser: ResultParser::default(),
            input_digest: input_digest.into(),
            reasons: Vec::new(),
        }
    }
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
    pub const SCHEMA_VERSION: u32 = 1;

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
        }
    }
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
                "schema_version": 1,
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
}
