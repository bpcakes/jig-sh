use super::*;
use crate::crypto::{derive_audit_key, random_array, seal};

fn field(reference: &str) -> VaultReference {
    VaultReference::parse(reference).unwrap()
}

fn read_file(store: &VaultStore) -> VaultFile {
    serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap()
}

fn new_store() -> (tempfile::TempDir, VaultStore) {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    (temp, store)
}

fn set_text(store: &VaultStore, reference: &str, value: &[u8]) {
    store
        .write_field(
            &passphrase(),
            field(reference),
            FieldKind::Text,
            SecretBytes::new(value.to_vec()),
            VaultWriteMode::Upsert,
        )
        .unwrap();
}

/// Generation and mutation MAC recorded inside the encrypted state.
fn state_anchor(store: &VaultStore, passphrase: &SecretString) -> (u64, String) {
    let vault = store.open_unlocked(passphrase).unwrap();
    let fields = vault.state.v3.as_ref().unwrap();
    (fields.generation, fields.mutation_audit_mac.clone())
}

fn assert_anchored_to_last_event(store: &VaultStore, action: &str, generation: u64) {
    let file = read_file(store);
    assert_eq!(file.header.version, V3_FORMAT_VERSION);
    assert_eq!(file.header.generation, Some(generation));
    let last = audit_events(store).pop().unwrap();
    assert_eq!(last.action, action);
    assert_eq!(last.details["generation"], generation);
    assert_eq!(state_anchor(store, &passphrase()), (generation, last.mac));
}

/// Decrypts state, lets `edit` change it and the public file, then reseals
/// the state with the vault DEK under the edited header's state AAD.
fn reseal_state_json(
    store: &VaultStore,
    passphrase: &SecretString,
    edit: impl FnOnce(&mut VaultFile, &mut serde_json::Value),
) {
    let mut file = read_file(store);
    let mut state: serde_json::Value =
        serde_json::from_slice(&decrypt_state_for_test(&file, passphrase)).unwrap();
    let dek = store.open_unlocked(passphrase).unwrap().dek;
    edit(&mut file, &mut state);
    let plaintext = Zeroizing::new(serde_json::to_vec(&state).unwrap());
    let nonce = random_array::<NONCE_LEN>().unwrap();
    let ciphertext = seal(
        &dek,
        &nonce,
        &payload_aad(&file.header, AeadRole::State),
        &plaintext,
    )
    .unwrap();
    file.state_nonce_b64 = B64.encode(nonce);
    file.state_b64 = B64.encode(ciphertext);
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();
}

#[test]
fn new_vault_anchors_generation_one_to_its_independent_initialization_event() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();

    assert_anchored_to_last_event(&store, "vault_initialized", 1);
    let vault = store.open_unlocked(&passphrase()).unwrap();
    let root = vault.state.v3.as_ref().unwrap().audit_root.as_bytes();
    assert_eq!(vault.audit_key.as_ref(), root);
    // A new root is random, never the legacy DEK-derived audit key.
    assert_ne!(derive_audit_key(&vault.dek).unwrap().as_ref(), root);
    assert!(!format!("{vault:?}").contains(&B64.encode(root)));
}

#[test]
fn ordinary_saves_advance_generation_and_keep_the_wrapped_dek() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    let initial = read_file(&store);

    set_text(&store, "jig://Example/FIRST", b"first value");
    assert_anchored_to_last_event(&store, "field_batch_apply", 2);
    set_text(&store, "jig://Example/SECOND", b"second value");
    assert_anchored_to_last_event(&store, "field_batch_apply", 3);

    let after = read_file(&store);
    assert_eq!(after.wrapped_dek_b64, initial.wrapped_dek_b64);
    assert_eq!(after.wrapped_dek_nonce_b64, initial.wrapped_dek_nonce_b64);
    assert_eq!(after.header.salt_b64, initial.header.salt_b64);
    assert_eq!(store.list_fields(&passphrase()).unwrap().len(), 2);
    assert_eq!(store.verify_audit(&passphrase()).unwrap().event_count, 3);
}

#[test]
fn no_op_edits_record_audit_only_without_advancing_generation() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    set_text(&store, "jig://Example/KEPT", b"kept value");
    let vault_before = store.read_vault_text().unwrap().unwrap();

    assert!(!store.remove_secret(&passphrase(), "absent_name").unwrap());
    let last = audit_events(&store).pop().unwrap();
    assert_eq!(last.action, "secret_remove");
    assert_eq!(last.details["removed"], false);
    assert!(last.details.get("generation").is_none());

    store
        .apply_field_batch(
            &passphrase(),
            vec![FieldMutation::remove(field("jig://Example/ABSENT"))],
        )
        .unwrap();
    let kind = store
        .change_field_kind(&passphrase(), field("jig://Example/KEPT"), FieldKind::Text)
        .unwrap();
    assert!(!kind.changed);

    assert_eq!(store.read_vault_text().unwrap().unwrap(), vault_before);
    assert_eq!(state_anchor(&store, &passphrase()).0, 2);
    let events = audit_events(&store);
    assert_eq!(events.len(), 4);
    assert_eq!(events[3].action, "field_batch_apply");
    assert!(events[3].details.get("generation").is_none());
    store.verify_audit(&passphrase()).unwrap();
}

