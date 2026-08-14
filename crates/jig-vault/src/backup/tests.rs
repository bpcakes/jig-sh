#[cfg(target_os = "linux")]
use std::fs::{self, File};
#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
#[cfg(target_os = "linux")]
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::crypto::{KEY_LEN, KdfParams, NONCE_LEN, SALT_LEN};
use crate::format::{AEAD_ALGORITHM, FORMAT_VERSION, MAGIC, V1_FORMAT_VERSION};
#[cfg(target_os = "linux")]
use crate::{FieldKind, FieldMutation, SecretBytes, Vault, VaultReference};

#[cfg(target_os = "linux")]
use super::codec::seal_archive;
use super::codec::{
    BACKUP_AAD_DOMAIN, BACKUP_MAGIC, BackupEnvelope, backup_aad, parse_archive_bytes,
};
use super::*;

#[cfg(target_os = "linux")]
fn test_passphrase() -> SecretString {
    SecretString::from("backup-test-passphrase".to_owned())
}

#[cfg(target_os = "linux")]
fn reference() -> VaultReference {
    VaultReference::parse("jig://Production/TOKEN").unwrap()
}

fn syntactically_valid_archive_value() -> serde_json::Value {
    serde_json::json!({
        "header": {
            "magic": BACKUP_MAGIC,
            "version": BACKUP_FORMAT_VERSION,
            "created_at_ms": 1,
            "kdf": KdfParams::default(),
            "salt_b64": B64.encode([0_u8; SALT_LEN]),
            "aead": AEAD_ALGORITHM,
            "nonce_b64": B64.encode([0_u8; NONCE_LEN]),
        },
        "ciphertext_b64": B64.encode([0_u8; 16]),
    })
}

fn syntactically_complete_vault_value() -> serde_json::Value {
    serde_json::json!({
        "header": {
            "magic": MAGIC,
            "version": FORMAT_VERSION,
            "vault_id": "01TESTVAULTIDENTIFIER000000",
            "created_at_ms": 1,
            "kdf": KdfParams::default(),
            "salt_b64": B64.encode([0_u8; SALT_LEN]),
            "aead": AEAD_ALGORITHM,
        },
        "wrapped_dek_nonce_b64": B64.encode([0_u8; NONCE_LEN]),
        "wrapped_dek_b64": B64.encode([0_u8; KEY_LEN + 16]),
        "state_nonce_b64": B64.encode([0_u8; NONCE_LEN]),
        "state_b64": B64.encode([0_u8; 16]),
    })
}

#[test]
fn public_header_is_strict_bounded_and_never_echoes_attacker_strings() {
    let mut unknown_kdf = syntactically_valid_archive_value();
    unknown_kdf["header"]["kdf"]["SENTINEL_UNKNOWN_KDF"] = serde_json::json!(true);
    let error =
        parse_archive_bytes(Zeroizing::new(serde_json::to_vec(&unknown_kdf).unwrap())).unwrap_err();
    assert!(!error.to_string().contains("SENTINEL_UNKNOWN_KDF"));

    let sentinel = "SENTINEL_MAGIC".repeat(2_000);
    let mut oversized_magic = syntactically_valid_archive_value();
    oversized_magic["header"]["magic"] = serde_json::json!(sentinel);
    let error = parse_archive_bytes(Zeroizing::new(
        serde_json::to_vec(&oversized_magic).unwrap(),
    ))
    .unwrap_err();
    assert!(error.to_string().contains("public header"));
    assert!(!error.to_string().contains("SENTINEL_MAGIC"));
}

#[test]
fn backup_aad_binds_every_public_header_field_and_role() {
    let envelope: BackupEnvelope =
        serde_json::from_value(syntactically_valid_archive_value()).unwrap();
    let aad = String::from_utf8(backup_aad(&envelope.header)).unwrap();
    for field in [
        "magic",
        "version",
        "created_at_ms",
        "kdf.algorithm",
        "kdf.memory_kib",
        "kdf.iterations",
        "kdf.parallelism",
        "kdf.output_len",
        "salt_b64",
        "aead",
        "nonce_b64",
        "payload_role:14:backup_payload",
    ] {
        assert!(aad.contains(field), "AAD omitted {field}");
    }
    assert!(aad.starts_with(BACKUP_AAD_DOMAIN));
}

