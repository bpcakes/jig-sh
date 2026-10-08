use super::*;
use jig_vault::{VaultErrorKind, VaultRecovery};

#[test]
fn non_directory_pending_targets_keep_guidance_and_recover_after_collision_removal() {
    for symlink in [false, true] {
        let temp = private_tempdir();
        let (_, _, archive) = source_with_backup(temp.path(), "source");
        let target = temp.path().join("restored");
        pending_restore(&archive, &target, TransactionFaultPoint::AfterPending);
        let referent = temp.path().join("referent");
        std::fs::create_dir(&referent).unwrap();
        std::fs::write(referent.join("sentinel"), b"untouched").unwrap();
        if symlink {
            std::os::unix::fs::symlink(&referent, &target).unwrap();
        } else {
            std::fs::write(&target, b"occupied").unwrap();
        }

        let (status, operations) =
            jig_vault::test_support::record_fs_ops(|| Vault::status(Some(target.clone())));
        let error = status.unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::Io);
        assert_eq!(error.recovery(), Some(VaultRecovery::StorageConflict));
        assert!(
            operations.is_empty(),
            "status mutated storage: {operations:?}"
        );
        for args in [
            vec!["status"],
            vec!["field", "list"],
            vec!["read", "jig://Example/TOKEN", "--reveal"],
            vec!["passphrase", "change"],
            vec!["backup", "restore", "--in", archive.to_str().unwrap()],
        ] {
            let output = if args[0] == "read" {
                Command::new(env!("CARGO_BIN_EXE_jig"))
                    .arg("vault")
                    .args(&args)
                    .arg("--home")
                    .arg(&target)
                    .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
                    .env_remove("JIG_VAULT_WITNESS_ROOT")
                    .output()
                    .unwrap()
            } else {
                jig(&args, &target)
            };
            let error = failure(&output);
            assert!(error.contains("Operator step"), "{args:?}: {error}");
            assert!(
                error.contains("without deleting the vault rollback witness or its journals"),
                "{args:?}: {error}"
            );
            assert!(!error.contains(PASSPHRASE));
            assert!(!error.contains("recovery value"));
        }
        assert_eq!(
            std::fs::read(referent.join("sentinel")).unwrap(),
            b"untouched"
        );
        assert_eq!(std::fs::read_dir(&referent).unwrap().count(), 1);
        if symlink {
            assert_eq!(std::fs::read_link(&target).unwrap(), referent);
        } else {
            assert_eq!(std::fs::read(&target).unwrap(), b"occupied");
        }
        std::fs::remove_file(&target).unwrap();
        assert!(
            Vault::status(Some(target.clone()))
                .unwrap()
                .pending_transaction
        );
        let listed = json(&jig(&["field", "list"], &target));
        assert_eq!(listed["fields"].as_array().unwrap().len(), 1);
        assert_eq!(
            json(&jig(&["status"], &target))["pending_transaction"],
            false
        );
    }
}
