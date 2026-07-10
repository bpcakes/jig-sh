use super::*;
use crate::test_env::CurrentDirGuard;
use std::collections::{BTreeMap, BTreeSet};

fn regular_file_tree_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let file_type = entry.file_type().unwrap();
            if file_type.is_dir() {
                visit(root, &path, snapshot);
            } else if file_type.is_file() {
                snapshot.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn rendered_vault_scope_id(repo: &std::path::Path) -> String {
    let text = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    let value = toml::from_str::<toml::Value>(&text).unwrap();
    value["vault"]["scope_id"].as_str().unwrap().to_string()
}

fn managed_manifest_paths(repo: &Path) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(
        &fs::read_to_string(repo.join(managed_paths::MANIFEST_PATH)).unwrap(),
    )
    .unwrap()["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().to_string())
        .collect()
}

fn add_managed_manifest_path(repo: &Path, relative: &str) {
    let path = repo.join(managed_paths::MANIFEST_PATH);
    let mut manifest =
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap();
    let paths = manifest["paths"].as_array_mut().unwrap();
    paths.push(serde_json::Value::String(relative.to_string()));
    paths.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
    )
    .unwrap();
}

fn footprint_adopt_opts(repo: &Path, template: &Path, minimal: bool, force: bool) -> AdoptOpts {
    AdoptOpts {
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force,
        write: true,
        minimal,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    }
}

fn add_project_runtime_tables(repo: &Path) {
    let path = repo.join(".jig.toml");
    let mut config = toml::from_str::<toml::Value>(&fs::read_to_string(&path).unwrap()).unwrap();
    let root = config.as_table_mut().unwrap();

    let mut commands = toml::Table::new();
    commands.insert(
        "release_command".into(),
        toml::Value::String("just release".into()),
    );
    root.insert("commands".into(), toml::Value::Table(commands));

    root.get_mut("work")
        .unwrap()
        .as_table_mut()
        .unwrap()
        .insert(
            "checks".into(),
            toml::Value::Array(vec![toml::Value::String("jig.fmt_check".into())]),
        );

    let mut workflow = toml::Table::new();
    workflow.insert("id".into(), toml::Value::String("project-status".into()));
    workflow.insert("kind".into(), toml::Value::String("noop_status".into()));
    let mut loop_config = toml::Table::new();
    loop_config.insert(
        "workflows".into(),
        toml::Value::Array(vec![toml::Value::Table(workflow)]),
    );
    root.insert("loop".into(), toml::Value::Table(loop_config));

    fs::write(path, toml::to_string_pretty(&config).unwrap()).unwrap();
}

fn assert_project_runtime_tables(config: &toml::Value) {
    assert_eq!(
        config["commands"]["release_command"].as_str(),
        Some("just release")
    );
    assert_eq!(config["work"]["checks"][0].as_str(), Some("jig.fmt_check"));
    assert_eq!(
        config["loop"]["workflows"][0]["id"].as_str(),
        Some("project-status")
    );
    assert_eq!(
        config["loop"]["workflows"][0]["kind"].as_str(),
        Some("noop_status")
    );
}

fn configure_frontend_fixture(repo: &Path) {
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage",
    "dev": "vite"
  }
}
"#,
    )
    .unwrap();
}

fn frontend_app() -> FrontendApp {
    FrontendApp {
        name: "web".into(),
        dir: "apps/web".into(),
        coverage_threshold: 80,
        kind: "vite".into(),
    }
}

const WEB_HARNESS_PATHS: &[&str] = &[
    ".github/workflows/webapp-checks.yml",
    "scripts/check-webapp-scripts.mjs",
    "scripts/check-webapps.sh",
    "scripts/enforce-coverage.cjs",
];

fn write_project_sentinels(repo: &Path, paths: &[&str]) {
    for relative in paths {
        let path = repo.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, format!("project-owned {relative}\n")).unwrap();
    }
}

fn assert_project_sentinels(repo: &Path, paths: &[&str]) {
    for relative in paths {
        assert_eq!(
            fs::read_to_string(repo.join(relative)).unwrap(),
            format!("project-owned {relative}\n")
        );
    }
}

