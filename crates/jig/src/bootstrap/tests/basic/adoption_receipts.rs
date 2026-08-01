use super::*;

#[test]
fn adopt_previews_by_default_without_writing_files() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(repo.join("bun.lock"), "").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["render_mode"], "preview");
    assert_eq!(output["write"], false);
    assert!(output.get("adoption_report").is_none());
    assert_eq!(output["render_report"]["dry_run"], true);
    assert_eq!(
        output["detection_report"]["web_package_manager"],
        serde_json::Value::Null
    );
    assert_eq!(
        output["adoption_profile"]["detected_stack"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        Vec::<&str>::new()
    );
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap().contains("jig adopt . --write"))
    );
    assert!(!repo.join(".jig.toml").exists());
    assert!(!repo.join("scripts/jig").exists());
}

#[test]
fn adopt_preview_reports_conflicts_without_overwriting() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();
    fs::write(repo.join(".agent/PLANS.md"), "repo-owned plan notes\n").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["render_mode"], "preview");
    assert!(
        output["render_report"]["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|conflict| {
                conflict["path"] == ".agent/PLANS.md" && conflict["kind"] == "modified_managed_path"
            })
    );
    assert_eq!(
        fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
        "repo-owned plan notes\n"
    );
}

#[test]
fn adopt_preserves_repo_gitattributes_while_adding_jig_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".gitattributes"),
        "* text=auto eol=lf\n*.sh text eol=lf\n",
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["render_mode"], "copy");
    assert!(
        output["render_report"]["managed_blocks_inserted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".gitattributes")
    );
    let attributes = fs::read_to_string(repo.join(".gitattributes")).unwrap();
    assert!(attributes.contains("* text=auto eol=lf"));
    assert!(attributes.contains(".agent/state/*.jsonl merge=union"));
}

#[test]
fn adopt_write_records_backup_receipt_for_overwritten_managed_files() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();
    fs::write(repo.join(".agent/PLANS.md"), "repo-owned plan notes\n").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["render_mode"], "copy");
    assert!(
        output["render_report"]["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|conflict| conflict["path"] == ".agent/PLANS.md")
    );
    let receipt_path = repo.join(".agent/.cache/adopt/adopt-last.json");
    let receipt: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();
    assert!(
        receipt["backup_root"]
            .as_str()
            .unwrap()
            .contains(".agent/.cache/adopt/backups")
    );
    let legacy_receipt_path = repo.join(".agent/state/adopt-last.json");
    let legacy_receipt: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&legacy_receipt_path).unwrap()).unwrap();
    assert_eq!(legacy_receipt, receipt);
    assert_eq!(
        receipt["canonical_receipt_path"],
        ".agent/.cache/adopt/adopt-last.json"
    );
    assert_eq!(receipt["legacy_receipt_deprecated"], true);
    assert!(!repo.join(".agent/state/adopt-backups").exists());
    let backup = receipt["apply_report"]["backups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|backup| backup["path"] == ".agent/PLANS.md")
        .expect("missing .agent/PLANS.md backup");
    let backup_path = backup["backup_path"].as_str().unwrap();
    assert_eq!(
        fs::read_to_string(backup_path).unwrap(),
        "repo-owned plan notes\n"
    );
    assert!(
        receipt["undo_hint"]
            .as_str()
            .unwrap()
            .contains("apply_report.files_created")
    );
    assert!(
        receipt["undo_hint"]
            .as_str()
            .unwrap()
            .contains("Delete backup_root")
    );
}

#[cfg(unix)]
#[test]
fn adopt_rejects_receipt_leaf_symlinks_before_managed_mutation_even_with_force() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for relative in ADOPT_RECEIPT_PATHS {
        for force in [false, true] {
            let temp = tempdir().unwrap();
            let repo = temp.path().join("repo");
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
            fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();

            let receipt_path = repo.join(relative);
            fs::remove_file(&receipt_path).unwrap();
            let outside = temp.path().join("outside");
            fs::create_dir(&outside).unwrap();
            let outside_target = outside.join("receipt.json");
            fs::write(&outside_target, "outside receipt\n").unwrap();
            create_symlink(&outside_target, &receipt_path).unwrap();
            let repo_before = regular_file_tree_snapshot(&repo);
            let outside_before = regular_file_tree_snapshot(&outside);

            let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, force))
                .unwrap_err()
                .to_string();

            assert!(
                error.contains("receipt path"),
                "{relative}/{force}: {error}"
            );
            assert!(
                error.contains("regular file"),
                "{relative}/{force}: {error}"
            );
            assert_eq!(regular_file_tree_snapshot(&repo), repo_before);
            assert_eq!(regular_file_tree_snapshot(&outside), outside_before);
            assert_eq!(
                fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
                "project plan notes\n"
            );
            assert_eq!(
                fs::read_to_string(&outside_target).unwrap(),
                "outside receipt\n"
            );
            assert!(
                fs::symlink_metadata(&receipt_path)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }
}

