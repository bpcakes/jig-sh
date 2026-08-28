use super::*;

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
fn new_vaults_use_version_two_envelopes() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();

    let file: VaultFile = serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();
    assert_eq!(file.header.version, FORMAT_VERSION);
    assert!(store.open_unlocked(&passphrase()).is_ok());
}

#[test]
fn cli_generated_v1_fixture_opens_lists_and_maps_concealed_fields() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&vault.store);
    let passphrase = cli_generated_v1_fixture_passphrase();

    let file: VaultFile =
        serde_json::from_str(&vault.store.read_vault_text().unwrap().unwrap()).unwrap();
    assert_eq!(file.header.magic, "jig-vault");
    assert_eq!(file.header.version, V1_FORMAT_VERSION);
    assert_eq!(file.header.kdf.algorithm, "argon2id");
    assert_eq!(file.header.kdf.memory_kib, 131_072);
    assert_eq!(file.header.kdf.iterations, 3);
    assert_eq!(file.header.kdf.parallelism, 4);
    assert_eq!(file.header.kdf.output_len, KEY_LEN as u32);
    assert!(payload_aad(&file.header, AeadRole::State).starts_with(b"jig-vault-header-v1\n"));

    let secrets = vault.list(&passphrase).unwrap();
    assert_eq!(secrets.len(), 1);
    assert_eq!(secrets[0].name, CLI_GENERATED_V1_SECRET_NAME);
    assert_eq!(secrets[0].value_len, CLI_GENERATED_V1_SECRET_VALUE.len());

    let fields = vault.list_fields(&passphrase).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].reference,
        VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap()
    );
    assert_eq!(fields[0].kind, FieldKind::Concealed);
    assert_eq!(fields[0].value_len, CLI_GENERATED_V1_SECRET_VALUE.len());

    let reopened = vault.store.open_unlocked(&passphrase).unwrap();
    assert_eq!(
        reopened
            .secret_value(&SecretName::parse(CLI_GENERATED_V1_SECRET_NAME).unwrap())
            .unwrap()
            .as_slice(),
        CLI_GENERATED_V1_SECRET_VALUE
    );

    let audit = vault.verify_audit(&passphrase).unwrap();
    assert_eq!(audit.event_count, 2);
    assert_eq!(
        audit.latest_mac.as_deref(),
        Some(CLI_GENERATED_V1_AUDIT_MAC)
    );
    assert_eq!(audit.torn_tail_bytes, 0);
}

#[cfg(unix)]
#[test]
fn cli_generated_v1_fixture_runs_without_emitting_plaintext() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&vault.store);
    let passphrase = cli_generated_v1_fixture_passphrase();
    let request = BrokeredRun::new(
        vec![
            "sh".into(),
            "-c".into(),
            "test \"$V1_FIXTURE_RESTIC_PASSWORD\" = \"v1-fixture-restic-password-6f2ab1\" && printf fixture-v1-run-ok".into(),
        ],
        vec![
            BrokeredEnv::parse(
                "V1_FIXTURE_RESTIC_PASSWORD=Production/RESTIC_PASSWORD",
            )
            .unwrap(),
        ],
    )
    .unwrap();

    let output = vault.run_brokered(&passphrase, request).unwrap();
    assert_eq!(output.exit_status, 0);
    assert_eq!(output.exit_signal, None);
    assert_eq!(output.stdout, "fixture-v1-run-ok");
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains("v1-fixture-restic-password-6f2ab1"));
    assert!(!output.stderr.contains("v1-fixture-restic-password-6f2ab1"));

    let audit = vault.verify_audit(&passphrase).unwrap();
    assert_eq!(audit.event_count, 4);
    assert_eq!(audit.torn_tail_bytes, 0);
}

