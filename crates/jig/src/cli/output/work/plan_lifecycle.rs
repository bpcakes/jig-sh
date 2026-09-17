use super::{concise_preview, format_work_plan_line, value_str};

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
