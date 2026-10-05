use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use secrecy::SecretString;

use super::super::*;
use crate::acl::test_support::{
    UNSAFE_DIRECTORY_ACLS, UNSAFE_DIRECTORY_REFUSAL, acl_fixture_directory, add_acl_entry,
    has_acl_entries, inheriting_read_directory,
};
use crate::{FieldKind, SecretBytes, Vault, VaultReference};

fn passphrase() -> SecretString {
    SecretString::from("acl-test-passphrase".to_owned())
}

fn private_root() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    (temp, root)
}

fn home_and_state_files(root: &Path) -> [PathBuf; 4] {
    [
        root.to_path_buf(),
        root.join(VAULT_FILE),
        root.join(AUDIT_FILE),
        root.join(LOCK_FILE),
    ]
}

#[test]
fn init_clears_inherited_acls_from_the_home_and_every_state_file() {
    let (_temp, root) = private_root();
    let shared = inheriting_read_directory(&root);

    for home in [shared.join("vault"), shared.join("vault-base/scopes/repo")] {
        let vault = Vault::resolve_for_test(Some(home)).unwrap();
        vault.init(&passphrase()).unwrap();

        for path in home_and_state_files(vault.root()) {
            assert!(path.exists(), "{}", path.display());
            assert!(!has_acl_entries(&path), "{}", path.display());
        }
    }
}

#[test]
fn reuse_clears_acls_that_existing_vault_state_gained() {
    let (_temp, root) = private_root();
    let home = root.join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init(&passphrase()).unwrap();
    add_acl_entry(vault.root(), "everyone allow list,search");
    for path in &home_and_state_files(vault.root())[1..] {
        add_acl_entry(path, "everyone allow read");
    }

    let vault = Vault::resolve_for_test(Some(home)).unwrap();
    assert!(!has_acl_entries(vault.root()));
    vault
        .set_field(
            &passphrase(),
            VaultReference::parse("jig://Production/TOKEN").unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(b"acl-sentinel".to_vec()),
        )
        .unwrap();

    for path in home_and_state_files(vault.root()) {
        assert!(!has_acl_entries(&path), "{}", path.display());
    }
}

#[test]
fn resolve_refuses_home_ancestors_whose_acl_allows_write_or_delete() {
    let (_temp, root) = private_root();

    for (name, entry) in UNSAFE_DIRECTORY_ACLS {
        let parent = acl_fixture_directory(&root, name, entry);
        for home in [parent.join("vault"), parent.join("vault-base/scopes/repo")] {
            let error = VaultStore::resolve_for_test(Some(home))
                .unwrap_err()
                .to_string();
            assert!(error.contains(UNSAFE_DIRECTORY_REFUSAL), "{entry}: {error}");
        }
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0, "{entry}");
    }

    let narrowed = acl_fixture_directory(&root, "deny-delete", "everyone deny delete");
    for home in [
        narrowed.join("vault"),
        narrowed.join("vault-base/scopes/repo"),
    ] {
        let store = VaultStore::resolve_for_test(Some(home)).unwrap();
        assert!(!has_acl_entries(store.root()));
    }

    let later = root.join("later");
    fs::create_dir(&later).unwrap();
    fs::set_permissions(&later, fs::Permissions::from_mode(0o700)).unwrap();
    VaultStore::resolve_for_test(Some(later.join("vault"))).unwrap();
    add_acl_entry(&later, "everyone allow delete");

    let error = VaultStore::resolve_for_test(Some(later.join("vault")))
        .unwrap_err()
        .to_string();
    assert!(error.contains(UNSAFE_DIRECTORY_REFUSAL), "{error}");
}
