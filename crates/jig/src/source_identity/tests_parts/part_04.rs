
#[test]
fn changed_path_discovery_fails_closed_at_the_entry_ceiling() {
    let mut destination = vec!["one".into()];
    let mut discovered_entries = 1;

    let error = extend_discovered_paths_with_limit(
        &mut destination,
        vec!["two".into(), "three".into()],
        &mut discovered_entries,
        "test paths",
        2,
    )
    .unwrap_err();

    assert!(error.to_string().contains("limit of 2 path entries"));
    assert_eq!(destination, ["one"]);
    assert_eq!(discovered_entries, 1);
    assert!(
        parse_nul_utf8_paths_with_limit(b"one\0two\0", "test", 1, "test paths")
            .unwrap_err()
            .to_string()
            .contains("remaining limit of 1 path entries")
    );
    assert!(
        parse_name_status_z(b"M\0one\0M\0two\0", 1, "test diff")
            .unwrap_err()
            .to_string()
            .contains("remaining limit of 1 path entries")
    );
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

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn dirty_submodules_fail_closed_even_when_ambient_config_ignores_them() {
    let _env = crate::test_env::lock_env();
    let dependency = tempdir().unwrap();
    run_git(dependency.path(), &["init"]);
    run_git(
        dependency.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(dependency.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(dependency.path().join("source.txt"), "one\n").unwrap();
    run_git(dependency.path(), &["add", "."]);
    run_git(dependency.path(), &["commit", "-m", "dependency"]);

    let parent = tempdir().unwrap();
    run_git(parent.path(), &["init"]);
    run_git(
        parent.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(parent.path(), &["config", "user.name", "Fixture"]);
    run_git(
        parent.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            dependency.path().to_str().unwrap(),
            "vendor/dependency",
        ],
    );
    run_git(parent.path(), &["add", "."]);
    run_git(parent.path(), &["commit", "-m", "parent"]);
    run_git(parent.path(), &["config", "diff.ignoreSubmodules", "all"]);
    let untracked = parent.path().join("vendor/dependency/generated");
    std::fs::create_dir_all(&untracked).unwrap();
    for index in 0..2_000 {
        std::fs::write(untracked.join(format!("entry-{index:04}.txt")), "dirty\n").unwrap();
    }

    let whole = repo_worktree_fingerprint(parent.path())
        .unwrap_err()
        .to_string();

    assert!(
        whole.contains("gitlink") || whole.contains("submodule"),
        "{whole}"
    );
}

#[test]
fn untracked_embedded_repository_cannot_be_fingerprinted_as_a_directory() {
    let _env = crate::test_env::lock_env();
    let parent = tempdir().unwrap();
    run_git(parent.path(), &["init"]);
    run_git(
        parent.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(parent.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(parent.path().join("tracked.txt"), "tracked\n").unwrap();
    run_git(parent.path(), &["add", "."]);
    run_git(parent.path(), &["commit", "-m", "parent"]);
    let embedded = parent.path().join("vendor/embedded");
    std::fs::create_dir_all(&embedded).unwrap();
    run_git(&embedded, &["init"]);
    std::fs::write(embedded.join("source.txt"), "nested\n").unwrap();

    let whole = repo_worktree_fingerprint(parent.path())
        .unwrap_err()
        .to_string();

    assert!(whole.contains("untracked directory"), "{whole}");
}
