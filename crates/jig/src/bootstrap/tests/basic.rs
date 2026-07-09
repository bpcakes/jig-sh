use super::*;
use crate::test_env::CurrentDirGuard;

fn rendered_vault_scope_id(repo: &std::path::Path) -> String {
    let text = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    let value = toml::from_str::<toml::Value>(&text).unwrap();
    value["vault"]["scope_id"].as_str().unwrap().to_string()
}

#[test]
fn parses_frontend_app_flag() {
    let app = parse_frontend_app("frontend:web:40").unwrap();
    assert_eq!(
        app,
        FrontendApp {
            name: "frontend".into(),
            dir: "web".into(),
            coverage_threshold: 40,
            kind: "vite".into(),
        }
    );

    let app = parse_frontend_app("frontend:web:40:env-port").unwrap();
    assert_eq!(app.kind, "env-port");
}

#[test]
fn parses_scaffold_frontend_aliases_and_explicit_kinds() {
    let admin = parse_scaffold_frontend("admin").unwrap();
    assert_eq!(admin.name, "admin-panel");
    assert_eq!(admin.kind, ScaffoldFrontendKind::Admin);

    let docs = parse_scaffold_frontend("docs:astro").unwrap();
    assert_eq!(docs.name, "docs");
    assert_eq!(docs.kind, ScaffoldFrontendKind::Astro);

    let operations = parse_scaffold_frontend("operations:admin").unwrap();
    assert_eq!(operations.name, "operations");
    assert_eq!(operations.kind, ScaffoldFrontendKind::Admin);

    let billing = parse_scaffold_frontend("billing").unwrap();
    assert_eq!(billing.name, "billing");
    assert_eq!(billing.kind, ScaffoldFrontendKind::Spa);

    assert!(
        parse_scaffold_frontend("bad/name")
            .unwrap_err()
            .contains("frontend name must use ASCII")
    );
    assert!(
        parse_scaffold_frontend("-")
            .unwrap_err()
            .contains("frontend name must include at least one ASCII letter or number")
    );
    assert!(
        parse_scaffold_frontend("web:unknown")
            .unwrap_err()
            .contains("unsupported frontend kind 'unknown'")
    );
}

#[test]
fn seed_answers_only_serializes_provided_values() {
    let toml = seed_answers_toml(
        &AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            rust_crate_roots: vec!["crates".into()],
            frontend_apps: vec![FrontendApp {
                name: "frontend".into(),
                dir: "web".into(),
                coverage_threshold: 40,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
        &PrivateAnswerOverrides::default(),
    );

    let mapping = toml.as_table().unwrap();
    assert_eq!(
        mapping.get("repo_name").unwrap(),
        &TomlValue::String("demo".into())
    );
    assert_eq!(
        mapping.get("sqlx_enabled").unwrap(),
        &TomlValue::Boolean(false)
    );
    assert!(mapping.contains_key("rust_crate_roots"));
    assert!(!mapping.contains_key("default_branch"));
}

#[test]
fn initial_next_steps_and_notes_are_tailored_to_rendered_config() {
    assert_eq!(template_progress_label(None), "default jig-sh template");
    assert_eq!(template_progress_label(Some("/tmp/jig-sh")), "/tmp/jig-sh");

    let destination = PathBuf::from("/tmp/demo");
    let result = initial_copy::BootstrapCopyResult {
        default_branch: Some("main".into()),
        bootstrap_command_configured: true,
        frontend_apps_configured: true,
        dev_apps_configured: true,
        codex_skills_configured: true,
        sqlx_enabled: true,
        schema_dump_enabled: true,
        render_preview: initial_copy::AdoptionRenderPreview::default(),
        apply_report: sync::ApplyRenderReport::default(),
        notes: Vec::new(),
    };

    let steps = initial_next_steps(InitialCommand::Adopt, &destination, &result);

    assert_eq!(steps[0], "cd /tmp/demo");
    for expected in [
        "scripts/jig bootstrap",
        "scripts/jig doctor",
        "scripts/jig agent bootstrap",
        "scripts/jig check contract",
        "scripts/jig check test",
        "scripts/jig dev",
    ] {
        assert!(steps.iter().any(|step| step == expected));
    }
    assert!(
        steps
            .iter()
            .any(|step| step.contains("scripts/jig check sqlx"))
    );
    assert!(
        steps
            .iter()
            .any(|step| step.contains("scripts/dump-schema.sh"))
    );
    assert!(
        steps
            .iter()
            .any(|step| step.contains("Commit the adoption diff"))
    );
    assert!(!steps.iter().any(|step| step.starts_with("Review ")));

    let notes = initial_notes(Vec::new(), true, None);
    for expected in [
        "Review generated .jig.toml",
        "scripts/jig check typescript-lint",
        "scripts/jig check contract",
    ] {
        assert!(notes.iter().any(|note| note.contains(expected)));
    }

    let preview_steps = initial_next_steps(
        InitialCommand::Adopt,
        Path::new("/tmp/preview"),
        &initial_copy::BootstrapCopyResult {
            default_branch: Some("main".into()),
            bootstrap_command_configured: true,
            frontend_apps_configured: true,
            dev_apps_configured: true,
            codex_skills_configured: true,
            sqlx_enabled: true,
            schema_dump_enabled: true,
            render_preview: initial_copy::AdoptionRenderPreview::default(),
            apply_report: sync::ApplyRenderReport {
                dry_run: true,
                ..sync::ApplyRenderReport::default()
            },
            notes: Vec::new(),
        },
    );
    assert!(
        preview_steps
            .iter()
            .any(|step| step.contains("jig adopt . --write"))
    );
    assert!(
        preview_steps
            .iter()
            .any(|step| step == "No files were changed by this preview.")
    );
    assert!(
        !preview_steps
            .iter()
            .any(|step| step.starts_with("scripts/jig"))
    );

    let quoted_steps = initial_next_steps(
        InitialCommand::Init,
        Path::new("/tmp/demo repo"),
        &initial_copy::BootstrapCopyResult {
            default_branch: Some("main".into()),
            bootstrap_command_configured: true,
            frontend_apps_configured: false,
            dev_apps_configured: false,
            codex_skills_configured: false,
            sqlx_enabled: false,
            schema_dump_enabled: false,
            render_preview: initial_copy::AdoptionRenderPreview::default(),
            apply_report: sync::ApplyRenderReport::default(),
            notes: Vec::new(),
        },
    );
    assert_eq!(quoted_steps[0], "cd '/tmp/demo repo'");

    let no_bootstrap_steps = initial_next_steps(
        InitialCommand::Init,
        Path::new("/tmp/no-bootstrap"),
        &initial_copy::BootstrapCopyResult {
            default_branch: Some("main".into()),
            bootstrap_command_configured: false,
            frontend_apps_configured: false,
            dev_apps_configured: false,
            codex_skills_configured: false,
            sqlx_enabled: false,
            schema_dump_enabled: false,
            render_preview: initial_copy::AdoptionRenderPreview::default(),
            apply_report: sync::ApplyRenderReport::default(),
            notes: Vec::new(),
        },
    );
    assert!(
        !no_bootstrap_steps
            .iter()
            .any(|step| step == "scripts/jig bootstrap")
    );
}

fn write_answers_fixture(dir: &Path, sqlx_enabled: Option<bool>) {
    let mut body = String::from("default_branch = \"main\"\n");
    if let Some(sqlx_enabled) = sqlx_enabled {
        body.push_str(&format!(
            "sqlx_enabled = {}\n",
            if sqlx_enabled { "true" } else { "false" }
        ));
    }
    fs::write(dir.join(".jig.toml"), body).unwrap();
}

#[test]
fn rendered_conflicts_detects_generated_paths() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "rendered").unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(destination.path().join("scripts/jig"), "existing").unwrap();

    let conflicts = rendered_conflicts(rendered.path(), destination.path()).unwrap();
    assert_eq!(conflicts, vec!["scripts/jig"]);
}