fn update_opts(repo: &Path, template: &Path, force: bool) -> UpdateOpts {
    UpdateOpts {
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: None,
        recopy: true,
        force,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    }
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
        minimal_footprint: false,
        full_to_minimal_transition: false,
        render_preview: initial_copy::AdoptionRenderPreview::default(),
        apply_report: sync::ApplyRenderReport::default(),
        notes: Vec::new(),
    };

    let steps = initial_next_steps(InitialCommand::Adopt, &destination, &result);
    let command_report = initial_command_report(&result);

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
    assert!(
        command_report
            .iter()
            .all(|command| !command.contains("run jig ") && !command.contains("through jig "))
    );
    assert!(
        command_report
            .iter()
            .all(|command| command.contains("scripts/jig"))
    );

    let notes = initial_notes(Vec::new(), true, None, false);
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
            minimal_footprint: false,
            full_to_minimal_transition: false,
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
            minimal_footprint: false,
            full_to_minimal_transition: false,
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
            minimal_footprint: false,
            full_to_minimal_transition: false,
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
    let no_bootstrap_report = initial_command_report(&initial_copy::BootstrapCopyResult {
        default_branch: Some("main".into()),
        bootstrap_command_configured: false,
        frontend_apps_configured: false,
        dev_apps_configured: false,
        codex_skills_configured: false,
        sqlx_enabled: false,
        schema_dump_enabled: false,
        minimal_footprint: true,
        full_to_minimal_transition: false,
        render_preview: initial_copy::AdoptionRenderPreview::default(),
        apply_report: sync::ApplyRenderReport::default(),
        notes: Vec::new(),
    });
    assert_eq!(
        no_bootstrap_report[0],
        "bootstrap_command not configured; skip jig bootstrap"
    );
    assert!(
        no_bootstrap_report
            .iter()
            .all(|command| !command.contains("scripts/jig"))
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
        active_paths: BTreeSet::from([PathBuf::from("scripts/jig")]),
        retirement_paths: BTreeSet::new(),
    };
    let report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: true,
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
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
fn apply_staged_render_writes_the_managed_path_manifest_last() {
    use std::collections::BTreeSet;

    let staged_root = tempdir().unwrap();
    let rendered_destination = staged_root.path().join("rendered");
    let destination = tempdir().unwrap();
    fs::create_dir_all(rendered_destination.join(".agent")).unwrap();
    fs::write(rendered_destination.join("z-active"), "active\n").unwrap();
    fs::write(
        rendered_destination.join(managed_paths::MANIFEST_PATH),
        "manifest\n",
    )
    .unwrap();
    let staged = staged_render::StagedRender {
        _root: staged_root,
        destination: rendered_destination,
        active_paths: BTreeSet::from([
            PathBuf::from(managed_paths::MANIFEST_PATH),
            PathBuf::from("z-active"),
        ]),
        retirement_paths: BTreeSet::new(),
    };

    let report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: true,
            allow_answers_overwrite: false,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            dry_run: false,
            backup_root: None,
            conflict_message: "conflict",
            progress: CliProgress::new("test"),
        },
    )
    .unwrap();

    assert_eq!(
        report.files_created,
        vec!["z-active", managed_paths::MANIFEST_PATH]
    );
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
        active_paths: BTreeSet::from([PathBuf::from("AGENTS.md")]),
        retirement_paths: BTreeSet::new(),
    };
    let report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: true,
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
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
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
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
        active_paths: BTreeSet::from([PathBuf::from("AGENTS.md")]),
        retirement_paths: BTreeSet::new(),
    };
    let report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: false,
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
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

#[test]
fn apply_staged_render_hard_fails_on_blocking_ancestors_before_preview_or_write() {
    for (force, dry_run) in [(false, true), (true, true), (true, false)] {
        let staged_root = tempdir().unwrap();
        let rendered_destination = staged_root.path().join("rendered");
        let destination = tempdir().unwrap();
        fs::create_dir_all(rendered_destination.join("blocked")).unwrap();
        fs::write(rendered_destination.join("a-safe"), "new\n").unwrap();
        fs::write(rendered_destination.join("blocked/file"), "new\n").unwrap();
        fs::write(destination.path().join("a-safe"), "original\n").unwrap();
        fs::write(destination.path().join("blocked"), "blocking\n").unwrap();
        let staged = staged_render::StagedRender {
            _root: staged_root,
            destination: rendered_destination,
            active_paths: BTreeSet::from([PathBuf::from("a-safe"), PathBuf::from("blocked/file")]),
            retirement_paths: BTreeSet::new(),
        };

        let error = apply_staged_render(
            &staged,
            destination.path(),
            ApplyRenderOptions {
                force,
                dry_run,
                allow_answers_overwrite: false,
                allow_contract_overwrite: false,
                allow_manifest_overwrite: false,
                backup_root: None,
                conflict_message: "conflict",
                progress: CliProgress::new("test"),
            },
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("is not a directory"), "{error}");
        assert_eq!(
            fs::read_to_string(destination.path().join("a-safe")).unwrap(),
            "original\n"
        );
        assert_eq!(
            fs::read_to_string(destination.path().join("blocked")).unwrap(),
            "blocking\n"
        );
    }
}

