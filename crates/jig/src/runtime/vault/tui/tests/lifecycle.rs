use super::*;

#[cfg(unix)]
#[test]
fn lifecycle_tools_backup_restore_rotate_verify_and_project_activity() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Production/TOKEN".parse().unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(b"lifecycle-secret-sentinel".to_vec()),
        )
        .unwrap();
    let backend = VaultTuiBackend::new(request(home.clone())).unwrap();
    backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap();

    let VaultActionResult::Activity(activity) = backend
        .execute(VaultAction::Activity { limit: 10 })
        .unwrap()
    else {
        panic!("expected activity");
    };
    assert!(!activity.records.is_empty());
    assert_eq!(activity.audit.torn_tail_bytes, 0);
    assert!(!format!("{activity:?}").contains("lifecycle-secret-sentinel"));
    let VaultActionResult::Audit(verification) = backend.execute(VaultAction::VerifyAudit).unwrap()
    else {
        panic!("expected audit verification");
    };
    assert_eq!(verification.torn_tail_bytes, 0);

    let export = temp.path().join("token.bin");
    let VaultActionResult::Exported {
        bytes_written,
        snapshot,
        ..
    } = backend
        .execute(VaultAction::ExportField {
            reference: "jig://Production/TOKEN".parse().unwrap(),
            output: export.clone(),
            overwrite: false,
        })
        .unwrap()
    else {
        panic!("expected export result");
    };
    assert_eq!(bytes_written, b"lifecycle-secret-sentinel".len());
    assert_eq!(snapshot.fields.len(), 1);
    assert_eq!(
        std::fs::read(&export).unwrap(),
        b"lifecycle-secret-sentinel"
    );
    let export_collision = backend
        .execute(VaultAction::ExportField {
            reference: "jig://Production/TOKEN".parse().unwrap(),
            output: export,
            overwrite: false,
        })
        .unwrap_err();
    assert_eq!(export_collision.kind(), VaultUiErrorKind::Conflict);

    let mut peeked = zeroize::Zeroizing::new(Vec::new());
    let peeked_len = backend
        .peek(&"jig://Production/TOKEN".parse().unwrap(), &mut *peeked)
        .unwrap();
    assert_eq!(peeked_len, b"lifecycle-secret-sentinel".len());
    assert_eq!(&peeked[..], b"lifecycle-secret-sentinel");

    let VaultActionResult::BackupCreated {
        bytes_written,
        backup_version,
        ..
    } = backend
        .execute(VaultAction::CreateBackup {
            output: lifecycle_backup_path(&temp),
            overwrite: false,
        })
        .unwrap()
    else {
        panic!("expected backup result");
    };
    assert!(bytes_written > 0);
    assert_eq!(backup_version, jig_vault::BACKUP_FORMAT_VERSION);
    let collision = backend
        .execute(VaultAction::CreateBackup {
            output: lifecycle_backup_path(&temp),
            overwrite: false,
        })
        .unwrap_err();
    assert_eq!(collision.kind(), VaultUiErrorKind::Conflict);

    #[cfg(target_os = "linux")]
    {
        let restored_home = temp.path().join("restored-vault");
        let restored_backend = VaultTuiBackend::new(request(restored_home.clone())).unwrap();
        let VaultActionResult::Restored {
            root,
            format_version,
            ..
        } = restored_backend
            .execute(VaultAction::RestoreBackup {
                input: lifecycle_backup_path(&temp),
                passphrase: SecretBytes::new(b"correct horse battery staple".to_vec()),
            })
            .unwrap()
        else {
            panic!("expected restore result");
        };
        assert_eq!(root, restored_home);
        assert_eq!(format_version, 2);
        let restored = restored_backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();
        assert_eq!(restored.fields.len(), 1);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let restored_home = temp.path().join("restored-vault");
        let restored_backend = VaultTuiBackend::new(request(restored_home.clone())).unwrap();
        let error = restored_backend
            .execute(VaultAction::RestoreBackup {
                input: lifecycle_backup_path(&temp),
                passphrase: SecretBytes::new(b"correct horse battery staple".to_vec()),
            })
            .unwrap_err();
        assert_eq!(error.kind(), VaultUiErrorKind::InvalidInput);
        assert!(error.message().contains("unsupported on this platform"));
        assert!(!restored_home.exists());
    }

    let new_passphrase = b"new correct horse battery staple";
    let VaultActionResult::Snapshot(rotated) = backend
        .execute(VaultAction::ChangePassphrase {
            new_passphrase: SecretBytes::new(new_passphrase.to_vec()),
        })
        .unwrap()
    else {
        panic!("expected refreshed snapshot");
    };
    assert_eq!(rotated.fields.len(), 1);
    assert_eq!(
        vault.snapshot(&passphrase).unwrap_err().kind(),
        VaultErrorKind::Authentication
    );
    assert!(
        vault
            .snapshot(&SecretString::from(
                String::from_utf8(new_passphrase.to_vec()).unwrap()
            ))
            .is_ok()
    );
    assert!(backend.refresh().is_ok());

    let audit_path = home.join("audit.jsonl");
    let audit = std::fs::read_to_string(&audit_path).unwrap();
    std::fs::write(
        &audit_path,
        audit.replace(
            "\"action\":\"field_batch_apply\"",
            "\"action\":\"secret_get\"",
        ),
    )
    .unwrap();
    let tampered = backend.execute(VaultAction::VerifyAudit).unwrap_err();
    assert_eq!(tampered.kind(), VaultUiErrorKind::Audit);
}

