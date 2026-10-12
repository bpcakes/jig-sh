use super::*;

#[test]
fn whole_worktree_fingerprint_fails_closed_on_large_binary_diff_output() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("asset.bin"), [0_u8; 32]).unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "baseline"]);
    let mut state = 0x1234_5678_u32;
    let changed = (0..16_384)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect::<Vec<_>>();
    std::fs::write(temp.path().join("asset.bin"), changed).unwrap();

    WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.set(Some(256));
    let result = repo_worktree_fingerprint(temp.path());
    WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.set(None);
    let error = result.unwrap_err();

    assert!(
        format!("{error:#}").contains("worktree proof Git output limit of 256 bytes"),
        "{error:#}"
    );
}

#[test]
fn whole_worktree_fingerprint_fails_closed_on_too_many_status_entries() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    for name in ["one.txt", "two.txt", "three.txt"] {
        std::fs::write(temp.path().join(name), name).unwrap();
    }

    WORKTREE_STATUS_ENTRY_LIMIT_OVERRIDE.set(Some(2));
    let result = repo_worktree_fingerprint(temp.path());
    WORKTREE_STATUS_ENTRY_LIMIT_OVERRIDE.set(None);
    let error = result.unwrap_err();

    assert!(
        format!("{error:#}").contains("worktree proof entry limit of 2"),
        "{error:#}"
    );
}

#[test]
fn worktree_gitlink_probe_scales_with_changed_paths_not_full_index() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    for index in 0..100 {
        fs::write(
            temp.path()
                .join(format!("unrelated-index-entry-{index:03}.txt")),
            "stable\n",
        )
        .unwrap();
    }
    fs::write(temp.path().join("selected.txt"), "one\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "large index fixture"]);
    fs::write(temp.path().join("selected.txt"), "two\n").unwrap();

    WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.set(Some(512));
    let result = repo_worktree_fingerprint(temp.path());
    WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.set(None);

    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn staged_deletion_with_ignored_same_path_replacement_fails_all_evidence_closed() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join(".gitignore"), "ignored-input.txt\n").unwrap();
    std::fs::write(temp.path().join("ignored-input.txt"), "baseline\n").unwrap();
    run_git(
        temp.path(),
        &["add", "-f", ".gitignore", "ignored-input.txt"],
    );
    run_git(temp.path(), &["commit", "-m", "baseline"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    run_git(temp.path(), &["rm", "--cached", "ignored-input.txt"]);

    for replacement in ["first replacement\n", "different replacement\n"] {
        std::fs::write(temp.path().join("ignored-input.txt"), replacement).unwrap();
        let plan =
            plan_change_snapshot_with_cancellation(temp.path(), &baseline, &|| false).unwrap();
        assert!(
            plan.changed_paths
                .iter()
                .any(|path| path == "ignored-input.txt")
        );
        assert!(plan.untracked_paths.is_empty());

        let whole_error = repo_worktree_fingerprint(temp.path())
            .unwrap_err()
            .to_string();
        assert!(
            whole_error.contains("staged deletion ignored-input.txt"),
            "{whole_error}"
        );
    }
}

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
