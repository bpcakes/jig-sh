//! Backup compatibility across embedded vault formats 2 and 3.

use super::*;
use crate::test_fixtures::{
    GENERATED_V2_BACKUP, GENERATED_V2_BACKUP_AUDIT_EVENTS, GENERATED_V2_VAULT_ID,
    generated_v2_passphrase, install_generated_v2_fixture,
};

fn embedded_header(version: u32, generation: Option<u64>) -> serde_json::Value {
    let mut vault = syntactically_complete_vault_value();
    vault["header"]["version"] = serde_json::json!(version);
    if let Some(generation) = generation {
        vault["header"]["generation"] = serde_json::json!(generation);
    }
    vault
}

#[test]
fn embedded_v3_headers_require_generation_and_v2_headers_reject_it() {
    let inspect =
        |value: serde_json::Value| inspect_embedded_vault(&serde_json::to_vec(&value).unwrap());
    assert_eq!(
        inspect(embedded_header(V3_FORMAT_VERSION, Some(4)))
            .unwrap()
            .1,
        V3_FORMAT_VERSION
    );
    assert!(inspect(embedded_header(V3_FORMAT_VERSION, None)).is_err());
    assert!(inspect(embedded_header(V3_FORMAT_VERSION, Some(0))).is_err());
    assert!(inspect(embedded_header(V2_FORMAT_VERSION, Some(1))).is_err());
    assert!(inspect(embedded_header(4, Some(1))).is_err());
}

