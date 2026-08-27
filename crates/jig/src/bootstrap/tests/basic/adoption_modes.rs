use super::*;

#[test]
fn full_readoption_preserves_required_on_an_unchanged_generated_evidence_gate() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let verify = config["work"]["gates"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|gate| gate["id"].as_str() == Some("verify"))
        .unwrap()
        .as_table_mut()
        .unwrap();
    verify.insert("required".into(), toml::Value::Boolean(false));
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    let updated = toml::from_str::<toml::Value>(&fs::read_to_string(config_path).unwrap()).unwrap();
    let verify = updated["work"]["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"].as_str() == Some("verify"))
        .unwrap();
    assert_eq!(verify["required"].as_bool(), Some(false));
}

#[test]
fn full_readoption_reconciles_work_config_against_the_new_contract() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);

    let mut initial = footprint_adopt_opts(&repo, template.path(), false, false);
    initial.answers.sqlx_enabled = Some(true);
    initial.answers.schema_dump_enabled = Some(true);
    initial.answers.rust_migration_dir = Some("migrations".into());
    initial.answers.web_package_manager = Some("npm".into());
    initial.answers.frontend_apps = vec![frontend_app()];
    run_adopt(initial).unwrap();

    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let work = config["work"].as_table_mut().unwrap();
    work.insert(
        "checks".into(),
        toml::Value::Array(
            [
                "jig.sqlx_check",
                "jig.schema_check",
                "jig.typescript_lint",
                "jig.fmt_check",
            ]
            .into_iter()
            .map(|tool| toml::Value::String(tool.into()))
            .collect(),
        ),
    );
    let gates = work["gates"].as_array_mut().unwrap();
    for gate in gates.iter_mut() {
        let gate = gate.as_table_mut().unwrap();
        if gate["id"].as_str().unwrap() == "verify" {
            gate.insert("profile".into(), toml::Value::String("outdated".into()));
            gate.insert("required".into(), toml::Value::Boolean(false));
        }
    }
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-fmt".into())),
        ("kind".into(), toml::Value::String("check".into())),
        ("tool".into(), toml::Value::String("jig.fmt_check".into())),
        ("required".into(), toml::Value::Boolean(false)),
    ])));
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-review".into())),
        ("kind".into(), toml::Value::String("codex_review".into())),
        ("skill".into(), toml::Value::String("cc:review".into())),
        ("fail_on".into(), toml::Value::String("warning".into())),
        ("scope".into(), toml::Value::String("uncommitted".into())),
        ("model".into(), toml::Value::String("gpt-5".into())),
    ])));
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-evidence".into())),
        ("kind".into(), toml::Value::String("evidence".into())),
        ("profile".into(), toml::Value::String("verify".into())),
        ("required".into(), toml::Value::Boolean(false)),
    ])));
    work.insert(
        "refinements".into(),
        toml::Value::Array(vec![toml::Value::Table(toml::Table::from_iter([
            (
                "id".into(),
                toml::Value::String("project-refinement".into()),
            ),
            (
                "skill".into(),
                toml::Value::String("jig-rust:rust-simplify".into()),
            ),
            ("mode".into(), toml::Value::String("write".into())),
            ("model".into(), toml::Value::String("gpt-5".into())),
        ]))]),
    );
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    crate::context::RepoContext::load_from(&repo).unwrap();
    fs::remove_file(repo.join("apps/web/package.json")).unwrap();
    fs::remove_file(repo.join("package.json")).unwrap();
    fs::remove_file(repo.join("package-lock.json")).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    let checks = config["work"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(checks, vec!["jig.fmt_check"]);

    let gates = config["work"]["gates"].as_array().unwrap();
    let gate = |id: &str| {
        gates
            .iter()
            .find(|gate| gate["id"].as_str() == Some(id))
            .unwrap()
    };
    assert_eq!(gate("verify")["kind"].as_str(), Some("evidence"));
    assert_eq!(gate("verify")["profile"].as_str(), Some("verify"));
    assert_eq!(
        gate("verify")
            .as_table()
            .unwrap()
            .get("required")
            .and_then(toml::Value::as_bool),
        None
    );
    assert_eq!(gate("project-fmt")["tool"].as_str(), Some("jig.fmt_check"));
    assert_eq!(gate("project-fmt")["required"].as_bool(), Some(false));
    assert_eq!(
        gate("project-review")["kind"].as_str(),
        Some("codex_review")
    );
    assert_eq!(gate("project-evidence")["kind"].as_str(), Some("evidence"));
    assert_eq!(gate("project-evidence")["required"].as_bool(), Some(false));
    assert_eq!(
        config["work"]["refinements"][0]["id"].as_str(),
        Some("project-refinement")
    );
    for stale_id in [
        "sqlx",
        "schema",
        "schema-dump",
        "typescript-lint",
        "typescript-typecheck",
        "typescript-build",
        "typescript-coverage",
    ] {
        assert!(
            gates
                .iter()
                .all(|gate| gate["id"].as_str() != Some(stale_id))
        );
    }
    let ids = gates
        .iter()
        .map(|gate| gate["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), gates.len());

    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(crate::policy::contract_check(&ctx).exit_status, 0);
}

#[test]
fn full_readoption_drops_argument_taking_tools_from_preserved_work_config() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let mut initial = footprint_adopt_opts(&repo, template.path(), false, false);
    initial.answers.sqlx_enabled = Some(true);
    initial.answers.rust_migration_dir = Some("migrations".into());
    run_adopt(initial).unwrap();

    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let work = config["work"].as_table_mut().unwrap();
    work.insert(
        "checks".into(),
        toml::Value::Array(
            ["jig.migration_add", "jig.fmt_check"]
                .into_iter()
                .map(|tool| toml::Value::String(tool.into()))
                .collect(),
        ),
    );
    let gates = work["gates"].as_array_mut().unwrap();
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-migration".into())),
        ("kind".into(), toml::Value::String("check".into())),
        (
            "tool".into(),
            toml::Value::String("jig.migration_add".into()),
        ),
    ])));
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-fmt".into())),
        ("kind".into(), toml::Value::String("check".into())),
        ("tool".into(), toml::Value::String("jig.fmt_check".into())),
    ])));
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-review".into())),
        ("kind".into(), toml::Value::String("codex_review".into())),
        ("skill".into(), toml::Value::String("cc:review".into())),
    ])));
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let mut readopt = footprint_adopt_opts(&repo, template.path(), false, true);
    readopt.answers.sqlx_enabled = Some(true);
    readopt.answers.rust_migration_dir = Some("migrations".into());
    run_adopt(readopt).unwrap();

    let config = toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let checks = config["work"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(checks, vec!["jig.fmt_check"]);
    let gates = config["work"]["gates"].as_array().unwrap();
    assert!(
        gates
            .iter()
            .all(|gate| gate["id"].as_str() != Some("project-migration"))
    );
    assert!(
        gates
            .iter()
            .any(|gate| gate["id"].as_str() == Some("project-fmt"))
    );
    assert!(
        gates
            .iter()
            .any(|gate| gate["id"].as_str() == Some("project-review"))
    );
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(r#""name": "jig.migration_add""#));
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(crate::policy::contract_check(&ctx).exit_status, 0);
}

#[test]
fn staging_rejects_generated_evidence_gate_with_conflicting_selectors() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let config_template = template.path().join("templates/project/.jig.toml.jinja");
    let original_config = fs::read_to_string(&config_template).unwrap();
    assert!(original_config.contains("kind = \"evidence\""));
    let config = original_config.replacen(
        "kind = \"evidence\"",
        "kind = \"evidence\"\ntarget = \"api:test\"",
        1,
    );
    fs::write(&config_template, config).unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let mut opts = footprint_adopt_opts(&repo, template.path(), false, false);
    opts.answers.sqlx_enabled = Some(true);
    opts.answers.rust_migration_dir = Some("migrations".into());

    let error = format!("{:#}", run_adopt(opts).unwrap_err());

    assert!(
        error.contains("requires exactly one of target or profile"),
        "{error}"
    );
    assert!(!repo.join(".jig.toml").exists());
}