#[test]
fn rendered_conflicts_marks_task_mutated_outputs() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::write(rendered.path().join("agent-map.md"), "placeholder").unwrap();
    fs::write(destination.path().join("agent-map.md"), "existing").unwrap();

    let conflicts = rendered_conflicts(rendered.path(), destination.path()).unwrap();
    assert_eq!(conflicts, vec!["agent-map.md"]);
}

#[test]
fn rendered_conflicts_marks_retired_managed_paths() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(false));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(
        rendered.path().join("scripts/add-migration.sh"),
        "templated",
    )
    .unwrap();
    fs::write(
        destination.path().join("scripts/add-migration.sh"),
        "existing",
    )
    .unwrap();

    let conflicts = rendered_conflicts(rendered.path(), destination.path()).unwrap();
    assert_eq!(conflicts, vec!["scripts/add-migration.sh"]);
}

#[test]
fn rendered_conflicts_ignores_identical_files() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "same").unwrap();
    fs::write(destination.path().join("scripts/jig"), "same").unwrap();

    let conflicts = rendered_conflicts(rendered.path(), destination.path()).unwrap();
    assert!(conflicts.is_empty());
}

#[cfg(unix)]
#[test]
fn apply_staged_render_does_not_rewrite_preserved_files() {
    use std::collections::BTreeSet;
    use std::os::unix::fs::PermissionsExt;

    let staged_root = tempdir().unwrap();
    let rendered_destination = staged_root.path().join("rendered");
    let destination = tempdir().unwrap();
    fs::create_dir_all(rendered_destination.join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(rendered_destination.join("scripts/jig"), "same").unwrap();
    fs::write(destination.path().join("scripts/jig"), "same").unwrap();

    fs::set_permissions(
        destination.path().join("scripts"),
        fs::Permissions::from_mode(0o555),
    )
    .unwrap();

    let staged = staged_render::StagedRender {
        _root: staged_root,
        destination: rendered_destination,
        managed_paths: BTreeSet::from([PathBuf::from("scripts/jig")]),
    };
    let report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: true,
            allow_answers_overwrite: true,
            dry_run: false,
            backup_root: None,
            conflict_message: "conflict",
            progress: CliProgress::new("test"),
        },
    )
    .unwrap();

    fs::set_permissions(
        destination.path().join("scripts"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    assert_eq!(report.files_unchanged, vec!["scripts/jig"]);
    assert!(report.files_modified.is_empty());
}

#[test]
fn apply_staged_render_reports_managed_block_insertions_only_when_inserted() {
    use std::collections::BTreeSet;

    let staged_root = tempdir().unwrap();
    let rendered_destination = staged_root.path().join("rendered");
    let destination = tempdir().unwrap();
    fs::create_dir_all(&rendered_destination).unwrap();
    fs::write(
        rendered_destination.join("AGENTS.md"),
        "# Guide\n\n<!-- BEGIN JIG MANAGED BLOCK -->\nmanaged\n<!-- END JIG MANAGED BLOCK -->\n",
    )
    .unwrap();
    fs::write(destination.path().join("AGENTS.md"), "# Existing\n").unwrap();

    let staged = staged_render::StagedRender {
        _root: staged_root,
        destination: rendered_destination,
        managed_paths: BTreeSet::from([PathBuf::from("AGENTS.md")]),
    };
    let report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: true,
            allow_answers_overwrite: true,
            dry_run: false,
            backup_root: None,
            conflict_message: "conflict",
            progress: CliProgress::new("test"),
        },
    )
    .unwrap();

    assert_eq!(report.managed_blocks_inserted, vec!["AGENTS.md"]);
    assert!(report.managed_blocks_rendered.is_empty());

    let second_report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: true,
            allow_answers_overwrite: true,
            dry_run: false,
            backup_root: None,
            conflict_message: "conflict",
            progress: CliProgress::new("test"),
        },
    )
    .unwrap();

    assert!(second_report.managed_blocks_inserted.is_empty());
    assert!(second_report.managed_blocks_rendered.is_empty());
    assert_eq!(second_report.files_unchanged, vec!["AGENTS.md"]);
}

#[test]
fn apply_staged_render_allows_root_agents_managed_block_update_without_force() {
    use std::collections::BTreeSet;

    let staged_root = tempdir().unwrap();
    let rendered_destination = staged_root.path().join("rendered");
    let destination = tempdir().unwrap();
    fs::create_dir_all(&rendered_destination).unwrap();
    fs::write(
        rendered_destination.join("AGENTS.md"),
        "# Existing\n\nCustom repo guidance.\n\n<!-- BEGIN JIG MANAGED BLOCK -->\nnew\n<!-- END JIG MANAGED BLOCK -->\n",
    )
    .unwrap();
    fs::write(
        destination.path().join("AGENTS.md"),
        "# Existing\n\nCustom repo guidance.\n\n<!-- BEGIN JIG MANAGED BLOCK -->\nold\n<!-- END JIG MANAGED BLOCK -->\n",
    )
    .unwrap();

    let staged = staged_render::StagedRender {
        _root: staged_root,
        destination: rendered_destination,
        managed_paths: BTreeSet::from([PathBuf::from("AGENTS.md")]),
    };
    let report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: false,
            allow_answers_overwrite: true,
            dry_run: false,
            backup_root: None,
            conflict_message: "conflict",
            progress: CliProgress::new("test"),
        },
    )
    .unwrap();

    let root_guide = fs::read_to_string(destination.path().join("AGENTS.md")).unwrap();
    assert_eq!(report.files_modified, vec!["AGENTS.md"]);
    assert!(root_guide.contains("Custom repo guidance."));
    assert!(root_guide.contains("new"));
}

#[cfg(unix)]
#[test]
fn rendered_conflicts_detects_executable_bit_changes() {
    use std::os::unix::fs::PermissionsExt;

    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "same").unwrap();
    fs::write(destination.path().join("scripts/jig"), "same").unwrap();
    fs::set_permissions(
        rendered.path().join("scripts/jig"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::set_permissions(
        destination.path().join("scripts/jig"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();

    let conflicts = rendered_conflicts(rendered.path(), destination.path()).unwrap();
    assert_eq!(conflicts, vec!["scripts/jig"]);
}

#[cfg(unix)]
#[test]
fn rendered_conflicts_detects_file_replacing_symlink() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "same").unwrap();
    fs::write(destination.path().join("scripts/target"), "same").unwrap();
    create_symlink(Path::new("target"), &destination.path().join("scripts/jig")).unwrap();

    let conflicts = rendered_conflicts(rendered.path(), destination.path()).unwrap();
    assert_eq!(conflicts, vec!["scripts/jig"]);
}

#[test]
fn rendered_conflicts_detects_blocking_ancestor_file() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "rendered").unwrap();
    fs::write(destination.path().join("scripts"), "blocking file").unwrap();

    let conflicts = rendered_conflicts(rendered.path(), destination.path()).unwrap();
    assert_eq!(conflicts, vec!["scripts"]);
}

#[test]
fn preview_workspace_only_copies_agent_guides() {
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    fs::create_dir_all(source.path().join("crates/api")).unwrap();
    fs::create_dir_all(source.path().join("crates/vendor/.git/modules/demo")).unwrap();
    fs::create_dir_all(source.path().join("target/debug")).unwrap();
    fs::create_dir_all(source.path().join("target/package/demo")).unwrap();
    fs::write(source.path().join("AGENTS.md"), "root").unwrap();
    fs::write(source.path().join("crates/api/AGENTS.md"), "nested").unwrap();
    fs::write(
        source
            .path()
            .join("crates/vendor/.git/modules/demo/AGENTS.md"),
        "submodule metadata",
    )
    .unwrap();
    fs::write(source.path().join("target/debug/build.log"), "noise").unwrap();
    fs::write(
        source.path().join("target/package/demo/AGENTS.md"),
        "artifact",
    )
    .unwrap();

    seed_preview_workspace(source.path(), destination.path()).unwrap();

    assert!(destination.path().join("AGENTS.md").exists());
    assert!(destination.path().join("crates/api/AGENTS.md").exists());
    assert!(
        !destination
            .path()
            .join("crates/vendor/.git/modules/demo/AGENTS.md")
            .exists()
    );
    assert!(!destination.path().join("target/debug/build.log").exists());
    assert!(
        !destination
            .path()
            .join("target/package/demo/AGENTS.md")
            .exists()
    );
}

