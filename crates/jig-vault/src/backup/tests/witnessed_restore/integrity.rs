//! Archived checkpoint integrity and recovery across witness profiles.

use super::*;
use crate::crypto::{decode_array, derive_wrap_key, open, random_array, seal};
use crate::format::{AeadRole, VaultFile, decode_b64_array, payload_aad};

fn with_anchor(bytes: &[u8], mac: &str) -> Vec<u8> {
    let mut file: VaultFile = serde_json::from_slice(bytes).unwrap();
    let salt = decode_b64_array::<SALT_LEN>("salt", &file.header.salt_b64).unwrap();
    let key = derive_wrap_key(&test_passphrase(), &salt, &file.header.kdf).unwrap();
    let nonce = decode_b64_array::<NONCE_LEN>("nonce", &file.wrapped_dek_nonce_b64).unwrap();
    let plain = open(
        &key,
        &nonce,
        &payload_aad(&file.header, AeadRole::WrappedDek),
        &B64.decode(&file.wrapped_dek_b64).unwrap(),
    )
    .unwrap();
    let dek = Zeroizing::new(decode_array::<KEY_LEN>("key", &plain).unwrap());
    let nonce = decode_b64_array::<NONCE_LEN>("nonce", &file.state_nonce_b64).unwrap();
    let plain = open(
        &dek,
        &nonce,
        &payload_aad(&file.header, AeadRole::State),
        &B64.decode(&file.state_b64).unwrap(),
    )
    .unwrap();
    let mut state: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    state["mutation_audit_mac"] = mac.into();
    let plain = Zeroizing::new(serde_json::to_vec(&state).unwrap());
    let nonce = random_array::<NONCE_LEN>().unwrap();
    file.state_b64 = B64.encode(
        seal(
            &dek,
            &nonce,
            &payload_aad(&file.header, AeadRole::State),
            &plain,
        )
        .unwrap(),
    );
    file.state_nonce_b64 = B64.encode(nonce);
    serde_json::to_vec(&file).unwrap()
}

#[test]
fn inconsistent_archived_checkpoints_leave_target_and_witness_unchanged() {
    let temp = private_temp();
    let (home, vault) = source(temp.path());
    let bytes = fs::read(home.join("vault.json")).unwrap();
    let audit = fs::read_to_string(home.join("audit.jsonl")).unwrap();
    let first = audit.lines().next().unwrap();
    let event: serde_json::Value = serde_json::from_str(first).unwrap();
    let wrong_anchor = with_anchor(&bytes, event["mac"].as_str().unwrap());
    let (id, version) = inspect_embedded_vault(&bytes).unwrap();
    let witness = WitnessLocation::for_home(&home)
        .unwrap()
        .open_existing()
        .unwrap()
        .unwrap();
    let before = serde_json::to_vec(&witness.read_record(&id).unwrap()).unwrap();
    for (name, envelope, chain) in [
        ("missing-anchor", bytes.as_slice(), format!("{first}\n")),
        ("wrong-generation", wrong_anchor.as_slice(), audit),
    ] {
        let sealed = seal_archive(
            &test_passphrase(),
            &id,
            version,
            envelope,
            chain.as_bytes(),
            now_ms(),
        )
        .unwrap();
        let archive = temp.path().join(format!("{name}.backup"));
        PreparedPrivateFile::prepare(&archive, sealed.bytes, false)
            .unwrap()
            .install()
            .unwrap();
        let target = temp.path().join(name);
        let error = restore(&archive, &target).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
        assert!(!target.exists());
        assert!(journal_paths(&target).is_empty());
        assert!(staging_dirs(temp.path()).is_empty());
        assert_eq!(
            serde_json::to_vec(&witness.read_record(&id).unwrap()).unwrap(),
            before
        );
    }
    vault.verify_audit(&test_passphrase()).unwrap();
}

#[test]
fn an_orphan_restore_allows_first_call_initialization_without_removing_staging() {
    let temp = private_temp();
    let (home, source) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let target = temp.path().join("initialized");
    crate::store::arm_fault_for_test(FaultPoint::AfterJournal);
    restore(&archive, &target).unwrap_err();
    let [staging] = staging_dirs(temp.path()).try_into().unwrap();
    let staging = temp.path().join(staging);
    let staged = staged_bytes(&staging);
    let vault = Vault::resolve_for_test(Some(target.clone())).unwrap();
    assert!(!target.exists());
    vault.init(&test_passphrase()).unwrap();
    assert!(vault.list_fields(&test_passphrase()).unwrap().is_empty());
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(journal_paths(&target).is_empty());
    assert_eq!(staged_bytes(&staging), staged);
    assert_ne!(
        vault.snapshot(&test_passphrase()).unwrap().vault_id,
        source.snapshot(&test_passphrase()).unwrap().vault_id
    );
}

#[test]
fn initialization_preserves_marker_bound_restore_recovery() {
    for missing_journal in [false, true] {
        let temp = private_temp();
        let (_archive, target) = pending_restore(temp.path());
        let [journal] = journal_paths(&target).try_into().unwrap();
        if missing_journal {
            fs::remove_file(journal).unwrap();
        }
        let vault = Vault::resolve_for_test(Some(target.clone())).unwrap();
        assert!(!target.exists());
        let error = vault.init(&test_passphrase()).unwrap_err();
        if missing_journal {
            assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
            assert!(!target.exists());
            assert!(Vault::status(Some(target)).unwrap().pending_transaction);
        } else {
            assert_eq!(error.kind(), VaultErrorKind::AlreadyExists, "{error}");
            assert_eq!(vault.list_fields(&test_passphrase()).unwrap().len(), 1);
        }
    }
}

#[test]
fn independent_profiles_diverge_and_recover_through_an_authenticated_backup() {
    let temp = private_temp();
    let profile_a = temp.path().join("ExampleProfileA/witness");
    let profile_b = temp.path().join("ExampleProfileB/witness");
    let home;
    {
        let _profile = crate::store::witness::override_root_for_test(profile_a.clone());
        (home, _) = source(temp.path());
    }
    let archive = temp.path().join("current.backup");
    {
        let _profile = crate::store::witness::override_root_for_test(profile_b);
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        vault.list_fields(&test_passphrase()).unwrap();
        set_text(&vault, "jig://Example/LATEST", b"current value");
        backup(&home, &archive, &test_passphrase());
    }
    let _profile = crate::store::witness::override_root_for_test(profile_a);
    let vault = Vault::resolve_for_test(Some(home)).unwrap();
    let error = vault.list_fields(&test_passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    for expected in [
        "newer than its witnessed checkpoint",
        "Operator step:",
        "encrypted backup",
        "Agents must ask the operator",
        "Never delete or edit",
    ] {
        assert!(error.to_string().contains(expected), "{error}");
    }
    let restored = restore(&archive, &temp.path().join("recovered")).unwrap();
    let recovered = Vault::resolve_for_test(Some(restored.root)).unwrap();
    assert_eq!(recovered.list_fields(&test_passphrase()).unwrap().len(), 2);
    recovered.verify_audit(&test_passphrase()).unwrap();
    assert!(vault.list_fields(&test_passphrase()).is_err());
}
