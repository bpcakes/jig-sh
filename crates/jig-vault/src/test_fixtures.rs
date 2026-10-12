//! Frozen generic vault fixtures shared by unit tests.
//!
//! The version 2 fixture was captured from the unmodified version 2
//! implementation in disposable storage with a test-only passphrase. Never
//! regenerate it with newer code: its purpose is to prove that bytes written
//! by released readers remain compatible. The version 3 fixture was captured
//! the same way from the first version 3 implementation and freezes that wire
//! contract for later restore and recovery work.

use secrecy::SecretString;

use crate::store::VaultStore;

pub(crate) const GENERATED_V2_VAULT_JSON: &str =
    include_str!("../tests/fixtures/generated-v2/vault.json");
pub(crate) const GENERATED_V2_AUDIT_JSONL: &str =
    include_str!("../tests/fixtures/generated-v2/audit.jsonl");
pub(crate) const GENERATED_V2_BACKUP: &[u8] =
    include_bytes!("../tests/fixtures/generated-v2/backup.jigvault");
pub(crate) const GENERATED_V2_PASSPHRASE: &str = "fixture-v2-pass";
pub(crate) const GENERATED_V2_VAULT_ID: &str = "01M48RP93PRPX1WAGHTYAHXGHZ";
pub(crate) const GENERATED_V2_CONCEALED_REFERENCE: &str = "jig://ExampleProject/API_TOKEN";
pub(crate) const GENERATED_V2_CONCEALED_VALUE: &[u8] = b"example-v2-api-token-7c41";
pub(crate) const GENERATED_V2_TEXT_REFERENCE: &str = "jig://ExampleProject/USERNAME";
pub(crate) const GENERATED_V2_TEXT_VALUE: &[u8] = b"example-user";
pub(crate) const GENERATED_V2_LEGACY_NAME: &str = "example_legacy_token";
pub(crate) const GENERATED_V2_LEGACY_VALUE: &[u8] = b"example-v2-legacy-value-91d3";
pub(crate) const GENERATED_V2_AUDIT_EVENTS: usize = 6;
pub(crate) const GENERATED_V2_AUDIT_MAC: &str =
    "0333a9eae5357175cebff8c647c37b6b56ae2ca20100e96d35d4df40fcd51ac1";
/// The archive embeds the audit log as it was before `backup_finish`.
pub(crate) const GENERATED_V2_BACKUP_AUDIT_EVENTS: usize = 5;

pub(crate) fn generated_v2_passphrase() -> SecretString {
    SecretString::from(GENERATED_V2_PASSPHRASE.to_owned())
}

pub(crate) fn install_generated_v2_fixture(store: &VaultStore) {
    install_fixture(store, GENERATED_V2_VAULT_JSON, GENERATED_V2_AUDIT_JSONL);
}

fn install_fixture(store: &VaultStore, vault_json: &str, audit_jsonl: &str) {
    store.write_vault_text(vault_json).unwrap();
    std::fs::write(store.audit_path(), audit_jsonl).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(store.audit_path(), std::fs::Permissions::from_mode(0o600))
            .unwrap();
    }
}

pub(crate) const GENERATED_V3_VAULT_JSON: &str =
    include_str!("../tests/fixtures/generated-v3/vault.json");
pub(crate) const GENERATED_V3_AUDIT_JSONL: &str =
    include_str!("../tests/fixtures/generated-v3/audit.jsonl");
pub(crate) const GENERATED_V3_BACKUP: &[u8] =
    include_bytes!("../tests/fixtures/generated-v3/backup.jigvault");
pub(crate) const GENERATED_V3_PASSPHRASE: &str = "fixture-v3-passphrase";
pub(crate) const GENERATED_V3_VAULT_ID: &str = "01M48T2RCNHCQT45TTQ6EV177B";
pub(crate) const GENERATED_V3_GENERATION: u64 = 4;
/// MAC of the `secret_set` event that committed generation 4. The later
/// backup events are audit-only and do not move the state anchor.
pub(crate) const GENERATED_V3_MUTATION_MAC: &str =
    "d22dd73feba914f339b60ebe16d769266cb54b202ca9e3fe2c0fa2877db79b4c";
pub(crate) const GENERATED_V3_AUDIT_MAC: &str =
    "ecf6aab08dea6c9d48c5f13795050a1c766aac65783f25deeb126a66aaa1093b";
pub(crate) const GENERATED_V3_AUDIT_EVENTS: usize = 6;
pub(crate) const GENERATED_V3_BACKUP_AUDIT_EVENTS: usize = 5;
pub(crate) const GENERATED_V3_CONCEALED_REFERENCE: &str = "jig://ExampleProject/API_TOKEN";
pub(crate) const GENERATED_V3_CONCEALED_VALUE: &[u8] = b"example-v3-api-token-2be9";

pub(crate) fn generated_v3_passphrase() -> SecretString {
    SecretString::from(GENERATED_V3_PASSPHRASE.to_owned())
}

pub(crate) fn install_generated_v3_fixture(store: &VaultStore) {
    install_fixture(store, GENERATED_V3_VAULT_JSON, GENERATED_V3_AUDIT_JSONL);
}