#[test]
fn run_init_uses_native_renderer_and_git() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
        &git_path,
        format!(
            "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log_path.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["git_initialized"], true);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("git init -b main"));
    assert!(destination.exists());
    assert!(destination.join(".jig.toml").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("[vault]"));
    assert!(answers.contains("scope = \"repo\""));
    assert!(answers.contains("allow_global = false"));
    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(gitignore.contains("node_modules/"));
    assert!(gitignore.contains("target/"));
    assert!(gitignore.contains(".agent/.cache/*"));
    assert!(gitignore.contains("# BEGIN JIG MANAGED BLOCK"));
    let attributes = fs::read_to_string(destination.join(".gitattributes")).unwrap();
    assert!(attributes.contains(".agent/state/*.jsonl merge=union"));
    assert!(destination.join("scripts/jig").exists());
}

#[test]
fn run_init_sqlx_disabled_defaults_to_harness_only_safe_commands() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
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

    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(answers.contains("Command values are project-owned."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo bootstrap."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo test."));
}

#[test]
fn run_init_rust_react_scaffold_generates_backend_and_frontends() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("my-app");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
                parse_scaffold_frontend("admin").unwrap(),
            ],
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

    assert_eq!(output["scaffold"]["preset"], "rust-react");
    assert_eq!(output["scaffold"]["db"], "postgres");
    assert!(destination.join(".env.example").exists());
    assert!(destination.join("Cargo.toml").exists());
    assert!(destination.join("apps/my-app-api/src/main.rs").exists());
    assert!(destination.join("crates/my-app-core/src/lib.rs").exists());
    assert!(destination.join("crates/my-app/src/lib.rs").exists());
    assert!(destination.join("crates/my-app/AGENTS.md").exists());
    assert!(destination.join("crates/my-app-http/src/lib.rs").exists());
    assert!(destination.join("crates/my-app-http/AGENTS.md").exists());
    assert!(destination.join("crates/my-app-db/src/lib.rs").exists());
    assert!(destination.join("crates/my-app-db/AGENTS.md").exists());
    assert!(
        destination
            .join("crates/my-app-test-support/src/lib.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/AGENTS.md")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/src/app.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/src/http.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/src/responses.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/src/db.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/tests/http.rs")
            .exists()
    );
    assert!(destination.join("web/package.json").exists());
    assert!(destination.join("landing/astro.config.mjs").exists());
    assert!(destination.join("admin-panel/package.json").exists());
    let web_package = fs::read_to_string(destination.join("web/package.json")).unwrap();
    assert!(web_package.contains(r#""dev": "bun install && vite""#));
    let web_vite_config = fs::read_to_string(destination.join("web/vite.config.ts")).unwrap();
    assert!(web_vite_config.contains("const devPort = Number(process.env.PORT);"));
    assert!(web_vite_config.contains("process.env.API_ORIGIN"));
    assert!(web_vite_config.contains("process.env.JIG_DEV_API_ORIGIN"));
    assert!(web_vite_config.contains(r#""http://api.my-app.localhost:1355""#));
    assert!(web_vite_config.contains(r#""/api""#));
    assert!(web_vite_config.contains(r#"target: apiOrigin"#));
    assert!(web_vite_config.contains(r#"host: "127.0.0.1""#));
    assert!(web_vite_config.contains("clientPort: devPort"));
    let landing_package = fs::read_to_string(destination.join("landing/package.json")).unwrap();
    assert!(landing_package.contains(
        r#""dev": "bun install && astro dev --host ${HOST:-127.0.0.1} --port ${PORT:-4321}""#
    ));

    let api_main = fs::read_to_string(destination.join("apps/my-app-api/src/main.rs")).unwrap();
    assert!(api_main.contains("use anyhow::Context;"));
    assert!(api_main.contains("use my_app::AppConfig;"));
    assert!(api_main.contains("load_dotenv();"));
    assert!(api_main.contains("warning: failed to load .env"));
    assert!(api_main.contains("let bound_addr = listener"));
    assert!(api_main.contains("Failed to read API listener address after bind"));
    assert!(api_main.contains("tracing::info!(%bound_addr, \"listening\")"));
    assert!(api_main.contains("my_app_http::router"));
    assert!(api_main.contains("AppConfig::from_env()"));
    assert!(api_main.contains("AppState::from_config(config)"));
    assert!(api_main.contains("install_panic_hook"));
    assert!(api_main.contains("tracing::error!(error = ?error, \"API server failed\")"));
    assert!(api_main.contains("Failed to bind API listener"));
    assert!(api_main.contains("API server exited with an error"));
    assert!(api_main.contains("SignalKind::terminate"));
    assert!(api_main.contains("failed to listen for Ctrl-C"));
    let jig_toml = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(jig_toml.contains("[[dev.apps]]\nname = \"api\""));
    assert!(jig_toml.contains("kind = \"env-port\""));
    assert!(!jig_toml.contains("proxy = false"));
    assert!(
        jig_toml
            .contains("command = \"BIND_ADDR=\\\"${HOST}:${PORT}\\\" cargo run -p my-app-api\"")
    );
    assert!(!jig_toml.contains("port = 3000"));
    assert_eq!(
        fs::read_to_string(destination.join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=my_app=info,tower_http=info\nDATABASE_URL=postgres://postgres:postgres@localhost:5432/my_app_dev\n"
    );
    let workspace_cargo = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(workspace_cargo.contains("dotenvy = \"0.15\""));
    let api_cargo = fs::read_to_string(destination.join("apps/my-app-api/Cargo.toml")).unwrap();
    assert!(api_cargo.contains("dotenvy.workspace = true"));
    let app_lib = fs::read_to_string(destination.join("crates/my-app/src/lib.rs")).unwrap();
    assert!(app_lib.contains("pub struct AppConfig"));
    assert!(app_lib.contains("pub fn from_env() -> Result<Self>"));
    assert!(app_lib.contains("DATABASE_URL is required when the db feature is enabled"));
    assert!(app_lib.contains("pub async fn from_config(config: AppConfig) -> Result<Self>"));
    assert!(app_lib.contains("pub fn new_with_version(version: impl Into<String>)"));
    assert!(app_lib.contains("pub fn version(&self) -> &AppVersion"));
    assert!(app_lib.contains("pub fn is_ready(&self) -> bool"));
    assert!(!app_lib.contains("use axum::"));
    assert!(!app_lib.contains("pub fn router"));
    let http_lib = fs::read_to_string(destination.join("crates/my-app-http/src/lib.rs")).unwrap();
    assert!(http_lib.contains("pub fn router(state: AppState) -> Router"));
    assert!(http_lib.contains("TraceLayer::new_for_http()"));
    assert!(http_lib.contains("SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid)"));
    assert!(http_lib.contains(r#".route("/health/live", get(live))"#));
    assert!(http_lib.contains(r#".route("/health/ready", get(ready))"#));
    assert!(http_lib.contains(r#".route("/api/version", get(version))"#));
    let test_support_cargo =
        fs::read_to_string(destination.join("crates/my-app-test-support/Cargo.toml")).unwrap();
    assert!(test_support_cargo.contains(r#"my-app = { path = "../my-app""#));
    assert!(test_support_cargo.contains(r#"my-app-http = { path = "../my-app-http""#));
    assert!(test_support_cargo.contains(r#"tower = { workspace = true, features = ["util"] }"#));
    let test_support_app =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/app.rs")).unwrap();
    assert!(test_support_app.contains("pub struct TestApp"));
    assert!(test_support_app.contains(".oneshot(request)"));
    let test_support_response =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/responses.rs"))
            .unwrap();
    assert!(test_support_response.contains("pub struct TestResponse"));
    assert!(test_support_response.contains("failed to decode response JSON"));
    assert!(test_support_response.contains("pub fn assert_error"));
    let test_support_http_test =
        fs::read_to_string(destination.join("crates/my-app-test-support/tests/http.rs")).unwrap();
    assert!(test_support_http_test.contains("use my_app_test_support::TestApp;"));
    assert!(test_support_http_test.contains("async fn health_returns_ok()"));
    assert!(test_support_http_test.contains("async fn readiness_reflects_state()"));
    assert!(test_support_http_test.contains("StatusCode::SERVICE_UNAVAILABLE"));
    assert!(test_support_http_test.contains("async fn responses_include_request_id()"));
    assert!(test_support_http_test.contains("async fn version_returns_json()"));
    let db_lib = fs::read_to_string(destination.join("crates/my-app-db/src/lib.rs")).unwrap();
    assert!(db_lib.contains("PgPool"));
    assert!(db_lib.contains("DEFAULT_DB_TIMEOUT"));
    assert!(db_lib.contains("connect_with_timeout"));
    assert!(db_lib.contains("migrate_with_timeout"));
    let test_support_db =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/db.rs")).unwrap();
    assert!(test_support_db.contains("pub struct DatabaseTestConfig"));
    assert!(test_support_db.contains("validate_test_database_name"));
    let http_agents = fs::read_to_string(destination.join("crates/my-app-http/AGENTS.md")).unwrap();
    assert!(http_agents.contains("routes, handlers, middleware, extractors, and HTTP DTOs"));
    let app_agents = fs::read_to_string(destination.join("crates/my-app/AGENTS.md")).unwrap();
    assert!(app_agents.contains("Parse environment configuration once at startup"));

    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"my-app\""));
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("rust_sqlx_metadata_dir = \".sqlx\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(answers.contains("rust_crate_roots = [\"apps\", \"crates\"]"));
    assert!(answers.contains("web_package_manager = \"bun\""));
    assert!(answers.contains("bootstrap_command = \"if [ -f Cargo.toml ]; then cargo fetch;"));
    assert!(answers.contains("&& (cd web && bun install)"));
    assert!(answers.contains("&& (cd landing && bun install)"));
    assert!(answers.contains("&& (cd admin-panel && bun install)"));
    assert!(answers.contains("name = \"web\""));
    assert!(answers.contains("dir = \"landing\""));
    assert!(answers.contains("kind = \"env-port\""));
    assert!(answers.contains("name = \"admin-panel\""));
}

#[test]
fn scaffold_options_require_preset() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: None,
            db: Some(ScaffoldDb::Sqlite),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold options require --preset rust-react"));
}

#[test]
fn run_init_rejects_invalid_frontend_package_names_before_writes() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![ScaffoldFrontend {
                name: "-".into(),
                kind: ScaffoldFrontendKind::Spa,
            }],
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold frontend name must contain"));
    assert!(!destination.exists());
}

#[test]
fn scaffold_defaults_to_web_frontend_and_no_db() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();

    assert_eq!(report["db"], "none");
    assert_eq!(report["frontends"][0]["name"], "web");
    assert_eq!(report["frontends"][0]["kind"], "spa");
    assert!(temp.path().join("web/package.json").exists());
    let has_db_crate = fs::read_dir(temp.path().join("crates"))
        .unwrap()
        .any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with("-db")
        });
    assert!(!has_db_crate);
    let cargo_toml = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert!(!cargo_toml.contains("sqlx ="));
    assert!(cargo_toml.contains("\"signal\", \"time\""));
    assert!(cargo_toml.ends_with('\n'));
    let env_example = fs::read_to_string(temp.path().join(".env.example")).unwrap();
    assert!(env_example.starts_with("BIND_ADDR=127.0.0.1:3000\nRUST_LOG="));
    assert!(env_example.ends_with("=info,tower_http=info\n"));
    assert!(!env_example.contains("DATABASE_URL"));
    assert_eq!(env_example.lines().count(), 2);
}

#[test]
fn scaffold_db_defaults_set_sqlx_metadata_and_disable_schema_dump() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    assert_eq!(answers.rust_sqlx_metadata_dir.as_deref(), Some(".sqlx"));
    assert_eq!(answers.schema_dump_enabled, Some(false));
}

#[test]
fn scaffold_bootstrap_command_uses_configured_frontend_package_managers() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
            ],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    for (package_manager, install_command) in [
        ("bun", "bun install"),
        ("npm", "npm install"),
        ("pnpm", "pnpm install"),
        ("yarn", "yarn install"),
    ] {
        let mut answers = AnswerOpts {
            web_package_manager: Some(package_manager.into()),
            ..AnswerOpts::default()
        };
        plan.apply_answer_defaults(&mut answers);
        let bootstrap_command = answers.bootstrap_command.unwrap();
        assert!(bootstrap_command.contains(&format!("(cd web && {install_command})")));
        assert!(bootstrap_command.contains(&format!("(cd landing && {install_command})")));
    }

    let mut default_answers = AnswerOpts::default();
    plan.apply_answer_defaults(&mut default_answers);
    assert_eq!(default_answers.web_package_manager.as_deref(), Some("bun"));
    assert!(
        default_answers
            .bootstrap_command
            .unwrap()
            .contains("(cd web && bun install)")
    );
}

#[test]
fn scaffold_frontend_dev_scripts_install_dependencies_before_launch() {
    for (package_manager, install_command) in [
        ("bun", "bun install"),
        ("npm", "npm install"),
        ("pnpm", "pnpm install"),
        ("yarn", "yarn install"),
    ] {
        let temp = tempdir().unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: vec![
                    parse_scaffold_frontend("web").unwrap(),
                    parse_scaffold_frontend("landing").unwrap(),
                ],
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some("demo".into()),
                web_package_manager: Some(package_manager.into()),
                ..AnswerOpts::default()
            },
            temp.path(),
        )
        .unwrap()
        .unwrap();

        plan.write(temp.path(), false).unwrap();

        let web_package = fs::read_to_string(temp.path().join("web/package.json")).unwrap();
        assert!(
            web_package.contains(&format!(r#""dev": "{install_command} && vite""#)),
            "missing Vite dev install command for {package_manager}"
        );
        let landing_package = fs::read_to_string(temp.path().join("landing/package.json")).unwrap();
        assert!(
            landing_package.contains(&format!(
                r#""dev": "{install_command} && astro dev --host ${{HOST:-127.0.0.1}} --port ${{PORT:-4321}}""#
            )),
            "missing Astro dev install command for {package_manager}"
        );
    }
}

#[test]
fn scaffold_uses_existing_frontend_app_kind() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            frontend_apps: vec![
                FrontendApp {
                    name: "docs".into(),
                    dir: "docs-site".into(),
                    coverage_threshold: 0,
                    kind: "env-port".into(),
                },
                FrontendApp {
                    name: "marketing".into(),
                    dir: "marketing".into(),
                    coverage_threshold: 0,
                    kind: "vite".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();
    assert_eq!(report["frontends"][0]["kind"], "astro");
    assert_eq!(report["frontends"][1]["kind"], "spa");
    assert!(temp.path().join("docs-site/astro.config.mjs").exists());
    assert!(temp.path().join("marketing/vite.config.ts").exists());

    let mut answers = AnswerOpts::default();
    plan.apply_answer_defaults(&mut answers);
    assert_eq!(answers.frontend_apps[0].name, "docs");
    assert_eq!(answers.frontend_apps[0].dir, "docs-site");
    assert_eq!(answers.frontend_apps[0].kind, "env-port");
    assert_eq!(answers.frontend_apps[1].name, "marketing");
    assert_eq!(answers.frontend_apps[1].dir, "marketing");
    assert_eq!(answers.frontend_apps[1].kind, "vite");
}

#[test]
fn scaffold_rejects_duplicate_and_unsafe_frontend_app_dirs() {
    let temp = tempdir().unwrap();
    let duplicate = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate.contains("Duplicate scaffold frontend 'web'"));

    let duplicate_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![
                FrontendApp {
                    name: "docs".into(),
                    dir: "shared".into(),
                    coverage_threshold: 0,
                    kind: "env-port".into(),
                },
                FrontendApp {
                    name: "marketing".into(),
                    dir: "shared".into(),
                    coverage_threshold: 0,
                    kind: "env-port".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate_dir.contains("Duplicate scaffold frontend dir 'shared'"));

    let unsafe_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "../web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(unsafe_dir.contains("Scaffold frontend dir must not contain '.' or '..'"));

    let empty_segment_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "web//app".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(empty_segment_dir.contains("must not contain empty path segments"));

    let rust_root_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "ui".into(),
                dir: "crates/ui".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(rust_root_dir.contains("uses reserved directory 'crates/ui'"));
}

#[test]
fn scaffold_rejects_mixed_scaffold_and_existing_frontend_app_inputs() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "admin".into(),
                dir: "admin".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot be combined with --frontend-app"));
}

#[test]
fn scaffold_rejects_frontend_dirs_reserved_for_rust_roots() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("apps").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("uses reserved directory 'apps'"));
}

#[test]
fn scaffold_db_rejects_explicit_sqlx_disabled_answer() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold --db requires SQLx"));
}

#[test]
fn scaffold_prefixes_repo_names_that_are_invalid_rust_crate_identifiers() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("123-type".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    assert!(plan.summary().contains("repo name app-123-type"));
    assert!(
        plan.sanitized_repo_name_note()
            .unwrap()
            .contains("normalized to 'app-123-type'")
    );
    plan.write(temp.path(), false).unwrap();

    assert!(
        temp.path()
            .join("apps/app-123-type-api/src/main.rs")
            .exists()
    );
    let main_rs =
        fs::read_to_string(temp.path().join("apps/app-123-type-api/src/main.rs")).unwrap();
    assert!(main_rs.contains("app_123_type_http::router"));
    let core_lib =
        fs::read_to_string(temp.path().join("crates/app-123-type-core/src/lib.rs")).unwrap();
    assert!(core_lib.contains("APP_NAME: &str = \"app-123-type\""));

    let mixed_case = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("MyApp".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    assert!(
        mixed_case
            .sanitized_repo_name_note()
            .unwrap()
            .contains("normalized to 'myapp'")
    );
}

#[test]
fn run_init_scaffold_writes_sanitized_repo_name_answer() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("123-type".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["scaffold"]["repo_name"], "app-123-type");
    assert_eq!(output["scaffold"]["repo_name_sanitized_from"], "123-type");
    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("requested repo name '123-type' was normalized")
    }));
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"app-123-type\""));
    assert!(
        destination
            .join("apps/app-123-type-api/src/main.rs")
            .exists()
    );
}

#[test]
fn scaffold_sqlite_branch_generates_sqlite_db_helper() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Sqlite),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            rust_migration_dir: Some("db/migrations".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();

    assert_eq!(report["db"], "sqlite");
    let cargo_toml = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("\"sqlite\""));
    assert!(cargo_toml.contains("\"signal\", \"time\""));
    assert!(cargo_toml.ends_with('\n'));
    assert_eq!(
        fs::read_to_string(temp.path().join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=demo=info,tower_http=info\nDATABASE_URL=sqlite:demo.db\n"
    );
    let db_cargo = fs::read_to_string(temp.path().join("crates/demo-db/Cargo.toml")).unwrap();
    assert!(db_cargo.contains("anyhow.workspace = true"));
    assert!(db_cargo.contains("tokio.workspace = true"));
    let db_lib = fs::read_to_string(temp.path().join("crates/demo-db/src/lib.rs")).unwrap();
    assert!(db_lib.contains("SqlitePool"));
    assert!(db_lib.contains(r#"sqlx::migrate!("../../db/migrations")"#));
    assert!(db_lib.contains("DEFAULT_DB_TIMEOUT"));
    assert!(db_lib.contains("connect_with_timeout"));
    assert!(db_lib.contains("migrate_with_timeout"));
    assert!(temp.path().join("db/migrations/.gitkeep").exists());
}

#[test]
fn scaffold_output_paths_include_template_collision_candidates() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
                parse_scaffold_frontend("admin").unwrap(),
            ],
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let paths = plan.output_paths();
    for expected in [
        ".env.example",
        "Cargo.toml",
        "crates/demo-http/Cargo.toml",
        "crates/demo-http/AGENTS.md",
        "crates/demo-http/src/lib.rs",
        "crates/demo-db/Cargo.toml",
        "crates/demo-db/AGENTS.md",
        "crates/demo-db/src/lib.rs",
        "crates/demo/AGENTS.md",
        "crates/demo-test-support/AGENTS.md",
        "crates/demo-test-support/src/app.rs",
        "crates/demo-test-support/src/db.rs",
        "crates/demo-test-support/tests/http.rs",
        "migrations/.gitkeep",
        "web/package.json",
        "web/src/App.tsx",
        "landing/package.json",
        "landing/src/pages/index.astro",
        "admin-panel/package.json",
        "admin-panel/src/App.tsx",
    ] {
        assert!(
            paths.iter().any(|path| path == Path::new(expected)),
            "missing output path {expected}"
        );
    }
}

#[test]
fn scaffold_rejects_unsupported_package_manager_before_scripts_render() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            web_package_manager: Some("cargo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported web_package_manager 'cargo'"));
}

#[test]
fn scaffold_generated_rust_workspace_has_valid_cargo_metadata() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    plan.write(temp.path(), false).unwrap();

    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo metadata failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let package_names = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| package["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    for expected in [
        "demo",
        "demo-api",
        "demo-core",
        "demo-db",
        "demo-http",
        "demo-test-support",
    ] {
        assert!(
            package_names.contains(&expected),
            "missing package {expected}"
        );
    }
}