#[test]
fn adopt_rejects_receipt_leaf_directories_before_managed_mutation() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for relative in ADOPT_RECEIPT_PATHS {
        for force in [false, true] {
            let temp = tempdir().unwrap();
            let repo = temp.path().join("repo");
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
            fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();

            let receipt_path = repo.join(relative);
            fs::remove_file(&receipt_path).unwrap();
            fs::create_dir(&receipt_path).unwrap();
            let repo_before = regular_file_tree_snapshot(&repo);

            let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, force))
                .unwrap_err()
                .to_string();

            assert!(
                error.contains("receipt path"),
                "{relative}/{force}: {error}"
            );
            assert!(
                error.contains("regular file"),
                "{relative}/{force}: {error}"
            );
            assert_eq!(regular_file_tree_snapshot(&repo), repo_before);
            assert_eq!(
                fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
                "project plan notes\n"
            );
            assert!(receipt_path.is_dir());
        }
    }
}

#[cfg(unix)]
#[test]
fn adopt_preview_ignores_unsafe_receipt_leaves_and_remains_read_only() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();

    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    for (index, relative) in ADOPT_RECEIPT_PATHS.into_iter().enumerate() {
        let receipt_path = repo.join(relative);
        fs::remove_file(&receipt_path).unwrap();
        let outside_target = outside.join(format!("receipt-{index}.json"));
        fs::write(&outside_target, format!("outside receipt {index}\n")).unwrap();
        create_symlink(&outside_target, &receipt_path).unwrap();
    }
    let repo_before = regular_file_tree_snapshot(&repo);
    let outside_before = regular_file_tree_snapshot(&outside);
    let mut opts = footprint_adopt_opts(&repo, template.path(), false, false);
    opts.write = false;

    let output = run_adopt(opts).unwrap();

    assert_eq!(output["render_mode"], "preview");
    assert_eq!(regular_file_tree_snapshot(&repo), repo_before);
    assert_eq!(regular_file_tree_snapshot(&outside), outside_before);
    assert_eq!(
        fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
        "project plan notes\n"
    );
}

#[test]
fn adopt_atomically_replaces_regular_receipts_with_equal_contents_and_preserves_permissions() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    let canonical = repo.join(ADOPT_RECEIPT_PATH);
    let legacy = repo.join(LEGACY_ADOPT_RECEIPT_PATH);
    fs::write(&canonical, "stale canonical receipt\n").unwrap();
    fs::write(&legacy, "stale legacy receipt\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&canonical, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&legacy, fs::Permissions::from_mode(0o640)).unwrap();
    }

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    let canonical_bytes = fs::read(&canonical).unwrap();
    let legacy_bytes = fs::read(&legacy).unwrap();
    assert_eq!(legacy_bytes, canonical_bytes);
    assert_ne!(canonical_bytes, b"stale canonical receipt\n");
    serde_json::from_slice::<serde_json::Value>(&canonical_bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            fs::metadata(&canonical).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&legacy).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}

#[cfg(unix)]
#[test]
fn adopt_first_receipt_modes_match_same_parent_fs_write() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let mut expected_modes = Vec::new();
    for (index, relative) in ADOPT_RECEIPT_PATHS.into_iter().enumerate() {
        let parent = repo.join(relative).parent().unwrap().to_path_buf();
        fs::create_dir_all(&parent).unwrap();
        let probe = parent.join(format!("fs-write-mode-probe-{index}"));
        fs::write(&probe, "probe\n").unwrap();
        expected_modes.push(fs::metadata(&probe).unwrap().permissions().mode() & 0o777);
    }

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    for (relative, expected_mode) in ADOPT_RECEIPT_PATHS.into_iter().zip(expected_modes) {
        assert_eq!(
            fs::metadata(repo.join(relative))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            expected_mode,
            "{relative} should use the same create mode and ambient umask as fs::write"
        );
    }
}

#[cfg(unix)]
#[test]
fn adopt_rejects_unsafe_receipt_and_backup_ancestors_before_managed_mutation() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for unsafe_kind in ["receipt", "backup"] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
        fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();
        let outside = temp.path().join(format!("outside-{unsafe_kind}"));
        let unsafe_path = match unsafe_kind {
            "receipt" => {
                fs::rename(repo.join(".agent/.cache/adopt"), &outside).unwrap();
                repo.join(".agent/.cache/adopt")
            }
            "backup" => {
                fs::create_dir(&outside).unwrap();
                repo.join(".agent/.cache/adopt/backups")
            }
            _ => unreachable!(),
        };
        fs::write(outside.join("project-sentinel"), "outside\n").unwrap();
        let outside_before = regular_file_tree_snapshot(&outside);
        create_symlink(&outside, &unsafe_path).unwrap();

        let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true))
            .unwrap_err()
            .to_string();

        assert!(error.contains("is a symlink"), "{unsafe_kind}: {error}");
        assert_eq!(
            fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
            "project plan notes\n"
        );
        assert_eq!(
            fs::read_to_string(outside.join("project-sentinel")).unwrap(),
            "outside\n"
        );
        assert_eq!(regular_file_tree_snapshot(&outside), outside_before);
        assert!(
            fs::symlink_metadata(&unsafe_path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
