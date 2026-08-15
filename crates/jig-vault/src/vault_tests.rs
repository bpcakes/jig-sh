use super::*;
#[cfg(unix)]
use crate::BrokeredEnv;
use crate::{ExecEnvBinding, VaultExec};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::SecretString;
use std::io::{self, Write};
use zeroize::Zeroizing;

#[path = "vault_tests/exec.rs"]
mod exec;
#[path = "vault_tests/import.rs"]
mod import;
#[path = "vault_tests/legacy.rs"]
mod legacy;
#[path = "vault_tests/lifecycle.rs"]
mod lifecycle;
#[path = "vault_tests/management.rs"]
mod management;
#[path = "vault_tests/mutations.rs"]
mod mutations;
#[path = "vault_tests/reveal.rs"]
mod reveal;

fn passphrase() -> SecretString {
    SecretString::from("correct horse battery staple".to_string())
}

fn exec_var(name: &str) -> EnvVarName {
    EnvVarName::parse(name).unwrap()
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
