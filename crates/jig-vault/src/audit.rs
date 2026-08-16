use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use time::OffsetDateTime;

use crate::store::VaultStore;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuditAction {
    BackupFailed,
    BackupFinish,
    #[cfg(target_os = "linux")]
    BackupRestore,
    BackupStart,
    BrokeredRunFailed,
    BrokeredRunFinish,
    BrokeredRunStart,
    ExecFailed,
    ExecFinish,
    ExecStart,
    FieldBatchApply,
    FieldKindChange,
    FieldRename,
    FieldReadFailed,
    FieldReadFinish,
    FieldReadStart,
    ItemRemove,
    ItemRename,
    LegacySecretConvert,
    OnePasswordImport,
    PassphraseChange,
    SecretRemove,
    SecretSet,
    TemplateInjectFailed,
    TemplateInjectFinish,
    TemplateInjectStart,
    VaultFormatMigrate,
    VaultInitialized,
}

impl AuditAction {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::BackupFailed => "backup_failed",
            Self::BackupFinish => "backup_finish",
            #[cfg(target_os = "linux")]
            Self::BackupRestore => "backup_restore",
            Self::BackupStart => "backup_start",
            Self::BrokeredRunFailed => "brokered_run_failed",
            Self::BrokeredRunFinish => "brokered_run_finish",
            Self::BrokeredRunStart => "brokered_run_start",
            Self::ExecFailed => "exec_failed",
            Self::ExecFinish => "exec_finish",
            Self::ExecStart => "exec_start",
            Self::FieldBatchApply => "field_batch_apply",
            Self::FieldKindChange => "field_kind_change",
            Self::FieldRename => "field_rename",
            Self::FieldReadFailed => "field_read_failed",
            Self::FieldReadFinish => "field_read_finish",
            Self::FieldReadStart => "field_read_start",
            Self::ItemRemove => "item_remove",
            Self::ItemRename => "item_rename",
            Self::LegacySecretConvert => "legacy_secret_convert",
            Self::OnePasswordImport => "onepassword_import",
            Self::PassphraseChange => "passphrase_change",
            Self::SecretRemove => "secret_remove",
            Self::SecretSet => "secret_set",
            Self::TemplateInjectFailed => "template_inject_failed",
            Self::TemplateInjectFinish => "template_inject_finish",
            Self::TemplateInjectStart => "template_inject_start",
            Self::VaultFormatMigrate => "vault_format_migrate",
            Self::VaultInitialized => "vault_initialized",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AuditEvent {
    pub version: u32,
    pub event_id: String,
    pub timestamp_ms: i128,
    pub action: String,
    pub previous_mac: Option<String>,
    pub details: Value,
    pub mac: String,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditVerification {
    pub event_count: usize,
    pub latest_mac: Option<String>,
    pub torn_tail_bytes: usize,
}

/// One verified audit event projected into metadata safe for an interactive
/// activity view.
///
/// The original event details and MAC fields are intentionally not exposed.
/// `subject` and `outcome` are assembled only from action-specific,
/// crate-controlled metadata.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultActivityRecord {
    pub event_id: String,
    pub timestamp_ms: i128,
    pub action: String,
    pub subject: Option<String>,
    pub outcome: Option<String>,
}

/// Verified audit metadata and its bounded, safe activity projection.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedVaultActivity {
    pub audit: AuditVerification,
    pub records: Vec<VaultActivityRecord>,
}

/// Maximum number of verified activity records returned by one request.
pub const MAX_VAULT_ACTIVITY_RECORDS: usize = 1_000;

#[derive(Serialize)]
struct AuditEventForMac<'a> {
    version: u32,
    event_id: &'a str,
    timestamp_ms: i128,
    action: &'a str,
    previous_mac: &'a Option<String>,
    details: &'a Value,
}

#[derive(Serialize)]
struct CanonicalAuditEventForMac<'a> {
    version: u32,
    event_id: &'a str,
    timestamp_ms: i128,
    action: &'a str,
    previous_mac: &'a Option<String>,
    details: Value,
}