#[test]
fn minimal_to_full_uses_existing_answers_and_preserves_runtime_tables() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let mut minimal = footprint_adopt_opts(&repo, template.path(), true, false);
    minimal.answers.default_branch = Some("release".into());
    run_adopt(minimal).unwrap();
    add_project_runtime_tables(&repo);

    let mut full = footprint_adopt_opts(&repo, template.path(), false, true);
    full.answers.repo_name = None;
    full.answers.ci_github_runner = Some("macos-14".into());
    full.answers.sqlx_enabled = Some(true);
    full.answers.rust_migration_dir = Some("db/migrations".into());
    full.answers.rust_sqlx_metadata_dir = Some("db/sqlx-cache".into());
    full.answers.sqlx_check_command = Some("scripts/check-custom-sqlx.sh".into());
    run_adopt(full).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["repo_name"].as_str(), Some("demo"));
    assert_eq!(config["default_branch"].as_str(), Some("release"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("macos-14"));
    assert_eq!(config["sqlx_enabled"].as_bool(), Some(true));
    assert_eq!(config["rust_migration_dir"].as_str(), Some("db/migrations"));
    assert_eq!(
        config["rust_sqlx_metadata_dir"].as_str(),
        Some("db/sqlx-cache")
    );
    assert_eq!(
        config["sqlx_check_command"].as_str(),
        Some("scripts/check-custom-sqlx.sh")
    );
    assert_eq!(config["harness_footprint"].as_str(), Some("full"));
    assert_project_runtime_tables(&config);

    let workflow = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    let workflow = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    for job in ["fmt", "clippy", "test"] {
        assert_eq!(workflow["jobs"][job]["runs-on"], "macos-14");
    }
    for event in ["pull_request", "push"] {
        let paths = workflow["on"][event]["paths"].as_array().unwrap();
        assert!(paths.iter().any(|path| path == "db/migrations/**"));
        assert!(paths.iter().any(|path| path == "db/sqlx-cache/**"));
    }
    for job in ["clippy", "test"] {
        assert_eq!(
            workflow["jobs"][job]["env"]["SQLX_OFFLINE_DIR"],
            "${{ github.workspace }}/db/sqlx-cache"
        );
    }
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(r#""name": "jig.sqlx_check""#));
    assert!(contract.contains(r#""name": "jig.migration_add""#));
}

#[test]
fn minimal_to_full_preserves_a_complete_authored_repository_model() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("services/api")).unwrap();
    fs::create_dir_all(repo.join("services/worker")).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let authored = authored_mixed_repository_config();
    config["commands"] = authored["commands"].clone();
    config["repository"] = authored["repository"].clone();
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    let updated =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let targets = updated["repository"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| {
            format!(
                "{}:{}",
                action["target"]["component"].as_str().unwrap(),
                action["target"]["action"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        [
            "repo:contract",
            "repo:bootstrap",
            "api:verify-custom",
            "worker:verify-custom"
        ]
    );
    assert_eq!(
        updated["repository"]["profiles"][0]["targets"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        updated["commands"]["api_verify_command"].as_str(),
        Some("go test ./...")
    );
    assert_eq!(
        updated["commands"]["worker_verify_command"].as_str(),
        Some("cargo test -p worker")
    );
}

#[test]
fn minimal_to_full_uses_explicit_answers_file_and_preserves_runtime_tables() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    add_project_runtime_tables(&repo);
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
default_branch = "file-branch"
sqlx_enabled = false
rust_test_command = "cargo nextest run"
"#,
    )
    .unwrap();

    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers = AnswerOpts {
        answers_file: Some(answers_file),
        ci_github_runner: Some("ubuntu-24.04".into()),
        ..AnswerOpts::default()
    };
    run_adopt(full).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["repo_name"].as_str(), Some("from-file"));
    assert_eq!(config["default_branch"].as_str(), Some("file-branch"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("ubuntu-24.04"));
    assert_eq!(
        config["commands"]["api_test_command"].as_str(),
        Some("cargo nextest run")
    );
    assert_eq!(config["harness_footprint"].as_str(), Some("full"));
    assert_project_runtime_tables(&config);
    let workflow = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    let workflow = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    assert_eq!(workflow["jobs"]["test"]["runs-on"], "ubuntu-24.04");
    for event in ["pull_request", "push"] {
        let paths = workflow["on"][event]["paths"].as_array().unwrap();
        assert!(!paths.iter().any(|path| path == "migrations/**"));
        assert!(!paths.iter().any(|path| path == ".sqlx/**"));
    }
    assert!(workflow["jobs"]["clippy"]["env"].is_null());
    assert!(workflow["jobs"]["test"]["env"].is_null());
}

#[test]
fn full_to_minimal_seeds_existing_answers_before_cli_overrides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.default_branch = Some("release".into());
    full.answers.ci_github_runner = Some("macos-14".into());
    full.answers.rust_test_command = Some("cargo nextest run".into());
    full.answers.dev_apps = vec![DevApp {
        name: "api".into(),
        dir: Some("crates/api".into()),
        kind: "env-port".into(),
        command: Some("cargo run -p api".into()),
        argv: Vec::new(),
        port: Some(8080),
        host: None,
        proxy: true,
    }];
    run_adopt(full).unwrap();
    let initial_config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(
        initial_config["commands"]["api_test_command"].as_str(),
        Some("cargo nextest run")
    );
    add_project_runtime_tables(&repo);
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["vault"]["allow_global"] = toml::Value::Boolean(true);
    config["agent_tooling"]["codex"]["marketplaces"][0]["source"] =
        toml::Value::String("example/custom-skills".into());
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let mut minimal = footprint_adopt_opts(&repo, template.path(), true, true);
    minimal.answers.ci_github_runner = Some("ubuntu-24.04".into());
    run_adopt(minimal).unwrap();

    let config = toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(config["default_branch"].as_str(), Some("release"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("ubuntu-24.04"));
    assert_eq!(
        config["commands"]["api_test_command"].as_str(),
        Some("cargo nextest run")
    );
    assert_eq!(config["dev"]["apps"][0]["name"].as_str(), Some("api"));
    assert_eq!(config["vault"]["allow_global"].as_bool(), Some(true));
    assert_eq!(
        config["agent_tooling"]["codex"]["marketplaces"][0]["source"].as_str(),
        Some("example/custom-skills")
    );
    assert_project_runtime_tables(&config);
    assert_eq!(config["harness_footprint"].as_str(), Some("minimal"));
}

#[test]
fn full_to_minimal_keeps_explicit_answers_file_authoritative() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.default_branch = Some("release".into());
    run_adopt(full).unwrap();
    let answers_file = temp.path().join("minimal-answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
default_branch = "file-branch"
ci_github_runner = "macos-14"
sqlx_enabled = false
"#,
    )
    .unwrap();

    let mut minimal = footprint_adopt_opts(&repo, template.path(), true, true);
    minimal.answers = AnswerOpts {
        answers_file: Some(answers_file),
        ci_github_runner: Some("ubuntu-24.04".into()),
        ..AnswerOpts::default()
    };
    run_adopt(minimal).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["repo_name"].as_str(), Some("from-file"));
    assert_eq!(config["default_branch"].as_str(), Some("file-branch"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("ubuntu-24.04"));
    assert_eq!(config["harness_footprint"].as_str(), Some("minimal"));
}

#[test]
fn minimal_to_full_adoption_still_rejects_unrelated_managed_conflicts() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, false))
        .unwrap_err()
        .to_string();

    assert!(error.contains(".agent/PLANS.md"));
    assert_eq!(
        fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
        "project plan notes\n"
    );
    assert!(
        fs::read_to_string(repo.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"minimal\"")
    );
}

include!("adoption_modes_parts/part_02.rs");

mod vault_policy;
