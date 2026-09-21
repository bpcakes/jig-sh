use super::{concise_preview, gate_recovery, value_bool, value_str};

pub(super) fn format_phase_check_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let phase = value_str(value, "phase").unwrap_or("final");
    let explain = value_bool(value, "explain") == Some(true);
    let selected_ok = value_bool(value, "selected_ok") == Some(true);
    let final_ok = value_bool(value, "final_gates_ok") == Some(true);
    let pending = value["pending_final_requirements"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let heading = if explain {
        format!("Work check preview: {phase}")
    } else if selected_ok && phase == "iteration" && !final_ok {
        "Work check: iteration passed; final validation pending".into()
    } else if selected_ok {
        format!("Work check: {phase} selection passed")
    } else {
        format!("Work check: {phase} selection failed")
    };
    let mut lines = vec![heading, format!("  Plan: {plan_id}")];
    if explain {
        lines.push("  No actions were launched.".into());
    } else if !selected_ok && let Some(error) = value_str(value, "error") {
        lines.push(format!("  Failure: {}", concise_preview(error, 180)));
    }
    let invocations = value["selected_invocations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    lines.push(format!("  Selected invocations: {}", invocations.len()));
    for invocation in invocations.iter().take(8) {
        let target = &invocation["target"];
        let evidence = &invocation["evidence_validity"];
        lines.push(format!(
            "  - {}:{}: {} ({}), receipt {}, run {}",
            value_str(target, "component").unwrap_or("?"),
            value_str(target, "action").unwrap_or("?"),
            value_str(evidence, "status").unwrap_or("missing"),
            value_str(invocation, "disposition").unwrap_or("unknown"),
            value_str(evidence, "receipt_id").unwrap_or("none"),
            value_str(evidence, "run_id").unwrap_or("none"),
        ));
        append_rust_scope(&mut lines, &invocation["invocation"]["prepared_rust_input"]);
    }
    append_omitted(&mut lines, invocations.len(), "invocations");
    append_rust_fallbacks(&mut lines, value);
    let checks = value["selected_checks"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if !checks.is_empty() {
        lines.push(format!("  Selected legacy checks: {}", checks.len()));
        for check in checks {
            lines.push(format!(
                "  - {}: {}",
                value_str(check, "tool").unwrap_or("<unknown>"),
                value_str(check, "disposition").unwrap_or("unknown")
            ));
        }
    }
    lines.push(format!("  Pending final requirements: {}", pending.len()));
    for gate in pending {
        lines.push(format!(
            "  - {}: {} ({})",
            value_str(gate, "id").unwrap_or("<unknown>"),
            value_str(gate, "status").unwrap_or("unknown"),
            value_str(gate, "kind").unwrap_or("unknown")
        ));
    }
    let recovery = &value["final_recovery"];
    if recovery.is_object() {
        lines.push(format!(
            "  Final recovery inspection: {}",
            value_str(recovery, "inspection").unwrap_or("unknown")
        ));
        gate_recovery::append_recovery_details(&mut lines, recovery);
    }
    if pending.is_empty() {
        lines.push(format!(
            "Next step: scripts/jig work finish --plan-id {plan_id}"
        ));
    } else {
        if pending
            .iter()
            .any(|gate| value_str(gate, "kind") != Some("codex_review"))
        {
            if recovery.is_object() {
                gate_recovery::append_recovery_next_step(&mut lines, recovery);
            } else {
                lines.push(format!(
                    "Next check step: scripts/jig work check --plan-id {plan_id} --phase final"
                ));
            }
        }
        if pending
            .iter()
            .any(|gate| value_str(gate, "kind") == Some("codex_review"))
        {
            lines.push(format!(
                "Next review step: scripts/jig work review --plan-id {plan_id}"
            ));
        }
    }
    lines.join("\n")
}

fn safe_preview(value: &str) -> String {
    let sanitized = jig_tui::sanitize_text(value);
    concise_preview(
        &sanitized.split_whitespace().collect::<Vec<_>>().join(" "),
        140,
    )
}

fn append_omitted(lines: &mut Vec<String>, count: usize, label: &str) {
    if count > 8 {
        lines.push(format!(
            "    ... {} more {label}; use --json for details",
            count - 8
        ));
    }
}

fn append_scope_items(
    lines: &mut Vec<String>,
    label: &str,
    items: &[serde_json::Value],
    render: impl Fn(&serde_json::Value) -> String,
) {
    if items.is_empty() {
        return;
    }
    let shown = items
        .iter()
        .take(8)
        .map(render)
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!("    {label}: {shown}"));
    append_omitted(lines, items.len(), label);
}

fn append_rust_scope(lines: &mut Vec<String>, scope: &serde_json::Value) {
    if !scope.is_object() {
        return;
    }
    lines.push(format!(
        "    Rust scope: {}",
        safe_preview(value_str(scope, "disposition").unwrap_or("unknown"))
    ));
    let packages = scope["packages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if packages.is_empty() {
        lines.push("    Packages: workspace".into());
    } else {
        append_scope_items(lines, "Packages", packages, |value| {
            safe_preview(value.as_str().unwrap_or("?"))
        });
    }
    let targets = scope["targets"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if targets.is_empty() {
        lines.push("    Targets: all targets".into());
    } else {
        append_scope_items(lines, "Targets", targets, |target| {
            let kind = safe_preview(value_str(target, "kind").unwrap_or("?"));
            match value_str(target, "name") {
                Some(name) => format!("{kind}:{}", safe_preview(name)),
                None => kind,
            }
        });
    }
    if let Some(filter) = scope["args"].as_array().and_then(|args| {
        args.windows(2).find_map(|pair| {
            (pair[0].as_str() == Some("--filter-expr"))
                .then(|| pair[1].as_str())
                .flatten()
        })
    }) {
        lines.push(format!(
            "    Explicit test filter (preview): {}",
            safe_preview(filter)
        ));
    }
    let reasons = scope["reasons"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    append_scope_items(lines, "Scope reasons", reasons, |value| {
        safe_preview(value.as_str().unwrap_or("?"))
    });
}

fn append_rust_fallbacks(lines: &mut Vec<String>, value: &serde_json::Value) {
    let fallbacks = value["rust_focus_fallbacks"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for fallback in fallbacks.iter().take(8) {
        let target = &fallback["target"];
        lines.push(format!(
            "  Rust focus fallback: {}:{}; scope {}; reason {}",
            safe_preview(value_str(target, "component").unwrap_or("?")),
            safe_preview(value_str(target, "action").unwrap_or("?")),
            safe_preview(value_str(fallback, "scope").unwrap_or("unknown")),
            safe_preview(value_str(fallback, "reason").unwrap_or("unknown")),
        ));
    }
    append_omitted(lines, fallbacks.len(), "Rust focus fallbacks");
}
