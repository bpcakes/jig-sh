use super::*;
use crate::{BrokeredEnv, ExecEnvBinding, VaultExec};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::SecretString;
use std::io::{self, Write};
use zeroize::Zeroizing;

#[path = "vault_tests/lifecycle.rs"]
mod lifecycle;
fn passphrase() -> SecretString {
    SecretString::from("correct horse battery staple".to_string())
}

const CLI_GENERATED_V1_VAULT_JSON: &str =
    include_str!("../tests/fixtures/cli-generated-v1/vault.json");
const CLI_GENERATED_V1_AUDIT_JSONL: &str =
    include_str!("../tests/fixtures/cli-generated-v1/audit.jsonl");
const CLI_GENERATED_V1_PASSPHRASE: &str = "fixture-v1-passphrase-only";
const CLI_GENERATED_V1_SECRET_NAME: &str = "Production/RESTIC_PASSWORD";
const CLI_GENERATED_V1_SECRET_VALUE: &[u8] = b"v1-fixture-restic-password-6f2ab1";
const CLI_GENERATED_V1_AUDIT_MAC: &str =
    "3dc428c1d662dca49f7a872b4089ff51ed94f7183e937bb263230b3b2e567ce1";

fn cli_generated_v1_fixture_passphrase() -> SecretString {
    SecretString::from(CLI_GENERATED_V1_PASSPHRASE.to_string())
}

fn install_cli_generated_v1_fixture(store: &VaultStore) {
    store.write_vault_text(CLI_GENERATED_V1_VAULT_JSON).unwrap();
    std::fs::write(store.audit_path(), CLI_GENERATED_V1_AUDIT_JSONL).unwrap();
}

fn cli_generated_v1_fixture_file() -> serde_json::Value {
    serde_json::from_str(CLI_GENERATED_V1_VAULT_JSON).unwrap()
}

fn audit_events(store: &VaultStore) -> Vec<AuditEvent> {
    store
        .read_audit_text()
        .unwrap()
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn init_v1(store: &VaultStore, passphrase: &SecretString) {
    store
        .with_lock(|| {
            let envelope = NewVaultEnvelope::seal_v1(passphrase, now_ms())?;
            AuditEvent::append_unlocked(
                store,
                envelope.audit_key.as_ref(),
                AuditAction::VaultInitialized,
                serde_json::json!({
                    "vault_id": envelope.file.header.vault_id,
                }),
            )?;
            store.write_vault_text_unlocked(&envelope.file_text)?;
            Ok(())
        })
        .unwrap();
}

fn decrypt_state_for_test(file: &VaultFile, passphrase: &SecretString) -> Zeroizing<Vec<u8>> {
    let salt = decode_b64_array::<SALT_LEN>("vault salt", &file.header.salt_b64).unwrap();
    let wrap_key = derive_wrap_key(passphrase, &salt, &file.header.kdf).unwrap();
    let wrapped_dek_nonce =
        decode_b64_array::<NONCE_LEN>("wrapped vault key nonce", &file.wrapped_dek_nonce_b64)
            .unwrap();
    let wrapped_dek = B64.decode(&file.wrapped_dek_b64).unwrap();
    let dek_plaintext = open(
        &wrap_key,
        &wrapped_dek_nonce,
        &payload_aad(&file.header, AeadRole::WrappedDek),
        &wrapped_dek,
    )
    .unwrap();
    let dek = Zeroizing::new(
        crate::crypto::decode_array::<KEY_LEN>("vault key", &dek_plaintext).unwrap(),
    );
    let state_nonce =
        decode_b64_array::<NONCE_LEN>("vault state nonce", &file.state_nonce_b64).unwrap();
    let state = B64.decode(&file.state_b64).unwrap();
    open(
        &dek,
        &state_nonce,
        &payload_aad(&file.header, AeadRole::State),
        &state,
    )
    .unwrap()
}

fn rewrite_single_entry_kind_for_test(
    store: &VaultStore,
    passphrase: &SecretString,
    name: &str,
    kind: serde_json::Value,
) {
    let mut file: VaultFile =
        serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();
    let state = decrypt_state_for_test(&file, passphrase);
    let mut state: serde_json::Value = serde_json::from_slice(&state).unwrap();
    state["secrets"][name]["kind"] = kind;
    let state_plaintext = Zeroizing::new(serde_json::to_vec(&state).unwrap());
    let vault = store.open_unlocked(passphrase).unwrap();
    let state_nonce = crate::crypto::random_array::<NONCE_LEN>().unwrap();
    let encrypted_state = crate::crypto::seal(
        &vault.dek,
        &state_nonce,
        &payload_aad(&file.header, AeadRole::State),
        &state_plaintext,
    )
    .unwrap();
    file.state_nonce_b64 = B64.encode(state_nonce);
    file.state_b64 = B64.encode(encrypted_state);
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();
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
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
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
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
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

#[test]
fn direct_field_read_writes_exact_binary_bytes_and_terminalizes_success() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/BINARY").unwrap();
    let value = b"\xff\0binary-value";
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(value.to_vec()),
        )
        .unwrap();

    let before_count = audit_events(&vault.store).len();
    let mut output = Vec::new();
    let result = vault
        .read_field_to(&passphrase(), reference.clone(), &mut output)
        .unwrap();
    assert_eq!(output, value);
    assert_eq!(result.bytes_written, value.len());

    let events = audit_events(&vault.store);
    assert_eq!(events.len(), before_count + 2);
    let start = &events[events.len() - 2];
    let finish = &events[events.len() - 1];
    assert_eq!(start.action, "field_read_start");
    assert_eq!(finish.action, "field_read_finish");
    assert_eq!(
        start.details["operation_id"],
        finish.details["operation_id"]
    );
    assert_eq!(start.details["reference"], reference.to_string());
    assert_eq!(finish.details["sink"], "stream");
    assert_eq!(finish.details["bytes_written"], value.len());
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    assert!(!audit.contains("binary-value"));
}

