use std::fmt::Write as _;

use anyhow::{Result, anyhow};

use super::{concise_preview, status, value_bool, value_i64, value_str};

pub(super) fn format_work_start_plan_id(value: &serde_json::Value) -> Result<String> {
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

pub(super) fn format_work_status_summary(value: &serde_json::Value) -> String {
    status::format_work_summary(value)
}

pub(super) fn format_work_check_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let checks = value["checks"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let status = work_check_summary_status(checks);
    let skipped_checks = checks
        .iter()
        .filter(|check| {
            check["result"]["exit_status"].as_i64() == Some(0)
                && work_check_summary_harness_skip_output(check).is_some()
        })
        .count();
    let status_label = if matches!(status, WorkCheckSummaryStatus::Passed) {
        match (skipped_checks, checks.len()) {
            (0, _) => status.label(),
            (skipped, total) if skipped == total => "passed (all skipped)",
            _ => "passed (some skipped)",
        }
    } else {
        status.label()
    };
    let mut lines = vec![
        format!("Work check: {status_label}"),
        format!("  Plan: {plan_id}"),
        format!(
            "  Batch receipt: {}",
            value_str(value, "receipt_id").unwrap_or("none")
        ),
        format!("  Checks: {}", checks.len()),
    ];

    for check in checks {
        let tool = value_str(check, "tool").unwrap_or("<unknown>");
        let exit_status = check["result"]["exit_status"].as_i64();
        let exit_status_label = exit_status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "?".into());
        let receipt = value_str(check, "receipt_id").unwrap_or("none");
        let output_note = work_check_summary_output_note(check, exit_status)
            .map(|note| format!(", output: {note}"))
            .unwrap_or_default();
        lines.push(format!(
            "  - {tool}: exit {exit_status_label}, receipt {receipt}{output_note}"
        ));
    }

    if skipped_checks > 0 && skipped_checks == checks.len() {
        lines.push(
            "Note: all configured Cargo checks skipped because no root Cargo.toml exists; set explicit commands if this repo has Rust code outside a root workspace.".into(),
        );
    }

    match status {
        WorkCheckSummaryStatus::Passed => lines.push(format!(
            "Next step: scripts/jig work gates --plan-id {plan_id}"
        )),
        WorkCheckSummaryStatus::Failed => lines.push(format!(
            "Next step: inspect failing receipts, fix issues, then rerun scripts/jig work check --plan-id {plan_id}"
        )),
        WorkCheckSummaryStatus::Unknown => lines.push(format!(
            "Next step: inspect receipts with unknown exit status, then rerun scripts/jig work check --plan-id {plan_id}"
        )),
        WorkCheckSummaryStatus::NoChecksConfigured => lines.push(format!(
            "Next step: configure work checks or rerun scripts/jig work check --plan-id {plan_id} --tool <tool>"
        )),
    }
    lines.join("\n")
}

fn work_check_summary_output_note(
    check: &serde_json::Value,
    exit_status: Option<i64>,
) -> Option<String> {
    if exit_status == Some(0)
        && let Some(output) = work_check_summary_harness_skip_output(check)
    {
        return Some(concise_preview(output, 120));
    }

    let result = &check["result"];
    let stdout = value_str(result, "stdout").filter(|output| !output.trim().is_empty());
    let stderr = value_str(result, "stderr").filter(|output| !output.trim().is_empty());
    match exit_status {
        Some(0) => None,
        Some(_) | None => stderr.or(stdout).map(|output| concise_preview(output, 120)),
    }
}

fn work_check_summary_harness_skip_output(check: &serde_json::Value) -> Option<&str> {
    let result = &check["result"];
    value_str(result, "stdout")
        .filter(|output| !output.trim().is_empty())
        .filter(|output| work_check_summary_has_harness_skip(output))
}

fn work_check_summary_has_harness_skip(output: &str) -> bool {
    output.lines().any(|line| {
        line.trim_start()
            .starts_with(crate::CARGO_SKIP_OUTPUT_PREFIX)
    })
}

#[derive(Clone, Copy)]
enum WorkCheckSummaryStatus {
    Passed,
    Failed,
    Unknown,
    NoChecksConfigured,
}

impl WorkCheckSummaryStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
            Self::NoChecksConfigured => "no checks configured",
        }
    }
}

fn work_check_summary_status(checks: &[serde_json::Value]) -> WorkCheckSummaryStatus {
    if checks.is_empty() {
        return WorkCheckSummaryStatus::NoChecksConfigured;
    }

    let mut saw_unknown = false;
    for check in checks {
        match check["result"]["exit_status"].as_i64() {
            Some(0) => {}
            Some(_) => return WorkCheckSummaryStatus::Failed,
            None => saw_unknown = true,
        }
    }

    if saw_unknown {
        WorkCheckSummaryStatus::Unknown
    } else {
        WorkCheckSummaryStatus::Passed
    }
}