#[test]
fn embedded_vault_validation_requires_complete_strict_v2_envelope() {
    let complete = syntactically_complete_vault_value();
    let (vault_id, version) =
        inspect_embedded_vault(&serde_json::to_vec(&complete).unwrap()).unwrap();
    assert_eq!(vault_id, "01TESTVAULTIDENTIFIER000000");
    assert_eq!(version, FORMAT_VERSION);

    let incomplete = serde_json::json!({ "header": complete["header"].clone() });
    let error = inspect_embedded_vault(&serde_json::to_vec(&incomplete).unwrap()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("complete embedded vault envelope")
    );

    let mut unknown_kdf = complete.clone();
    unknown_kdf["header"]["kdf"]["SENTINEL_EMBEDDED_KDF"] = serde_json::json!(true);
    let error = inspect_embedded_vault(&serde_json::to_vec(&unknown_kdf).unwrap()).unwrap_err();
    assert!(!error.to_string().contains("SENTINEL_EMBEDDED_KDF"));

    let mut malformed_salt = complete.clone();
    malformed_salt["header"]["salt_b64"] = serde_json::json!(B64.encode([0_u8; 1]));
    let error = inspect_embedded_vault(&serde_json::to_vec(&malformed_salt).unwrap()).unwrap_err();
    assert!(error.to_string().contains("salt"));

    let mut legacy = complete;
    legacy["header"]["version"] = serde_json::json!(V1_FORMAT_VERSION);
    let error = inspect_embedded_vault(&serde_json::to_vec(&legacy).unwrap()).unwrap_err();
    assert_eq!(
        crate::error::classified_kind(&error),
        Some(VaultErrorKind::InvalidInput)
    );
    assert!(error.to_string().contains("migrate --to 2"));
}