#[test]
fn template_injection_resolves_all_references_deduplicates_and_preserves_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    vault
        .apply_field_batch(
            &passphrase(),
            vec![
                FieldMutation::set(
                    VaultReference::parse("jig://Production/TOKEN").unwrap(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"hidden-value".to_vec()),
                ),
                FieldMutation::set(
                    VaultReference::parse("jig://Production/FLAG").unwrap(),
                    FieldKind::Text,
                    SecretBytes::new(vec![0xff, 0x00, b'0']),
                ),
            ],
        )
        .unwrap();

    let template = InjectionTemplate::parse(SecretBytes::new(
        b"A={{ jig://Production/TOKEN }}\0B={{jig://Production/FLAG}} C={{ jig://Production/TOKEN }}"
            .to_vec(),
    ))
    .unwrap();
    let mut output = Vec::new();
    vault
        .inject_template_to(&passphrase(), template, &mut output)
        .unwrap();
    assert_eq!(output, b"A=hidden-value\0B=\xff\x000 C=hidden-value");

    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let finish = &events[events.len() - 1];
    assert_eq!(start.action, "template_inject_start");
    assert_eq!(start.details["reference_count"], 2);
    assert_eq!(start.details["references"].as_array().unwrap().len(), 2);
    assert_eq!(finish.action, "template_inject_finish");
    assert_eq!(
        start.details["operation_id"],
        finish.details["operation_id"]
    );
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    assert!(!audit.contains("hidden-value"));
}

#[test]
fn missing_late_template_reference_records_failure_without_output() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let present = VaultReference::parse("jig://Production/PRESENT").unwrap();
    vault
        .set_field(
            &passphrase(),
            present,
            FieldKind::Concealed,
            SecretBytes::new(b"present-value".to_vec()),
        )
        .unwrap();
    let template = InjectionTemplate::parse(SecretBytes::new(
        b"{{jig://Production/PRESENT}}/{{jig://Production/MISSING}}".to_vec(),
    ))
    .unwrap();

    let mut output = Vec::new();
    let error = vault
        .inject_template_to(&passphrase(), template, &mut output)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::NotFound);
    assert!(output.is_empty());
    assert!(!error.to_string().contains("present-value"));
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = &events[events.len() - 1];
    assert_eq!(start.action, "template_inject_start");
    assert_eq!(failed.action, "template_inject_failed");
    assert_eq!(failed.details["stage"], "resolve");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
}

