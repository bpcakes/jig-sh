use super::{value_bool, value_str, value_u64};

pub(super) fn format_loop_tick_summary(value: &serde_json::Value) -> String {
    let workflow = value["workflow"]["id"]
        .as_str()
        .or_else(|| value_str(value, "workflow"))
        .unwrap_or("<unknown>");
    let status = value_str(value, "status").unwrap_or("unknown");
    let mut lines = vec![
        format!("Loop tick: {status}"),
        format!("  Workflow: {workflow}"),
    ];
    if let Some(idle) = value_bool(value, "idle") {
        lines.push(format!("  Idle: {}", if idle { "yes" } else { "no" }));
    }
    if let Some(warning) = value_str(value, "release_warning") {
        lines.push(format!("  Warning: {warning}"));
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_loop_dispatch_summary(value: &serde_json::Value) -> String {
    let status = value_str(value, "status").unwrap_or("unknown");
    let due = value_u64(value, "due_count").unwrap_or(0);
    let executed = value_u64(value, "executed_count").unwrap_or(0);
    let deferred = value_u64(value, "deferred_count").unwrap_or(0);
    let skipped = value_u64(value, "skipped_count").unwrap_or(0);
    let failed = value_u64(value, "failed_count").unwrap_or(0);
    let scheduled_attention = value_u64(value, "needs_attention_count").unwrap_or(0);
    let exhausted_attempts = value_u64(value, "exhausted_attempt_count").unwrap_or(0);
    let state_errors = value_u64(value, "state_error_count").unwrap_or(0);
    [
        format!("Loop dispatch: {status}"),
        format!("  Due: {due}"),
        format!("  Executed: {executed} ({failed} failed)"),
        format!("  Deferred: {deferred}"),
        format!("  Skipped: {skipped}"),
        format!("  State errors: {state_errors}"),
        format!(
            "  Needs attention: {scheduled_attention} scheduled, {exhausted_attempts} exhausted attempts"
        ),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_loop_status_summary(value: &serde_json::Value) -> String {
    let workflows = value["workflows"].as_array().map(Vec::len).unwrap_or(0);
    let leases = value["leases"].as_array().map(Vec::len).unwrap_or(0);
    let attempts = value["attempts"].as_array().map(Vec::len).unwrap_or(0);
    let waiting = value["waiting_attempts"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let exhausted = value["needs_attention"]["exhausted_attempts"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let scheduled = value["scheduled_occurrences"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let scheduled_attention = value["needs_attention"]["scheduled_occurrences"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    [
        "Loop status:".into(),
        format!("  Workflows: {workflows}"),
        format!("  Leases: {leases}"),
        format!("  Attempts: {attempts} ({waiting} waiting)"),
        format!("  Scheduled runs: {scheduled}"),
        format!("  Needs attention: {exhausted} exhausted, {scheduled_attention} scheduled"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_loop_run_summary(value: &serde_json::Value) -> String {
    let status = value_str(value, "status").unwrap_or("unknown");
    let tick_count = value_u64(value, "tick_count").unwrap_or(0);
    let until = value_str(value, "until").unwrap_or("unknown");
    [
        format!("Loop run: {status}"),
        format!("  Until: {until}"),
        format!("  Ticks: {tick_count}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_loop_clear_attempt_summary(value: &serde_json::Value) -> String {
    let workflow = value_str(value, "workflow_id")
        .or_else(|| value["workflow"]["id"].as_str())
        .or_else(|| value_str(value, "workflow"))
        .unwrap_or("<unknown>");
    let item_key = value_str(value, "item_key").unwrap_or("<unknown>");
    let cleared = value_bool(value, "cleared").unwrap_or(false);
    [
        format!(
            "Loop clear-attempt: {}",
            if cleared { "cleared" } else { "unchanged" }
        ),
        format!("  Workflow: {workflow}"),
        format!("  Item: {item_key}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_loop_acknowledge_occurrence_summary(value: &serde_json::Value) -> String {
    let occurrence_id = value_str(value, "occurrence_id").unwrap_or("<unknown>");
    let changed = value_bool(value, "changed").unwrap_or(false);
    [
        format!(
            "Loop acknowledge-occurrence: {}",
            if changed {
                "acknowledged"
            } else {
                "already acknowledged"
            }
        ),
        format!("  Occurrence: {occurrence_id}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn dispatch_summary_reports_deferred_occurrences() {
        let summary = super::format_loop_dispatch_summary(&json!({
            "status": "deferred",
            "due_count": 1,
            "executed_count": 0,
            "deferred_count": 1,
            "skipped_count": 0,
            "failed_count": 0,
            "needs_attention_count": 0,
            "exhausted_attempt_count": 0,
            "state_error_count": 0,
        }));

        assert!(summary.contains("  Deferred: 1"), "{summary}");
        assert!(summary.contains("  Skipped: 0"), "{summary}");
        assert!(
            summary.contains("Needs attention: 0 scheduled, 0 exhausted attempts"),
            "{summary}"
        );
    }

    #[test]
    fn unsuccessful_attention_summaries_preserve_their_status() {
        let tick = super::format_loop_tick_summary(&json!({
            "ok": false,
            "workflow": {"id": "noop-status"},
            "status": "needs_attention",
            "idle": false,
        }));
        let run = super::format_loop_run_summary(&json!({
            "ok": false,
            "status": "needs_attention",
            "until": "idle",
            "tick_count": 1,
        }));

        assert!(tick.starts_with("Loop tick: needs_attention"), "{tick}");
        assert!(tick.contains("Workflow: noop-status"), "{tick}");
        assert!(run.starts_with("Loop run: needs_attention"), "{run}");
    }

    #[test]
    fn clear_attempt_summary_accepts_the_compatible_workflow_object() {
        let summary = super::format_loop_clear_attempt_summary(&json!({
            "workflow": {"id": "removed-workflow", "removed": true},
            "workflow_id": "removed-workflow",
            "item_key": "pr-7",
            "cleared": true,
        }));

        assert!(summary.contains("Workflow: removed-workflow"), "{summary}");
        assert!(summary.contains("Item: pr-7"), "{summary}");
    }
}