#[cfg(unix)]
#[test]
fn cli_generated_v1_fixture_supports_transparent_exec_as_concealed() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&vault.store);
    let passphrase = cli_generated_v1_fixture_passphrase();
    let reference = VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap();
    let request = VaultExec::new(
        vec![
            "sh".into(),
            "-c".into(),
            "test \"$TOKEN\" = \"v1-fixture-restic-password-6f2ab1\"".into(),
        ],
        vec![ExecEnvBinding::field(exec_var("TOKEN"), reference)],
    )
    .unwrap();

    let outcome = vault.exec(&passphrase, request).unwrap();
    assert_eq!(outcome.exit_status, 0);
    assert_eq!(outcome.exit_signal, None);
    let events = audit_events(&vault.store);
    assert_eq!(events[events.len() - 2].action, "exec_start");
    assert_eq!(events.last().unwrap().action, "exec_finish");
    assert_eq!(
        events[events.len() - 2].details["operation_id"],
        events.last().unwrap().details["operation_id"]
    );
    assert!(
        !vault
            .store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .contains("v1-fixture-restic-password-6f2ab1")
    );
}

#[test]
fn cli_generated_v1_fixture_migrates_without_rewriting_its_audit_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&vault.store);
    let passphrase = cli_generated_v1_fixture_passphrase();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();
    let before: VaultFile = serde_json::from_str(&before_vault).unwrap();

    let migration = vault.migrate(&passphrase, FORMAT_VERSION).unwrap();
    assert_eq!(
        migration,
        VaultMigration {
            from_version: V1_FORMAT_VERSION,
            to_version: FORMAT_VERSION,
            changed: true,
        }
    );

    let after_vault = vault.store.read_vault_text().unwrap().unwrap();
    let after: VaultFile = serde_json::from_str(&after_vault).unwrap();
    assert_eq!(after.header.version, FORMAT_VERSION);
    assert_eq!(after.header.vault_id, before.header.vault_id);
    assert_eq!(after.header.created_at_ms, before.header.created_at_ms);
    assert_eq!(after.header.salt_b64, before.header.salt_b64);
    assert_eq!(after.header.kdf.algorithm, before.header.kdf.algorithm);
    assert_eq!(after.header.kdf.memory_kib, before.header.kdf.memory_kib);
    assert_eq!(after.header.kdf.iterations, before.header.kdf.iterations);
    assert_eq!(after.header.kdf.parallelism, before.header.kdf.parallelism);
    assert_eq!(after.header.kdf.output_len, before.header.kdf.output_len);
    assert_ne!(after.wrapped_dek_nonce_b64, before.wrapped_dek_nonce_b64);
    assert_ne!(after.state_nonce_b64, before.state_nonce_b64);

    let reopened = vault.store.open_unlocked(&passphrase).unwrap();
    assert_eq!(
        reopened
            .secret_value(&SecretName::parse(CLI_GENERATED_V1_SECRET_NAME).unwrap())
            .unwrap()
            .as_slice(),
        CLI_GENERATED_V1_SECRET_VALUE
    );
    assert_eq!(
        vault.list_fields(&passphrase).unwrap()[0].reference,
        VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap()
    );
    assert_eq!(
        vault.list_fields(&passphrase).unwrap()[0].kind,
        FieldKind::Concealed
    );

    let after_audit = vault.store.read_audit_text().unwrap().unwrap();
    assert!(after_audit.starts_with(&before_audit));
    assert!(after_audit.contains("\"action\":\"vault_format_migrate\""));
    let audit = vault.verify_audit(&passphrase).unwrap();
    assert_eq!(audit.event_count, 3);
    assert_eq!(audit.torn_tail_bytes, 0);
}

#[test]
fn cli_generated_v1_fixture_validates_header_before_salt_and_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&store);
    let mut file = cli_generated_v1_fixture_file();
    file["header"]["magic"] = serde_json::json!("not-a-vault");
    file["header"]["salt_b64"] = serde_json::json!("not valid base64");
    file["wrapped_dek_nonce_b64"] = serde_json::json!("also not valid base64");
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();

    let error = format!(
        "{:#}",
        store
            .open_unlocked(&cli_generated_v1_fixture_passphrase())
            .unwrap_err()
    );
    assert!(error.contains("vault header is invalid"));
    assert!(error.contains("unsupported vault magic"));
    assert!(!error.contains("vault salt is invalid"));
    assert!(!error.contains("wrapped vault key nonce is invalid"));
}

