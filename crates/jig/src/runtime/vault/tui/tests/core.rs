use super::*;

#[test]
fn descriptor_does_not_create_an_absent_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("absent");

    let backend = VaultTuiBackend::new(request(home.clone())).unwrap();

    assert_eq!(backend.descriptor().home_state, VaultHomeState::Absent);
    assert_eq!(backend.home_state().unwrap(), VaultHomeState::Absent);
    assert!(!home.exists());

    std::fs::create_dir(&home).unwrap();
    assert_eq!(backend.home_state().unwrap(), VaultHomeState::Uninitialized);
    std::fs::write(home.join("vault.json"), b"installed").unwrap();
    assert_eq!(backend.home_state().unwrap(), VaultHomeState::Initialized);
}

#[test]
fn invalid_initialization_passphrase_is_rejected_before_home_creation() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("absent");
    let backend = VaultTuiBackend::new(request(home.clone())).unwrap();

    let error = backend
        .initialize(SecretBytes::new(b"too-short".to_vec()))
        .unwrap_err();

    assert_eq!(error.kind(), VaultUiErrorKind::InvalidInput);
    assert!(!home.exists());
    assert_eq!(backend.home_state().unwrap(), VaultHomeState::Absent);
}

#[test]
fn failed_unlock_retains_no_session_and_lock_drops_a_valid_session() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let backend = VaultTuiBackend::new(request(home)).unwrap();

    let error = backend
        .unlock(SecretBytes::new(b"wrong passphrase".to_vec()))
        .unwrap_err();
    assert_eq!(error.kind(), VaultUiErrorKind::Authentication);
    assert_eq!(
        backend.refresh().unwrap_err().kind(),
        VaultUiErrorKind::Authentication
    );

    let snapshot = backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap();
    assert_eq!(snapshot.format_version, 2);
    backend.lock();
    assert_eq!(
        backend.refresh().unwrap_err().kind(),
        VaultUiErrorKind::Authentication
    );
}

#[test]
fn committed_action_result_preserves_success_when_refresh_fails() {
    let action = VaultCommittedAction::Exported {
        output: "/tmp/private-export".into(),
        bytes_written: 17,
    };
    let refresh_error = VaultUiError::new(VaultUiErrorKind::Io, "safe refresh failure");

    let result = committed_action_result(action.clone(), Err(refresh_error.clone()));

    assert_eq!(
        result,
        VaultActionResult::Committed {
            action,
            refresh_error,
        }
    );
}

#[test]
fn lock_erases_credentials_even_when_the_session_mutex_is_poisoned() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let backend = VaultTuiBackend::new(request(home)).unwrap();
    backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap();

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _session = backend.session.lock().unwrap();
        panic!("poison the vault TUI session mutex");
    }));
    assert!(panic.is_err());

    backend.lock();

    let session = match backend.session.lock() {
        Ok(_) => panic!("session mutex unexpectedly lost its poisoned state"),
        Err(poisoned) => poisoned.into_inner(),
    };
    assert!(session.credential.is_none());
    assert!(session.pending_import.is_none());
}

#[cfg(unix)]
#[test]
fn private_output_actions_reject_the_vault_home_before_sink_work() {
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
            SecretBytes::new(b"reserved-tui-output-sentinel".to_vec()),
        )
        .unwrap();
    let backend = VaultTuiBackend::new(request(home.clone())).unwrap();
    backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap();
    let vault_path = home.join("vault.json");
    let audit_path = home.join("audit.jsonl");
    let before_vault = std::fs::read(&vault_path).unwrap();
    let before_audit = std::fs::read(&audit_path).unwrap();

    let export_error = backend
        .execute(VaultAction::ExportField {
            reference: "jig://Production/TOKEN".parse().unwrap(),
            output: vault_path.clone(),
            overwrite: true,
        })
        .unwrap_err();
    assert_eq!(export_error.kind(), VaultUiErrorKind::InvalidInput);

    let source = temp.path().join("source.env");
    std::fs::write(&source, b"MODE=production\n").unwrap();
    let import_error = backend
        .execute(VaultAction::PreviewOnePasswordImport {
            env_file: source,
            item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
            out_env: audit_path.clone(),
            replace: true,
            overwrite: true,
            dry_run: false,
        })
        .unwrap_err();
    assert_eq!(import_error.kind(), VaultUiErrorKind::InvalidInput);

    assert_eq!(std::fs::read(&vault_path).unwrap(), before_vault);
    assert_eq!(std::fs::read(&audit_path).unwrap(), before_audit);
    assert!(backend.session().unwrap().pending_import.is_none());
    assert!(!format!("{export_error:?}{import_error:?}").contains("reserved-tui-output-sentinel"));
}