impl AuditEvent {
    pub(crate) fn append(
        store: &VaultStore,
        audit_key: &[u8],
        action: AuditAction,
        details: Value,
    ) -> Result<Self> {
        store.with_lock(|| Self::append_unlocked(store, audit_key, action, details))
    }

    pub(crate) fn append_unlocked(
        store: &VaultStore,
        audit_key: &[u8],
        action: AuditAction,
        details: Value,
    ) -> Result<Self> {
        // Future audit rotation/checkpointing should replace this full-chain
        // append-time verification before high-volume vault runs make append
        // cost grow quadratically over the audit log lifetime.
        let verified = verify_chain_for_append_unlocked(
            store,
            audit_key,
            matches!(action, AuditAction::VaultInitialized),
        )?;
        let truncated_torn_tail_bytes = verified.audit_len.saturating_sub(verified.valid_len);
        // Build recovery details before truncating so a reserved-key collision
        // rejects the append without mutating the audit file.
        let details = details_with_recovery(details, truncated_torn_tail_bytes)?;
        if verified.valid_len < verified.audit_len {
            store.truncate_audit_unlocked(verified.valid_len as u64)?;
        }
        let previous_mac = verified.verification.latest_mac;
        let event_id = ulid::Ulid::new().to_string();
        let timestamp_ms = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        let action = action.as_str();
        let mac = event_mac(
            audit_key,
            &AuditEventForMac {
                version: 1,
                event_id: &event_id,
                timestamp_ms,
                action,
                previous_mac: &previous_mac,
                details: &details,
            },
        )?;
        let event = Self {
            version: 1,
            event_id,
            timestamp_ms,
            action: action.into(),
            previous_mac,
            details,
            mac,
        };
        store.append_audit_line_unlocked(&serde_json::to_string(&event)?)?;
        Ok(event)
    }

    #[cfg(test)]
    pub(crate) fn verify_chain(store: &VaultStore, audit_key: &[u8]) -> Result<AuditVerification> {
        store.with_lock(|| verify_chain_unlocked(store, audit_key))
    }
}

pub(crate) fn verify_chain_unlocked(
    store: &VaultStore,
    audit_key: &[u8],
) -> Result<AuditVerification> {
    let text = store.read_audit_text()?.ok_or_else(|| {
        anyhow::anyhow!(
            "vault audit log is missing at {}; remove the stale vault home or restore audit.jsonl before continuing",
            store.audit_path().display()
        )
    })?;
    Ok(verify_chain_text(&text, audit_key)?.verification)
}

