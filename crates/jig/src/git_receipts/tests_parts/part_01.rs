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
fn repository_redirect_environment_cannot_change_whole_worktree_proofs() {
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
    std::fs::write(root.path().join("tracked.txt"), "changed\n").unwrap();
    let expected_whole = repo_worktree_fingerprint(root.path()).unwrap();

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
            .env(REDIRECT_HELPER_WHOLE_ENV, &expected_whole);
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
    let expected_whole = std::env::var(REDIRECT_HELPER_WHOLE_ENV).unwrap();

    assert_eq!(
        repo_worktree_fingerprint(&root).unwrap(),
        expected_whole,
        "ambient {} changed the whole-worktree proof",
        redirect.to_string_lossy(),
    );
}
