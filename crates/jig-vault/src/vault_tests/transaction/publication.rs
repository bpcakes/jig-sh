//! Publication can succeed even when the following directory sync fails.

use super::*;
use crate::store::durable::recording::{Publication, fail_sync_after_publication};

#[test]
fn init_and_edit_marker_sync_failures_report_uncertainty_and_recover() {
    for initializing in [true, false] {
        let (_temp, store) = new_store();
        if !initializing {
            store.init(&passphrase()).unwrap();
        }
        let ids = store.witness().open_or_create().unwrap().root().join("ids");
        fail_sync_after_publication(&ids, Publication::Rename, 0);
        let error = if initializing {
            store.init(&passphrase()).unwrap_err()
        } else {
            set_value(&store, "jig://Example/KEPT", b"kept value").unwrap_err()
        };
        assert_eq!(error.kind(), VaultErrorKind::Io);
        assert!(error.to_string().contains("may have been recorded"));
        assert!(!error.to_string().contains("before changing the vault"));
        if initializing {
            assert!(!store.exists().unwrap());
            store.init(&passphrase()).unwrap();
        } else {
            assert_eq!(field_names(&store), vec!["jig://Example/KEPT"]);
        }
        assert_eq!(
            committed_generation(&store),
            if initializing { 1 } else { 2 }
        );
        assert!(journal_candidate(&store).is_none());
    }
}

#[test]
fn passphrase_marker_sync_failure_preserves_new_credential_guidance() {
    let (_temp, store) = new_store();
    let old = passphrase();
    let new = SecretString::from("replacement passphrase after publication".to_owned());
    store.init(&old).unwrap();
    let before = store.read_vault_text().unwrap().unwrap();
    let ids = witness(&store).root().join("ids");
    fail_sync_after_publication(&ids, Publication::Rename, 0);

    let error = store.change_passphrase(&old, &new).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("may have been recorded"));
    assert!(diagnostic.contains("keep both passphrases"));
    assert!(diagnostic.contains("same current and new passphrases"));
    assert!(!diagnostic.contains(old.expose_secret()));
    assert!(!diagnostic.contains(new.expose_secret()));
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before);
    assert_eq!(
        pending_kind(&store, &vault_id(&store)),
        Some(TransactionKind::PassphraseChange)
    );
    assert_eq!(
        store.list(&old).unwrap_err().kind(),
        VaultErrorKind::Authentication
    );

    // Following the diagnostic completes exactly the recorded rotation.
    store.change_passphrase(&old, &new).unwrap();
    store.list(&new).unwrap();
    assert!(store.list(&old).is_err());
    assert_eq!(committed_generation(&store), 2);
    assert_eq!(
        audit_events(&store)
            .iter()
            .filter(|event| event.action == "passphrase_change")
            .count(),
        1
    );
}
