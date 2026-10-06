//! Data-encryption key rotation on a format 3 passphrase change, through
//! the public vault API. Rotation never revokes older envelopes, backups,
//! previously revealed values, or the audit key.

use super::*;
use crate::store::FaultPoint;

const VALUE: &[u8] = b"rotation-kept-value";
const REFERENCE: &str = "jig://Example/TOKEN";

fn credential(text: &str) -> SecretString {
    SecretString::from(text.to_owned())
}

fn new_vault() -> (tempfile::TempDir, Vault) {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    vault
        .set_field(
            &passphrase(),
            VaultReference::parse(REFERENCE).unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(VALUE.to_vec()),
        )
        .unwrap();
    (temp, vault)
}

fn envelope(vault: &Vault) -> VaultFile {
    serde_json::from_str(&vault.store.read_vault_text().unwrap().unwrap()).unwrap()
}

/// The DEK an attacker recovers from `file` using that file's credential.
pub(super) fn extract_dek(file: &VaultFile, credential: &SecretString) -> Zeroizing<[u8; KEY_LEN]> {
    let salt = decode_b64_array::<SALT_LEN>("vault salt", &file.header.salt_b64).unwrap();
    let wrap_key = derive_wrap_key(credential, &salt, &file.header.kdf).unwrap();
    let nonce =
        decode_b64_array::<NONCE_LEN>("wrapped key nonce", &file.wrapped_dek_nonce_b64).unwrap();
    let wrapped = B64.decode(&file.wrapped_dek_b64).unwrap();
    let aad = payload_aad(&file.header, AeadRole::WrappedDek);
    let dek = open(&wrap_key, &nonce, &aad, &wrapped).unwrap();
    Zeroizing::new(<[u8; KEY_LEN]>::try_from(dek.as_slice()).unwrap())
}

/// Whether `dek` authenticates `file`'s state under that file's own state
/// AAD and nonce.
pub(super) fn dek_opens_state(file: &VaultFile, dek: &[u8; KEY_LEN]) -> bool {
    let nonce = decode_b64_array::<NONCE_LEN>("state nonce", &file.state_nonce_b64).unwrap();
    let ciphertext = B64.decode(&file.state_b64).unwrap();
    let aad = payload_aad(&file.header, AeadRole::State);
    open(dek, &nonce, &aad, &ciphertext).is_ok()
}

fn committed_generation(vault: &Vault) -> u64 {
    let witness = vault.store.witness().open_existing().unwrap().unwrap();
    let record = witness
        .read_record(&envelope(vault).header.vault_id)
        .unwrap()
        .unwrap();
    assert!(record.pending.is_none());
    record.committed.unwrap().generation
}

/// The candidate envelope a pending in-place transaction recorded.
fn journal_candidate(vault: &Vault) -> Option<String> {
    let journals = vault.store.witness().root().join("journals");
    let entry = std::fs::read_dir(journals).ok()?.next()?.unwrap();
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap();
    Some(
        journal["payload"]["in_place"]["candidate_envelope"]
            .as_str()?
            .to_owned(),
    )
}

#[test]
fn a_dek_extracted_before_rotation_never_decrypts_a_later_state() {
    let (_temp, vault) = new_vault();
    let credentials = [
        passphrase(),
        credential("first rotated passphrase for this vault"),
        credential("second rotated passphrase for this vault"),
    ];
    let first = envelope(&vault);
    let mut extracted = vec![extract_dek(&first, &credentials[0])];
    assert!(dek_opens_state(&first, &extracted[0]));

    for step in 1..credentials.len() {
        vault
            .change_passphrase(&credentials[step - 1], &credentials[step])
            .unwrap();
        let current = envelope(&vault);
        // Every earlier key, recovered with its own credential, fails to
        // authenticate the new state under the new state AAD and nonce.
        for old in &extracted {
            assert!(!dek_opens_state(&current, old), "rotation {step}");
        }
        let fresh = extract_dek(&current, &credentials[step]);
        assert!(dek_opens_state(&current, &fresh));
        assert!(extracted.iter().all(|old| old.as_ref() != fresh.as_ref()));
        extracted.push(fresh);
    }

    let (newest, older) = credentials.split_last().unwrap();
    assert_eq!(vault.list_fields(newest).unwrap().len(), 1);
    for old in older {
        let error = vault.list_fields(old).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::Authentication);
    }
}