struct PrefixThenFailWriter {
    output: Vec<u8>,
    remaining: usize,
}

impl Write for PrefixThenFailWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "writer-failure-sentinel",
            ));
        }
        let len = bytes.len().min(self.remaining);
        self.output.extend_from_slice(&bytes[..len]);
        self.remaining -= len;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn partial_stream_failure_records_failed_terminal_event_without_value_leak() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/TOKEN").unwrap();
    let value = b"writer-failure-sentinel";
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(value.to_vec()),
        )
        .unwrap();
    let mut writer = PrefixThenFailWriter {
        output: Vec::new(),
        remaining: 3,
    };

    let error = vault
        .read_field_to(&passphrase(), reference, &mut writer)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);
    assert_eq!(writer.output, &value[..3]);
    assert!(!format!("{error:#}").contains("writer-failure-sentinel"));
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = &events[events.len() - 1];
    assert_eq!(start.action, "field_read_start");
    assert_eq!(failed.action, "field_read_failed");
    assert_eq!(failed.details["stage"], "sink");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
    assert!(
        !vault
            .store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .contains("writer-failure-sentinel")
    );
}

#[test]
fn tampered_audit_rejects_reveal_start_before_any_value_is_prepared() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/TOKEN").unwrap();
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(b"audit-failure-sentinel".to_vec()),
        )
        .unwrap();
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    let tampered = audit.replacen("field_batch_apply", "field_batch_tamper", 1);
    assert_ne!(tampered, audit);
    std::fs::write(vault.store.audit_path(), &tampered).unwrap();

    let mut output = Vec::new();
    let error = vault
        .read_field_to(&passphrase(), reference, &mut output)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(output.is_empty());
    assert!(!format!("{error:#}").contains("audit-failure-sentinel"));
    assert_eq!(vault.store.read_audit_text().unwrap().unwrap(), tampered);
}

#[test]
fn static_version_one_fixture_supports_controlled_read_and_injection() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&vault.store);
    let passphrase = cli_generated_v1_fixture_passphrase();
    let reference = VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap();

    let mut read_output = Vec::new();
    vault
        .read_field_to(&passphrase, reference, &mut read_output)
        .unwrap();
    assert_eq!(read_output, CLI_GENERATED_V1_SECRET_VALUE);

    let template = InjectionTemplate::parse(SecretBytes::new(
        b"before={{jig://Production/RESTIC_PASSWORD}}:after".to_vec(),
    ))
    .unwrap();
    let mut inject_output = Vec::new();
    vault
        .inject_template_to(&passphrase, template, &mut inject_output)
        .unwrap();
    let mut expected = b"before=".to_vec();
    expected.extend_from_slice(CLI_GENERATED_V1_SECRET_VALUE);
    expected.extend_from_slice(b":after");
    assert_eq!(inject_output, expected);
    assert_eq!(vault.verify_audit(&passphrase).unwrap().event_count, 6);
}

#[test]
fn rendered_output_bound_records_template_failure() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/LARGE").unwrap();
    vault
        .set_field(
            &passphrase(),
            reference,
            FieldKind::Concealed,
            SecretBytes::new(vec![b'x'; MAX_SECRET_VALUE_LEN]),
        )
        .unwrap();
    let template = InjectionTemplate::parse(SecretBytes::new(
        "{{jig://Production/LARGE}}".repeat(17).into_bytes(),
    ))
    .unwrap();

    let mut output = Vec::new();
    let error = vault
        .inject_template_to(&passphrase(), template, &mut output)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(output.is_empty());
    assert!(error.to_string().contains("rendered template exceeds"));
    let events = audit_events(&vault.store);
    assert_eq!(events[events.len() - 2].action, "template_inject_start");
    assert_eq!(events.last().unwrap().action, "template_inject_failed");
    assert_eq!(events.last().unwrap().details["stage"], "render");
}

fn exec_var(name: &str) -> EnvVarName {
    EnvVarName::parse(name).unwrap()
}

