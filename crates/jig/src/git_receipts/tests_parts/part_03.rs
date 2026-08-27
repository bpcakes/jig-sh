#[test]
fn supported_gate_globs_select_the_same_tracked_diff_they_classify() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    std::fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const value = 1;\n",
    )
    .unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();

    for pattern in [
        "apps/*/main.ts",
        "apps/web/main.?s",
        "apps/[w]eb/**",
        "apps/**",
    ] {
        let paths = vec![pattern.to_string()];
        let before = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&paths),
            &[],
            "frontend-signature",
        )
        .unwrap();
        assert_eq!(before.facts.applicability, GateApplicability::NotApplicable);

        std::fs::write(
            temp.path().join("apps/web/main.ts"),
            format!("export const selectedBy = {pattern:?};\n"),
        )
        .unwrap();
        let changed = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&paths),
            &[],
            "frontend-signature",
        )
        .unwrap();
        assert_eq!(
            changed.facts.applicability,
            GateApplicability::Applicable,
            "classification did not select {pattern}"
        );
        assert_eq!(changed.facts.matching_paths, ["apps/web/main.ts"]);
        assert_ne!(
            before.scope_fingerprint, changed.scope_fingerprint,
            "Git fingerprint pathspec did not select {pattern}"
        );
        std::fs::write(
            temp.path().join("apps/web/main.ts"),
            "export const value = 1;\n",
        )
        .unwrap();
    }
}
#[test]
fn classifier_selected_directory_paths_are_hashed_literally() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("docs")).unwrap();
    std::fs::write(temp.path().join("docs/guide.md"), "before\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    let paths = vec!["**".to_string()];

    for ignored in ["docs", "docs/"] {
        let ignores = vec![ignored.to_string()];
        let before =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &ignores, "docs")
                .unwrap();
        std::fs::write(
            temp.path().join("docs/guide.md"),
            format!("selected despite {ignored:?}\n"),
        )
        .unwrap();
        let after = gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &ignores, "docs")
            .unwrap();
        assert_eq!(after.facts.applicability, GateApplicability::Applicable);
        assert_eq!(after.facts.matching_paths, ["docs/guide.md"]);
        assert_ne!(before.scope_fingerprint, after.scope_fingerprint);
        std::fs::write(temp.path().join("docs/guide.md"), "before\n").unwrap();
    }
}

