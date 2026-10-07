//! Deterministic interleavings at the legacy restore publication boundary.

use std::cell::RefCell;

use fs4::fs_std::FileExt;

use super::*;
use crate::Vault;
use crate::format::{V2_FORMAT_VERSION, V3_FORMAT_VERSION};
use crate::store::witness::{WitnessLocation, id_key};
use crate::test_fixtures::{
    GENERATED_V2_BACKUP, GENERATED_V2_VAULT_ID, generated_v2_passphrase,
    install_generated_v2_fixture,
};

type Hook = Option<Box<dyn FnOnce()>>;
thread_local! {
    static BEFORE_FINALIZE: RefCell<Hook> = const { RefCell::new(None) };
    static BEFORE_LOCK: RefCell<Hook> = const { RefCell::new(None) };
    static BEFORE_INSTALL: RefCell<Hook> = const { RefCell::new(None) };
}

pub(super) fn before_finalize() {
    let hook = BEFORE_FINALIZE.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

pub(super) fn before_install_lock() {
    let hook = BEFORE_LOCK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

pub(super) fn before_install() {
    let hook = BEFORE_INSTALL.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

struct ClearHooks;

impl Drop for ClearHooks {
    fn drop(&mut self) {
        BEFORE_FINALIZE.with(|slot| slot.borrow_mut().take());
        BEFORE_LOCK.with(|slot| slot.borrow_mut().take());
        BEFORE_INSTALL.with(|slot| slot.borrow_mut().take());
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let archive = temp.path().join("example.backup");
    fs::write(&archive, GENERATED_V2_BACKUP).unwrap();
    let target = temp.path().join("restored");
    (temp, archive, target)
}

#[test]
fn pending_migration_before_legacy_finalization_preserves_recovery_metadata() {
    let (temp, archive, target) = fixture();
    let source_home = temp.path().join("source");
    let store = VaultStore::resolve_for_test(Some(source_home.clone())).unwrap();
    install_generated_v2_fixture(&store);
    let source = Vault::resolve_for_test(Some(source_home.clone())).unwrap();
    let original = fs::read(source_home.join(VAULT_FILE)).unwrap();
    let request = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    let _clear = ClearHooks;
    BEFORE_FINALIZE.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            // Restore selected the unwitnessed legacy path and created its
            // staging. Publish a same-ID migration before staging opens for
            // authenticated finalization, then leave that migration pending.
            std::thread::spawn(move || {
                store.arm_fault_for_test(crate::store::FaultPoint::AfterPending);
                let error = source
                    .migrate(&generated_v2_passphrase(), V3_FORMAT_VERSION)
                    .unwrap_err();
                assert_eq!(error.kind(), VaultErrorKind::Io);
            })
            .join()
            .unwrap();
        }));
    });

    let error = Vault::restore_backup(&generated_v2_passphrase(), request).unwrap_err();
    assert!(BEFORE_FINALIZE.with(|slot| slot.borrow().is_none()));
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(error.recovery(), Some(VaultRecovery::StorageConflict));
    assert!(!target.exists());
    assert_eq!(fs::read(source_home.join(VAULT_FILE)).unwrap(), original);
    assert!(
        Vault::status(Some(source_home.clone()))
            .unwrap()
            .pending_transaction
    );
    assert!(!fs::read_dir(temp.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("jig-vault-restore")
    }));

    // The conflicting restore must retain the source's recovery journal:
    // an authenticated source open can still complete its recorded migration.
    let source = Vault::resolve_for_test(Some(source_home.clone())).unwrap();
    assert_eq!(
        source
            .list_fields(&generated_v2_passphrase())
            .unwrap()
            .len(),
        2
    );
    let status = Vault::status(Some(source_home)).unwrap();
    assert!(!status.pending_transaction);
    assert_eq!(status.format_version, Some(V3_FORMAT_VERSION));
}

#[test]
fn migration_after_legacy_finalization_leaves_target_absent_and_retryable() {
    let (temp, archive, target) = fixture();
    let source_home = temp.path().join("source");
    let store = VaultStore::resolve_for_test(Some(source_home.clone())).unwrap();
    install_generated_v2_fixture(&store);
    let source = Vault::resolve_for_test(Some(source_home)).unwrap();
    let request = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    let _clear = ClearHooks;
    BEFORE_LOCK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            // Staging has authenticated and appended its restore event, but
            // the target is still absent. A same-ID copy now wins migration.
            std::thread::spawn(move || {
                source
                    .migrate(&generated_v2_passphrase(), V3_FORMAT_VERSION)
                    .unwrap();
            })
            .join()
            .unwrap();
        }));
    });

    let error = Vault::restore_backup(&generated_v2_passphrase(), request).unwrap_err();
    assert!(error.to_string().contains("retry"), "{error}");
    assert!(!target.exists());
    assert!(!fs::read_dir(temp.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("jig-vault-restore")
    }));

    let request = Vault::preflight_backup_restore(&archive, target).unwrap();
    let restored = Vault::restore_backup(&generated_v2_passphrase(), request).unwrap();
    assert_eq!(restored.source_format_version, V2_FORMAT_VERSION);
    assert_eq!(restored.format_version, V3_FORMAT_VERSION);
    assert_eq!(restored.generation, Some(2));
    let vault = Vault::resolve_for_test(Some(restored.root)).unwrap();
    assert_eq!(
        vault.list_fields(&generated_v2_passphrase()).unwrap().len(),
        2
    );
}

#[test]
fn legacy_install_keeps_same_id_locked_until_publication() {
    let (_temp, archive, target) = fixture();
    let request = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    let witness = WitnessLocation::for_home(&target).unwrap();
    let lock_path = witness
        .root()
        .join("locks")
        .join(format!("id-{}.lock", id_key(GENERATED_V2_VAULT_ID)));
    let probe_path = lock_path.clone();
    let _clear = ClearHooks;
    BEFORE_INSTALL.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            assert!(!target.exists());
            // A real competing descriptor cannot acquire the ID lock at
            // the final publication boundary. No timing/sleep assumption.
            std::thread::spawn(move || {
                let probe = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(probe_path)
                    .unwrap();
                assert!(!probe.try_lock_exclusive().unwrap());
            })
            .join()
            .unwrap();
        }));
    });

    let restored = Vault::restore_backup(&generated_v2_passphrase(), request).unwrap();
    assert!(BEFORE_INSTALL.with(|slot| slot.borrow().is_none()));
    assert_eq!(restored.format_version, V2_FORMAT_VERSION);
    assert_eq!(restored.generation, None);
    let probe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .unwrap();
    assert!(probe.try_lock_exclusive().unwrap());
    FileExt::unlock(&probe).unwrap();
    let vault = Vault::resolve_for_test(Some(restored.root)).unwrap();
    assert_eq!(
        vault.list_fields(&generated_v2_passphrase()).unwrap().len(),
        2
    );
}
