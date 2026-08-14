use super::*;

fn field(reference: &str) -> VaultReference {
    VaultReference::parse(reference).unwrap()
}

fn item(selector: &str) -> VaultItem {
    VaultItem::parse(selector).unwrap()
}

fn new_vault() -> (tempfile::TempDir, Vault) {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    (temp, vault)
}

#[test]
fn snapshot_is_a_verified_disjoint_canonical_and_legacy_view() {
    let (_temp, vault) = new_vault();
    vault
        .set_field(
            &passphrase(),
            field("jig://Production/TOKEN"),
            FieldKind::Concealed,
            SecretBytes::new(b"canonical-sentinel".to_vec()),
        )
        .unwrap();
    vault
        .set_secret(
            &passphrase(),
            "legacy_token",
            SecretBytes::new(b"legacy-sentinel-one".to_vec()),
        )
        .unwrap();
    vault
        .set_secret(
            &passphrase(),
            "legacy/token/extra",
            SecretBytes::new(b"legacy-sentinel-two".to_vec()),
        )
        .unwrap();

    let snapshot = vault.snapshot(&passphrase()).unwrap();
    assert_eq!(snapshot.format_version, FORMAT_VERSION);
    assert!(!snapshot.vault_id.is_empty());
    assert_eq!(snapshot.fields.len(), 1);
    assert_eq!(
        snapshot.fields[0].reference,
        field("jig://Production/TOKEN")
    );
    assert_eq!(
        snapshot
            .legacy_secrets
            .iter()
            .map(|record| record.name.as_str())
            .collect::<Vec<_>>(),
        vec!["legacy/token/extra", "legacy_token"]
    );
    assert_eq!(snapshot.audit.event_count, 4);
    let debug = format!("{snapshot:?}");
    assert!(!debug.contains("canonical-sentinel"));
    assert!(!debug.contains("legacy-sentinel"));

    let audit = vault.store.read_audit_text().unwrap().unwrap();
    std::fs::write(
        vault.store.audit_path(),
        audit.replace("\"action\":\"secret_set\"", "\"action\":\"secret_get\""),
    )
    .unwrap();
    let error = vault.snapshot(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
}

#[test]
fn version_one_snapshot_remains_readable_and_reports_its_format() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&vault.store);

    let snapshot = vault
        .snapshot(&cli_generated_v1_fixture_passphrase())
        .unwrap();

    assert_eq!(snapshot.format_version, V1_FORMAT_VERSION);
    assert_eq!(snapshot.fields.len(), 1);
    assert!(snapshot.legacy_secrets.is_empty());
    assert_eq!(snapshot.audit.event_count, 2);
}

#[test]
fn activity_is_newest_first_bounded_and_metadata_only() {
    let (_temp, vault) = new_vault();
    vault
        .set_secret(
            &passphrase(),
            "legacy_token",
            SecretBytes::new(b"activity-secret-sentinel".to_vec()),
        )
        .unwrap();
    vault
        .set_field(
            &passphrase(),
            field("jig://Production/TOKEN"),
            FieldKind::Concealed,
            SecretBytes::new(b"field-secret-sentinel".to_vec()),
        )
        .unwrap();
    vault
        .change_field_kind(
            &passphrase(),
            field("jig://Production/TOKEN"),
            FieldKind::Text,
        )
        .unwrap();

    let activity = vault.activity(&passphrase(), 3).unwrap();
    assert_eq!(activity.audit.torn_tail_bytes, 0);
    assert_eq!(activity.records.len(), 3);
    assert_eq!(activity.records[0].action, "field_kind_change");
    assert_eq!(
        activity.records[0].subject.as_deref(),
        Some("jig://Production/TOKEN")
    );
    assert_eq!(activity.records[0].outcome.as_deref(), Some("applied"));
    assert_eq!(activity.records[1].action, "field_batch_apply");
    assert_eq!(activity.records[2].action, "secret_set");
    assert_eq!(activity.records[2].subject.as_deref(), Some("legacy_token"));
    let debug = format!("{activity:?}");
    assert!(!debug.contains("activity-secret-sentinel"));
    assert!(!debug.contains("field-secret-sentinel"));
    assert!(!debug.contains("previous_mac"));
    assert!(!debug.contains("details"));

    for limit in [0, MAX_VAULT_ACTIVITY_RECORDS + 1] {
        let error = vault.activity(&passphrase(), limit).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    }

    let audit = vault.store.read_audit_text().unwrap().unwrap();
    std::fs::write(vault.store.audit_path(), format!("{audit}{{\"partial\"")).unwrap();
    let activity = vault.activity(&passphrase(), 3).unwrap();
    assert!(activity.audit.torn_tail_bytes > 0);
    assert_eq!(activity.records.len(), 3);

    std::fs::write(
        vault.store.audit_path(),
        audit.replace(
            "\"action\":\"field_kind_change\"",
            "\"action\":\"field_kind_changed\"",
        ),
    )
    .unwrap();
    let error = vault.activity(&passphrase(), 10).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
}

