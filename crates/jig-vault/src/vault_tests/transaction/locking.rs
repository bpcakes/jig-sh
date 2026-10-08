//! Recovery must retain same-ID serialization through the caller's audit work.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use fs4::fs_std::FileExt;

use super::*;
use crate::store::witness::id_key;

fn id_lock_path(store: &VaultStore, id: &str) -> PathBuf {
    store
        .witness()
        .root()
        .join("locks")
        .join(format!("id-{}.lock", id_key(id)))
}

fn competing_lock_available(path: &Path) -> bool {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let available = file.try_lock_exclusive().unwrap();
    if available {
        FileExt::unlock(&file).unwrap();
    }
    available
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn recovered_absent_target_serializes_a_competing_restore_until_audit_finishes() {
    let (temp, source) = new_store();
    source.init(&passphrase()).unwrap();
    let archive = temp.path().join("ExampleVault.backup");
    let create =
        Vault::preflight_backup_create(source.root().to_path_buf(), &archive, false).unwrap();
    Vault::create_backup(&passphrase(), create).unwrap();
    let target = temp.path().join("restored");
    let restore = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    crate::store::arm_fault_for_test(FaultPoint::AfterPending);
    Vault::restore_backup(&passphrase(), restore).unwrap_err();
    assert!(!target.exists());
    let recovering = VaultStore::open_existing(target).unwrap();
    let lock = id_lock_path(&source, &vault_id(&source));
    let competing =
        Vault::preflight_backup_restore(&archive, temp.path().join("competing")).unwrap();
    let (start_tx, start_rx) = mpsc::channel();
    let (probe_tx, probe_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        start_rx.recv().unwrap();
        // A real nonblocking lock attempt establishes exclusion without a
        // sleep or a scheduling assumption. Restore uses this same ID lock.
        probe_tx.send(competing_lock_available(&lock)).unwrap();
        Vault::restore_backup(&passphrase(), competing).unwrap()
    });
    recovering
        .edit_with_audit(
            &passphrase(),
            AuditAction::FieldBatchApply,
            |_| {
                start_tx.send(()).unwrap();
                assert!(!probe_rx.recv().unwrap(), "recovery released the ID lock");
                Ok(())
            },
            |_| serde_json::json!({"sets": [], "removes": []}),
        )
        .unwrap();
    let restored = worker.join().unwrap();
    assert_eq!(restored.generation, Some(3));
    let audit = std::fs::read_to_string(recovering.audit_path()).unwrap();
    assert!(audit.lines().last().unwrap().contains("field_batch_apply"));
    // The second restore may fence this home only after its audit completes.
    assert!(recovering.list_fields(&passphrase()).is_err());
}

#[test]
fn recovered_init_without_a_header_keeps_the_id_lock_until_the_operation_finishes() {
    let (_temp, store) = new_store();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    store.init(&passphrase()).unwrap_err();
    assert!(!store.vault_path().exists());
    let lock = store
        .edit_with_audit(
            &passphrase(),
            AuditAction::FieldBatchApply,
            |_| {
                let lock = id_lock_path(&store, &vault_id(&store));
                let probe = lock.clone();
                assert!(
                    !std::thread::spawn(move || competing_lock_available(&probe))
                        .join()
                        .unwrap()
                );
                Ok(lock)
            },
            |_| serde_json::json!({"sets": [], "removes": []}),
        )
        .unwrap();
    assert!(competing_lock_available(&lock));
    assert_eq!(committed_generation(&store), 1);
    assert!(store.list_fields(&passphrase()).unwrap().is_empty());
}