#[cfg(target_os = "linux")]
#[test]
fn restore_preflight_rejects_symlink_truncation_oversize_and_existing_target() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let valid = temp.path().join("valid.backup");
    fs::write(
        &valid,
        serde_json::to_vec(&syntactically_valid_archive_value()).unwrap(),
    )
    .unwrap();

    let link = temp.path().join("link.backup");
    symlink(&valid, &link).unwrap();
    let error =
        Vault::preflight_backup_restore(&link, temp.path().join("link-target")).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);

    let truncated = temp.path().join("truncated.backup");
    fs::write(&truncated, b"{\"header\":").unwrap();
    let error = Vault::preflight_backup_restore(&truncated, temp.path().join("truncated-target"))
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Serialization);

    let oversized = temp.path().join("oversized.backup");
    File::create(&oversized)
        .unwrap()
        .set_len(MAX_BACKUP_ARCHIVE_BYTES as u64 + 1)
        .unwrap();
    let error = Vault::preflight_backup_restore(&oversized, temp.path().join("oversized-target"))
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);

    let existing = temp.path().join("existing-target");
    fs::create_dir(&existing).unwrap();
    let error = Vault::preflight_backup_restore(&valid, existing.clone()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert!(existing.read_dir().unwrap().next().is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn backup_create_preflight_is_noncreating_and_rejects_v1_before_capture() {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let missing = temp.path().join("missing-source");
    let output = temp.path().join("missing.backup");
    assert!(Vault::preflight_backup_create(missing.clone(), &output, false).is_err());
    assert!(!missing.exists());
    assert!(!output.exists());

    let legacy = temp.path().join("legacy-source");
    fs::create_dir(&legacy).unwrap();
    fs::set_permissions(&legacy, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        legacy.join("vault.json"),
        include_bytes!("../../tests/fixtures/cli-generated-v1/vault.json"),
    )
    .unwrap();
    fs::write(
        legacy.join("audit.jsonl"),
        include_bytes!("../../tests/fixtures/cli-generated-v1/audit.jsonl"),
    )
    .unwrap();
    for file in ["vault.json", "audit.jsonl"] {
        fs::set_permissions(legacy.join(file), fs::Permissions::from_mode(0o600)).unwrap();
    }
    let error = Vault::preflight_backup_create(legacy.clone(), &output, false).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("migrate --to 2"));
    assert!(!legacy.join("vault.lock").exists());
    assert!(!output.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn authenticated_tampered_audit_fails_and_cleans_owned_staging() {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let source_home = temp.path().join("tamper-source");
    let source = Vault::resolve(Some(source_home)).unwrap();
    source.init(&test_passphrase()).unwrap();
    let vault_bytes = fs::read(source.root().join("vault.json")).unwrap();
    let audit = fs::read_to_string(source.root().join("audit.jsonl")).unwrap();
    let tampered_audit = audit.replacen("vault_initialized", "vault_initializeD", 1);
    let (vault_id, format_version) = inspect_embedded_vault(&vault_bytes).unwrap();
    let sealed = seal_archive(
        &test_passphrase(),
        &vault_id,
        format_version,
        &vault_bytes,
        tampered_audit.as_bytes(),
        now_ms(),
    )
    .unwrap();
    let input = temp.path().join("tampered-audit.backup");
    PreparedPrivateFile::prepare(&input, sealed.bytes, false)
        .unwrap()
        .install()
        .unwrap();

    let target = temp.path().join("tampered-audit-target");
    let request = Vault::preflight_backup_restore(&input, target.clone()).unwrap();
    let error = Vault::restore_backup(&test_passphrase(), request).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(!target.exists());
    assert!(!fs::read_dir(temp.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("jig-vault-restore.tmp")
    }));
    source.verify_audit(&test_passphrase()).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn creates_and_restores_private_complete_vault() {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let source_home = temp.path().join("source-vault");
    let source = Vault::resolve(Some(source_home.clone())).unwrap();
    source.init(&test_passphrase()).unwrap();
    source
        .apply_field_batch(
            &test_passphrase(),
            vec![FieldMutation::set(
                reference(),
                FieldKind::Concealed,
                SecretBytes::new(b"backup-secret-sentinel".to_vec()),
            )],
        )
        .unwrap();

    let raced_output = temp.path().join("raced-output.backup");
    let raced_request =
        Vault::preflight_backup_create(source_home.clone(), &raced_output, false).unwrap();
    let audit_path = source.root().join("audit.jsonl");
    let racer_output = raced_output.clone();
    let racer = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let started = fs::read_to_string(&audit_path)
                .is_ok_and(|audit| audit.contains("\"action\":\"backup_start\""));
            if started {
                fs::write(&racer_output, b"raced-existing-output").unwrap();
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "backup start was not observed"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });
    let error = Vault::create_backup(&test_passphrase(), raced_request).unwrap_err();
    racer.join().unwrap();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(fs::read(&raced_output).unwrap(), b"raced-existing-output");
    let raced_events = fs::read_to_string(source.root().join("audit.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<crate::audit::AuditEvent>(line).unwrap())
        .filter(|event| event.action.starts_with("backup_"))
        .collect::<Vec<_>>();
    assert_eq!(raced_events.len(), 2);
    assert_eq!(raced_events[0].action, "backup_start");
    assert_eq!(raced_events[1].action, "backup_failed");
    assert_eq!(
        raced_events[0].details["operation_id"],
        raced_events[1].details["operation_id"]
    );

    let output = temp.path().join("vault.backup");
    let request = Vault::preflight_backup_create(source_home, &output, false).unwrap();
    let created = Vault::create_backup(&test_passphrase(), request).unwrap();
    assert_eq!(created.backup_version, BACKUP_FORMAT_VERSION);
    assert_eq!(
        created.bytes_written,
        fs::metadata(&output).unwrap().len() as usize
    );
    assert!(
        !fs::read(&output)
            .unwrap()
            .windows(b"backup-secret-sentinel".len())
            .any(|window| window == b"backup-secret-sentinel")
    );
    assert_eq!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let source_audit_after_create = fs::read(source.root().join("audit.jsonl")).unwrap();
    let source_events = source_audit_after_create
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<crate::audit::AuditEvent>(line).unwrap())
        .collect::<Vec<_>>();
    let start = source_events
        .iter()
        .rev()
        .find(|event| event.action == "backup_start")
        .unwrap();
    let finish = source_events
        .iter()
        .rev()
        .find(|event| event.action == "backup_finish")
        .unwrap();
    assert_eq!(
        start.details["operation_id"],
        finish.details["operation_id"]
    );
    let audit_text = String::from_utf8(source_audit_after_create.clone()).unwrap();
    assert!(!audit_text.contains("backup-secret-sentinel"));
    assert!(!audit_text.contains(output.to_string_lossy().as_ref()));

    let before_refusal = source_audit_after_create;
    let collision =
        Vault::preflight_backup_create(source.root().to_path_buf(), &output, false).unwrap_err();
    assert_eq!(collision.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(
        fs::read(source.root().join("audit.jsonl")).unwrap(),
        before_refusal
    );

    let inside_home = source.root().join("inside.backup");
    let inside_error =
        Vault::preflight_backup_create(source.root().to_path_buf(), &inside_home, false)
            .unwrap_err();
    assert_eq!(inside_error.kind(), VaultErrorKind::InvalidInput);
    assert_eq!(
        inside_error.to_string(),
        "backup output must be outside the source vault home"
    );

    let hardlink = temp.path().join("source-hardlink.backup");
    fs::hard_link(source.root().join("vault.json"), &hardlink).unwrap();
    let alias_error =
        Vault::preflight_backup_create(source.root().to_path_buf(), &hardlink, true).unwrap_err();
    assert_eq!(alias_error.kind(), VaultErrorKind::InvalidInput);
    assert_eq!(
        alias_error.to_string(),
        "backup output must not alias a source vault file"
    );
    fs::remove_file(&hardlink).unwrap();

    let output_two = temp.path().join("vault-two.backup");
    let request =
        Vault::preflight_backup_create(source.root().to_path_buf(), &output_two, false).unwrap();
    Vault::create_backup(&test_passphrase(), request).unwrap();
    assert_ne!(fs::read(&output).unwrap(), fs::read(&output_two).unwrap());

    let wrong_target = temp.path().join("wrong-pass-target");
    let request = Vault::preflight_backup_restore(&output, wrong_target.clone()).unwrap();
    let wrong = SecretString::from("different backup passphrase".to_owned());
    let error = Vault::restore_backup(&wrong, request).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication);
    assert!(!wrong_target.exists());
    assert!(!fs::read_dir(temp.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("jig-vault-restore.tmp")
    }));

    let mut tampered: serde_json::Value =
        serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    tampered["header"]["created_at_ms"] = serde_json::json!(created.created_at_ms + 1);
    let tampered_path = temp.path().join("tampered.backup");
    fs::write(
        &tampered_path,
        serde_json::to_vec_pretty(&tampered).unwrap(),
    )
    .unwrap();
    let tampered_target = temp.path().join("tampered-target");
    let request = Vault::preflight_backup_restore(&tampered_path, tampered_target.clone()).unwrap();
    let error = Vault::restore_backup(&test_passphrase(), request).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication);
    assert!(!tampered_target.exists());

    let raced_target = temp.path().join("raced-target");
    let request = Vault::preflight_backup_restore(&output, raced_target.clone()).unwrap();
    fs::create_dir(&raced_target).unwrap();
    fs::write(raced_target.join("marker"), b"unchanged").unwrap();
    let error = Vault::restore_backup(&test_passphrase(), request).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(fs::read(raced_target.join("marker")).unwrap(), b"unchanged");

    let target = temp.path().join("restored-vault");
    let request = Vault::preflight_backup_restore(&output, target.clone()).unwrap();
    let restored = Vault::restore_backup(&test_passphrase(), request).unwrap();
    assert_eq!(restored.root, target);
    assert_eq!(restored.format_version, FORMAT_VERSION);
    assert_eq!(
        fs::metadata(&restored.root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for file in ["vault.json", "audit.jsonl", "vault.lock"] {
        assert_eq!(
            fs::metadata(restored.root.join(file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let restored_vault = Vault::resolve(Some(restored.root)).unwrap();
    assert_eq!(
        restored_vault
            .list_fields(&test_passphrase())
            .unwrap()
            .len(),
        1
    );
    restored_vault.verify_audit(&test_passphrase()).unwrap();
    let restored_audit = fs::read_to_string(restored_vault.root().join("audit.jsonl")).unwrap();
    let restored_events = restored_audit
        .lines()
        .map(|line| serde_json::from_str::<crate::audit::AuditEvent>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(restored_events.last().unwrap().action, "backup_restore");
    assert!(
        restored_events
            .iter()
            .any(|event| event.action == "backup_start")
    );
    assert!(!restored_audit.contains("backup-secret-sentinel"));
}
