use super::*;
use crate::test_env::CurrentDirGuard;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(unix)]
use std::process::Command;

fn numeric_semver_major_minor(version: &str) -> (u64, u64) {
    let components = version
        .split('.')
        .map(|component| component.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        components.len(),
        3,
        "generated Node versions must be exact numeric semver pins"
    );
    (components[0], components[1])
}

#[test]
fn generated_node_typings_do_not_exceed_the_runtime_floor() {
    let (runtime_major, runtime_minor) = numeric_semver_major_minor(GENERATED_NODE_VERSION);
    let (types_major, types_minor) = numeric_semver_major_minor(GENERATED_NODE_TYPES_VERSION);

    assert_eq!(
        types_major, runtime_major,
        "generated Node typings must match the runtime major"
    );
    assert!(
        types_minor <= runtime_minor,
        "generated Node typings must not expose APIs newer than the minimum runtime"
    );
}

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

#[cfg(unix)]
fn write_executable_test_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn rollback_test_init_opts(path: PathBuf, force: bool) -> InitOpts {
    InitOpts {
        path,
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("rollback-demo".into()),
            ..AnswerOpts::default()
        },
    }
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
        role: "spa".into(),
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
            role: "spa".into(),
        }
    );

    let app = parse_frontend_app("frontend:web:40:env-port").unwrap();
    assert_eq!(app.kind, "env-port");
    assert_eq!(app.role, "astro");

    let admin = parse_frontend_app("console:console:80:vite:admin").unwrap();
    assert_eq!(admin.kind, "vite");
    assert_eq!(admin.role, "admin");

    let legacy_admin = parse_frontend_app("admin-panel:admin-panel:80").unwrap();
    assert_eq!(legacy_admin.kind, "vite");
    assert_eq!(legacy_admin.role, "admin");

    let explicit_kind_legacy_admin = parse_frontend_app("admin-panel:admin-panel:80:vite").unwrap();
    assert_eq!(explicit_kind_legacy_admin.kind, "vite");
    assert_eq!(explicit_kind_legacy_admin.role, "admin");

    let explicit_marketing = parse_frontend_app("marketing:marketing:80:vite").unwrap();
    assert_eq!(explicit_marketing.kind, "vite");
    assert_eq!(explicit_marketing.role, "spa");

    let answers_marketing: FrontendApp = toml::from_str(
        r#"name = "marketing"
dir = "marketing"
coverage_threshold = 80
kind = "vite"
"#,
    )
    .unwrap();
    assert_eq!(explicit_marketing, answers_marketing);

    let answers_admin: FrontendApp = toml::from_str(
        r#"name = "admin-panel"
dir = "admin-panel"
coverage_threshold = 80
"#,
    )
    .unwrap();
    assert_eq!(legacy_admin, answers_admin);
    assert_eq!(explicit_kind_legacy_admin, answers_admin);

    for (value, expected) in [
        ("bad/name:web:40", "Invalid frontend app name"),
        ("frontend:/outside:40", "must be relative"),
        ("frontend:web:40:unknown", "Invalid frontend app kind"),
        ("frontend:web:40:vite:unknown", "Invalid frontend app role"),
    ] {
        let error = parse_frontend_app(value).unwrap_err();
        assert!(error.contains(expected), "{value}: {error}");
    }
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

    for alias in [
        "web",
        "admin",
        "admin-panel",
        "landing",
        "marketing",
        "astro",
    ] {
        assert_eq!(
            parse_scaffold_frontend(alias)
                .unwrap()
                .custom_default_name_notice(),
            None,
            "{alias}"
        );
    }

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
                role: "spa".into(),
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

    let steps = initial_next_steps(InitialCommand::Adopt, &destination, &result, false);
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
        false,
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
        false,
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
        false,
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
            init_transaction: None,
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
            init_transaction: None,
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
            init_transaction: None,
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
            init_transaction: None,
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
            init_transaction: None,
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
                init_transaction: None,
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
                    init_transaction: None,
                },
            )
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("reserved Git metadata component")
                    || error.contains("not portable to Windows"),
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
                init_transaction: None,
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
            init_transaction: None,
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
                init_transaction: None,
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
            init_transaction: None,
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
            "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nexec git \"$@\"\n",
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
    assert!(log.contains(" init -b main"));
    assert!(destination.exists());
    assert!(destination.join(".jig.toml").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("[vault]"));
    assert!(answers.contains("scope = \"repo\""));
    assert!(answers.contains("allow_global = false"));
    assert!(answers.contains(
        "CARGO=cargo SQLX_OFFLINE=false SQLX_OFFLINE_DIR='.sqlx' sqlx prepare --check --workspace -- --workspace --all-targets"
    ));
    assert!(!answers.contains("cargo sqlx prepare --check"));
    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(gitignore.contains("node_modules/"));
    assert!(gitignore.contains(".pnp.*"));
    assert!(gitignore.contains("!.yarn/patches"));
    assert!(gitignore.contains("target/"));
    assert!(gitignore.contains(".agent/.cache/*"));
    assert!(gitignore.contains(".agent/tmp/"));
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
fn run_init_explicit_harness_only_writes_no_starter_application() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

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
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["scaffold"].is_null());
    assert!(!destination.join("Cargo.toml").exists());
    assert!(!destination.join("package.json").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}

#[test]
fn run_init_rejects_minimal_answers_with_rust_react_before_writes() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repo");
    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
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
            harness_footprint: Some(HarnessFootprint::Minimal),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot combine harness_footprint = \"minimal\""));
    assert!(error.contains("Rust React scaffold"));
    assert!(!destination.exists());
}

#[test]
fn run_init_normalizes_minimal_answers_to_harness_only() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
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
            harness_footprint: Some(HarnessFootprint::Minimal),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["scaffold"].is_null());
    assert!(destination.join(".agent/jig-contract.json").is_file());
    assert!(
        fs::read_to_string(destination.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"minimal\"")
    );
    assert!(!destination.join("scripts/jig").exists());
    assert!(!destination.join("Cargo.toml").exists());
}

