use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::*;

#[cfg(target_os = "macos")]
#[test]
fn output_uses_the_verified_physical_macos_temp_path() {
    let temp = tempfile::Builder::new()
        .prefix("jig-vault-output-path-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let logical_output = temp.path().join("result.bin");
    let physical_output = fs::canonicalize(temp.path()).unwrap().join("result.bin");

    let precondition = preview(&logical_output).unwrap();
    assert!(precondition.destination.starts_with("/private/tmp"));
    assert_eq!(precondition.destination, physical_output);

    prepare_private_bytes(&logical_output, b"protected", false)
        .unwrap()
        .install()
        .unwrap();
    assert_eq!(fs::read(physical_output).unwrap(), b"protected");
}
