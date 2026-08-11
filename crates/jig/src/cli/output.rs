use std::io::Write;

use anyhow::Result;

use self::agent::{format_agent_bootstrap_summary, format_agent_doctor_summary};
use self::codex::{
    format_codex_homes_summary, format_codex_launch_summary, format_codex_resume_summary,
};
pub(super) use self::doctor::format_doctor_summary;
pub(super) use self::info::format_info_summary;
use self::loops::{
    format_loop_clear_attempt_summary, format_loop_run_summary, format_loop_status_summary,
    format_loop_tick_summary,
};
pub(super) use self::prompt::{format_prompt_human_output, print_prompt_warnings};
use self::state::{
    format_state_archive_summary, format_state_compact_summary, format_state_diagnose_summary,
    format_state_export_summary, format_state_restore_summary, format_state_summary,
};
use self::vault::{format_vault_generic_summary, format_vault_run_summary};
use self::work::{
    format_work_append_summary, format_work_check_summary, format_work_decide_summary,
    format_work_evidence_summary, format_work_finish_summary, format_work_gates_summary,
    format_work_goal_summary, format_work_receipts_summary, format_work_refine_summary,
    format_work_review_summary, format_work_start_plan_id, format_work_start_summary,
    format_work_status_summary,
};

mod agent;
mod codex;
mod dev;
mod doctor;
mod info;
mod loops;
mod prompt;
mod state;
mod status;
mod vault;
mod work;

pub(super) enum HumanOutput {
    Doctor,
    Setup,
    Info,
    Status,
    VaultRun,
    VaultGeneric,
    AgentDoctor,
    AgentBootstrap,
    CodexHomes,
    CodexLaunch,
    CodexResume,
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
    StateDiagnose,
    StateCompact,
    StateRestore,
    StateExport,
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
        HumanOutput::Doctor => format_doctor_summary(value),
        HumanOutput::Setup => format_setup_summary(value),
        HumanOutput::Info => format_info_summary(value),
        HumanOutput::Status => status::format_summary(value),
        HumanOutput::VaultRun => format_vault_run_summary(value),
        HumanOutput::VaultGeneric => format_vault_generic_summary(value),
        HumanOutput::AgentDoctor => format_agent_doctor_summary(value),
        HumanOutput::AgentBootstrap => format_agent_bootstrap_summary(value),
        HumanOutput::CodexHomes => format_codex_homes_summary(value),
        HumanOutput::CodexLaunch => format_codex_launch_summary(value),
        HumanOutput::CodexResume => format_codex_resume_summary(value),
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
        HumanOutput::StateDiagnose => format_state_diagnose_summary(value),
        HumanOutput::StateCompact => format_state_compact_summary(value),
        HumanOutput::StateRestore => format_state_restore_summary(value),
        HumanOutput::StateExport => format_state_export_summary(value),
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

pub(super) fn format_setup_summary(value: &serde_json::Value) -> String {
    let ready = value_bool(value, "ok").unwrap_or(false);
    let before_ready = value["doctor_before"]["ok"].as_bool().unwrap_or(false);
    let bootstrap_ready = value["bootstrap"]["ok"].as_bool().unwrap_or(false);
    let agent_ready = value["agent"]["after"]["ok"].as_bool().unwrap_or(false);
    let registrations = value["agent"]["registrations"]
        .as_array()
        .map_or(0, Vec::len);
    let contract_ready = value["contract"]["ok"].as_bool().unwrap_or(false);
    let doctor_ready = value["doctor_after"]["ok"].as_bool().unwrap_or(false);
    let mut lines = vec![
        format!(
            "Jig setup: {}",
            if ready { "ready" } else { "needs attention" }
        ),
        format!(
            "  Doctor preflight: {}",
            if before_ready {
                "ready"
            } else {
                "setup required"
            }
        ),
        format!(
            "  Bootstrap: {}",
            if bootstrap_ready { "passed" } else { "failed" }
        ),
        format!(
            "  Agent tooling: {}",
            if agent_ready { "ready" } else { "needs setup" }
        ),
    ];
    if registrations > 0 {
        lines.push(format!(
            "  Marketplace registrations: {registrations} completed"
        ));
    }
    lines.extend([
        format!(
            "  Contract: {}",
            if contract_ready { "passed" } else { "failed" }
        ),
        format!(
            "  Doctor final: {}",
            if doctor_ready {
                "ready"
            } else {
                "needs attention"
            }
        ),
    ]);
    if ready {
        lines.push("Next step: jig status".into());
    } else if let Some(step) = value["doctor_after"]["next_required_step"].as_str() {
        lines.push(format!("Next required step: {step}"));
    } else if let Some(step) = value["agent"]["after"]["next_steps"]
        .as_array()
        .and_then(|steps| steps.first())
        .and_then(serde_json::Value::as_str)
    {
        lines.push(format!("Next required step: {step}"));
    }
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

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
