use super::{concise_preview_with_truncation, value_bool, value_i64, value_str, value_u64};

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