#[cfg(unix)]
#[test]
fn import_discard_consumes_only_the_matching_pending_plan() {
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

    let source = temp.path().join("source.env");
    let destination = temp.path().join("generated.env");
    std::fs::write(&source, b"MODE=production\n").unwrap();
    let VaultActionResult::ImportPreview(preview) = backend
        .execute(VaultAction::PreviewOnePasswordImport {
            env_file: source,
            item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
            out_env: destination.clone(),
            replace: false,
            overwrite: false,
            dry_run: false,
        })
        .unwrap()
    else {
        panic!("expected import preview");
    };
    let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
        panic!("expected commit authority");
    };

    let wrong_plan = backend
        .execute(VaultAction::DiscardOnePasswordImport {
            plan: ImportPlanToken::generate(),
        })
        .unwrap_err();
    assert_eq!(wrong_plan.kind(), VaultUiErrorKind::Conflict);
    assert!(
        backend
            .session()
            .unwrap()
            .pending_import
            .as_ref()
            .is_some_and(|pending| pending.token == plan)
    );

    assert!(matches!(
        backend
            .execute(VaultAction::DiscardOnePasswordImport { plan: plan.clone() })
            .unwrap(),
        VaultActionResult::ImportDiscarded
    ));
    assert!(backend.session().unwrap().pending_import.is_none());

    let consumed = backend
        .execute(VaultAction::CommitOnePasswordImport {
            plan,
            replace: false,
            overwrite: false,
        })
        .unwrap_err();
    assert_eq!(consumed.kind(), VaultUiErrorKind::Conflict);
    assert!(!destination.exists());
    assert!(vault.snapshot(&passphrase).unwrap().fields.is_empty());
}

#[test]
fn management_actions_require_the_snapshot_revision_that_authorized_them() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let backend = VaultTuiBackend::new(request(home)).unwrap();
    let mut current = backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap();

    let field: VaultReference = "jig://Production/TOKEN".parse().unwrap();
    current = mutate(
        &backend,
        &current,
        VaultMutation::SetField {
            reference: field.clone(),
            kind: FieldKind::Concealed,
            value: SecretBytes::new(b"initial-secret".to_vec()),
            mode: VaultWriteMode::Create,
        },
    )
    .unwrap();
    assert_eq!(current.fields.len(), 1);

    let collision = mutate(
        &backend,
        &current,
        VaultMutation::SetField {
            reference: field.clone(),
            kind: FieldKind::Text,
            value: SecretBytes::new(b"stale-overwrite-sentinel".to_vec()),
            mode: VaultWriteMode::Create,
        },
    )
    .unwrap_err();
    assert_eq!(collision.kind(), VaultUiErrorKind::Conflict);

    // An external CLI write invalidates every command authorized by the
    // previous TUI snapshot. In particular, a stale item delete must not
    // erase a field the operator never saw.
    let external: VaultReference = "jig://Production/EXTERNAL".parse().unwrap();
    vault
        .write_field(
            &passphrase,
            external.clone(),
            FieldKind::Concealed,
            SecretBytes::new(b"external-secret".to_vec()),
            VaultWriteMode::Create,
        )
        .unwrap();
    let after_external = backend.refresh().unwrap();
    let stale_delete = mutate(
        &backend,
        &current,
        VaultMutation::RemoveItem {
            item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
        },
    )
    .unwrap_err();
    assert_eq!(stale_delete.kind(), VaultUiErrorKind::Conflict);
    let after_rejection = backend.refresh().unwrap();
    assert_eq!(after_rejection.revision, after_external.revision);
    assert_eq!(
        after_rejection.audit.event_count,
        after_external.audit.event_count
    );
    assert!(
        after_rejection
            .fields
            .iter()
            .any(|record| record.reference == external)
    );
    current = after_rejection;

    current = mutate(
        &backend,
        &current,
        VaultMutation::ChangeFieldKind {
            reference: field.clone(),
            kind: FieldKind::Text,
        },
    )
    .unwrap();
    let moved: VaultReference = "jig://Production/RENAMED".parse().unwrap();
    current = mutate(
        &backend,
        &current,
        VaultMutation::RenameField {
            source: field,
            destination: moved,
        },
    )
    .unwrap();
    let destination = jig_vault::VaultItem::parse("jig://RenamedItem").unwrap();
    current = mutate(
        &backend,
        &current,
        VaultMutation::RenameItem {
            source: jig_vault::VaultItem::parse("jig://Production").unwrap(),
            destination: destination.clone(),
        },
    )
    .unwrap();

    current = mutate(
        &backend,
        &current,
        VaultMutation::SetLegacy {
            name: "old_token".to_owned(),
            value: SecretBytes::new(b"legacy-secret".to_vec()),
            mode: VaultWriteMode::Create,
        },
    )
    .unwrap();
    let converted: VaultReference = "jig://Imported/TOKEN".parse().unwrap();
    current = mutate(
        &backend,
        &current,
        VaultMutation::ConvertLegacy {
            name: "old_token".to_owned(),
            reference: converted.clone(),
            kind: FieldKind::Concealed,
        },
    )
    .unwrap();
    assert!(current.legacy_secrets.is_empty());
    assert!(
        current
            .fields
            .iter()
            .any(|record| record.reference == converted)
    );

    current = mutate(
        &backend,
        &current,
        VaultMutation::RemoveField {
            reference: converted,
        },
    )
    .unwrap();
    let empty = mutate(
        &backend,
        &current,
        VaultMutation::RemoveItem { item: destination },
    )
    .unwrap();
    assert!(empty.fields.is_empty());
}
