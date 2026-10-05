//! macOS ACL fixtures and an `ls -le` oracle independent of the ACL module.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// An inheritable allow entry that grants other users read access only.
pub(crate) const INHERITED_READ_ACL: &str =
    "everyone allow read,readattr,readextattr,readsecurity,file_inherit,directory_inherit";

/// One fixture name and allow entry per permission that lets another user
/// disturb a directory.
pub(crate) const UNSAFE_DIRECTORY_ACLS: [(&str, &str); 6] = [
    ("allow-add-file", "everyone allow add_file"),
    ("allow-add-subdirectory", "everyone allow add_subdirectory"),
    ("allow-delete", "everyone allow delete"),
    ("allow-delete-child", "everyone allow delete_child"),
    ("allow-writesecurity", "everyone allow writesecurity"),
    ("allow-chown", "everyone allow chown"),
];

/// The message fragment every unsafe directory refusal carries.
pub(crate) const UNSAFE_DIRECTORY_REFUSAL: &str =
    "lets other users write to, delete, or re-permission it";

pub(crate) fn add_acl_entry(path: &Path, entry: &str) {
    let status = std::process::Command::new("/bin/chmod")
        .arg("+a")
        .arg(entry)
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success(), "chmod +a {entry:?} failed");
}

/// Uses `ls -le` as an oracle independent of the ACL module.
pub(crate) fn has_acl_entries(path: &Path) -> bool {
    let output = std::process::Command::new("/bin/ls")
        .arg("-led")
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().lines().count() > 1
}

/// Creates an owner-only directory carrying `entry`.
pub(crate) fn acl_fixture_directory(root: &Path, name: &str, entry: &str) -> PathBuf {
    let directory = root.join(name);
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    add_acl_entry(&directory, entry);
    directory
}

/// Creates an owner-only directory whose inheritable read entry demonstrably
/// reaches new children.
pub(crate) fn inheriting_read_directory(root: &Path) -> PathBuf {
    let directory = acl_fixture_directory(root, "shared-read", INHERITED_READ_ACL);
    let control = directory.join("control");
    fs::create_dir(&control).unwrap();
    assert!(has_acl_entries(&control), "fixture ACL was not inherited");
    fs::remove_dir(&control).unwrap();
    directory
}

/// A temporary root below the sticky shared `/private/tmp`, where an allowed
/// `delete` still lets another user rename a child directory away.
pub(crate) fn sticky_shared_tempdir(prefix: &str) -> tempfile::TempDir {
    let temp = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in("/private/tmp")
        .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}