pub(crate) fn verified_activity_unlocked(
    store: &VaultStore,
    audit_key: &[u8],
    limit: usize,
) -> Result<VerifiedVaultActivity> {
    let text = store.read_audit_text()?.ok_or_else(|| {
        anyhow::anyhow!(
            "vault audit log is missing at {}; remove the stale vault home or restore audit.jsonl before continuing",
            store.audit_path().display()
        )
    })?;
    let verified = verify_chain_text(&text, audit_key)?;
    let events = text[..verified.valid_len]
        .lines()
        .rev()
        .take(limit)
        .map(|line| {
            serde_json::from_str::<AuditEvent>(line)
                .context("failed to re-read a verified vault audit event")
                .map(project_activity_record)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(VerifiedVaultActivity {
        audit: verified.verification,
        records: events,
    })
}

struct VerifiedAuditLog {
    verification: AuditVerification,
    valid_len: usize,
    audit_len: usize,
}

fn verify_chain_for_append_unlocked(
    store: &VaultStore,
    audit_key: &[u8],
    allow_missing: bool,
) -> Result<VerifiedAuditLog> {
    match store.read_audit_text()? {
        Some(text) => verify_chain_text(&text, audit_key),
        None if allow_missing => Ok(empty_verified_audit_log()),
        None if !store.exists()? => Ok(empty_verified_audit_log()),
        None => Err(anyhow::anyhow!(
            "vault audit log is missing at {}; remove the stale vault home or restore audit.jsonl before continuing",
            store.audit_path().display()
        )),
    }
}

const fn empty_verified_audit_log() -> VerifiedAuditLog {
    VerifiedAuditLog {
        verification: AuditVerification {
            event_count: 0,
            latest_mac: None,
            torn_tail_bytes: 0,
        },
        valid_len: 0,
        audit_len: 0,
    }
}

fn verify_chain_text(text: &str, audit_key: &[u8]) -> Result<VerifiedAuditLog> {
    let mut previous_mac = None;
    let mut event_count = 0;
    let mut valid_len = 0;
    let mut offset = 0;
    let audit_len = text.len();
    let mut torn_tail_bytes = 0;
    for (index, line_with_ending) in text.split_inclusive('\n').enumerate() {
        let line_ended = line_with_ending.ends_with('\n');
        let line = line_with_ending
            .strip_suffix('\n')
            .unwrap_or(line_with_ending);
        if line.trim().is_empty() {
            bail_audit_chain(index, "blank audit lines are not allowed")?;
        }
        let event: AuditEvent = match serde_json::from_str(line) {
            Ok(event) => event,
            Err(_) if !line_ended && offset + line_with_ending.len() == audit_len => {
                torn_tail_bytes = line_with_ending.len();
                break;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to parse vault audit event at line {}", index + 1)
                });
            }
        };
        // `previous_mac` is public audit metadata; only event MAC verification
        // below needs constant-time comparison.
        if event.previous_mac != previous_mac {
            bail_audit_chain(
                index,
                format!(
                    "expected previous_mac {:?}, found {:?}",
                    previous_mac, event.previous_mac
                ),
            )?;
        }
        let expected_mac = event_mac(
            audit_key,
            &AuditEventForMac {
                version: event.version,
                event_id: &event.event_id,
                timestamp_ms: event.timestamp_ms,
                action: &event.action,
                previous_mac: &event.previous_mac,
                details: &event.details,
            },
        )?;
        if !constant_time_bytes_eq(event.mac.as_bytes(), expected_mac.as_bytes()) {
            bail_audit_chain(index, "event MAC does not match event contents")?;
        }
        previous_mac = Some(event.mac);
        event_count += 1;
        offset += line_with_ending.len();
        valid_len = offset;
    }
    Ok(VerifiedAuditLog {
        verification: AuditVerification {
            event_count,
            latest_mac: previous_mac,
            torn_tail_bytes,
        },
        valid_len,
        audit_len,
    })
}

fn project_activity_record(event: AuditEvent) -> VaultActivityRecord {
    let action = safe_activity_text(&event.action).unwrap_or_else(|| "unsupported".into());
    VaultActivityRecord {
        event_id: safe_activity_text(&event.event_id).unwrap_or_else(|| "unknown".into()),
        timestamp_ms: event.timestamp_ms,
        subject: activity_subject(&action, &event.details),
        outcome: activity_outcome(&action, &event.details),
        action,
    }
}

fn activity_subject(action: &str, details: &Value) -> Option<String> {
    match action {
        "vault_initialized" => detail_string(details, "vault_id"),
        "secret_set" | "secret_remove" => detail_string(details, "secret_name"),
        "field_kind_change" | "field_read_start" => detail_string(details, "reference"),
        "field_rename" => detail_pair(details, "from", "to"),
        "item_remove" => detail_string(details, "item"),
        "item_rename" => detail_pair(details, "from", "to"),
        "legacy_secret_convert" => detail_pair(details, "secret_name", "reference"),
        "field_batch_apply" => first_field_batch_reference(details),
        "onepassword_import" => detail_count(details, "field_count", "fields"),
        "vault_format_migrate" => detail_number_pair(details, "from_version", "to_version"),
        "backup_start" | "backup_restore" => detail_string(details, "source_vault_id"),
        "backup_failed"
        | "backup_finish"
        | "exec_failed"
        | "exec_finish"
        | "exec_start"
        | "field_read_failed"
        | "field_read_finish"
        | "template_inject_failed"
        | "template_inject_finish"
        | "template_inject_start" => detail_string(details, "operation_id"),
        "brokered_run_failed" | "brokered_run_finish" | "brokered_run_start" => {
            detail_string(details, "run_id")
        }
        "passphrase_change" => details
            .get("format_version")
            .and_then(Value::as_u64)
            .map(|version| format!("format v{version}")),
        _ => None,
    }
}

