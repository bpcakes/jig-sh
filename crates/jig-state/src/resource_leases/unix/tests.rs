use std::os::unix::fs::{PermissionsExt, symlink};

use super::*;

#[test]
fn private_namespace_rejects_symlinks_and_permissive_modes() {
    let root = tempfile::tempdir().unwrap();
    let root_name = CString::new(root.path().as_os_str().as_encoded_bytes()).unwrap();
    let directory = open_directory(libc::AT_FDCWD, &root_name).unwrap();
    let uid = unsafe { libc::geteuid() };
    std::fs::create_dir(root.path().join("permissive")).unwrap();
    std::fs::set_permissions(
        root.path().join("permissive"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(private_namespace(&directory, c"permissive", uid).is_err());
    symlink(root.path().join("permissive"), root.path().join("alias")).unwrap();
    assert!(private_namespace(&directory, c"alias", uid).is_err());
    let valid = private_namespace(&directory, c"valid", uid).unwrap();
    assert!(validate_private_directory(&valid, uid.wrapping_add(1)).is_err());
}

#[test]
fn claim_files_reject_aliases_nonregular_files_modes_and_wrong_owner() {
    let root = tempfile::tempdir().unwrap();
    let root_name = CString::new(root.path().as_os_str().as_encoded_bytes()).unwrap();
    let directory = open_directory(libc::AT_FDCWD, &root_name).unwrap();
    let uid = unsafe { libc::geteuid() };
    let file = open_claim(&directory, c"valid", uid).unwrap();
    assert!(valid_claim_metadata(&file.metadata().unwrap(), uid));
    assert!(!valid_claim_metadata(
        &file.metadata().unwrap(),
        uid.wrapping_add(1)
    ));
    symlink(root.path().join("valid"), root.path().join("symlink")).unwrap();
    assert!(open_claim(&directory, c"symlink", uid).is_err());
    std::fs::hard_link(root.path().join("valid"), root.path().join("hardlink")).unwrap();
    assert!(open_claim(&directory, c"hardlink", uid).is_err());
    std::fs::create_dir(root.path().join("directory")).unwrap();
    assert!(open_claim(&directory, c"directory", uid).is_err());
    let fifo = CString::new(root.path().join("fifo").as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(open_claim(&directory, c"fifo", uid).is_err());
    std::fs::write(root.path().join("permissive"), b"").unwrap();
    std::fs::set_permissions(
        root.path().join("permissive"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let error = open_claim(&directory, c"permissive", uid)
        .unwrap_err()
        .to_string();
    assert!(!error.contains(root.path().to_str().unwrap()));
}

#[test]
fn inheritance_configuration_keeps_parent_descriptors_close_on_exec() {
    let root = tempfile::tempdir().unwrap();
    let root_name = CString::new(root.path().as_os_str().as_encoded_bytes()).unwrap();
    let directory = open_directory(libc::AT_FDCWD, &root_name).unwrap();
    let file = Arc::new(open_claim(&directory, c"claim", unsafe { libc::geteuid() }).unwrap());
    let mut command = Command::new("unused-command");
    inherit_into(&[Arc::clone(&file)], &mut command).unwrap();
    assert_ne!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    assert_eq!(
        Arc::strong_count(&file),
        2,
        "command pins the FD until spawn/drop"
    );
}
