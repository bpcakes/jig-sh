//! An orphan journal authorizes unlinking only itself, and recovery never
//! deletes staging: a journal's description of a directory (its name,
//! inode, or plausible contents) never confers ownership.

use std::os::unix::fs::MetadataExt;

use super::*;

/// Rewrites a journal's claims while keeping it parseable.
fn rewrite_journal(path: &Path, edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    edit(&mut value);
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(path, &bytes).unwrap();
    bytes
}

fn identity(path: &Path) -> (u64, u64) {
    let metadata = fs::symlink_metadata(path).unwrap();
    (metadata.dev(), metadata.ino())
}

/// Points a restore journal's staging reference at `victim`, by name and
/// by identity.
fn point_staging_at(journal: &Path, victim: &Path) -> Vec<u8> {
    let (device, inode) = identity(victim);
    let leaf = victim.file_name().unwrap().to_str().unwrap().to_owned();
    rewrite_journal(journal, |value| {
        let restore = &mut value["payload"]["restore"];
        restore["staging_leaf"] = leaf.into();
        restore["staging_device"] = device.into();
        restore["staging_inode"] = inode.into();
    })
}

/// A restore installed and committed at `target` whose journal a crash
/// left behind: no marker references it.
fn leftover_journal(archive: &Path, target: &Path) -> PathBuf {
    crate::store::arm_fault_for_test(FaultPoint::AfterPromotion);
    restore(archive, target).unwrap_err();
    assert!(target.join("vault.json").exists());
    let [journal] = journal_paths(target).try_into().unwrap();
    journal
}

#[test]
fn an_orphan_journal_naming_any_directory_never_changes_it() {
    let temp = private_temp();
    let (home, _vault) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let wrong = SecretString::from("an unrelated wrong passphrase".to_owned());
    // A directory shaped like generated staging, holding vault files.
    let lookalike = temp.path().join(format!(
        ".jig-vault-restore-{}-{}.tmp",
        "0".repeat(16),
        ulid::Ulid::new()
    ));
    fs::create_dir(&lookalike).unwrap();
    fs::set_permissions(&lookalike, fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["vault.json", "audit.jsonl"] {
        fs::copy(home.join(name), lookalike.join(name)).unwrap();
    }

    for (index, victim) in [None, Some(home), Some(lookalike)].into_iter().enumerate() {
        // Each orphan is reached once through a restore retry and once
        // through an open whose authentication fails.
        for retry in [true, false] {
            let target = temp.path().join(format!("restored-{index}-{retry}"));
            let journal = leftover_journal(&archive, &target);
            let victim = victim.clone().unwrap_or_else(|| target.clone());
            point_staging_at(&journal, &victim);
            let before = staged_bytes(&victim);

            assert!(
                Vault::status(Some(target.clone()))
                    .unwrap()
                    .pending_transaction
            );
            let error = if retry {
                restore(&archive, &target).unwrap_err()
            } else {
                Vault::resolve_for_test(Some(target.clone()))
                    .unwrap()
                    .list_fields(&wrong)
                    .unwrap_err()
            };
            if retry {
                assert_eq!(error.kind(), VaultErrorKind::AlreadyExists, "{error}");
            } else {
                assert_eq!(error.kind(), VaultErrorKind::Authentication, "{error}");
            }
            assert!(journal_paths(&target).is_empty(), "{}", victim.display());
            assert_eq!(staged_bytes(&victim), before, "{}", victim.display());
            assert!(target.join("vault.json").exists());
        }
    }
}

#[test]
fn an_altered_referenced_journal_keeps_its_staging_journal_and_marker() {
    let temp = private_temp();
    let (archive, target) = pending_restore(temp.path());
    let home = temp.path().join("source");
    let [journal] = journal_paths(&target).try_into().unwrap();
    let [staging] = staging_dirs(temp.path()).try_into().unwrap();
    let staging = temp.path().join(staging);
    let staged = staged_bytes(&staging);
    let source_files = staged_bytes(&home);
    let altered = point_staging_at(&journal, &home);

    let error = restore(&archive, &target).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(
        error.to_string().contains("does not match its marker"),
        "{error}"
    );
    let error = Vault::resolve_for_test(Some(target.clone()))
        .unwrap()
        .list_fields(&test_passphrase())
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");

    assert_eq!(fs::read(&journal).unwrap(), altered);
    assert_eq!(staged_bytes(&staging), staged);
    assert_eq!(staged_bytes(&home), source_files);
    assert!(
        Vault::status(Some(target.clone()))
            .unwrap()
            .pending_transaction
    );
    let vault_id = serde_json::from_slice::<serde_json::Value>(&altered).unwrap()["vault_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let record = WitnessLocation::for_home(&target)
        .unwrap()
        .open_existing()
        .unwrap()
        .unwrap()
        .read_record(&vault_id)
        .unwrap()
        .unwrap();
    assert!(record.pending.is_some());
    assert!(!target.exists());
}

#[test]
fn recovery_installs_the_recorded_staging_itself_by_rename() {
    let temp = private_temp();
    let (_archive, target) = pending_restore(temp.path());
    let [staging] = staging_dirs(temp.path()).try_into().unwrap();
    let staging = temp.path().join(staging);
    let recorded = identity(&staging);

    Vault::resolve_for_test(Some(target.clone()))
        .unwrap()
        .list_fields(&test_passphrase())
        .unwrap();
    assert_eq!(identity(&target), recorded);
    assert!(!staging.exists());
}
