#[test]
fn prepared_plan_change_snapshot_feeds_multiple_gate_scopes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    std::fs::create_dir_all(temp.path().join("crates/api")).unwrap();
    std::fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 1;\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("crates/api/lib.rs"),
        "pub const V: u8 = 1;\n",
    )
    .unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    std::fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 2;\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("crates/api/lib.rs"),
        "pub const V: u8 = 2;\n",
    )
    .unwrap();

    let plan = plan_change_snapshot(temp.path(), &baseline).unwrap();
    GATE_SCOPE_INPUT_COLLECTION_COUNT.set(0);
    let frontend = gate_scope_snapshot_from_plan_change(
        temp.path(),
        &plan,
        Some(&["apps/**".into()]),
        &[],
        "frontend-signature",
    )
    .unwrap();
    let rust = gate_scope_snapshot_from_plan_change(
        temp.path(),
        &plan,
        Some(&["crates/**".into()]),
        &[],
        "rust-signature",
    )
    .unwrap();

    assert_eq!(frontend.facts.applicability, GateApplicability::Applicable);
    assert_eq!(rust.facts.applicability, GateApplicability::Applicable);
    assert_eq!(frontend.facts.changed_path_count, 2);
    assert_eq!(rust.facts.changed_path_count, 2);
    assert_eq!(frontend.facts.matching_paths, ["apps/web/main.ts"]);
    assert_eq!(rust.facts.matching_paths, ["crates/api/lib.rs"]);
    assert_eq!(GATE_SCOPE_INPUT_COLLECTION_COUNT.get(), 2);

    let equivalent_frontend = gate_scope_snapshot_from_plan_change(
        temp.path(),
        &plan,
        Some(&["apps/**".into(), "apps/**".into()]),
        &[],
        "another-frontend-signature",
    )
    .unwrap();
    assert_eq!(GATE_SCOPE_INPUT_COLLECTION_COUNT.get(), 2);
    assert_eq!(
        equivalent_frontend.facts.matching_paths,
        ["apps/web/main.ts"],
        "normalized equivalent policies must share their cached input snapshot"
    );
    assert_eq!(
        frontend.facts, equivalent_frontend.facts,
        "signature binding must preserve every cached gate-scope fact"
    );
    assert_ne!(
        frontend.scope_fingerprint, equivalent_frontend.scope_fingerprint,
        "the cached input must still be bound to each gate signature"
    );
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn plan_change_snapshot_fails_closed_for_non_utf8_repository_paths() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "tracked\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    let invalid = OsString::from_vec(b"bad-\xff.rs".to_vec());
    std::fs::write(temp.path().join(invalid), "untracked\n").unwrap();

    let error = plan_change_snapshot(temp.path(), &baseline).unwrap_err();

    assert!(
        format!("{error:#}").contains("git ls-files path was not UTF-8"),
        "{error:#}"
    );
}

#[test]
fn plan_change_snapshot_fails_closed_when_changed_path_output_exceeds_limit() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    let path = temp
        .path()
        .join("a-tracked-file-name-longer-than-the-test-output-limit.txt");
    std::fs::write(&path, "baseline\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    std::fs::write(path, "changed\n").unwrap();

    CHANGED_PATH_GIT_OUTPUT_LIMIT_OVERRIDE.set(Some(16));
    let result = plan_change_snapshot(temp.path(), &baseline);
    CHANGED_PATH_GIT_OUTPUT_LIMIT_OVERRIDE.set(None);
    let error = result.unwrap_err();

    assert!(
        format!("{error:#}").contains("changed-path Git output limit of 16 bytes"),
        "{error:#}"
    );
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
fn gate_scope_classifies_both_sides_of_a_rename() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("crates/api")).unwrap();
    std::fs::write(temp.path().join("crates/api/lib.rs"), "pub fn value() {}\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    std::fs::create_dir_all(temp.path().join("docs")).unwrap();
    std::fs::rename(
        temp.path().join("crates/api/lib.rs"),
        temp.path().join("docs/lib.rs"),
    )
    .unwrap();

    let snapshot = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&["crates/**".into()]),
        &[],
        "rust-signature",
    )
    .unwrap();
    assert_eq!(snapshot.facts.applicability, GateApplicability::Applicable);
    assert!(
        snapshot
            .facts
            .matching_paths
            .contains(&"crates/api/lib.rs".into())
    );
}

