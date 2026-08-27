use super::*;

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
        match gate["id"].as_str().unwrap() {
            "jig-contract" => {
                gate.insert("id".into(), toml::Value::String("contract".into()));
                gate.insert("tool".into(), toml::Value::String("jig.fmt_check".into()));
                gate.insert("required".into(), toml::Value::Boolean(false));
            }
            "rust-tests" => {
                gate.insert("required".into(), toml::Value::Boolean(false));
            }
            _ => {}
        }
    }
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-fmt".into())),
        ("kind".into(), toml::Value::String("check".into())),
        ("tool".into(), toml::Value::String("jig.fmt_check".into())),
        ("required".into(), toml::Value::Boolean(false)),
    ])));
    gates.push(toml::Value::Table(toml::Table::from_iter([
        (
            "id".into(),
            toml::Value::String("project-backend-tests".into()),
        ),
        ("kind".into(), toml::Value::String("check".into())),
        ("tool".into(), toml::Value::String("jig.test".into())),
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
    let contract_path = repo.join(".agent/jig-contract.json");
    let mut contract: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["contract_version"] = serde_json::json!(4);
    fs::write(
        &contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
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
    assert_eq!(
        gate("jig-contract")["tool"].as_str(),
        Some("jig.contract_check")
    );
    assert_eq!(
        gate("jig-contract")
            .as_table()
            .unwrap()
            .get("required")
            .and_then(toml::Value::as_bool),
        None
    );
    assert_eq!(gate("contract")["tool"].as_str(), Some("jig.fmt_check"));
    assert_eq!(gate("contract")["required"].as_bool(), Some(false));
    assert_eq!(gate("rust-tests")["tool"].as_str(), Some("jig.test"));
    assert_eq!(gate("rust-tests")["required"].as_bool(), Some(false));
    assert_eq!(gate("project-fmt")["tool"].as_str(), Some("jig.fmt_check"));
    assert_eq!(gate("project-fmt")["required"].as_bool(), Some(false));
    assert_eq!(
        gate("project-backend-tests")["tool"].as_str(),
        Some("jig.test")
    );
    assert_eq!(
        gate("project-backend-tests")["required"].as_bool(),
        Some(false)
    );
    assert_eq!(
        gate("project-review")["kind"].as_str(),
        Some("codex_review")
    );
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
fn staging_rejects_generated_work_gate_that_requires_an_argument() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let config_template = template.path().join("templates/project/.jig.toml.jinja");
    let config = fs::read_to_string(&config_template).unwrap().replacen(
        "tool = \"jig.contract_check\"",
        "tool = \"jig.migration_add\"",
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
        error
            .contains("Configured work check or gate tool requires an argument: jig.migration_add"),
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
        config["rust_test_command"].as_str(),
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
fn readoption_rejects_generated_gate_id_collision_with_another_tool() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let jig_contract = config["work"]["gates"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|gate| gate["id"].as_str() == Some("jig-contract"))
        .unwrap()
        .as_table_mut()
        .unwrap();
    jig_contract.insert("tool".into(), toml::Value::String("jig.fmt_check".into()));
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    let before = fs::read(&config_path).unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Cannot reconcile generated work gate 'jig-contract'"));
    assert!(error.contains("jig.fmt_check"));
    assert_eq!(fs::read(config_path).unwrap(), before);
}

#[test]
fn readoption_prefers_an_exact_generated_gate_over_its_legacy_alias() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let gates = config["work"]["gates"].as_array_mut().unwrap();
    let exact = gates
        .iter_mut()
        .find(|gate| gate["id"].as_str() == Some("jig-contract"))
        .unwrap()
        .as_table_mut()
        .unwrap();
    exact.insert("required".into(), toml::Value::Boolean(true));
    exact.insert("reuse".into(), toml::Value::Boolean(false));
    let exact_rust_tests = gates
        .iter_mut()
        .find(|gate| gate["id"].as_str() == Some("rust-tests"))
        .unwrap()
        .as_table_mut()
        .unwrap();
    exact_rust_tests.insert("required".into(), toml::Value::Boolean(true));
    exact_rust_tests.insert("reuse".into(), toml::Value::Boolean(false));
    gates.insert(
        0,
        toml::Value::Table(toml::Table::from_iter([
            ("id".into(), toml::Value::String("contract".into())),
            ("kind".into(), toml::Value::String("check".into())),
            (
                "tool".into(),
                toml::Value::String("jig.contract_check".into()),
            ),
            ("required".into(), toml::Value::Boolean(false)),
            ("reuse".into(), toml::Value::Boolean(true)),
        ])),
    );
    gates.insert(
        1,
        toml::Value::Table(toml::Table::from_iter([
            ("id".into(), toml::Value::String("tests".into())),
            ("kind".into(), toml::Value::String("check".into())),
            ("tool".into(), toml::Value::String("jig.test".into())),
            ("required".into(), toml::Value::Boolean(false)),
            ("reuse".into(), toml::Value::Boolean(true)),
        ])),
    );
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    let config = toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let gates = config["work"]["gates"].as_array().unwrap();
    assert_eq!(
        gates
            .iter()
            .filter(|gate| gate["id"].as_str() == Some("jig-contract"))
            .count(),
        1
    );
    let exact = gates
        .iter()
        .find(|gate| gate["id"].as_str() == Some("jig-contract"))
        .unwrap();
    assert_eq!(exact["required"].as_bool(), Some(true));
    assert_eq!(exact["reuse"].as_bool(), Some(false));
    assert!(
        gates
            .iter()
            .all(|gate| gate["id"].as_str() != Some("contract"))
    );
    let exact_rust_tests = gates
        .iter()
        .find(|gate| gate["id"].as_str() == Some("rust-tests"))
        .unwrap();
    assert_eq!(exact_rust_tests["required"].as_bool(), Some(true));
    assert_eq!(exact_rust_tests["reuse"].as_bool(), Some(false));
    assert!(
        gates
            .iter()
            .all(|gate| gate["id"].as_str() != Some("tests"))
    );
}

#[test]
fn readoption_refreshes_generated_gate_scopes_but_preserves_project_policy() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let rust_tests = config["work"]["gates"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|gate| gate["id"].as_str() == Some("rust-tests"))
        .unwrap()
        .as_table_mut()
        .unwrap();
    rust_tests.insert(
        "paths".into(),
        toml::Value::Array(vec![toml::Value::String("project-only/**".into())]),
    );
    rust_tests.insert(
        "paths_ignore".into(),
        toml::Value::Array(vec![toml::Value::String(
            "project-only/generated/**".into(),
        )]),
    );
    rust_tests.insert("required".into(), toml::Value::Boolean(false));
    rust_tests.insert("reuse".into(), toml::Value::Boolean(true));
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    let config = toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let rust_tests = config["work"]["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"].as_str() == Some("rust-tests"))
        .unwrap();
    let paths = rust_tests["paths"].as_array().unwrap();
    assert!(paths.iter().any(|path| path.as_str() == Some("crates/**")));
    assert!(
        !paths
            .iter()
            .any(|path| path.as_str() == Some("project-only/**"))
    );
    assert!(rust_tests.as_table().unwrap().get("paths_ignore").is_none());
    assert_eq!(rust_tests["required"].as_bool(), Some(false));
    assert_eq!(rust_tests["reuse"].as_bool(), Some(true));
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
        config["rust_test_command"].as_str(),
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

#[test]
fn forced_full_to_minimal_adoption_retires_full_harness_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    add_project_runtime_tables(&repo);
    let full_manifest = managed_manifest_paths(&repo)
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(repo.join(".mcp.json").is_file());
    assert!(repo.join("scripts/jig").is_file());
    assert!(repo.join(".github/workflows/rust-tests.yml").is_file());

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();
    let minimal_manifest = managed_manifest_paths(&repo)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let expected_retirements = full_manifest
        .difference(&minimal_manifest)
        .cloned()
        .collect::<Vec<_>>();
    let reported_retirements = output["adoption_profile"]["retired_managed_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(reported_retirements, expected_retirements);
    assert_eq!(
        reported_retirements,
        output["render_report"]["retired_managed_paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );

    assert_eq!(output["harness_footprint"], "minimal");
    assert!(!repo.join(".mcp.json").exists());
    assert!(!repo.join("scripts/jig").exists());
    assert!(!repo.join(".github/workflows/rust-tests.yml").exists());
    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert_eq!(root_guide, "# Repository Guidelines\n");
    assert!(
        output["render_report"]["files_removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".mcp.json")
    );
    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_project_runtime_tables(&config);
    crate::context::RepoContext::load_from(&repo).unwrap();
}

#[cfg(unix)]
#[test]
fn minimal_adoption_rejects_managed_symlink_ancestors_in_preview_write_and_force_modes() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for ancestor in [".agent", ".github", "scripts"] {
        for (label, write, force) in [
            ("preview", false, false),
            ("write", true, false),
            ("force", true, true),
        ] {
            let temp = tempdir().unwrap();
            let repo = temp.path().join("repo");
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
            let config_before = fs::read(repo.join(".jig.toml")).unwrap();
            let outside = temp.path().join(format!(
                "outside-{}-{label}",
                ancestor.trim_start_matches('.')
            ));
            fs::rename(repo.join(ancestor), &outside).unwrap();
            fs::write(outside.join("project-sentinel"), "outside\n").unwrap();
            let protected_relative = match ancestor {
                ".agent" => managed_paths::MANIFEST_PATH
                    .strip_prefix(".agent/")
                    .unwrap(),
                ".github" => "workflows/rust-tests.yml",
                "scripts" => "jig",
                _ => unreachable!(),
            };
            let protected_before = fs::read(outside.join(protected_relative)).unwrap();
            let outside_before = regular_file_tree_snapshot(&outside);
            create_symlink(&outside, &repo.join(ancestor)).unwrap();
            let mut opts = footprint_adopt_opts(&repo, template.path(), true, force);
            opts.write = write;

            let error = run_adopt(opts).unwrap_err().to_string();

            assert!(
                error.contains("is a symlink"),
                "{ancestor}/{label}: {error}"
            );
            assert_eq!(fs::read(repo.join(".jig.toml")).unwrap(), config_before);
            assert_eq!(
                fs::read(outside.join(protected_relative)).unwrap(),
                protected_before,
                "{ancestor}/{label} changed an outside managed path"
            );
            assert_eq!(
                fs::read_to_string(outside.join("project-sentinel")).unwrap(),
                "outside\n"
            );
            assert_eq!(regular_file_tree_snapshot(&outside), outside_before);
            assert!(
                fs::symlink_metadata(repo.join(ancestor))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }
}

#[test]
fn full_to_minimal_removes_only_the_root_agents_managed_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("AGENTS.md"),
        "# Project Guide\n\nKeep this project-owned guidance.\n",
    )
    .unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        "# Project Guide\n\nKeep this project-owned guidance.\n"
    );
}

#[test]
fn full_to_minimal_preserves_root_agents_bytes_around_the_managed_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let rendered = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    let spec = managed_paths::managed_block_spec(Path::new("AGENTS.md")).unwrap();
    let start = rendered.find(spec.begin).unwrap();
    let end = rendered.find(spec.end).unwrap() + spec.end.len();
    let block = &rendered[start..end];
    let before = "# Project Guide\n\nKeep two trailing spaces.  \n\tindented tab\t\n\n";
    let after = "\n\n    indented code\n\ttrailing tab\t\n";
    fs::write(repo.join("AGENTS.md"), format!("{before}{block}{after}")).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        format!("{}{}", &before[..before.len() - 1], &after[1..])
    );
}

#[test]
fn full_to_minimal_preserves_crlf_root_agents_bytes_around_the_managed_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let rendered = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    let spec = managed_paths::managed_block_spec(Path::new("AGENTS.md")).unwrap();
    let start = rendered.find(spec.begin).unwrap();
    let end = rendered.find(spec.end).unwrap() + spec.end.len();
    let block = rendered[start..end].replace('\n', "\r\n");
    let before = b"# Project Guide\r\n\r\n";
    let after = b"\r\nPreserve tail spaces.  \r\n";
    let mut contents = before.to_vec();
    contents.extend_from_slice(block.as_bytes());
    contents.extend_from_slice(after);
    fs::write(repo.join("AGENTS.md"), contents).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    let mut expected = before[..before.len() - 2].to_vec();
    expected.extend_from_slice(&after[2..]);
    assert_eq!(fs::read(repo.join("AGENTS.md")).unwrap(), expected);
}

#[test]
fn full_to_minimal_writes_an_empty_root_agents_residual() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let rendered = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    let spec = managed_paths::managed_block_spec(Path::new("AGENTS.md")).unwrap();
    let start = rendered.find(spec.begin).unwrap();
    let end = rendered.find(spec.end).unwrap() + spec.end.len();
    let mut block_only = rendered.as_bytes()[start..end].to_vec();
    block_only.push(b'\n');
    fs::write(repo.join("AGENTS.md"), block_only).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert!(repo.join("AGENTS.md").is_file());
    assert_eq!(fs::read(repo.join("AGENTS.md")).unwrap(), b"");
}