/// Replica of the strict embedded-envelope schema released with format 2.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ReleasedV2EmbeddedVaultFile {
    header: ReleasedV2EmbeddedVaultHeader,
    wrapped_dek_nonce_b64: String,
    wrapped_dek_b64: String,
    state_nonce_b64: String,
    state_b64: String,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ReleasedV2EmbeddedVaultHeader {
    magic: String,
    version: u32,
    vault_id: String,
    created_at_ms: i128,
    kdf: super::super::codec::BackupKdfParams,
    salt_b64: String,
    aead: String,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn decrypt_backup(path: &Path, passphrase: &SecretString) -> payload::DecodedBackupArchive {
    let bytes = Zeroizing::new(fs::read(path).unwrap());
    decrypt_archive(passphrase, parse_archive_bytes(bytes).unwrap()).unwrap()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn private_temp() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn backup_events(root: &Path) -> Vec<crate::audit::AuditEvent> {
    fs::read_to_string(root.join("audit.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn frozen_v2_archive_restores_as_version_two() {
    let temp = private_temp();
    let input = temp.path().join("frozen-v2.backup");
    fs::write(&input, GENERATED_V2_BACKUP).unwrap();
    let target = temp.path().join("restored-v2");
    let request = Vault::preflight_backup_restore(&input, target).unwrap();
    let restored = Vault::restore_backup(&generated_v2_passphrase(), request).unwrap();

    assert_eq!(restored.format_version, V2_FORMAT_VERSION);
    assert_eq!(restored.vault_id, GENERATED_V2_VAULT_ID);
    let status = Vault::status(Some(restored.root.clone())).unwrap();
    assert_eq!(status.format_version, Some(V2_FORMAT_VERSION));
    let vault = Vault::resolve_for_test(Some(restored.root.clone())).unwrap();
    assert_eq!(
        vault.list_fields(&generated_v2_passphrase()).unwrap().len(),
        2
    );
    let audit = vault.verify_audit(&generated_v2_passphrase()).unwrap();
    assert_eq!(audit.event_count, GENERATED_V2_BACKUP_AUDIT_EVENTS + 1);
    let last = backup_events(&restored.root).pop().unwrap();
    assert_eq!(last.action, "backup_restore");
    assert_eq!(last.details["source_format_version"], V2_FORMAT_VERSION);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn backups_record_and_restore_the_actual_source_format() {
    for version in [V2_FORMAT_VERSION, V3_FORMAT_VERSION] {
        let temp = private_temp();
        let source_home = temp.path().join("source");
        let source = Vault::resolve_for_test(Some(source_home.clone())).unwrap();
        let passphrase = if version == V2_FORMAT_VERSION {
            install_generated_v2_fixture(
                &crate::store::VaultStore::resolve_for_test(Some(source_home.clone())).unwrap(),
            );
            generated_v2_passphrase()
        } else {
            source.init(&test_passphrase()).unwrap();
            test_passphrase()
        };
        let source_file = fs::read_to_string(source_home.join("vault.json")).unwrap();
        let output = temp.path().join("source.backup");
        let request = Vault::preflight_backup_create(source_home.clone(), &output, false).unwrap();
        Vault::create_backup(&passphrase, request).unwrap();

        let start = backup_events(&source_home)
            .into_iter()
            .rev()
            .find(|event| event.action == "backup_start")
            .unwrap();
        assert_eq!(start.details["source_format_version"], version);
        let decoded = decrypt_backup(&output, &passphrase);
        assert_eq!(decoded.source_format_version, version);
        assert_eq!(
            inspect_embedded_vault(decoded.vault_bytes()).unwrap().1,
            version
        );

        let target = temp.path().join("restored");
        let request = Vault::preflight_backup_restore(&output, target.clone()).unwrap();
        let restored = Vault::restore_backup(&passphrase, request).unwrap();
        assert_eq!(restored.format_version, version, "v{version}");
        // Restore currently installs the exact archived envelope and appends
        // only an audit-only restore event, so the generation is unchanged.
        assert_eq!(
            fs::read_to_string(restored.root.join("vault.json")).unwrap(),
            source_file
        );
        let vault = Vault::resolve_for_test(Some(restored.root.clone())).unwrap();
        vault.verify_audit(&passphrase).unwrap();
        vault.list_fields(&passphrase).unwrap();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn released_version_two_readers_reject_version_three_archives() {
    let temp = private_temp();
    let source_home = temp.path().join("source");
    let source = Vault::resolve_for_test(Some(source_home.clone())).unwrap();
    source.init(&test_passphrase()).unwrap();
    let output = temp.path().join("v3.backup");
    let request = Vault::preflight_backup_create(source_home, &output, false).unwrap();
    Vault::create_backup(&test_passphrase(), request).unwrap();

    let decoded = decrypt_backup(&output, &test_passphrase());
    // Format 2 readers gated the embedded version and used a strict schema
    // without a generation, so either check refuses a format 3 archive.
    assert_ne!(decoded.source_format_version, V2_FORMAT_VERSION);
    assert!(serde_json::from_slice::<ReleasedV2EmbeddedVaultFile>(decoded.vault_bytes()).is_err());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn frozen_v3_archive_restores_as_version_three() {
    use crate::test_fixtures::{
        GENERATED_V3_BACKUP, GENERATED_V3_BACKUP_AUDIT_EVENTS, GENERATED_V3_GENERATION,
        GENERATED_V3_VAULT_ID, generated_v3_passphrase,
    };
    let temp = private_temp();
    let input = temp.path().join("frozen-v3.backup");
    fs::write(&input, GENERATED_V3_BACKUP).unwrap();
    let target = temp.path().join("restored-v3");
    let request = Vault::preflight_backup_restore(&input, target).unwrap();
    let restored = Vault::restore_backup(&generated_v3_passphrase(), request).unwrap();

    assert_eq!(restored.format_version, V3_FORMAT_VERSION);
    assert_eq!(restored.vault_id, GENERATED_V3_VAULT_ID);
    let restored_file: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(restored.root.join("vault.json")).unwrap())
            .unwrap();
    assert_eq!(
        restored_file["header"]["generation"],
        GENERATED_V3_GENERATION
    );
    let vault = Vault::resolve_for_test(Some(restored.root.clone())).unwrap();
    assert_eq!(
        vault.list_fields(&generated_v3_passphrase()).unwrap().len(),
        2
    );
    let audit = vault.verify_audit(&generated_v3_passphrase()).unwrap();
    assert_eq!(audit.event_count, GENERATED_V3_BACKUP_AUDIT_EVENTS + 1);
    let last = backup_events(&restored.root).pop().unwrap();
    assert_eq!(last.action, "backup_restore");
    assert_eq!(last.details["source_format_version"], V3_FORMAT_VERSION);
    assert!(last.details.get("generation").is_none());
}