#[test]
fn exec_preparation_resolves_fields_and_builds_concealed_only_redaction() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let concealed = VaultReference::parse("jig://Production/TOKEN").unwrap();
    let text = VaultReference::parse("jig://Production/FEATURE_FLAG").unwrap();
    vault
        .apply_field_batch(
            &passphrase(),
            vec![
                FieldMutation::set(
                    concealed.clone(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"secret-value".to_vec()),
                ),
                FieldMutation::set(
                    text.clone(),
                    FieldKind::Text,
                    SecretBytes::new(b"false".to_vec()),
                ),
            ],
        )
        .unwrap();
    let request = VaultExec::new(
        vec![
            "argv-secret-sentinel".into(),
            "argument-value-sentinel".into(),
        ],
        vec![
            ExecEnvBinding::literal(
                exec_var("LITERAL"),
                SecretBytes::new(b"literal-value-sentinel".to_vec()),
            )
            .unwrap(),
            ExecEnvBinding::field(exec_var("TOKEN"), concealed),
            ExecEnvBinding::field(exec_var("FEATURE_FLAG"), text),
        ],
    )
    .unwrap();

    let prepared = vault.store.prepare_exec(&passphrase(), request).unwrap();
    assert_eq!(prepared.command.len(), 2);
    assert_eq!(prepared.env.len(), 3);
    assert_eq!(prepared.env[0].field_kind, None);
    assert_eq!(prepared.env[0].value.as_str(), "literal-value-sentinel");
    assert_eq!(prepared.env[1].field_kind, Some(FieldKind::Concealed));
    assert_eq!(prepared.env[1].value.as_str(), "secret-value");
    assert_eq!(prepared.env[2].field_kind, Some(FieldKind::Text));
    assert_eq!(prepared.env[2].value.as_str(), "false");

    let mut redactor = prepared.redactor.independent_stream();
    let mut output = Vec::new();
    redactor
        .push_chunk(
            b"raw=secret-value b64=c2VjcmV0LXZhbHVl text=false literal=literal-value-sentinel",
            &mut output,
        )
        .unwrap();
    redactor.finish(&mut output).unwrap();
    assert_eq!(
        output,
        b"raw=[REDACTED] b64=[REDACTED] text=false literal=literal-value-sentinel"
    );

    let events = audit_events(&vault.store);
    let start = events.last().unwrap();
    assert_eq!(start.action, "exec_start");
    assert_eq!(start.details["operation_id"], prepared.operation_id);
    assert_eq!(start.details["argument_count"], 2);
    assert_eq!(start.details["binding_count"], 3);
    assert_eq!(start.details["literal_binding_count"], 1);
    assert_eq!(start.details["field_binding_count"], 2);
    assert_eq!(start.details["field_bindings"][0]["var"], "TOKEN");
    assert_eq!(
        start.details["field_bindings"][0]["reference"],
        "jig://Production/TOKEN"
    );
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    for forbidden in [
        "argv-secret-sentinel",
        "argument-value-sentinel",
        "literal-value-sentinel",
        "secret-value",
        "c2VjcmV0LXZhbHVl",
    ] {
        assert!(!audit.contains(forbidden), "audit leaked {forbidden}");
    }

    prepared.record_finish(0, None).unwrap();
    let events = audit_events(&vault.store);
    let finish = events.last().unwrap();
    assert_eq!(finish.action, "exec_finish");
    assert_eq!(
        finish.details["operation_id"],
        start.details["operation_id"]
    );
    assert_eq!(finish.details["exit_status"], 0);
    assert!(finish.details["exit_signal"].is_null());
}

#[test]
fn exec_preparation_missing_field_records_value_free_failed_event() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let request = VaultExec::new(
        vec!["missing-field-command-sentinel".into()],
        vec![ExecEnvBinding::field(
            exec_var("TOKEN"),
            VaultReference::parse("jig://Production/MISSING").unwrap(),
        )],
    )
    .unwrap();

    let error = vault
        .store
        .prepare_exec(&passphrase(), request)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::NotFound);
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = events.last().unwrap();
    assert_eq!(start.action, "exec_start");
    assert_eq!(failed.action, "exec_failed");
    assert_eq!(failed.details["stage"], "resolve");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
    assert_eq!(failed.details["error"], "vault exec failed");
    assert!(
        !vault
            .store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .contains("missing-field-command-sentinel")
    );
}

