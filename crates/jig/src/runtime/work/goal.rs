use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::command::WorkGoalRequest;
use crate::context::{RepoContext, WorkGate};
use crate::state::PlanOpenRequest;

use super::start;

struct GoalHarness {
    objective: String,
    success: String,
    validations: Vec<String>,
    constraints: Vec<String>,
    checkpoints: Vec<String>,
    title: String,
    notes: Option<String>,
}

impl GoalHarness {
    fn from_request(request: WorkGoalRequest) -> Result<Self> {
        let objective = trimmed_required_text("--objective", &request.objective)?;
        let success = trimmed_required_text("--success", &request.success)?;
        let validations = clean_provided_items("--validation", &request.validations)?;
        if validations.is_empty() {
            bail!("At least one non-empty --validation is required for a goal harness.");
        }
        let constraints = clean_provided_items("--constraint", &request.constraints)?;

        let checkpoints = if request.checkpoints.is_empty() {
            vec![single_line_text(&success)]
        } else {
            clean_provided_items("--checkpoint", &request.checkpoints)?
        };
        let title = request
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| goal_title(&single_line_text(&objective)));
        let notes = request
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
            .map(str::to_string);

        Ok(Self {
            objective,
            success,
            validations,
            constraints,
            checkpoints,
            title,
            notes,
        })
    }
}

pub(super) fn goal(ctx: &RepoContext, request: WorkGoalRequest) -> Result<Value> {
    let goal = GoalHarness::from_request(request)?;

    let body = goal_body(ctx, &goal);
    let output = start(
        ctx,
        PlanOpenRequest {
            title: goal.title.clone(),
            body: Some(body),
            body_file: None,
            base: None,
        },
    )?;

    let plan_id = output["plan"]["plan_id"]
        .as_str()
        .ok_or_else(|| anyhow!("Goal harness failed to create a plan id"))?;
    let body_path = output["plan"]["body_path"]
        .as_str()
        .ok_or_else(|| anyhow!("Goal harness failed to create a plan body path"))?;
    let goal_prompt = goal_prompt(plan_id, body_path, &goal);

    Ok(json!({
        "ok": true,
        "session": output["session"],
        "plan": output["plan"],
        "goal_prompt": goal_prompt,
        "commands": {
            "status": "scripts/jig work status",
            "check": format!("scripts/jig work check --plan-id {plan_id}"),
            "gates": format!("scripts/jig work gates --plan-id {plan_id}"),
            "finish": format!("scripts/jig work finish --plan-id {plan_id}")
        }
    }))
}

fn goal_title(objective: &str) -> String {
    const MAX_TITLE_CHARS: usize = 80;
    const ELLIPSIS: &str = "...";
    let objective = objective.trim();
    if objective.chars().count() <= MAX_TITLE_CHARS {
        return objective.to_string();
    }

    let ellipsis_chars = ELLIPSIS.chars().count();
    let mut title = objective
        .chars()
        .take(MAX_TITLE_CHARS.saturating_sub(ellipsis_chars))
        .collect::<String>();
    title.push_str(ELLIPSIS);
    title
}

fn goal_body(ctx: &RepoContext, goal: &GoalHarness) -> String {
    let configured_gates = ctx
        .work_gates()
        .into_iter()
        .map(|gate| match gate {
            WorkGate::Check(gate) => format!("{}: check ({})", gate.id, gate.tool),
            WorkGate::Evidence(gate) => match gate.selector {
                crate::context::WorkEvidenceSelector::Target(target) => {
                    format!("{}: evidence (target {target})", gate.id)
                }
                crate::context::WorkEvidenceSelector::Profile(profile) => {
                    format!("{}: evidence (profile {profile})", gate.id)
                }
            },
            WorkGate::CodexReview(gate) => {
                format!("{}: codex_review ({})", gate.id, gate.skill)
            }
            WorkGate::Unsupported(gate) => format!("{}: {}", gate.id, gate.kind),
        })
        .collect::<Vec<_>>();

    format!(
        r"# Goal Harness

## Objective

{objective}

## Verifiable Stopping Condition

{success}

## Validation

{validations}

## Constraints

{constraints}

## Checkpoints

{checkpoints}

## Configured Jig Gates

{configured_gates}

## Progress Log

- Created; outcome unverified. Record completed work, material decisions, current evidence, the next action, and any blocker here. On resume, reconcile this checkpoint with the actual worktree and evidence before continuing.

## Execution

Carry the authorized objective through acceptance and required checks. A research or planning objective authorizes that deliverable only. Preserve explicit approval checkpoints; ordinary progress checkpoints do not require renewed permission. Resolve routine reversible choices within scope and continue independent authorized work while a material question is pending. Pause dependent work when a user decision or required authority is missing, and record the blocker and next action.

Stop and report a blocker if acceptance or required checks cannot be satisfied without changing the objective, success condition, constraints, or configured gates, or would require unsafe permissions. Record the evidence and the decision or authority needed to proceed; do not weaken checks or redefine success.

Use qualifying current evidence for the supplied validations and configured gates. Repeat or broaden checks only for changed inputs, unresolved concerns, or repository requirements. Finish when acceptance and required checks are satisfied.

## Notes

{notes}
",
        objective = goal.objective.as_str(),
        success = goal.success.as_str(),
        validations = markdown_bullets(&goal.validations, "No validation command specified."),
        constraints = markdown_bullets(&goal.constraints, "No additional constraints specified."),
        checkpoints = markdown_checkboxes(&goal.checkpoints),
        configured_gates = markdown_bullets(&configured_gates, "No work gates configured."),
        notes = goal.notes.as_deref().unwrap_or("No extra notes.")
    )
}

fn goal_prompt(plan_id: &str, body_path: &str, goal: &GoalHarness) -> String {
    format!(
        "/goal Complete the authorized objective in {body_path}. Success: {success}. Follow its constraints, explicit approval checkpoints, and execution guidance; a planning objective remains planning. Continue through acceptance and required checks, using qualifying current evidence. Keep restart state and evidence in {body_path}; reconcile them with the worktree on resume. Resolve routine choices within scope and continue independent authorized work while questions are pending. Pause dependent work for a missing user decision or required authority. Stop and report a blocker if acceptance or required checks cannot be satisfied without changing the objective, success condition, constraints, or configured gates, or would require unsafe permissions. Record the evidence and the decision or authority needed to proceed; do not weaken checks or redefine success. Inspect gates with `scripts/jig work gates --plan-id {plan_id}` and finish only when acceptance and required checks are satisfied.",
        success = single_line_text(&goal.success),
    )
}

fn trimmed_required_text(flag: &str, value: &str) -> Result<String> {
    let text = value.trim();
    if text.is_empty() {
        bail!("{flag} cannot be empty.");
    }
    Ok(text.to_string())
}

fn single_line_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_provided_items(flag: &str, items: &[String]) -> Result<Vec<String>> {
    let cleaned = clean_items(items);
    if cleaned.len() != items.len() {
        bail!("{flag} values cannot be empty.");
    }
    Ok(cleaned)
}

fn markdown_bullets(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        return format!("- {empty}");
    }

    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn markdown_checkboxes(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- [ ] {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn clean_items(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}
