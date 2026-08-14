use super::*;

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
fn planned_import_applies_only_to_its_exact_ordered_field_set() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let existing = VaultReference::parse("jig://Production/EXISTING").unwrap();
    let new = VaultReference::parse("jig://Production/NEW").unwrap();
    vault
        .set_field(
            &passphrase(),
            existing.clone(),
            FieldKind::Text,
            SecretBytes::new(b"before".to_vec()),
        )
        .unwrap();

    let plan = vault
        .plan_import_fields(&passphrase(), &[existing.clone(), new.clone()])
        .unwrap();
    assert_eq!(
        plan.fields()
            .map(|(reference, existed)| (reference.clone(), existed))
            .collect::<Vec<_>>(),
        vec![(existing.clone(), true), (new.clone(), false)]
    );
    vault
        .import_fields_if_unchanged(
            &passphrase(),
            vec![
                FieldMutation::set(
                    existing,
                    FieldKind::Text,
                    SecretBytes::new(b"after".to_vec()),
                ),
                FieldMutation::set(
                    new.clone(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"new-secret".to_vec()),
                ),
            ],
            plan,
            true,
        )
        .unwrap();

    let mismatch_plan = vault
        .plan_import_fields(&passphrase(), std::slice::from_ref(&new))
        .unwrap();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();
    let mismatch = vault
        .import_fields_if_unchanged(
            &passphrase(),
            vec![import_set(
                "jig://Production/DIFFERENT",
                FieldKind::Text,
                b"different",
            )],
            mismatch_plan,
            true,
        )
        .unwrap_err();
    assert_eq!(mismatch.kind(), VaultErrorKind::InvalidInput);
    assert!(mismatch.message().contains("do not match"));
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
fn planned_import_rejects_intervening_vault_state_without_audit_or_write() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let target = VaultReference::parse("jig://Production/TARGET").unwrap();
    let plan = vault
        .plan_import_fields(&passphrase(), std::slice::from_ref(&target))
        .unwrap();

    vault
        .set_field(
            &passphrase(),
            VaultReference::parse("jig://Production/UNRELATED").unwrap(),
            FieldKind::Text,
            SecretBytes::new(b"changed".to_vec()),
        )
        .unwrap();
    let before_vault = vault.store.read_vault_text().unwrap().unwrap();
    let before_audit = vault.store.read_audit_text().unwrap().unwrap();

    let error = vault
        .import_fields_if_unchanged(
            &passphrase(),
            vec![FieldMutation::set(
                target,
                FieldKind::Concealed,
                SecretBytes::new(b"planned-secret".to_vec()),
            )],
            plan,
            true,
        )
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert!(error.message().contains("changed since the import preview"));
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
