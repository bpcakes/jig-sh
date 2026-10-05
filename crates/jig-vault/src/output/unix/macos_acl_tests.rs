use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::acl::test_support::{
    UNSAFE_DIRECTORY_ACLS, UNSAFE_DIRECTORY_REFUSAL, acl_fixture_directory, add_acl_entry,
    has_acl_entries, inheriting_read_directory,
};
use crate::{PreparedPrivateFile, SecretBytes, VaultErrorKind};

fn private_root() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    (temp, root)
}

#[test]
fn output_clears_inherited_acls_before_writing_for_every_install_policy() {
    let (_temp, root) = private_root();
    let shared = inheriting_read_directory(&root);

    let created = shared.join("created.bin");
    let prepared = prepare_private_bytes(&created, b"created", false).unwrap();
    let temporary = prepared.path.as_ref().unwrap().temporary.clone();
    assert!(!has_acl_entries(&temporary));
    prepared.install().unwrap();

    let replaced = shared.join("replaced.bin");
    fs::write(&replaced, b"existing").unwrap();
    assert!(has_acl_entries(&replaced), "fixture ACL was not inherited");
    prepare_private_bytes(&replaced, b"replaced", true)
        .unwrap()
        .install()
        .unwrap();

    let previewed = shared.join("previewed.env");
    let precondition = PreparedPrivateFile::preview(&previewed).unwrap();
    PreparedPrivateFile::prepare_if_unchanged(
        precondition,
        SecretBytes::new(b"previewed".to_vec()),
        false,
    )
    .unwrap()
    .install()
    .unwrap();

    for (path, contents) in [
        (&created, &b"created"[..]),
        (&replaced, b"replaced"),
        (&previewed, b"previewed"),
    ] {
        assert_eq!(fs::read(path).unwrap(), contents);
        assert!(!has_acl_entries(path), "{}", path.display());
    }
}

#[test]
fn output_refuses_parents_whose_acl_allows_write_or_delete() {
    let (_temp, root) = private_root();

    for (name, entry) in UNSAFE_DIRECTORY_ACLS {
        let parent = acl_fixture_directory(&root, name, entry);
        let output = parent.join("result.bin");
        let error = preflight_private_destination(&output, false).unwrap_err();
        assert_eq!(error.kind, VaultErrorKind::InvalidInput, "{entry}");
        assert!(
            error.error.to_string().contains(UNSAFE_DIRECTORY_REFUSAL),
            "{entry}: {:#}",
            error.error
        );
        let Err(error) = prepare_private_bytes(&output, b"secret", false) else {
            panic!("{entry}: preparation accepted an unsafe parent");
        };
        assert!(
            error.error.to_string().contains(UNSAFE_DIRECTORY_REFUSAL),
            "{entry}: {:#}",
            error.error
        );
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0, "{entry}");
    }

    let narrowed = acl_fixture_directory(&root, "deny-delete", "everyone deny delete");
    let output = narrowed.join("result.bin");
    prepare_private_bytes(&output, b"narrowed", false)
        .unwrap()
        .install()
        .unwrap();
    assert_eq!(fs::read(&output).unwrap(), b"narrowed");
}

#[test]
fn install_refuses_a_parent_that_gained_an_unsafe_acl_after_preparation() {
    let (_temp, root) = private_root();
    let parent = root.join("later");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    let output = parent.join("result.bin");
    let prepared = prepare_private_bytes(&output, b"protected", false).unwrap();
    add_acl_entry(&parent, "everyone allow delete");

    let error = prepared.install().unwrap_err();

    assert!(
        error.error.to_string().contains(UNSAFE_DIRECTORY_REFUSAL),
        "{:#}",
        error.error
    );
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
}

#[test]
fn install_refuses_a_temporary_that_gained_an_acl_before_its_identity_was_recorded() {
    let (_temp, root) = private_root();
    let output = root.join("result.bin");
    let mut prepared = prepare_private_bytes(&output, b"protected", false).unwrap();
    let path = prepared.path.as_mut().unwrap();
    add_acl_entry(&path.temporary, "everyone allow read");
    // Model an entry added after clearing but before the identity was taken.
    path.temporary_identity = Some(file_identity(
        &fs::symlink_metadata(&path.temporary).unwrap(),
    ));

    let error = prepared.install().unwrap_err();

    assert!(
        error.error.to_string().contains("access control list"),
        "{:#}",
        error.error
    );
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
}