#[test]
fn full_to_minimal_preserves_project_owned_root_agents_without_managed_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::write(repo.join("AGENTS.md"), "# Project Guide\n\nProject only.\n").unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        "# Project Guide\n\nProject only.\n"
    );
}

#[test]
fn forced_full_to_minimal_rejects_malformed_root_agents_block_without_deleting_it() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let malformed = "# Project Guide\n\n<!-- BEGIN JIG MANAGED BLOCK -->\nmissing end\n";
    fs::write(repo.join("AGENTS.md"), malformed).unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Malformed Jig managed block"));
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        malformed
    );
}

#[test]
fn forced_full_to_minimal_preserves_nonregular_root_agents_path() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::remove_file(repo.join("AGENTS.md")).unwrap();
    fs::create_dir(repo.join("AGENTS.md")).unwrap();
    fs::write(repo.join("AGENTS.md/project.txt"), "project-owned\n").unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert!(repo.join("AGENTS.md").is_dir());
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md/project.txt")).unwrap(),
        "project-owned\n"
    );
}

#[cfg(unix)]
#[test]
fn forced_full_to_minimal_preserves_symlinked_root_agents_path() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::remove_file(repo.join("AGENTS.md")).unwrap();
    fs::write(repo.join("AGENTS.shared.md"), "# Shared Project Guide\n").unwrap();
    create_symlink(Path::new("AGENTS.shared.md"), &repo.join("AGENTS.md")).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert!(
        fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.shared.md")).unwrap(),
        "# Shared Project Guide\n"
    );
}