#[test]
fn apply_staged_render_rejects_reserved_git_metadata_aliases_before_any_operation() {
    for alias in [
        ".GiT. . /config",
        "GIT~1/config",
        ".git::$INDEX_ALLOCATION",
        ".git...:alternate-stream",
        ".g\u{200c}it/config",
        "vendor\\.GiT...\\config",
    ] {
        for (operation, force, dry_run) in [
            ("active", false, true),
            ("active", true, true),
            ("active", false, false),
            ("active", true, false),
            ("retirement", false, true),
            ("retirement", true, true),
            ("retirement", false, false),
            ("retirement", true, false),
        ] {
            let staged_root = tempdir().unwrap();
            let rendered_destination = staged_root.path().join("rendered");
            let destination = tempdir().unwrap();
            fs::create_dir_all(&rendered_destination).unwrap();
            fs::write(rendered_destination.join("a-safe"), "new\n").unwrap();
            fs::write(destination.path().join("a-safe"), "original\n").unwrap();
            fs::create_dir(destination.path().join(".git")).unwrap();
            fs::write(destination.path().join(".git/config"), "git metadata\n").unwrap();

            let mut active_paths = BTreeSet::from([PathBuf::from("a-safe")]);
            let mut retirement_paths = BTreeSet::new();
            if operation == "active" {
                active_paths.insert(PathBuf::from(alias));
            } else {
                retirement_paths.insert(PathBuf::from(alias));
            }
            let staged = staged_render::StagedRender {
                _root: staged_root,
                destination: rendered_destination,
                active_paths,
                retirement_paths,
            };

            let error = apply_staged_render(
                &staged,
                destination.path(),
                ApplyRenderOptions {
                    force,
                    dry_run,
                    allow_answers_overwrite: false,
                    allow_contract_overwrite: false,
                    allow_manifest_overwrite: false,
                    backup_root: None,
                    conflict_message: "re-run with --force",
                    progress: CliProgress::new("test"),
                },
            )
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("reserved Git metadata component"),
                "{alias}/{operation}/{force}/{dry_run}: {error}"
            );
            assert!(
                !error.contains("re-run with --force"),
                "{alias}/{operation}/{force}/{dry_run}: {error}"
            );
            assert_eq!(
                fs::read_to_string(destination.path().join("a-safe")).unwrap(),
                "original\n",
                "{alias}/{operation}/{force}/{dry_run} applied an earlier managed path"
            );
            assert_eq!(
                fs::read_to_string(destination.path().join(".git/config")).unwrap(),
                "git metadata\n",
                "{alias}/{operation}/{force}/{dry_run} changed Git metadata"
            );
        }
    }
}

#[test]
fn apply_staged_render_rejects_active_and_retired_directory_leaves_before_any_operation() {
    for (operation, force, dry_run) in [
        ("active", false, true),
        ("active", true, true),
        ("active", false, false),
        ("active", true, false),
        ("retirement", false, true),
        ("retirement", true, true),
        ("retirement", false, false),
        ("retirement", true, false),
    ] {
        let staged_root = tempdir().unwrap();
        let rendered_destination = staged_root.path().join("rendered");
        let destination = tempdir().unwrap();
        fs::create_dir_all(&rendered_destination).unwrap();
        fs::write(rendered_destination.join("a-safe"), "new\n").unwrap();
        fs::write(destination.path().join("a-safe"), "original\n").unwrap();
        fs::create_dir(destination.path().join("z-directory")).unwrap();
        fs::write(
            destination.path().join("z-directory/sentinel"),
            "preserved\n",
        )
        .unwrap();

        let mut active_paths = BTreeSet::from([PathBuf::from("a-safe")]);
        let mut retirement_paths = BTreeSet::new();
        if operation == "active" {
            fs::write(rendered_destination.join("z-directory"), "rendered\n").unwrap();
            active_paths.insert(PathBuf::from("z-directory"));
        } else {
            retirement_paths.insert(PathBuf::from("z-directory"));
        }
        let staged = staged_render::StagedRender {
            _root: staged_root,
            destination: rendered_destination,
            active_paths,
            retirement_paths,
        };

        let error = apply_staged_render(
            &staged,
            destination.path(),
            ApplyRenderOptions {
                force,
                dry_run,
                allow_answers_overwrite: false,
                allow_contract_overwrite: false,
                allow_manifest_overwrite: false,
                backup_root: None,
                conflict_message: "re-run with --force",
                progress: CliProgress::new("test"),
            },
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("destination leaf"), "{operation}: {error}");
        assert!(error.contains("is a directory"), "{operation}: {error}");
        assert!(
            !error.contains("re-run with --force"),
            "{operation}: {error}"
        );
        assert_eq!(
            fs::read_to_string(destination.path().join("a-safe")).unwrap(),
            "original\n",
            "{operation}/{force}/{dry_run} applied an earlier managed path"
        );
        assert_eq!(
            fs::read_to_string(destination.path().join("z-directory/sentinel")).unwrap(),
            "preserved\n",
            "{operation}/{force}/{dry_run} changed the directory leaf"
        );
    }
}

