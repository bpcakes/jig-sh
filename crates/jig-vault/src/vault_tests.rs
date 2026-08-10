use super::*;
use secrecy::SecretString;

fn passphrase() -> SecretString {
    SecretString::from("correct horse battery staple".to_string())
}

#[test]
fn create_open_set_list_remove_secret() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();

    store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();

    assert_eq!(store.list(&passphrase()).unwrap()[0].name, "api_token");
    let reopened = store.open_unlocked(&passphrase()).unwrap();
    assert_eq!(
        reopened
            .secret_value(&SecretName::parse("api_token").unwrap())
            .unwrap()
            .as_slice(),
        b"secret-value"
    );
    store.remove_secret(&passphrase(), "api_token").unwrap();
    assert!(store.list(&passphrase()).unwrap().is_empty());
}

#[test]
fn missing_audit_log_with_existing_vault_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    std::fs::remove_file(store.audit_path()).unwrap();

    let verification = store.verify_audit(&passphrase()).unwrap_err();
    assert_eq!(verification.kind(), VaultErrorKind::AuditTampered);
    assert!(verification.to_string().contains("audit log is missing"));

    let mutation = store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap_err();
    assert_eq!(mutation.kind(), VaultErrorKind::AuditTampered);
    assert!(error_chain_contains(&mutation, "audit log is missing"));
}

fn error_chain_contains(error: &(dyn std::error::Error + 'static), needle: &str) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.to_string().contains(needle) {
            return true;
        }
        current = error.source();
    }
    false
}

#[test]
fn secret_value_rejects_corrupt_serialized_entry_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();

    let mut vault = store.open_unlocked(&passphrase()).unwrap();
    let entry = vault.state.secrets.get_mut("api_token").unwrap();
    entry.value_len += 1;
    let error = vault
        .secret_value(&SecretName::parse("api_token").unwrap())
        .unwrap_err()
        .to_string();

    assert!(error.contains("value length metadata is invalid"));
}

#[test]
fn secret_value_rejects_out_of_bounds_serialized_entry_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();

    let mut vault = store.open_unlocked(&passphrase()).unwrap();
    let entry = vault.state.secrets.get_mut("api_token").unwrap();
    entry.value_len = MAX_SECRET_VALUE_LEN + 1;
    let error = vault
        .secret_value(&SecretName::parse("api_token").unwrap())
        .unwrap_err()
        .to_string();

    assert!(error.contains("outside supported bounds"));
}

#[test]
fn open_vault_debug_output_does_not_include_secret_values() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();

    let debug = format!("{:?}", store.open_unlocked(&passphrase()).unwrap());
    assert!(!debug.contains("secret-value"));
    assert!(!debug.contains("c2VjcmV0LXZhbHVl"));
    assert!(debug.contains("secret_count"));
}

#[test]
fn updating_secret_preserves_created_at() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();
    let first = store.list(&passphrase()).unwrap().remove(0);
    store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"other-secret".to_vec()),
        )
        .unwrap();
    let second = store.list(&passphrase()).unwrap().remove(0);

    assert_eq!(second.created_at_ms, first.created_at_ms);
    assert!(second.updated_at_ms >= first.updated_at_ms);
}

#[test]
fn consecutive_saves_rotate_state_nonce() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let initial: VaultFile =
        serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();

    store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();
    let after_set: VaultFile =
        serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();

    store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"other-secret".to_vec()),
        )
        .unwrap();
    let after_update: VaultFile =
        serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();

    assert_ne!(after_set.state_nonce_b64, initial.state_nonce_b64);
    assert_ne!(after_update.state_nonce_b64, after_set.state_nonce_b64);
}

#[test]
fn second_init_refuses_existing_vault() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let error = store.init(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert!(error.to_string().contains("vault already exists"));
}

#[test]
fn init_refuses_stale_audit_without_vault() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    std::fs::write(store.audit_path(), "stale audit\n").unwrap();

    let error = store.init(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(!store.exists().unwrap());
}

#[test]
fn init_rejects_short_passphrase() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let error = store
        .init(&SecretString::from("too-short".to_string()))
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("at least 12 bytes"));
}

#[test]
fn wrong_passphrase_fails() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let error = store
        .open_unlocked(&SecretString::from("wrong passphrase".to_string()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("failed to unlock vault key"));
}