#[test]
fn gate_scope_honors_ignores_and_hashes_matching_untracked_content() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("crates/api/generated")).unwrap();
    std::fs::write(temp.path().join("crates/api/lib.rs"), "pub fn value() {}\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    let paths = vec!["crates/**".into()];
    let ignores = vec!["crates/**/generated/**".into()];

    std::fs::write(
        temp.path().join("crates/api/generated/cache.sql"),
        "ignored one\n",
    )
    .unwrap();
    let ignored = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&paths),
        &ignores,
        "rust-signature",
    )
    .unwrap();
    assert_eq!(
        ignored.facts.applicability,
        GateApplicability::NotApplicable
    );
    std::fs::write(
        temp.path().join("crates/api/generated/cache.sql"),
        "ignored two\n",
    )
    .unwrap();
    let ignored_changed = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&paths),
        &ignores,
        "rust-signature",
    )
    .unwrap();
    assert_eq!(ignored.scope_fingerprint, ignored_changed.scope_fingerprint);

    std::fs::write(temp.path().join("crates/api/query.sql"), "select 1;\n").unwrap();
    let first = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&paths),
        &ignores,
        "rust-signature",
    )
    .unwrap();
    assert_eq!(first.facts.applicability, GateApplicability::Applicable);
    assert_eq!(first.facts.matching_paths, ["crates/api/query.sql"]);
    std::fs::write(temp.path().join("crates/api/query.sql"), "select 2;\n").unwrap();
    let second = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&paths),
        &ignores,
        "rust-signature",
    )
    .unwrap();
    assert_ne!(first.scope_fingerprint, second.scope_fingerprint);

    run_git(temp.path(), &["add", "crates/api/query.sql"]);
    run_git(temp.path(), &["commit", "-m", "commit during plan"]);
    let committed = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&paths),
        &ignores,
        "rust-signature",
    )
    .unwrap();
    assert_eq!(committed.facts.applicability, GateApplicability::Applicable);
    assert_eq!(committed.facts.matching_paths, ["crates/api/query.sql"]);
}

#[test]
fn gate_scope_cancellation_before_baseline_resolution_remains_typed() {
    let temp = tempdir().unwrap();
    let error = gate_scope_snapshot_with_cancellation(
        temp.path(),
        "HEAD",
        Some(&["crates/**".into()]),
        &[],
        "signature",
        &|| true,
    )
    .unwrap_err();

    assert!(is_git_receipt_collection_cancellation(&error), "{error:#}");
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
    let baseline = resolve_git_commit(parent.path(), "HEAD").unwrap();
    run_git(parent.path(), &["config", "diff.ignoreSubmodules", "all"]);
    let untracked = parent.path().join("vendor/dependency/generated");
    std::fs::create_dir_all(&untracked).unwrap();
    for index in 0..2_000 {
        std::fs::write(untracked.join(format!("entry-{index:04}.txt")), "dirty\n").unwrap();
    }

    let scope = gate_scope_snapshot(
        parent.path(),
        &baseline,
        Some(&["vendor/**".into()]),
        &[],
        "submodule",
    )
    .unwrap_err()
    .to_string();
    let whole = repo_worktree_fingerprint(parent.path())
        .unwrap_err()
        .to_string();

    assert!(
        scope.contains("gitlink") || scope.contains("submodule"),
        "{scope}"
    );
    assert!(
        whole.contains("gitlink") || whole.contains("submodule"),
        "{whole}"
    );
}

#[test]
fn staged_clean_submodule_pointer_is_attested() {
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
    let baseline = resolve_git_commit(parent.path(), "HEAD").unwrap();
    let checkout = parent.path().join("vendor/dependency");
    run_git(&checkout, &["config", "user.email", "fixture@example.com"]);
    run_git(&checkout, &["config", "user.name", "Fixture"]);
    std::fs::write(checkout.join("source.txt"), "two\n").unwrap();
    run_git(&checkout, &["add", "."]);
    run_git(&checkout, &["commit", "-m", "advance dependency"]);

    assert!(
        gate_scope_snapshot(
            parent.path(),
            &baseline,
            Some(&["vendor/**".into()]),
            &[],
            "submodule",
        )
        .is_err()
    );
    run_git(parent.path(), &["add", "vendor/dependency"]);
    let scope = gate_scope_snapshot(
        parent.path(),
        &baseline,
        Some(&["vendor/**".into()]),
        &[],
        "submodule",
    )
    .unwrap();
    assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
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
    let baseline = resolve_git_commit(parent.path(), "HEAD").unwrap();
    let embedded = parent.path().join("vendor/embedded");
    std::fs::create_dir_all(&embedded).unwrap();
    run_git(&embedded, &["init"]);
    std::fs::write(embedded.join("source.txt"), "nested\n").unwrap();

    let scope = gate_scope_snapshot(
        parent.path(),
        &baseline,
        Some(&["vendor/**".into()]),
        &[],
        "embedded",
    )
    .unwrap_err()
    .to_string();
    let whole = repo_worktree_fingerprint(parent.path())
        .unwrap_err()
        .to_string();

    assert!(scope.contains("untracked directory"), "{scope}");
    assert!(whole.contains("untracked directory"), "{whole}");
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