fn activity_outcome(action: &str, details: &Value) -> Option<String> {
    if action.ends_with("_failed") {
        return Some("failed".into());
    }
    if action.ends_with("_finish") {
        return Some("finished".into());
    }
    if action.ends_with("_start") {
        return Some("started".into());
    }
    match action {
        "vault_initialized" => Some("initialized".into()),
        "vault_format_migrate" => Some("migrated".into()),
        "passphrase_change" => Some("changed".into()),
        "secret_set" => Some("saved".into()),
        "secret_remove" => details
            .get("removed")
            .and_then(Value::as_bool)
            .map(|removed| if removed { "removed" } else { "not found" }.into()),
        "field_batch_apply"
        | "field_kind_change"
        | "field_rename"
        | "item_remove"
        | "item_rename"
        | "legacy_secret_convert" => Some("applied".into()),
        "onepassword_import" => Some("imported".into()),
        "backup_restore" => Some("restored".into()),
        _ => None,
    }
}

fn detail_string(details: &Value, key: &str) -> Option<String> {
    details
        .get(key)
        .and_then(Value::as_str)
        .and_then(safe_activity_text)
}

fn detail_pair(details: &Value, left: &str, right: &str) -> Option<String> {
    let left = detail_string(details, left)?;
    let right = detail_string(details, right)?;
    Some(format!("{left} -> {right}"))
}

fn detail_number_pair(details: &Value, left: &str, right: &str) -> Option<String> {
    let left = details.get(left)?.as_u64()?;
    let right = details.get(right)?.as_u64()?;
    Some(format!("{left} -> {right}"))
}

fn detail_count(details: &Value, key: &str, label: &str) -> Option<String> {
    let count = details.get(key)?.as_u64()?;
    Some(format!("{count} {label}"))
}

fn first_field_batch_reference(details: &Value) -> Option<String> {
    ["set", "remove"].into_iter().find_map(|key| {
        details
            .get(key)
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(|value| detail_string(value, "reference"))
    })
}

fn safe_activity_text(value: &str) -> Option<String> {
    const MAX_ACTIVITY_TEXT_BYTES: usize = 256;
    if value.is_empty()
        || value.len() > MAX_ACTIVITY_TEXT_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        return None;
    }
    Some(value.to_owned())
}

fn bail_audit_chain<T>(zero_based_index: usize, reason: impl std::fmt::Display) -> Result<T> {
    anyhow::bail!(
        "vault audit chain verification failed at line {}: {}",
        zero_based_index + 1,
        reason
    )
}

fn constant_time_bytes_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && bool::from(left.ct_eq(right))
}

fn details_with_recovery(mut details: Value, truncated_torn_tail_bytes: usize) -> Result<Value> {
    if truncated_torn_tail_bytes == 0 {
        return Ok(details);
    }
    if contains_recovery_key(&details) {
        anyhow::bail!("audit details use reserved recovery key");
    }
    match &mut details {
        Value::Object(map) => {
            map.insert(
                "truncated_torn_tail_bytes".into(),
                serde_json::json!(truncated_torn_tail_bytes),
            );
            Ok(details)
        }
        _ => Ok(serde_json::json!({
            "details": details,
            "truncated_torn_tail_bytes": truncated_torn_tail_bytes,
        })),
    }
}

