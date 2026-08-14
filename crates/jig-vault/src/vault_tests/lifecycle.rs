use super::*;
use crate::crypto::KdfParams;

#[test]
fn passphrase_change_preserves_identity_keys_state_and_rotates_encryption() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    let old = passphrase();
    let new = SecretString::from("new correct horse battery staple".to_owned());
    store.init(&old).unwrap();
    store
        .apply_field_batch(
            &old,
            vec![FieldMutation::set(
                VaultReference::parse("jig://Production/TOKEN").unwrap(),
                FieldKind::Text,
                SecretBytes::new(b"preserved encrypted value".to_vec()),
            )],
        )
        .unwrap();

    // Model a still-valid v2 envelope created under an older, weaker policy.
    // Rotation must adopt today's defaults rather than cloning these values.
    let opened = store.open_unlocked(&old).unwrap();
    let mut legacy_policy_file: VaultFile =
        serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();
    legacy_policy_file.header.kdf = KdfParams {
        memory_kib: 19_456,
        iterations: 1,
        parallelism: 1,
        ..KdfParams::default()
    };
    let salt =
        decode_b64_array::<SALT_LEN>("vault salt", &legacy_policy_file.header.salt_b64).unwrap();
    let legacy_wrap_key = derive_wrap_key(&old, &salt, &legacy_policy_file.header.kdf).unwrap();
    let wrapped_nonce = crate::crypto::random_array::<NONCE_LEN>().unwrap();
    legacy_policy_file.wrapped_dek_nonce_b64 = B64.encode(wrapped_nonce);
    legacy_policy_file.wrapped_dek_b64 = B64.encode(
        crate::crypto::seal(
            &legacy_wrap_key,
            &wrapped_nonce,
            &payload_aad(&legacy_policy_file.header, AeadRole::WrappedDek),
            opened.dek.as_ref(),
        )
        .unwrap(),
    );
    let state_plaintext =
        Zeroizing::new(opened.state.serialize_for_version(FORMAT_VERSION).unwrap());
    let state_nonce = crate::crypto::random_array::<NONCE_LEN>().unwrap();
    legacy_policy_file.state_nonce_b64 = B64.encode(state_nonce);
    legacy_policy_file.state_b64 = B64.encode(
        crate::crypto::seal(
            &opened.dek,
            &state_nonce,
            &payload_aad(&legacy_policy_file.header, AeadRole::State),
            &state_plaintext,
        )
        .unwrap(),
    );
    drop(opened);
    store
        .write_vault_text(&serde_json::to_string_pretty(&legacy_policy_file).unwrap())
        .unwrap();

    let before_text = store.read_vault_text().unwrap().unwrap();
    let before_file: VaultFile = serde_json::from_str(&before_text).unwrap();
    let before_open = store.open_unlocked(&old).unwrap();
    let before_dek = before_open.dek.clone();
    let before_audit_key = before_open.audit_key.clone();
    let before_state = Zeroizing::new(
        before_open
            .state
            .serialize_for_version(FORMAT_VERSION)
            .unwrap(),
    );
    drop(before_open);

    store.change_passphrase(&old, &new).unwrap();

    assert!(store.open_unlocked(&old).is_err());
    let after_open = store.open_unlocked(&new).unwrap();
    let after_state = Zeroizing::new(
        after_open
            .state
            .serialize_for_version(FORMAT_VERSION)
            .unwrap(),
    );
    assert_eq!(after_open.dek.as_ref(), before_dek.as_ref());
    assert_eq!(after_open.audit_key.as_ref(), before_audit_key.as_ref());
    assert_eq!(after_state.as_slice(), before_state.as_slice());
    drop(after_open);

    let after_file: VaultFile =
        serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();
    assert_eq!(after_file.header.vault_id, before_file.header.vault_id);
    assert_eq!(
        after_file.header.created_at_ms,
        before_file.header.created_at_ms
    );
    assert_eq!(
        serde_json::to_value(&after_file.header.kdf).unwrap(),
        serde_json::to_value(KdfParams::default()).unwrap()
    );
    assert_ne!(
        serde_json::to_value(&before_file.header.kdf).unwrap(),
        serde_json::to_value(&after_file.header.kdf).unwrap()
    );
    assert_ne!(after_file.header.salt_b64, before_file.header.salt_b64);
    assert_ne!(
        after_file.wrapped_dek_nonce_b64,
        before_file.wrapped_dek_nonce_b64
    );
    assert_ne!(after_file.state_nonce_b64, before_file.state_nonce_b64);
    assert_ne!(after_file.wrapped_dek_b64, before_file.wrapped_dek_b64);
    assert_ne!(after_file.state_b64, before_file.state_b64);
    store.verify_audit(&new).unwrap();
    assert_eq!(
        audit_events(&store).last().unwrap().action,
        "passphrase_change"
    );
    let audit = store.read_audit_text().unwrap().unwrap();
    assert!(!audit.contains(old.expose_secret()));
    assert!(!audit.contains(new.expose_secret()));
}

