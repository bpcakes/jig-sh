use super::*;

#[test]
fn missing_manifest_blocks_update_and_explicit_adopt_establishes_ownership() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    add_project_runtime_tables(&repo);
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["web_package_manager"] = toml::Value::String("npm".into());
    config["dev"].as_table_mut().unwrap().insert(
        "apps".into(),
        toml::Value::Array(vec![toml::Value::Table(toml::Table::from_iter([
            ("name".into(), toml::Value::String("api".into())),
            ("kind".into(), toml::Value::String("env-port".into())),
            (
                "command".into(),
                toml::Value::String("cargo run -p api".into()),
            ),
        ]))]),
    );
    config["agent_tooling"]["codex"]["marketplaces"][0]["source"] =
        toml::Value::String("example/custom-skills".into());
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    fs::remove_file(repo.join(managed_paths::MANIFEST_PATH)).unwrap();
    let project_owned = ["scripts/check-agent-guides.sh", "scripts/add-migration.sh"];
    write_project_sentinels(&repo, &project_owned);

    let error = run_update(update_opts(&repo, template.path(), false))
        .unwrap_err()
        .to_string();
    assert!(error.contains(managed_paths::MANIFEST_PATH), "{error}");
    assert!(error.contains("jig adopt . --write"), "{error}");

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    assert!(repo.join(managed_paths::MANIFEST_PATH).is_file());
    assert_project_sentinels(&repo, &project_owned);
    assert!(
        output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| { !project_owned.contains(&path.as_str()) })
    );
    let established =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(established["web_package_manager"].as_str(), Some("npm"));
    assert_eq!(established["dev"]["apps"][0]["name"].as_str(), Some("api"));
    assert_eq!(
        established["agent_tooling"]["codex"]["marketplaces"][0]["source"].as_str(),
        Some("example/custom-skills")
    );
    assert_project_runtime_tables(&established);
    run_update(update_opts(&repo, template.path(), false)).unwrap();
}

#[test]
fn missing_manifest_blocks_full_to_minimal_until_full_ownership_is_established() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::remove_file(repo.join(managed_paths::MANIFEST_PATH)).unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true))
        .unwrap_err()
        .to_string();
    assert!(error.contains("without --minimal"), "{error}");
    assert!(repo.join("scripts/jig").is_file());
    assert!(
        fs::read_to_string(repo.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"full\"")
    );

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();
    assert!(!repo.join("scripts/jig").exists());
}

#[test]
fn invalid_manifest_blocks_forced_adoption_without_changes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let sentinel = fs::read(repo.join("scripts/jig")).unwrap();
    fs::write(
        repo.join(managed_paths::MANIFEST_PATH),
        r#"{"version":1,"paths":["../outside",".agent/jig-managed-paths.json"]}"#,
    )
    .unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true))
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("Invalid Jig managed-path manifest"),
        "{error}"
    );
    assert_eq!(fs::read(repo.join("scripts/jig")).unwrap(), sentinel);
}

#[test]
fn tampered_manifest_cannot_make_update_or_adopt_remove_project_directory() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for mode in [
        "update",
        "update-force",
        "adopt-preview",
        "adopt-write",
        "adopt-force",
    ] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

        fs::create_dir(repo.join("project-directory")).unwrap();
        fs::write(
            repo.join("project-directory/project-sentinel"),
            "project metadata\n",
        )
        .unwrap();
        fs::write(repo.join(".agent/state/.gitkeep"), "project notes\n").unwrap();
        let existing_backup = repo.join(".agent/.cache/adopt/backups/existing");
        fs::create_dir_all(&existing_backup).unwrap();
        fs::write(existing_backup.join("project-sentinel"), "backup\n").unwrap();
        add_managed_manifest_path(&repo, "project-directory");

        let manifest_before = fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap();
        let canonical_receipt_before = fs::read(repo.join(ADOPT_RECEIPT_PATH)).unwrap();
        let legacy_receipt_before = fs::read(repo.join(LEGACY_ADOPT_RECEIPT_PATH)).unwrap();
        let repo_before = regular_file_tree_snapshot(&repo);

        let error = match mode {
            "update" => run_update(update_opts(&repo, template.path(), false)).unwrap_err(),
            "update-force" => run_update(update_opts(&repo, template.path(), true)).unwrap_err(),
            "adopt-preview" => {
                let mut opts = footprint_adopt_opts(&repo, template.path(), false, false);
                opts.write = false;
                run_adopt(opts).unwrap_err()
            }
            "adopt-write" => {
                run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap_err()
            }
            "adopt-force" => {
                run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap_err()
            }
            _ => unreachable!(),
        }
        .to_string();

        assert!(error.contains("destination leaf"), "{mode}: {error}");
        assert!(error.contains("project-directory"), "{mode}: {error}");
        assert!(error.contains("is a directory"), "{mode}: {error}");
        assert!(
            !error.contains("Re-run with --force") && !error.contains("re-run with --force"),
            "{mode}: structural errors must not suggest force: {error}"
        );
        assert_eq!(regular_file_tree_snapshot(&repo), repo_before, "{mode}");
        assert_eq!(
            fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap(),
            manifest_before,
            "{mode}: manifest changed"
        );
        assert_eq!(
            fs::read_to_string(repo.join("project-directory/project-sentinel")).unwrap(),
            "project metadata\n",
            "{mode}: project directory changed"
        );
        assert_eq!(
            fs::read(repo.join(ADOPT_RECEIPT_PATH)).unwrap(),
            canonical_receipt_before,
            "{mode}: canonical receipt changed"
        );
        assert_eq!(
            fs::read(repo.join(LEGACY_ADOPT_RECEIPT_PATH)).unwrap(),
            legacy_receipt_before,
            "{mode}: legacy receipt changed"
        );
        assert_eq!(
            fs::read_to_string(existing_backup.join("project-sentinel")).unwrap(),
            "backup\n",
            "{mode}: existing backup changed"
        );
        assert_eq!(
            fs::read_to_string(repo.join(".agent/state/.gitkeep")).unwrap(),
            "project notes\n",
            "{mode}: an earlier managed path changed"
        );
    }
}