#[test]
fn run_init_applies_relative_answers_file_before_scaffold_defaults() {
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
    fs::write(
        invocation.join("answers.toml"),
        r#"repo_name = "file-app"
default_branch = "trunk"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf file-bootstrap"
web_package_manager = "pnpm"

[[frontend_apps]]
name = "portal"
dir = "clients/portal"
coverage_threshold = 77
kind = "vite"
role = "spa"

[dev]
[[dev.apps]]
name = "worker"
kind = "env-port"
command = "cargo run -p worker"
proxy = false
"#,
    )
    .unwrap();
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());
    let _cwd = CurrentDirGuard::set(&other);
    let destination = invocation.join("generated");

    let output = run_init(InitOpts {
        path: PathBuf::from("generated"),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some("template".into()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(PathBuf::from("answers.toml")),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(destination.join("apps/file-app-api").is_dir());
    assert!(destination.join("clients/portal/package.json").is_file());
    assert!(!destination.join("web").exists());
    assert_eq!(output["scaffold"]["frontends"][0]["name"], "portal");
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    assert!(workspace_package.contains(r#""packageManager": "pnpm@"#));
    assert!(destination.join("pnpm-workspace.yaml").is_file());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"file-app\""));
    assert!(answers.contains("default_branch = \"trunk\""));
    assert!(answers.contains("bootstrap_command = \"printf file-bootstrap\""));
    assert!(answers.contains("name = \"worker\""));
    assert!(answers.contains("command = \"cargo run -p worker\""));
    assert!(!answers.contains("[[dev.apps]]\nname = \"api\""));
    assert!(!answers.contains("cargo run -p file-app-api -- --bootstrap-database"));
    let workflow = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert!(workflow.contains("      - \"trunk\""));
    assert_eq!(
        git_stdout(&destination, ["symbolic-ref", "--short", "HEAD"]).unwrap(),
        "trunk"
    );
}

#[test]
fn run_init_cli_answers_override_answers_file_before_scaffold_defaults() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "file-app"
default_branch = "file-branch"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf file-bootstrap"
web_package_manager = "pnpm"
"#,
    )
    .unwrap();
    let destination = temp.path().join("generated");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
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
            answers_file: Some(answers_file),
            repo_name: Some("cli-app".into()),
            default_branch: Some("cli-branch".into()),
            bootstrap_command: Some("printf cli-bootstrap".into()),
            web_package_manager: Some("npm".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(destination.join("apps/cli-app-api").is_dir());
    assert!(!destination.join("apps/file-app-api").exists());
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    assert!(workspace_package.contains(r#""packageManager": "npm@"#));
    assert!(!destination.join("pnpm-workspace.yaml").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"cli-app\""));
    assert!(answers.contains("default_branch = \"cli-branch\""));
    assert!(answers.contains("bootstrap_command = \"printf cli-bootstrap\""));
    assert!(!answers.contains("printf file-bootstrap"));
    assert_eq!(
        git_stdout(&destination, ["symbolic-ref", "--short", "HEAD"]).unwrap(),
        "cli-branch"
    );
}

#[test]
fn run_init_rejects_malformed_or_conflicting_answers_before_destination_writes() {
    let temp = tempdir().unwrap();
    let malformed_answers = temp.path().join("malformed.toml");
    fs::write(&malformed_answers, "repo_name = [\n").unwrap();
    let malformed_destination = temp.path().join("malformed-repo");

    let error = run_init(InitOpts {
        path: malformed_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
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
            answers_file: Some(malformed_answers),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("Failed to parse"));
    assert!(!malformed_destination.exists());

    let conflicting_answers = temp.path().join("conflicting.toml");
    fs::write(
        &conflicting_answers,
        r#"repo_name = "demo"
sqlx_enabled = false
schema_dump_enabled = false

[[frontend_apps]]
name = "portal"
dir = "portal"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let conflicting_destination = temp.path().join("conflicting-repo");
    let error = run_init(InitOpts {
        path: conflicting_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
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
            answers_file: Some(conflicting_answers),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("cannot be combined with --frontend-app answers"));
    assert!(!conflicting_destination.exists());

    let unsafe_metadata_destination = temp.path().join("unsafe-metadata-repo");
    let error = run_init(InitOpts {
        path: unsafe_metadata_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
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
            rust_sqlx_metadata_dir: Some("../sqlx-cache".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("Scaffold SQLx metadata dir must not contain '.' or '..'"));
    assert!(!unsafe_metadata_destination.exists());

    let custom_metadata_destination = temp.path().join("custom-metadata-repo");
    let error = run_init(InitOpts {
        path: custom_metadata_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
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
            rust_sqlx_metadata_dir: Some("db/sqlx-cache".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("pin SQLx 0.8"));
    assert!(error.contains("rust_sqlx_metadata_dir = '.sqlx'"));
    assert!(error.contains("jig adopt"));
    assert!(error.contains("sqlx_check_command"));
    assert!(!custom_metadata_destination.exists());
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
        answers: AnswerOpts {
            ci_github_runner: Some("macos-14".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let next_steps = output["next_steps"].as_array().unwrap();
    let database_config = next_steps
        .iter()
        .position(|step| {
            step.as_str()
                .is_some_and(|step| step.contains("Export DATABASE_URL"))
        })
        .unwrap();
    let bootstrap = next_steps
        .iter()
        .position(|step| step.as_str() == Some("scripts/jig bootstrap"))
        .unwrap();
    assert!(database_config < bootstrap);

    let context = crate::context::RepoContext::load_from(&destination).unwrap();
    let agent_map_check = crate::policy::run_check(
        &context,
        crate::policy::PolicyCheckCommand::AgentMap(crate::policy::AgentMapInput {
            map_path: PathBuf::from("agent-map.md"),
        }),
    )
    .unwrap();
    assert_eq!(agent_map_check["ok"], true);
    assert_eq!(agent_map_check["agents"], 5);
    assert!(
        agent_map_check["missing_agents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        agent_map_check["broken_links"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let agent_guides_check =
        crate::policy::run_check(&context, crate::policy::PolicyCheckCommand::AgentGuides).unwrap();
    assert_eq!(agent_guides_check["ok"], true);
    assert_eq!(agent_guides_check["guide_count"], 4);
    assert!(
        agent_guides_check["missing_entry_ref"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    assert_eq!(output["scaffold"]["preset"], "rust-react");
    assert_eq!(output["scaffold"]["db"], "postgres");
    assert_eq!(output["scaffold"]["frontends"][0]["role"], "spa");
    assert_eq!(
        output["scaffold"]["frontends"][0]["ui"]["style"],
        "radix-nova"
    );
    assert_eq!(output["scaffold"]["frontends"][2]["role"], "admin");
    assert_eq!(
        output["scaffold"]["frontends"][2]["ui"]["cli_version"],
        "4.13.0"
    );
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
    let web_gitignore = fs::read_to_string(destination.join("web/.gitignore")).unwrap();
    assert!(web_gitignore.contains("playwright-report/"));
    assert!(web_gitignore.contains("test-results/"));
    assert!(web_gitignore.contains("blob-report/"));
    assert!(web_gitignore.contains("*.tsbuildinfo"));
    assert!(destination.join("landing/astro.config.mjs").exists());
    assert!(destination.join("admin-panel/package.json").exists());
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    let workspace_package_json: serde_json::Value =
        serde_json::from_str(&workspace_package).unwrap();
    let expected_node_engine = format!(">={GENERATED_NODE_VERSION}");
    assert!(workspace_package.contains(r#""packageManager": "bun@1.3.14""#));
    assert_eq!(
        workspace_package_json["engines"]["node"].as_str(),
        Some(expected_node_engine.as_str())
    );
    assert!(workspace_package.contains(r#""admin-panel""#));
    assert_eq!(
        fs::read_to_string(destination.join(".node-version")).unwrap(),
        format!("{GENERATED_NODE_VERSION}\n")
    );
    let web_package = fs::read_to_string(destination.join("web/package.json")).unwrap();
    let web_package_json: serde_json::Value = serde_json::from_str(&web_package).unwrap();
    assert_eq!(
        web_package_json["devDependencies"]["@types/node"].as_str(),
        Some(GENERATED_NODE_TYPES_VERSION)
    );
    assert!(web_package.contains(r#""dev": "vite""#));
    assert!(web_package.contains(r#""shadcn": "4.13.0""#));
    assert!(web_package.contains(r#""tailwindcss": "4.3.2""#));
    assert!(web_package.contains(r#""@testing-library/dom": "10.4.1""#));
    assert!(web_package.contains(r#""@playwright/test": "1.61.1""#));
    assert!(web_package.contains(r#""test:e2e": "playwright test""#));
    assert!(web_package.contains(r#""test:e2e:install": "playwright install chromium""#));
    assert!(
        web_package.contains(r#""test:e2e:install:ci": "playwright install --with-deps chromium""#)
    );
    assert!(!web_package.contains(" install && "));
    assert!(destination.join("web/src/api.ts").exists());
    assert!(destination.join("web/playwright.config.ts").exists());
    assert!(destination.join("web/e2e/app.spec.ts").exists());
    assert!(destination.join("web/tsconfig.app.json").exists());
    assert!(destination.join("web/tsconfig.node.json").exists());
    let web_tsconfig_app = fs::read_to_string(destination.join("web/tsconfig.app.json")).unwrap();
    assert!(web_tsconfig_app.contains(r#""types": ["vite/client", "vitest/globals"]"#));
    assert!(!web_tsconfig_app.contains(r#""node""#));
    assert!(web_tsconfig_app.contains(r#""include": ["src"]"#));
    let web_tsconfig_node = fs::read_to_string(destination.join("web/tsconfig.node.json")).unwrap();
    assert!(web_tsconfig_node.contains(r#""types": ["node"]"#));
    assert!(web_tsconfig_node.contains(r#""playwright.config.ts""#));
    assert!(web_tsconfig_node.contains(r#""e2e""#));
    assert!(destination.join("web/components.json").exists());
    assert!(
        destination
            .join("web/src/components/ui/button.tsx")
            .exists()
    );
    assert!(destination.join("web/src/components/ui/card.tsx").exists());
    assert!(destination.join("web/src/lib/utils.ts").exists());
    let web_components = fs::read_to_string(destination.join("web/components.json")).unwrap();
    assert!(web_components.contains(r#""style": "radix-nova""#));
    let web_css = fs::read_to_string(destination.join("web/src/index.css")).unwrap();
    assert!(web_css.contains(r#"@import "tailwindcss";"#));
    assert!(web_css.contains(r#"@import "shadcn/tailwind.css";"#));
    let web_app = fs::read_to_string(destination.join("web/src/App.tsx")).unwrap();
    assert!(web_app.contains(r#"from "@/components/ui/card""#));
    let web_vite_config = fs::read_to_string(destination.join("web/vite.config.ts")).unwrap();
    assert!(web_vite_config.contains("const devPort = Number(process.env.PORT);"));
    assert!(web_vite_config.contains("port: devPort"));
    assert!(web_vite_config.contains("process.env.API_ORIGIN"));
    assert!(web_vite_config.contains("process.env.JIG_DEV_API_ORIGIN"));
    assert!(
        web_vite_config
            .contains("firstNonEmpty(process.env.JIG_DEV_API_ORIGIN, process.env.API_ORIGIN)")
    );
    assert!(
        !web_vite_config
            .contains("firstNonEmpty(process.env.API_ORIGIN, process.env.JIG_DEV_API_ORIGIN)")
    );
    assert!(web_vite_config.contains(r#""http://api.my-app.localhost:1355""#));
    assert!(web_vite_config.contains(r#""/api""#));
    assert!(web_vite_config.contains(r#"target: apiOrigin"#));
    assert!(!web_vite_config.contains("apiOrigin ?"));
    assert!(web_vite_config.contains(r#"host: "127.0.0.1""#));
    assert!(web_vite_config.contains("strictPort: true"));
    assert!(web_vite_config.contains("clientPort: devPort"));
    assert!(
        web_vite_config.contains(r#"include: ["src/**/*.test.{ts,tsx}"]"#),
        "Vitest must not collect Playwright specs"
    );
    assert!(web_vite_config.contains(r#"include: ["src/**/*.{ts,tsx}"]"#));
    for excluded in [
        "src/**/*.d.ts",
        "src/**/*.test.{ts,tsx}",
        "src/test-setup.ts",
        "src/main.tsx",
        "src/components/ui/**/*.{ts,tsx}",
        "src/lib/utils.ts",
    ] {
        assert!(
            web_vite_config.contains(&format!(r#""{excluded}""#)),
            "SPA coverage must explicitly exclude {excluded}"
        );
    }
    assert!(
        !web_vite_config.contains(r#"include: ["src/App.tsx", "src/api.ts"]"#),
        "future production modules must not escape the coverage denominator"
    );
    let web_playwright = fs::read_to_string(destination.join("web/playwright.config.ts")).unwrap();
    assert!(web_playwright.contains("cargo run --locked -p my-app-api"));
    assert!(web_playwright.contains("-- --bootstrap-database"));
    assert!(web_playwright.contains("my_app_web_e2e"));
    assert!(web_playwright.contains(r#"url: `${apiOrigin}/health/ready`"#));
    assert!(web_playwright.contains("reuseExistingServer: false"));
    assert!(web_playwright.contains("E2E_SERVER_TIMEOUT_MS"));
    assert!(web_playwright.contains("E2E_GLOBAL_TIMEOUT_MS"));
    assert!(web_playwright.contains("managedWebServerCount * serverTimeout + 5 * 60_000"));
    assert!(web_playwright.contains("const configured = process.env[name]?.trim()"));
    assert!(web_playwright.contains("E2E_WEB_PORT and E2E_API_PORT must use different ports"));
    assert!(web_playwright.contains("failOnFlakyTests keeps a recovered retry red"));
    assert!(web_playwright.contains(r#"gracefulShutdown: { signal: "SIGTERM""#));
    assert!(web_playwright.contains(r#"command: "vite --host 127.0.0.1 --strictPort""#));
    assert!(web_playwright.contains("API_ORIGIN: apiOrigin"));
    assert!(web_playwright.contains("JIG_DEV_API_ORIGIN: apiOrigin"));
    let web_e2e = fs::read_to_string(destination.join("web/e2e/app.spec.ts")).unwrap();
    assert!(web_e2e.contains("page.waitForResponse"));
    assert!(web_e2e.contains(r#"versionResponse.headers()["x-request-id"]"#));
    assert!(web_e2e.contains(r#"name: "my-app""#));
    assert!(web_e2e.contains(r#"getByRole("group", { name: "Application", exact: true })"#));
    assert!(web_e2e.contains(r#"locator('[data-slot="card-title"]')"#));
    assert!(web_e2e.contains(r#"getByRole("group", { name: "Rust API", exact: true })"#));
    assert!(web_e2e.contains(r#"serviceStatusCard.getByText("Ready", { exact: true })"#));
    assert!(!web_e2e.contains("page.route"));
    let e2e_workflow = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    let e2e_workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&e2e_workflow)
        .expect("generated Postgres E2E workflow must be valid YAML");
    assert_eq!(e2e_workflow_yaml["jobs"]["e2e"]["runs-on"], "ubuntu-latest");
    assert_eq!(
        e2e_workflow_yaml["jobs"]["e2e"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert!(e2e_workflow.contains("name: Browser E2E"));
    assert!(e2e_workflow.contains("timeout-minutes: 30"));
    assert!(e2e_workflow.contains("outside Playwright's 15-minute default CI suite budget"));
    assert_eq!(e2e_workflow.matches(r#"- "rust-toolchain""#).count(), 2);
    assert_eq!(
        e2e_workflow.matches(r#"- "npm-shrinkwrap.json""#).count(),
        2
    );
    assert!(e2e_workflow.contains("E2E_SERVER_TIMEOUT_MS: \"300000\""));
    assert!(e2e_workflow.contains("- name: \"web\"\n            dir: \"web\""));
    assert!(!e2e_workflow.contains("dir: landing"));
    assert!(!e2e_workflow.contains("dir: admin-panel"));
    assert!(e2e_workflow.contains(r#"- "migrations/**""#));
    assert!(e2e_workflow.contains(r#"- ".sqlx/**""#));
    assert!(e2e_workflow.contains("image: postgres:18"));
    assert!(e2e_workflow.contains(
        "postgres://postgres:postgres@127.0.0.1:5432/jig_e2e_${{ github.run_id }}_${{ github.run_attempt }}"
    ));
    assert!(e2e_workflow.contains(r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#));
    assert!(
        e2e_workflow
            .contains(r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e:install:ci"#)
    );
    assert!(e2e_workflow.contains(r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e"#));
    assert!(!e2e_workflow.contains("bun run test:e2e"));
    assert!(e2e_workflow.contains("actions/upload-artifact@v6"));
    let rust_workflow =
        fs::read_to_string(destination.join(".github/workflows/rust-tests.yml")).unwrap();
    let rust_workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&rust_workflow).unwrap();
    for job in ["fmt", "clippy", "test"] {
        assert_eq!(rust_workflow_yaml["jobs"][job]["runs-on"], "macos-14");
    }
    for event in ["pull_request", "push"] {
        let paths = rust_workflow_yaml["on"][event]["paths"].as_array().unwrap();
        assert!(paths.iter().any(|path| path == "migrations/**"));
        assert!(paths.iter().any(|path| path == ".sqlx/**"));
    }
    assert_eq!(
        rust_workflow_yaml["jobs"]["clippy"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert_eq!(
        rust_workflow_yaml["jobs"]["test"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert!(rust_workflow_yaml["jobs"]["fmt"]["env"].is_null());
    for (workflow_name, jobs) in [
        ("agent-map-check.yml", &["agent-map-check"][..]),
        (
            "repo-policy.yml",
            &[
                "no-mod-rs",
                "rust-file-loc",
                "sqlx-unchecked-queries",
                "migration-immutability",
            ][..],
        ),
    ] {
        let workflow =
            fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
        let workflow = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
        for job in jobs {
            assert_eq!(workflow["jobs"][job]["runs-on"], "macos-14");
        }
    }
    let landing_package = fs::read_to_string(destination.join("landing/package.json")).unwrap();
    assert!(landing_package.contains(r#""dev": "astro dev""#));
    assert!(!landing_package.contains(" install && "));
    let landing_config = fs::read_to_string(destination.join("landing/astro.config.mjs")).unwrap();
    assert!(landing_config.contains("process.env.HOST?.trim() || '127.0.0.1'"));
    assert!(landing_config.contains("strictPort: true"));
    assert!(landing_config.contains("Number(process.env.PORT || '4321')"));
    assert!(landing_config.contains("port < 1 || port > 65_535"));
    assert!(!destination.join("landing/playwright.config.ts").exists());
    let admin_package = fs::read_to_string(destination.join("admin-panel/package.json")).unwrap();
    let admin_package_json: serde_json::Value = serde_json::from_str(&admin_package).unwrap();
    assert_eq!(
        admin_package_json["devDependencies"]["@types/node"].as_str(),
        Some(GENERATED_NODE_TYPES_VERSION)
    );
    assert!(admin_package.contains(r#""shadcn": "4.13.0""#));
    assert!(admin_package.contains(r#""tailwindcss": "4.3.2""#));
    assert!(admin_package.contains(r#""@testing-library/dom": "10.4.1""#));
    assert!(admin_package.contains(r#""lint": "eslint . && prettier --check .""#));
    assert!(admin_package.contains(r#""format": "prettier --write .""#));
    assert!(admin_package.contains(r#""format:check": "prettier --check .""#));
    assert!(!admin_package.contains("@playwright/test"));
    let admin_readme = fs::read_to_string(destination.join("admin-panel/README.md")).unwrap();
    assert!(admin_readme.contains("real-backend Playwright starter for product SPA roles only"));
    let admin_vite_config =
        fs::read_to_string(destination.join("admin-panel/vite.config.ts")).unwrap();
    assert!(admin_vite_config.contains("const devPort = Number(process.env.PORT)"));
    assert!(admin_vite_config.contains("port: devPort"));
    assert!(admin_vite_config.contains("strictPort: true"));
    assert!(admin_vite_config.contains("clientPort: devPort"));
    assert!(
        admin_vite_config
            .contains("firstNonEmpty(process.env.JIG_DEV_API_ORIGIN, process.env.API_ORIGIN)")
    );
    assert!(
        !admin_vite_config
            .contains("firstNonEmpty(process.env.API_ORIGIN, process.env.JIG_DEV_API_ORIGIN)")
    );
    let admin_index = fs::read_to_string(destination.join("admin-panel/index.html")).unwrap();
    let theme_storage_key = "admin-panel-theme";
    let theme_bootstrap = admin_index
        .find(&format!("const themeStorageKey = \"{theme_storage_key}\""))
        .unwrap();
    let react_entry = admin_index.find("/src/main.tsx").unwrap();
    assert!(theme_bootstrap < react_entry);
    assert_eq!(admin_index.matches(theme_storage_key).count(), 1);
    assert!(admin_index.contains("localStorage.getItem(themeStorageKey)"));
    assert!(admin_index.contains("<!-- prettier-ignore -->\n    <title>Admin Panel</title>"));
    assert!(admin_index.contains("prefers-color-scheme: dark"));
    assert!(admin_index.contains("root.style.colorScheme = resolved"));
    let theme_provider =
        fs::read_to_string(destination.join("admin-panel/src/components/theme-provider.tsx"))
            .unwrap();
    assert!(theme_provider.contains("storage = window.localStorage"));
    assert!(theme_provider.contains("if (event.storageArea !== storage)"));
    let providers =
        fs::read_to_string(destination.join("admin-panel/src/app/providers.tsx")).unwrap();
    assert!(providers.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(providers.matches(theme_storage_key).count(), 1);
    assert!(providers.contains("storageKey={themeStorageKey}"));
    let admin_shell =
        fs::read_to_string(destination.join("admin-panel/src/app/shell.tsx")).unwrap();
    assert!(admin_shell.contains("const appTitle = \"Admin Panel\""));
    assert!(admin_shell.contains(">{appTitle}</p>"));
    let admin_sidebar =
        fs::read_to_string(destination.join("admin-panel/src/components/app-sidebar.tsx")).unwrap();
    assert!(admin_sidebar.contains("const appName = \"my-app\""));
    assert_eq!(admin_sidebar.matches("\"my-app\"").count(), 1);
    assert!(admin_sidebar.contains(">{appName}</span>"));
    let admin_overview_test = fs::read_to_string(
        destination.join("admin-panel/src/features/overview/overview-page.test.tsx"),
    )
    .unwrap();
    assert!(admin_overview_test.contains("const expectedAppName = \"my-app\""));
    assert_eq!(admin_overview_test.matches("\"my-app\"").count(), 1);
    assert!(admin_overview_test.contains("name: expectedAppName"));
    assert!(admin_overview_test.contains("screen.findByText(expectedAppName)"));
    let admin_prettierignore =
        fs::read_to_string(destination.join("admin-panel/.prettierignore")).unwrap();
    assert_eq!(admin_prettierignore.matches("dist/\n").count(), 1);
    assert_eq!(admin_prettierignore.matches("pnpm-lock.yaml").count(), 1);
    assert_eq!(
        admin_prettierignore.matches("npm-shrinkwrap.json").count(),
        1
    );
    assert!(admin_prettierignore.contains("bun.lock\nbun.lockb\n"));
    let admin_empty =
        fs::read_to_string(destination.join("admin-panel/src/components/ui/empty.tsx")).unwrap();
    assert!(admin_empty.contains(r#"import type { ComponentProps } from "react""#));
    assert!(!admin_empty.contains("React.ComponentProps"));
    let admin_skeleton =
        fs::read_to_string(destination.join("admin-panel/src/components/ui/skeleton.tsx")).unwrap();
    assert!(admin_skeleton.contains(r#"import type { ComponentProps } from "react""#));
    assert!(!admin_skeleton.contains("React.ComponentProps"));
    let admin_sonner =
        fs::read_to_string(destination.join("admin-panel/src/components/ui/sonner.tsx")).unwrap();
    assert!(admin_sonner.contains(r#"import type { CSSProperties } from "react""#));
    assert!(!admin_sonner.contains("React.CSSProperties"));
    let components = fs::read_to_string(destination.join("admin-panel/components.json")).unwrap();
    assert!(components.contains(r#""style": "radix-nova""#));
    assert!(
        destination
            .join("admin-panel/src/components/ui/sidebar.tsx")
            .exists()
    );
    assert!(
        destination
            .join("admin-panel/src/features/overview/overview-page.tsx")
            .exists()
    );
    assert!(destination.join("admin-panel/src/lib/api.ts").exists());

    let agent_map = fs::read_to_string(destination.join("agent-map.md")).unwrap();
    for guide in [
        "crates/my-app/AGENTS.md",
        "crates/my-app-db/AGENTS.md",
        "crates/my-app-http/AGENTS.md",
        "crates/my-app-test-support/AGENTS.md",
    ] {
        assert!(agent_map.contains(guide), "agent map is missing {guide}");
    }

    let root_gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(root_gitignore.contains("/my_app.db\n"));
    assert!(root_gitignore.contains("/my_app.db-*\n"));
    for database_file in [
        "my_app.db",
        "my_app.db-wal",
        "my_app.db-shm",
        "my_app.db-journal",
        "my_app.db-jig-migrate.lock",
    ] {
        fs::write(destination.join(database_file), "local database artifact").unwrap();
    }
    assert_eq!(
        git_stdout(
            &destination,
            [
                "check-ignore",
                "--",
                "my_app.db",
                "my_app.db-wal",
                "my_app.db-shm",
                "my_app.db-journal",
                "my_app.db-jig-migrate.lock",
            ],
        )
        .unwrap(),
        "my_app.db\nmy_app.db-wal\nmy_app.db-shm\nmy_app.db-journal\nmy_app.db-jig-migrate.lock"
    );

    let api_main = fs::read_to_string(destination.join("apps/my-app-api/src/main.rs")).unwrap();
    assert!(api_main.contains("use anyhow::Context;"));
    assert!(api_main.contains("use ::my_app as app_crate;"));
    assert!(api_main.contains("use ::my_app_http as app_http_crate;"));
    assert!(api_main.contains("load_dotenv();"));
    assert!(api_main.contains("warning: failed to load .env"));
    assert!(api_main.contains("let bound_addr = listener"));
    assert!(api_main.contains("Failed to read API listener address after bind"));
    assert!(api_main.contains("tracing::info!(%bound_addr, \"listening\")"));
    assert!(api_main.contains("app_http_crate::router"));
    assert!(api_main.contains("app_crate::AppConfig::from_env()"));
    assert!(api_main.contains("app_crate::AppState::from_config(config)"));
    assert!(api_main.contains("--bootstrap-database"));
    assert!(api_main.contains(
        "    let command = parse_command()?;\n    let config = app_crate::AppConfig::from_env()"
    ));
    assert!(api_main.contains("match (arguments.next(), arguments.next())"));
    assert!(api_main.contains("unexpected API argument"));
    assert!(!api_main.contains("args_os().any"));
    assert!(api_main.contains("app_crate::AppState::bootstrap_database(&config)"));
    assert!(api_main.contains("install_panic_hook"));
    assert!(api_main.contains("tracing::error!(error = ?error, \"API server failed\")"));
    assert!(api_main.contains("#[allow(clippy::useless_concat)]\n    let default_filter"));
    assert!(api_main.contains("let default_filter = concat!("));
    assert!(api_main.contains("\"my_app=info,\","));
    assert!(api_main.contains("\"my_app_api=info,\","));
    assert!(api_main.contains("\"tower_http=info\","));
    assert!(api_main.contains("Failed to bind API listener"));
    assert!(api_main.contains("API server exited with an error"));
    assert!(api_main.contains("SignalKind::terminate"));
    assert!(api_main.contains("failed to listen for Ctrl-C"));
    let jig_toml = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(jig_toml.contains("[[dev.apps]]\nname = \"api\""));
    assert!(jig_toml.contains("kind = \"env-port\""));
    assert!(!jig_toml.contains("proxy = false"));
    assert!(jig_toml.contains("argv = [\"cargo\", \"run\", \"-p\", \"my-app-api\"]"));
    assert!(!jig_toml.contains("BIND_ADDR=\"${HOST}:${PORT}\""));
    assert!(!jig_toml.contains("port = 3000"));
    assert_eq!(
        fs::read_to_string(destination.join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=my_app=info,my_app_api=info,tower_http=info\nDATABASE_URL=postgres://postgres:postgres@localhost:5432/my_app_dev\n"
    );
    let workspace_cargo = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(workspace_cargo.contains("dotenvy = \"0.15\""));
    let api_cargo = fs::read_to_string(destination.join("apps/my-app-api/Cargo.toml")).unwrap();
    assert!(api_cargo.contains("dotenvy.workspace = true"));
    let app_lib = fs::read_to_string(destination.join("crates/my-app/src/lib.rs")).unwrap();
    assert!(app_lib.contains("pub struct AppConfig"));
    assert!(app_lib.contains("pub fn from_env() -> Result<Self>"));
    assert!(app_lib.contains("std::env::var(\"HOST\")"));
    assert!(app_lib.contains("std::env::var(\"PORT\")"));
    assert!(app_lib.contains("fn resolve_bind_addr("));
    assert!(app_lib.contains("injected_host_and_port_override_the_dotenv_bind_address"));
    assert!(app_lib.contains("partial_jig_bind_values_fall_back_to_bind_addr"));
    assert!(app_lib.contains("DATABASE_URL is required when the db feature is enabled"));
    assert!(app_lib.contains("pub async fn from_config(config: AppConfig) -> Result<Self>"));
    assert!(app_lib.contains("pub async fn bootstrap_database(config: &AppConfig)"));
    assert!(app_lib.contains("pub fn new_with_version(version: impl Into<String>)"));
    assert!(app_lib.contains("pub fn version(&self) -> &AppVersion"));
    assert!(app_lib.contains("pub fn is_ready(&self) -> bool"));
    assert!(!app_lib.contains("return Ok(Self"));
    assert!(!app_lib.contains("return self.db.is_some()"));
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
    assert!(test_support_http_test.contains("use ::my_app_test_support::TestApp;"));
    assert!(test_support_http_test.contains("async fn health_returns_ok()"));
    assert!(test_support_http_test.contains("async fn readiness_reflects_state()"));
    assert!(test_support_http_test.contains("StatusCode::SERVICE_UNAVAILABLE"));
    assert!(test_support_http_test.contains("async fn responses_include_request_id()"));
    assert!(test_support_http_test.contains("async fn version_returns_json()"));
    let db_lib = fs::read_to_string(destination.join("crates/my-app-db/src/lib.rs")).unwrap();
    assert!(db_lib.contains("PgPool"));
    assert!(db_lib.contains("sqlx::Postgres::database_exists"));
    assert!(db_lib.contains("sqlx::Postgres::create_database"));
    assert!(db_lib.contains("Could not confirm database existence after creation failed"));
    assert!(db_lib.contains("create_if_missing"));
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
    assert!(answers.contains("if [ -f Cargo.toml ]; then cargo fetch;"));
    assert!(answers.contains("cargo run -p my-app-api -- --bootstrap-database"));
    assert!(answers.contains("export it or copy .env.example to .env before bootstrap"));
    assert!(answers.contains("${DATABASE_URL:-}"));
    assert!(answers.contains(
        "cargo run -p my-app-api -- --bootstrap-database && scripts/check-webapps.sh bootstrap"
    ));
    assert!(!answers.contains("(cd web && bun install)"));
    assert!(answers.contains("name = \"web\""));
    assert!(answers.contains("dir = \"landing\""));
    assert!(answers.contains("kind = \"env-port\""));
    assert!(answers.contains("name = \"admin-panel\""));
    assert!(answers.contains("role = \"spa\""));
    assert!(answers.contains("role = \"astro\""));
    assert!(answers.contains("role = \"admin\""));
}

// Repeat the dependency-backed proof with:
// cargo test -p jig-sh bootstrap::tests::basic::generated_spa_coverage_counts_uncovered_future_production_modules -- --ignored --exact --nocapture
#[cfg(unix)]
#[test]
#[ignore = "requires npm registry access and a local Node/npm toolchain"]
fn generated_spa_coverage_counts_uncovered_future_production_modules() {
    use std::fmt::Write as _;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("coverage-proof");

    run_init(InitOpts {
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
        answers: AnswerOpts {
            web_package_manager: Some("npm".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let dependencies = Command::new("scripts/check-webapps.sh")
        .args(["dependencies-bootstrap", "web"])
        .env("NODE_ENV", "production")
        .env("NPM_CONFIG_OMIT", "dev")
        .current_dir(&destination)
        .output()
        .unwrap();
    assert!(
        dependencies.status.success(),
        "generated dependency bootstrap could not prepare the coverage fixture:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&dependencies.stdout),
        String::from_utf8_lossy(&dependencies.stderr)
    );

    let run_coverage = || {
        Command::new("scripts/check-webapps.sh")
            .arg("coverage")
            .current_dir(&destination)
            .output()
            .unwrap()
    };
    let statement_coverage = || {
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(destination.join("web/coverage/coverage-summary.json")).unwrap(),
        )
        .unwrap()["total"]["statements"]["pct"]
            .as_f64()
            .unwrap()
    };

    let baseline = run_coverage();
    assert!(
        baseline.status.success(),
        "generated SPA coverage baseline failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&baseline.stdout),
        String::from_utf8_lossy(&baseline.stderr)
    );
    let baseline_statements = statement_coverage();
    assert!(baseline_statements >= 80.0);

    let mut uncovered_module = String::new();
    for index in 0..20 {
        write!(
            uncovered_module,
            "export function uncovered{index}(value: number): number {{\n  const shifted = value + {index};\n  const doubled = shifted * 2;\n  return doubled > 10 ? doubled : 10;\n}}\n\n"
        )
        .unwrap();
    }
    fs::write(
        destination.join("web/src/uncovered-production.ts"),
        uncovered_module,
    )
    .unwrap();

    let negative = run_coverage();
    assert!(!negative.status.success());
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&negative.stdout),
        String::from_utf8_lossy(&negative.stderr)
    );
    assert!(diagnostics.contains("Coverage below threshold 80%"));
    let uncovered_statements = statement_coverage();
    assert!(
        uncovered_statements < 80.0,
        "future production module stayed outside the coverage denominator: baseline {baseline_statements}%, after addition {uncovered_statements}%"
    );
}

#[test]
fn rust_react_admin_dynamic_values_use_formatter_stable_boundaries() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo_name = "r".repeat(120);
    let frontend_name = format!("admin-{}", "x".repeat(100));
    let destination = temp.path().join(&repo_name);

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend(&format!("{frontend_name}:admin")).unwrap(),
            ],
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let admin = destination.join(&frontend_name);
    let theme_storage_key = format!("{frontend_name}-theme");
    let index = fs::read_to_string(admin.join("index.html")).unwrap();
    assert!(index.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(index.matches(&theme_storage_key).count(), 1);
    assert!(index.contains("localStorage.getItem(themeStorageKey)"));
    assert!(index.contains("<!-- prettier-ignore -->\n    <title>"));

    let providers = fs::read_to_string(admin.join("src/app/providers.tsx")).unwrap();
    assert!(providers.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(providers.matches(&theme_storage_key).count(), 1);
    assert!(providers.contains("storageKey={themeStorageKey}"));

    let shell = fs::read_to_string(admin.join("src/app/shell.tsx")).unwrap();
    assert!(shell.contains("const appTitle = \""));
    assert!(shell.contains(">{appTitle}</p>"));

    let sidebar = fs::read_to_string(admin.join("src/components/app-sidebar.tsx")).unwrap();
    assert!(sidebar.contains(&format!("const appName = \"{repo_name}\"")));
    assert_eq!(sidebar.matches(&repo_name).count(), 1);
    assert!(sidebar.contains(">{appName}</span>"));

    let overview_test =
        fs::read_to_string(admin.join("src/features/overview/overview-page.test.tsx")).unwrap();
    assert!(overview_test.contains(&format!("const expectedAppName = \"{repo_name}\"")));
    assert_eq!(overview_test.matches(&repo_name).count(), 1);
    assert!(overview_test.contains("name: expectedAppName"));
    assert!(overview_test.contains("screen.findByText(expectedAppName)"));
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
fn rust_react_reserves_backend_dev_identity_across_frontend_sources() {
    let cases = vec![
        (
            "--frontend",
            ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: vec![parse_scaffold_frontend("api:spa").unwrap()],
                frontend_list: Vec::new(),
            },
            AnswerOpts::default(),
            "api",
        ),
        (
            "--frontends",
            ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: Vec::new(),
                frontend_list: vec![parse_scaffold_frontend("API:admin").unwrap()],
            },
            AnswerOpts::default(),
            "API",
        ),
        (
            "frontend_apps",
            ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                ..ScaffoldOpts::default()
            },
            AnswerOpts {
                frontend_apps: vec![FrontendApp {
                    name: "Api".into(),
                    dir: "site".into(),
                    coverage_threshold: 80,
                    kind: "env-port".into(),
                    role: "astro".into(),
                }],
                ..AnswerOpts::default()
            },
            "Api",
        ),
    ];

    for (source, opts, answers, supplied_name) in cases {
        let error = opts
            .validate_init_invariants(&answers)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("frontend app name '{supplied_name}'")),
            "{source}: {error}"
        );
        assert!(
            error.contains("reserved backend dev app 'api'"),
            "{source}: {error}"
        );
        assert!(error.contains("JIG_DEV_API"), "{source}: {error}");
        assert!(
            error.contains("choose another frontend name"),
            "{source}: {error}"
        );
    }
}

#[test]
fn reserved_backend_dev_identity_is_scoped_to_rust_react() {
    let api_frontend = FrontendApp {
        name: "api".into(),
        dir: "api".into(),
        coverage_threshold: 80,
        kind: "vite".into(),
        role: "spa".into(),
    };
    let answers = AnswerOpts {
        frontend_apps: vec![api_frontend],
        ..AnswerOpts::default()
    };

    for preset in [None, Some(ScaffoldPreset::HarnessOnly)] {
        ScaffoldOpts {
            preset,
            ..ScaffoldOpts::default()
        }
        .validate_init_invariants(&answers)
        .unwrap();
    }

    ScaffoldOpts {
        preset: Some(ScaffoldPreset::RustReact),
        frontends: vec![parse_scaffold_frontend("api-client:spa").unwrap()],
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts::default())
    .unwrap();
}

#[test]
fn run_init_rejects_merged_backend_named_frontend_before_template_or_destination_writes() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"[[frontend_apps]]
name = "Api"
dir = "site"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        template: Some(temp.path().join("missing-template").display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: false,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("frontend app name 'Api'"));
    assert!(error.contains("reserved backend dev app 'api'"));
    assert!(!destination.exists());
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
                custom_default_name: false,
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

#[cfg(unix)]
fn assert_rendered_scaffold_rust_is_formatted(plan: &scaffold::InitScaffoldPlan, case: &str) {
    let rendered = plan.render_files().unwrap();
    let temp = tempdir().unwrap();
    let mut rust_paths = Vec::new();

    for (index, file) in rendered
        .into_iter()
        .filter(|file| file.relative.ends_with(".rs"))
        .enumerate()
    {
        let path = temp.path().join(format!("rendered-{index}.rs"));
        fs::write(&path, file.contents).unwrap();
        rust_paths.push(path);
    }

    assert!(!rust_paths.is_empty(), "{case}: scaffold rendered no Rust");
    let output = Command::new("rustfmt")
        .args([
            "--edition",
            "2024",
            "--check",
            "--config",
            "skip_children=true",
        ])
        .args(&rust_paths)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "rendered Rust was not rustfmt-stable for {case}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn rust_react_package_stem_limit_is_applied_before_destination_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();

    let accepted_name = "r".repeat(216);
    let accepted_destination = temp.path().join("accepted");
    fs::create_dir(&accepted_destination).unwrap();
    let accepted_plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some(accepted_name.clone()),
            ..AnswerOpts::default()
        },
        &accepted_destination,
    )
    .unwrap()
    .unwrap();
    accepted_plan.write(&accepted_destination, false).unwrap();

    assert!(
        accepted_destination
            .join(format!("crates/{accepted_name}-test-support/Cargo.toml"))
            .is_file()
    );
    let vite_config = fs::read_to_string(accepted_destination.join("web/vite.config.ts")).unwrap();
    let repo_label = vite_config
        .split_once("http://api.")
        .unwrap()
        .1
        .split_once(".localhost:1355")
        .unwrap()
        .0;
    assert_eq!(repo_label.len(), 63);
    assert_eq!(repo_label, "r".repeat(63));

    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&accepted_destination)
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "maximum supported scaffold has invalid Cargo metadata\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&metadata.stdout),
        String::from_utf8_lossy(&metadata.stderr)
    );

    let rejected_destination = temp.path().join("rejected");
    let error = run_init(InitOpts {
        path: rejected_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some(materialize_template_worktree().path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("r".repeat(217)),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("217-byte Cargo package stem"), "{error}");
    assert!(error.contains("at most 216 bytes"), "{error}");
    assert!(
        error.contains("lib<stem>_test_support-<hash>.rmeta"),
        "{error}"
    );
    assert!(!rejected_destination.exists());
}

#[cfg(unix)]
#[test]
fn scaffold_rendered_rust_is_formatted_across_names_databases_and_migration_paths() {
    let planning_root = tempdir().unwrap();
    let names = [
        ("manual-qa", "node22-npm12-sqlite-web".to_string()),
        ("width-40", format!("r{}", "a".repeat(39))),
        ("width-52", format!("r{}", "a".repeat(51))),
        ("width-71", format!("r{}", "a".repeat(70))),
        ("supported-max-216", format!("r{}", "a".repeat(215))),
    ];
    for (label, name) in &names {
        let expected_len = match *label {
            "manual-qa" => 23,
            "width-40" => 40,
            "width-52" => 52,
            "width-71" => 71,
            "supported-max-216" => 216,
            _ => unreachable!(),
        };
        assert_eq!(name.len(), expected_len, "{label}");
    }

    for db in [ScaffoldDb::None, ScaffoldDb::Sqlite, ScaffoldDb::Postgres] {
        let db_label = match db {
            ScaffoldDb::None => "none",
            ScaffoldDb::Sqlite => "sqlite",
            ScaffoldDb::Postgres => "postgres",
        };
        for (name_label, repo_name) in &names {
            let plan = scaffold::InitScaffoldPlan::from_opts(
                &ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(db),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                &AnswerOpts {
                    repo_name: Some(repo_name.clone()),
                    ..AnswerOpts::default()
                },
                planning_root.path(),
            )
            .unwrap()
            .unwrap();
            assert_rendered_scaffold_rust_is_formatted(&plan, &format!("{db_label}/{name_label}"));
        }
    }

    for db in [ScaffoldDb::Sqlite, ScaffoldDb::Postgres] {
        let db_label = match db {
            ScaffoldDb::Sqlite => "sqlite",
            ScaffoldDb::Postgres => "postgres",
            ScaffoldDb::None => unreachable!(),
        };
        for migration_len in [13, 80, 216] {
            let plan = scaffold::InitScaffoldPlan::from_opts(
                &ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(db),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                &AnswerOpts {
                    repo_name: Some("demo".into()),
                    rust_migration_dir: Some("m".repeat(migration_len)),
                    ..AnswerOpts::default()
                },
                planning_root.path(),
            )
            .unwrap()
            .unwrap();
            assert_rendered_scaffold_rust_is_formatted(
                &plan,
                &format!("{db_label}/migration-width-{migration_len}"),
            );
        }
    }
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
    assert_eq!(report["frontends"][0]["kind"], "vite");
    assert_eq!(report["frontends"][0]["role"], "spa");
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
    let repo_name = report["repo_name"].as_str().unwrap();
    let module_name = repo_name.replace('-', "_");
    let env_example = fs::read_to_string(temp.path().join(".env.example")).unwrap();
    assert_eq!(
        env_example,
        format!(
            "BIND_ADDR=127.0.0.1:3000\nRUST_LOG={module_name}=info,{module_name}_api=info,tower_http=info\n"
        )
    );
    let playwright = fs::read_to_string(temp.path().join("web/playwright.config.ts")).unwrap();
    assert!(playwright.contains("const backendCommand = \"cargo run --locked"));
    assert!(!playwright.contains("-- --bootstrap-database"));
    assert!(!playwright.contains("E2E_DATABASE_URL"));
    let workflow = fs::read_to_string(temp.path().join(".github/workflows/e2e.yml")).unwrap();
    assert!(!workflow.contains("image: postgres"));
    assert!(!workflow.contains("E2E_DATABASE_URL"));
    assert!(!workflow.contains("SQLX_OFFLINE"));
    let api_main = fs::read_to_string(
        temp.path()
            .join("apps")
            .join(format!("{repo_name}-api/src/main.rs")),
    )
    .unwrap();
    assert!(
        api_main
            .contains("    parse_command()?;\n    let config = app_crate::AppConfig::from_env()")
    );
    assert!(!api_main.contains("let command = parse_command()?;"));
    assert!(!api_main.contains("--bootstrap-database"));
    assert!(api_main.contains("match (arguments.next(), arguments.next())"));
    assert!(api_main.contains("unexpected API argument"));
    assert!(!api_main.contains("args_os().any"));

    let output = std::process::Command::new("cargo")
        .args(["fmt", "--all", "--", "--check"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo fmt failed for the no-database scaffold\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn scaffold_playwright_api_environment_overrides_hostile_inherited_bindings() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
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

    let config = fs::read_to_string(temp.path().join("web/playwright.config.ts")).unwrap();
    let api_server_config = config
        .split_once(r#"name: "Rust API""#)
        .unwrap()
        .1
        .split_once(r#"name: "Vite web""#)
        .unwrap()
        .0;
    for fixed_binding in [
        r#"HOST: "127.0.0.1""#,
        "PORT: String(apiPort)",
        r#"BIND_ADDR: `127.0.0.1:${apiPort}`"#,
    ] {
        assert!(
            api_server_config.contains(fixed_binding),
            "hostile inherited bindings must be replaced by {fixed_binding}"
        );
    }
    assert!(!api_server_config.contains("process.env.HOST"));
    assert!(!api_server_config.contains("process.env.PORT"));
}

#[test]
fn scaffold_e2e_workflow_uses_each_package_manager_portably() {
    let temp = tempdir().unwrap();
    for (package_manager, setup, run, root_cache_locks, app_cache_locks) in [
        (
            "bun",
            "oven-sh/setup-bun@v2",
            "bun run",
            "'bun.lock', 'bun.lockb'",
            "format('{0}/bun.lock', matrix.app.dir), format('{0}/bun.lockb', matrix.app.dir)",
        ),
        (
            "npm",
            "npm install --global npm@",
            "npm run",
            "'npm-shrinkwrap.json', 'package-lock.json'",
            "format('{0}/npm-shrinkwrap.json', matrix.app.dir), format('{0}/package-lock.json', matrix.app.dir)",
        ),
        (
            "pnpm",
            r#"package_manager_spec="$(scripts/check-webapps.sh package-manager-spec"#,
            "pnpm run",
            "'pnpm-lock.yaml'",
            "format('{0}/pnpm-lock.yaml', matrix.app.dir)",
        ),
        (
            "yarn",
            r#"package_manager_spec="$(scripts/check-webapps.sh package-manager-spec"#,
            "yarn run",
            "'yarn.lock'",
            "format('{0}/yarn.lock', matrix.app.dir)",
        ),
    ] {
        let destination = temp.path().join(package_manager);
        fs::create_dir(&destination).unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some("demo".into()),
                ci_github_runner: Some("macos-14".into()),
                web_package_manager: Some(package_manager.into()),
                ..AnswerOpts::default()
            },
            &destination,
        )
        .unwrap()
        .unwrap();

        plan.write(&destination, false).unwrap();

        let workflow = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
        let workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&workflow)
            .expect("generated E2E workflow must be valid YAML");
        assert_eq!(workflow_yaml["jobs"]["e2e"]["runs-on"], "macos-14");
        assert_eq!(
            workflow_yaml["jobs"]["e2e"]["defaults"]["run"]["shell"],
            "bash"
        );
        assert!(workflow.contains(setup), "missing {package_manager} setup");
        assert!(workflow.contains("Classic required status checks can remain pending"));
        assert!(workflow.contains("Bootstrap Node for dependency metadata"));
        assert!(workflow.contains("scripts/check-webapps.sh node-version-file"));
        assert!(workflow.contains("status=$?"));
        assert!(workflow.contains("if [ \"$status\" -eq 1 ]"));
        assert!(workflow.contains("exit \"$status\""));
        assert!(!workflow.contains("if ! node_version_file="));
        assert!(workflow.contains("${RUNNER_TEMP:?GitHub Actions did not provide RUNNER_TEMP}"));
        assert!(workflow.contains("mktemp -d \"$RUNNER_TEMP/jig-node-version.XXXXXX\""));
        assert!(workflow.contains("set -o noclobber"));
        assert!(!workflow.contains("> .node-version"));
        assert!(workflow.contains("APP_DIR: ${{ matrix.app.dir }}"));
        assert_eq!(workflow.matches(r#"- "rust-toolchain""#).count(), 2);
        assert!(workflow.contains(r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#));
        assert!(workflow.contains(
            "PLAYWRIGHT_BROWSERS_PATH: ${{ github.workspace }}/.agent/tmp/ms-playwright"
        ));
        assert!(workflow.contains("- name: Cache Playwright Chromium"));
        assert!(workflow.contains("path: ${{ env.PLAYWRIGHT_BROWSERS_PATH }}"));
        assert!(workflow.contains("playwright-chromium-${{ hashFiles("));
        assert!(
            workflow
                .contains("hashFiles('package.json', format('{0}/package.json', matrix.app.dir),")
        );
        assert!(
            workflow.contains(root_cache_locks),
            "Playwright cache key is missing root {package_manager} lockfiles"
        );
        assert!(
            workflow.contains(app_cache_locks),
            "Playwright cache key is missing app {package_manager} lockfiles"
        );
        if package_manager == "npm" {
            for cache_path in [
                "            npm-shrinkwrap.json",
                "            package-lock.json",
                "            ${{ matrix.app.dir }}/npm-shrinkwrap.json",
                "            ${{ matrix.app.dir }}/package-lock.json",
            ] {
                assert!(
                    workflow.contains(cache_path),
                    "npm dependency cache is missing {cache_path}"
                );
            }
            assert_eq!(workflow.matches(r#"- "npm-shrinkwrap.json""#).count(), 2);
        }
        assert!(workflow.contains(r#"- "**/.yarnrc.yml""#));
        assert!(workflow.contains(r#"- "**/.yarn/**""#));
        assert!(workflow.contains(r#"- "**/.node-version""#));
        assert!(workflow.contains(r#"- "**/.npmrc""#));
        assert!(workflow.contains("${{ matrix.app.dir }}/"));
        assert!(
            workflow
                .contains(r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e:install:ci"#)
        );
        assert!(workflow.contains(r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e"#));
        assert!(
            !workflow.contains(&format!("{run} test:e2e")),
            "{package_manager} E2E must use the managed checker launcher"
        );
        assert!(!workflow.contains(r#"cd "$APP_DIR" &&"#));
        assert!(!workflow.contains("test:e2e:install --"));
    }
}

#[test]
fn scaffold_omits_e2e_workflow_without_spa_frontends() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![
                parse_scaffold_frontend("docs:astro").unwrap(),
                parse_scaffold_frontend("operations:admin").unwrap(),
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

    assert!(
        !plan
            .output_paths()
            .iter()
            .any(|path| path == Path::new(".github/workflows/e2e.yml"))
    );
    plan.write(temp.path(), false).unwrap();
    assert!(!temp.path().join(".github/workflows/e2e.yml").exists());
}

#[test]
fn scaffold_named_ready_scopes_the_live_status_badge() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("ready:spa").unwrap()],
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
    let app = fs::read_to_string(temp.path().join("ready/src/App.tsx")).unwrap();
    let spec = fs::read_to_string(temp.path().join("ready/e2e/app.spec.ts")).unwrap();

    assert!(app.contains(r#"aria-labelledby="service-status-card-label""#));
    assert!(app.contains(r#"id="service-status-card-label">Rust API"#));
    assert!(spec.contains(r#"getByRole("heading", { name: "Ready" })"#));
    assert!(spec.contains(r#"getByRole("group", { name: "Rust API", exact: true })"#));
    assert!(spec.contains(r#"serviceStatusCard.getByText("Ready", { exact: true })"#));
    assert!(!spec.contains(r#"page.getByText("Ready", { exact: true })"#));
}

#[test]
fn scaffold_e2e_workflow_serializes_dynamic_yaml_scalars() {
    let temp = tempdir().unwrap();
    let default_branch = r#"release/"quoted"\branch"#;
    let runner = "self-hosted # e2e";
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("null:spa").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            default_branch: Some(default_branch.into()),
            ci_github_runner: Some(runner.into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();

    let workflow = fs::read_to_string(temp.path().join(".github/workflows/e2e.yml")).unwrap();
    let workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    assert_eq!(workflow_yaml["on"]["push"]["branches"][0], default_branch);
    assert_eq!(workflow_yaml["jobs"]["e2e"]["runs-on"], runner);
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["defaults"]["run"]["shell"],
        "bash"
    );
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["strategy"]["matrix"]["app"][0]["name"],
        "null"
    );
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["strategy"]["matrix"]["app"][0]["dir"],
        "null"
    );
    assert_eq!(
        workflow_yaml["on"]["pull_request"]["paths"], workflow_yaml["on"]["push"]["paths"],
        "pull and push must render from one E2E path authority"
    );
    let setup_bun = workflow_yaml["jobs"]["e2e"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["name"] == "Setup Bun")
        .unwrap();
    assert_eq!(setup_bun["with"]["bun-version"], "1.3.14");
}

#[test]
fn scaffold_postgres_development_database_name_respects_identifier_limit() {
    let temp = tempdir().unwrap();
    let repo_name = "project".repeat(12);
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some(repo_name),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();

    let env_example = fs::read_to_string(temp.path().join(".env.example")).unwrap();
    let database_name = env_example
        .lines()
        .find_map(|line| line.strip_prefix("DATABASE_URL="))
        .and_then(|url| url.rsplit('/').next())
        .unwrap();
    assert_eq!(database_name.len(), 63);
    assert!(database_name.contains('_'));
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
fn scaffold_bootstrap_command_records_shared_web_dependency_state() {
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

    for package_manager in ["bun", "npm", "pnpm", "yarn"] {
        let mut answers = AnswerOpts {
            web_package_manager: Some(package_manager.into()),
            ..AnswerOpts::default()
        };
        plan.apply_answer_defaults(&mut answers);
        let bootstrap_command = answers.bootstrap_command.unwrap();
        assert!(bootstrap_command.ends_with("&& scripts/check-webapps.sh bootstrap"));
        assert_eq!(
            bootstrap_command
                .matches("scripts/check-webapps.sh bootstrap")
                .count(),
            1
        );
        assert!(!bootstrap_command.contains("cd web"));
        assert!(!bootstrap_command.contains("cd landing"));
    }

    let mut default_answers = AnswerOpts::default();
    plan.apply_answer_defaults(&mut default_answers);
    assert_eq!(default_answers.web_package_manager.as_deref(), Some("bun"));
    assert!(
        default_answers
            .bootstrap_command
            .unwrap()
            .ends_with("&& scripts/check-webapps.sh bootstrap")
    );
}

#[test]
fn scaffold_database_bootstrap_validates_env_then_creates_and_migrates_database() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
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
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    let command = answers.bootstrap_command.unwrap();
    let env_check = command
        .find("if [ -z \"${DATABASE_URL:-}\" ] && ! awk")
        .unwrap();
    assert!(command.contains("export it or copy .env.example to .env"));
    assert!(command.contains("export[[:space:]]+)?DATABASE_URL"));
    let cargo_fetch = command.find("cargo fetch").unwrap();
    let database_bootstrap = command
        .find("cargo run -p demo-api -- --bootstrap-database")
        .unwrap();
    let frontend_bootstrap = command.find("scripts/check-webapps.sh bootstrap").unwrap();
    assert!(env_check < cargo_fetch);
    assert!(cargo_fetch < database_bootstrap);
    assert!(database_bootstrap < frontend_bootstrap);
}

#[test]
fn scaffold_frontend_dev_scripts_only_launch_the_dev_server() {
    for package_manager in ["bun", "npm", "pnpm", "yarn"] {
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

        assert_eq!(
            plan.output_paths()
                .iter()
                .any(|path| path == Path::new(".yarnrc.yml")),
            package_manager == "yarn"
        );
        plan.write(temp.path(), false).unwrap();

        let web_package = fs::read_to_string(temp.path().join("web/package.json")).unwrap();
        assert!(web_package.contains(r#""dev": "vite""#));
        assert!(!web_package.contains(" install && "));
        let landing_package = fs::read_to_string(temp.path().join("landing/package.json")).unwrap();
        assert!(landing_package.contains(r#""dev": "astro dev""#));
        assert!(!landing_package.contains(" install && "));
        let landing_config =
            fs::read_to_string(temp.path().join("landing/astro.config.mjs")).unwrap();
        assert!(landing_config.contains("process.env.HOST?.trim()"));
        assert!(landing_config.contains("process.env.PORT"));
        assert!(landing_config.contains("strictPort: true"));
        let workspace_package = fs::read_to_string(temp.path().join("package.json")).unwrap();
        assert!(workspace_package.contains(&format!(r#""packageManager": "{package_manager}@"#)));
        assert_eq!(
            temp.path().join("pnpm-workspace.yaml").exists(),
            package_manager == "pnpm"
        );
        assert_eq!(
            temp.path().join(".yarnrc.yml").exists(),
            package_manager == "yarn"
        );
        if package_manager == "pnpm" {
            let pnpm_workspace =
                fs::read_to_string(temp.path().join("pnpm-workspace.yaml")).unwrap();
            let pnpm_workspace_yaml: serde_yaml_ng::Value =
                serde_yaml_ng::from_str(&pnpm_workspace).unwrap();
            assert_eq!(
                pnpm_workspace_yaml["enableGlobalVirtualStore"].as_bool(),
                Some(false)
            );
            assert!(
                pnpm_workspace.contains("pre-run validation rewrite installed executable shims")
            );
            assert!(pnpm_workspace.contains("Keep\n# this allowlist narrow"));
            assert!(pnpm_workspace.contains("authorizes dependency code execution"));
            assert!(pnpm_workspace.contains("\nallowBuilds:\n  esbuild: true\n"));
        }
        if package_manager == "yarn" {
            assert_eq!(
                fs::read_to_string(temp.path().join(".yarnrc.yml")).unwrap(),
                "nodeLinker: node-modules\n"
            );
        }
    }
}

#[test]
fn scaffold_preserves_legacy_frontend_kind_role_inference() {
    let temp = tempdir().unwrap();
    let legacy_astro = toml::from_str::<FrontendApp>(
        r#"name = "docs"
dir = "docs-site"
coverage_threshold = 0
kind = "env-port"
"#,
    )
    .unwrap();
    assert_eq!(legacy_astro.role, "astro");
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
                legacy_astro,
                FrontendApp {
                    name: "marketing".into(),
                    dir: "marketing".into(),
                    coverage_threshold: 0,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();
    assert_eq!(report["frontends"][0]["kind"], "env-port");
    assert_eq!(report["frontends"][0]["role"], "astro");
    assert_eq!(report["frontends"][1]["kind"], "vite");
    assert_eq!(report["frontends"][1]["role"], "spa");
    assert!(temp.path().join("docs-site/astro.config.mjs").exists());
    assert!(temp.path().join("marketing/vite.config.ts").exists());

    let mut answers = AnswerOpts::default();
    plan.apply_answer_defaults(&mut answers);
    assert_eq!(answers.frontend_apps[0].name, "docs");
    assert_eq!(answers.frontend_apps[0].dir, "docs-site");
    assert_eq!(answers.frontend_apps[0].kind, "env-port");
    assert_eq!(answers.frontend_apps[0].role, "astro");
    assert_eq!(answers.frontend_apps[1].name, "marketing");
    assert_eq!(answers.frontend_apps[1].dir, "marketing");
    assert_eq!(answers.frontend_apps[1].kind, "vite");
    assert_eq!(answers.frontend_apps[1].role, "spa");
}

#[test]
fn scaffold_playwright_resolves_repo_root_from_nested_spa_dir() {
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
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "clients/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();

    let config = fs::read_to_string(temp.path().join("clients/web/playwright.config.ts")).unwrap();
    assert!(config.contains(r#"path.resolve(appDir, "../..")"#));
}

#[test]
fn scaffold_uses_explicit_frontend_role_without_name_inference() {
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
                    name: "admin".into(),
                    dir: "plain-admin-name".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "operations".into(),
                    dir: "operations".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "admin".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();

    assert_eq!(report["frontends"][0]["role"], "spa");
    assert_eq!(report["frontends"][0]["ui"]["style"], "radix-nova");
    assert!(temp.path().join("plain-admin-name/src/App.tsx").exists());
    assert!(
        temp.path()
            .join("plain-admin-name/components.json")
            .exists()
    );
    assert!(
        !temp
            .path()
            .join("plain-admin-name/src/components/ui/sidebar.tsx")
            .exists()
    );
    assert_eq!(report["frontends"][1]["role"], "admin");
    assert_eq!(report["frontends"][1]["ui"]["style"], "radix-nova");
    assert!(temp.path().join("operations/components.json").exists());
    assert!(
        temp.path()
            .join("operations/src/components/ui/sidebar.tsx")
            .exists()
    );

    for (index, dir) in [(0, "plain-admin-name"), (1, "operations")] {
        let ui = &report["frontends"][index]["ui"];
        let package: serde_json::Value =
            serde_json::from_slice(&fs::read(temp.path().join(dir).join("package.json")).unwrap())
                .unwrap();
        let components: serde_json::Value = serde_json::from_slice(
            &fs::read(temp.path().join(dir).join("components.json")).unwrap(),
        )
        .unwrap();
        let readme = fs::read_to_string(temp.path().join(dir).join("README.md")).unwrap();
        let cli_version = ui["cli_version"].as_str().unwrap();
        let preset = ui["preset"].as_str().unwrap();
        let base = ui["base"].as_str().unwrap();
        let base_display = format!("{}{}", base[..1].to_ascii_uppercase(), &base[1..]);
        let tailwind_major = ui["tailwind_major"].as_u64().unwrap();

        assert_eq!(package["dependencies"]["shadcn"], cli_version);
        assert_eq!(components["style"], ui["style"]);
        assert!(readme.contains(&format!("shadcn CLI {cli_version}")));
        assert!(readme.contains(&format!("`{preset}` preset")));
        assert!(readme.contains(&format!("{base_display} primitives")));
        assert!(readme.contains(&format!("Tailwind CSS {tailwind_major}")));
        assert!(readme.contains(&format!("shadcn@{cli_version} info")));
    }
}

#[test]
fn scaffold_rejects_unknown_frontend_role() {
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
            frontend_apps: vec![FrontendApp {
                name: "console".into(),
                dir: "console".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "dashboard".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported frontend app role 'dashboard'"));
    assert!(error.contains("spa, admin, or astro"));
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
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "marketing".into(),
                    dir: "shared".into(),
                    coverage_threshold: 0,
                    kind: "env-port".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate_dir.contains("Duplicate scaffold frontend dir 'shared'"));

    let duplicate_package_name = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![
                FrontendApp {
                    name: "foo_bar".into(),
                    dir: "foo_bar".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "foo-bar".into(),
                    dir: "foo-bar".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate_package_name.contains("names 'foo_bar' and 'foo-bar' normalize"));
    assert!(duplicate_package_name.contains("workspace package name 'foo-bar'"));

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
                role: "spa".into(),
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
                role: "spa".into(),
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
                role: "spa".into(),
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
fn scaffold_rejects_frontend_package_name_reserved_by_root_workspace() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("demo_workspace").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("frontend 'demo_workspace'"));
    assert!(error.contains("reserved root workspace package name 'demo-workspace'"));
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
                role: "spa".into(),
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
    assert!(main_rs.contains("use ::app_123_type_http as app_http_crate;"));
    assert!(main_rs.contains("app_http_crate::router"));
    let core_lib =
        fs::read_to_string(temp.path().join("crates/app-123-type-core/src/lib.rs")).unwrap();
    assert!(core_lib.contains("#[allow(clippy::useless_concat)]\npub const APP_NAME"));
    assert!(core_lib.contains("pub const APP_NAME: &str = concat!("));
    assert!(core_lib.contains("\"app-123-type\","));

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
fn run_init_sqlite_scaffold_keeps_sanitized_database_names_and_ignores_aligned() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Sqlite),
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
    assert_eq!(
        fs::read_to_string(destination.join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=app_123_type=info,app_123_type_api=info,tower_http=info\nDATABASE_URL=sqlite:app_123_type.db\n"
    );
    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/app_123_type.db\n"));
    assert!(gitignore.contains("/app_123_type.db-*\n"));
    for database_file in [
        "app_123_type.db",
        "app_123_type.db-wal",
        "app_123_type.db-shm",
        "app_123_type.db-journal",
        "app_123_type.db-jig-migrate.lock",
    ] {
        fs::write(destination.join(database_file), "local database artifact").unwrap();
    }
    assert_eq!(
        git_stdout(
            &destination,
            [
                "check-ignore",
                "--",
                "app_123_type.db",
                "app_123_type.db-wal",
                "app_123_type.db-shm",
                "app_123_type.db-journal",
                "app_123_type.db-jig-migrate.lock",
            ],
        )
        .unwrap(),
        "app_123_type.db\napp_123_type.db-wal\napp_123_type.db-shm\napp_123_type.db-journal\napp_123_type.db-jig-migrate.lock"
    );
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
            ci_github_runner: Some("macos-14".into()),
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
    assert!(cargo_toml.contains("\"signal\", \"sync\", \"time\""));
    assert!(cargo_toml.contains("fs4 = \"0.13.1\""));
    assert!(cargo_toml.contains("url = \"2\""));
    assert!(cargo_toml.ends_with('\n'));
    assert_eq!(
        fs::read_to_string(temp.path().join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=demo=info,demo_api=info,tower_http=info\nDATABASE_URL=sqlite:demo.db\n"
    );
    let db_cargo = fs::read_to_string(temp.path().join("crates/demo-db/Cargo.toml")).unwrap();
    assert!(db_cargo.contains("anyhow.workspace = true"));
    assert!(db_cargo.contains("fs4.workspace = true"));
    assert!(db_cargo.contains("url.workspace = true"));
    assert!(db_cargo.contains("tokio.workspace = true"));
    let db_lib = fs::read_to_string(temp.path().join("crates/demo-db/src/lib.rs")).unwrap();
    assert!(db_lib.contains("SqlitePool"));
    assert!(db_lib.contains("sqlx::Sqlite::database_exists"));
    assert!(db_lib.contains("OpenOptions::new()"));
    assert!(db_lib.contains(".create_new(true)"));
    assert!(db_lib.contains("options.get_filename()"));
    assert!(db_lib.contains("fs::create_dir_all(parent)"));
    assert!(!db_lib.contains("sqlx::Sqlite::create_database"));
    assert!(db_lib.contains("create_if_missing"));
    assert!(db_lib.contains("concurrent_create_if_missing_calls_are_idempotent"));
    assert!(db_lib.contains("sqlx::migrate!(\n"));
    assert!(db_lib.contains("\"../../db/migrations\"\n        )"));
    assert!(db_lib.contains("DEFAULT_DB_TIMEOUT"));
    assert!(db_lib.contains("connect_with_timeout"));
    assert!(db_lib.contains("fs::canonicalize(&database_filename)"));
    assert!(db_lib.contains("sqlite_database_url_is_in_memory"));
    assert!(db_lib.contains("sqlite_database_url_semantics"));
    assert!(db_lib.contains("requires_single_connection_pool"));
    assert!(db_lib.contains("SqlitePoolOptions::new()"));
    assert!(db_lib.contains(".max_connections(1)"));
    assert!(db_lib.contains(".min_connections(1)"));
    assert!(db_lib.contains(".idle_timeout(None)"));
    assert!(db_lib.contains(".max_lifetime(None)"));
    assert!(db_lib.contains(".test_before_acquire(false)"));
    assert!(!db_lib.contains("num_idle()"));
    assert!(db_lib.contains("mirrors_sqlx_ordered_in_memory_cache_semantics"));
    assert!(db_lib.contains("in_memory_mode_ignores_an_existing_filename_for_locking"));
    assert!(db_lib.contains("create_if_missing_does_not_materialize_an_in_memory_filename"));
    assert!(db_lib.contains("symlink_aliases_share_the_canonical_migration_lock"));
    assert!(db_lib.contains("migrate_with_timeout"));
    assert!(db_lib.contains("static SQLITE_MIGRATION_LOCK"));
    assert!(db_lib.contains("fs4::fs_std::FileExt::try_lock_exclusive(&file)"));
    assert!(db_lib.contains("Ok(true) => return Ok(Some(file))"));
    assert!(db_lib.contains("Ok(false) =>"));
    assert!(!db_lib.contains("fs4::lock_contended_error"));
    assert!(db_lib.contains("in_memory_database_connects_and_migrates_without_a_file_lock"));
    assert!(db_lib.contains("private_cache_in_memory_pool_waits_for_the_active_checkout"));
    assert!(db_lib.contains("shared_in_memory_urls_keep_multiple_schema_aware_connections"));
    assert!(db_lib.contains("ordinary_file_pool_keeps_multiple_schema_aware_connections"));
    assert!(db_lib.contains("migration_mutex_is_shared_by_separate_in_memory_connections"));
    assert!(temp.path().join("db/migrations/.gitkeep").exists());
    let playwright = fs::read_to_string(temp.path().join("web/playwright.config.ts")).unwrap();
    assert!(playwright.contains("E2E_DATABASE_URL"));
    assert!(playwright.contains("sqlite:${defaultDatabasePath}"));
    assert!(playwright.contains("demo_web_e2e.sqlite"));
    assert!(playwright.contains("-- --bootstrap-database"));
    assert!(playwright.contains("['','-shm','-wal','-journal']"));
    #[cfg(unix)]
    {
        let reset_line = playwright
            .lines()
            .find(|line| line.contains("node -e") && line.contains("fs.rmSync"))
            .unwrap()
            .trim();
        let reset_command = reset_line
            .strip_prefix('`')
            .and_then(|line| line.strip_suffix("`,"))
            .unwrap()
            .replace("${defaultDatabasePath}", ".agent/tmp/demo_web_e2e.sqlite");
        let database = temp.path().join(".agent/tmp/demo_web_e2e.sqlite");
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        for suffix in ["", "-shm", "-wal", "-journal"] {
            fs::write(format!("{}{}", database.display(), suffix), "stale\n").unwrap();
        }
        assert!(
            std::process::Command::new("bash")
                .args(["-c", &reset_command])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success()
        );
        for suffix in ["", "-shm", "-wal", "-journal"] {
            assert!(!Path::new(&format!("{}{}", database.display(), suffix)).exists());
        }
    }
    let workflow = fs::read_to_string(temp.path().join(".github/workflows/e2e.yml")).unwrap();
    let workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    assert_eq!(workflow_yaml["jobs"]["e2e"]["runs-on"], "macos-14");
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert!(!workflow.contains("image: postgres"));
    assert!(!workflow.contains("E2E_DATABASE_URL"));
    assert!(workflow.contains(r#"- "db/migrations/**""#));
    assert!(workflow.contains(r#"- ".sqlx/**""#));
    assert!(workflow.contains(r#"SQLX_OFFLINE: "true""#));
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
        "package.json",
        ".node-version",
        ".github/workflows/e2e.yml",
        "web/package.json",
        "web/.gitignore",
        "web/playwright.config.ts",
        "web/e2e/app.spec.ts",
        "web/components.json",
        "web/src/App.tsx",
        "web/src/api.ts",
        "web/src/components/ui/button.tsx",
        "web/src/lib/utils.ts",
        "landing/package.json",
        "landing/src/pages/index.astro",
        "admin-panel/package.json",
        "admin-panel/components.json",
        "admin-panel/src/app/router.tsx",
        "admin-panel/src/components/ui/sidebar.tsx",
        "admin-panel/src/features/overview/overview-page.tsx",
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
fn scaffold_test_support_uses_absolute_paths_for_local_module_name_collisions() {
    let temp = tempdir().unwrap();

    for repo_name in ["app", "db", "http", "responses"] {
        let destination = temp.path().join(repo_name);
        fs::create_dir(&destination).unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: Some(ScaffoldDb::Sqlite),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some(repo_name.into()),
                ..AnswerOpts::default()
            },
            &destination,
        )
        .unwrap()
        .unwrap();
        plan.write(&destination, false).unwrap();

        let module_name = repo_name.replace('-', "_");
        let test_support = destination
            .join("crates")
            .join(format!("{repo_name}-test-support"));
        let lib = fs::read_to_string(test_support.join("src/lib.rs")).unwrap();
        assert!(
            lib.contains(&format!("use ::{module_name} as app_crate;"))
                && lib.contains("app_crate::AppState::new()"),
            "application crate path was ambiguous for {repo_name}:\n{lib}"
        );
        let app = fs::read_to_string(test_support.join("src/app.rs")).unwrap();
        assert!(
            app.contains(&format!("use ::{module_name} as app_crate;"))
                && app.contains("app_crate::AppState::for_tests()"),
            "application crate path was ambiguous for {repo_name}:\n{app}"
        );
        let db = fs::read_to_string(test_support.join("src/db.rs")).unwrap();
        assert!(
            db.contains(&format!("use ::{module_name}_db as app_db_crate;"))
                && db.contains("pub type TestDbPool = app_db_crate::DbPool;"),
            "database crate path was ambiguous for {repo_name}:\n{db}"
        );

        if repo_name == "app" {
            let output = std::process::Command::new("cargo")
                .args(["fmt", "--all", "--", "--check"])
                .current_dir(&destination)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "cargo fmt failed for the colliding-name database scaffold\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
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

#[cfg(unix)]
#[test]
fn scaffold_preflight_rejects_symlink_boundaries_without_partial_or_outside_writes() {
    use std::os::unix::fs::symlink;

    fn plan_for(destination: &Path) -> scaffold::InitScaffoldPlan {
        scaffold::InitScaffoldPlan::from_opts(
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
            destination,
        )
        .unwrap()
        .unwrap()
    }

    for force in [false, true] {
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join(format!("outside-{force}.toml"));
        fs::write(&outside_file, "outside sentinel\n").unwrap();
        let destination = tempdir().unwrap();
        symlink(&outside_file, destination.path().join("Cargo.toml")).unwrap();

        let error = plan_for(destination.path())
            .write(destination.path(), force)
            .unwrap_err()
            .to_string();

        assert!(error.contains("is a symlink"), "{error}");
        assert_eq!(
            fs::read_to_string(&outside_file).unwrap(),
            "outside sentinel\n"
        );
        assert!(!destination.path().join("apps").exists());
        assert!(!destination.path().join("web").exists());
    }

    for force in [false, true] {
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join(format!("missing-{force}.toml"));
        let destination = tempdir().unwrap();
        symlink(&outside_file, destination.path().join("Cargo.toml")).unwrap();

        let error = plan_for(destination.path())
            .write(destination.path(), force)
            .unwrap_err()
            .to_string();

        assert!(error.contains("is a symlink"), "{error}");
        assert!(!outside_file.exists(), "broken link target was created");
        assert!(!destination.path().join("apps").exists());
        assert!(!destination.path().join("web").exists());
    }

    for force in [false, true] {
        let outside = tempdir().unwrap();
        let destination = tempdir().unwrap();
        symlink(outside.path(), destination.path().join("web")).unwrap();

        let error = plan_for(destination.path())
            .write(destination.path(), force)
            .unwrap_err()
            .to_string();

        assert!(error.contains("ancestor"), "{error}");
        assert!(error.contains("is a symlink"), "{error}");
        assert!(
            !destination.path().join("Cargo.toml").exists(),
            "a late unsafe output must fail before earlier scaffold files are published"
        );
        assert!(
            fs::read_dir(outside.path()).unwrap().next().is_none(),
            "scaffold wrote through a symlinked output ancestor"
        );
    }

    for force in [false, true] {
        let destination = tempdir().unwrap();
        fs::create_dir(destination.path().join("Cargo.toml")).unwrap();

        let error = plan_for(destination.path())
            .write(destination.path(), force)
            .unwrap_err()
            .to_string();

        assert!(error.contains("destination leaf"), "{error}");
        assert!(error.contains("is a directory"), "{error}");
        assert!(!destination.path().join("apps").exists());
        assert!(!destination.path().join("web").exists());
    }
}

#[cfg(unix)]
#[test]
fn init_preflights_scaffold_and_agent_map_outputs_before_rendering_the_harness() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for relative in ["Cargo.toml", managed_paths::AGENT_MAP_PATH] {
        let destination = temp.path().join(relative.replace(['/', '.'], "-"));
        fs::create_dir(&destination).unwrap();
        let outside = temp
            .path()
            .join(format!("outside-{}", relative.replace('/', "-")));
        fs::write(&outside, "outside sentinel\n").unwrap();
        symlink(&outside, destination.join(relative)).unwrap();

        let error = run_init(InitOpts {
            path: destination.clone(),
            scaffold: ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: Some(ScaffoldDb::None),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: true,
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

        assert!(error.contains("is a symlink"), "{relative}: {error}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside sentinel\n");
        assert!(
            !destination.join(".jig.toml").exists(),
            "managed rendering started before {relative} was rejected"
        );
        assert!(!destination.join("scripts/jig").exists());
    }
}

#[test]
fn init_rejects_portable_scaffold_output_collisions_before_any_repository_write() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let app = |name: &str, dir: &str| FrontendApp {
        name: name.into(),
        dir: dir.into(),
        coverage_threshold: 80,
        kind: "vite".into(),
        role: "spa".into(),
    };

    for force in [false, true] {
        for (case_name, frontend_apps, expected_paths) in [
            (
                "scaffold-file-ancestor",
                vec![app("client", "package.json")],
                ["package.json", "package.json/.gitignore"],
            ),
            (
                "template-file-ancestor",
                vec![app("client", "scripts/jig")],
                ["scripts/jig", "scripts/jig/.gitignore"],
            ),
            (
                "case-folded-frontends",
                vec![app("first", "Web"), app("second", "web")],
                ["Web/", "web/"],
            ),
        ] {
            let destination = temp.path().join(format!("{case_name}-{force}"));
            fs::create_dir(&destination).unwrap();
            let outside = temp.path().join(format!("outside-{case_name}-{force}"));
            fs::write(&outside, "outside sentinel\n").unwrap();

            let error = run_init(InitOpts {
                path: destination.clone(),
                scaffold: ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(ScaffoldDb::None),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                template: Some(template.path().display().to_string()),
                template_mode: None,
                vcs_ref: None,
                force,
                defaults: false,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    repo_name: Some("demo".into()),
                    frontend_apps,
                    ..AnswerOpts::default()
                },
            })
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("Portable planned repository file collision"),
                "{case_name}/{force}: {error}"
            );
            for expected in expected_paths {
                assert!(
                    error.contains(expected),
                    "{case_name}/{force}: missing {expected:?} in {error}"
                );
            }
            assert_eq!(fs::read_to_string(&outside).unwrap(), "outside sentinel\n");
            assert!(
                destination.is_dir(),
                "{case_name}/{force}: a pre-existing empty destination must remain"
            );
            assert!(
                fs::read_dir(&destination).unwrap().next().is_none(),
                "{case_name}/{force}: collision preflight partially mutated the destination"
            );
            assert!(!destination.join(".jig.toml").exists());
            assert!(!destination.join("scripts/jig").exists());
            assert!(!destination.join("Cargo.toml").exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn harness_only_init_rejects_win32_forbidden_managed_template_paths_before_publication() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    fs::write(
        template.path().join("templates/project/bad:name.jinja"),
        "nonportable\n",
    )
    .unwrap();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::HarnessOnly),
            ..ScaffoldOpts::default()
        },
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

    assert!(error.contains("not portable to Windows"), "{error}");
    assert!(error.contains("bad:name"), "{error}");
    assert!(!destination.exists());
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn harness_only_init_rejects_non_unicode_managed_parent_before_publication() {
    use std::os::unix::ffi::OsStringExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let invalid_parent =
        template
            .path()
            .join("templates/project")
            .join(std::ffi::OsString::from_vec(
                b"invalid-\xff-parent".to_vec(),
            ));
    fs::create_dir(&invalid_parent).unwrap();
    fs::write(invalid_parent.join("valid-leaf.jinja"), "nonportable\n").unwrap();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::HarnessOnly),
            ..ScaffoldOpts::default()
        },
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

    assert!(error.contains("valid Unicode"), "{error}");
    assert!(error.contains("valid-leaf"), "{error}");
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn adopt_rejects_win32_forbidden_managed_template_paths_before_repository_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    fs::write(
        template.path().join("templates/project/bad?name.jinja"),
        "nonportable\n",
    )
    .unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("sentinel"), "preserve\n").unwrap();

    let error = run_adopt(AdoptOpts {
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
    .unwrap_err()
    .to_string();

    assert!(error.contains("not portable to Windows"), "{error}");
    assert_eq!(
        fs::read_to_string(repo.join("sentinel")).unwrap(),
        "preserve\n"
    );
    assert!(!repo.join("bad?name").exists());
    assert!(!repo.join(".jig.toml").exists());
}

#[cfg(unix)]
#[test]
fn update_rejects_new_control_bearing_managed_template_paths_before_repository_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("sentinel"), "preserve\n").unwrap();
    let template_path = template.path().display().to_string();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template_path.clone()),
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
    let answers_before = fs::read(repo.join(".jig.toml")).unwrap();
    fs::write(
        template
            .path()
            .join("templates/project/bad\u{1f}name.jinja"),
        "nonportable\n",
    )
    .unwrap();

    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: Some(template_path),
        template_mode: None,
        recopy: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("not portable to Windows"), "{error}");
    assert_eq!(fs::read(repo.join(".jig.toml")).unwrap(), answers_before);
    assert_eq!(
        fs::read_to_string(repo.join("sentinel")).unwrap(),
        "preserve\n"
    );
    assert!(!repo.join("bad\u{1f}name").exists());
}

#[test]
fn init_rolls_back_new_destination_after_planned_output_collision() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let app = |name: &str, dir: &str| FrontendApp {
        name: name.into(),
        dir: dir.into(),
        coverage_threshold: 80,
        kind: "vite".into(),
        role: "spa".into(),
    };

    for force in [false, true] {
        for (case_name, frontend_apps) in [
            (
                "internal-case-folded-frontends",
                vec![app("first", "Web"), app("second", "web")],
            ),
            (
                "managed-scaffold-ancestor",
                vec![app("client", "scripts/jig")],
            ),
        ] {
            let created_ancestor = temp.path().join(format!("{case_name}-{force}"));
            let destination = created_ancestor.join("nested/new-repo");
            assert!(!destination.exists());

            let error = run_init(InitOpts {
                path: destination.clone(),
                scaffold: ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(ScaffoldDb::None),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                template: Some(template.path().display().to_string()),
                template_mode: None,
                vcs_ref: None,
                force,
                defaults: false,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    repo_name: Some("demo".into()),
                    frontend_apps,
                    ..AnswerOpts::default()
                },
            })
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("Portable planned repository file collision"),
                "{case_name}/{force}: {error}"
            );
            assert!(
                !destination.exists(),
                "{case_name}/{force}: failed init left its new destination behind"
            );
            assert!(
                !created_ancestor.exists(),
                "{case_name}/{force}: failed init left created parent directories behind"
            );
        }
    }
}

#[test]
fn init_destination_rollback_preserves_existing_and_concurrently_created_destinations() {
    let temp = tempdir().unwrap();

    let pre_existing = temp.path().join("pre-existing");
    fs::create_dir(&pre_existing).unwrap();
    InitMutationTransaction::create(&pre_existing)
        .unwrap()
        .rollback()
        .unwrap();
    assert!(pre_existing.is_dir());

    let with_content = temp.path().join("created/with-content");
    let mut rollback = InitMutationTransaction::create(&with_content).unwrap();
    fs::create_dir_all(&with_content).unwrap();
    fs::write(with_content.join("concurrent.txt"), "preserve\n").unwrap();
    rollback.rollback().unwrap();
    assert_eq!(
        fs::read_to_string(with_content.join("concurrent.txt")).unwrap(),
        "preserve\n"
    );
    assert!(temp.path().join("created").is_dir());
}

#[cfg(unix)]
#[test]
fn init_rejects_an_existing_final_symlink_destination() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    fs::create_dir(&target).unwrap();
    symlink(&target, &link).unwrap();
    let resolved = path::resolve_init_destination(&link, temp.path()).unwrap();
    assert_eq!(resolved, link);
    let error = validate_init_destination(&resolved, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a real directory"), "{error}");
}

#[cfg(unix)]
#[test]
fn missing_init_tree_is_private_then_published_with_normal_directory_mode() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let probe = temp.path().join("mode-probe");
    fs::create_dir(&probe).unwrap();
    let expected_mode = fs::metadata(&probe).unwrap().permissions().mode() & 0o777;
    fs::remove_dir(&probe).unwrap();

    let destination = temp.path().join("new-top/nested/repo");
    let mut transaction = InitMutationTransaction::create(&destination).unwrap();
    let staging = transaction
        .staged_publication
        .as_ref()
        .unwrap()
        .publish_source
        .clone();
    assert_eq!(
        fs::metadata(&staging).unwrap().permissions().mode() & 0o777,
        0o700
    );
    fs::write(
        transaction.work_destination().join("sentinel"),
        "complete\n",
    )
    .unwrap();
    assert!(!destination.exists());

    transaction.commit().unwrap();
    assert_eq!(
        fs::metadata(temp.path().join("new-top"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        expected_mode
    );
    assert_eq!(
        fs::read_to_string(destination.join("sentinel")).unwrap(),
        "complete\n"
    );
    assert!(!staging.exists());
}

#[test]
fn missing_init_tree_publication_never_replaces_concurrent_top_component() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("contended/nested/repo");
    let mut transaction = InitMutationTransaction::create(&destination).unwrap();
    let staging = transaction
        .staged_publication
        .as_ref()
        .unwrap()
        .publish_source
        .clone();
    fs::write(transaction.work_destination().join("generated"), "jig\n").unwrap();
    fs::create_dir(temp.path().join("contended")).unwrap();
    fs::write(temp.path().join("contended/foreign"), "preserve\n").unwrap();

    let error = transaction.commit().unwrap_err().to_string();
    assert!(
        error.contains("without replacing concurrent path"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("contended/foreign")).unwrap(),
        "preserve\n"
    );
    assert!(!destination.exists());
    assert!(!staging.exists());
}

#[cfg(unix)]
#[test]
fn missing_init_tree_rejects_an_intermediate_symlink_swap_before_file_publication() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let destination = temp.path().join("new-top/nested/repo");
    let mut transaction = InitMutationTransaction::create(&destination).unwrap();
    let relative = Path::new("generated");
    transaction.prepare_file_publication(relative).unwrap();

    let staging = transaction
        .staged_publication
        .as_ref()
        .unwrap()
        .publish_source
        .clone();
    let intermediate = staging.join("nested");
    let retained_intermediate = staging.join("nested-original");
    let foreign_intermediate = temp.path().join("foreign-nested");
    fs::create_dir(&foreign_intermediate).unwrap();
    fs::create_dir(foreign_intermediate.join("repo")).unwrap();
    fs::write(foreign_intermediate.join("marker"), "preserve\n").unwrap();
    fs::rename(&intermediate, &retained_intermediate).unwrap();
    symlink(&foreign_intermediate, &intermediate).unwrap();

    let error = path::write_repository_file_atomic_staged(
        transaction.work_destination(),
        relative,
        b"jig\n",
        path::RepositoryFileLeaf::Missing,
        || transaction.verify_destination_identity(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("replaced while init was running"), "{error}");
    assert!(!foreign_intermediate.join("repo/generated").exists());
    assert_eq!(
        fs::read_to_string(foreign_intermediate.join("marker")).unwrap(),
        "preserve\n"
    );

    let rollback = transaction.rollback().unwrap_err().to_string();
    assert!(rollback.contains("Preserving the complete staging tree"));
    fs::remove_file(&intermediate).unwrap();
    fs::rename(&retained_intermediate, &intermediate).unwrap();
    fs::remove_dir_all(&staging).unwrap();
}

#[test]
fn second_disposal_quarantine_preserves_post_inspection_replacements() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir(&root).unwrap();
    let transaction = InitMutationTransaction::create(&root).unwrap();

    let inspected_file = root.join("inspected-file");
    let retained_file = root.join("retained-file");
    fs::write(&inspected_file, "jig\n").unwrap();
    let expected_file = transaction.snapshot_absolute_path(&inspected_file).unwrap();
    fs::rename(&inspected_file, &retained_file).unwrap();
    fs::write(&inspected_file, "foreign\n").unwrap();
    let error = transaction
        .dispose_snapshot_leaf(Path::new("managed"), &inspected_file, &expected_file)
        .unwrap_err()
        .to_string();
    assert!(error.contains("refusing to unlink replacement"), "{error}");
    assert_eq!(fs::read_to_string(&inspected_file).unwrap(), "foreign\n");
    assert_eq!(fs::read_to_string(&retained_file).unwrap(), "jig\n");

    let inspected_directory = root.join("inspected-directory");
    let retained_directory = root.join("retained-directory");
    fs::create_dir(&inspected_directory).unwrap();
    let expected_directory = path::repository_directory_commit_at(&inspected_directory).unwrap();
    fs::rename(&inspected_directory, &retained_directory).unwrap();
    fs::create_dir(&inspected_directory).unwrap();
    fs::write(inspected_directory.join("foreign"), "preserve\n").unwrap();
    let error = transaction
        .dispose_empty_owned_directory(
            Path::new("owned"),
            &inspected_directory,
            &inspected_directory,
            expected_directory,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("refusing to remove replacement"), "{error}");
    assert_eq!(
        fs::read_to_string(inspected_directory.join("foreign")).unwrap(),
        "preserve\n"
    );
    assert!(retained_directory.is_dir());
}

#[test]
fn retained_generation_budget_fails_before_a_low_soft_handle_limit() {
    let planned = (0..12)
        .map(|index| PathBuf::from(format!("nested/{index}/generated")))
        .collect::<BTreeSet<_>>();
    let repeated_generation_count = 2;
    let required = retained_generation_handle_requirement(&planned, repeated_generation_count);

    let error = validate_retained_generation_budget(
        &planned,
        repeated_generation_count,
        Some(required + 9),
        10,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("soft handle limit"), "{error}");
    validate_retained_generation_budget(
        &planned,
        repeated_generation_count,
        Some(required + 10),
        10,
    )
    .unwrap();
}

#[test]
fn retained_generation_budget_caps_planned_and_repeated_generations_together() {
    let planned = (0..MAX_EXISTING_INIT_RETAINED_GENERATIONS)
        .map(|index| PathBuf::from(format!("generated-{index}")))
        .collect::<BTreeSet<_>>();

    validate_retained_generation_budget(&planned, 0, None, 0).unwrap();
    let error = validate_retained_generation_budget(&planned, 1, None, 0)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&format!(
            "plans {} generated file generations",
            MAX_EXISTING_INIT_RETAINED_GENERATIONS + 1
        )),
        "{error}"
    );
}

#[test]
fn retained_generation_model_matches_preimages_first_outputs_and_explicit_repeats() {
    fn snapshot_handle_count(snapshot: &InitPathSnapshot) -> usize {
        usize::from(!matches!(snapshot, InitPathSnapshot::Missing))
    }

    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join("nested")).unwrap();
    let first = Path::new("nested/first");
    let second = Path::new("nested/second");
    fs::write(root.join(first), "first preimage\n").unwrap();
    fs::write(root.join(second), "second preimage\n").unwrap();

    let mut transaction = InitMutationTransaction::create(&root).unwrap();
    publish_existing_transaction_file(&mut transaction, first, b"first Jig generation\n");
    publish_existing_transaction_file(&mut transaction, second, b"second Jig generation\n");
    publish_existing_transaction_file(&mut transaction, first, b"repeated Jig generation\n");

    let retained_file_generations = transaction
        .files
        .values()
        .map(|mutation| {
            snapshot_handle_count(&mutation.before)
                + mutation
                    .expected_jig_states
                    .iter()
                    .map(snapshot_handle_count)
                    .sum::<usize>()
        })
        .sum::<usize>();
    assert_eq!(retained_file_generations, 2 * 2 + 1);

    let planned = BTreeSet::from([first.to_path_buf(), second.to_path_buf()]);
    assert_eq!(
        retained_generation_handle_requirement(&planned, 1),
        retained_file_generations
            + 1 // one retained directory prefix: nested
            + 1 // one private write-staging directory for that parent
            + RETAINED_GENERATION_HANDLE_HEADROOM
    );

    transaction.rollback().unwrap();
    assert_eq!(
        fs::read_to_string(root.join(first)).unwrap(),
        "first preimage\n"
    );
    assert_eq!(
        fs::read_to_string(root.join(second)).unwrap(),
        "second preimage\n"
    );
}

#[cfg(unix)]
const EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV: &str =
    "JIG_TEST_EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER";
#[cfg(unix)]
const EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_TEST: &str = "bootstrap::tests::basic::existing_empty_default_init_succeeds_with_256_soft_handle_limit_helper";

#[cfg(unix)]
#[test]
fn existing_empty_default_init_succeeds_with_256_soft_handle_limit() {
    let _guard = lock_env();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_TEST,
            "--nocapture",
        ])
        .env(EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV, "1")
        .env_remove(GIT_BIN_ENV)
        .env_remove(path::INVOCATION_CWD_ENV)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "soft-limit init helper failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn existing_empty_default_init_succeeds_with_256_soft_handle_limit_helper() {
    if std::env::var_os(EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV).is_none() {
        return;
    }

    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: the isolated helper owns this process and `limit` is writable.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0,
        "failed to read helper descriptor limit: {}",
        std::io::Error::last_os_error()
    );
    let requested: libc::rlim_t = 256;
    assert!(
        limit.rlim_max == libc::RLIM_INFINITY || limit.rlim_max >= requested,
        "helper hard descriptor limit {} is below {requested}",
        limit.rlim_max
    );
    limit.rlim_cur = requested;
    // SAFETY: this limit change is confined to the isolated helper subprocess.
    assert_eq!(
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) },
        0,
        "failed to set helper descriptor limit: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(process_soft_handle_limit(), Some(256));

    let temp = tempdir().unwrap();
    let destination = temp.path().join("existing-empty");
    fs::create_dir(&destination).unwrap();
    let report = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap();

    assert_eq!(report["scaffold"]["preset"], "rust-react");
    assert!(destination.join(".jig.toml").is_file());
    assert!(
        destination
            .join("apps/rollback-demo-api/Cargo.toml")
            .is_file()
    );
    assert!(destination.join("web/e2e/app.spec.ts").is_file());
}

fn publish_existing_transaction_file(
    transaction: &mut InitMutationTransaction,
    relative: &Path,
    contents: &[u8],
) {
    transaction
        .plan_regular_file_bytes(relative, contents)
        .unwrap();
    transaction.prepare_file_publication(relative).unwrap();
    let permissions = transaction.publication_permissions(relative).unwrap();
    let staging = transaction
        .write_staging_path(relative)
        .unwrap()
        .to_path_buf();
    let commit = path::write_repository_file_atomic_guarded(
        transaction.work_destination(),
        relative,
        contents,
        permissions,
        &staging,
        || transaction.verify_destination_identity(),
    )
    .unwrap();
    transaction.record_regular_commit(relative, commit).unwrap();
}

#[cfg(unix)]
#[test]
fn guarded_publication_rejects_root_and_nested_parent_swaps_without_touching_foreign_trees() {
    for swap_nested_parent in [false, true] {
        let temp = tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        let relative = if swap_nested_parent {
            fs::create_dir(root.join("scripts")).unwrap();
            Path::new("scripts/generated")
        } else {
            Path::new("generated")
        };
        let mut transaction = InitMutationTransaction::create(&root).unwrap();
        transaction
            .plan_regular_file_bytes(relative, b"jig\n")
            .unwrap();
        transaction.prepare_file_publication(relative).unwrap();
        let staging = transaction
            .write_staging_path(relative)
            .unwrap()
            .to_path_buf();
        let root_identity = path::repository_path_identity(&root).unwrap();
        let nested_identity = swap_nested_parent
            .then(|| path::repository_path_identity(&root.join("scripts")).unwrap());
        let moved = temp.path().join(if swap_nested_parent {
            "moved-scripts"
        } else {
            "moved-repo"
        });
        let mut checks = 0;
        let error = path::write_repository_file_atomic_guarded(
            &root,
            relative,
            b"jig\n",
            None,
            &staging,
            || {
                checks += 1;
                if checks == 2 {
                    if swap_nested_parent {
                        fs::rename(root.join("scripts"), &moved)?;
                        fs::create_dir(root.join("scripts"))?;
                        fs::write(root.join("scripts/foreign"), "preserve\n")?;
                    } else {
                        fs::rename(&root, &moved)?;
                        fs::create_dir(&root)?;
                        fs::write(root.join("foreign"), "preserve\n")?;
                    }
                }
                if path::repository_path_identity(&root)? != root_identity {
                    bail!("root changed at guarded publication boundary");
                }
                if let Some(expected) = &nested_identity {
                    if path::repository_path_identity(&root.join("scripts"))? != *expected {
                        bail!("nested parent changed at guarded publication boundary");
                    }
                }
                Ok(())
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("changed at guarded publication boundary"),
            "{error}"
        );
        let foreign_root = if swap_nested_parent {
            root.join("scripts")
        } else {
            root.clone()
        };
        assert_eq!(
            fs::read_to_string(foreign_root.join("foreign")).unwrap(),
            "preserve\n"
        );
        assert!(!foreign_root.join("generated").exists());
        assert!(!moved.join("generated").exists());
        let _ = transaction.rollback();
    }
}

#[test]
fn rollback_preserves_same_inode_foreign_rewrite_and_recreated_owned_directory() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir(&root).unwrap();

    let mut file_transaction = InitMutationTransaction::create(&root).unwrap();
    publish_existing_transaction_file(&mut file_transaction, Path::new("managed"), b"jig-state\n");
    fs::write(root.join("managed"), b"foreign!!\n").unwrap();
    let error = file_transaction.rollback().unwrap_err().to_string();
    assert!(error.contains("changed after Jig wrote it"), "{error}");
    assert_eq!(fs::read(root.join("managed")).unwrap(), b"foreign!!\n");

    let mut directory_transaction = InitMutationTransaction::create(&root).unwrap();
    publish_existing_transaction_file(
        &mut directory_transaction,
        Path::new("owned/generated"),
        b"jig\n",
    );
    fs::remove_file(root.join("owned/generated")).unwrap();
    fs::remove_dir(root.join("owned")).unwrap();
    fs::create_dir(root.join("owned")).unwrap();
    fs::write(root.join("owned/foreign"), "preserve\n").unwrap();
    let error = directory_transaction.rollback().unwrap_err().to_string();
    assert!(error.contains("owned ancestor"), "{error}");
    assert_eq!(
        fs::read_to_string(root.join("owned/foreign")).unwrap(),
        "preserve\n"
    );
}

#[cfg(unix)]
#[test]
fn late_init_failure_removes_managed_scaffold_agent_map_and_partial_git_outputs() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let git = temp.path().join("failing-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    mkdir -p .git/objects/aa\n    printf 'ref: refs/heads/main\\n' > .git/HEAD\n    printf 'partial\\n' > .git/objects/aa/object\n    printf 'fatal: injected late failure\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);

    let created_parent = temp.path().join("created-parent");
    let destination = created_parent.join("nested/repo");
    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("git init -b main failed"), "{error}");
    assert!(error.contains("injected late failure"), "{error}");
    assert!(
        !destination.exists(),
        "late failure left generated repo output"
    );
    assert!(
        !created_parent.exists(),
        "late failure left transaction-owned parent directories"
    );

    let existing = temp.path().join("existing-empty");
    fs::create_dir(&existing).unwrap();
    with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(existing.clone(), false))
    })
    .unwrap_err();
    assert!(existing.is_dir());
    assert!(fs::read_dir(&existing).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn late_forced_init_failure_restores_user_files_bytes_and_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let git = temp.path().join("failing-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    printf 'fatal: injected rollback test\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);
    let destination = temp.path().join("existing");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join(".gitignore"), b"user bytes\n").unwrap();
    fs::set_permissions(
        destination.join(".gitignore"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    fs::write(destination.join("sentinel.txt"), "keep me\n").unwrap();
    let before = regular_file_tree_snapshot(&destination);

    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), true))
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("injected rollback test"), "{error}");
    assert_eq!(regular_file_tree_snapshot(&destination), before);
    assert_eq!(
        fs::metadata(destination.join(".gitignore"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn late_init_rollback_preserves_foreign_file_changes_and_surfaces_both_failures() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let git = temp.path().join("mutating-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    printf 'foreign concurrent contents\\n' > \"$JIG_TEST_FOREIGN_DESTINATION/.jig.toml\"\n    printf 'fatal: injected primary failure\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);
    let destination = temp.path().join("existing");
    fs::create_dir(&destination).unwrap();
    let _destination = EnvVarGuard::set("JIG_TEST_FOREIGN_DESTINATION", destination.as_os_str());

    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("injected primary failure"), "{error}");
    assert!(
        error.contains("failed to roll back init changes"),
        "{error}"
    );
    assert!(
        error.contains(".jig.toml changed after Jig wrote it"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(destination.join(".jig.toml")).unwrap(),
        "foreign concurrent contents\n"
    );
    let remaining = regular_file_tree_snapshot(&destination);
    assert_eq!(remaining.len(), 1, "{remaining:?}");
    assert!(remaining.contains_key(Path::new(".jig.toml")));
}

#[cfg(unix)]
#[test]
fn failed_staged_git_init_never_claims_concurrent_destination_metadata() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let destination = temp.path().join("existing");
    fs::create_dir(&destination).unwrap();
    let git = temp.path().join("concurrent-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    mkdir -p \"$JIG_TEST_CONCURRENT_GIT_DESTINATION/.git\"\n    printf 'foreign git metadata\\n' > \"$JIG_TEST_CONCURRENT_GIT_DESTINATION/.git/foreign\"\n    mkdir -p .git/objects\n    printf 'partial staged metadata\\n' > .git/HEAD\n    printf 'fatal: staged git failure\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);
    let _destination = EnvVarGuard::set(
        "JIG_TEST_CONCURRENT_GIT_DESTINATION",
        destination.as_os_str(),
    );

    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("staged git failure"), "{error}");
    assert!(
        !error.contains("failed to roll back init changes"),
        "foreign .git must not be transaction-owned: {error}"
    );
    assert_eq!(
        fs::read_to_string(destination.join(".git/foreign")).unwrap(),
        "foreign git metadata\n"
    );
    assert!(!destination.join(".jig.toml").exists());
    assert!(!destination.join("Cargo.toml").exists());
    assert!(fs::read_dir(&destination).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".jig-git-init-")
    }));
}

#[test]
fn init_destination_accepts_an_existing_real_directory_after_create_is_denied() {
    let temp = tempdir().unwrap();
    let existing = temp.path().join("existing");
    fs::create_dir(&existing).unwrap();

    validate_existing_init_directory_after_create_error(
        &existing,
        io::Error::new(io::ErrorKind::PermissionDenied, "root create denied"),
        true,
    )
    .unwrap();

    let file = temp.path().join("file");
    fs::write(&file, "not a directory\n").unwrap();
    let error = validate_existing_init_directory_after_create_error(
        &file,
        io::Error::new(io::ErrorKind::AlreadyExists, "already exists"),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("not a real directory"), "{error}");
}

#[cfg(unix)]
#[test]
fn init_destination_never_accepts_an_existing_directory_symlink_after_create_fails() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    fs::create_dir(&target).unwrap();
    symlink(&target, &link).unwrap();

    let error = validate_existing_init_directory_after_create_error(
        &link,
        io::Error::new(io::ErrorKind::AlreadyExists, "already exists"),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("not a real directory"), "{error}");
}

#[test]
fn init_rejects_windows_aliased_scaffold_components_before_any_repository_write() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for force in [false, true] {
        for (case_name, frontend_dir) in [
            ("trailing-dot", "web."),
            ("device", "CON"),
            ("device-extension", "NUL.txt"),
        ] {
            let destination = temp.path().join(format!("{case_name}-{force}"));
            fs::create_dir(&destination).unwrap();
            let outside = temp.path().join(format!("outside-{case_name}-{force}"));
            fs::write(&outside, "outside sentinel\n").unwrap();

            let error = run_init(InitOpts {
                path: destination.clone(),
                scaffold: ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(ScaffoldDb::None),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                template: Some(template.path().display().to_string()),
                template_mode: None,
                vcs_ref: None,
                force,
                defaults: false,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    repo_name: Some("demo".into()),
                    frontend_apps: vec![FrontendApp {
                        name: "client".into(),
                        dir: frontend_dir.into(),
                        coverage_threshold: 80,
                        kind: "vite".into(),
                        role: "spa".into(),
                    }],
                    ..AnswerOpts::default()
                },
            })
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("not portable to Windows"),
                "{case_name}/{force}: {error}"
            );
            assert!(error.contains(frontend_dir), "{case_name}/{force}: {error}");
            assert_eq!(fs::read_to_string(&outside).unwrap(), "outside sentinel\n");
            assert!(
                fs::read_dir(&destination).unwrap().next().is_none(),
                "{case_name}/{force}: portability preflight partially mutated the destination"
            );
        }
    }
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
fn adopt_resolves_relative_answers_file_from_the_launcher_invocation_directory() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let invocation = temp.path().join("invocation");
    let other = temp.path().join("other");
    let repo = invocation.join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&other).unwrap();
    fs::write(
        invocation.join("answers.toml"),
        "repo_name = \"invocation-answers\"\nsqlx_enabled = false\n",
    )
    .unwrap();
    fs::write(
        other.join("answers.toml"),
        "repo_name = \"process-cwd-answers\"\nsqlx_enabled = false\n",
    )
    .unwrap();
    let template = materialize_template_worktree();
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());
    let _cwd = CurrentDirGuard::set(&other);

    run_adopt(AdoptOpts {
        path: PathBuf::from("repo"),
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
            answers_file: Some(PathBuf::from("answers.toml")),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(config.contains("repo_name = \"invocation-answers\""));
    assert!(!config.contains("process-cwd-answers"));
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
    assert!(answers.contains(
        "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]"
    ));
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
fn init_rejects_parent_components_before_answers_or_directory_creation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let invocation = temp.path().join("caller");
    fs::create_dir(&invocation).unwrap();
    fs::create_dir(invocation.join("existing")).unwrap();
    fs::write(invocation.join("existing/sentinel.txt"), "preserve\n").unwrap();
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());

    for force in [false, true] {
        for requested in ["missing/../existing", "missing/.."] {
            let opts = InitOpts {
                path: PathBuf::from(requested),
                scaffold: ScaffoldOpts::default(),
                template: None,
                template_mode: None,
                vcs_ref: None,
                force,
                defaults: false,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    answers_file: Some(invocation.join("answers-that-must-not-be-read.toml")),
                    ..AnswerOpts::default()
                },
            };

            for error in [
                preflight_init_destination(&opts).unwrap_err(),
                run_init(opts).unwrap_err(),
            ] {
                let error = error.to_string();
                assert!(
                    error.contains("must not contain '..'"),
                    "{requested}: {error}"
                );
                assert!(
                    !error.contains("answers-that-must-not-be-read"),
                    "{requested}: {error}"
                );
            }
            assert!(!invocation.join("missing").exists());
            assert_eq!(
                fs::read_to_string(invocation.join("existing/sentinel.txt")).unwrap(),
                "preserve\n"
            );
        }
    }
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
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nprevious=\nfor arg in \"$@\"; do\n  if [ \"$previous\" = \"init\" ] && [ \"$arg\" = \"-b\" ]; then\n    printf 'error: unknown switch `b`\\n' >&2\n    exit 129\n  fi\n  previous=$arg\ndone\nexec git \"$@\"\n",
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
    assert!(log.contains(" init -b trunk"));
    assert!(log.lines().any(|line| line.ends_with(" init")));
    assert!(log.contains(" symbolic-ref HEAD refs/heads/trunk"));
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
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nprevious=\nfor arg in \"$@\"; do\n  if [ \"$previous\" = \"init\" ] && [ \"$arg\" = \"-b\" ]; then\n    printf 'fatal: repository storage is broken\\n' >&2\n    exit 1\n  fi\n  previous=$arg\ndone\nexec git \"$@\"\n",
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

    assert!(error.contains("git init -b main failed"), "{error}");
    assert!(error.contains("repository storage is broken"), "{error}");
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains(" init -b main"));
    assert!(!log.contains(" symbolic-ref HEAD refs/heads/main"));
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