#[test]
fn exec_preparation_invalid_field_bytes_record_resolve_failure() {
    for (field, value, requirement) in [
        ("BINARY", vec![b's', b'e', b'c', 0xff], "UTF-8"),
        ("NUL", b"sec\0ret".to_vec(), "NUL"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
        vault.init(&passphrase()).unwrap();
        let reference = VaultReference::parse(&format!("jig://Production/{field}")).unwrap();
        vault
            .set_field(
                &passphrase(),
                reference.clone(),
                FieldKind::Concealed,
                SecretBytes::new(value),
            )
            .unwrap();
        let request = VaultExec::new(
            vec!["command".into()],
            vec![ExecEnvBinding::field(exec_var("VALUE"), reference)],
        )
        .unwrap();

        let error = vault
            .store
            .prepare_exec(&passphrase(), request)
            .unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
        assert!(error.to_string().contains(requirement));
        let events = audit_events(&vault.store);
        let start = &events[events.len() - 2];
        let failed = events.last().unwrap();
        assert_eq!(start.action, "exec_start");
        assert_eq!(failed.action, "exec_failed");
        assert_eq!(failed.details["stage"], "resolve");
        assert_eq!(
            start.details["operation_id"],
            failed.details["operation_id"]
        );
    }
}

#[test]
fn exec_preparation_redaction_bound_records_redaction_failure() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/LARGE_TOKEN").unwrap();
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(vec![b'x'; crate::exec::MAX_EXEC_CONCEALED_VALUE_LEN + 1]),
        )
        .unwrap();
    let request = VaultExec::new(
        vec!["command".into()],
        vec![ExecEnvBinding::field(exec_var("TOKEN"), reference)],
    )
    .unwrap();

    let error = vault
        .store
        .prepare_exec(&passphrase(), request)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = events.last().unwrap();
    assert_eq!(start.action, "exec_start");
    assert_eq!(failed.action, "exec_failed");
    assert_eq!(failed.details["stage"], "redaction");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
}

#[test]
fn exec_spawn_failure_records_value_free_terminal_event() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let command_sentinel = "jig-vault-missing-command-secret-sentinel";
    let request = VaultExec::new(vec![command_sentinel.into()], Vec::new()).unwrap();

    let error = vault.exec(&passphrase(), request).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Process);
    assert!(!error.to_string().contains(command_sentinel));
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = events.last().unwrap();
    assert_eq!(start.action, "exec_start");
    assert_eq!(failed.action, "exec_failed");
    assert_eq!(failed.details["stage"], "spawn");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
    assert!(
        !vault
            .store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .contains(command_sentinel)
    );
}

fn import_set(reference: &str, kind: FieldKind, value: &[u8]) -> FieldMutation {
    FieldMutation::set(
        VaultReference::parse(reference).unwrap(),
        kind,
        SecretBytes::new(value.to_vec()),
    )
}

