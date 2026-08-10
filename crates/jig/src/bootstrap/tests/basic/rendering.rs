use super::*;

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
        "scripts/jig setup",
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
    assert!(
        no_bootstrap_steps
            .iter()
            .any(|step| step == "scripts/jig setup")
    );
    let no_bootstrap_report = initial_command_report(&initial_copy::BootstrapCopyResult {
        default_branch: Some("main".into()),
        bootstrap_command_configured: false,
        frontend_apps_configured: false,
        dev_apps_configured: false,
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
        let _ = writeln!(
            body,
            "sqlx_enabled = {}",
            if sqlx_enabled { "true" } else { "false" }
        );
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
            conflict_policy: ApplyRenderConflictPolicy::Accept,
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            dry_run: false,
            backup_root: None,
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
            conflict_policy: ApplyRenderConflictPolicy::Accept,
            allow_answers_overwrite: false,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            dry_run: false,
            backup_root: None,
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
            conflict_policy: ApplyRenderConflictPolicy::Accept,
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            dry_run: false,
            backup_root: None,
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
            conflict_policy: ApplyRenderConflictPolicy::Accept,
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            dry_run: false,
            backup_root: None,
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
            conflict_policy: ApplyRenderConflictPolicy::Reject("conflict"),
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            dry_run: false,
            backup_root: None,
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
                conflict_policy: if force {
                    ApplyRenderConflictPolicy::Accept
                } else {
                    ApplyRenderConflictPolicy::Reject("conflict")
                },
                dry_run,
                allow_answers_overwrite: false,
                allow_contract_overwrite: false,
                allow_manifest_overwrite: false,
                backup_root: None,
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
                    conflict_policy: if force {
                        ApplyRenderConflictPolicy::Accept
                    } else {
                        ApplyRenderConflictPolicy::Reject("re-run with --force")
                    },
                    dry_run,
                    allow_answers_overwrite: false,
                    allow_contract_overwrite: false,
                    allow_manifest_overwrite: false,
                    backup_root: None,
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
                conflict_policy: if force {
                    ApplyRenderConflictPolicy::Accept
                } else {
                    ApplyRenderConflictPolicy::Reject("re-run with --force")
                },
                dry_run,
                allow_answers_overwrite: false,
                allow_contract_overwrite: false,
                allow_manifest_overwrite: false,
                backup_root: None,
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
            conflict_policy: ApplyRenderConflictPolicy::Accept,
            dry_run: false,
            allow_answers_overwrite: false,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            backup_root: None,
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
                conflict_policy: ApplyRenderConflictPolicy::Accept,
                dry_run: false,
                allow_answers_overwrite: false,
                allow_contract_overwrite: false,
                allow_manifest_overwrite: false,
                backup_root: Some(&destination.path().join("backups")),
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
            conflict_policy: ApplyRenderConflictPolicy::Accept,
            dry_run: false,
            allow_answers_overwrite: false,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: false,
            backup_root: Some(&backup_root),
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