#[cfg(unix)]
#[test]
fn onepassword_preview_avoids_op_and_commit_reuses_the_hardened_resolver() {
    use std::os::unix::fs::PermissionsExt;

    use crate::test_env::{EnvVarGuard, lock_env};

    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let backend = VaultTuiBackend::new(request(home)).unwrap();
    backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap();

    let bin = temp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let op = bin.join("op");
    let log = temp.path().join("op.log");
    std::fs::write(
        &op,
        r#"#!/bin/sh
set -eu
if [ "${JIG_VAULT_PASSPHRASE+set}" = set ] || [ "${JIG_VAULT_NEW_PASSPHRASE+set}" = set ]; then
  printf '%s\n' 'reserved-env-leaked' >> "$OP_TEST_LOG"
  exit 87
fi
printf '%s\n' "$3" >> "$OP_TEST_LOG"
printf '%s' 'resolved-tui-import-secret'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut path_parts = vec![bin];
    if let Some(path) = std::env::var_os("PATH") {
        path_parts.extend(std::env::split_paths(&path));
    }
    let _path = EnvVarGuard::set("PATH", std::env::join_paths(path_parts).unwrap());
    let _current = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "must-not-reach-op");
    let _new = EnvVarGuard::set("JIG_VAULT_NEW_PASSPHRASE", "must-not-reach-op");
    let _log = EnvVarGuard::set("OP_TEST_LOG", &log);

    let source = temp.path().join("source.env");
    let destination = temp.path().join("generated.env");
    std::fs::write(&source, b"TOKEN=op://Test/Login/TOKEN\nMODE=production\n").unwrap();
    let item = jig_vault::VaultItem::parse("jig://Production").unwrap();
    let VaultActionResult::ImportPreview(preview) = backend
        .execute(VaultAction::PreviewOnePasswordImport {
            env_file: source.clone(),
            item,
            out_env: destination.clone(),
            replace: false,
            overwrite: false,
            dry_run: false,
        })
        .unwrap()
    else {
        panic!("expected import preview");
    };
    assert_eq!(preview.rows.len(), 2);
    assert!(!preview.destination_exists);
    assert!(!log.exists(), "preview unexpectedly invoked op");
    let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
        panic!("non-dry-run preview did not return commit authority");
    };

    // The approved plan owns the parsed protected source. Replacing the
    // path after preview must not change the committed field set.
    std::fs::write(&source, b"OTHER=op://Changed/Login/OTHER\n").unwrap();

    let VaultActionResult::Snapshot(imported) = backend
        .execute(VaultAction::CommitOnePasswordImport {
            plan: plan.clone(),
            replace: false,
            overwrite: false,
        })
        .unwrap()
    else {
        panic!("expected imported snapshot");
    };
    assert_eq!(imported.fields.len(), 2);
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"TOKEN=jig://Production/TOKEN\nMODE=jig://Production/MODE\n"
    );
    let log_text = std::fs::read_to_string(log).unwrap();
    assert!(log_text.contains("op://Test/Login/TOKEN"));
    assert!(!log_text.contains("op://Changed/Login/OTHER"));
    assert!(!log_text.contains("reserved-env-leaked"));

    let reused = backend
        .execute(VaultAction::CommitOnePasswordImport {
            plan,
            replace: false,
            overwrite: false,
        })
        .unwrap_err();
    assert_eq!(reused.kind(), VaultUiErrorKind::Conflict);
    assert!(reused.message().contains("already used"));

    std::fs::write(
        temp.path().join("source.env"),
        b"TOKEN=literal-token\nMODE=op://Test/Login/MODE\n",
    )
    .unwrap();
    let log_len = log_text.len();
    let VaultActionResult::ImportPreview(existing) = backend
        .execute(VaultAction::PreviewOnePasswordImport {
            env_file: temp.path().join("source.env"),
            item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
            out_env: destination,
            replace: false,
            overwrite: false,
            dry_run: true,
        })
        .unwrap()
    else {
        panic!("expected existing import preview");
    };
    assert!(existing.destination_exists);
    assert!(
        existing
            .rows
            .iter()
            .all(|row| row.change.replaces_existing())
    );
    assert_eq!(
        existing.rows[0].change,
        ImportFieldChange::Replace {
            previous_kind: FieldKind::Concealed,
            kind: FieldKind::Text,
        }
    );
    assert_eq!(
        existing.rows[1].change,
        ImportFieldChange::Replace {
            previous_kind: FieldKind::Text,
            kind: FieldKind::Concealed,
        }
    );
    assert!(matches!(
        existing.authorization,
        ImportPreviewAuthorization::DryRun
    ));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("op.log"))
            .unwrap()
            .len(),
        log_len
    );
}

