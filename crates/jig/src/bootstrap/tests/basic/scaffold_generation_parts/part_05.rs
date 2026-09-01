fn assert_rendered_paths(rendered: &[scaffold::ScaffoldFile], expected: &[&str]) {
    for path in expected {
        assert!(
            rendered.iter().any(|file| file.relative == *path),
            "missing nested Go component output {path}"
        );
    }
}

fn assert_rendered_paths_absent(rendered: &[scaffold::ScaffoldFile], forbidden: &[&str]) {
    for path in forbidden {
        assert!(
            rendered.iter().all(|file| file.relative != *path),
            "Go component output escaped to the repository root: {path}"
        );
    }
}

fn assert_nested_go_defaults(plan: &scaffold::InitScaffoldPlan) {
    let mut defaults = AnswerOpts::default();
    plan.apply_answer_defaults(&mut defaults);
    assert_eq!(
        defaults.migration_dir.as_deref(),
        Some("services/api/internal/database/migrations")
    );
    assert_eq!(defaults.dev_apps[0].dir.as_deref(), Some("services/api"));
    let bootstrap = defaults.bootstrap_command.unwrap();
    assert_contains_all(
        &bootstrap,
        &[
            "(cd services/api && go mod tidy)",
            "(cd services/api && if [ -z",
            "(cd services/api && go tool sqlc generate && go run ./cmd/api --bootstrap-database)",
        ],
    );
}

#[test]
fn go_browser_scaffold_honors_the_authored_backend_root() {
    let planning_root = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![ScaffoldFrontend {
                name: "web".into(),
                kind: ScaffoldFrontendKind::Spa,
                custom_default_name: false,
            }],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            go_module: Some("example.com/demo".into()),
            scaffold_go_component_roots: vec!["services/api".into()],
            migration_dir: Some("services/api/internal/database/migrations".into()),
            ..AnswerOpts::default()
        },
        planning_root.path(),
    )
    .unwrap()
    .unwrap();

    let rendered = plan.render_files().unwrap();
    let contents = |path: &str| {
        rendered
            .iter()
            .find(|file| file.relative == path)
            .unwrap_or_else(|| panic!("missing rendered {path}"))
            .contents
            .as_str()
    };
    assert_rendered_paths(&rendered, &[
        "services/api/.env.example",
        "services/api/go.mod",
        "services/api/cmd/api/main.go",
        "services/api/cmd/openapi/main.go",
        "services/api/sqlc.yaml",
        "services/api/internal/database/database.go",
    ]);
    assert_rendered_paths_absent(&rendered, &[".env.example", "go.mod", "cmd/api/main.go", "sqlc.yaml"]);
    let output_paths = plan.output_paths();
    assert!(
        output_paths
            .iter()
            .any(|path| path == Path::new("services/api/go.mod"))
    );
    assert!(
        output_paths
            .iter()
            .all(|path| path != Path::new("go.mod"))
    );
    assert!(rendered.iter().any(|file| file.relative == "openapi/public.json"));
    let postgres_script = contents("scripts/test-postgres.sh");
    assert_contains_all(postgres_script, &[r#"go -C "services/api" test -count=1"#]);
    let httpapi_test = contents("services/api/internal/httpapi/httpapi_test.go");
    assert!(httpapi_test.contains(
        r#"filepath.FromSlash("../../../../openapi/public.json")"#
    ));
    let workflow = contents(".github/workflows/e2e.yml");
    assert_eq!(workflow.matches(r#"- "services/api/**""#).count(), 2);
    assert_eq!(
        workflow
            .matches(r#"- "services/api/internal/database/migrations/**""#)
            .count(),
        2
    );
    assert_contains_none(workflow, &[r#"- "cmd/**""#, r#"- "internal/**""#, r#"- "**""#]);

    let playwright = contents("web/playwright.config.ts");
    assert_contains_all(playwright, &[r#"path.resolve(repoRoot, "services/api")"#, "cwd: backendRoot"]);
    let contracts = contents("scripts/contracts.mjs");
    assert_contains_all(contracts, &[
        r#"resolve(repoRoot, "services/api")"#,
        r#"join(backendRoot, "go.mod")"#,
        r#"run("go", ["run", "./cmd/openapi", "--output", document], backendRoot)"#,
    ]);
    assert_nested_go_defaults(&plan);
}

#[test]
fn go_react_rejects_missing_module_sqlite_and_admin() {
    let planning_root = tempdir().unwrap();
    let missing_module = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        &AnswerOpts::default(),
        planning_root.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(missing_module.contains("--go-module"));

    let sqlite = ScaffoldOpts {
        preset: Some(ScaffoldPreset::GoReact),
        db: Some(ScaffoldDb::Sqlite),
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts {
        go_module: Some("example.com/demo".into()),
        ..AnswerOpts::default()
    })
    .unwrap_err()
    .to_string();
    assert!(sqlite.contains("does not support --db sqlite"));

    let admin = ScaffoldOpts {
        preset: Some(ScaffoldPreset::GoReact),
        db: Some(ScaffoldDb::None),
        frontends: vec![ScaffoldFrontend {
            name: "admin".into(),
            kind: ScaffoldFrontendKind::Admin,
            custom_default_name: false,
        }],
        frontend_list: Vec::new(),
    }
    .validate_init_invariants(&AnswerOpts {
        go_module: Some("example.com/demo".into()),
        ..AnswerOpts::default()
    })
    .unwrap_err()
    .to_string();
    assert!(admin.contains("separate privileged API and client boundary"));
}