pub(super) fn format_work_gates_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let plan_state = value_str(value, "plan_state").unwrap_or("open");
    let overall = value_str(value, "overall").unwrap_or("unknown");
    let gates = value["gates"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut lines = vec![
        format!("Work gates: {overall}"),
        format_work_plan_line(plan_id, plan_state),
        format!("  Gates: {}", gates.len()),
    ];

    for gate in gates {
        let id = value_str(gate, "id").unwrap_or("<unknown>");
        let status = value_str(gate, "status").unwrap_or("unknown");
        let required = value_bool(gate, "required").unwrap_or(true);
        let required_label = if required { "required" } else { "optional" };
        let tool = value_str(gate, "tool")
            .map(|tool| format!(" ({tool})"))
            .or_else(|| value_str(gate, "skill").map(|skill| format!(" ({skill})")))
            .unwrap_or_default();
        let freshness = value_str(gate, "freshness")
            .map(|freshness| format!(", freshness {freshness}"))
            .unwrap_or_default();
        let mut line = format!("  - {id}: {status}{freshness}, {required_label}{tool}");
        if !matches!(status, "passed" | "missing")
            && let Some(reason) = value_str(gate, "freshness_reason")
        {
            let _ = write!(line, "; {reason}");
        }
        lines.push(line);
        if status != "missing"
            && let Some(diff) = value_str(gate, "diff_summary").filter(|diff| !diff.is_empty())
        {
            lines.push(format!("    receipt diff: {diff}"));
        }
        if status == "invalid_output"
            && let Some(parse_error) = value_str(gate, "parse_error")
        {
            lines.push(format!("    parse error: {parse_error}"));
        }
        let changed_paths = value_string_list(gate, "changed_paths");
        if status != "missing" && !changed_paths.is_empty() {
            lines.push(format!(
                "    changed paths covered: {}",
                changed_paths.join(", ")
            ));
        }
    }

    if overall == "passed" && plan_state == "open" {
        lines.push(format!(
            "Next step: scripts/jig work finish --plan-id {plan_id} --resolution <summary> --outcome success"
        ));
    } else if overall == "passed" {
        lines.push("Next step: none; plan is closed".into());
    } else {
        match gate_blocker_summary(value) {
            Some(blockers) => lines.push(format!("Blocked: {blockers}")),
            None => lines.push(format!(
                "Status: {overall}; no categorized blockers reported"
            )),
        }
        if plan_state == "open" {
            lines.push(format!(
                "Next step: scripts/jig work check --plan-id {plan_id}"
            ));
        } else {
            lines.push("Next step: start a new work plan for follow-up changes".into());
        }
    }

    lines.join("\n")
}

