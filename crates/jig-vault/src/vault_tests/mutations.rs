use super::*;

#[test]
fn field_mutations_require_explicit_version_one_migration_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    init_v1(&store, &passphrase());
    let before_vault = store.read_vault_text().unwrap().unwrap();
    let before_audit = store.read_audit_text().unwrap().unwrap();
    let reference = VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap();

    let error = store
        .apply_field_batch(
            &passphrase(),
            vec![FieldMutation::set(
                reference,
                FieldKind::Concealed,
                SecretBytes::new(b"field-secret-value".to_vec()),
            )],
        )
        .unwrap_err();

    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("jig vault migrate --to 2"));
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), before_audit);
}

#[test]
fn field_batch_is_atomic_and_preserves_encrypted_text_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let concealed = VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap();
    let text = VaultReference::parse("jig://Production/RESTIC_COMPRESSION").unwrap();

    let batch = vault
        .apply_field_batch(
            &passphrase(),
            vec![
                FieldMutation::set(
                    concealed.clone(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"field-secret-value".to_vec()),
                ),
                FieldMutation::set(text.clone(), FieldKind::Text, SecretBytes::new(Vec::new())),
            ],
        )
        .unwrap();
    assert_eq!(batch.changed, vec![concealed.clone(), text.clone()]);
    assert!(batch.removed.is_empty());

    let fields = vault.list_fields(&passphrase()).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].reference, text);
    assert_eq!(fields[0].kind, FieldKind::Text);
    assert_eq!(fields[0].value_len, 0);
    assert_eq!(fields[1].reference, concealed);
    assert_eq!(fields[1].kind, FieldKind::Concealed);

    let opened = vault.store.open_unlocked(&passphrase()).unwrap();
    assert_eq!(
        opened
            .secret_value(&text.to_secret_name())
            .unwrap()
            .as_slice(),
        b""
    );

    let file_text = std::fs::read_to_string(vault.root().join("vault.json")).unwrap();
    assert!(!file_text.contains("field-secret-value"));
    let audit = std::fs::read_to_string(vault.root().join("audit.jsonl")).unwrap();
    assert!(audit.contains("\"action\":\"field_batch_apply\""));
    assert!(audit.contains("jig://Production/RESTIC_PASSWORD"));
    assert!(audit.contains("\"kind\":\"concealed\""));
    assert!(audit.contains("\"kind\":\"text\""));
    assert!(!audit.contains("field-secret-value"));

    let removal = vault
        .remove_field(&passphrase(), concealed.clone())
        .unwrap();
    assert!(removal.changed.is_empty());
    assert_eq!(removal.removed, vec![concealed.clone()]);
    let repeated_removal = vault.remove_field(&passphrase(), concealed).unwrap();
    assert!(repeated_removal.removed.is_empty());
    let audit_after_removals = std::fs::read_to_string(vault.root().join("audit.jsonl")).unwrap();
    assert!(audit_after_removals.contains("\"removed\":true"));
    assert!(audit_after_removals.contains("\"removed\":false"));

    let before_vault = std::fs::read_to_string(vault.root().join("vault.json")).unwrap();
    let before_audit = std::fs::read_to_string(vault.root().join("audit.jsonl")).unwrap();
    let duplicate = VaultReference::parse("jig://Production/DUPLICATE").unwrap();
    let error = vault
        .apply_field_batch(
            &passphrase(),
            vec![
                FieldMutation::set(
                    duplicate.clone(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"first-secret-value".to_vec()),
                ),
                FieldMutation::set(
                    duplicate,
                    FieldKind::Text,
                    SecretBytes::new(b"second-value".to_vec()),
                ),
            ],
        )
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("duplicate reference"));
    assert_eq!(
        std::fs::read_to_string(vault.root().join("vault.json")).unwrap(),
        before_vault
    );
    assert_eq!(
        std::fs::read_to_string(vault.root().join("audit.jsonl")).unwrap(),
        before_audit
    );
}

