use anyhow::Result;
use serde_json::{Value, json};

use crate::context::{RepoContext, WorkRefinementConfig, WorkReviewGate};

pub(super) fn review_prompt(
    ctx: &RepoContext,
    plan_id: &str,
    gate: &WorkReviewGate,
) -> Result<String> {
    let plan_path = crate::state::plan_body_path(ctx, plan_id)?;
    Ok(format!(
        "Apply the Codex review skill `{skill}` as a structured Jig work review gate.\n\
         Review scope: {scope}.\n\
         Work plan id: {plan_id}.\n\
         Work plan path: {plan_path}.\n\
         Gate id: {gate_id}.\n\
         Report all concrete findings, but the gate fails only for findings at or above `{threshold}`.\n\
         Return only JSON matching the provided schema. Do not include markdown or prose outside JSON.",
        skill = gate.skill.as_str(),
        scope = gate.scope.as_str(),
        plan_path = plan_path.display(),
        gate_id = gate.id,
        threshold = gate.threshold,
    ))
}

pub(super) fn refine_prompt(
    plan_id: &str,
    iteration: usize,
    gates: &[WorkReviewGate],
    refinement: Option<&WorkRefinementConfig>,
    findings: &[Value],
) -> String {
    let gate_list = gates
        .iter()
        .map(|gate| format!("{} ({})", gate.id, gate.skill))
        .collect::<Vec<_>>()
        .join(", ");
    let refinement_line = refinement.map_or_else(
        || "Refinement profile: default.".to_string(),
        |refinement| {
            format!(
                "Refinement profile: {}{}{}.",
                refinement.id,
                refinement
                    .skill
                    .as_deref()
                    .map(|skill| format!(", skill {skill}"))
                    .unwrap_or_default(),
                refinement
                    .mode
                    .as_deref()
                    .map(|mode| format!(", mode {mode}"))
                    .unwrap_or_default(),
            )
        },
    );
    format!(
        "You are running Jig work refinement for plan `{plan_id}`, iteration {iteration}.\n\
         Address the actionable review findings below by editing the repository directly.\n\
         Keep changes tightly scoped to the findings, preserve existing behavior, and do not run git.\n\
         Review gates: {gate_list}.\n\
         {refinement_line}\n\
         Findings JSON:\n{findings}\n\
         After edits, stop. Jig will rerun review gates and check gates.",
        findings = serde_json::to_string_pretty(findings)
            .expect("review findings are already valid JSON values")
    )
}

pub(super) fn review_output_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "findings"],
        "properties": {
            "summary": { "type": "string" },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["severity", "issue", "recommendation"],
                    "properties": {
                        "severity": {
                            "type": "string",
                            "enum": ["critical", "warning", "suggestion", "high", "medium", "low"]
                        },
                        "path": { "type": ["string", "null"] },
                        "line": { "type": ["integer", "null"], "minimum": 1 },
                        "issue": { "type": "string" },
                        "evidence": { "type": "string" },
                        "recommendation": { "type": "string" }
                    }
                }
            }
        }
    })
}
