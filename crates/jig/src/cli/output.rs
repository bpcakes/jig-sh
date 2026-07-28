use std::fmt::Write as _;
use std::io::Write;

use anyhow::{Result, anyhow};

use crate::{doctor, info};

mod dev;
mod status;

pub(super) enum HumanOutput {
    Doctor,
    Info,
    Status,
    VaultRun,
    VaultGeneric,
    AgentDoctor,
    AgentBootstrap,
    WorkCheck,
    WorkGates,
    WorkEvidence,
    WorkReview,
    WorkRefine,
    WorkStart,
    WorkStartPlanId,
    WorkGoal,
    WorkAppend,
    WorkDecide,
    WorkFinish,
    WorkReceipts,
    WorkStatus,
    ToolExecution,
    AgentMapGenerate,
    MigrationAdd,
    LoopTick,
    LoopStatus,
    LoopRun,
    LoopClearAttempt,
    StateSummary,
    StateArchive,
    Dev,
    DevStatus,
    DevStop,
    Proxy,
}

pub(super) fn emit(
    json_output: bool,
    human_output: HumanOutput,
    value: &serde_json::Value,
) -> Result<()> {
    if json_output {
        return print_json(value);
    }
    print_text(&render_human(human_output, value)?)
}

fn render_human(human_output: HumanOutput, value: &serde_json::Value) -> Result<String> {
    Ok(match human_output {
        HumanOutput::Doctor => doctor::format_summary(value),
        HumanOutput::Info => info::format_summary(value),
        HumanOutput::Status => status::format_summary(value),
        HumanOutput::VaultRun => format_vault_run_summary(value),
        HumanOutput::VaultGeneric => format_vault_generic_summary(value),
        HumanOutput::AgentDoctor => format_agent_doctor_summary(value),
        HumanOutput::AgentBootstrap => format_agent_bootstrap_summary(value),
        HumanOutput::WorkCheck => format_work_check_summary(value),
        HumanOutput::WorkGates => format_work_gates_summary(value),
        HumanOutput::WorkEvidence => format_work_evidence_summary(value),
        HumanOutput::WorkReview => format_work_review_summary(value),
        HumanOutput::WorkRefine => format_work_refine_summary(value),
        HumanOutput::WorkStart => format_work_start_summary(value),
        HumanOutput::WorkStartPlanId => format_work_start_plan_id(value)?,
        HumanOutput::WorkGoal => format_work_goal_summary(value),
        HumanOutput::WorkAppend => format_work_append_summary(value),
        HumanOutput::WorkDecide => format_work_decide_summary(value),
        HumanOutput::WorkFinish => format_work_finish_summary(value),
        HumanOutput::WorkReceipts => format_work_receipts_summary(value),
        HumanOutput::WorkStatus => format_work_status_summary(value),
        HumanOutput::ToolExecution => format_tool_execution_summary(value),
        HumanOutput::AgentMapGenerate => format_agent_map_generate_summary(value),
        HumanOutput::MigrationAdd => format_migration_add_summary(value),
        HumanOutput::LoopTick => format_loop_tick_summary(value),
        HumanOutput::LoopStatus => format_loop_status_summary(value),
        HumanOutput::LoopRun => format_loop_run_summary(value),
        HumanOutput::LoopClearAttempt => format_loop_clear_attempt_summary(value),
        HumanOutput::StateSummary => format_state_summary(value),
        HumanOutput::StateArchive => format_state_archive_summary(value),
        HumanOutput::Dev => format_dev_summary(value),
        HumanOutput::DevStatus => format_dev_status_summary(value),
        HumanOutput::DevStop => format_dev_stop_summary(value),
        HumanOutput::Proxy => format_proxy_summary(value),
    })
}

pub(super) fn print_json(value: &serde_json::Value) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    handle.write_all(b"\n")?;
    // `jig vault run` may return a structured non-zero child status after
    // printing, so flush before unwinding through main.
    handle.flush()?;
    Ok(())
}

