use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::super::*;

#[test]
fn resolve_creates_private_directory() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    assert!(store.root().is_dir());
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(store.root()).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[cfg(target_os = "macos")]
#[test]
fn resolve_uses_the_verified_physical_macos_temp_path() {
    let temp = tempfile::Builder::new()
        .prefix("jig-vault-path-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let logical_root = temp.path().join("vault");

    let store = VaultStore::resolve_for_test(Some(logical_root.clone())).unwrap();

    assert!(store.root().starts_with("/private/tmp"));
    assert_eq!(store.root(), fs::canonicalize(logical_root).unwrap());
}