#[test]
fn generation_tampering_and_downgrade_fail_closed() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    set_text(&store, "jig://Example/VALUE", b"generation two");
    let original = store.read_vault_text().unwrap().unwrap();

    let mut file = read_file(&store);
    file.header.generation = Some(3);
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();
    let error = store.list(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication);

    let mut file: serde_json::Value = serde_json::from_str(&original).unwrap();
    file["header"].as_object_mut().unwrap().remove("generation");
    store.write_vault_text(&file.to_string()).unwrap();
    let error = store.list(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Serialization);

    // A version 2 header cannot reuse version 3 ciphertext.
    let mut file: serde_json::Value = serde_json::from_str(&original).unwrap();
    file["header"]["version"] = serde_json::json!(V2_FORMAT_VERSION);
    file["header"].as_object_mut().unwrap().remove("generation");
    store.write_vault_text(&file.to_string()).unwrap();
    let error = store.list(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication);

    // A state whose own generation disagrees with its authenticated header
    // is rejected even though its ciphertext authenticates.
    store.write_vault_text(&original).unwrap();
    reseal_state_json(&store, &passphrase(), |_, state| {
        state["generation"] = serde_json::json!(7);
    });
    let error = store.list(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Serialization);
    assert!(error.to_string().contains("authenticated header"));
}

#[test]
fn state_security_fields_are_required_end_to_end() {
    for missing in ["audit_root_b64", "generation", "mutation_audit_mac"] {
        let (_temp, store) = new_store();
        store.init(&passphrase()).unwrap();
        reseal_state_json(&store, &passphrase(), |_, state| {
            state.as_object_mut().unwrap().remove(missing);
        });
        let damaged = store.read_vault_text().unwrap().unwrap();
        let error = store.list(&passphrase()).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::Serialization, "{missing}");
        let error = store
            .set_secret(
                &passphrase(),
                "after_damage",
                SecretBytes::new(b"never saved".to_vec()),
            )
            .unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::Serialization, "{missing}");
        assert_eq!(store.read_vault_text().unwrap().unwrap(), damaged);
    }
}

fn assert_migrated_to_v3(
    store: &VaultStore,
    passphrase: &SecretString,
    before_audit: &str,
    from_version: u32,
) {
    let file = read_file(store);
    assert_eq!(file.header.version, V3_FORMAT_VERSION);
    assert_eq!(file.header.generation, Some(1));
    let events = audit_events(store);
    let last = events.last().unwrap();
    assert_eq!(last.action, "vault_format_migrate");
    assert_eq!(last.details["from_version"], from_version);
    assert_eq!(last.details["to_version"], V3_FORMAT_VERSION);
    assert_eq!(last.details["generation"], 1);
    assert!(
        store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .starts_with(before_audit)
    );

    let vault = store.open_unlocked(passphrase).unwrap();
    let fields = vault.state.v3.as_ref().unwrap();
    assert_eq!(fields.generation, 1);
    assert_eq!(fields.mutation_audit_mac, last.mac);
    // The migrated root is exactly the legacy derived audit key, so every
    // historical MAC verifies unchanged.
    assert_eq!(
        fields.audit_root.as_bytes(),
        derive_audit_key(&vault.dek).unwrap().as_ref()
    );
    drop(vault);
    let audit = store.verify_audit(passphrase).unwrap();
    assert_eq!(audit.event_count, events.len());
}