#[cfg(unix)]
#[test]
fn apply_staged_render_retires_leaf_symlink_without_touching_its_target() {
    let staged_root = tempdir().unwrap();
    let rendered_destination = staged_root.path().join("rendered");
    let destination = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::create_dir_all(&rendered_destination).unwrap();
    fs::write(outside.path().join("target"), "outside\n").unwrap();
    create_symlink(
        &outside.path().join("target"),
        &destination.path().join("retired"),
    )
    .unwrap();
    let staged = staged_render::StagedRender {
        _root: staged_root,
        destination: rendered_destination,
        active_paths: BTreeSet::new(),
        retirement_paths: BTreeSet::from([PathBuf::from("retired")]),
    };

    let report = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: true,
            dry_run: false,
            allow_answers_overwrite: false,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            backup_root: None,
            conflict_message: "conflict",
            progress: CliProgress::new("test"),
        },
    )
    .unwrap();

    assert_eq!(report.files_removed, vec!["retired"]);
    assert!(fs::symlink_metadata(destination.path().join("retired")).is_err());
    assert_eq!(
        fs::read_to_string(outside.path().join("target")).unwrap(),
        "outside\n"
    );
}

#[cfg(unix)]
#[test]
fn apply_staged_render_rejects_unsafe_backup_leaves_before_managed_mutation() {
    for leaf_kind in ["directory", "symlink"] {
        let staged_root = tempdir().unwrap();
        let rendered_destination = staged_root.path().join("rendered");
        let destination = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::create_dir_all(&rendered_destination).unwrap();
        fs::write(rendered_destination.join("managed"), "new\n").unwrap();
        fs::write(destination.path().join("managed"), "original\n").unwrap();
        fs::create_dir(destination.path().join("backups")).unwrap();
        let backup_leaf = destination.path().join("backups/managed");
        if leaf_kind == "directory" {
            fs::create_dir(&backup_leaf).unwrap();
            fs::write(backup_leaf.join("sentinel"), "preserved\n").unwrap();
        } else {
            fs::write(outside.path().join("target"), "outside\n").unwrap();
            create_symlink(&outside.path().join("target"), &backup_leaf).unwrap();
        }
        let staged = staged_render::StagedRender {
            _root: staged_root,
            destination: rendered_destination,
            active_paths: BTreeSet::from([PathBuf::from("managed")]),
            retirement_paths: BTreeSet::new(),
        };

        let error = apply_staged_render(
            &staged,
            destination.path(),
            ApplyRenderOptions {
                force: true,
                dry_run: false,
                allow_answers_overwrite: false,
                allow_contract_overwrite: false,
                allow_manifest_overwrite: false,
                backup_root: Some(&destination.path().join("backups")),
                conflict_message: "conflict",
                progress: CliProgress::new("test"),
            },
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("backup") || error.contains("backups"),
            "{error}"
        );
        assert_eq!(
            fs::read_to_string(destination.path().join("managed")).unwrap(),
            "original\n"
        );
        if leaf_kind == "directory" {
            assert_eq!(
                fs::read_to_string(backup_leaf.join("sentinel")).unwrap(),
                "preserved\n"
            );
        } else {
            assert_eq!(
                fs::read_to_string(outside.path().join("target")).unwrap(),
                "outside\n"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn apply_staged_render_rejects_unsafe_backup_ancestors_before_managed_mutation() {
    let staged_root = tempdir().unwrap();
    let rendered_destination = staged_root.path().join("rendered");
    let destination = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::create_dir_all(&rendered_destination).unwrap();
    fs::write(rendered_destination.join("managed"), "new\n").unwrap();
    fs::write(destination.path().join("managed"), "original\n").unwrap();
    create_symlink(outside.path(), &destination.path().join("backups")).unwrap();
    let staged = staged_render::StagedRender {
        _root: staged_root,
        destination: rendered_destination,
        active_paths: BTreeSet::from([PathBuf::from("managed")]),
        retirement_paths: BTreeSet::new(),
    };
    let backup_root = destination.path().join("backups/run");

    let error = apply_staged_render(
        &staged,
        destination.path(),
        ApplyRenderOptions {
            force: true,
            dry_run: false,
            allow_answers_overwrite: false,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            backup_root: Some(&backup_root),
            conflict_message: "conflict",
            progress: CliProgress::new("test"),
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("is a symlink"), "{error}");
    assert_eq!(
        fs::read_to_string(destination.path().join("managed")).unwrap(),
        "original\n"
    );
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
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
    let manifest_paths = managed_manifest_paths(&destination);
    assert!(
        manifest_paths
            .iter()
            .any(|path| path == managed_paths::MANIFEST_PATH)
    );
    assert!(
        manifest_paths
            .iter()
            .all(|path| destination.join(path).is_file())
    );
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
        minimal: false,
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
            .is_empty()
    );
}

#[test]
fn adopt_minimal_writes_config_and_agent_scaffolding_only() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("README.md"), "project\n").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
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

    assert_eq!(output["harness_footprint"], "minimal");
    assert_eq!(output["ok"], true);
    let generated_gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap();
    assert!(
        generated_gates
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("jig "))
    );
    assert!(generated_gates.iter().any(|gate| gate == "jig bootstrap"));
    let command_report = output["render_report"]["commands_detected_or_skipped"]
        .as_array()
        .unwrap();
    assert!(
        command_report
            .iter()
            .all(|command| { !command.as_str().unwrap().contains("scripts/jig") })
    );
    assert!(command_report.iter().any(|command| {
        command
            .as_str()
            .unwrap()
            .contains("bootstrap_command configured; run jig bootstrap")
    }));
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("harness_footprint = \"minimal\""));
    assert!(repo.join(".agent/jig-contract.json").is_file());
    assert!(repo.join(".agent/PLANS.md").is_file());
    assert!(repo.join(".agent/plans/.gitkeep").is_file());
    assert!(repo.join(".agent/state/.gitkeep").is_file());
    assert!(repo.join(".agent/.cache/.gitignore").is_file());
    assert!(repo.join(managed_paths::MANIFEST_PATH).is_file());
    assert!(repo.join(".gitignore").is_file());
    assert!(repo.join(".gitattributes").is_file());
    assert!(!repo.join("scripts/jig").exists());
    assert!(!repo.join("scripts/install-jig.sh").exists());
    assert!(!repo.join(".mcp.json").exists());
    assert!(!repo.join("AGENTS.md").exists());
    assert!(!repo.join("agent-map.md").exists());
    assert!(!repo.join(".github/workflows/rust-tests.yml").exists());
    assert!(!repo.join(".github/workflows/repo-policy.yml").exists());
    assert!(!repo.join(".github/workflows/agent-map-check.yml").exists());
    let manifest_paths = managed_manifest_paths(&repo);
    assert_eq!(
        manifest_paths,
        output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        manifest_paths,
        output["render_report"]["active_managed_paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        output["render_report"]["retired_managed_paths"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(manifest_paths.windows(2).all(|paths| paths[0] < paths[1]));
    assert!(manifest_paths.iter().all(|path| repo.join(path).is_file()));
    assert!(
        manifest_paths
            .iter()
            .any(|path| path == managed_paths::MANIFEST_PATH)
    );
    assert!(manifest_paths.iter().all(|path| path != "AGENTS.md"));
    assert!(
        output["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("Minimal adoption"))
    );
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap().contains("jig loop"))
    );

    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(ctx.repo_name(), "demo");
    assert!(!ctx.required_commands().is_empty());
    assert_eq!(crate::policy::contract_check(&ctx).unwrap().exit_status, 0);

    run_update(UpdateOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        recopy: true,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    assert!(!repo.join("scripts/jig").exists());
    assert!(!repo.join("AGENTS.md").exists());
    assert!(!repo.join("agent-map.md").exists());
    let answers_after_update = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers_after_update.contains("harness_footprint = \"minimal\""));
}

#[test]
fn minimal_frontend_keeps_metadata_without_enabling_web_harness_capabilities() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);
    let mut opts = footprint_adopt_opts(&repo, template.path(), true, false);
    opts.answers.frontend_apps = vec![frontend_app()];
    opts.answers.sqlx_enabled = Some(true);
    opts.answers.rust_migration_dir = Some("migrations".into());

    let output = run_adopt(opts).unwrap();

    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(config.contains("[[frontend_apps]]"));
    assert!(config.contains("[[dev.apps]]"));
    assert!(!config.contains("typescript_lint_command"));
    assert!(!config.contains("tool = \"jig.typescript_"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(!contract.contains("typescript_"));
    assert!(contract.contains(r#""name": "jig.sqlx_check""#));
    assert!(!repo.join("scripts/check-webapps.sh").exists());
    let generated_gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap();
    assert!(
        generated_gates
            .iter()
            .all(|gate| !gate.as_str().unwrap().contains("typescript"))
    );
    assert!(generated_gates.iter().any(|gate| gate == "jig check sqlx"));
    assert!(
        generated_gates
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("jig "))
    );
    let command_report = output["render_report"]["commands_detected_or_skipped"]
        .as_array()
        .unwrap();
    assert!(
        command_report
            .iter()
            .any(|command| { command.as_str() == Some("[[dev.apps]] configured; run jig dev") })
    );
    assert!(command_report.iter().all(|command| {
        !command.as_str().unwrap().contains("scripts/jig")
            && !command.as_str().unwrap().contains("typescript")
    }));
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(ctx.frontend_apps().len(), 1);
    assert!(
        jig_features::required_contract_tools(&ctx)
            .iter()
            .all(|tool| !tool.contains("typescript"))
    );
    assert_eq!(crate::policy::contract_check(&ctx).unwrap().exit_status, 0);
}

#[test]
fn first_time_minimal_adoption_preserves_project_owned_omitted_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let mcp_contents = b"{\"mcpServers\":{\"project\":{}}}\n";
    let workflow_contents = b"name: project rust tests\n";
    let legacy_paths = [
        "scripts/check-agent-guides.sh",
        "scripts/add-migration.sh",
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];

    for force in [false, true] {
        let repo = temp.path().join(if force { "forced" } else { "normal" });
        fs::create_dir_all(repo.join(".github/workflows")).unwrap();
        fs::write(repo.join(".mcp.json"), mcp_contents).unwrap();
        fs::write(
            repo.join(".github/workflows/rust-tests.yml"),
            workflow_contents,
        )
        .unwrap();
        write_project_sentinels(&repo, &legacy_paths);

        let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, force)).unwrap();

        assert_eq!(fs::read(repo.join(".mcp.json")).unwrap(), mcp_contents);
        assert_eq!(
            fs::read(repo.join(".github/workflows/rust-tests.yml")).unwrap(),
            workflow_contents
        );
        assert_project_sentinels(&repo, &legacy_paths);
        assert!(
            !output["render_report"]["files_removed"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path == ".mcp.json" || path == ".github/workflows/rust-tests.yml")
        );
    }
}

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
        fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();
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
            fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
            "project plan notes\n",
            "{mode}: an earlier managed path changed"
        );
    }
}