#[test]
fn onepassword_import_is_atomic_replace_explicit_and_value_free() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let concealed = "import-secret-value-sentinel";
    let text = "import-text-value-sentinel";

    let imported = vault
        .import_fields(
            &passphrase(),
            vec![
                import_set(
                    "jig://Production/TOKEN",
                    FieldKind::Concealed,
                    concealed.as_bytes(),
                ),
                import_set("jig://Production/FLAG", FieldKind::Text, text.as_bytes()),
            ],
            false,
        )
        .unwrap();
    assert_eq!(imported.changed.len(), 2);
    assert!(imported.removed.is_empty());
    let events = audit_events(&vault.store);
    let import = events.last().unwrap();
    assert_eq!(import.action, "onepassword_import");
    assert_eq!(import.details["field_count"], 2);
    assert_eq!(import.details["concealed_count"], 1);
    assert_eq!(import.details["text_count"], 1);
    assert_eq!(import.details["fields"][0]["kind"], "concealed");
    assert_eq!(import.details["fields"][1]["kind"], "text");
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    assert!(!audit.contains(concealed));
    assert!(!audit.contains(text));

    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = audit;
    let collision = vault
        .import_fields(
            &passphrase(),
            vec![
                import_set(
                    "jig://Production/TOKEN",
                    FieldKind::Concealed,
                    b"replacement-secret-sentinel",
                ),
                import_set(
                    "jig://Production/NEW_FIELD",
                    FieldKind::Text,
                    b"new-text-sentinel",
                ),
            ],
            false,
        )
        .unwrap_err();
    assert_eq!(collision.kind(), VaultErrorKind::AlreadyExists);
    assert!(collision.to_string().contains("jig://Production/TOKEN"));
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        before_vault
    );
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );

    vault
        .import_fields(
            &passphrase(),
            vec![
                import_set(
                    "jig://Production/TOKEN",
                    FieldKind::Concealed,
                    b"replacement-secret-sentinel",
                ),
                import_set(
                    "jig://Production/NEW_FIELD",
                    FieldKind::Text,
                    b"new-text-sentinel",
                ),
            ],
            true,
        )
        .unwrap();
    let fields = vault.list_fields(&passphrase()).unwrap();
    assert_eq!(fields.len(), 3);
    assert_eq!(
        audit_events(&vault.store)
            .iter()
            .filter(|event| event.action == "onepassword_import")
            .count(),
        2
    );
    let opened = vault.store.open_unlocked(&passphrase()).unwrap();
    assert_eq!(
        opened
            .secret_value(&SecretName::parse("Production/TOKEN").unwrap())
            .unwrap()
            .as_slice(),
        b"replacement-secret-sentinel"
    );
}

#[test]
fn onepassword_import_validates_sets_duplicates_and_values_before_writing() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    let duplicate = VaultReference::parse("jig://Production/DUPLICATE").unwrap();
    let duplicate_error = vault
        .import_fields(
            &passphrase(),
            vec![
                FieldMutation::set(
                    duplicate.clone(),
                    FieldKind::Text,
                    SecretBytes::new(b"first".to_vec()),
                ),
                FieldMutation::set(
                    duplicate,
                    FieldKind::Text,
                    SecretBytes::new(b"second".to_vec()),
                ),
            ],
            false,
        )
        .unwrap_err();
    assert_eq!(duplicate_error.kind(), VaultErrorKind::InvalidInput);
    assert!(duplicate_error.to_string().contains("duplicate reference"));

    let remove_error = vault
        .import_fields(
            &passphrase(),
            vec![FieldMutation::remove(
                VaultReference::parse("jig://Production/REMOVE").unwrap(),
            )],
            true,
        )
        .unwrap_err();
    assert_eq!(remove_error.kind(), VaultErrorKind::InvalidInput);
    assert!(remove_error.to_string().contains("only field set"));

    let short_error = vault
        .import_fields(
            &passphrase(),
            vec![import_set(
                "jig://Production/SHORT",
                FieldKind::Concealed,
                b"bad",
            )],
            false,
        )
        .unwrap_err();
    assert_eq!(short_error.kind(), VaultErrorKind::InvalidInput);
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        before_vault
    );
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );
}

