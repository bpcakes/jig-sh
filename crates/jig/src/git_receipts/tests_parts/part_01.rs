#[test]
fn read_only_git_commands_disable_optional_locks() {
    let mut command = Command::new("git");
    command.env("GIT_OPTIONAL_LOCKS", "1");

    configure_read_only_git_environment(&mut command);

    assert_eq!(
        command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("GIT_OPTIONAL_LOCKS"))
            .and_then(|(_, value)| value),
        Some(OsStr::new("0"))
    );
}

#[test]
fn read_only_git_commands_scrub_repository_and_command_config_redirects() {
    let mut command = Command::new("git");
    command
        .env("GIT_DIR", "elsewhere/.git")
        .env("GIT_WORK_TREE", "elsewhere")
        .env("GIT_INDEX_FILE", "elsewhere/index")
        .env("GIT_REPLACE_REF_BASE", "refs/replacements")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.worktree")
        .env("GIT_CONFIG_VALUE_0", "elsewhere");

    configure_read_only_git_environment(&mut command);

    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_REPLACE_REF_BASE",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
    ] {
        assert_eq!(
            command
                .get_envs()
                .find(|(candidate, _)| *candidate == OsStr::new(name))
                .map(|(_, value)| value),
            Some(None),
            "{name} was not scrubbed"
        );
    }
}