#[test]
fn field_kind_change_preserves_value_and_creation_time() {
    let (_temp, vault) = new_vault();
    let reference = field("jig://Production/TOKEN");
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(b"kind-change-sentinel".to_vec()),
        )
        .unwrap();
    let before = vault.snapshot(&passphrase()).unwrap().fields.remove(0);

    let result = vault
        .change_field_kind(&passphrase(), reference.clone(), FieldKind::Text)
        .unwrap();
    assert!(result.changed);
    assert_eq!(result.previous_kind, FieldKind::Concealed);
    assert_eq!(result.kind, FieldKind::Text);
    let before_noop_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_noop_audit = vault.store.read_audit_text().unwrap().unwrap();
    let unchanged = vault
        .change_field_kind(&passphrase(), reference.clone(), FieldKind::Text)
        .unwrap();
    assert!(!unchanged.changed);
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        before_noop_vault
    );
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_noop_audit
    );

    let after = vault.snapshot(&passphrase()).unwrap().fields.remove(0);
    assert_eq!(after.created_at_ms, before.created_at_ms);
    assert!(after.updated_at_ms >= before.updated_at_ms);
    assert_eq!(after.kind, FieldKind::Text);
    let opened = vault.store.open_unlocked(&passphrase()).unwrap();
    assert_eq!(
        opened
            .secret_value(&reference.to_secret_name())
            .unwrap()
            .as_slice(),
        b"kind-change-sentinel"
    );
}

#[test]
fn changing_short_text_to_concealed_fails_without_audit_or_state_change() {
    let (_temp, vault) = new_vault();
    let reference = field("jig://Production/EMPTY");
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Text,
            SecretBytes::new(Vec::new()),
        )
        .unwrap();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    let error = vault
        .change_field_kind(&passphrase(), reference, FieldKind::Concealed)
        .unwrap_err();

    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
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
fn field_rename_is_atomic_on_collision_and_preserves_encrypted_entry() {
    let (_temp, vault) = new_vault();
    let source = field("jig://Production/TOKEN");
    let destination = field("jig://Staging/TOKEN");
    vault
        .set_field(
            &passphrase(),
            source.clone(),
            FieldKind::Text,
            SecretBytes::new(b"rename-source-sentinel".to_vec()),
        )
        .unwrap();
    vault
        .set_field(
            &passphrase(),
            destination.clone(),
            FieldKind::Concealed,
            SecretBytes::new(b"rename-target-sentinel".to_vec()),
        )
        .unwrap();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();
    let collision = vault
        .rename_field(&passphrase(), source.clone(), destination.clone())
        .unwrap_err();
    assert_eq!(collision.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        before_vault
    );
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );

    vault
        .remove_field(&passphrase(), destination.clone())
        .unwrap();
    let before = vault
        .snapshot(&passphrase())
        .unwrap()
        .fields
        .into_iter()
        .find(|record| record.reference == source)
        .unwrap();
    let renamed = vault
        .rename_field(&passphrase(), source.clone(), destination.clone())
        .unwrap();
    assert_eq!(renamed.removed, vec![source]);
    assert_eq!(renamed.changed, vec![destination.clone()]);
    let after = vault.snapshot(&passphrase()).unwrap().fields.remove(0);
    assert_eq!(after.reference, destination);
    assert_eq!(after.kind, FieldKind::Text);
    assert_eq!(after.created_at_ms, before.created_at_ms);
    assert!(after.updated_at_ms >= before.updated_at_ms);
    let opened = vault.store.open_unlocked(&passphrase()).unwrap();
    assert_eq!(
        opened
            .secret_value(&destination.to_secret_name())
            .unwrap()
            .as_slice(),
        b"rename-source-sentinel"
    );
}