#[test]
fn tampered_manifest_cannot_manage_linked_worktree_git_file() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for alias in [
        ".git",
        "GIT~1/config",
        ".git::$INDEX_ALLOCATION",
        ".g\u{200c}it/config",
        "vendor\\.GiT...\\config",
    ] {
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

            fs::write(repo.join(".git"), "gitdir: ../main/.git/worktrees/demo\n").unwrap();
            fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();
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
                "gitdir: ../main/.git/worktrees/demo\n",
                "{alias}/{mode}: linked-worktree metadata changed"
            );
            assert_eq!(
                fs::read_to_string(existing_backup.join("project-sentinel")).unwrap(),
                "backup\n",
                "{alias}/{mode}: existing backup changed"
            );
            assert_eq!(
                fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
                "project plan notes\n",
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

#[test]
fn full_without_web_preserves_project_web_paths_during_minimal_retirement() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    write_project_sentinels(&repo, WEB_HARNESS_PATHS);

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_project_sentinels(&repo, WEB_HARNESS_PATHS);
    assert!(WEB_HARNESS_PATHS.iter().all(|path| {
        !output["render_report"]["files_removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|removed| removed == *path)
    }));
}

#[test]
fn full_with_web_retires_web_paths_when_switching_to_minimal() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);
    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.frontend_apps = vec![frontend_app()];
    run_adopt(full).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"].as_table_mut().unwrap().insert(
        "release_command".into(),
        toml::Value::String("just release".into()),
    );
    config["commands"].as_table_mut().unwrap().insert(
        "typescript_lint_command".into(),
        toml::Value::String("npm run project-lint".into()),
    );
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    assert!(
        WEB_HARNESS_PATHS
            .iter()
            .all(|path| repo.join(path).is_file())
    );

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert!(
        WEB_HARNESS_PATHS
            .iter()
            .all(|path| !repo.join(path).exists())
    );
    assert!(WEB_HARNESS_PATHS.iter().all(|path| {
        output["render_report"]["files_removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|removed| removed == *path)
    }));
    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(config.contains("[[frontend_apps]]"));
    assert!(config.contains("[[dev.apps]]"));
    assert!(config.contains("typescript_lint_command = \"npm run project-lint\""));
    assert!(!config.contains("typescript_typecheck_command"));
    assert!(!config.contains("typescript_build_command"));
    assert!(!config.contains("typescript_coverage_command"));
    assert!(!config.contains("tool = \"jig.typescript_"));
    assert!(config.contains("release_command = \"just release\""));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(!contract.contains("typescript_"));
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| { !WEB_HARNESS_PATHS.contains(&path.as_str()) })
    );
}

