
#[cfg(unix)]
#[test]
fn literal_os_path_chunking_uses_raw_encoded_byte_lengths() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let paths = [
        PathBuf::from(OsString::from_vec(vec![0xff; 40 * 1024])),
        PathBuf::from(OsString::from_vec(vec![0xfe; 40 * 1024])),
    ];

    let chunks = literal_os_path_chunks(&paths);

    assert_eq!(chunks, [&paths[..1], &paths[1..]]);
    assert_eq!(
        chunks.into_iter().flatten().cloned().collect::<Vec<_>>(),
        paths
    );
}

#[cfg(unix)]
#[test]
fn tracked_execution_mode_changes_are_fingerprinted_when_core_file_mode_is_disabled() {
    use std::os::unix::fs::PermissionsExt as _;

    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let script = temp.path().join("scripts/check.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let whole_before = repo_worktree_fingerprint(temp.path()).unwrap();
    run_git(temp.path(), &["config", "core.fileMode", "false"]);

    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let whole_after = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(whole_before, whole_after);
}