#[test]
fn custom_template_retires_git_blocks_to_exact_project_residuals() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    let gitignore_spec = managed_paths::managed_block_spec(Path::new(".gitignore")).unwrap();
    let gitignore_rendered = fs::read_to_string(repo.join(".gitignore")).unwrap();
    let gitignore_start = gitignore_rendered.find(gitignore_spec.begin).unwrap();
    let gitignore_end =
        gitignore_rendered.find(gitignore_spec.end).unwrap() + gitignore_spec.end.len();
    let gitignore_block = &gitignore_rendered.as_bytes()[gitignore_start..gitignore_end];
    let mut gitignore = b"project-cache/  \n\tproject-tab\t\n\n".to_vec();
    gitignore.extend_from_slice(gitignore_block);
    gitignore.extend_from_slice(b"\nkeep-after/  \n");
    fs::write(repo.join(".gitignore"), gitignore).unwrap();

    let attributes_spec = managed_paths::managed_block_spec(Path::new(".gitattributes")).unwrap();
    let attributes_rendered = fs::read_to_string(repo.join(".gitattributes")).unwrap();
    let attributes_start = attributes_rendered.find(attributes_spec.begin).unwrap();
    let attributes_end =
        attributes_rendered.find(attributes_spec.end).unwrap() + attributes_spec.end.len();
    let mut attributes = attributes_rendered.as_bytes()[attributes_start..attributes_end].to_vec();
    attributes.push(b'\n');
    fs::write(repo.join(".gitattributes"), attributes).unwrap();

    fs::remove_file(template.path().join("templates/project/.gitignore.jinja")).unwrap();
    fs::remove_file(
        template
            .path()
            .join("templates/project/.gitattributes.jinja"),
    )
    .unwrap();

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    assert_eq!(
        fs::read(repo.join(".gitignore")).unwrap(),
        b"project-cache/  \n\tproject-tab\t\nkeep-after/  \n"
    );
    assert!(repo.join(".gitattributes").is_file());
    assert_eq!(fs::read(repo.join(".gitattributes")).unwrap(), b"");
    let manifest = managed_manifest_paths(&repo);
    assert!(manifest.iter().all(|path| path != ".gitignore"));
    assert!(manifest.iter().all(|path| path != ".gitattributes"));
    for retired in [".gitignore", ".gitattributes"] {
        assert!(
            output["render_report"]["retired_managed_paths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path == retired)
        );
        assert!(
            output["render_report"]["files_modified"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path == retired)
        );
        assert!(
            output["render_report"]["files_removed"]
                .as_array()
                .unwrap()
                .iter()
                .all(|path| path != retired)
        );
    }
}