pub(super) fn format_work_evidence_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let plan_state = value_str(value, "plan_state").unwrap_or("open");
    let overall = value_str(value, "overall").unwrap_or("unknown");
    let latest = value["latest_passing_gates"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let gates = value["gates"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut lines = vec![
        format!("Work evidence: {overall}"),
        format_work_plan_line(plan_id, plan_state),
    ];

    if latest.is_empty() {
        lines.push("Latest gate evidence per gate: none".into());
    } else {
        lines.push("Latest gate evidence per gate:".into());
        for gate in latest {
            let tool = value_str(gate, "tool")
                .or_else(|| value_str(gate, "skill"))
                .unwrap_or("<unknown>");
            let gate_id = value_str(gate, "gate_id").unwrap_or("<unknown>");
            let receipt = value_str(gate, "freshness_receipt_id")
                .or_else(|| value_str(gate, "receipt_id"))
                .unwrap_or("none");
            let freshness = value_str(gate, "freshness").unwrap_or("unknown");
            let matches = value_bool(gate, "matches_current_worktree").unwrap_or(false);
            let matches_label = if matches { "yes" } else { "no" };
            lines.push(format!(
                "  - {tool}: {gate_id}, receipt {receipt}, matches current worktree {matches_label} ({freshness})"
            ));
            if let Some(reason) = value_str(gate, "freshness_reason") {
                lines.push(format!("    reason: {reason}"));
            }
            if let Some(diff) = value_str(gate, "diff_summary").filter(|diff| !diff.is_empty()) {
                lines.push(format!("    receipt diff: {diff}"));
            }
            let changed_paths = value_string_list(gate, "changed_paths");
            if !changed_paths.is_empty() {
                lines.push(format!(
                    "    changed paths covered: {}",
                    changed_paths.join(", ")
                ));
            }
        }
    }

    let unresolved = gates
        .iter()
        .filter(|gate| value_str(gate, "status") != Some("passed"))
        .collect::<Vec<_>>();
    if unresolved.is_empty() {
        lines.push("Unresolved gates: none".into());
    } else {
        lines.push("Unresolved gates:".into());
        for gate in unresolved {
            let id = value_str(gate, "id").unwrap_or("<unknown>");
            let status = value_str(gate, "status").unwrap_or("unknown");
            let reason =
                value_str(gate, "freshness_reason").unwrap_or("no receipt evidence for this gate");
            lines.push(format!("  - {id}: {status}; {reason}"));
        }
    }

    if overall == "passed" && plan_state == "open" {
        lines.push(format!(
            "Next step: scripts/jig work finish --plan-id {plan_id} --resolution <summary> --outcome success"
        ));
    } else if overall == "passed" {
        lines.push("Next step: none; plan is closed".into());
    } else if plan_state == "open" {
        lines.push(format!(
            "Next step: scripts/jig work check --plan-id {plan_id}"
        ));
    } else {
        lines.push("Next step: start a new work plan for follow-up changes".into());
    }

    lines.join("\n")
}

pub(super) fn format_work_review_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let status = value_str(value, "status").unwrap_or("unknown");
    let reviews = value["reviews"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut lines = vec![
        format!("Work review: {status}"),
        format_work_plan_line(plan_id, "open"),
        format!("  Review gates: {}", reviews.len()),
    ];

    for review in reviews {
        let gate_id = value_str(review, "gate_id").unwrap_or("<unknown>");
        let gate_status = value_str(review, "status").unwrap_or("unknown");
        let skill = value_str(review, "skill").unwrap_or("<unknown>");
        let finding_count = review["finding_count"].as_u64().unwrap_or(0);
        let actionable_count = review["actionable_count"].as_u64().unwrap_or(0);
        let retained_finding_count = review["retained_finding_count"]
            .as_u64()
            .unwrap_or(finding_count);
        let retained_actionable_count = review["retained_actionable_count"]
            .as_u64()
            .unwrap_or(actionable_count);
        let truncated = review["findings_truncated"].as_bool().unwrap_or(false)
            || review["actionable_findings_truncated"]
                .as_bool()
                .unwrap_or(false);
        let count_summary = if truncated {
            format!(
                "{actionable_count}/{finding_count} actionable, showing {retained_actionable_count}/{retained_finding_count}"
            )
        } else {
            format!("{actionable_count}/{finding_count} actionable")
        };
        lines.push(format!(
            "  - {gate_id}: {gate_status}, {count_summary} ({skill})"
        ));
    }

    if status == "passed" {
        lines.push(format!(
            "Next step: scripts/jig work check --plan-id {plan_id}"
        ));
    } else {
        lines.push(format!(
            "Next step: scripts/jig work refine --plan-id {plan_id}"
        ));
    }

    lines.join("\n")
}

pub(super) fn format_work_refine_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let status = value_str(value, "status").unwrap_or("unknown");
    let iterations = value["iterations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let remaining = value["remaining_actionable_findings"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let mut lines = vec![
        format!("Work refine: {status}"),
        format_work_plan_line(plan_id, "open"),
        format!("  Fixer iterations: {}", iterations.len()),
        format!("  Remaining actionable findings: {remaining}"),
    ];

    for iteration in iterations {
        let index = iteration["iteration"].as_u64().unwrap_or(0);
        let receipt = value_str(iteration, "receipt_id").unwrap_or("none");
        let finding_count = iteration["finding_count"].as_u64().unwrap_or(0);
        lines.push(format!(
            "  - iteration {index}: receipt {receipt}, findings addressed {finding_count}"
        ));
    }

    if status == "passed" {
        lines.push(format!(
            "Next step: scripts/jig work finish --plan-id {plan_id} --resolution <summary> --outcome success"
        ));
    } else {
        lines.push(format!(
            "Next step: inspect remaining findings, then rerun scripts/jig work refine --plan-id {plan_id}"
        ));
    }

    lines.join("\n")
}

fn format_work_plan_line(plan_id: &str, plan_state: &str) -> String {
    if plan_state == "closed" {
        format!("  Plan: {plan_id} (closed)")
    } else {
        format!("  Plan: {plan_id}")
    }
}

