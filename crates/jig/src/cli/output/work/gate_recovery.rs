use serde_json::Value;

use super::{value_bool, value_str};

pub(super) fn append_details(lines: &mut Vec<String>, value: &Value) {
    for gate in value["gates"].as_array().into_iter().flatten() {
        for target in gate["targets"].as_array().into_iter().flatten() {
            if target["status"] == "passed" && target["freshness"] == "fresh" {
                continue;
            }
            lines.push(format!(
                "    {}: {}; {}",
                target_name(&target["target"]),
                value_str(target, "status").unwrap_or("unknown"),
                value_str(target, "freshness_reason").unwrap_or("inspect target evidence")
            ));
            for reason in target["freshness_reasons"].as_array().into_iter().flatten() {
                let code = value_str(reason, "code").unwrap_or("unknown");
                let path = value_str(reason, "path")
                    .map(|path| format!("; input {path}"))
                    .unwrap_or_default();
                let dependency = reason
                    .get("target")
                    .filter(|target| target.is_object())
                    .map(|target| format!("; target {}", target_name(target)))
                    .unwrap_or_default();
                lines.push(format!("      {code}{path}{dependency}"));
            }
            if value_bool(target, "freshness_reasons_truncated") == Some(true) {
                lines.push("      Input/reason preview is truncated; inspect structured evidence for retained diagnostics.".into());
            }
        }
    }
    let recovery = &value["recovery"];
    if !recovery.is_object() {
        return;
    }
    if value_bool(recovery, "preview_available") == Some(true) {
        lines.push(format!(
            "Native checks would execute: {}",
            target_list(&recovery["execute"])
        ));
        lines.push(format!(
            "Native passes would be reused: {}",
            target_list(&recovery["reuse"])
        ));
        for target in recovery["targets"].as_array().into_iter().flatten() {
            if let Some(command) = command(&target["refresh"]) {
                let reason = if target["reason"] == "dependency_execution" {
                    " (scheduled because a prerequisite executes)"
                } else {
                    ""
                };
                lines.push(format!(
                    "  Force {}{reason}: {command}",
                    target_name(&target["target"])
                ));
            }
        }
        if !recovery["execute"].as_array().is_some_and(Vec::is_empty)
            && let Some(note) = value_str(recovery, "legacy_tool_note")
        {
            lines.push(note.into());
        }
    }
    if let Some(message) = value_str(recovery, "message") {
        lines.push(message.into());
    }
}

/// True also means recovery deliberately withheld execution advice.
pub(super) fn append_next_step(lines: &mut Vec<String>, value: &Value) -> bool {
    let recovery = &value["recovery"];
    if let Some(command) = command(&recovery["next_step"]) {
        lines.push(format!("Next step: {command}"));
        return true;
    }
    if recovery.is_object() && value_bool(recovery, "preview_available") == Some(false) {
        lines.push("Next step: resolve the inspection diagnostics, then repeat read-only evidence inspection.".into());
        return true;
    }
    false
}

fn command(value: &Value) -> Option<String> {
    let argv = value["argv"].as_array()?;
    let args = argv
        .iter()
        .map(|arg| arg.as_str().map(crate::shell::quote))
        .collect::<Option<Vec<_>>>()?;
    (!args.is_empty()).then(|| args.join(" "))
}

fn target_name(value: &Value) -> String {
    format!(
        "{}:{}",
        value_str(value, "component").unwrap_or("?"),
        value_str(value, "action").unwrap_or("?")
    )
}

fn target_list(value: &Value) -> String {
    let targets: Vec<_> = value
        .as_array()
        .into_iter()
        .flatten()
        .map(target_name)
        .collect();
    if targets.is_empty() {
        "none".into()
    } else {
        targets.join(", ")
    }
}