#[test]
fn migration_matrix_upgrades_directly_and_refuses_downgrades() {
    for from_version in [V1_FORMAT_VERSION, V2_FORMAT_VERSION] {
        let (_temp, store) = new_store();
        init_with_format(&store, &passphrase(), from_version);
        store
            .set_secret(
                &passphrase(),
                "Example/LEGACY_NAME",
                SecretBytes::new(b"legacy secret bytes".to_vec()),
            )
            .unwrap();
        let before = store.list(&passphrase()).unwrap();
        let before_audit = store.read_audit_text().unwrap().unwrap();

        let migration = store.migrate(&passphrase(), V3_FORMAT_VERSION).unwrap();
        assert_eq!(
            migration,
            VaultMigration {
                from_version,
                to_version: V3_FORMAT_VERSION,
                changed: true,
            }
        );
        assert_migrated_to_v3(&store, &passphrase(), &before_audit, from_version);
        assert_eq!(store.list(&passphrase()).unwrap(), before);
        let vault = store.open_unlocked(&passphrase()).unwrap();
        assert_eq!(
            vault
                .secret_value(&SecretName::parse("Example/LEGACY_NAME").unwrap())
                .unwrap()
                .as_slice(),
            b"legacy secret bytes"
        );
        drop(vault);

        let vault_after = store.read_vault_text().unwrap().unwrap();
        let audit_after = store.read_audit_text().unwrap().unwrap();
        let same = store.migrate(&passphrase(), V3_FORMAT_VERSION).unwrap();
        assert!(!same.changed);
        let downgrade = store.migrate(&passphrase(), V2_FORMAT_VERSION).unwrap_err();
        assert_eq!(downgrade.kind(), VaultErrorKind::InvalidInput);
        assert!(
            downgrade
                .to_string()
                .contains("downgrades are not supported")
        );
        let wrong = SecretString::from("wrong passphrase for target".to_owned());
        for target in [0, V1_FORMAT_VERSION, 4, u32::MAX] {
            let error = store.migrate(&wrong, target).unwrap_err();
            assert_eq!(error.kind(), VaultErrorKind::InvalidInput, "{target}");
        }
        assert_eq!(store.read_vault_text().unwrap().unwrap(), vault_after);
        assert_eq!(store.read_audit_text().unwrap().unwrap(), audit_after);
    }
}

#[test]
fn legacy_two_step_migration_and_v2_noop_remain_available() {
    let (_temp, store) = new_store();
    init_v1(&store, &passphrase());
    assert!(
        store
            .migrate(&passphrase(), V2_FORMAT_VERSION)
            .unwrap()
            .changed
    );
    let v2_vault = store.read_vault_text().unwrap().unwrap();
    let v2_audit = store.read_audit_text().unwrap().unwrap();
    assert!(
        !store
            .migrate(&passphrase(), V2_FORMAT_VERSION)
            .unwrap()
            .changed
    );
    assert_eq!(store.read_vault_text().unwrap().unwrap(), v2_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), v2_audit);

    let migration = store.migrate(&passphrase(), V3_FORMAT_VERSION).unwrap();
    assert_eq!(migration.from_version, V2_FORMAT_VERSION);
    assert_migrated_to_v3(&store, &passphrase(), &v2_audit, V2_FORMAT_VERSION);
}

#[test]
fn weak_legacy_passphrases_still_unlock_and_migrate() {
    // Shorter than the new-passphrase floor: existing credentials are never
    // revalidated by unlock or migration.
    let weak = SecretString::from("weak-v2!".to_owned());
    assert!(crate::validate_new_vault_passphrase(&weak).is_err());
    for from_version in [V1_FORMAT_VERSION, V2_FORMAT_VERSION] {
        let (_temp, store) = new_store();
        init_with_format(&store, &weak, from_version);
        store.list(&weak).unwrap();
        let migration = store.migrate(&weak, V3_FORMAT_VERSION).unwrap();
        assert!(migration.changed);
        store.list(&weak).unwrap();
        store.verify_audit(&weak).unwrap();
    }
}

#[test]
fn v3_passphrase_change_commits_the_next_generation_with_a_stable_root() {
    let (_temp, store) = new_store();
    let old = passphrase();
    let new = SecretString::from("replacement passphrase for v3".to_owned());
    store.init(&old).unwrap();
    set_text(&store, "jig://Example/KEPT", b"kept across rekey");
    let before = store.open_unlocked(&old).unwrap();
    let root = *before.state.v3.as_ref().unwrap().audit_root.as_bytes();
    drop(before);

    store.change_passphrase_for_test(&old, &new).unwrap();

    assert!(store.open_unlocked(&old).is_err());
    let last = audit_events(&store).pop().unwrap();
    assert_eq!(last.action, "passphrase_change");
    assert_eq!(last.details["format_version"], V3_FORMAT_VERSION);
    assert_eq!(last.details["generation"], 3);
    let vault = store.open_unlocked(&new).unwrap();
    let fields = vault.state.v3.as_ref().unwrap();
    assert_eq!(fields.generation, 3);
    assert_eq!(fields.mutation_audit_mac, last.mac);
    assert_eq!(fields.audit_root.as_bytes(), &root);
    drop(vault);
    assert_eq!(read_file(&store).header.generation, Some(3));
    assert_eq!(store.list_fields(&new).unwrap().len(), 1);
    store.verify_audit(&new).unwrap();
}

