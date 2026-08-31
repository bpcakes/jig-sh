use super::*;
use crate::bootstrap::repository_model::RepositoryRenderModel;
use jig_contract::{ActionRunner, ComparisonRequestV1, StrictInventoryReasonV1};
use sha2::{Digest, Sha256};

#[test]
fn adopt_local_git_template_defaults_to_committed_mode() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

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
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("_template_mode = \"committed\""));
    assert!(answers.contains("_template_local_path = "));
    assert!(
        answers.contains(
            &fs::canonicalize(template.path())
                .unwrap()
                .display()
                .to_string()
        )
    );
}

#[test]
fn adopt_local_git_template_rejects_dirty_committed_source() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::write(template.path().join("DIRTY.txt"), "dirty").unwrap();
    write_test_crate_guide(&repo);

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("clean git working tree"));
    assert!(error.contains("Commit or stash template changes"));
}

#[test]
fn update_rejects_legacy_working_tree_template_state() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert(
        TEMPLATE_MODE_KEY.into(),
        TomlValue::String("working-tree".into()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();

    let error = run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported legacy template mode 'working-tree'"));
    assert!(error.contains("committed template source"));
}

#[test]
fn update_committed_mode_rejects_switching_local_template_checkout() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    let other_template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);

    let error = run_update(UpdateOpts {
        path: repo,
        template: Some(other_template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot switch template source paths in-place"));
}

#[test]
fn update_default_committed_mode_uses_clean_local_template_head() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    commit_template_root_guide(
        template.path(),
        "# Default Update Marker\n",
        "template update",
    );

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Default Update Marker"));
}

#[test]
fn update_replaces_jig_block_without_overwriting_custom_root_agents() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.md"),
        "# Existing Agent Guide\n\nCustom repo guidance.\n",
    )
    .unwrap();

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    commit_template_root_guide(template.path(), "Updated Jig Block\n", "template update");

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Custom repo guidance."));
    assert!(root_guide.contains("Updated Jig Block"));
    assert_eq!(
        root_guide
            .matches("<!-- BEGIN JIG MANAGED BLOCK -->")
            .count(),
        1
    );
}

#[test]
fn update_recopy_normalizes_legacy_schema_dump_true_when_sqlx_disabled() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert("schema_dump_enabled".into(), TomlValue::Boolean(true));
    answers.insert(
        "bootstrap_command".into(),
        TomlValue::String("cargo fetch".into()),
    );
    answers.insert(
        "rust_fmt_check_command".into(),
        TomlValue::String("cargo fmt --all -- --check".into()),
    );
    answers.insert(
        "rust_clippy_command".into(),
        TomlValue::String("cargo clippy --workspace --all-targets --locked -- -D warnings".into()),
    );
    answers.insert(
        "rust_test_command".into(),
        TomlValue::String("cargo test --workspace".into()),
    );
    answers.insert(
        "rust_test_locked_command".into(),
        TomlValue::String("cargo test --workspace --locked".into()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(answers.contains("No Cargo.toml found; skipping cargo bootstrap."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo fmt."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo clippy."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo test."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo test-locked."));
    assert!(!answers.contains("tool = \"jig.schema_check\""));
}

#[test]
fn update_recopy_seeds_then_preserves_authored_file_budget_policy() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let policy_path = repo.join(".jig/file-budget.toml");
    let seeded = fs::read_to_string(&policy_path).unwrap();
    assert!(seeded.contains("id = \"rust-source\""), "{seeded}");
    assert!(seeded.contains("\"**/*.rs\""), "{seeded}");
    assert!(!repo.join("scripts/check-rust-file-loc.sh").exists());
    let manifest = fs::read_to_string(repo.join(".agent/jig-managed-paths.json")).unwrap();
    assert!(!manifest.contains(".jig/file-budget.toml"), "{manifest}");

    let authored = format!("# authored policy survives recopy\n{seeded}");
    fs::write(&policy_path, &authored).unwrap();
    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert("default_branch".into(), TomlValue::String("master".into()));
    write_answers_toml(&answers_path, &answers).unwrap();
    run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    assert_eq!(fs::read_to_string(policy_path).unwrap(), authored);
    let answers = RenderAnswers::from_answers_file(&answers_path).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();
    let budget = model
        .actions
        .iter()
        .find(|action| action.target.to_string() == "repo:file-budget")
        .unwrap();
    assert!(matches!(budget.runner, ActionRunner::Native { .. }));
}