#[test]
fn scaffold_rejects_conflicting_file_unless_forced_and_reports_rerun() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();
    fs::write(temp.path().join("Cargo.toml"), "project-owned\n").unwrap();

    let error = plan.write(temp.path(), false).unwrap_err().to_string();
    assert!(error.contains("already exist and differ"));
    assert!(error.contains("pass --force"));

    let preflight = tempdir().unwrap();
    fs::write(preflight.path().join("Cargo.toml"), "project-owned\n").unwrap();
    let error = plan.write(preflight.path(), false).unwrap_err().to_string();
    assert!(error.contains("Cargo.toml"));
    assert!(
        !preflight.path().join("web/package.json").exists(),
        "scaffold conflict preflight should fail before writing later files"
    );

    let forced = plan.write(temp.path(), true).unwrap();
    assert!(
        forced["files_modified"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "Cargo.toml")
    );
    assert_ne!(
        fs::read_to_string(temp.path().join("Cargo.toml")).unwrap(),
        "project-owned\n"
    );

    let rerun = plan.write(temp.path(), false).unwrap();
    assert!(
        rerun["files_unchanged"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "Cargo.toml")
    );
}

#[test]
fn adopt_defaults_to_tooling_only_when_sqlx_answers_are_omitted() {
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
        write: true,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert!(
        output["detection_report"]["summary"]
            .as_str()
            .unwrap()
            .contains("no Rust workspace, no SQLx")
    );
    assert!(
        !output["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| { note.as_str().unwrap().contains("tooling-only profile") })
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"repo\""));
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!repo.join(".github/workflows/webapp-checks.yml").exists());
    assert!(!repo.join("scripts/check-webapps.sh").exists());
    assert!(!repo.join("scripts/check-webapp-scripts.mjs").exists());
    assert!(!repo.join("scripts/enforce-coverage.js").exists());
    assert!(!repo.join("scripts/enforce-coverage.cjs").exists());
    assert!(
        !output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".github/workflows/webapp-checks.yml")
    );
    assert!(
        output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".github/workflows/webapp-checks.yml")
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
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
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
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
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
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unknown [vault].unexpected"));
}

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

