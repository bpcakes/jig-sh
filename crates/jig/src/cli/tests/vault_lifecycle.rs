use super::*;

#[test]
fn parses_vault_lifecycle_commands() {
    let passphrase =
        Cli::try_parse_from(["jig", "vault", "passphrase", "change", "--global"]).unwrap();
    match passphrase.command {
        CommandKind::Vault(VaultCommand::Passphrase(VaultPassphraseCommand::Change(opts))) => {
            assert!(opts.vault.global);
        }
        other => panic!("expected vault passphrase change command, got {other:?}"),
    }
    assert!(
        Cli::try_parse_from([
            "jig",
            "vault",
            "passphrase",
            "change",
            "--new-passphrase",
            "must-not-be-accepted",
        ])
        .is_err()
    );

    let backup_create = Cli::try_parse_from([
        "jig",
        "vault",
        "backup",
        "create",
        "--out",
        "vault.backup",
        "--overwrite",
    ])
    .unwrap();
    match backup_create.command {
        CommandKind::Vault(VaultCommand::Backup(VaultBackupCommand::Create(opts))) => {
            assert_eq!(opts.out, PathBuf::from("vault.backup"));
            assert!(opts.overwrite);
        }
        other => panic!("expected vault backup create command, got {other:?}"),
    }
    assert!(Cli::try_parse_from(["jig", "vault", "backup", "create"]).is_err());

    let backup_restore = Cli::try_parse_from([
        "jig",
        "vault",
        "backup",
        "restore",
        "--in",
        "vault.backup",
        "--home",
        "restored-vault",
    ])
    .unwrap();
    match backup_restore.command {
        CommandKind::Vault(VaultCommand::Backup(VaultBackupCommand::Restore(opts))) => {
            assert_eq!(opts.input, PathBuf::from("vault.backup"));
            assert_eq!(opts.vault.home, Some(PathBuf::from("restored-vault")));
        }
        other => panic!("expected vault backup restore command, got {other:?}"),
    }
    assert!(Cli::try_parse_from(["jig", "vault", "backup", "restore"]).is_err());
}
