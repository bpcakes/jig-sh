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
        "vault passphrase change" => {
            if let Some(changed) = value_bool(value, "changed") {
                lines.push(format!("  Changed: {}", if changed { "yes" } else { "no" }));
            }
        }
        "vault backup create" => {
            if let Some(backup) = value_str(value, "backup") {
                lines.push(format!("  Backup: {backup}"));
            }
            if let Some(version) = value_u64(value, "backup_version") {
                lines.push(format!("  Backup version: {version}"));
            }
            if let Some(bytes) = value_u64(value, "bytes_written") {
                lines.push(format!("  Bytes written: {bytes}"));
            }
        }
        "vault backup restore" => {
            if let Some(backup) = value_str(value, "backup") {
                lines.push(format!("  Backup: {backup}"));
            }
            if let Some(restored) = value_bool(value, "restored") {
                lines.push(format!(
                    "  Restored: {}",
                    if restored { "yes" } else { "no" }
                ));
            }
            if let Some(version) = value_u64(value, "format_version") {
                lines.push(format!("  Vault format: {version}"));
            }
        }
        "vault migrate" => {
            if let Some(from_version) = value_u64(value, "from_version") {
                lines.push(format!("  From version: {from_version}"));
            }
            if let Some(to_version) = value_u64(value, "to_version") {
                lines.push(format!("  To version: {to_version}"));
            }
            if let Some(changed) = value_bool(value, "changed") {
                lines.push(format!("  Changed: {}", if changed { "yes" } else { "no" }));
            }
        }
        "vault field list" => {
            let fields = value["fields"].as_array().map(Vec::len).unwrap_or(0);
            lines.push(format!("  Fields: {fields}"));
            if let Some(item) = value_str(value, "item") {
                lines.push(format!("  Item: {item}"));
            }
            if let Some(items) = value["fields"].as_array() {
                for field in items.iter().take(20) {
                    let reference = value_str(field, "reference").unwrap_or("<unknown>");
                    let kind = value_str(field, "kind").unwrap_or("unknown");
                    lines.push(format!("  - {reference} ({kind})"));
                }
                if items.len() > 20 {
                    lines.push(format!("  (and {} more)", items.len() - 20));
                }
            }
        }
        "vault field set" => {
            if let Some(reference) = value_str(value, "reference") {
                lines.push(format!("  Field: {reference}"));
            }
            if let Some(kind) = value_str(value, "kind") {
                lines.push(format!("  Kind: {kind}"));
            }
            if let Some(changed) = value_bool(value, "changed") {
                lines.push(format!("  Changed: {}", if changed { "yes" } else { "no" }));
            }
        }
        "vault field remove" => {
            if let Some(reference) = value_str(value, "reference") {
                lines.push(format!("  Field: {reference}"));
            }
            if let Some(removed) = value_bool(value, "removed") {
                lines.push(format!("  Removed: {}", if removed { "yes" } else { "no" }));
            }
        }
        "vault import onepassword" => {
            if let Some(dry_run) = value_bool(value, "dry_run") {
                lines.push(format!("  Dry run: {}", if dry_run { "yes" } else { "no" }));
            }
            if let Some(destination) = value_str(value, "destination") {
                lines.push(format!("  Destination: {destination}"));
            }
            if let Some(fields) = value["fields"].as_array() {
                lines.push(format!("  Fields: {}", fields.len()));
                for field in fields.iter().take(20) {
                    let reference = value_str(field, "reference").unwrap_or("<unknown>");
                    let kind = value_str(field, "kind").unwrap_or("unknown");
                    let action = value_str(field, "action")
                        .map(|action| format!(", {action}"))
                        .unwrap_or_default();
                    lines.push(format!("  - {reference} ({kind}{action})"));
                }
                if fields.len() > 20 {
                    lines.push(format!("  (and {} more)", fields.len() - 20));
                }
            }
            if value_bool(value, "requires_replace") == Some(true) {
                lines.push("  Requires: --replace".into());
            }
            if value_bool(value, "requires_overwrite") == Some(true) {
                lines.push("  Requires: --overwrite".into());
            }
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
            if let Some(torn) = value_u64(value, "torn_tail_bytes")
                && torn > 0
            {
                lines.push(format!("  Torn tail bytes: {torn}"));
            }
        }
        _ => {}
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::format_vault_generic_summary;

    #[test]
    fn field_list_summary_uses_only_metadata() {
        let summary = format_vault_generic_summary(&json!({
            "ok": true,
            "command": "vault field list",
            "vault_scope": "explicit-home",
            "vault_home": "/tmp/jig-vault",
            "item": "jig://Production",
            "fields": [{
                "reference": "jig://Production/RESTIC_COMPRESSION",
                "kind": "text",
                "value_len": 5,
                "created_at_ms": 1,
                "updated_at_ms": 1,
            }],
        }));

        assert!(summary.contains("Fields: 1"));
        assert!(summary.contains("jig://Production/RESTIC_COMPRESSION (text)"));
        assert!(!summary.contains("value_len"));
    }

    #[test]
    fn migration_summary_reports_an_idempotent_result() {
        let summary = format_vault_generic_summary(&json!({
            "ok": true,
            "command": "vault migrate",
            "vault_scope": "explicit-home",
            "vault_home": "/tmp/jig-vault",
            "from_version": 2,
            "to_version": 2,
            "changed": false,
        }));

        assert!(summary.contains("From version: 2"));
        assert!(summary.contains("To version: 2"));
        assert!(summary.contains("Changed: no"));
    }

    #[test]
    fn lifecycle_summaries_report_only_public_metadata() {
        let passphrase = "do-not-print-current-or-new-passphrase";
        let changed = format_vault_generic_summary(&json!({
            "ok": true,
            "command": "vault passphrase change",
            "vault_scope": "explicit-home",
            "vault_home": "/tmp/jig-vault",
            "changed": true,
            "unexpected_sensitive_field": passphrase,
        }));
        assert!(changed.contains("Changed: yes"));

        let created = format_vault_generic_summary(&json!({
            "ok": true,
            "command": "vault backup create",
            "vault_scope": "explicit-home",
            "vault_home": "/tmp/jig-vault",
            "backup": "/tmp/jig-vault.backup",
            "bytes_written": 4096,
            "backup_version": 1,
            "created_at_ms": 1,
        }));
        assert!(created.contains("Backup version: 1"));
        assert!(created.contains("Bytes written: 4096"));
        assert!(!created.contains("created_at_ms"));

        let restored = format_vault_generic_summary(&json!({
            "ok": true,
            "command": "vault backup restore",
            "vault_scope": "explicit-home",
            "vault_home": "/tmp/restored-vault",
            "backup": "/tmp/jig-vault.backup",
            "restored": true,
            "vault_id": "public-vault-id",
            "format_version": 2,
        }));
        assert!(restored.contains("Restored: yes"));
        assert!(restored.contains("Vault format: 2"));
        assert!(!restored.contains("public-vault-id"));

        for summary in [changed, created, restored] {
            assert!(!summary.contains(passphrase));
        }
    }
}