#[test]
fn repository_redirect_environment_cannot_change_scope_or_whole_worktree_proofs() {
    let root = tempdir().unwrap();
    let decoy = tempdir().unwrap();
    for repo in [root.path(), decoy.path()] {
        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.email", "fixture@example.com"]);
        run_git(repo, &["config", "user.name", "Fixture"]);
        std::fs::write(repo.join("tracked.txt"), "baseline\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "baseline"]);
    }
    let baseline = resolve_git_commit(root.path(), "HEAD").unwrap();
    std::fs::write(root.path().join("tracked.txt"), "changed\n").unwrap();
    let expected_whole = repo_worktree_fingerprint(root.path()).unwrap();
    let expected_scope = gate_scope_snapshot(
        root.path(),
        &baseline,
        Some(&["tracked.txt".into()]),
        &[],
        "fixture",
    )
    .unwrap();

    for (name, value) in [
        ("GIT_DIR", decoy.path().join(".git")),
        ("GIT_WORK_TREE", decoy.path().to_path_buf()),
        ("GIT_INDEX_FILE", decoy.path().join(".git/index")),
    ] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", REDIRECT_HELPER_TEST, "--nocapture"])
            .env(REDIRECT_HELPER_ENV, name)
            .env(REDIRECT_HELPER_ROOT_ENV, root.path())
            .env(REDIRECT_HELPER_WHOLE_ENV, &expected_whole)
            .env(REDIRECT_HELPER_SCOPE_ENV, &expected_scope.scope_fingerprint);
        configure_read_only_git_environment(&mut command);
        let output = command.env(name, value).output().unwrap();
        assert!(
            output.status.success(),
            "ambient {name} helper failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn repository_redirect_environment_helper() {
    let Some(redirect) = std::env::var_os(REDIRECT_HELPER_ENV) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(REDIRECT_HELPER_ROOT_ENV).unwrap());
    let baseline = resolve_git_commit(&root, "HEAD").unwrap();
    let expected_whole = std::env::var(REDIRECT_HELPER_WHOLE_ENV).unwrap();
    let expected_scope = std::env::var(REDIRECT_HELPER_SCOPE_ENV).unwrap();

    assert_eq!(
        repo_worktree_fingerprint(&root).unwrap(),
        expected_whole,
        "ambient {} changed the whole-worktree proof",
        redirect.to_string_lossy(),
    );
    assert_eq!(
        gate_scope_snapshot(
            &root,
            &baseline,
            Some(&["tracked.txt".into()]),
            &[],
            "fixture",
        )
        .unwrap()
        .scope_fingerprint,
        expected_scope,
        "ambient {} changed the scoped proof",
        redirect.to_string_lossy(),
    );
}

#[test]
fn whole_worktree_fingerprint_disables_external_diff_and_textconv_configuration() {
    use std::os::unix::fs::PermissionsExt as _;

    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    let tools = tempdir().unwrap();
    let marker = tools.path().join("external-diff-ran");
    let external = tools.path().join("external-diff.sh");
    std::fs::write(
        &external,
        format!("#!/bin/sh\n: > '{}'\nexit 0\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&external, std::fs::Permissions::from_mode(0o755)).unwrap();

    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join(".gitattributes"), "*.txt diff=fixture\n").unwrap();
    std::fs::write(temp.path().join("tracked.txt"), "baseline\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "baseline"]);
    std::fs::write(temp.path().join("tracked.txt"), "changed once\n").unwrap();
    let expected = repo_worktree_fingerprint(temp.path()).unwrap();

    let global = tools.path().join("global.gitconfig");
    std::fs::write(
        &global,
        format!(
            "[diff]\n\texternal = {}\n[diff \"fixture\"]\n\ttextconv = {}\n",
            external.display(),
            external.display()
        ),
    )
    .unwrap();
    let _global = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", &global);
    assert_eq!(repo_worktree_fingerprint(temp.path()).unwrap(), expected);
    assert!(!marker.exists(), "global diff program was executed");

    run_git(
        temp.path(),
        &["config", "diff.external", external.to_str().unwrap()],
    );
    run_git(
        temp.path(),
        &[
            "config",
            "diff.fixture.textconv",
            external.to_str().unwrap(),
        ],
    );
    assert_eq!(repo_worktree_fingerprint(temp.path()).unwrap(), expected);
    assert!(!marker.exists(), "local diff program was executed");

    std::fs::write(temp.path().join("tracked.txt"), "changed twice\n").unwrap();
    assert_ne!(repo_worktree_fingerprint(temp.path()).unwrap(), expected);
    assert!(
        !marker.exists(),
        "diff program was executed after content changed"
    );
}

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
fn gate_scope_fingerprint_fails_closed_on_oversized_committed_binary_diff() {
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
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    let mut state = 0x0bad_f00d_u32;
    let changed = (0..16_384)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect::<Vec<_>>();
    std::fs::write(temp.path().join("asset.bin"), changed).unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "large binary"]);

    GATE_SCOPE_DIFF_OUTPUT_LIMIT_OVERRIDE.set(Some(256));
    let result = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&["asset.bin".into()]),
        &[],
        "fixture",
    );
    GATE_SCOPE_DIFF_OUTPUT_LIMIT_OVERRIDE.set(None);
    let error = result.unwrap_err();

    assert!(
        format!("{error:#}").contains("gate-scope proof Git output limit of 256 bytes"),
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancellation_before_fingerprint_git_spawn_remains_typed() {
    let temp = tempdir().unwrap();
    let calls = Cell::new(0);
    let error = repo_worktree_fingerprint_with_cancellation(temp.path(), &|| {
        let current = calls.get();
        calls.set(current + 1);
        current == 1
    })
    .unwrap_err();

    assert!(is_git_receipt_collection_cancellation(&error), "{error:#}");
    assert_eq!(calls.get(), 2);
}

#[test]
fn parse_diff_stat_output_counts_binary_files_without_swallowing_other_errors() {
    let diff_stat =
        parse_diff_stat_output("12\t3\tsrc/main.rs\n-\t-\tassets/logo.png\n").unwrap();
    assert_eq!(diff_stat.files, 2);
    assert_eq!(diff_stat.insertions, 12);
    assert_eq!(diff_stat.deletions, 3);
}

#[test]
fn parse_diff_stat_output_rejects_invalid_counts() {
    let error = parse_diff_stat_output("oops\t3\tsrc/main.rs\n")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Invalid git diff --numstat insertions count"));
}

#[test]
fn collect_git_receipt_metadata_records_git_failures() {
    let temp = tempdir().unwrap();
    let metadata = collect_git_receipt_metadata(temp.path());

    assert!(metadata.changed_paths.is_empty());
    assert_eq!(metadata.changed_path_count, None);
    assert!(!metadata.changed_paths_truncated);
    assert_eq!(metadata.changed_paths_digest, None);
    assert_eq!(metadata.diff_stat.files, 0);
    assert!(metadata.git_status_error.is_some());
    assert!(metadata.git_diff_stat_error.is_some());
    assert!(metadata.worktree_fingerprint.is_none());
    assert!(metadata.worktree_fingerprint_error.is_some());
}

#[test]
fn cancelled_receipt_metadata_starts_no_git_subcollection() {
    let temp = tempdir().unwrap();
    let checks = Cell::new(0);

    let metadata = collect_git_receipt_metadata_with_cancellation(temp.path(), &|| {
        checks.set(checks.get() + 1);
        true
    });

    assert!(metadata.changed_paths.is_empty());
    assert_eq!(metadata.changed_path_count, None);
    assert_eq!(metadata.diff_stat.files, 0);
    assert!(
        metadata
            .git_status_error
            .as_deref()
            .is_some_and(|error| error.contains("collection was cancelled"))
    );
    assert!(
        metadata
            .git_diff_stat_error
            .as_deref()
            .is_some_and(|error| error.contains("collection was cancelled"))
    );
    assert!(metadata.worktree_fingerprint.is_none());
    assert!(
        metadata
            .worktree_fingerprint_error
            .as_deref()
            .is_some_and(|error| error.contains("collection was cancelled"))
    );
    assert_eq!(checks.get(), 3);
}

#[test]
fn changed_paths_preserve_spaces_and_rename_paths() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("old name.txt"), "tracked").unwrap();
    run_git(temp.path(), &["add", "old name.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    run_git(temp.path(), &["mv", "old name.txt", "new name.txt"]);
    std::fs::write(temp.path().join("loose note.txt"), "untracked").unwrap();

    let paths = repo_changed_paths(temp.path()).unwrap();

    assert!(paths.contains(&"new name.txt".to_string()));
    assert!(paths.contains(&"old name.txt".to_string()));
    assert!(paths.contains(&"loose note.txt".to_string()));
}

#[test]
fn receipt_metadata_excludes_agent_state_from_paths_and_diff_stat() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    std::fs::write(temp.path().join("src.rs"), "one\n").unwrap();
    std::fs::write(temp.path().join(".agent/state/receipts.jsonl"), "old\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    std::fs::write(temp.path().join("src.rs"), "one\ntwo\n").unwrap();
    std::fs::write(
        temp.path().join(".agent/state/receipts.jsonl"),
        "old\nnew\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("note.txt"), "untracked\n").unwrap();
    std::fs::write(
        temp.path().join(".agent/state/untracked.jsonl"),
        "ignored by receipt metadata\n",
    )
    .unwrap();

    let metadata = collect_git_receipt_metadata_without_worktree_fingerprint(temp.path());

    assert_eq!(metadata.changed_paths, ["note.txt", "src.rs"]);
    assert_eq!(metadata.changed_path_count, Some(2));
    assert!(!metadata.changed_paths_truncated);
    assert!(metadata.changed_paths_digest.is_some());
    assert_eq!(metadata.diff_stat.files, 1);
    assert_eq!(metadata.diff_stat.insertions, 1);
    assert_eq!(metadata.diff_stat.deletions, 0);
}

#[test]
fn changed_path_preview_is_bounded_sorted_and_digest_covers_the_full_set() {
    let paths = (0..105)
        .rev()
        .map(|index| format!("src/path-{index:03}.rs"))
        .chain(["src/path-042.rs".to_string()])
        .collect::<Vec<_>>();
    let bounded = bounded_changed_paths(paths.clone());
    let reordered = bounded_changed_paths(paths.into_iter().rev().collect());

    assert_eq!(bounded.preview.len(), MAX_RECEIPT_CHANGED_PATHS);
    assert_eq!(bounded.total, 105);
    assert!(bounded.truncated);
    assert_eq!(bounded.preview[0], "src/path-000.rs");
    assert_eq!(bounded.preview[99], "src/path-099.rs");
    assert!(bounded.digest.starts_with("sha256:"));
    assert_eq!(bounded.digest, reordered.digest);
    assert_eq!(bounded.preview, reordered.preview);

    let preview_only_digest = changed_paths_digest(&bounded.preview);
    assert_ne!(bounded.digest, preview_only_digest);
}
