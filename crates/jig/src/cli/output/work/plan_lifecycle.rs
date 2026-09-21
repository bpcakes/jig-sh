use anyhow::{Result, anyhow};

use super::{concise_preview, format_work_plan_line, value_str};

pub(in crate::cli::output) fn format_work_start_plan_id(
    value: &serde_json::Value,
) -> Result<String> {
    let plan = value
        .get("plan")
        .ok_or_else(|| anyhow!("work start output did not include plan"))?;
    if !plan.is_object() {
        anyhow::bail!("work start output plan was not an object");
    }

    plan.get("plan_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("work start output did not include plan.plan_id"))
}

pub(super) fn append_plan_lifecycle(
    lines: &mut Vec<String>,
    value: &serde_json::Value,
    plan_id: &str,
    plan_state: &str,
) {
    let retirement = &value["plan_retirement"];
    let Some(disposition) = value_str(retirement, "disposition") else {
        lines.push(format_work_plan_line(plan_id, plan_state));
        return;
    };

    lines.push(format!("  Plan: {plan_id} (retired: {disposition})"));
    if let Some(reason) = value_str(retirement, "reason") {
        lines.push(format!(
            "  Retirement reason: {}",
            concise_preview(reason, 180)
        ));
    }
    if let Some(superseded_by) = value_str(retirement, "superseded_by") {
        lines.push(format!("  Superseded by: {superseded_by}"));
    }
}