#[cfg(unix)]
#[test]
fn import_commit_rejects_destination_and_vault_drift_from_preview() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let backend = VaultTuiBackend::new(request(home)).unwrap();
    backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap();

    let source = temp.path().join("destination-drift.env");
    let destination = temp.path().join("destination-drift.generated.env");
    std::fs::write(&source, b"MODE=production\n").unwrap();
    let VaultActionResult::ImportPreview(preview) = backend
        .execute(VaultAction::PreviewOnePasswordImport {
            env_file: source,
            item: jig_vault::VaultItem::parse("jig://DestinationDrift").unwrap(),
            out_env: destination.clone(),
            replace: false,
            overwrite: false,
            dry_run: false,
        })
        .unwrap()
    else {
        panic!("expected destination-drift preview");
    };
    let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
        panic!("expected destination-drift commit plan");
    };
    std::fs::write(&destination, b"must-not-be-overwritten").unwrap();

    let destination_error = backend
        .execute(VaultAction::CommitOnePasswordImport {
            plan,
            replace: false,
            overwrite: true,
        })
        .unwrap_err();
    assert_eq!(destination_error.kind(), VaultUiErrorKind::Conflict);
    assert!(
        destination_error
            .message()
            .contains("changed since preview")
    );
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"must-not-be-overwritten"
    );
    assert_eq!(
        vault
            .preview_import_fields(
                &passphrase,
                &[jig_vault::VaultReference::parse("jig://DestinationDrift/MODE").unwrap()]
            )
            .unwrap(),
        vec![false]
    );

    let source = temp.path().join("vault-drift.env");
    let destination = temp.path().join("vault-drift.generated.env");
    std::fs::write(&source, b"MODE=production\n").unwrap();
    let VaultActionResult::ImportPreview(preview) = backend
        .execute(VaultAction::PreviewOnePasswordImport {
            env_file: source,
            item: jig_vault::VaultItem::parse("jig://VaultDrift").unwrap(),
            out_env: destination.clone(),
            replace: false,
            overwrite: false,
            dry_run: false,
        })
        .unwrap()
    else {
        panic!("expected vault-drift preview");
    };
    let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
        panic!("expected vault-drift commit plan");
    };
    vault
        .set_field(
            &passphrase,
            jig_vault::VaultReference::parse("jig://External/CHANGE").unwrap(),
            FieldKind::Text,
            SecretBytes::new(b"external-change".to_vec()),
        )
        .unwrap();

    let vault_error = backend
        .execute(VaultAction::CommitOnePasswordImport {
            plan,
            replace: false,
            overwrite: false,
        })
        .unwrap_err();
    assert_eq!(vault_error.kind(), VaultUiErrorKind::Conflict);
    assert!(
        vault_error
            .message()
            .contains("changed since the import preview")
    );
    assert!(!destination.exists());
    assert_eq!(
        vault
            .preview_import_fields(
                &passphrase,
                &[jig_vault::VaultReference::parse("jig://VaultDrift/MODE").unwrap()]
            )
            .unwrap(),
        vec![false]
    );
}