#[test]
fn adopt_infers_repo_shape_before_resolving_answers() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("crates/api/src")).unwrap();
    fs::create_dir_all(repo.join("migrations")).unwrap();
    fs::create_dir_all(repo.join(".sqlx")).unwrap();
    fs::create_dir_all(repo.join("web")).unwrap();
    fs::create_dir_all(repo.join(".github/workflows")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]

[workspace.dependencies]
sqlx = "0.8"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("crates/api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"

[dependencies]
sqlx = { workspace = true }
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/api/src/lib.rs"), "sqlx::migrate!();").unwrap();
    fs::write(repo.join("migrations/0001_init.sql"), "select 1;").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["web"]}"#,
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    fs::write(
        repo.join("web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "dev": "vite --host 127.0.0.1",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();
    fs::write(
        repo.join(".github/workflows/rust.yml"),
        "jobs:\n  test:\n    runs-on: ubuntu-24.04\n",
    )
    .unwrap();
    init_git_repo_for_test(&repo);
    git(
        &repo,
        [
            "remote",
            "add",
            "origin",
            "git@github.com:owner/inferred-demo.git",
        ],
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["detection_report"]["repo_name"], "inferred-demo");
    assert_eq!(output["detection_report"]["rust_crate_roots"][0], "crates");
    assert_eq!(output["detection_report"]["sqlx_enabled"], true);
    assert_eq!(
        output["detection_report"]["rust_migration_dir"],
        "migrations"
    );
    assert_eq!(output["detection_report"]["web_package_manager"], "pnpm");
    assert_eq!(output["detection_report"]["frontend_apps"][0]["dir"], "web");
    assert_eq!(
        output["detection_report"]["metadata"]["sqlx_enabled"]["confidence"],
        "high"
    );
    assert!(
        output["detection_report"]["metadata"]["sqlx_enabled"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source.as_str().unwrap().contains("workspace.dependencies"))
    );
    assert!(
        output["detection_report"]["metadata"]["sqlx_enabled"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source.as_str() == Some("migrations/0001_init.sql"))
    );
    assert_eq!(
        output["adoption_profile"]["detected_stack"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Rust workspace", "SQLx", "pnpm", "Vite", "GitHub Actions"]
    );
    assert_eq!(
        output["adoption_profile"]["ci_shape"]["workflow_files"][0],
        ".github/workflows/rust.yml"
    );
    assert_eq!(
        output["adoption_profile"]["ci_shape"]["generated_jig_checks_role"],
        "supplement_existing_ci"
    );
    assert!(
        !output["adoption_review"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("overrides:"))
    );
    assert!(
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "scripts/jig check sqlx")
    );
    assert!(
        !output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "scripts/jig check schema")
    );
    assert!(
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "scripts/jig check typescript-coverage")
    );
    assert!(
        output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".jig.toml")
    );
    assert!(
        !output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "scripts/check-agent-guides.sh")
    );
    assert!(
        output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "scripts/check-agent-guides.sh")
    );
    assert!(
        !output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".jig.toml")
    );
    assert!(
        output["adoption_profile"]["assumptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|assumption| assumption
                .as_str()
                .unwrap()
                .contains("online cargo sqlx prepare"))
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"inferred-demo\""));
    assert!(answers.contains("default_branch = \"main\""));
    assert!(answers.contains("ci_github_runner = \"ubuntu-24.04\""));
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_crate_roots = [\"crates\"]"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("rust_sqlx_metadata_dir = \".sqlx\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!answers.contains("schema_dump_command"));
    assert!(answers.contains("sqlx_check_command = "));
    assert!(answers.contains("cargo sqlx prepare --check"));
    assert!(answers.contains("web_package_manager = \"pnpm\""));
    assert!(answers.contains("[[frontend_apps]]"));
    assert!(answers.contains("name = \"web\""));
    assert!(answers.contains("dir = \"web\""));
    assert!(answers.contains("argv = [\"pnpm\", \"run\", \"dev\"]"));
    let generated_gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|gate| gate.as_str().unwrap())
        .collect::<Vec<_>>();
    let rendered_work_gate_tools = answers
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("tool = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .collect::<Vec<_>>();
    for tool in rendered_work_gate_tools {
        let expected = match tool {
            "jig.contract_check" => "scripts/jig check contract",
            "jig.test" => "scripts/jig check test",
            "jig.typescript_lint" => "scripts/jig check typescript-lint",
            "jig.typescript_typecheck" => "scripts/jig check typescript-typecheck",
            "jig.typescript_build" => "scripts/jig check typescript-build",
            "jig.typescript_coverage" => "scripts/jig check typescript-coverage",
            "jig.sqlx_check" => "scripts/jig check sqlx",
            "jig.schema_check" => "scripts/jig check schema",
            "jig.schema_dump" => "scripts/jig schema-dump",
            other => panic!("unmapped rendered work gate tool {other}"),
        };
        assert!(
            generated_gates.contains(&expected),
            "generated_gates missing rendered work gate command {expected}"
        );
    }
    assert!(!repo.join("crates/api/AGENTS.md").exists());
}

#[test]
fn adopt_reports_rust_crate_topology_and_skips_fixture_guides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("crates/api/src")).unwrap();
    fs::create_dir_all(repo.join("crates/util/src")).unwrap();
    fs::create_dir_all(repo.join("crates/fixtures/src")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]