#[test]
fn full_with_web_retires_web_paths_when_readopted_without_web() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);
    let mut with_web = footprint_adopt_opts(&repo, template.path(), false, false);
    with_web.answers.frontend_apps = vec![frontend_app()];
    run_adopt(with_web).unwrap();
    fs::remove_dir_all(repo.join("apps")).unwrap();
    fs::remove_file(repo.join("package.json")).unwrap();
    fs::remove_file(repo.join("package-lock.json")).unwrap();

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    assert!(
        WEB_HARNESS_PATHS
            .iter()
            .all(|path| !repo.join(path).exists())
    );
    assert!(WEB_HARNESS_PATHS.iter().all(|path| {
        output["render_report"]["files_removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|removed| removed == *path)
    }));
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

#[test]
fn minimal_adoption_staging_still_rejects_invalid_commands_and_tools() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let config_template = template.path().join("templates/project/.jig.toml.jinja");
    let config = fs::read_to_string(&config_template).unwrap();
    let config = config
        .lines()
        .map(|line| {
            if line.starts_with("rust_test_command = ") {
                "rust_test_command = \"  \""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&config_template, format!("{config}\n")).unwrap();
    let contract_template = template
        .path()
        .join("templates/project/.agent/jig-contract.json.jinja");
    let contract = fs::read_to_string(&contract_template).unwrap().replacen(
        "\"name\": \"jig.contract_check\"",
        "\"name\": \"jig.unsupported\"",
        1,
    );
    fs::write(&contract_template, contract).unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap_err();
    let error = format!("{error:#}");

    assert!(
        error.contains("Command key rust_test_command is empty"),
        "{error}"
    );
    assert!(
        error.contains("Unsupported native tool: jig.unsupported"),
        "{error}"
    );
    assert!(!repo.join(".jig.toml").exists());
}

#[test]
fn forced_minimal_adoption_with_invalid_prior_config_preserves_omitted_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    let mcp_contents = b"{\"projectOwned\":true}\n";
    let workflow_contents = b"name: project policy\n";
    fs::create_dir_all(repo.join(".github/workflows")).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        "harness_footprint = \"not-a-footprint\"\n",
    )
    .unwrap();
    fs::write(repo.join(".mcp.json"), mcp_contents).unwrap();
    fs::write(
        repo.join(".github/workflows/repo-policy.yml"),
        workflow_contents,
    )
    .unwrap();
    let legacy_paths = [
        "scripts/check-agent-guides.sh",
        "scripts/add-migration.sh",
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];
    write_project_sentinels(&repo, &legacy_paths);

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_eq!(fs::read(repo.join(".mcp.json")).unwrap(), mcp_contents);
    assert_eq!(
        fs::read(repo.join(".github/workflows/repo-policy.yml")).unwrap(),
        workflow_contents
    );
    assert_project_sentinels(&repo, &legacy_paths);
    assert!(
        fs::read_to_string(repo.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"minimal\"")
    );
}

