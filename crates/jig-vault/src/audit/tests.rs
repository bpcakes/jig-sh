use super::*;

#[test]
fn append_chains_previous_mac() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let key = [7_u8; 32];
    let first =
        AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({})).unwrap();
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
    let first =
        AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({})).unwrap();
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
    let first =
        AuditEvent::append(&store, &key, AuditAction::SecretSet, serde_json::json!({})).unwrap();

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