#[test]
fn rejected_passphrase_changes_leave_vault_and_audit_bytes_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    let old = passphrase();
    store.init(&old).unwrap();
    let before_vault = store.read_vault_text().unwrap().unwrap();
    let before_audit = store.read_audit_text().unwrap().unwrap();

    let short = SecretString::from("too-short".to_owned());
    let error = store.change_passphrase(&old, &short).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), before_audit);

    let wrong = SecretString::from("wrong current passphrase".to_owned());
    let replacement = SecretString::from("valid replacement passphrase".to_owned());
    let error = store.change_passphrase(&wrong, &replacement).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), before_audit);

    let tampered = before_audit.replacen("vault_initialized", "vault_initializeD", 1);
    std::fs::write(store.audit_path(), &tampered).unwrap();
    let error = store.change_passphrase(&old, &replacement).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), tampered);
}

#[test]
fn passphrase_change_save_failure_leaves_old_envelope_and_leading_intent() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    let old = passphrase();
    let new = SecretString::from("replacement passphrase after fault".to_owned());
    store.init(&old).unwrap();
    let before_vault = store.read_vault_text().unwrap().unwrap();
    store.fail_next_vault_write_for_test();

    let error = store.change_passphrase(&old, &new).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert!(store.open_unlocked(&old).is_ok());
    assert!(store.open_unlocked(&new).is_err());
    store.verify_audit(&old).unwrap();
    assert_eq!(
        audit_events(&store).last().unwrap().action,
        "passphrase_change"
    );

    store.change_passphrase(&old, &new).unwrap();
    assert!(store.open_unlocked(&old).is_err());
    assert!(store.open_unlocked(&new).is_ok());
}

#[test]
fn passphrase_preflight_is_noncreating_and_rejects_version_one() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing-vault");
    assert!(Vault::preflight_passphrase_change(missing.clone()).is_err());
    assert!(!missing.exists());

    let home = temp.path().join("legacy-vault");
    let store = VaultStore::resolve(Some(home.clone())).unwrap();
    init_v1(&store, &passphrase());
    let before_vault = store.read_vault_text().unwrap().unwrap();
    let before_audit = store.read_audit_text().unwrap().unwrap();
    let error = Vault::preflight_passphrase_change(home).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("migrate --to 2"));
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), before_audit);
}

#[cfg(unix)]
#[test]
fn direct_file_output_is_private_atomic_and_terminalizes_overwrite_refusal() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/TOKEN").unwrap();
    let value = b"private-file-sentinel";
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(value.to_vec()),
        )
        .unwrap();
    let output = temp.path().join("rendered.bin");

    let result = vault
        .read_field_to_file(&passphrase(), reference.clone(), &output, false)
        .unwrap();
    assert_eq!(result.bytes_written, value.len());
    assert_eq!(std::fs::read(&output).unwrap(), value);
    assert_eq!(
        std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let error = vault
        .read_field_to_file(&passphrase(), reference, &output, false)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&output).unwrap(), value);
    let events = audit_events(&vault.store);
    assert_eq!(events[events.len() - 2].action, "field_read_start");
    assert_eq!(events.last().unwrap().action, "field_read_failed");
    assert_eq!(events.last().unwrap().details["stage"], "sink_preflight");
}

#[cfg(unix)]
#[test]
fn vault_bound_private_output_precondition_rechecks_namespace_ownership() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();

    let reserved = vault.root().join("vault.json");
    let error = vault.preview_private_output(&reserved).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("outside the source vault home"));

    let destination = temp.path().join("generated.env");
    let precondition = vault.preview_private_output(&destination).unwrap();
    assert!(!precondition.destination_exists());
    crate::PreparedPrivateFile::prepare_if_unchanged(
        precondition,
        SecretBytes::new(b"MODE=production\n".to_vec()),
        false,
    )
    .unwrap()
    .install()
    .unwrap();
    assert_eq!(std::fs::read(destination).unwrap(), b"MODE=production\n");
}

#[cfg(unix)]
#[test]
fn direct_file_output_cannot_replace_vault_owned_paths() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/TOKEN").unwrap();
    let value = b"reserved-path-secret-sentinel";
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(value.to_vec()),
        )
        .unwrap();
    let nested = vault.root().join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o700)).unwrap();
    let vault_path = vault.root().join("vault.json");
    let audit_path = vault.root().join("audit.jsonl");
    let lock_path = vault.root().join("vault.lock");
    let before_vault = std::fs::read(&vault_path).unwrap();
    let before_lock = std::fs::read(&lock_path).unwrap();

    for destination in [
        vault_path.clone(),
        audit_path.clone(),
        lock_path.clone(),
        nested.join("rendered.bin"),
    ] {
        let error = vault
            .read_field_to_file(&passphrase(), reference.clone(), &destination, true)
            .unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
        assert!(error.to_string().contains("outside the source vault home"));
        assert!(!error.to_string().contains("reserved-path-secret-sentinel"));
    }

    assert_eq!(std::fs::read(&vault_path).unwrap(), before_vault);
    assert_eq!(std::fs::read(&lock_path).unwrap(), before_lock);
    assert!(!nested.join("rendered.bin").exists());
    assert_eq!(vault.snapshot(&passphrase()).unwrap().fields.len(), 1);
    vault.verify_audit(&passphrase()).unwrap();
    let audit = std::fs::read_to_string(audit_path).unwrap();
    assert!(!audit.contains("reserved-path-secret-sentinel"));
}