"#,
    )
    .unwrap();
    fs::write(
        repo.join("crates/api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/api/src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        repo.join("crates/util/Cargo.toml"),
        r#"[package]
name = "util"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/util/src/lib.rs"), "").unwrap();
    fs::write(repo.join("crates/util/AGENTS.md"), "# util guide\n").unwrap();
    fs::write(
        repo.join("crates/fixtures/Cargo.toml"),
        r#"[package]
name = "fixtures"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/fixtures/src/lib.rs"), "").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let crates = output["adoption_profile"]["repo_topology"]["rust_crates"]
        .as_array()
        .unwrap();
    let api = crates
        .iter()
        .find(|krate| krate["dir"] == "crates/api")
        .unwrap();
    assert_eq!(api["kind"], "binary");
    assert_eq!(api["role"], "app/service");
    assert_eq!(api["guide_action"], "missing_project_owned");
    let util = crates
        .iter()
        .find(|krate| krate["dir"] == "crates/util")
        .unwrap();
    assert_eq!(util["kind"], "library");
    assert_eq!(util["role"], "support");
    assert_eq!(util["guide_action"], "existing");
    assert_eq!(util["owner_guide"], "crates/util/AGENTS.md");
    let fixtures = crates
        .iter()
        .find(|krate| krate["dir"] == "crates/fixtures")
        .unwrap();
    assert_eq!(fixtures["role"], "example/fixture/test");
    assert_eq!(fixtures["guide_action"], "skip_non_production");
    assert!(
        fixtures["guide_action_reason"]
            .as_str()
            .unwrap()
            .contains("non-production")
    );
    assert!(!repo.join("crates/api/AGENTS.md").exists());
    assert!(!repo.join("crates/fixtures/AGENTS.md").exists());
}

#[test]
fn adopt_reports_sources_for_multiple_migration_dirs() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("crates/api/migrations")).unwrap();
    fs::create_dir_all(repo.join("migrations")).unwrap();
    fs::write(
        repo.join("crates/api/migrations/0001_api.sql"),
        "select 1;\n",
    )
    .unwrap();
    fs::write(repo.join("migrations/0001_root.sql"), "select 1;\n").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(
        output["detection_report"]["rust_migration_dirs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|dir| dir.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["crates/api/migrations", "migrations"]
    );
    let sources = output["detection_report"]["metadata"]["rust_migration_dirs"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| source.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        vec![
            "crates/api/migrations/0001_api.sql",
            "migrations/0001_root.sql"
        ]
    );
    assert!(
        output["detection_report"]["metadata"]["rust_migration_dirs"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("multiple migration directories detected"))
    );
}

#[test]
fn adopt_infers_rust_wrapper_commands_and_web_tool_hints() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("crates/api/src")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]
"#,
    )
    .unwrap();
    fs::write(
        repo.join("crates/api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/api/src/lib.rs"), "").unwrap();
    fs::write(
        repo.join("Justfile"),
        r#"fmt-check:
    cargo fmt --all -- --check
clippy:
    cargo hack clippy --workspace --all-targets -- -D warnings
test:
    cargo nextest run --workspace
test-locked:
    cargo nextest run --workspace --locked
"#,
    )
    .unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{
  "private": true,
  "scripts": {
    "lint": "biome check . && eslint .",
    "test": "vitest run && playwright test",
    "build": "turbo run build",
    "graph": "nx graph"
  },
  "devDependencies": {
    "@biomejs/biome": "1.9.0",
    "@playwright/test": "1.0.0",
    "eslint": "9.0.0",
    "nx": "20.0.0",
    "turbo": "2.0.0",
    "vitest": "2.0.0"
  }
}
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["detection_report"]["rust_test_command"], "just test");
    assert_eq!(
        output["detection_report"]["metadata"]["rust_test_command"]["confidence"],
        "high"
    );
    assert!(
        output["adoption_profile"]["command_profile"]["rust"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "cargo-hack")
    );
    let web_tools = output["adoption_profile"]["command_profile"]["web"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    for expected in ["biome", "eslint", "nx", "playwright", "turbo", "vitest"] {
        assert!(web_tools.contains(&expected), "missing {expected}");
    }
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("rust_fmt_check_command = \"just fmt-check\""));
    assert!(answers.contains("rust_clippy_command = \"just clippy\""));
    assert!(answers.contains("rust_test_command = \"just test\""));
    assert!(answers.contains("rust_test_locked_command = \"just test-locked\""));
}