#[test]
fn legacy_secret_set_remains_concealed_in_version_two() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    store
        .set_secret(
            &passphrase(),
            "Production/RESTIC_PASSWORD",
            SecretBytes::new(b"legacy-secret-value".to_vec()),
        )
        .unwrap();

    let fields = store.list_fields(&passphrase()).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Concealed);
    assert_eq!(
        fields[0].reference.to_string(),
        "jig://Production/RESTIC_PASSWORD"
    );
}

#[test]
fn write_side_vault_limit_accepts_the_read_boundary_and_refuses_one_extra_byte() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    let boundary = "x".repeat(crate::store::VAULT_TEXT_READ_LIMIT as usize);
    store.write_vault_text(&boundary).unwrap();
    assert_eq!(
        store.read_vault_text().unwrap().unwrap().len(),
        boundary.len()
    );

    let oversized = format!("{boundary}x");
    let error = store.write_vault_text(&oversized).unwrap_err().to_string();
    assert!(error.contains("persistent vault limit"));
    assert_eq!(store.read_vault_text().unwrap().unwrap(), boundary);
}

#[test]
fn oversized_valid_field_batch_fails_before_audit_or_state_write() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let before_vault = store.read_vault_text().unwrap().unwrap();
    let before_audit = store.read_audit_text().unwrap().unwrap();
    let mut mutations = Vec::new();
    for index in 0..12 {
        mutations.push(FieldMutation::set(
            VaultReference::parse(&format!("jig://Production/LARGE_FIELD_{index}")).unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(vec![b'x'; MAX_SECRET_VALUE_LEN]),
        ));
    }

    let error = store
        .apply_field_batch(&passphrase(), mutations)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("too large to save safely"));
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), before_audit);
    assert!(store.open_unlocked(&passphrase()).is_ok());
    assert!(store.verify_audit(&passphrase()).is_ok());
}

#[test]
fn batch_validation_rejects_mixed_valid_short_and_oversized_values_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let before_vault = store.read_vault_text().unwrap().unwrap();
    let before_audit = store.read_audit_text().unwrap().unwrap();

    for invalid_value in [
        SecretBytes::new(b"bad".to_vec()),
        SecretBytes::new(vec![b'x'; MAX_SECRET_VALUE_LEN + 1]),
    ] {
        let error = store
            .apply_field_batch(
                &passphrase(),
                vec![
                    FieldMutation::set(
                        VaultReference::parse("jig://Production/VALID").unwrap(),
                        FieldKind::Concealed,
                        SecretBytes::new(b"valid-secret-value".to_vec()),
                    ),
                    FieldMutation::set(
                        VaultReference::parse("jig://Production/INVALID").unwrap(),
                        FieldKind::Concealed,
                        invalid_value,
                    ),
                ],
            )
            .unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
        assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
        assert_eq!(store.read_audit_text().unwrap().unwrap(), before_audit);
    }
    assert!(store.open_unlocked(&passphrase()).is_ok());
}

#[test]
fn field_batch_save_failure_leaves_audited_leading_intent_and_can_be_retried() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let before_vault = store.read_vault_text().unwrap().unwrap();
    let reference = VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap();

    store.fail_next_vault_write_for_test();
    let error = store
        .apply_field_batch(
            &passphrase(),
            vec![FieldMutation::set(
                reference.clone(),
                FieldKind::Concealed,
                SecretBytes::new(b"field-secret-value".to_vec()),
            )],
        )
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert!(store.verify_audit(&passphrase()).is_ok());
    assert!(store.list_fields(&passphrase()).unwrap().is_empty());

    store
        .apply_field_batch(
            &passphrase(),
            vec![FieldMutation::set(
                reference,
                FieldKind::Concealed,
                SecretBytes::new(b"field-secret-value".to_vec()),
            )],
        )
        .unwrap();
    assert_eq!(store.list_fields(&passphrase()).unwrap().len(), 1);
}