#[test]
fn cli_generated_v1_fixture_validates_kdf_before_wrapped_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&store);
    let mut file = cli_generated_v1_fixture_file();
    file["header"]["kdf"]["memory_kib"] = serde_json::json!(0);
    file["wrapped_dek_nonce_b64"] = serde_json::json!("not valid base64");
    file["state_nonce_b64"] = serde_json::json!("also not valid base64");
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();

    let error = format!(
        "{:#}",
        store
            .open_unlocked(&cli_generated_v1_fixture_passphrase())
            .unwrap_err()
    );
    assert!(error.contains("vault KDF parameters are invalid"));
    assert!(error.contains("memory cost"));
    assert!(!error.contains("wrapped vault key nonce is invalid"));
    assert!(!error.contains("vault state nonce is invalid"));
}

#[test]
fn cli_generated_v1_fixture_decodes_wrapped_payload_before_state_payload() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&store);
    let mut file = cli_generated_v1_fixture_file();
    file["wrapped_dek_nonce_b64"] = serde_json::json!("not valid base64");
    file["state_nonce_b64"] = serde_json::json!("also not valid base64");
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();

    let error = format!(
        "{:#}",
        store
            .open_unlocked(&cli_generated_v1_fixture_passphrase())
            .unwrap_err()
    );
    assert!(error.contains("wrapped vault key nonce is invalid"));
    assert!(!error.contains("vault state nonce is invalid"));
}

#[test]
fn version_one_fixture_remains_readable_and_uses_the_original_state_shape() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    init_v1(&store, &passphrase());

    store
        .set_secret(
            &passphrase(),
            "Production/RESTIC_PASSWORD",
            SecretBytes::new(b"legacy-secret-value".to_vec()),
        )
        .unwrap();

    let file: VaultFile = serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();
    assert_eq!(file.header.version, V1_FORMAT_VERSION);
    let state = decrypt_state_for_test(&file, &passphrase());
    assert!(
        !state
            .windows(b"\"kind\"".len())
            .any(|bytes| bytes == b"\"kind\"")
    );

    let reopened = store.open_unlocked(&passphrase()).unwrap();
    assert_eq!(
        reopened
            .secret_value(&SecretName::parse("Production/RESTIC_PASSWORD").unwrap())
            .unwrap()
            .as_slice(),
        b"legacy-secret-value"
    );
    assert_eq!(
        reopened
            .list_fields()
            .into_iter()
            .map(|record| record.kind)
            .collect::<Vec<_>>(),
        vec![FieldKind::Concealed]
    );
}

#[test]
fn version_one_state_ignores_stray_kind_values_and_treats_every_entry_as_concealed() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    init_v1(&store, &passphrase());
    store
        .set_secret(
            &passphrase(),
            "Production/RESTIC_PASSWORD",
            SecretBytes::new(b"legacy-secret-value".to_vec()),
        )
        .unwrap();

    for kind in [
        serde_json::json!("text"),
        serde_json::json!("future"),
        serde_json::Value::Null,
    ] {
        rewrite_single_entry_kind_for_test(
            &store,
            &passphrase(),
            "Production/RESTIC_PASSWORD",
            kind,
        );
        let opened = store.open_unlocked(&passphrase()).unwrap();
        assert_eq!(
            opened.state.secrets["Production/RESTIC_PASSWORD"].kind,
            FieldKind::Concealed
        );
    }
}

#[test]
fn version_two_state_rejects_unknown_field_kind() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    store
        .set_secret(
            &passphrase(),
            "Production/RESTIC_PASSWORD",
            SecretBytes::new(b"current-secret-value".to_vec()),
        )
        .unwrap();
    rewrite_single_entry_kind_for_test(
        &store,
        &passphrase(),
        "Production/RESTIC_PASSWORD",
        serde_json::json!("future"),
    );

    let error = store.open_unlocked(&passphrase()).unwrap_err().to_string();
    assert!(error.contains("failed to parse vault state"));
}