#[test]
fn adopt_merges_rust_wrapper_commands_across_wrapper_files() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Justfile"),
        r#"clippy:
    cargo clippy --workspace --all-targets -- -D warnings
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Makefile"),
        r#"fmt-check:
	cargo fmt --all -- --check
test:
	cargo test --workspace
test-locked:
	cargo test --workspace --locked
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(
        output["detection_report"]["rust_fmt_check_command"],
        "make fmt-check"
    );
    assert_eq!(
        output["detection_report"]["rust_clippy_command"],
        "just clippy"
    );
    assert_eq!(output["detection_report"]["rust_test_command"], "make test");
    assert_eq!(
        output["detection_report"]["rust_test_locked_command"],
        "make test-locked"
    );
    assert!(
        output["detection_report"]["metadata"]["rust_fmt_check_command"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("multiple files"))
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("rust_fmt_check_command = \"make fmt-check\""));
    assert!(answers.contains("rust_clippy_command = \"just clippy\""));
    assert!(answers.contains("rust_test_command = \"make test\""));
    assert!(answers.contains("rust_test_locked_command = \"make test-locked\""));
}

#[test]
fn adopt_infers_just_recipes_with_default_arguments() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Justfile"),
        r#"test target="all":
    cargo test --workspace {{target}}
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["detection_report"]["rust_test_command"], "just test");
}

#[test]
fn adopt_warns_when_wrapper_test_pairs_with_nextest_locked_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join(".config/nextest.toml"), "[profile.default]\n").unwrap();
    fs::write(
        repo.join("Justfile"),
        r#"test:
    cargo test --workspace
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["detection_report"]["rust_test_command"], "just test");
    assert_eq!(
        output["detection_report"]["rust_test_locked_command"],
        "cargo nextest run --workspace --locked"
    );
    assert!(
        output["detection_report"]["metadata"]["rust_test_locked_command"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("different runners"))
    );
}

#[test]
fn adopt_ignores_make_assignments_that_look_like_rust_recipes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Makefile"),
        r#"test := cargo test --workspace
fmt-check:
	cargo fmt --all -- --check
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(
        output["detection_report"]["rust_fmt_check_command"],
        "make fmt-check"
    );
    assert!(output["detection_report"]["rust_test_command"].is_null());
}

#[test]
fn adopt_infers_nextest_when_no_project_wrapper_exists() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join(".config/nextest.toml"), "[profile.default]\n").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(
        output["detection_report"]["rust_test_command"],
        "cargo nextest run --workspace"
    );
    assert_eq!(
        output["detection_report"]["rust_test_locked_command"],
        "cargo nextest run --workspace --locked"
    );
    assert_eq!(
        output["detection_report"]["metadata"]["rust_test_command"]["sources"][0],
        ".config/nextest.toml"
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("rust_test_command = \"cargo nextest run --workspace\""));
}

#[test]
fn adopt_keeps_explicit_answers_ahead_of_inference() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("web")).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
sqlx_enabled = false
web_package_manager = "yarn"
rust_test_command = "cargo test --workspace"
frontend_apps = []
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Justfile"),
        r#"test:
    cargo nextest run --workspace
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            repo_name: Some("from-cli".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("web_package_manager: inferred npm ignored"))
    );
    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("frontend_apps: inferred web ignored"))
    );
    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("rust_test_command: inferred just test ignored"))
    );

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"from-cli\""));
    assert!(answers.contains("web_package_manager = \"yarn\""));
    assert!(answers.contains("rust_test_command = \"cargo test --workspace\""));
    assert!(answers.contains("frontend_apps = []"));
    assert!(!answers.contains("[[frontend_apps]]"));
}

#[test]
fn adopt_answer_file_migration_dir_keeps_sqlx_enabled_when_inference_finds_no_sqlx() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
rust_migration_dir = "migrations"
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!answers.contains("schema_dump_command"));
}

#[test]
fn adopt_answer_file_sqlx_disabled_suppresses_inferred_migration_defaults() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("migrations")).unwrap();
    fs::write(repo.join("migrations/0001_init.sql"), "select 1;").unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
sqlx_enabled = false
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(!answers.contains("rust_migration_dir ="));
}

#[test]
fn adopt_answer_file_schema_dump_disabled_still_uses_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
schema_dump_enabled = false
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
}

#[test]
fn adopt_answer_file_schema_dump_enabled_blocks_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
schema_dump_enabled = true
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("schema_dump_enabled implies SQLx"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_cli_sqlx_metadata_dir_blocks_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_infers_root_frontend_app() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("root-web");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{
  "name": "root-web",
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("web_package_manager = \"npm\""));
    assert!(answers.contains("name = \"root-web\""));
    assert!(answers.contains("dir = \".\""));
    assert!(answers.contains("kind = \"vite\""));
    assert!(answers.contains("argv = [\"npm\", \"run\", \"dev\"]"));
}

#[test]
fn adopt_defaults_with_migration_dir_keeps_sqlx_enabled() {
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
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!answers.contains("schema_dump_command"));
}

#[test]
fn adopt_schema_dump_command_opts_into_schema_dumps() {
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
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_migration_dir: Some("migrations".into()),
            schema_dump_command: Some("scripts/custom-dump-schema.sh".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("schema_dump_enabled = true"));
    assert!(answers.contains("schema_dump_command = \"scripts/custom-dump-schema.sh\""));
}

#[test]
fn adopt_defaults_with_schema_dump_enabled_still_requires_sqlx_migration_answer() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            schema_dump_enabled: Some(true),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_no_input_without_defaults_uses_inferred_no_sqlx_profile() {
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
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}

#[test]
fn bootstrap_invocation_cwd_rejects_invalid_env_values() {
    let _guard = lock_env();
    let _relative = EnvVarGuard::set(path::INVOCATION_CWD_ENV, "relative");
    let error = path::bootstrap_invocation_cwd().unwrap_err().to_string();
    assert!(error.contains("JIG_INVOKE_CWD must be an absolute path"));
    drop(_relative);

    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing");
    let _missing = EnvVarGuard::set(path::INVOCATION_CWD_ENV, missing.as_os_str());
    let error = path::bootstrap_invocation_cwd().unwrap_err().to_string();
    assert!(error.contains("JIG_INVOKE_CWD is not a directory"));
}