#[test]
fn item_rename_and_remove_touch_only_canonical_fields() {
    let (_temp, vault) = new_vault();
    for (reference, value) in [
        ("jig://Old/A", b"item-value-a".as_slice()),
        ("jig://Old/B", b"item-value-b".as_slice()),
        ("jig://Other/C", b"item-value-c".as_slice()),
    ] {
        vault
            .set_field(
                &passphrase(),
                field(reference),
                FieldKind::Concealed,
                SecretBytes::new(value.to_vec()),
            )
            .unwrap();
    }
    vault
        .set_secret(
            &passphrase(),
            "Old/legacy/extra",
            SecretBytes::new(b"legacy-item-sentinel".to_vec()),
        )
        .unwrap();

    let renamed = vault
        .rename_item(&passphrase(), item("jig://Old"), item("jig://New"))
        .unwrap();
    assert_eq!(
        renamed.changed,
        vec![field("jig://New/A"), field("jig://New/B")]
    );
    assert_eq!(
        renamed.removed,
        vec![field("jig://Old/A"), field("jig://Old/B")]
    );
    let snapshot = vault.snapshot(&passphrase()).unwrap();
    assert!(
        snapshot
            .fields
            .iter()
            .any(|record| record.reference == field("jig://Other/C"))
    );
    assert_eq!(snapshot.legacy_secrets[0].name, "Old/legacy/extra");

    let removed = vault.remove_item(&passphrase(), item("jig://New")).unwrap();
    assert_eq!(
        removed.removed,
        vec![field("jig://New/A"), field("jig://New/B")]
    );
    let snapshot = vault.snapshot(&passphrase()).unwrap();
    assert_eq!(snapshot.fields.len(), 1);
    assert_eq!(snapshot.fields[0].reference, field("jig://Other/C"));
    assert_eq!(snapshot.legacy_secrets[0].name, "Old/legacy/extra");
}

#[test]
fn item_rename_collision_is_all_or_nothing() {
    let (_temp, vault) = new_vault();
    for reference in ["jig://Old/A", "jig://Old/B", "jig://New/B"] {
        vault
            .set_field(
                &passphrase(),
                field(reference),
                FieldKind::Concealed,
                SecretBytes::new(format!("value-for-{reference}").into_bytes()),
            )
            .unwrap();
    }
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    let error = vault
        .rename_item(&passphrase(), item("jig://Old"), item("jig://New"))
        .unwrap_err();

    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
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
fn legacy_conversion_is_atomic_and_preserves_value_and_creation_time() {
    let (_temp, vault) = new_vault();
    vault
        .set_secret(
            &passphrase(),
            "legacy/token/extra",
            SecretBytes::new(b"legacy-conversion-sentinel".to_vec()),
        )
        .unwrap();
    let before = vault
        .snapshot(&passphrase())
        .unwrap()
        .legacy_secrets
        .remove(0);
    let target = field("jig://Production/TOKEN");

    let converted = vault
        .convert_legacy_secret(
            &passphrase(),
            "legacy/token/extra",
            target.clone(),
            FieldKind::Text,
        )
        .unwrap();

    assert_eq!(converted.secret_name, "legacy/token/extra");
    assert_eq!(converted.reference, target);
    assert_eq!(converted.kind, FieldKind::Text);
    let snapshot = vault.snapshot(&passphrase()).unwrap();
    assert!(snapshot.legacy_secrets.is_empty());
    assert_eq!(snapshot.fields[0].reference, target);
    assert_eq!(snapshot.fields[0].kind, FieldKind::Text);
    assert_eq!(snapshot.fields[0].created_at_ms, before.created_at_ms);
    let opened = vault.store.open_unlocked(&passphrase()).unwrap();
    assert_eq!(
        opened
            .secret_value(&target.to_secret_name())
            .unwrap()
            .as_slice(),
        b"legacy-conversion-sentinel"
    );
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    assert!(audit.contains("\"action\":\"legacy_secret_convert\""));
    assert!(!audit.contains("legacy-conversion-sentinel"));
}

#[test]
fn legacy_conversion_rejects_canonical_sources_and_destination_collisions() {
    let (_temp, vault) = new_vault();
    vault
        .set_secret(
            &passphrase(),
            "legacy_token",
            SecretBytes::new(b"legacy-collision-sentinel".to_vec()),
        )
        .unwrap();
    let target = field("jig://Production/TOKEN");
    vault
        .set_field(
            &passphrase(),
            target.clone(),
            FieldKind::Concealed,
            SecretBytes::new(b"target-collision-sentinel".to_vec()),
        )
        .unwrap();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    let collision = vault
        .convert_legacy_secret(&passphrase(), "legacy_token", target, FieldKind::Text)
        .unwrap_err();
    assert_eq!(collision.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        before_vault
    );
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );

    let canonical = vault
        .convert_legacy_secret(
            &passphrase(),
            "Production/TOKEN",
            field("jig://Other/TOKEN"),
            FieldKind::Text,
        )
        .unwrap_err();
    assert_eq!(canonical.kind(), VaultErrorKind::InvalidInput);
    assert!(canonical.to_string().contains("already a canonical field"));
}