fn contains_recovery_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| key == "truncated_torn_tail_bytes" || contains_recovery_key(value)),
        Value::Array(values) => values.iter().any(contains_recovery_key),
        // String values cannot introduce reserved JSON object keys.
        _ => false,
    }
}

fn event_mac(key: &[u8], event: &AuditEventForMac<'_>) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(key).context("invalid vault audit key")?;
    let canonical_event = CanonicalAuditEventForMac {
        version: event.version,
        event_id: event.event_id,
        timestamp_ms: event.timestamp_ms,
        action: event.action,
        previous_mac: event.previous_mac,
        details: canonical_json_value(event.details),
    };
    // Struct field order is part of the v1 MAC input; changing it needs a
    // format bump.
    mac.update(&serde_json::to_vec(&canonical_event)?);
    Ok(hex_lower(&mac.finalize().into_bytes()))
}

fn canonical_json_value(value: &Value) -> Value {
    // Audit detail schemas are crate-controlled; object key order is the only
    // representation choice normalized here. Numeric representations are not
    // normalized because audit details do not accept external JSON in v1.
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_json_value).collect()),
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let mut canonical = Map::new();
            for (key, value) in entries {
                canonical.insert(key.clone(), canonical_json_value(value));
            }
            Value::Object(canonical)
        }
        _ => value.clone(),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_chains_previous_mac() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        let first = AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({}))
            .unwrap();
        let second = AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretRemove,
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(second.previous_mac.as_deref(), Some(first.mac.as_str()));
        let verification = AuditEvent::verify_chain(&store, &key).unwrap();
        assert_eq!(verification.event_count, 2);
        assert_eq!(verification.torn_tail_bytes, 0);
        assert_eq!(
            verification.latest_mac.as_deref(),
            Some(second.mac.as_str())
        );
    }

    #[test]
    fn append_truncates_torn_final_audit_line() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        let first = AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({}))
            .unwrap();
        let mut text = store.read_audit_text().unwrap().unwrap();
        text.push_str("{\"partial\"");
        std::fs::write(store.audit_path(), text).unwrap();

        let verification = AuditEvent::verify_chain(&store, &key).unwrap();
        assert_eq!(verification.event_count, 1);
        assert_eq!(verification.latest_mac.as_deref(), Some(first.mac.as_str()));
        assert!(verification.torn_tail_bytes > 0);

        let second = AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretRemove,
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(
            second.details["truncated_torn_tail_bytes"].as_u64(),
            Some(10)
        );
        let verification = AuditEvent::verify_chain(&store, &key).unwrap();
        assert_eq!(verification.event_count, 2);
        assert_eq!(
            verification.latest_mac.as_deref(),
            Some(second.mac.as_str())
        );
        assert_eq!(verification.torn_tail_bytes, 0);
    }

    #[test]
    fn append_rejects_reserved_recovery_key_on_torn_tail_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({})).unwrap();
        let mut text = store.read_audit_text().unwrap().unwrap();
        text.push_str("{\"partial\"");
        std::fs::write(store.audit_path(), text).unwrap();
        let text_before = store.read_audit_text().unwrap().unwrap();

        let error = AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretRemove,
            serde_json::json!({
                "truncated_torn_tail_bytes": 99,
            }),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("reserved recovery key"));
        assert_eq!(store.read_audit_text().unwrap().unwrap(), text_before);
    }

    #[test]
    fn append_rejects_nested_reserved_recovery_key_on_torn_tail_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({})).unwrap();
        let mut text = store.read_audit_text().unwrap().unwrap();
        text.push_str("{\"partial\"");
        std::fs::write(store.audit_path(), text).unwrap();

        let error = AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretRemove,
            serde_json::json!({
                "nested": {
                    "truncated_torn_tail_bytes": 99,
                },
            }),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("reserved recovery key"));
    }

    #[test]
    fn append_preserves_complete_final_audit_line_without_newline() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({})).unwrap();
        let text = store.read_audit_text().unwrap().unwrap();
        std::fs::write(store.audit_path(), text.trim_end_matches('\n')).unwrap();

        AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretRemove,
            serde_json::json!({}),
        )
        .unwrap();
        let verification = AuditEvent::verify_chain(&store, &key).unwrap();
        assert_eq!(verification.event_count, 2);
        assert_eq!(verification.torn_tail_bytes, 0);
    }

    #[test]
    fn event_mac_is_independent_of_json_object_insertion_order() {
        let key = [7_u8; 32];
        let mut left = serde_json::Map::new();
        left.insert("a".into(), serde_json::json!(1));
        left.insert("b".into(), serde_json::json!({"x": 1, "y": 2}));
        let mut right = serde_json::Map::new();
        right.insert("b".into(), serde_json::json!({"y": 2, "x": 1}));
        right.insert("a".into(), serde_json::json!(1));
        let previous_mac = None;
        let left = AuditEventForMac {
            version: 1,
            event_id: "event",
            timestamp_ms: 1,
            action: "secret_set",
            previous_mac: &previous_mac,
            details: &serde_json::Value::Object(left),
        };
        let right = AuditEventForMac {
            version: 1,
            event_id: "event",
            timestamp_ms: 1,
            action: "secret_set",
            previous_mac: &previous_mac,
            details: &serde_json::Value::Object(right),
        };

        assert_eq!(
            event_mac(&key, &left).unwrap(),
            event_mac(&key, &right).unwrap()
        );
    }

    #[test]
    fn verify_chain_rejects_tampered_event_details() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretSet,
            serde_json::json!({"ok": true}),
        )
        .unwrap();

        let text = store.read_audit_text().unwrap().unwrap();
        let tampered = text.replace("\"ok\":true", "\"ok\":false");
        std::fs::write(store.audit_path(), tampered).unwrap();

        let error = AuditEvent::verify_chain(&store, &key)
            .unwrap_err()
            .to_string();
        assert!(error.contains("verification failed"));
    }

    #[test]
    fn append_rejects_existing_tampered_audit_log() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretSet,
            serde_json::json!({"ok": true}),
        )
        .unwrap();

        let text = store.read_audit_text().unwrap().unwrap();
        let tampered = text.replace("\"ok\":true", "\"ok\":false");
        std::fs::write(store.audit_path(), tampered).unwrap();

        let error = AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretRemove,
            serde_json::json!({}),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("verification failed"));
    }

    #[test]
    fn append_rejects_forged_inserted_audit_event() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        let first = AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({}))
            .unwrap();

        let forged = AuditEvent {
            version: 1,
            event_id: ulid::Ulid::new().to_string(),
            timestamp_ms: first.timestamp_ms + 1,
            action: AuditAction::SecretRemove.as_str().into(),
            previous_mac: Some(first.mac),
            details: serde_json::json!({}),
            mac: "00".repeat(32),
        };
        let mut text = store.read_audit_text().unwrap().unwrap();
        text.push_str(&serde_json::to_string(&forged).unwrap());
        text.push('\n');
        std::fs::write(store.audit_path(), text).unwrap();

        let verify_error = AuditEvent::verify_chain(&store, &key)
            .unwrap_err()
            .to_string();
        assert!(verify_error.contains("verification failed"));
        let append_error = AuditEvent::append(
            &store,
            &key,
            AuditAction::SecretRemove,
            serde_json::json!({}),
        )
        .unwrap_err()
        .to_string();
        assert!(append_error.contains("verification failed"));
    }

    #[test]
    fn verify_chain_rejects_inserted_blank_lines() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let key = [7_u8; 32];
        AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({})).unwrap();
        let text = store.read_audit_text().unwrap().unwrap();
        std::fs::write(store.audit_path(), format!("\n{text}")).unwrap();

        let error = AuditEvent::verify_chain(&store, &key)
            .unwrap_err()
            .to_string();
        assert!(error.contains("blank audit lines"));
    }
}