#[test]
fn custom_template_preserves_git_block_paths_without_valid_blocks() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::write(repo.join(".gitignore"), "project-only/\n").unwrap();
    fs::remove_file(repo.join(".gitattributes")).unwrap();
    fs::create_dir(repo.join(".gitattributes")).unwrap();
    fs::write(
        repo.join(".gitattributes/project-owned"),
        "directory sentinel\n",
    )
    .unwrap();
    fs::remove_file(template.path().join("templates/project/.gitignore.jinja")).unwrap();
    fs::remove_file(
        template
            .path()
            .join("templates/project/.gitattributes.jinja"),
    )
    .unwrap();

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join(".gitignore")).unwrap(),
        "project-only/\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join(".gitattributes/project-owned")).unwrap(),
        "directory sentinel\n"
    );
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| path != ".gitignore" && path != ".gitattributes")
    );
    assert!(
        output["render_report"]["retired_managed_paths"]
            .as_array()
            .unwrap()
            .iter()
            .all(|path| path != ".gitignore" && path != ".gitattributes")
    );
}

#[cfg(unix)]
#[test]
fn custom_template_preserves_symlinked_retired_git_block_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    for (relative, target) in [
        (".gitignore", "project.gitignore"),
        (".gitattributes", "project.gitattributes"),
    ] {
        fs::remove_file(repo.join(relative)).unwrap();
        fs::write(repo.join(target), format!("project-owned {relative}\n")).unwrap();
        create_symlink(Path::new(target), &repo.join(relative)).unwrap();
        fs::remove_file(
            template
                .path()
                .join(format!("templates/project/{relative}.jinja")),
        )
        .unwrap();
    }

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    for (relative, target) in [
        (".gitignore", "project.gitignore"),
        (".gitattributes", "project.gitattributes"),
    ] {
        assert!(
            fs::symlink_metadata(repo.join(relative))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(repo.join(target)).unwrap(),
            format!("project-owned {relative}\n")
        );
    }
}

