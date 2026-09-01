fn assert_legacy_scaffold_files(
    destination: &Path,
    scaffold: &serde_json::Value,
    required: &[&str],
    absent: &[&str],
) {
    assert_eq!(scaffold["files_modified"], serde_json::json!([]));
    assert_eq!(scaffold["files_unchanged"], serde_json::json!([]));

    let created = scaffold["files_created"].as_array().unwrap();
    let created_paths = created
        .iter()
        .map(|path| path.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(created_paths.len(), created.len());
    for path in &created_paths {
        assert!(destination.join(path).is_file(), "reported file is missing: {path}");
    }
    for path in required {
        assert!(created_paths.contains(path), "unreported compatibility file: {path}");
        assert!(destination.join(path).is_file(), "missing compatibility file: {path}");
    }
    for path in absent {
        assert!(!created_paths.contains(path), "unexpected compatibility report path: {path}");
        assert!(!destination.join(path).exists(), "unexpected compatibility output: {path}");
    }
}

fn assert_legacy_web_frontend(scaffold: &serde_json::Value) {
    assert_eq!(scaffold["frontend_notices"], serde_json::json!([]));
    let frontends = scaffold["frontends"].as_array().unwrap();
    assert_eq!(frontends.len(), 1);
    assert_eq!(frontends[0]["name"], "web");
    assert_eq!(frontends[0]["dir"], "web");
    assert_eq!(frontends[0]["kind"], "vite");
    assert_eq!(frontends[0]["role"], "spa");
    assert_eq!(frontends[0]["ui"]["style"], "radix-nova");
    assert_eq!(frontends[0]["ui"]["cli_version"], "4.18.0");
}

#[test]
fn rust_only_foundation_preserves_rust_react_output_and_report() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_git_worktree();
    let destination = temp.path().join("ExampleProject");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();
    let scaffold = &output["scaffold"];

    assert_eq!(scaffold["preset"], "rust-react");
    assert_eq!(scaffold["repo_name"], "exampleproject");
    assert_eq!(scaffold["db"], "none");
    assert_legacy_web_frontend(scaffold);
    assert_legacy_scaffold_files(
        &destination,
        scaffold,
        &[
            "Cargo.toml",
            "clippy.toml",
            "apps/exampleproject-api/src/main.rs",
            "crates/exampleproject/src/lib.rs",
            "crates/exampleproject-core/src/lib.rs",
            "crates/exampleproject-http/src/public.rs",
            "openapi/public.json",
            "package.json",
            "web/package.json",
        ],
        &[
            "crates/exampleproject/src/main.rs",
            "go.mod",
            "openapi/admin.json",
        ],
    );
    let cargo = toml::from_str::<toml::Value>(
        &fs::read_to_string(destination.join("Cargo.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        cargo["workspace"]["members"].as_array().unwrap(),
        &[
            "apps/exampleproject-api",
            "crates/exampleproject-core",
            "crates/exampleproject",
            "crates/exampleproject-http",
            "crates/exampleproject-http-common",
            "crates/exampleproject-test-support",
        ]
        .map(|member| toml::Value::String(member.into()))
    );
    assert_eq!(
        cargo["workspace"]["lints"]["clippy"]["cognitive_complexity"].as_str(),
        Some("warn")
    );
    assert_eq!(
        fs::read_to_string(destination.join("clippy.toml")).unwrap(),
        "cognitive-complexity-threshold = 20\n"
    );
}

#[test]
fn rust_only_foundation_preserves_go_react_output_and_report() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_git_worktree();
    let destination = temp.path().join("ExampleProject");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            go_module: Some("example.com/ExampleProject".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    let scaffold = &output["scaffold"];

    assert_eq!(scaffold["preset"], "go-react");
    assert_eq!(scaffold["repo_name"], "exampleproject");
    assert_eq!(scaffold["db"], "none");
    assert_legacy_web_frontend(scaffold);
    assert_legacy_scaffold_files(
        &destination,
        scaffold,
        &[
            "go.mod",
            "cmd/api/main.go",
            "cmd/api/main_test.go",
            "cmd/openapi/main.go",
            "internal/config/config.go",
            "internal/httpapi/httpapi.go",
            "openapi/public.json",
            "package.json",
            "web/package.json",
        ],
        &[
            "Cargo.toml",
            "crates/exampleproject/src/lib.rs",
            "crates/exampleproject/src/main.rs",
            "internal/database/database.go",
            "openapi/admin.json",
        ],
    );
    assert!(
        fs::read_to_string(destination.join("go.mod"))
            .unwrap()
            .starts_with("module example.com/ExampleProject\n")
    );
}

#[test]
fn rust_only_foundation_preserves_harness_only_output_and_report() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_git_worktree();
    let destination = temp.path().join("ExampleVault");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::HarnessOnly),
            ..ScaffoldOpts::default()
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert!(output["scaffold"].is_null());
    for path in [
        ".jig.toml",
        ".agent/jig-contract.json",
        "AGENTS.md",
        "agent-map.md",
        "scripts/jig",
    ] {
        assert!(destination.join(path).is_file(), "missing harness file: {path}");
    }
    for path in ["Cargo.toml", "README.md", "go.mod", "package.json", "apps", "crates", "web"] {
        assert!(!destination.join(path).exists(), "unexpected project output: {path}");
    }
    let config = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(config.contains("sqlx_enabled = false"));
    assert!(config.contains("schema_dump_enabled = false"));
}