#[test]
fn status_reports_the_unauthenticated_public_format_without_side_effects() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault");
    assert_eq!(
        Vault::status(Some(home.clone())).unwrap().format_version,
        None
    );
    std::fs::create_dir(&home).unwrap();
    assert_eq!(
        Vault::status(Some(home.clone())).unwrap().format_version,
        None
    );

    let store = VaultStore::resolve_for_test(Some(home.clone())).unwrap();
    for (version, label) in [
        (V1_FORMAT_VERSION, "v1"),
        (V2_FORMAT_VERSION, "v2"),
        (V3_FORMAT_VERSION, "v3"),
    ] {
        for file in ["vault.json", "audit.jsonl"] {
            let _ = std::fs::remove_file(home.join(file));
        }
        init_with_format(&store, &passphrase(), version);
        let entries_before = std::fs::read_dir(&home).unwrap().count();
        let status = Vault::status(Some(home.clone())).unwrap();
        assert_eq!(status.format_version, Some(version), "{label}");
        assert_eq!(std::fs::read_dir(&home).unwrap().count(), entries_before);
    }

    // Malformed files keep the existing status behavior and simply omit
    // the version instead of failing.
    std::fs::write(home.join("vault.json"), b"{not json").unwrap();
    let status = Vault::status(Some(home.clone())).unwrap();
    assert_eq!(status.home_state, VaultHomeState::Initialized);
    assert_eq!(status.format_version, None);
    std::fs::write(
        home.join("vault.json"),
        br#"{"header":{"magic":"other","version":3}}"#,
    )
    .unwrap();
    assert_eq!(
        Vault::status(Some(home.clone())).unwrap().format_version,
        None
    );

    #[cfg(unix)]
    {
        std::fs::remove_file(home.join("vault.json")).unwrap();
        let fifo = std::ffi::CString::new(
            home.join("vault.json")
                .into_os_string()
                .into_encoded_bytes(),
        )
        .unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let status = Vault::status(Some(home)).unwrap();
        assert_eq!(status.home_state, VaultHomeState::Initialized);
        assert_eq!(status.format_version, None);
    }
}

#[test]
fn frozen_v3_fixture_stays_anchored_to_its_last_state_mutation() {
    use crate::test_fixtures::{
        GENERATED_V3_AUDIT_EVENTS, GENERATED_V3_AUDIT_MAC, GENERATED_V3_CONCEALED_REFERENCE,
        GENERATED_V3_CONCEALED_VALUE, GENERATED_V3_GENERATION, GENERATED_V3_MUTATION_MAC,
        GENERATED_V3_VAULT_ID, generated_v3_passphrase, install_generated_v3_fixture,
    };
    let (_temp, store) = new_store();
    install_generated_v3_fixture(&store);
    let passphrase = generated_v3_passphrase();

    let file = read_file(&store);
    assert_eq!(file.header.vault_id, GENERATED_V3_VAULT_ID);
    assert_eq!(file.header.generation, Some(GENERATED_V3_GENERATION));
    // Backup events after the last mutation are audit-only, so the state
    // still names the mutation event rather than the latest audit record.
    assert_eq!(
        state_anchor(&store, &passphrase),
        (
            GENERATED_V3_GENERATION,
            GENERATED_V3_MUTATION_MAC.to_owned()
        )
    );
    let audit = store.verify_audit(&passphrase).unwrap();
    assert_eq!(audit.event_count, GENERATED_V3_AUDIT_EVENTS);
    assert_eq!(audit.latest_mac.as_deref(), Some(GENERATED_V3_AUDIT_MAC));
    let anchor = audit_events(&store)
        .into_iter()
        .find(|event| event.mac == GENERATED_V3_MUTATION_MAC)
        .unwrap();
    assert_eq!(anchor.details["generation"], GENERATED_V3_GENERATION);
    let vault = store.open_unlocked(&passphrase).unwrap();
    assert_eq!(
        vault
            .secret_value(&field(GENERATED_V3_CONCEALED_REFERENCE).to_secret_name())
            .unwrap()
            .as_slice(),
        GENERATED_V3_CONCEALED_VALUE
    );
    drop(vault);

    set_fixture_field(&store, &passphrase);
    assert_eq!(
        read_file(&store).header.generation,
        Some(GENERATED_V3_GENERATION + 1)
    );
}

fn set_fixture_field(store: &VaultStore, passphrase: &SecretString) {
    store
        .write_field(
            passphrase,
            field("jig://ExampleProject/NEXT"),
            FieldKind::Text,
            SecretBytes::new(b"next generation".to_vec()),
            VaultWriteMode::Create,
        )
        .unwrap();
}