#[test]
fn import_preview_is_read_only_audit_verified_and_version_two_only() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    vault
        .import_fields(
            &passphrase(),
            vec![import_set(
                "jig://Production/EXISTING",
                FieldKind::Text,
                b"false",
            )],
            false,
        )
        .unwrap();
    let existing = VaultReference::parse("jig://Production/EXISTING").unwrap();
    let new = VaultReference::parse("jig://Production/NEW").unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    assert_eq!(
        vault
            .preview_import_fields(&passphrase(), &[existing.clone(), new])
            .unwrap(),
        vec![true, false]
    );
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );
    let duplicate = vault
        .preview_import_fields(&passphrase(), &[existing.clone(), existing])
        .unwrap_err();
    assert_eq!(duplicate.kind(), VaultErrorKind::InvalidInput);
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );

    let v1 = Vault::resolve(Some(temp.path().join("v1"))).unwrap();
    init_v1(&v1.store, &passphrase());
    let v1_error = v1
        .preview_import_fields(
            &passphrase(),
            &[VaultReference::parse("jig://Production/FIELD").unwrap()],
        )
        .unwrap_err();
    assert_eq!(v1_error.kind(), VaultErrorKind::InvalidInput);
    assert!(
        v1_error
            .to_string()
            .contains("does not support field mutations")
    );
    let before_v1_vault = v1.store.read_vault_text().unwrap().unwrap();
    let before_v1_audit = v1.store.read_audit_text().unwrap().unwrap();
    let v1_import_error = v1
        .import_fields(
            &passphrase(),
            vec![import_set(
                "jig://Production/FIELD",
                FieldKind::Text,
                b"value",
            )],
            false,
        )
        .unwrap_err();
    assert_eq!(v1_import_error.kind(), VaultErrorKind::InvalidInput);
    assert_eq!(
        v1.store.read_vault_text().unwrap().unwrap(),
        before_v1_vault
    );
    assert_eq!(
        v1.store.read_audit_text().unwrap().unwrap(),
        before_v1_audit
    );

    let mut tampered = before_audit;
    tampered.push_str("not json\n");
    std::fs::write(vault.store.audit_path(), &tampered).unwrap();
    let audit_error = vault
        .preview_import_fields(
            &passphrase(),
            &[VaultReference::parse("jig://Production/OTHER").unwrap()],
        )
        .unwrap_err();
    assert_eq!(audit_error.kind(), VaultErrorKind::AuditTampered);
    assert_eq!(vault.store.read_audit_text().unwrap().unwrap(), tampered);
}

#[test]
fn onepassword_import_save_fault_leaves_intent_ahead_and_retry_converges() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let before_vault = store.read_vault_text().unwrap().unwrap();
    store.fail_next_vault_write_for_test();

    let error = store
        .import_fields(
            &passphrase(),
            vec![import_set(
                "jig://Production/TOKEN",
                FieldKind::Concealed,
                b"fault-secret-sentinel",
            )],
            false,
        )
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert!(store.list_fields(&passphrase()).unwrap().is_empty());
    assert_eq!(
        audit_events(&store).last().unwrap().action,
        "onepassword_import"
    );
    assert!(
        !store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .contains("fault-secret-sentinel")
    );

    store
        .import_fields(
            &passphrase(),
            vec![import_set(
                "jig://Production/TOKEN",
                FieldKind::Concealed,
                b"fault-secret-sentinel",
            )],
            false,
        )
        .unwrap();
    assert_eq!(store.list_fields(&passphrase()).unwrap().len(), 1);
    assert_eq!(
        audit_events(&store)
            .iter()
            .filter(|event| event.action == "onepassword_import")
            .count(),
        2
    );
}

#[test]
fn oversized_onepassword_import_fails_before_audit_or_state_write() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
    store.init(&passphrase()).unwrap();
    let before_vault = store.read_vault_text().unwrap().unwrap();
    let before_audit = store.read_audit_text().unwrap().unwrap();
    let mutations = (0..12)
        .map(|index| {
            FieldMutation::set(
                VaultReference::parse(&format!("jig://Production/IMPORT_LARGE_{index}")).unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(vec![b'x'; MAX_SECRET_VALUE_LEN]),
            )
        })
        .collect();

    let error = store
        .import_fields(&passphrase(), mutations, false)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("too large to save safely"));
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), before_audit);
}

#[test]
fn concurrent_no_replace_imports_recheck_collisions_under_the_vault_lock() {
    use std::sync::{Arc, Barrier};

    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let mut threads = Vec::new();
    for value in [b"race-value-one".as_slice(), b"race-value-two".as_slice()] {
        let vault = vault.clone();
        let barrier = barrier.clone();
        let value = value.to_vec();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            vault.import_fields(
                &passphrase(),
                vec![import_set(
                    "jig://Production/RACE",
                    FieldKind::Concealed,
                    &value,
                )],
                false,
            )
        }));
    }
    barrier.wait();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .filter(|error| error.kind() == VaultErrorKind::AlreadyExists)
            .count(),
        1
    );
    assert_eq!(
        audit_events(&vault.store)
            .iter()
            .filter(|event| event.action == "onepassword_import")
            .count(),
        1
    );
}
