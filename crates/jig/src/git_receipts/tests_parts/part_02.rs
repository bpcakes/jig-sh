#[cfg(unix)]
#[test]
fn porcelain_z_parser_preserves_non_utf8_path_bytes() {
    let entries = parse_porcelain_status_z(b"?? bad\xFFname\0").unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].path.as_os_str().as_encoded_bytes(),
        b"bad\xFFname"
    );
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn whole_worktree_fingerprint_preserves_non_utf8_tracked_paths() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    let file_name = OsString::from_vec(b"tracked-\xff.rs".to_vec());
    let path = temp.path().join(&file_name);
    fs::write(&path, "one\n").unwrap();
    let added = Command::new("git")
        .current_dir(temp.path())
        .arg("add")
        .arg("--")
        .arg(&file_name)
        .output()
        .unwrap();
    assert!(added.status.success(), "{:?}", added.stderr);
    run_git(temp.path(), &["commit", "-m", "non-UTF-8 fixture"]);

    let clean = repo_worktree_fingerprint(temp.path()).unwrap();
    fs::write(&path, "two\n").unwrap();
    let changed = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(clean, changed);
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
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let script = temp.path().join("scripts/check.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    let paths = vec!["scripts/**".to_string()];
    let whole_before = repo_worktree_fingerprint(temp.path()).unwrap();
    let scope_before =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "scripts").unwrap();

    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let whole_after = repo_worktree_fingerprint(temp.path()).unwrap();
    let scope_after =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "scripts").unwrap();

    assert_ne!(whole_before, whole_after);
    assert_ne!(
        scope_before.scope_fingerprint,
        scope_after.scope_fingerprint
    );
}

#[test]
fn empty_tree_baseline_classifies_initial_repository_files() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "pub fn initial() {}\n").unwrap();
    let empty_tree = resolve_empty_tree_for_unborn_repository(temp.path())
        .unwrap()
        .unwrap();
    let plan = plan_change_snapshot_from_empty_tree(temp.path(), &empty_tree).unwrap();
    let paths = vec!["src/**".to_string()];

    let scope = gate_scope_snapshot_from_plan_change(
        temp.path(),
        &plan,
        Some(&paths),
        &[],
        "initial-rust",
    )
    .unwrap();

    assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
    assert_eq!(scope.facts.matching_paths, ["src/lib.rs"]);
}

#[test]
fn masked_staged_change_is_classified_and_fails_scope_proof() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    let source = temp.path().join("src/lib.rs");
    std::fs::write(&source, "pub const VALUE: u8 = 1;\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "baseline"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();

    std::fs::write(&source, "pub const VALUE: u8 = 2;\n").unwrap();
    run_git(temp.path(), &["add", "src/lib.rs"]);
    std::fs::write(&source, "pub const VALUE: u8 = 1;\n").unwrap();

    let plan = plan_change_snapshot(temp.path(), &baseline).unwrap();
    assert!(plan.changed_paths.iter().any(|path| path == "src/lib.rs"));
    let paths = vec!["src/**".to_string()];
    let error =
        gate_scope_snapshot_from_plan_change(temp.path(), &plan, Some(&paths), &[], "rust")
            .unwrap_err()
            .to_string();
    assert!(
        error.contains("partially staged gate input src/lib.rs"),
        "{error}"
    );

    std::fs::write(&source, "pub const VALUE: u8 = 2;\n").unwrap();
    let aligned = plan_change_snapshot(temp.path(), &baseline).unwrap();
    let scope =
        gate_scope_snapshot_from_plan_change(temp.path(), &aligned, Some(&paths), &[], "rust")
            .unwrap();
    assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
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
    let paths = vec!["ignored-input.txt".to_string()];

    for replacement in ["first replacement\n", "different replacement\n"] {
        std::fs::write(temp.path().join("ignored-input.txt"), replacement).unwrap();
        let plan = plan_change_snapshot(temp.path(), &baseline).unwrap();
        assert!(
            plan.changed_paths
                .iter()
                .any(|path| path == "ignored-input.txt")
        );
        assert!(plan.untracked_paths.is_empty());

        let scoped_error = gate_scope_snapshot_from_plan_change(
            temp.path(),
            &plan,
            Some(&paths),
            &[],
            "ignored-replacement",
        )
        .unwrap_err()
        .to_string();
        assert!(
            scoped_error.contains("staged deletion ignored-input.txt"),
            "{scoped_error}"
        );

        let whole_error = repo_worktree_fingerprint(temp.path())
            .unwrap_err()
            .to_string();
        assert!(
            whole_error.contains("staged deletion ignored-input.txt"),
            "{whole_error}"
        );
    }
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn canonical_diff_order_file_preserves_non_utf8_temporary_directory() {
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("source.txt"), "baseline\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "baseline"]);
    std::fs::write(temp.path().join("source.txt"), "changed\n").unwrap();

    let raw_temp = temp
        .path()
        .join(OsString::from_vec(b"proof-temp-\xff".to_vec()));
    std::fs::create_dir(&raw_temp).unwrap();

    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", NON_UTF8_TMPDIR_HELPER_TEST, "--nocapture"])
        .env(NON_UTF8_TMPDIR_HELPER_ENV, "1")
        .env(NON_UTF8_TMPDIR_HELPER_ROOT_ENV, temp.path())
        .env("TMPDIR", raw_temp);
    configure_read_only_git_environment(&mut command);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "non-UTF-8 TMPDIR helper failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn canonical_diff_order_file_preserves_non_utf8_temporary_directory_helper() {
    if std::env::var_os(NON_UTF8_TMPDIR_HELPER_ENV).is_none() {
        return;
    }
    let root = PathBuf::from(std::env::var_os(NON_UTF8_TMPDIR_HELPER_ROOT_ENV).unwrap());
    let baseline = resolve_git_commit(&root, "HEAD").unwrap();

    let whole = repo_worktree_fingerprint(&root).unwrap();
    assert!(whole.starts_with("sha256:"));
    let scope = gate_scope_snapshot(
        &root,
        &baseline,
        Some(&["source.txt".to_string()]),
        &[],
        "non-utf8-temp",
    )
    .unwrap();
    assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
    assert!(scope.scope_fingerprint.starts_with("sha256:"));
}

#[test]
fn gate_scope_is_path_sensitive_and_ignores_unrelated_fingerprint_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    std::fs::create_dir_all(temp.path().join("crates/api/src")).unwrap();
    std::fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 1;\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("crates/api/src/lib.rs"),
        "pub const V: u8 = 1;\n",
    )
    .unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    let rust_paths = vec!["crates/**".to_string()];
    let before = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&rust_paths),
        &[],
        "rust-signature",
    )
    .unwrap();
    assert_eq!(before.facts.applicability, GateApplicability::NotApplicable);

    std::fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 2;\n",
    )
    .unwrap();
    let frontend_only = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&rust_paths),
        &[],
        "rust-signature",
    )
    .unwrap();
    assert_eq!(
        frontend_only.facts.applicability,
        GateApplicability::NotApplicable
    );
    assert_eq!(before.scope_fingerprint, frontend_only.scope_fingerprint);

    std::fs::write(
        temp.path().join("crates/api/src/lib.rs"),
        "pub const V: u8 = 2;\n",
    )
    .unwrap();
    let rust_changed = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&rust_paths),
        &[],
        "rust-signature",
    )
    .unwrap();
    assert_eq!(
        rust_changed.facts.applicability,
        GateApplicability::Applicable
    );
    assert_ne!(
        frontend_only.scope_fingerprint,
        rust_changed.scope_fingerprint
    );
    assert_eq!(rust_changed.facts.matching_paths, ["crates/api/src/lib.rs"]);
    let changed_signature = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&rust_paths),
        &[],
        "changed-rust-signature",
    )
    .unwrap();
    assert_ne!(
        rust_changed.scope_fingerprint, changed_signature.scope_fingerprint,
        "gate policy and command changes must invalidate scoped evidence"
    );
}