#[test]
fn global_gate_authorities_apply_to_every_path_aware_gate() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join(".agent")).unwrap();
    std::fs::write(temp.path().join(".jig.toml"), "contract_version = 5\n").unwrap();
    std::fs::write(temp.path().join(".agent/jig-contract.json"), "{}\n").unwrap();
    std::fs::write(temp.path().join("src.rs"), "pub fn stable() {}\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    let paths = vec!["crates/**".to_string()];
    let ignores = vec![".jig.toml".to_string()];

    std::fs::write(
        temp.path().join(".jig.toml"),
        "contract_version = 5\n# changed\n",
    )
    .unwrap();
    let config_scope =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &ignores, "rust").unwrap();
    assert_eq!(
        config_scope.facts.applicability,
        GateApplicability::Applicable
    );
    assert_eq!(config_scope.facts.matching_paths, [".jig.toml"]);

    std::fs::write(temp.path().join(".jig.toml"), "contract_version = 5\n").unwrap();
    std::fs::write(
        temp.path().join(".agent/jig-contract.json"),
        "{\"changed\":true}\n",
    )
    .unwrap();
    let manifest_scope =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "rust").unwrap();
    assert_eq!(
        manifest_scope.facts.applicability,
        GateApplicability::Applicable
    );
    assert_eq!(
        manifest_scope.facts.matching_paths,
        [".agent/jig-contract.json"]
    );
}

#[test]
fn gate_scope_diff_is_independent_of_ambient_git_diff_configuration() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(
        temp.path().join("source.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("second.txt"), "alpha\nbeta\ngamma\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    std::fs::write(
        temp.path().join("source.txt"),
        "zero\none\nthree\nfour\nfive\nchanged\nseven\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("second.txt"), "alpha\nchanged\ngamma\n").unwrap();
    let paths = vec!["*.txt".to_string()];
    let before =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "source").unwrap();

    run_git(temp.path(), &["config", "diff.algorithm", "histogram"]);
    run_git(temp.path(), &["config", "diff.indentHeuristic", "true"]);
    run_git(temp.path(), &["config", "diff.renames", "copies"]);
    run_git(temp.path(), &["config", "diff.mnemonicPrefix", "true"]);
    run_git(temp.path(), &["config", "diff.context", "0"]);
    run_git(temp.path(), &["config", "diff.interHunkContext", "99"]);
    run_git(temp.path(), &["config", "diff.relative", "true"]);
    std::fs::write(temp.path().join("diff-order"), "second.txt\nsource.txt\n").unwrap();
    run_git(temp.path(), &["config", "diff.orderFile", "diff-order"]);
    let after =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "source").unwrap();

    assert_eq!(before.scope_fingerprint, after.scope_fingerprint);
}

#[cfg(unix)]
#[test]
fn tracked_execution_mode_changes_are_classified_when_core_file_mode_is_disabled() {
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
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    let paths = vec!["scripts/**".to_string()];
    let whole_before = repo_worktree_fingerprint(temp.path()).unwrap();
    let before =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "scripts").unwrap();
    run_git(temp.path(), &["config", "core.fileMode", "false"]);

    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let after =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "scripts").unwrap();
    let whole_after = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_eq!(before.facts.applicability, GateApplicability::NotApplicable);
    assert_eq!(after.facts.applicability, GateApplicability::Applicable);
    assert_eq!(after.facts.matching_paths, ["scripts/check.sh"]);
    assert_ne!(before.scope_fingerprint, after.scope_fingerprint);
    assert_ne!(whole_before, whole_after);
}

#[test]
fn literal_pathspec_chunking_bounds_many_thousand_paths() {
    let paths = (0..5_000)
        .map(|index| {
            format!("generated/very-long-component-{index:05}/source-file-{index:05}.rs")
        })
        .collect::<Vec<_>>();
    let references = paths.iter().collect::<Vec<_>>();
    let chunks = literal_pathspec_chunks(&references);

    assert!(chunks.len() > 1);
    assert_eq!(chunks.iter().map(|chunk| chunk.len()).sum::<usize>(), 5_000);
    for chunk in chunks {
        assert!(chunk.len() <= MAX_GIT_LITERAL_PATHS_PER_DIFF);
        assert!(
            chunk
                .iter()
                .map(|path| b":(top,literal)".len() + path.len() + 1)
                .sum::<usize>()
                <= MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF
        );
    }
}

#[test]
fn literal_pathspec_chunking_allows_one_oversized_path_and_keeps_progress() {
    let paths = [
        "x".repeat(MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF + 1),
        "tail.rs".to_string(),
    ];
    let references = paths.iter().collect::<Vec<_>>();

    let chunks = literal_pathspec_chunks(&references);

    assert_eq!(chunks, [&references[..1], &references[1..]]);
}

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

#[test]
fn gate_scope_hashes_thousands_of_tracked_paths_in_bounded_git_calls() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("generated")).unwrap();
    for index in 0..3_000 {
        std::fs::write(
            temp.path().join(format!("generated/file-{index:04}.txt")),
            "before\n",
        )
        .unwrap();
    }
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    for index in 0..3_000 {
        std::fs::write(
            temp.path().join(format!("generated/file-{index:04}.txt")),
            "after\n",
        )
        .unwrap();
    }

    let scope = gate_scope_snapshot(
        temp.path(),
        &baseline,
        Some(&["generated/**".into()]),
        &[],
        "generated",
    )
    .unwrap();

    assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
    assert_eq!(scope.facts.matching_path_count, 3_000);
    assert!(scope.facts.matching_paths_truncated);
    assert!(scope.scope_fingerprint.starts_with("sha256:"));
}

#[test]
fn gate_scope_streams_a_large_binary_diff_into_the_fingerprint() {
    fn fixture_bytes(mut state: u64, length: usize) -> Vec<u8> {
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("assets")).unwrap();
    let asset = temp.path().join("assets/blob.bin");
    std::fs::write(&asset, fixture_bytes(1, 5 * 1024 * 1024)).unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
    let paths = vec!["assets/**".to_string()];
    let before =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "asset-signature")
            .unwrap();

    std::fs::write(&asset, fixture_bytes(2, 5 * 1024 * 1024)).unwrap();
    let changed =
        gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "asset-signature")
            .unwrap();

    assert_eq!(changed.facts.applicability, GateApplicability::Applicable);
    assert_eq!(changed.facts.matching_paths, ["assets/blob.bin"]);
    assert_ne!(before.scope_fingerprint, changed.scope_fingerprint);
}
