use std::collections::BTreeSet;

use tempfile::{TempDir, tempdir};

use super::*;
use crate::test_env::lock_env;

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository() -> TempDir {
    let root = tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    root
}

fn staged(files: &[(&str, &[u8])], removals: &[&str]) -> StagedRender {
    let root = tempdir().unwrap();
    let destination = root.path().join("render");
    fs::create_dir_all(&destination).unwrap();
    let mut active_paths = BTreeSet::new();
    for (relative, bytes) in files {
        let path = destination.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        active_paths.insert(PathBuf::from(relative));
    }
    StagedRender {
        _root: root,
        destination,
        active_paths,
        retirement_paths: removals.iter().map(PathBuf::from).collect(),
    }
}

#[test]
fn uncommitted_recovery_rolls_back_and_committed_recovery_only_cleans_up() {
    let _guard = lock_env();
    let root = repository();
    fs::write(root.path().join("owned.txt"), b"before").unwrap();
    let first_stage = staged(&[("owned.txt", b"after"), ("new.txt", b"new")], &[]);
    let lock = RepositoryUpdateLock::acquire(root.path()).unwrap();
    let mut transaction =
        RepositoryUpdateTransaction::prepare(&lock, root.path(), &first_stage, false, None)
            .unwrap();
    transaction.apply_path(Path::new("owned.txt")).unwrap();
    transaction.apply_path(Path::new("new.txt")).unwrap();
    drop(transaction);

    lock.recover().unwrap();
    assert_eq!(fs::read(root.path().join("owned.txt")).unwrap(), b"before");
    assert!(!root.path().join("new.txt").exists());

    let committed_stage = staged(&[("owned.txt", b"committed")], &[]);
    let mut transaction =
        RepositoryUpdateTransaction::prepare(&lock, root.path(), &committed_stage, false, None)
            .unwrap();
    transaction.apply_path(Path::new("owned.txt")).unwrap();
    write_state(&lock.journal_root, "Committed").unwrap();
    drop(transaction);
    lock.recover().unwrap();
    assert_eq!(
        fs::read(root.path().join("owned.txt")).unwrap(),
        b"committed"
    );
}

#[test]
fn recovery_preserves_foreign_bytes_and_the_preimage_bundle() {
    let _guard = lock_env();
    let root = repository();
    fs::write(root.path().join("owned.txt"), b"before").unwrap();
    let staged = staged(&[("owned.txt", b"after")], &[]);
    let lock = RepositoryUpdateLock::acquire(root.path()).unwrap();
    let mut transaction =
        RepositoryUpdateTransaction::prepare(&lock, root.path(), &staged, true, None).unwrap();
    transaction.apply_path(Path::new("owned.txt")).unwrap();
    fs::write(root.path().join("owned.txt"), b"foreign").unwrap();
    drop(transaction);

    let error = lock.recover().unwrap_err().to_string();
    assert!(error.contains("Foreign writes"), "{error}");
    assert_eq!(fs::read(root.path().join("owned.txt")).unwrap(), b"foreign");
    assert!(lock.journal_root.join("preimages/0000").is_file());
}

#[test]
fn uncommitted_phase_two_recovery_restores_bash_without_reusing_stale_proof() {
    let _guard = lock_env();
    let root = repository();
    let checker = root.path().join("scripts/check-rust-file-loc.sh");
    fs::create_dir_all(checker.parent().unwrap()).unwrap();
    fs::write(&checker, b"#!/usr/bin/env bash\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&checker, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), b"pub fn before() {}\n").unwrap();
    let staged = staged(&[], &["scripts/check-rust-file-loc.sh"]);
    let identity = |byte: char| format!("sha256:{}", byte.to_string().repeat(64));
    let proof = super::super::file_budget_lifecycle::LifecycleProof {
        receipt_id: "receipt_fixture".into(),
        config_digest: identity('a'),
        input_digest: identity('b'),
        source_fingerprint: identity('c'),
        policy_raw_digest: identity('d'),
        comparison: serde_json::json!({"kind": "strict_inventory"}),
        evaluation_digest: identity('e'),
        valid_until_ms: Some(1),
    };
    let lock = RepositoryUpdateLock::acquire(root.path()).unwrap();
    let mut transaction =
        RepositoryUpdateTransaction::prepare(&lock, root.path(), &staged, false, Some(proof))
            .unwrap();
    // The lifecycle module's end-to-end test covers proof revalidation. This
    // test starts immediately after that boundary while leaving the serialized
    // proof in the durable manifest consumed by recovery.
    transaction.manifest.lifecycle_proof = None;
    transaction
        .apply_path(Path::new("scripts/check-rust-file-loc.sh"))
        .unwrap();
    assert!(!checker.exists());
    drop(transaction);

    // The governed source and proof validity can change while the process is
    // down. Recovery must roll back the uncommitted deletion; it never resumes
    // phase two or attempts to reuse the serialized proof.
    fs::write(root.path().join("src/lib.rs"), b"pub fn changed() {}\n").unwrap();
    lock.recover().unwrap();

    assert_eq!(
        fs::read(&checker).unwrap(),
        b"#!/usr/bin/env bash\nexit 0\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(&checker).unwrap().permissions().mode() & 0o100,
            0
        );
    }
    assert_eq!(
        fs::read(root.path().join("src/lib.rs")).unwrap(),
        b"pub fn changed() {}\n"
    );
    assert!(!lock.journal_root.exists());
}