fn print_text(text: &str) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(text.as_bytes())?;
    handle.write_all(b"\n")?;
    Ok(())
}

pub(super) fn format_vault_run_summary(value: &serde_json::Value) -> String {
    let result = &value["result"];
    let exit_status = value_i64(result, "exit_status")
        .map(|status| status.to_string())
        .unwrap_or_else(|| "?".into());
    let mut lines = vec![format!("Vault run: exit {exit_status}")];
    if let Some(signal) = value_i64(result, "exit_signal") {
        lines.push(format!("  Signal: {signal}"));
    }
    let mut truncated = false;
    if let Some(stdout) = value_str(result, "stdout").filter(|text| !text.is_empty()) {
        let (preview, was_truncated) = concise_preview_with_truncation(stdout, 240);
        truncated |= was_truncated;
        lines.push(format!("  stdout: {preview}"));
    }
    if let Some(stderr) = value_str(result, "stderr").filter(|text| !text.is_empty()) {
        let (preview, was_truncated) = concise_preview_with_truncation(stderr, 240);
        truncated |= was_truncated;
        lines.push(format!("  stderr: {preview}"));
    }
    if truncated {
        lines.push("  Output truncated; rerun with --json for full output.".into());
    }
    lines.join("\n")
}

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

pub(super) fn format_agent_doctor_summary(value: &serde_json::Value) -> String {
    let ready = value_bool(value, "ok").unwrap_or(false);
    let codex = &value["codex"];
    let codex_required = value_bool(codex, "required").unwrap_or(false);
    let codex_line = if codex_required {
        let codex_available = codex
            .get("available")
            .and_then(serde_json::Value::as_bool)
            .map(|available| {
                if available {
                    "available"
                } else {
                    "unavailable"
                }
            })
            .unwrap_or("unknown");
        format!("Codex: required ({codex_available})")
    } else {
        "Codex: not required (probe skipped)".into()
    };
    let mut lines = vec![
        format!(
            "Agent tooling: {}",
            if ready { "ready" } else { "needs setup" }
        ),
        codex_line,
    ];

    let marketplaces = value["marketplaces"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if marketplaces.is_empty() {
        lines.push("Marketplaces: none configured".into());
    } else {
        lines.push("Marketplaces:".into());
        for marketplace in marketplaces {
            let id = value_str(marketplace, "id").unwrap_or("<unknown>");
            let source = value_str(marketplace, "source").unwrap_or("<unknown>");
            let registered = value_bool(marketplace, "registered").unwrap_or(false);
            let configured = value_str(marketplace, "configured_source");
            let detail = match (registered, configured) {
                (true, _) => format!("registered ({source})"),
                (false, Some(configured)) => {
                    format!("not registered; repo config expects {source}, Codex has {configured}")
                }
                (false, None) => format!("missing registration for {source}"),
            };
            lines.push(format!("  - {id}: {detail}"));
        }
    }

    let next_steps = value["next_steps"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if next_steps.is_empty() {
        lines.push("Next steps: none".into());
    } else {
        lines.push("Next steps:".into());
        for step in next_steps {
            if let Some(step) = step.as_str() {
                lines.push(format!("  - {step}"));
            }
        }
    }

    lines.join("\n")
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
    if exit_status == Some(0) {
        if let Some(output) = work_check_summary_harness_skip_output(check) {
            return Some(concise_preview(output, 120));
        }
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
        if !matches!(status, "passed" | "missing") {
            if let Some(reason) = value_str(gate, "freshness_reason") {
                let _ = write!(line, "; {reason}");
            }
        }
        lines.push(line);
        if status != "missing" {
            if let Some(diff) = value_str(gate, "diff_summary").filter(|diff| !diff.is_empty()) {
                lines.push(format!("    receipt diff: {diff}"));
            }
        }
        if status == "invalid_output" {
            if let Some(parse_error) = value_str(gate, "parse_error") {
                lines.push(format!("    parse error: {parse_error}"));
            }
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

pub(super) fn format_agent_bootstrap_summary(value: &serde_json::Value) -> String {
    let ok = value_bool(value, "ok").unwrap_or(false);
    let marketplace = value_str(value, "marketplace_source").unwrap_or("<unknown>");
    let mut lines = vec![
        format!("Agent bootstrap: {}", if ok { "ok" } else { "failed" }),
        format!("  Marketplace: {marketplace}"),
    ];
    if let Some(stdout) = value_str(value, "stdout").filter(|text| !text.trim().is_empty()) {
        lines.push(format!("  stdout: {}", concise_preview(stdout, 160)));
    }
    if let Some(stderr) = value_str(value, "stderr").filter(|text| !text.trim().is_empty()) {
        lines.push(format!("  stderr: {}", concise_preview(stderr, 160)));
    }
    if ok {
        lines.push("Next step: scripts/jig agent doctor".into());
    } else {
        lines.push(
            "Next step: inspect the marketplace source, then rerun scripts/jig agent bootstrap"
                .into(),
        );
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_vault_generic_summary(value: &serde_json::Value) -> String {
    let command = value_str(value, "command").unwrap_or("vault");
    let ok = value_bool(value, "ok").unwrap_or(false);
    let scope = value_str(value, "vault_scope").unwrap_or("unknown");
    let home = value_str(value, "vault_home").unwrap_or("<unknown>");
    let mut lines = vec![
        format!("{command}: {}", if ok { "ok" } else { "failed" }),
        format!("  Scope: {scope}"),
        format!("  Home: {home}"),
    ];
    match command {
        "vault init" => {
            let created = value_bool(value, "created").unwrap_or(false);
            lines.push(format!("  Created: {}", if created { "yes" } else { "no" }));
        }
        "vault status" => {
            let exists = value_bool(value, "exists")
                .or_else(|| value_bool(value, "vault_file_exists"))
                .unwrap_or(false);
            lines.push(format!("  Exists: {}", if exists { "yes" } else { "no" }));
        }
        "vault secret list" => {
            let secrets = value["secrets"].as_array().map(Vec::len).unwrap_or(0);
            lines.push(format!("  Secrets: {secrets}"));
            if let Some(items) = value["secrets"].as_array() {
                for secret in items.iter().take(20) {
                    let name = value_str(secret, "name").unwrap_or("<unknown>");
                    lines.push(format!("  - {name}"));
                }
                if items.len() > 20 {
                    lines.push(format!("  (and {} more)", items.len() - 20));
                }
            }
        }
        "vault secret set" => {
            if let Some(name) = value_str(value, "name") {
                lines.push(format!("  Name: {name}"));
            }
        }
        "vault secret remove" => {
            if let Some(name) = value_str(value, "name") {
                lines.push(format!("  Name: {name}"));
            }
            if let Some(removed) = value_bool(value, "removed") {
                lines.push(format!("  Removed: {}", if removed { "yes" } else { "no" }));
            }
        }
        "vault audit verify" => {
            let events = value_u64(value, "event_count").unwrap_or(0);
            lines.push(format!("  Events: {events}"));
            if let Some(torn) = value_u64(value, "torn_tail_bytes") {
                if torn > 0 {
                    lines.push(format!("  Torn tail bytes: {torn}"));
                }
            }
        }
        _ => {}
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_tool_execution_summary(value: &serde_json::Value) -> String {
    let ok = value_bool(value, "ok").unwrap_or(false);
    let tool = value_str(value, "tool")
        .or_else(|| value_str(value, "command"))
        .unwrap_or("check");
    let exit_status = value["result"]["exit_status"]
        .as_i64()
        .map(|status| status.to_string())
        .unwrap_or_else(|| if ok { "0".into() } else { "?".into() });
    let mut lines = vec![
        format!("{tool}: {}", if ok { "ok" } else { "failed" }),
        format!("  Exit: {exit_status}"),
    ];
    if let Some(receipt) = value_str(value, "receipt_id") {
        lines.push(format!("  Receipt: {receipt}"));
    }
    append_policy_check_details(&mut lines, value);
    if let Some(preview) = tool_output_preview(value) {
        lines.push(format!("  Output: {preview}"));
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

fn append_policy_check_details(lines: &mut Vec<String>, value: &serde_json::Value) {
    if let Some(count) = value_u64(value, "guide_count") {
        lines.push(format!("  Guides: {count}"));
    }
    if let Some(agents) = value["agents"].as_array() {
        lines.push(format!("  Agents: {}", agents.len()));
    }
    if let Some(missing) = value["missing_agents"].as_array() {
        if !missing.is_empty() {
            lines.push(format!("  Missing agents: {}", missing.len()));
        }
    }
    if let Some(missing) = value["missing_sections"].as_array() {
        if !missing.is_empty() {
            lines.push(format!("  Missing sections: {}", missing.len()));
        }
    }
    if let Some(violations) = value["violations"].as_array() {
        if !violations.is_empty() {
            lines.push(format!("  Violations: {}", violations.len()));
            for violation in violations.iter().take(5) {
                let preview = if let Some(text) = violation.as_str() {
                    text.to_string()
                } else {
                    concise_preview(&violation.to_string(), 120)
                };
                lines.push(format!("  - {preview}"));
            }
        }
    }
    if let Some(errors) = value["errors"].as_array() {
        if !errors.is_empty() {
            lines.push(format!("  Errors: {}", errors.len()));
        }
    }
    if let Some(count) = value_u64(value, "non_test_count") {
        lines.push(format!("  Non-test unchecked queries: {count}"));
    }
}

fn tool_output_preview(value: &serde_json::Value) -> Option<String> {
    let result = &value["result"];
    value_str(result, "stderr")
        .filter(|text| !text.trim().is_empty())
        .or_else(|| value_str(result, "stdout").filter(|text| !text.trim().is_empty()))
        .map(|text| concise_preview(text, 180))
}

pub(super) fn format_agent_map_generate_summary(value: &serde_json::Value) -> String {
    let ok = value_bool(value, "ok").unwrap_or(false);
    let path = value_str(value, "path").unwrap_or("<unknown>");
    [
        format!("Agent map generate: {}", if ok { "ok" } else { "failed" }),
        format!("  Path: {path}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_migration_add_summary(value: &serde_json::Value) -> String {
    let ok = value_bool(value, "ok").unwrap_or(false);
    let name = value_str(value, "name").unwrap_or("<unknown>");
    let mut lines = vec![
        format!("Migration add: {}", if ok { "ok" } else { "failed" }),
        format!("  Name: {name}"),
    ];
    if let Some(receipt) = value_str(value, "receipt_id") {
        lines.push(format!("  Receipt: {receipt}"));
    }
    if let Some(preview) = tool_output_preview(value) {
        lines.push(format!("  Output: {preview}"));
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

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

pub(super) fn format_state_summary(value: &serde_json::Value) -> String {
    // Same payload shape as work status.
    format_work_status_summary(value).replacen("Work status:", "State summary:", 1)
}

pub(super) fn format_state_archive_summary(value: &serde_json::Value) -> String {
    let dry_run = value_bool(value, "dry_run").unwrap_or(false);
    let archived = value_u64(value, "receipts_archived").unwrap_or(0);
    let retained = value_u64(value, "receipts_retained").unwrap_or(0);
    let path = value_str(value, "archive_path").unwrap_or("<unknown>");
    let before = value_str(value, "before").unwrap_or("<unknown>");
    [
        format!(
            "State archive: {}",
            if dry_run { "dry run" } else { "archived" }
        ),
        format!("  Before: {before}"),
        format!("  Archive: {path}"),
        format!("  Receipts archived: {archived}"),
        format!("  Receipts retained: {retained}"),
        "  full report: rerun with --json".into(),
    ]
    .join("\n")
}

pub(super) fn format_dev_summary(value: &serde_json::Value) -> String {
    dev::format_dev_summary(value)
}

pub(super) fn format_dev_status_summary(value: &serde_json::Value) -> String {
    dev::format_dev_status_summary(value)
}

pub(super) fn format_dev_stop_summary(value: &serde_json::Value) -> String {
    dev::format_dev_stop_summary(value)
}

pub(super) fn format_proxy_summary(value: &serde_json::Value) -> String {
    if value_bool(value, "interrupted").unwrap_or(false) {
        let signal = value_str(value, "termination_signal").unwrap_or("signal");
        let mut lines = vec![format!("Proxy: stopped ({signal})")];
        if let Some(app) = value_str(value, "app") {
            lines.push(format!("  App: {app}"));
        }
        lines.push("  full report: rerun with --json".into());
        return lines.join("\n");
    }

    let ok = value_bool(value, "ok").unwrap_or(false);
    let mut lines = vec![format!("Proxy: {}", if ok { "ok" } else { "failed" })];
    if let Some(running) = value_bool(value, "running") {
        lines.push(format!("  Running: {}", if running { "yes" } else { "no" }));
    }
    if let Some(pid) = value_i64(value, "pid") {
        lines.push(format!("  PID: {pid}"));
    }
    if let Some(http) = value_u64(value, "http_port") {
        lines.push(format!("  HTTP port: {http}"));
    }
    if let Some(https) = value_u64(value, "https_port") {
        lines.push(format!("  HTTPS port: {https}"));
    }
    if let Some(hostname) = value_str(value, "hostname") {
        lines.push(format!("  Hostname: {hostname}"));
    }
    if let Some(app) = value_str(value, "app") {
        lines.push(format!("  App: {app}"));
    }
    if let Some(routes) = value["routes"].as_array() {
        lines.push(format!("  Routes: {}", routes.len()));
    }
    if let Some(path) = value_str(value, "path") {
        lines.push(format!("  Path: {path}"));
    }
    if let Some(state_dir) = value_str(value, "state_dir") {
        lines.push(format!("  State: {state_dir}"));
    }
    if let Some(warning) = value_str(value, "warning")
        .or_else(|| value_str(value, "trust_warning"))
        .or_else(|| value_str(value, "note"))
    {
        lines.push(format!("  Note: {warning}"));
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

fn receipt_preview(receipt: &serde_json::Value) -> Option<String> {
    value_str(receipt, "stderr_preview")
        .filter(|preview| !preview.trim().is_empty())
        .or_else(|| {
            value_str(receipt, "stdout_preview").filter(|preview| !preview.trim().is_empty())
        })
        .map(|preview| concise_preview(preview, 180))
}

fn concise_preview(preview: &str, max_chars: usize) -> String {
    concise_preview_with_truncation(preview, max_chars).0
}

fn concise_preview_with_truncation(preview: &str, max_chars: usize) -> (String, bool) {
    let trimmed = preview.trim();
    if trimmed.chars().count() <= max_chars {
        return (trimmed.to_string(), false);
    }

    let one_line = preview.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max_chars {
        return (one_line, false);
    }

    // Receipt previews are diagnostic text; truncate on scalar boundaries so
    // UTF-8 stays valid, accepting that grapheme clusters may split.
    let mut truncated = one_line
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    (truncated, true)
}

fn value_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

fn value_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(serde_json::Value::as_bool)
}

fn value_i64(value: &serde_json::Value, key: &str) -> Option<i64> {
    value.get(key).and_then(serde_json::Value::as_i64)
}

fn value_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
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

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