fn gate_blocker_summary(value: &serde_json::Value) -> Option<String> {
    let categories = [
        ("missing", "missing_required"),
        ("failed", "failed_required"),
        ("stale", "stale_required"),
        ("unknown", "unknown_required"),
        ("unsupported", "unsupported_required"),
    ];
    let blockers = categories
        .into_iter()
        .filter_map(|(label, key)| {
            let items = value_string_list(value, key);
            (!items.is_empty()).then(|| format!("{label} ({})", items.join(", ")))
        })
        .collect::<Vec<_>>();

    (!blockers.is_empty()).then(|| blockers.join("; "))
}

pub(super) fn format_work_receipts_summary(value: &serde_json::Value) -> String {
    let receipts = value["receipts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut lines = vec![
        "Work receipts:".into(),
        format!("  Showing: {}", receipts.len()),
    ];

    if receipts.is_empty() {
        lines.push("  No receipts matched the selected filters.".into());
        return lines.join("\n");
    }

    for receipt in receipts {
        let id = value_str(receipt, "id").unwrap_or("<unknown>");
        let tool = value_str(receipt, "tool_name").unwrap_or("<unknown>");
        let exit_status = value_i64(receipt, "exit_status")
            .map(|status| status.to_string())
            .unwrap_or_else(|| "?".into());
        let diff = value_str(receipt, "diff_summary").unwrap_or("unknown diff");
        lines.push(format!("  - {tool} ({id}): exit {exit_status}, {diff}"));

        let plan = value_str(receipt, "plan_id").unwrap_or("none");
        let session = value_str(receipt, "session_id").unwrap_or("none");
        lines.push(format!("    plan: {plan}; session: {session}"));

        if let Some(preview) = receipt_preview(receipt) {
            lines.push(format!("    output: {preview}"));
        }
    }

    lines.join("\n")
}

pub(super) fn format_work_start_summary(value: &serde_json::Value) -> String {
    let plan = &value["plan"];
    let session = &value["session"];
    let plan_id = value_str(plan, "plan_id").unwrap_or("<unknown>");
    let session_id = value_str(session, "session_id").unwrap_or("<unknown>");
    let body_path = value_str(plan, "body_path");
    let mut lines = vec![
        "Work start: opened".into(),
        format!("  Plan: {plan_id}"),
        format!("  Session: {session_id}"),
    ];
    if let Some(path) = body_path {
        lines.push(format!("  Body: {path}"));
    }
    lines.push(format!(
        "Next step: scripts/jig work check --plan-id {plan_id}"
    ));
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_work_goal_summary(value: &serde_json::Value) -> String {
    let plan = &value["plan"];
    let plan_id = value_str(plan, "plan_id").unwrap_or("<unknown>");
    let commands = &value["commands"];
    let mut lines = vec!["Work goal: opened".into(), format!("  Plan: {plan_id}")];
    if let Some(path) = value_str(plan, "body_path") {
        lines.push(format!("  Body: {path}"));
    }
    if let Some(status) = value_str(commands, "status") {
        lines.push(format!("  Status command: {status}"));
    }
    if let Some(check) = value_str(commands, "check") {
        lines.push(format!("Next step: {check}"));
    } else {
        lines.push(format!(
            "Next step: scripts/jig work check --plan-id {plan_id}"
        ));
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_work_append_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let receipt_id = value_str(value, "receipt_id").unwrap_or("none");
    [
        "Work append: recorded".into(),
        format!("  Plan: {plan_id}"),
        format!("  Receipt: {receipt_id}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_work_decide_summary(value: &serde_json::Value) -> String {
    let decision_id = value_str(value, "decision_id").unwrap_or("<unknown>");
    let receipt_id = value_str(value, "receipt_id").unwrap_or("none");
    [
        "Work decide: recorded".into(),
        format!("  Decision: {decision_id}"),
        format!("  Receipt: {receipt_id}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_work_finish_summary(value: &serde_json::Value) -> String {
    let plan = &value["plan"];
    let session = &value["session"];
    let plan_id = value_str(plan, "plan_id").unwrap_or("<unknown>");
    let session_id = value_str(session, "session_id").unwrap_or("none");
    [
        "Work finish: closed".into(),
        format!("  Plan: {plan_id}"),
        format!("  Session: {session_id}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

fn receipt_preview(receipt: &serde_json::Value) -> Option<String> {
    value_str(receipt, "stderr_preview")
        .filter(|preview| !preview.trim().is_empty())
        .or_else(|| {
            value_str(receipt, "stdout_preview").filter(|preview| !preview.trim().is_empty())
        })
        .map(|preview| concise_preview(preview, 180))
}

fn value_string_list(value: &serde_json::Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}