#[test]
fn explicit_migration_reseals_version_one_under_version_two_aad() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    init_v1(&store, &passphrase());
    store
        .set_secret(
            &passphrase(),
            "Production/RESTIC_PASSWORD",
            SecretBytes::new(b"legacy-secret-value".to_vec()),
        )
        .unwrap();
    let before_text = store.read_vault_text().unwrap().unwrap();
    let before: VaultFile = serde_json::from_str(&before_text).unwrap();

    let migration = store.migrate(&passphrase(), FORMAT_VERSION).unwrap();
    assert_eq!(
        migration,
        VaultMigration {
            from_version: V1_FORMAT_VERSION,
            to_version: FORMAT_VERSION,
            changed: true,
        }
    );

    let after_text = store.read_vault_text().unwrap().unwrap();
    let after: VaultFile = serde_json::from_str(&after_text).unwrap();
    assert_eq!(after.header.version, FORMAT_VERSION);
    assert_eq!(after.header.vault_id, before.header.vault_id);
    assert_eq!(after.header.created_at_ms, before.header.created_at_ms);
    assert_eq!(after.header.salt_b64, before.header.salt_b64);
    assert_eq!(after.header.kdf.algorithm, before.header.kdf.algorithm);
    assert_eq!(after.header.kdf.memory_kib, before.header.kdf.memory_kib);
    assert_eq!(after.header.kdf.iterations, before.header.kdf.iterations);
    assert_eq!(after.header.kdf.parallelism, before.header.kdf.parallelism);
    assert_eq!(after.header.kdf.output_len, before.header.kdf.output_len);
    assert_ne!(after.wrapped_dek_nonce_b64, before.wrapped_dek_nonce_b64);
    assert_ne!(after.state_nonce_b64, before.state_nonce_b64);

    let state = decrypt_state_for_test(&after, &passphrase());
    assert!(
        state
            .windows(b"\"kind\":\"concealed\"".len())
            .any(|bytes| bytes == b"\"kind\":\"concealed\"")
    );
    let reopened = store.open_unlocked(&passphrase()).unwrap();
    assert_eq!(
        reopened
            .secret_value(&SecretName::parse("Production/RESTIC_PASSWORD").unwrap())
            .unwrap()
            .as_slice(),
        b"legacy-secret-value"
    );

    let old_state_nonce =
        decode_b64_array::<NONCE_LEN>("vault state nonce", &before.state_nonce_b64).unwrap();
    let old_state = B64.decode(&before.state_b64).unwrap();
    let salt = decode_b64_array::<SALT_LEN>("vault salt", &before.header.salt_b64).unwrap();
    let wrap_key = derive_wrap_key(&passphrase(), &salt, &before.header.kdf).unwrap();
    let old_wrapped_nonce =
        decode_b64_array::<NONCE_LEN>("wrapped vault key nonce", &before.wrapped_dek_nonce_b64)
            .unwrap();
    let old_wrapped = B64.decode(&before.wrapped_dek_b64).unwrap();
    let old_dek_plaintext = open(
        &wrap_key,
        &old_wrapped_nonce,
        &payload_aad(&before.header, AeadRole::WrappedDek),
        &old_wrapped,
    )
    .unwrap();
    let old_dek = Zeroizing::new(
        crate::crypto::decode_array::<KEY_LEN>("vault key", &old_dek_plaintext).unwrap(),
    );
    let mut v2_header_for_v1_ciphertext = before.header;
    v2_header_for_v1_ciphertext.version = FORMAT_VERSION;
    assert!(
        open(
            &old_dek,
            &old_state_nonce,
            &payload_aad(&v2_header_for_v1_ciphertext, AeadRole::State),
            &old_state,
        )
        .is_err()
    );

    let audit = store.read_audit_text().unwrap().unwrap();
    assert!(audit.contains("\"action\":\"vault_format_migrate\""));
    assert!(audit.contains("\"from_version\":1"));
    assert!(audit.contains("\"to_version\":2"));
    assert!(!audit.contains("legacy-secret-value"));

    let migration_again = store.migrate(&passphrase(), FORMAT_VERSION).unwrap();
    assert!(!migration_again.changed);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), after_text);
}