#[test]
fn migration_save_failure_leaves_version_one_readable_and_can_be_retried() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    init_v1(&store, &passphrase());
    let before_vault = store.read_vault_text().unwrap().unwrap();

    store.fail_next_vault_write_for_test();
    let error = store.migrate(&passphrase(), FORMAT_VERSION).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert!(store.verify_audit(&passphrase()).is_ok());
    let still_v1: VaultFile = serde_json::from_str(&before_vault).unwrap();
    assert_eq!(still_v1.header.version, V1_FORMAT_VERSION);

    let retry = store.migrate(&passphrase(), FORMAT_VERSION).unwrap();
    assert!(retry.changed);
    let migrated: VaultFile =
        serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();
    assert_eq!(migrated.header.version, FORMAT_VERSION);
}

#[test]
fn tampered_audit_blocks_field_batches_and_migration_without_new_append() {
    let temp = tempfile::tempdir().unwrap();
    let field_store = VaultStore::resolve(Some(temp.path().join("field-vault"))).unwrap();
    field_store.init(&passphrase()).unwrap();
    let mut audit = field_store.read_audit_text().unwrap().unwrap();
    audit.push_str("not json\n");
    std::fs::write(field_store.audit_path(), &audit).unwrap();
    let before_field_vault = field_store.read_vault_text().unwrap().unwrap();
    let field_error = field_store
        .apply_field_batch(
            &passphrase(),
            vec![FieldMutation::set(
                VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(b"field-secret-value".to_vec()),
            )],
        )
        .unwrap_err();
    assert_eq!(field_error.kind(), VaultErrorKind::AuditTampered);
    assert_eq!(
        field_store.read_vault_text().unwrap().unwrap(),
        before_field_vault
    );
    assert_eq!(field_store.read_audit_text().unwrap().unwrap(), audit);

    let migration_store = VaultStore::resolve(Some(temp.path().join("migration-vault"))).unwrap();
    init_v1(&migration_store, &passphrase());
    let mut migration_audit = migration_store.read_audit_text().unwrap().unwrap();
    migration_audit.push_str("not json\n");
    std::fs::write(migration_store.audit_path(), &migration_audit).unwrap();
    let before_migration_vault = migration_store.read_vault_text().unwrap().unwrap();
    let migration_error = migration_store
        .migrate(&passphrase(), FORMAT_VERSION)
        .unwrap_err();
    assert_eq!(migration_error.kind(), VaultErrorKind::AuditTampered);
    assert_eq!(
        migration_store.read_vault_text().unwrap().unwrap(),
        before_migration_vault
    );
    assert_eq!(
        migration_store.read_audit_text().unwrap().unwrap(),
        migration_audit
    );
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
fn open_validates_header_before_decoding_payload_fields() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let text = store.read_vault_text().unwrap().unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
    file["header"]["magic"] = serde_json::Value::String("not-a-vault".into());
    file["header"]["salt_b64"] = serde_json::Value::String("not valid base64".into());
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();

    let error = format!("{:#}", store.open_unlocked(&passphrase()).unwrap_err());
    assert!(error.contains("vault header is invalid"));
    assert!(error.contains("unsupported vault magic"));
    assert!(!error.contains("vault salt is invalid"));
}

#[test]
fn open_validates_kdf_before_decoding_wrapped_key_fields() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let text = store.read_vault_text().unwrap().unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
    file["header"]["kdf"]["memory_kib"] = serde_json::json!(0);
    file["wrapped_dek_nonce_b64"] = serde_json::Value::String("not valid base64".into());
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();

    let error = format!("{:#}", store.open_unlocked(&passphrase()).unwrap_err());
    assert!(error.contains("vault KDF parameters are invalid"));
    assert!(error.contains("memory cost"));
    assert!(!error.contains("wrapped vault key nonce is invalid"));
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