#[test]
fn mutation_requires_a_usable_git_worktree_before_journaling() {
    let _guard = lock_env();
    let root = tempdir().unwrap();
    let error = RepositoryUpdateLock::acquire(root.path())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("usable Git worktree"), "{error}");
    assert!(!root.path().join(".git").exists());
}

#[test]
fn injected_uncommitted_failures_restore_every_transaction_owned_path() {
    let _guard = lock_env();
    for point in [
        "after_prepare",
        "before_operation_0",
        "after_operation_0",
        "after_progress_0",
        "before_operation_1",
        "after_operation_1",
        "after_progress_1",
        "before_committed",
    ] {
        let root = repository();
        fs::write(root.path().join("a.txt"), b"before-a").unwrap();
        fs::write(root.path().join("b.txt"), b"before-b").unwrap();
        let staged = staged(&[("a.txt", b"after-a"), ("b.txt", b"after-b")], &[]);
        let lock = RepositoryUpdateLock::acquire(root.path()).unwrap();
        let error = fault_injection::with_failure(point, || {
            let prepared =
                RepositoryUpdateTransaction::prepare(&lock, root.path(), &staged, false, None);
            match prepared {
                Err(error) => error,
                Ok(mut transaction) => {
                    let mut failure = None;
                    for relative in [Path::new("a.txt"), Path::new("b.txt")] {
                        if let Err(error) = transaction.apply_path(relative) {
                            failure = Some(error);
                            break;
                        }
                    }
                    match failure {
                        Some(error) => transaction.finish_failed(error),
                        None => transaction.commit().unwrap_err(),
                    }
                }
            }
        });
        assert!(error.to_string().contains(point), "{point}: {error:#}");
        assert_eq!(fs::read(root.path().join("a.txt")).unwrap(), b"before-a");
        assert_eq!(fs::read(root.path().join("b.txt")).unwrap(), b"before-b");
        assert!(!lock.journal_root.exists(), "{point}");
    }
}

#[test]
fn failure_after_committed_preserves_publication_and_next_recovery_only_cleans_up() {
    let _guard = lock_env();
    let root = repository();
    fs::write(root.path().join("owned.txt"), b"before").unwrap();
    let staged = staged(&[("owned.txt", b"after")], &[]);
    let lock = RepositoryUpdateLock::acquire(root.path()).unwrap();
    let mut transaction =
        RepositoryUpdateTransaction::prepare(&lock, root.path(), &staged, false, None).unwrap();
    transaction.apply_path(Path::new("owned.txt")).unwrap();
    let error = fault_injection::with_failure("after_committed", || {
        transaction.commit().unwrap_err().to_string()
    });
    assert!(error.contains("after_committed"), "{error}");
    assert_eq!(fs::read(root.path().join("owned.txt")).unwrap(), b"after");
    assert!(lock.journal_root.exists());
    lock.recover().unwrap();
    assert_eq!(fs::read(root.path().join("owned.txt")).unwrap(), b"after");
    assert!(!lock.journal_root.exists());
}

#[test]
fn injected_failures_are_scoped_to_the_current_thread() {
    fault_injection::with_failure("after_prepare", || {
        assert!(maybe_fail("after_prepare").is_err());
        std::thread::spawn(|| assert!(maybe_fail("after_prepare").is_ok()))
            .join()
            .unwrap();
    });
    assert!(maybe_fail("after_prepare").is_ok());
}

#[test]
fn authored_seed_is_journaled_and_managed_manifest_is_always_last() {
    let _guard = lock_env();
    let root = repository();
    let mut staged = staged(
        &[
            ("managed.txt", b"managed"),
            (super::super::managed_paths::MANIFEST_PATH, b"manifest"),
            (
                super::super::staged_render::FILE_BUDGET_POLICY_PATH,
                b"policy",
            ),
        ],
        &[],
    );
    staged.active_paths.remove(Path::new(
        super::super::staged_render::FILE_BUDGET_POLICY_PATH,
    ));
    let lock = RepositoryUpdateLock::acquire(root.path()).unwrap();
    let transaction =
        RepositoryUpdateTransaction::prepare(&lock, root.path(), &staged, false, None).unwrap();
    let paths = transaction
        .manifest
        .operations
        .iter()
        .map(|operation| operation.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths.first().copied(),
        Some(super::super::staged_render::FILE_BUDGET_POLICY_PATH)
    );
    assert_eq!(
        paths.last().copied(),
        Some(super::super::managed_paths::MANIFEST_PATH)
    );
    let _ = transaction.finish_failed(anyhow::anyhow!("test rollback"));
    assert!(
        !root
            .path()
            .join(super::super::staged_render::FILE_BUDGET_POLICY_PATH)
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn journal_directories_and_payloads_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let root = repository();
    fs::write(root.path().join("owned.txt"), b"before").unwrap();
    let staged = staged(&[("owned.txt", b"after")], &[]);
    let lock = RepositoryUpdateLock::acquire(root.path()).unwrap();
    let transaction =
        RepositoryUpdateTransaction::prepare(&lock, root.path(), &staged, false, None).unwrap();
    assert_eq!(
        fs::metadata(&lock.journal_root)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for payload in ["preimages/0000", "staged/0000"] {
        assert_eq!(
            fs::metadata(lock.journal_root.join(payload))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let _ = transaction.finish_failed(anyhow::anyhow!("test rollback"));
}