#[test]
fn invalid_runtime_config_is_not_preserved_by_readoption_or_update() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for update in [false, true] {
        let repo = temp.path().join(if update { "update" } else { "readopt" });
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

        let config_path = repo.join(".jig.toml");
        let mut config =
            toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
        config
            .as_table_mut()
            .unwrap()
            .insert("commands".into(), toml::Value::String("invalid".into()));
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        assert!(crate::context::RepoContext::validate_config_file(&repo).is_err());

        if update {
            run_update(update_opts(&repo, template.path(), false)).unwrap();
        } else {
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();
        }

        let repaired =
            toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
                .unwrap();
        assert!(repaired.get("commands").is_none());
        crate::context::RepoContext::load_from(&repo).unwrap();
    }
}

#[test]
fn minimal_adoption_expands_to_full_without_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    add_project_runtime_tables(&repo);
    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    assert_eq!(output["harness_footprint"], "full");
    assert!(repo.join("scripts/jig").is_file());
    assert!(repo.join(".mcp.json").is_file());
    assert!(repo.join(".github/workflows/rust-tests.yml").is_file());
    assert!(repo.join("AGENTS.md").is_file());
    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["harness_footprint"].as_str(), Some("full"));
    assert_project_runtime_tables(&config);
    crate::context::RepoContext::load_from(&repo).unwrap();
}

