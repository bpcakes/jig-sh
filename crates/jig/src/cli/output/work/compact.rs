use serde_json::Value;

use super::{gate_recovery, value_str};

pub(super) fn format(value: &Value) -> String {
    let readiness = if value["finish_ready"] == true {
        "ready to finish"
    } else {
        "not ready to finish"
    };
    let mut lines = vec![
        format!("Work completion: {readiness}"),
        format!("  Plan: {}", value_str(value, "plan_id").unwrap_or("?")),
    ];
    for activity in value["activity"].as_array().into_iter().flatten() {
        lines.push(format!(
            "  - {}: {} ({})",
            value_str(activity, "subject").unwrap_or("?"),
            value_str(activity, "status").unwrap_or("unknown"),
            value_str(activity, "disposition").unwrap_or("unknown")
        ));
    }
    for gate in value["gates"].as_array().into_iter().flatten() {
        lines.push(format!(
            "  Gate {} [{}]: {}{}",
            value_str(gate, "id").unwrap_or("?"),
            value_str(gate, "kind").unwrap_or("?"),
            value_str(gate, "status").unwrap_or("unknown"),
            if gate["required"] == true {
                " (required)"
            } else {
                " (optional)"
            }
        ));
        if !matches!(
            value_str(gate, "status"),
            Some("passed" | "reused" | "not_applicable")
        ) {
            lines.push(format!(
                "    {}",
                value_str(gate, "reason").unwrap_or("Inspect evidence.")
            ));
        }
        for target in gate["targets"].as_array().into_iter().flatten() {
            if target["status"] == "passed" && target["freshness"] == "fresh" {
                continue;
            }
            lines.push(format!(
                "    {}:{}: {}, freshness {}; {}",
                value_str(&target["target"], "component").unwrap_or("?"),
                value_str(&target["target"], "action").unwrap_or("?"),
                value_str(target, "status").unwrap_or("unknown"),
                value_str(target, "freshness").unwrap_or("unknown"),
                value_str(target, "reason").unwrap_or("Inspect evidence.")
            ));
        }
        if gate["targets_truncated"] == true {
            lines.push("    Target preview truncated; use full evidence.".into());
        }
    }
    if value["gates_truncated"] == true || value["activity_truncated"] == true {
        lines.push("  Preview truncated; use full evidence.".into());
    }
    if let Some(error) = value_str(value, "error") {
        lines.push(format!("  Check error: {error}"));
    }
    if value["observation"]["status"] != "complete"
        && let Some(message) = value_str(&value["observation"], "message")
    {
        lines.push(format!("  Observation: {message}"));
    }
    lines.push(value_str(value, "readiness_basis").unwrap_or("").into());
    for (key, label) in [
        ("next_step", "Next step"),
        ("evidence", "Full evidence"),
        ("receipts", "Receipts"),
    ] {
        if let Some(command) = gate_recovery::command(&value[key]) {
            lines.push(format!("{label}: {command}"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn compact_human_output_marks_unknown_and_quotes_recovery_arguments() {
        let report = json!({
            "plan_id": "plan example", "finish_ready": false,
            "readiness_basis": "Current observation only.",
            "gates": [{"id": "verify", "kind": "evidence", "required": true,
                "status": "unknown", "reason": "Observation deadline exhausted."}],
            "next_step": {"argv": ["scripts/jig", "work", "gates", "--plan-id", "plan example"]},
            "evidence": {"argv": ["scripts/jig", "work", "evidence", "--plan-id", "plan example"]}
        });
        let output = super::format(&report);
        assert!(output.contains("not ready to finish"));
        assert!(output.contains("unknown (required)"));
        assert!(output.contains("Observation deadline exhausted."));
        assert!(output.contains("--plan-id 'plan example'"));
        assert!(output.contains("Full evidence:"));
    }
}