#[test]
fn tampered_manifest_cannot_manage_linked_worktree_git_file() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for alias in [".git", ".g\u{200c}it/config"] {
        for mode in [
            "update",
            "update-force",
            "adopt-preview",
            "adopt-write",
            "adopt-force",
        ] {
            let temp = tempdir().unwrap();
            let main = temp.path().join("main");
            fs::create_dir_all(&main).unwrap();
            init_git_repo_for_test(&main);
            git(&main, ["commit", "--allow-empty", "-m", "fixture"]).unwrap();
            let repo = temp.path().join("repo");
            git(
                &main,
                [
                    "worktree",
                    "add",
                    "--quiet",
                    "-b",
                    "fixture-worktree",
                    repo.to_str().unwrap(),
                ],
            )
            .unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

            assert!(repo.join(".git").is_file());
            let git_metadata_before = fs::read_to_string(repo.join(".git")).unwrap();
            fs::write(repo.join(".agent/state/.gitkeep"), "project notes\n").unwrap();
            let existing_backup = repo.join(".agent/.cache/adopt/backups/existing");
            fs::create_dir_all(&existing_backup).unwrap();
            fs::write(existing_backup.join("project-sentinel"), "backup\n").unwrap();
            add_managed_manifest_path(&repo, alias);

            let repo_before = regular_file_tree_snapshot(&repo);

            let error = match mode {
                "update" => run_update(update_opts(&repo, template.path(), false)).unwrap_err(),
                "update-force" => {
                    run_update(update_opts(&repo, template.path(), true)).unwrap_err()
                }
                "adopt-preview" => {
                    let mut opts = footprint_adopt_opts(&repo, template.path(), false, false);
                    opts.write = false;
                    run_adopt(opts).unwrap_err()
                }
                "adopt-write" => {
                    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false))
                        .unwrap_err()
                }
                "adopt-force" => {
                    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true))
                        .unwrap_err()
                }
                _ => unreachable!(),
            }
            .to_string();

            assert!(
                error.contains("reserved Git metadata component"),
                "{alias}/{mode}: {error}"
            );
            assert!(error.contains(".git"), "{alias}/{mode}: {error}");
            assert!(
                !error.to_ascii_lowercase().contains("--force"),
                "{alias}/{mode}: reserved-path errors must not suggest force: {error}"
            );
            assert_eq!(
                regular_file_tree_snapshot(&repo),
                repo_before,
                "{alias}/{mode}"
            );
            assert_eq!(
                fs::read_to_string(repo.join(".git")).unwrap(),
                git_metadata_before,
                "{alias}/{mode}: linked-worktree metadata changed"
            );
            assert_eq!(
                fs::read_to_string(existing_backup.join("project-sentinel")).unwrap(),
                "backup\n",
                "{alias}/{mode}: existing backup changed"
            );
            assert_eq!(
                fs::read_to_string(repo.join(".agent/state/.gitkeep")).unwrap(),
                "project notes\n",
                "{alias}/{mode}: an earlier managed path changed"
            );
        }
    }
}

#[test]
fn custom_template_cannot_stage_reserved_git_metadata_path() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let custom_template = template.path().join("templates/project/.git/config.jinja");
    fs::create_dir_all(custom_template.parent().unwrap()).unwrap();
    fs::write(&custom_template, "managed git config\n").unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo_for_test(&repo);
    fs::write(repo.join("project-sentinel"), "project-owned\n").unwrap();
    let repo_before = regular_file_tree_snapshot(&repo);

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true))
        .unwrap_err()
        .to_string();

    assert!(error.contains("reserved Git metadata component"), "{error}");
    assert!(error.contains(".git/config"), "{error}");
    assert!(!error.to_ascii_lowercase().contains("--force"), "{error}");
    assert_eq!(regular_file_tree_snapshot(&repo), repo_before);
    assert_eq!(
        fs::read_to_string(repo.join("project-sentinel")).unwrap(),
        "project-owned\n"
    );
}

#[test]
fn manifest_retires_custom_template_paths_removed_by_a_later_render() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let custom_template = template
        .path()
        .join("templates/project/custom-policy.txt.jinja");
    fs::write(&custom_template, "managed custom policy\n").unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    assert!(repo.join("custom-policy.txt").is_file());
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .any(|path| path == "custom-policy.txt")
    );
    fs::remove_file(custom_template).unwrap();

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    assert!(!repo.join("custom-policy.txt").exists());
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| path != "custom-policy.txt")
    );
    assert!(
        output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "custom-policy.txt")
    );
}
