use super::*;

#[test]
fn worktree_fingerprint_changes_when_untracked_file_content_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    std::fs::write(temp.path().join("new.txt"), "one").unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();
    std::fs::write(temp.path().join("new.txt"), "two").unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(first, second);
}

#[cfg(unix)]
#[test]
fn worktree_fingerprint_frames_untracked_entries_against_nul_collisions() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    let first_path = temp.path().join("a");
    let second_path = temp.path().join("b");
    fs::write(&first_path, b"x").unwrap();
    fs::write(&second_path, b"placeholder").unwrap();
    fs::set_permissions(&first_path, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(&second_path, fs::Permissions::from_mode(0o644)).unwrap();

    let mut old_entry_boundary = b"\0b\0mode\0".to_vec();
    old_entry_boundary.extend_from_slice(&0o644_u32.to_be_bytes());
    old_entry_boundary.extend_from_slice(b"file\0");
    let first_second = [b"y".as_slice(), &old_entry_boundary, b"z"].concat();
    fs::write(&second_path, first_second).unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();

    let second_first = [b"x".as_slice(), &old_entry_boundary, b"y"].concat();
    fs::write(&first_path, second_first).unwrap();
    fs::write(&second_path, b"z").unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(first, second);
}

#[cfg(unix)]
#[test]
fn fingerprints_change_when_an_untracked_file_execution_mode_changes() {
    use std::os::unix::fs::PermissionsExt as _;

    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "tracked\n").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let script = temp.path().join("scripts/check.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    let whole_before = repo_worktree_fingerprint(temp.path()).unwrap();

    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let whole_after = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(whole_before, whole_after);
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

#[test]
fn worktree_fingerprint_changes_when_large_untracked_file_content_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let large_path = temp.path().join("large.bin");
    let fixed_mtime = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    std::fs::write(
        &large_path,
        vec![b'a'; MAX_INLINE_UNTRACKED_BYTES as usize + 1],
    )
    .unwrap();
    std::fs::File::open(&large_path)
        .unwrap()
        .set_modified(fixed_mtime)
        .unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();

    std::fs::write(
        &large_path,
        vec![b'b'; MAX_INLINE_UNTRACKED_BYTES as usize + 1],
    )
    .unwrap();
    std::fs::File::open(&large_path)
        .unwrap()
        .set_modified(fixed_mtime)
        .unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(first, second);
}

#[cfg(unix)]
#[test]
fn worktree_fingerprint_changes_when_untracked_symlink_target_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let first_target = temp.path().join("outside-one");
    let second_target = temp.path().join("outside-two");
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&first_target, &link).unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&second_target, &link).unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(first, second);
}
