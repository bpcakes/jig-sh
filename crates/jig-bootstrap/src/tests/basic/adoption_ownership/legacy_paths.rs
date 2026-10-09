use super::*;

#[test]
fn adoption_previews_legacy_budget_debt_and_refuses_unauthorized_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    init_git_repo_for_test(&repo);
    fs::write(
        repo.join("src/legacy.rs"),
        format!(
            "// agentic-loc-exception: inherited file\n{}",
            "fn legacy_item() {}\n".repeat(900)
        ),
    )
    .unwrap();

    let mut preview_opts = footprint_adopt_opts(&repo, template.path(), false, false);
    preview_opts.write = false;
    let preview = run_adopt(preview_opts).unwrap();
    let budget = &preview["adoption_profile"]["file_budget"];
    assert_eq!(budget["enabled"], true);
    assert_eq!(budget["policy"], "seed_once");
    assert_eq!(budget["candidate_count"], 1);
    assert_eq!(budget["current_debt_file_count"], 1);
    assert_eq!(budget["legacy_marker_count"], 1);
    assert_eq!(budget["human_authorization_required"], true);
    assert_eq!(budget["required_waivers"][0]["path"], "src/legacy.rs");
    assert_eq!(
        budget["required_waivers"][0]["authorization"],
        "human_required"
    );
    assert!(budget["required_waivers"][0]["reason"].is_null());
    assert!(budget["required_waivers"][0]["expires"].is_null());

    let before = regular_file_tree_snapshot(&repo);
    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, false))
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("Adoption requires human-authored file-budget waivers"),
        "{error}"
    );
    assert!(error.contains("No files were changed"), "{error}");
    assert_eq!(regular_file_tree_snapshot(&repo), before);
    assert!(!repo.join(".jig.toml").exists());
    assert!(!repo.join(".jig/file-budget.toml").exists());
}

#[test]
fn legacy_named_project_paths_absent_from_manifest_are_preserved() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let unconditional = ["scripts/check-agent-guides.sh"];
    let conditional = [
        "scripts/add-migration.sh",
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];
    write_project_sentinels(&repo, &unconditional);
    write_project_sentinels(&repo, &conditional);

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_project_sentinels(&repo, &unconditional);
    assert_project_sentinels(&repo, &conditional);
}

#[test]
fn runtime_sqlx_answers_do_not_infer_legacy_path_ownership() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.sqlx_enabled = Some(true);
    full.answers.rust_migration_dir = Some("migrations".into());
    full.answers.schema_dump_enabled = Some(false);
    run_adopt(full).unwrap();
    let sqlx_path = "scripts/add-migration.sh";
    let unrelated = [
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];
    write_project_sentinels(&repo, &[sqlx_path]);
    write_project_sentinels(&repo, &unrelated);

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_project_sentinels(&repo, &[sqlx_path]);
    assert_project_sentinels(&repo, &unrelated);
}

#[test]
fn runtime_feature_answers_do_not_authorize_legacy_retirement() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);
    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.frontend_apps = vec![frontend_app()];
    full.answers.sqlx_enabled = Some(true);
    full.answers.rust_migration_dir = Some("migrations".into());
    full.answers.schema_dump_enabled = Some(true);
    run_adopt(full).unwrap();
    let legacy = [
        "scripts/check-agent-guides.sh",
        "scripts/add-migration.sh",
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];
    write_project_sentinels(&repo, &legacy);

    let mut minimal = footprint_adopt_opts(&repo, template.path(), true, true);
    minimal.answers.sqlx_enabled = None;
    run_adopt(minimal).unwrap();

    assert_project_sentinels(&repo, &legacy);
}