#[test]
fn management_mutations_require_version_two_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&vault.store);
    let passphrase = cli_generated_v1_fixture_passphrase();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    let error = vault
        .change_field_kind(
            &passphrase,
            field("jig://Production/RESTIC_PASSWORD"),
            FieldKind::Text,
        )
        .unwrap_err();

    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(error.to_string().contains("jig vault migrate --to 2"));
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
fn conditional_field_writes_reject_concurrent_create_and_remove_without_mutation() {
    let (temp, vault) = new_vault();
    let external = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let reference = field("jig://Production/TOKEN");
    external
        .write_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(b"external-create-sentinel".to_vec()),
            VaultWriteMode::Create,
        )
        .unwrap();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    let collision = vault
        .write_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(b"tui-create-sentinel".to_vec()),
            VaultWriteMode::Create,
        )
        .unwrap_err();
    assert_eq!(collision.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        before_vault
    );
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );

    external
        .remove_field(&passphrase(), reference.clone())
        .unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();
    let missing = vault
        .write_field(
            &passphrase(),
            reference,
            FieldKind::Concealed,
            SecretBytes::new(b"tui-replace-sentinel".to_vec()),
            VaultWriteMode::Replace,
        )
        .unwrap_err();
    assert_eq!(missing.kind(), VaultErrorKind::NotFound);
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );

    let missing_remove = vault
        .remove_field_required(&passphrase(), field("jig://Production/TOKEN"))
        .unwrap_err();
    assert_eq!(missing_remove.kind(), VaultErrorKind::NotFound);
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );
}

#[test]
fn conditional_legacy_writes_reject_concurrent_create_and_remove_without_mutation() {
    let (temp, vault) = new_vault();
    let external = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    external
        .write_secret(
            &passphrase(),
            "old_token",
            SecretBytes::new(b"external-legacy-sentinel".to_vec()),
            VaultWriteMode::Create,
        )
        .unwrap();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    let collision = vault
        .write_secret(
            &passphrase(),
            "old_token",
            SecretBytes::new(b"tui-legacy-create-sentinel".to_vec()),
            VaultWriteMode::Create,
        )
        .unwrap_err();
    assert_eq!(collision.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        before_vault
    );
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );

    external.remove_secret(&passphrase(), "old_token").unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();
    let missing = vault
        .write_secret(
            &passphrase(),
            "old_token",
            SecretBytes::new(b"tui-legacy-replace-sentinel".to_vec()),
            VaultWriteMode::Replace,
        )
        .unwrap_err();
    assert_eq!(missing.kind(), VaultErrorKind::NotFound);
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );

    let missing_remove = vault
        .remove_secret_required(&passphrase(), "old_token")
        .unwrap_err();
    assert_eq!(missing_remove.kind(), VaultErrorKind::NotFound);
    assert_eq!(
        vault.store.read_audit_text().unwrap().unwrap(),
        before_audit
    );
}
