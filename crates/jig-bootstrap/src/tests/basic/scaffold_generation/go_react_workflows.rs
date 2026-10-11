use super::*;

fn assert_quoted_workflow_path_counts(workflow: &str, paths: &[&str], expected: usize) {
    for path in paths {
        assert_eq!(
            workflow.matches(&format!(r#"- "{path}""#)).count(),
            expected,
            "workflow has the wrong filter count for {path}"
        );
    }
}

fn assert_go_adapter_workflow(destination: &Path, workflow_name: &str) {
    let workflow =
        fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
    assert_contains_all(
        &workflow,
        &[
            "actions-rust-lang/setup-rust-toolchain@v1",
            "go-version: ${{ steps.go-version.outputs.version }}",
            "version=\"$(scripts/jig info go-version)\"",
            "cache-dependency-path: |\n            go.mod\n            go.sum\n            go.work\n            go.work.sum\n            **/go.mod",
        ],
    );
    assert_contains_none(
        &workflow,
        &[
            "go-version-file: .go-version",
            "go-version-file: \".go-version\"",
        ],
    );
    let expected_root_filters = usize::from(workflow_name == "go-tests.yml") * 2;
    assert_contains_count(&workflow, &[(r#"- "**""#, expected_root_filters)]);
    if workflow_name == "repo-policy.yml" {
        assert_contains_all(&workflow, &["JIG_PUSH_BEFORE: ${{ github.event.before }}"]);
    }
    assert_quoted_workflow_path_counts(
        &workflow,
        &[
            "go.mod",
            "go.sum",
            "go.work",
            "go.work.sum",
            "**/go.mod",
            "**/go.sum",
            "**/go.work",
            "**/go.work.sum",
            "vendor/modules.txt",
            "**/vendor/modules.txt",
            "sqlc.yaml",
            "**/sqlc.yaml",
            "internal/database/migrations/**",
            "**/*.sql",
        ],
        expected_root_filters,
    );
}

fn assert_go_test_workflow(destination: &Path) {
    let go_tests = fs::read_to_string(destination.join(".github/workflows/go-tests.yml")).unwrap();
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&go_tests).unwrap();
    assert_eq!(parsed["jobs"]["checks"]["defaults"]["run"]["shell"], "bash");
    assert_contains_count(
        &go_tests,
        &[
            (r#"- "openapi/**""#, 2),
            (r#"- "scripts/test-postgres.sh""#, 2),
            ("runs-on: \"macos-14\"", 1),
            ("runs-on: \"ubuntu-latest\"", 1),
            ("actions-rust-lang/setup-rust-toolchain@v1", 2),
        ],
    );
    for target in ["api:fmt", "api:lint", "api:test-locked", "api:sqlc"] {
        assert_contains_all(&go_tests, &[&format!("scripts/jig check {target}")]);
    }
    // The backend tests run uncached, so a restored Go cache cannot replay a
    // pass over a stale committed document; the other checks keep the cache.
    for step in parsed["jobs"]["checks"]["steps"].as_array().unwrap() {
        let Some(run) = step["run"].as_str() else {
            continue;
        };
        let expected = if run == "scripts/jig check api:test-locked" {
            serde_json::json!({ "GOFLAGS": "-count=1" })
        } else {
            serde_json::Value::Null
        };
        assert_eq!(step["env"], expected, "{run}");
    }
    assert_quoted_workflow_path_counts(
        &go_tests,
        &[
            "go.mod",
            "go.sum",
            "go.work",
            "go.work.sum",
            "**/go.mod",
            "**/go.sum",
            "**/go.work",
            "**/go.work.sum",
            "vendor/modules.txt",
            "**/vendor/modules.txt",
            "scripts/jig",
            "scripts/install-jig.sh",
        ],
        2,
    );
    let postgres_job = &go_tests[go_tests.find("postgres-integration:").unwrap()..];
    assert_text_before(
        postgres_job,
        "actions-rust-lang/setup-rust-toolchain@v1",
        "name: Resolve Go toolchain version",
    );
    assert_contains_all(&go_tests, &["run: bash scripts/test-postgres.sh"]);
    assert_paths_absent(destination, &[".go-version"]);
}

#[test]
fn go_react_web_workflow_observes_the_complete_application_contract() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("go-contract-workflow");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![ScaffoldFrontend {
                name: "web".into(),
                kind: ScaffoldFrontendKind::Spa,
                custom_default_name: false,
            }],
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("go-contract-workflow".into()),
            go_module: Some("example.com/go-contract-workflow".into()),
            ci_github_runner: Some("macos-14".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let workflow =
        fs::read_to_string(destination.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert_contains_count(
        &workflow,
        &[
            (r#"- "openapi/**""#, 2),
            (r#"- "packages/public-api-client/**""#, 2),
        ],
    );
    // The frontend job never runs Go, so Go sources do not trigger it. It lints
    // with the shared config, installs through the web-node helper, and its
    // client-check scans public docs and the module manifest.
    assert_contains_none(
        &workflow,
        &[
            r#"- "**""#,
            r#"- "**/*.go""#,
            r#"- "apps/*-api/Cargo.toml""#,
        ],
    );
    assert_workflow_triggered_by(
        &workflow,
        &[
            "eslint.config.shared.mjs",
            "scripts/web-node.cjs",
            "docs/public/guide.md",
            "public-docs/index.html",
            "go.mod",
        ],
    );
    assert_contains_all(&workflow, &["node scripts/contracts.mjs client-check"]);
    assert_contains_none(&workflow, &["if [ -f scripts/contracts.mjs ]"]);
    assert_text_before(
        &workflow,
        "Run build",
        "Check generated API clients and public boundary",
    );

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        assert_go_adapter_workflow(&destination, workflow_name);
    }
    assert_go_test_workflow(&destination);

    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert_contains_all(
        &gitignore,
        &[".contract-stage-*/", ".contract-client-stage-*/"],
    );

    let browser_e2e = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert_contains_all(
        &browser_e2e,
        &[
            "version=\"$(scripts/jig info go-version)\"",
            "go-version: ${{ steps.go-version.outputs.version }}",
            "cache-dependency-path: |",
        ],
    );
    assert_go_repository_contract(&destination);
    assert_go_generated_runtime_files(&destination);

    let config_path = destination.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        r#"migration_dir = "internal/database/migrations""#,
        r#"migration_dir = "database/migrations""#,
    );
    fs::write(&config_path, config).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        let workflow =
            fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
        assert_contains_count(
            &workflow,
            &[(
                r#"- "database/migrations/**""#,
                usize::from(workflow_name == "go-tests.yml") * 2,
            )],
        );
        assert_contains_none(&workflow, &[r#"- "internal/database/migrations/**""#]);
    }

    #[cfg(unix)]
    if node_available() {
        install_fake_openapi_ts(&destination);
        let fake_bin = destination.join(".fake-bin");
        fs::create_dir_all(&fake_bin).unwrap();
        write_executable_test_script(
            &fake_bin.join("go"),
            "#!/bin/sh\n: > \"$JIG_TEST_GO_MARKER\"\nexit 91\n",
        );
        let backend_marker = destination.join(".backend-exporter-ran");
        let before = regular_file_tree_snapshot(&destination);

        let output = run_contracts_mode(
            &destination,
            "client-check",
            &fake_bin,
            &[("JIG_TEST_GO_MARKER", backend_marker.as_os_str())],
        );

        assert_rust_only_command_output_success("client-check", &output);
        assert_paths_absent(&destination, &[".backend-exporter-ran"]);
        assert_eq!(regular_file_tree_snapshot(&destination), before);
        assert_go_drift_check_reruns_the_document_test_uncached(&destination, &fake_bin);
    }

    let nested_module_dir = destination.join("services/api");
    fs::create_dir_all(&nested_module_dir).unwrap();
    fs::rename(destination.join("go.mod"), nested_module_dir.join("go.mod")).unwrap();
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let api_component = config["repository"]["components"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|component| component["id"].as_str() == Some("api"))
        .unwrap();
    api_component["root"] = toml::Value::String("services/api".into());
    let locked_test = config["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|action| {
            action["target"]["component"].as_str() == Some("api")
                && action["target"]["action"].as_str() == Some("test-locked")
        })
        .unwrap();
    locked_test["inputs"]
        .as_array_mut()
        .unwrap()
        .push(toml::Value::String("shared/proto/**".into()));
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        let workflow =
            fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
        assert_contains_all(&workflow, &["version=\"$(scripts/jig info go-version)\""]);
        assert_contains_none(&workflow, &["go-version-file: go.mod", r#"- "**""#]);
        let expected_path_filters = usize::from(workflow_name == "go-tests.yml") * 2;
        assert_contains_count(
            &workflow,
            &[
                (r#"- "services/api/**""#, expected_path_filters),
                (r#"- "shared/proto/**""#, expected_path_filters),
            ],
        );
    }
    let webapp =
        fs::read_to_string(destination.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert_workflow_triggered_by(&webapp, &["services/api/go.mod"]);
    let context = jig_context::RepoContext::load_from(&destination).unwrap();
    assert_go_module_authority_declares(&context, "1.26.0");
    let browser_e2e = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert_contains_all(
        &browser_e2e,
        &["version=\"$(scripts/jig info go-version)\""],
    );
    assert_contains_none(&browser_e2e, &["go-version-file: go.mod"]);

    fs::remove_file(destination.join("scripts/test-postgres.sh")).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();
    let go_tests = fs::read_to_string(destination.join(".github/workflows/go-tests.yml")).unwrap();
    assert_contains_all(&go_tests, &["scripts/jig check api:sqlc"]);
    assert_contains_none(&go_tests, &["postgres-integration:"]);
}

/// drift-check never exports from the backend. It reruns the backend's
/// document test uncached: Go ignores files outside the module when it reuses
/// a cached pass, so a warmed cache must not hide a stale document.
#[cfg(unix)]
fn assert_go_drift_check_reruns_the_document_test_uncached(destination: &Path, fake_bin: &Path) {
    // A warmed cache passes; only an uncached run sees the stale document.
    write_executable_test_script(
        &fake_bin.join("go"),
        "#!/bin/sh\nprintf '%s|%s\\n' \"$(pwd -P)\" \"$*\" >> \"$JIG_TEST_GO_LOG\"\ncase \"$*\" in\n  run*) exit 91 ;;\n  *-count=1*) exit \"$JIG_TEST_GO_UNCACHED_STATUS\" ;;\nesac\nexit 0\n",
    );
    let log = destination.join(".go-invocations");
    let root = fs::canonicalize(destination).unwrap();
    let before = regular_file_tree_snapshot(destination);
    for (status, passes) in [("1", false), ("0", true)] {
        let output = run_contracts_mode(
            destination,
            "drift-check",
            fake_bin,
            &[
                ("JIG_TEST_GO_LOG", log.as_os_str()),
                ("JIG_TEST_GO_UNCACHED_STATUS", std::ffi::OsStr::new(status)),
            ],
        );
        assert_eq!(
            output.status.success(),
            passes,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            format!(
                "{}|test -count=1 -run ^TestOpenAPIIsCurrent$ ./internal/httpapi\n",
                root.display()
            )
        );
        fs::remove_file(&log).unwrap();
    }
    assert_eq!(regular_file_tree_snapshot(destination), before);
}