#[test]
fn update_recopy_preserves_authored_file_budget_action_alias_and_profile_removals() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_git_worktree();

    for choice in ["action", "alias", "profile"] {
        let repo = temp.path().join(format!("repo-{choice}"));
        write_test_crate_guide(&repo);
        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: Some(template.path().display().to_string()),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force: false,
            write: true,
            minimal: false,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some(format!("Example{choice}")),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        let answers_path = repo.join(".jig.toml");
        let mut answers = read_answers_toml(&answers_path).unwrap();
        let repository = answers
            .get_mut("repository")
            .and_then(TomlValue::as_table_mut)
            .unwrap();
        let is_budget_target = |value: &TomlValue| {
            let Some(target) = value
                .get("target")
                .and_then(TomlValue::as_table)
                .or_else(|| value.as_table())
            else {
                return false;
            };
            target.get("component").and_then(TomlValue::as_str) == Some("repo")
                && target.get("action").and_then(TomlValue::as_str) == Some("file-budget")
        };

        if choice == "action" {
            repository
                .get_mut("actions")
                .and_then(TomlValue::as_array_mut)
                .unwrap()
                .retain(|action| !is_budget_target(action));
        } else if choice == "alias" {
            let action = repository
                .get_mut("actions")
                .and_then(TomlValue::as_array_mut)
                .unwrap()
                .iter_mut()
                .find(|action| is_budget_target(action))
                .unwrap();
            action
                .as_table_mut()
                .unwrap()
                .insert("legacy_aliases".into(), TomlValue::Array(Vec::new()));
        }

        if matches!(choice, "action" | "profile") {
            for profile in repository
                .get_mut("profiles")
                .and_then(TomlValue::as_array_mut)
                .unwrap()
            {
                profile
                    .get_mut("targets")
                    .and_then(TomlValue::as_array_mut)
                    .unwrap()
                    .retain(|target| !is_budget_target(target));
            }
        }
        write_answers_toml(&answers_path, &answers).unwrap();

        run_update(UpdateOpts {
            path: repo.clone(),
            template: None,
            template_mode: None,
            recopy: true,
            launcher_only: false,
            force: true,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let reloaded = RenderAnswers::from_answers_file(&answers_path).unwrap();
        let context = RepoContext::load_from(&repo).unwrap();
        let generated_gates =
            crate::bootstrap::gate_preview::generated_gates(&context, &reloaded).unwrap();
        assert_eq!(
            generated_gates
                .iter()
                .any(|gate| gate == "scripts/jig check repo:file-budget"),
            choice != "action"
        );
        let model = RepositoryRenderModel::from_answers(&reloaded).unwrap();
        let budget = model
            .actions
            .iter()
            .find(|action| action.target.to_string() == "repo:file-budget");
        let policy = fs::read_to_string(repo.join(".github/workflows/repo-policy.yml")).unwrap();
        serde_yaml_ng::from_str::<serde_json::Value>(&policy)
            .unwrap_or_else(|error| panic!("repo policy was invalid YAML: {error}\n{policy}"));
        match choice {
            "action" => {
                assert!(budget.is_none());
                assert!(!policy.contains("scripts/jig check repo:file-budget"));
            }
            "alias" => {
                assert!(budget.unwrap().legacy_aliases.is_empty());
                assert!(policy.contains("scripts/jig check repo:file-budget"));
            }
            "profile" => {
                assert!(budget.is_some());
                assert!(policy.contains("scripts/jig check repo:file-budget"));
                assert!(model.profiles.iter().all(|profile| {
                    profile
                        .targets
                        .iter()
                        .all(|target| target.to_string() != "repo:file-budget")
                }));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn post_deletion_binary_retires_registry_recognized_legacy_asset_after_fresh_receipt() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);

    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    let repository = answers["repository"].as_table_mut().unwrap();
    let legacy =
        crate::bootstrap::repository_model::generated_legacy_rust_file_loc_action().unwrap();
    let actions = repository["actions"].as_array_mut().unwrap();
    let index = actions
        .iter()
        .position(|action| {
            action["target"]["component"].as_str() == Some("repo")
                && action["target"]["action"].as_str() == Some("file-budget")
        })
        .unwrap();
    actions[index] = TomlValue::try_from(&legacy).unwrap();
    actions.sort_by(|left, right| {
        let key = |value: &TomlValue| {
            format!(
                "{}:{}",
                value["target"]["component"].as_str().unwrap(),
                value["target"]["action"].as_str().unwrap()
            )
        };
        key(left).cmp(&key(right))
    });
    for profile in repository["profiles"].as_array_mut().unwrap() {
        let targets = profile["targets"].as_array_mut().unwrap();
        for target in targets.iter_mut() {
            if target["component"].as_str() == Some("repo")
                && target["action"].as_str() == Some("file-budget")
            {
                *target = TomlValue::try_from(&legacy.target).unwrap();
            }
        }
        targets.sort_by_key(|target| {
            format!(
                "{}:{}",
                target["component"].as_str().unwrap(),
                target["action"].as_str().unwrap()
            )
        });
    }
    answers["commands"].as_table_mut().unwrap().insert(
        "repo_rust_file_loc_command".into(),
        TomlValue::String("scripts/check-rust-file-loc.sh main".into()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();

    let checker = repo.join("scripts/check-rust-file-loc.sh");
    let legacy_bytes = b"post-deletion legacy asset fixture\n";
    fs::write(&checker, legacy_bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&checker, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let legacy_digest = format!("{:x}", Sha256::digest(legacy_bytes));
    fs::write(
        repo.join(".agent/jig-legacy-assets.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "assets": [{
                "generation": "test-post-deletion-registry",
                "path": "scripts/check-rust-file-loc.sh",
                "sha256": legacy_digest,
                "file_type": "regular",
                "executable": true
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let mut prior = managed_paths::load_manifest(&repo).unwrap().unwrap();
    prior.insert(PathBuf::from("scripts/check-rust-file-loc.sh"));
    managed_paths::write_manifest(&repo, &prior).unwrap();
    fs::remove_file(repo.join(".jig/file-budget.toml")).unwrap();

    let phase_one = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
    assert_eq!(
        phase_one["legacy_file_budget_migration"]["status"],
        "retained"
    );
    assert_eq!(
        phase_one["legacy_file_budget_migration"]["rerun_command"],
        "scripts/jig check repo:file-budget"
    );
    assert!(checker.is_file());
    assert!(repo.join(".jig/file-budget.toml").is_file());
    let rendered = RenderAnswers::from_answers_file(&answers_path).unwrap();
    let model = RepositoryRenderModel::from_answers(&rendered).unwrap();
    assert!(model.actions.iter().any(|action| {
        action.target.to_string() == "repo:file-budget"
            && matches!(action.runner, ActionRunner::Native { .. })
    }));

    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "phase one"]).unwrap();
    let ctx = RepoContext::load_from(&repo).unwrap();
    let checked = crate::runtime::dispatch(
        &ctx,
        crate::command::RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: vec!["repo:file-budget".into()],
                profile: None,
                affected_base: None,
                comparison: Some(ComparisonRequestV1::StrictInventory {
                    reason: StrictInventoryReasonV1::ExplicitCheck,
                }),
                explain: false,
                fail_fast: false,
                tool: crate::command::ToolRequest::default(),
            },
        )),
    )
    .unwrap();
    assert_eq!(checked["ok"], true, "{checked:#}");

    let phase_two = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
    assert_eq!(
        phase_two["legacy_file_budget_migration"]["status"],
        "retire"
    );
    assert!(!checker.exists());
    assert!(
        !managed_paths::load_manifest(&repo)
            .unwrap()
            .unwrap()
            .contains(Path::new("scripts/check-rust-file-loc.sh"))
    );
    assert!(repo.join(".agent/jig-legacy-assets.json").is_file());
}

#[test]
fn update_refuses_managed_file_changes_without_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let original_mcp = fs::read_to_string(repo.join(".mcp.json")).unwrap();
    fs::write(
        template.path().join("templates/project/.mcp.json.jinja"),
        "{\n  \"changed\": true\n}\n",
    )
    .unwrap();
    git(
        template.path(),
        ["add", "templates/project/.mcp.json.jinja"],
    )
    .unwrap();
    git(template.path(), ["commit", "-m", "template update"]).unwrap();

    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Update would overwrite or remove template-managed paths"));
    assert!(error.contains(".mcp.json"));
    assert_eq!(
        fs::read_to_string(repo.join(".mcp.json")).unwrap(),
        original_mcp
    );

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let mcp = fs::read_to_string(repo.join(".mcp.json")).unwrap();
    assert!(mcp.contains("\"changed\": true"));
}

#[test]
fn update_default_committed_mode_rejects_dirty_local_template() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    fs::write(template.path().join("DIRTY.txt"), "dirty").unwrap();

    let error = run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("clean git working tree"));
}

#[test]
fn update_committed_mode_with_vcs_ref_only_updates_metadata() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    commit_template_root_guide(template.path(), "# Older Marker\n", "older template");

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    let new_ref = commit_template_root_guide(template.path(), "# Newer Marker\n", "newer template");

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: Some(new_ref.clone()),
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Newer Marker"));
    assert!(!root_guide.contains("Older Marker"));

    let answers_path = repo.join(".jig.toml");
    assert_eq!(
        read_optional_answer_string(&answers_path, "_commit")
            .unwrap()
            .as_deref(),
        Some(new_ref.as_str())
    );
}