#[test]
fn public_open_errors_are_classified() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();

    let missing = store.list(&passphrase()).unwrap_err();
    assert_eq!(missing.kind(), VaultErrorKind::NotFound);

    store.init(&passphrase()).unwrap();
    let wrong_passphrase = store
        .list(&SecretString::from("wrong passphrase".to_string()))
        .unwrap_err();
    assert_eq!(wrong_passphrase.kind(), VaultErrorKind::Authentication);

    store.write_vault_text("{not json").unwrap();
    let corrupt = store.list(&passphrase()).unwrap_err();
    assert_eq!(corrupt.kind(), VaultErrorKind::Serialization);
}

#[test]
fn header_tamper_fails_authentication() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let text = store.read_vault_text().unwrap().unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
    file["header"]["vault_id"] = serde_json::Value::String("tampered".into());
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();
    let error = store.open_unlocked(&passphrase()).unwrap_err().to_string();
    assert!(error.contains("failed to unlock vault key"));
}

#[test]
fn wrapped_vault_key_rejects_state_aad_role() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let text = store.read_vault_text().unwrap().unwrap();
    let file: VaultFile = serde_json::from_str(&text).unwrap();
    let salt = decode_b64_array::<SALT_LEN>("vault salt", &file.header.salt_b64).unwrap();
    let wrap_key = derive_wrap_key(&passphrase(), &salt, &file.header.kdf).unwrap();
    let nonce =
        decode_b64_array::<NONCE_LEN>("wrapped vault key nonce", &file.wrapped_dek_nonce_b64)
            .unwrap();
    let wrapped_dek = B64.decode(&file.wrapped_dek_b64).unwrap();
    let wrong_role_aad = payload_aad(&file.header, AeadRole::State);

    assert!(open(&wrap_key, &nonce, &wrong_role_aad, &wrapped_dek).is_err());
}

#[test]
fn ciphertext_tamper_fails_authentication() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let text = store.read_vault_text().unwrap().unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
    let state = file["state_b64"].as_str().unwrap();
    let replacement = if state.starts_with('A') { "B" } else { "A" };
    file["state_b64"] = serde_json::Value::String(format!("{replacement}{}", &state[1..]));
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();
    assert!(store.open_unlocked(&passphrase()).is_err());
}

#[test]
fn audited_edit_rejects_tampered_audit_before_saving_state() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    store
        .edit_with_audit(
            &passphrase(),
            AuditAction::SecretSet,
            |vault| {
                vault.set_secret(
                    &SecretName::parse("api_token").unwrap(),
                    SecretBytes::new(b"secret-value".to_vec()),
                )
            },
            |()| serde_json::json!({"secret_name": "api_token"}),
        )
        .unwrap();

    let audit = store.read_audit_text().unwrap().unwrap();
    std::fs::write(
        store.audit_path(),
        audit.replace("\"secret_name\":\"api_token\"", "\"secret_name\":\"other\""),
    )
    .unwrap();
    let error = store
        .edit_with_audit(
            &passphrase(),
            AuditAction::SecretSet,
            |vault| {
                vault.set_secret(
                    &SecretName::parse("other").unwrap(),
                    SecretBytes::new(b"other-secret".to_vec()),
                )
            },
            |()| serde_json::json!({"secret_name": "other"}),
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("verification failed"));
    let public_error = store
        .set_secret(
            &passphrase(),
            "public_other",
            SecretBytes::new(b"public-other-secret".to_vec()),
        )
        .unwrap_err();
    assert_eq!(public_error.kind(), VaultErrorKind::AuditTampered);
    let reopened = store.open_unlocked(&passphrase()).unwrap();
    assert!(
        reopened
            .secret_value(&SecretName::parse("other").unwrap())
            .is_err()
    );
}

#[test]
fn public_verify_audit_reports_torn_tail_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let mut audit = store.read_audit_text().unwrap().unwrap();
    audit.push_str("{\"partial\"");
    std::fs::write(store.audit_path(), audit).unwrap();

    let verification = store.verify_audit(&passphrase()).unwrap();
    assert_eq!(verification.event_count, 1);
    assert!(verification.torn_tail_bytes > 0);
}

#[test]
fn set_secret_rejects_too_short_values_before_unlock() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();

    let error = store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(b"abc".to_vec()),
        )
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("at least 4 bytes"));
}

#[test]
fn set_secret_rejects_oversized_values() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let error = store
        .set_secret(
            &passphrase(),
            "api_token",
            SecretBytes::new(vec![b'x'; MAX_SECRET_VALUE_LEN + 1]),
        )
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("at most"));
}
