use super::{value_bool, value_str, value_u64};

pub(super) fn format_loop_tick_summary(value: &serde_json::Value) -> String {
    let ok = value_bool(value, "ok").unwrap_or(false);
    let workflow = value_str(value, "workflow").unwrap_or("<unknown>");
    let status = value_str(value, "status").unwrap_or("unknown");
    let mut lines = vec![
        format!("Loop tick: {}", if ok { status } else { "failed" }),
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
    [
        "Loop status:".into(),
        format!("  Workflows: {workflows}"),
        format!("  Leases: {leases}"),
        format!("  Attempts: {attempts} ({waiting} waiting)"),
        format!("  Needs attention: {exhausted} exhausted"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_loop_run_summary(value: &serde_json::Value) -> String {
    let ok = value_bool(value, "ok").unwrap_or(false);
    let status = value_str(value, "status").unwrap_or("unknown");
    let tick_count = value_u64(value, "tick_count").unwrap_or(0);
    let until = value_str(value, "until").unwrap_or("unknown");
    [
        format!("Loop run: {}", if ok { status } else { "failed" }),
        format!("  Until: {until}"),
        format!("  Ticks: {tick_count}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_loop_clear_attempt_summary(value: &serde_json::Value) -> String {
    let workflow = value_str(value, "workflow").unwrap_or("<unknown>");
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
