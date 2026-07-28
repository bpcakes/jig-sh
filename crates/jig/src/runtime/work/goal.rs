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
            default_checkpoints()
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

## Validation Loop

{validations}

## Constraints

{constraints}

## Checkpoints

{checkpoints}

## Configured Jig Gates

{configured_gates}

## Progress Log

- Goal harness created. Keep this section short and append dated checkpoints, failed attempts, and validation evidence.

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
        "/goal Complete the objective in {body_path} without stopping until this verifiable stopping condition is met: {success}. Use {body_path} as the durable progress log, keep changes scoped to the stated constraints, run the validation loop recorded there, inspect gates with `scripts/jig work gates --plan-id {plan_id}`, and stop if blocked by missing product guidance, unsafe permissions, or a validation result that cannot be improved without changing the goal.",
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

fn default_checkpoints() -> Vec<String> {
    [
        "Read the relevant AGENTS.md files and repo guidance.",
        "Establish the baseline validation result before risky edits.",
        "Make scoped changes and record each meaningful attempt.",
        "Run the validation loop and inspect gate status.",
        "Finish only after the stopping condition is met.",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
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