#[test]
fn rotation_keeps_identity_fields_timestamps_and_the_audit_chain() {
    let (_temp, vault) = new_vault();
    let new = credential("replacement passphrase for this rotation");
    let before = envelope(&vault);
    let before_fields = vault.list_fields(&passphrase()).unwrap();
    let opened = vault.store.open_unlocked(&passphrase()).unwrap();
    let root = *opened.state.v3.as_ref().unwrap().audit_root.as_bytes();
    drop(opened);
    let events = audit_events(&vault.store).len();

    vault.change_passphrase(&passphrase(), &new).unwrap();

    let after = envelope(&vault);
    assert_eq!(after.header.vault_id, before.header.vault_id);
    assert_eq!(after.header.created_at_ms, before.header.created_at_ms);
    assert_eq!(vault.list_fields(&new).unwrap(), before_fields);
    let opened = vault.store.open_unlocked(&new).unwrap();
    let name = VaultReference::parse(REFERENCE).unwrap().to_secret_name();
    assert_eq!(opened.secret_value(&name).unwrap().as_slice(), VALUE);
    assert_eq!(
        opened.state.v3.as_ref().unwrap().audit_root.as_bytes(),
        &root
    );
    drop(opened);

    let audit = audit_events(&vault.store);
    assert_eq!(audit.len(), events + 1);
    let last = audit.last().unwrap();
    assert_eq!(last.action, "passphrase_change");
    assert_eq!(last.details["format_version"], V3_FORMAT_VERSION);
    assert_eq!(last.details["generation"], 3);
    let text = vault.store.read_audit_text().unwrap().unwrap();
    for secret in [
        "rotation-kept-value",
        "correct horse battery staple",
        "replacement passphrase for this rotation",
    ] {
        assert!(!text.contains(secret));
    }
    vault.verify_audit(&new).unwrap();

    // Success was reported only with the rotated state committed.
    assert_eq!(committed_generation(&vault), 3);
}

#[cfg(unix)]
#[test]
fn an_exec_prepared_before_a_rotation_finishes_after_it() {
    let (_temp, vault) = new_vault();
    let new = credential("replacement passphrase for an in-flight exec");
    let request = VaultExec::new(
        vec![
            "sh".into(),
            "-c".into(),
            "test \"$TOKEN\" = rotation-kept-value".into(),
        ],
        vec![ExecEnvBinding::field(
            exec_var("TOKEN"),
            VaultReference::parse(REFERENCE).unwrap(),
        )],
    )
    .unwrap();
    let prepared = vault.store.prepare_exec(&passphrase(), request).unwrap();

    vault.change_passphrase(&passphrase(), &new).unwrap();

    let outcome = prepared.execute().unwrap();
    assert_eq!(outcome.exit_status, 0);
    let audit = audit_events(&vault.store);
    assert_eq!(audit.last().unwrap().action, "exec_finish");
    vault.verify_audit(&new).unwrap();
}

fn assert_secret_free(text: &str, point: FaultPoint) {
    for secret in [
        "rotation-kept-value",
        "correct horse battery staple",
        "replacement passphrase after a fault",
    ] {
        assert!(!text.contains(secret), "{point:?}: {text}");
    }
}

/// Before the pending marker is durable nothing is committed: the old
/// credential still opens the unchanged envelope and the new one does not.
fn assert_nothing_committed(vault: &Vault, before: &str, new: &SecretString, point: FaultPoint) {
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        before,
        "{point:?}"
    );
    assert_eq!(vault.list_fields(&passphrase()).unwrap().len(), 1);
    let error = vault.list_fields(new).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication, "{point:?}");
    assert_eq!(committed_generation(vault), 2, "{point:?}");
}

/// After the pending marker, recovery needs the new credential and keeps
/// the exact recorded successor until it is installed. After promotion the
/// change is committed and only its journal cleanup was interrupted.
fn assert_only_the_new_credential_finishes(vault: &Vault, new: &SecretString, point: FaultPoint) {
    if point == FaultPoint::AfterPromotion {
        let error = vault.list_fields(&passphrase()).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::Authentication);
        assert_eq!(vault.list_fields(new).unwrap().len(), 1);
        return;
    }
    let candidate = journal_candidate(vault).expect("pending journal");
    assert_secret_free(&candidate, point);
    let error = vault.list_fields(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication, "{point:?}");
    assert!(error.to_string().contains("new passphrase"), "{error}");
    assert_eq!(journal_candidate(vault), Some(candidate.clone()));
    assert_eq!(vault.list_fields(new).unwrap().len(), 1, "{point:?}");
    assert_eq!(
        vault.store.read_vault_text().unwrap().unwrap(),
        candidate,
        "{point:?}"
    );
}

#[test]
fn an_interrupted_rotation_keeps_the_old_credential_until_pending_then_needs_the_new_one() {
    let new = credential("replacement passphrase after a fault");
    for point in [
        FaultPoint::BeforeJournal,
        FaultPoint::AfterJournal,
        FaultPoint::AfterPending,
        FaultPoint::PartialAudit,
        FaultPoint::AfterAudit,
        FaultPoint::AfterEnvelope,
        FaultPoint::AfterPromotion,
    ] {
        let (_temp, vault) = new_vault();
        let before = vault.store.read_vault_text().unwrap().unwrap();
        let old_dek = extract_dek(&envelope(&vault), &passphrase());
        crate::store::arm_fault_for_test(point);

        let error = vault.change_passphrase(&passphrase(), &new).unwrap_err();
        assert_secret_free(&format!("{error} {error:?}"), point);
        if matches!(point, FaultPoint::BeforeJournal | FaultPoint::AfterJournal) {
            assert_nothing_committed(&vault, &before, &new, point);
            continue;
        }
        assert_only_the_new_credential_finishes(&vault, &new, point);
        assert_eq!(committed_generation(&vault), 3, "{point:?}");
        assert!(!dek_opens_state(&envelope(&vault), &old_dek), "{point:?}");
        vault.verify_audit(&new).unwrap();
    }
}