#[test]
fn init_and_adopt_resolve_relative_bootstrap_paths_from_invocation_cwd() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let invocation = temp.path().join("caller");
    let other = temp.path().join("other");
    let template = invocation.join("template");
    fs::create_dir_all(&invocation).unwrap();
    fs::create_dir_all(&other).unwrap();
    copy_dir_recursive(
        &template_repo_root().join("templates"),
        &template.join("templates"),
    );
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());
    let _cwd = CurrentDirGuard::set(&other);

    run_init(InitOpts {
        path: PathBuf::from("new-repo"),
        scaffold: ScaffoldOpts::default(),
        template: Some("template".into()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();
    assert!(invocation.join("new-repo/.jig.toml").exists());

    fs::create_dir_all(invocation.join("existing-repo")).unwrap();
    run_adopt(AdoptOpts {
        path: PathBuf::from("existing-repo"),
        template: Some("template".into()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();
    assert!(invocation.join("existing-repo/.jig.toml").exists());

    run_update(UpdateOpts {
        path: PathBuf::from("existing-repo"),
        template: Some("template".into()),
        template_mode: None,
        recopy: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
}

#[test]
fn run_init_rejects_schema_dumps_when_sqlx_is_disabled() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination,
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            schema_dump_enabled: Some(true),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("schema_dump_enabled cannot be true"));
    assert!(error.contains("sqlx_enabled is false"));
}

#[test]
fn run_init_renders_empty_agent_tooling_lists_as_toml_arrays() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[agent_tooling.codex]
marketplaces = []
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("marketplaces = []"));
    let ctx = crate::context::RepoContext::load_from(&destination).unwrap();
    assert!(ctx.codex_marketplaces().is_empty());
}

#[test]
fn run_init_renders_empty_agent_tooling_plugin_lists_as_toml_arrays() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "../jig-skills"
plugins = []
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("plugins = []"));
    let ctx = crate::context::RepoContext::load_from(&destination).unwrap();
    assert_eq!(ctx.codex_marketplaces().len(), 1);
    assert!(ctx.codex_marketplaces()[0].plugins.is_empty());
}

#[test]
fn run_init_falls_back_only_for_unsupported_git_branch_flag() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"init\" ] && [ \"$2\" = \"-b\" ]; then\n  printf 'error: unknown switch `b`\\n' >&2\n  exit 129\nfi\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: destination,
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            default_branch: Some("trunk".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["git_initialized"], true);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("git init -b trunk"));
    assert!(log.contains("git init"));
    assert!(log.contains("git symbolic-ref HEAD refs/heads/trunk"));
}

#[test]
fn run_init_surfaces_git_branch_init_failures() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"init\" ] && [ \"$2\" = \"-b\" ]; then\n  printf 'fatal: repository storage is broken\\n' >&2\n  exit 1\nfi\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let error = run_init(InitOpts {
        path: temp.path().join("repo"),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
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

    assert!(error.contains("git init -b main failed"));
    assert!(error.contains("repository storage is broken"));
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("git init -b main"));
    assert!(!log.contains("git symbolic-ref HEAD refs/heads/main"));
}

#[test]
fn adopt_with_real_template_runs_destination_tasks() {
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
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let agent_map = fs::read_to_string(repo.join("agent-map.md")).unwrap();
    assert!(agent_map.contains("[crates/api](./crates/api/AGENTS.md)"));
    assert!(!repo.join("scripts/add-migration.sh").exists());
    assert!(
        !repo
            .join("scripts/check-migration-immutability.sh")
            .exists()
    );
    let launcher = fs::read_to_string(repo.join("scripts/jig")).unwrap();
    assert!(launcher.contains("cd \"$ROOT_DIR\""));
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}

#[test]
fn adopt_keeps_project_owned_makefile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(repo.join("Makefile"), "project-owned:\n\t@true\n").unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
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

    assert_eq!(
        fs::read_to_string(repo.join("Makefile")).unwrap(),
        "project-owned:\n\t@true\n"
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(!answers.contains("makefile_enabled"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(r#""contract_version": 3"#));
    assert!(contract.contains(r#""kind": "command""#));
    assert!(!contract.contains("jig.run_target"));
}

#[test]
fn adopt_appends_jig_block_to_existing_root_agents() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.md"),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n",
    )
    .unwrap();
    fs::write(
        repo.join(".gitignore"),
        "# Project ignores\nproject-owned-cache/\n",
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
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

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.starts_with("# Existing Agent Guide"));
    assert!(root_guide.contains("Keep this repo-specific guidance."));
    assert!(root_guide.contains("<!-- BEGIN JIG MANAGED BLOCK -->"));
    assert!(root_guide.contains("Use `scripts/jig` for the typed repo contract"));
    assert_eq!(
        root_guide
            .matches("<!-- BEGIN JIG MANAGED BLOCK -->")
            .count(),
        1
    );

    let gitignore = fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert!(gitignore.starts_with("# Project ignores"));
    assert!(gitignore.contains("project-owned-cache/"));
    assert!(gitignore.contains("# BEGIN JIG MANAGED BLOCK"));
    assert!(gitignore.contains("node_modules/"));
    assert_eq!(gitignore.matches("# BEGIN JIG MANAGED BLOCK").count(), 1);
}

#[cfg(unix)]
#[test]
fn adopt_refuses_to_replace_symlinked_root_agents_without_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.shared.md"),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n",
    )
    .unwrap();
    create_symlink(Path::new("AGENTS.shared.md"), &repo.join("AGENTS.md")).unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
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

    assert!(error.contains("Adopt would overwrite template-managed paths"));
    assert!(error.contains("AGENTS.md"));
    assert!(
        fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.shared.md")).unwrap(),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n"
    );

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: true,
        write: true,
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

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(
        !fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(root_guide.contains("Keep this repo-specific guidance."));
    assert!(root_guide.contains("<!-- BEGIN JIG MANAGED BLOCK -->"));
}

#[test]
fn adopt_rejects_malformed_existing_root_agents_jig_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.md"),
        "# Existing Agent Guide\n\n<!-- BEGIN JIG MANAGED BLOCK -->\nmissing end\n",
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
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

    assert!(error.contains("Malformed Jig managed block"));
}

#[test]
fn adopt_with_real_template_keeps_sqlx_files_when_enabled() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(true),
            rust_migration_dir: Some("migrations".into()),
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let agent_map = fs::read_to_string(repo.join("agent-map.md")).unwrap();
    assert!(agent_map.contains("[crates/api](./crates/api/AGENTS.md)"));
    assert!(!repo.join("scripts/add-migration.sh").exists());
    assert!(
        !repo
            .join("scripts/check-migration-immutability.sh")
            .exists()
    );
    assert!(
        !repo
            .join("scripts/check-sqlx-unchecked-non-test.sh")
            .exists()
    );
    assert!(
        !repo
            .join("scripts/generate-sqlx-unchecked-queries-todo.sh")
            .exists()
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(!answers.contains("migration_add_command"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(r#""name": "jig.migration_add""#));
    assert!(contract.contains(r#""kind": "native""#));
}

#[test]
fn adopt_with_sqlx_and_schema_dumps_disabled_hides_schema_dump_target() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(true),
            schema_dump_enabled: Some(false),
            rust_migration_dir: Some("migrations".into()),
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(!repo.join("Makefile").exists());

    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(!contract.contains("\"schema-dump\""));
    assert!(!contract.contains("jig.schema_dump"));
    assert!(!contract.contains("\"schema_check_command\""));
    assert!(!contract.contains("jig.schema_check"));

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(!answers.contains("schema_dump_command"));
    assert!(!answers.contains("schema_check_command"));
    assert!(!answers.contains("tool = \"jig.schema_check\""));
}