#[test]
fn malformed_retired_git_block_fails_before_apply_and_preserves_prior_manifest() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let manifest_before = fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap();
    let attributes_before = fs::read(repo.join(".gitattributes")).unwrap();
    let malformed = b"project-only/\n# BEGIN JIG MANAGED BLOCK\nmissing end\n";
    fs::write(repo.join(".gitignore"), malformed).unwrap();
    fs::remove_file(template.path().join("templates/project/.gitignore.jinja")).unwrap();
    fs::remove_file(
        template
            .path()
            .join("templates/project/.gitattributes.jinja"),
    )
    .unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Malformed Jig managed block"), "{error}");
    assert_eq!(fs::read(repo.join(".gitignore")).unwrap(), malformed);
    assert_eq!(
        fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap(),
        manifest_before
    );
    assert_eq!(
        fs::read(repo.join(".gitattributes")).unwrap(),
        attributes_before
    );
}

#[test]
fn adopt_minimal_preview_keeps_write_flag_in_next_steps() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: false,
        minimal: true,
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

    assert_eq!(output["render_mode"], "preview");
    assert_eq!(output["harness_footprint"], "minimal");
    assert!(!repo.join(".jig.toml").exists());
    assert!(
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gate| {
                let gate = gate.as_str().unwrap();
                gate.starts_with("jig ") || gate.starts_with("scripts/check-rust-file-loc.sh ")
            })
    );
    assert!(
        output["render_report"]["commands_detected_or_skipped"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| !command.as_str().unwrap().contains("scripts/jig"))
    );
    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(output["next_steps"].as_array().unwrap().iter().all(|step| {
        !step
            .as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write --force")
    }));
}

#[test]
fn full_to_minimal_preview_requires_force_in_the_emitted_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| { step.as_str() == Some("jig adopt . --minimal --write --force") })
    );
}

#[test]
fn minimal_to_minimal_preview_does_not_add_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| { !step.as_str().unwrap().contains("--force") })
    );
}

#[test]
fn invalid_prior_minimal_preview_does_not_add_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        "harness_footprint = \"not-a-footprint\"\n",
    )
    .unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| { !step.as_str().unwrap().contains("--force") })
    );
}

#[test]
fn adopt_preserves_existing_vault_scope_id() {
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
    let first_scope = rendered_vault_scope_id(&repo);

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

    assert_eq!(rendered_vault_scope_id(&repo), first_scope);
}

#[test]
fn adopt_reports_legacy_vault_scope_migration_note() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("Existing .jig.toml had no [vault] block")
    }));
}

#[test]
fn adopt_rejects_existing_repo_vault_scope_without_scope_id() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[vault].scope_id is required"));
}

#[test]
fn adopt_rejects_malformed_existing_vault_policy() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_1"
allow_global = "false"
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[vault].allow_global"));
    assert!(error.contains("must be a boolean"));

    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = 123
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[vault].scope"));
    assert!(error.contains("must be a string"));

    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_1"
unexpected = true
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unknown [vault].unexpected"));
}