#[test]
fn update_preserves_project_runtime_tables_for_minimal_and_full_harnesses() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for minimal in [true, false] {
        for force in [false, true] {
            let repo = temp.path().join(format!(
                "{}-{force}",
                if minimal { "minimal" } else { "full" }
            ));
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), minimal, false)).unwrap();
            add_project_runtime_tables(&repo);

            run_update(update_opts(&repo, template.path(), force)).unwrap();

            let config =
                toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
                    .unwrap();
            assert_project_runtime_tables(&config);
            assert_eq!(
                config["harness_footprint"].as_str(),
                Some(if minimal { "minimal" } else { "full" })
            );
            crate::context::RepoContext::load_from(&repo).unwrap();
        }
    }
}

#[test]
fn minimal_expansion_adds_generated_frontend_commands_around_project_overrides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let mut commands = toml::Table::new();
    commands.insert(
        "release_command".into(),
        toml::Value::String("just release".into()),
    );
    commands.insert(
        "typescript_lint_command".into(),
        toml::Value::String("npm run project-lint".into()),
    );
    commands.insert(
        "typescript_typecheck_command".into(),
        toml::Value::String("  ".into()),
    );
    commands.insert(
        "typescript_build_command".into(),
        toml::Value::String(String::new()),
    );
    commands.insert(
        "rust_test_command".into(),
        toml::Value::String(" \t ".into()),
    );
    config
        .as_table_mut()
        .unwrap()
        .insert("commands".into(), toml::Value::Table(commands));
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.web_package_manager = Some("npm".into());
    full.answers.frontend_apps = vec![frontend_app()];
    run_adopt(full).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(
        config["commands"]["release_command"].as_str(),
        Some("just release")
    );
    assert_eq!(
        config["commands"]["typescript_lint_command"].as_str(),
        Some("npm run project-lint")
    );
    assert_eq!(
        config["commands"]["typescript_typecheck_command"].as_str(),
        Some("scripts/check-webapps.sh typecheck")
    );
    assert_eq!(
        config["commands"]["typescript_build_command"].as_str(),
        Some("scripts/check-webapps.sh build")
    );
    assert!(config["commands"].get("rust_test_command").is_none());
    for key in [
        "typescript_lint_command",
        "typescript_typecheck_command",
        "typescript_build_command",
        "typescript_coverage_command",
    ] {
        assert!(config["commands"][key].as_str().is_some(), "missing {key}");
    }
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(crate::policy::contract_check(&ctx).unwrap().exit_status, 0);
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
        match gate["id"].as_str().unwrap() {
            "contract" => {
                gate.insert("tool".into(), toml::Value::String("jig.fmt_check".into()));
                gate.insert("required".into(), toml::Value::Boolean(false));
            }
            "tests" => {
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
        gate("contract")["tool"].as_str(),
        Some("jig.contract_check")
    );
    assert_eq!(
        gate("contract")
            .as_table()
            .unwrap()
            .get("required")
            .and_then(toml::Value::as_bool),
        None
    );
    assert_eq!(gate("tests")["tool"].as_str(), Some("jig.test"));
    assert_eq!(gate("tests")["required"].as_bool(), Some(false));
    assert_eq!(gate("project-fmt")["tool"].as_str(), Some("jig.fmt_check"));
    assert_eq!(gate("project-fmt")["required"].as_bool(), Some(false));
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
    assert_eq!(crate::policy::contract_check(&ctx).unwrap().exit_status, 0);
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
    assert_eq!(crate::policy::contract_check(&ctx).unwrap().exit_status, 0);
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
    run_adopt(full).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["repo_name"].as_str(), Some("demo"));
    assert_eq!(config["default_branch"].as_str(), Some("release"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("macos-14"));
    assert_eq!(config["sqlx_enabled"].as_bool(), Some(true));
    assert_eq!(config["rust_migration_dir"].as_str(), Some("db/migrations"));
    assert_eq!(config["harness_footprint"].as_str(), Some("full"));
    assert_project_runtime_tables(&config);

    let workflow = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    assert!(workflow.contains("runs-on: macos-14"));
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
    assert!(workflow.contains("runs-on: ubuntu-24.04"));
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
            .all(|gate| gate.as_str().unwrap().starts_with("jig "))
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
        minimal: false,
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
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("scripts/jig "))
    );
    assert!(
        output["render_report"]["commands_detected_or_skipped"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| command.as_str().unwrap().contains("scripts/jig"))
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
        !output["adoption_profile"]["retired_managed_files"]
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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
        minimal: false,
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

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(
        !fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(root_guide.contains("Keep this repo-specific guidance."));
    assert!(root_guide.contains("<!-- BEGIN JIG MANAGED BLOCK -->"));
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.shared.md")).unwrap(),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n"
    );
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
        minimal: false,
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
        minimal: false,
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
