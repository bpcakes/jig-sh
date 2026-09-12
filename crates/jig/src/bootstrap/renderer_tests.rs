use crate::backend::BackendLanguage;
use crate::bootstrap::AnswerOpts;
use crate::bootstrap::answers::AnswerResolution;
use crate::bootstrap::repository_model::RepositoryProjectionHint;
use crate::bootstrap::template_source::PrivateAnswerOverrides;

use super::*;

#[path = "renderer_tests/freshness.rs"]
mod freshness;
#[path = "renderer_tests/verification.rs"]
mod verification;

fn rust_render_answers(projection: RepositoryProjectionHint) -> RenderAnswers {
    let destination = tempfile::tempdir().unwrap();
    let opts = AnswerOpts {
        repo_name: Some("ExampleProject".into()),
        backend_language: Some(BackendLanguage::Rust),
        repository_projection_hint: projection,
        sqlx_enabled: Some(false),
        schema_dump_enabled: Some(false),
        rust_crate_roots: vec!["crates".into()],
        ..AnswerOpts::default()
    };
    AnswerResolution::from_opts(&opts, destination.path(), false)
        .unwrap()
        .into_parts()
        .0
}

fn live_template_source() -> PreparedTemplateSource {
    PreparedTemplateSource::test_local(
        "fixture".into(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        None,
        PrivateAnswerOverrides::default(),
    )
}

#[test]
fn root_guidance_matches_rendered_native_and_legacy_work_gates() {
    let template = live_template_source();
    let selected = BTreeSet::from([PathBuf::from(".jig.toml"), PathBuf::from("AGENTS.md")]);
    for language in [BackendLanguage::Rust, BackendLanguage::Go] {
        for version in [5, 8] {
            let destination = tempfile::tempdir().unwrap();
            let answers = AnswerResolution::from_opts(
                &AnswerOpts {
                    repo_name: Some("ExampleProject".into()),
                    backend_language: Some(language),
                    sqlx_enabled: Some(language == BackendLanguage::Rust),
                    schema_dump_enabled: Some(language == BackendLanguage::Rust),
                    rust_migration_dir: (language == BackendLanguage::Rust)
                        .then(|| "migrations".into()),
                    go_database: (language == BackendLanguage::Go)
                        .then_some(crate::backend::GoDatabase::Postgres),
                    application_contracts_enabled: Some(true),
                    frontend_apps: vec![crate::bootstrap::FrontendApp {
                        name: "web".into(),
                        dir: "apps/web".into(),
                        coverage_threshold: 80,
                        kind: "vite".into(),
                        role: "spa".into(),
                    }],
                    ..AnswerOpts::default()
                },
                destination.path(),
                false,
            )
            .unwrap()
            .into_parts()
            .0;
            render_template_files(
                &template,
                &answers,
                destination.path(),
                Some(&selected),
                Some(version),
            )
            .unwrap();
            let config: toml::Value =
                toml::from_str(&fs::read_to_string(destination.path().join(".jig.toml")).unwrap())
                    .unwrap();
            let gates = config["work"]["gates"].as_array().unwrap();
            assert!(!gates.is_empty());
            let native = gates
                .iter()
                .all(|gate| gate["kind"].as_str() == Some("evidence"));
            assert_eq!(native, version >= 6);
            verification::assert_required_coverage(&config, native, language);
            let guide = fs::read_to_string(destination.path().join("AGENTS.md")).unwrap();
            assert_eq!(guide.contains("configured repository profile"), native);
            assert_eq!(
                guide.contains("Structured work uses configured check gates"),
                !native
            );
            assert_eq!(
                guide.contains("reuses current passing target evidence"),
                native
            );
            assert!(!guide.contains("four atomic path-aware gates per app"));
            assert!(!guide.contains("have separate `application-contracts`"));
            assert!(guide.contains("evidence must cover the configured tests"));
            assert!(!guide.contains("finish with `scripts/jig check test`"));
            assert!(guide.contains("`scripts/jig dev`"));
            assert!(guide.contains("`kind` selects `vite` or `env-port`"));
            assert!(guide.contains("`role` selects `spa`, `admin`, or `astro`"));
        }
    }
}

#[test]
fn database_guidance_matches_enabled_migration_and_schema_commands() {
    use crate::context::RustMigrationLayout::{FlatMigrations, VersionedArtifacts};

    let template = live_template_source();
    let selected = BTreeSet::from([
        PathBuf::from("AGENTS.md"),
        PathBuf::from(".agent/jig-contract.json"),
    ]);
    for (sqlx, schema, layout) in [
        (false, false, FlatMigrations),
        (true, false, FlatMigrations),
        (true, true, FlatMigrations),
        (true, true, VersionedArtifacts),
    ] {
        let destination = tempfile::tempdir().unwrap();
        let answers = AnswerResolution::from_opts(
            &AnswerOpts {
                repo_name: Some("ExampleProject".into()),
                sqlx_enabled: Some(sqlx),
                schema_dump_enabled: Some(schema),
                rust_migration_dir: sqlx.then(|| "migrations".into()),
                rust_migration_layout: sqlx.then_some(layout),
                ..AnswerOpts::default()
            },
            destination.path(),
            false,
        )
        .unwrap()
        .into_parts()
        .0;
        render_template_files(
            &template,
            &answers,
            destination.path(),
            Some(&selected),
            Some(crate::context::CURRENT_CONTRACT_VERSION),
        )
        .unwrap();
        let guide = fs::read_to_string(destination.path().join("AGENTS.md")).unwrap();
        let contract: JsonValue = serde_json::from_slice(
            &fs::read(destination.path().join(".agent/jig-contract.json")).unwrap(),
        )
        .unwrap();
        let has_tool = |name: &str| {
            contract["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == name)
        };
        assert_eq!(
            has_tool("jig.migration_add"),
            sqlx && layout == FlatMigrations
        );
        assert_eq!(has_tool("jig.schema_dump"), schema);
        assert_eq!(
            guide.contains("`scripts/jig migration add NAME`"),
            has_tool("jig.migration_add")
        );
        assert_eq!(
            guide.contains("`scripts/jig sqlx schema dump`"),
            has_tool("jig.schema_dump")
        );
    }
}

#[test]
fn action_arguments_and_freshness_render_in_v8_and_preserve_file_budget_configuration() {
    let destination = tempfile::tempdir().unwrap();
    let answers = AnswerResolution::from_opts(
        &AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(true),
            rust_migration_dir: Some("migrations".into()),
            schema_dump_enabled: Some(false),
            ..AnswerOpts::default()
        },
        destination.path(),
        false,
    )
    .unwrap()
    .into_parts()
    .0;
    let template = live_template_source();
    let v7 = render_context(&template, &answers, Some(7)).unwrap();
    let v8 = render_context(&template, &answers, Some(8)).unwrap();
    let action = |context: &JsonValue, operation: &str| {
        context["repository"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|action| action["runner"]["operation"] == operation)
            .unwrap()
            .clone()
    };
    let migration = action(&v8, jig_contract::tool::MIGRATION_ADD);
    assert_eq!(
        migration["arguments"]["name"],
        json!({"type":"string", "required":true, "allow_empty":false, "max_bytes":200})
    );
    assert!(
        action(&v7, jig_contract::tool::MIGRATION_ADD)
            .get("arguments")
            .is_none()
    );
    let source: toml::Value = toml::from_str(v8["repository_toml"].as_str().unwrap()).unwrap();
    let authored = source["repository"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["target"]["action"].as_str() == Some("migration-add"))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&authored["arguments"]).unwrap(),
        migration["arguments"]
    );
    let v7_file_budget = action(&v7, jig_contract::tool::FILE_BUDGET);
    let v8_file_budget = action(&v8, jig_contract::tool::FILE_BUDGET);
    assert_eq!(v7_file_budget["runner"], v8_file_budget["runner"]);
    assert!(v7_file_budget.get("inputs_policy").is_none());
    assert!(v7_file_budget.get("source_state").is_none());
    assert_eq!(v8_file_budget["inputs_policy"], "whole_repository");
    assert_eq!(v8_file_budget["source_state"], "git");
}

#[test]
fn neutral_rust_workspace_guidance_survives_authored_recopy() {
    let template = live_template_source();
    let initial = tempfile::tempdir().unwrap();
    let selected = BTreeSet::from([PathBuf::from(".jig.toml"), PathBuf::from("AGENTS.md")]);
    render_template_files(
        &template,
        &rust_render_answers(RepositoryProjectionHint::RustWorkspace),
        initial.path(),
        Some(&selected),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
    )
    .unwrap();
    let initial_guide = fs::read_to_string(initial.path().join("AGENTS.md")).unwrap();

    for expected in [
        "ownership guidance in crate-level guides",
        "when the owning area is unclear",
        "## Rust Defaults",
        "For Rust changes",
        "## Crate Guide Conventions",
    ] {
        assert!(initial_guide.contains(expected), "missing {expected}");
    }
    for absent in [
        "Keep transport logic thin",
        "`scripts/jig dev`",
        "## Backend Defaults",
        "For backend changes",
        "## Backend Guide Conventions",
    ] {
        assert!(!initial_guide.contains(absent), "unexpected {absent}");
    }

    let reloaded = RenderAnswers::from_answers_file(&initial.path().join(".jig.toml")).unwrap();
    assert_eq!(
        reloaded.repository_projection_hint(),
        RepositoryProjectionHint::Backend
    );
    let recopy = tempfile::tempdir().unwrap();
    let guide_only = BTreeSet::from([PathBuf::from("AGENTS.md")]);
    render_template_files(
        &template,
        &reloaded,
        recopy.path(),
        Some(&guide_only),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(recopy.path().join("AGENTS.md")).unwrap(),
        initial_guide
    );
}

#[test]
fn rust_backend_guidance_keeps_ownership_and_verification_rules() {
    let destination = tempfile::tempdir().unwrap();
    render_template_files(
        &live_template_source(),
        &rust_render_answers(RepositoryProjectionHint::Backend),
        destination.path(),
        Some(&BTreeSet::from([PathBuf::from("AGENTS.md")])),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
    )
    .unwrap();
    let guide = fs::read_to_string(destination.path().join("AGENTS.md")).unwrap();

    for expected in [
        "ownership guidance in backend-level guides",
        "when the owning area is unclear",
        "## Backend Defaults",
        "Keep transport logic thin and business logic in the owning crate.",
        "For backend changes, evidence must cover the configured tests (`scripts/jig check test`).",
        "`scripts/jig dev`",
        "## Backend Guide Conventions",
    ] {
        assert!(guide.contains(expected), "missing {expected}");
    }
    assert!(!guide.contains("## Rust Defaults"));
    assert!(!guide.contains("## Crate Guide Conventions"));
}

#[test]
fn template_output_paths_reject_reserved_git_metadata_aliases() {
    for relative in [
        ".git/config.jinja",
        "vendor/.GiT/config.jinja",
        ".g\u{200c}it/config.jinja",
        "\u{feff}.G\u{202e}i\u{206a}T/config.jinja",
    ] {
        let error = output_relative_path(Path::new(relative))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("reserved Git metadata component"),
            "{relative}: {error}"
        );
        assert!(
            error.contains(relative.trim_end_matches(".jinja")),
            "{relative}: {error}"
        );
    }
}

#[test]
fn template_output_paths_allow_git_near_misses() {
    for relative in [
        ".github/workflows/check.yml.jinja",
        ".gitignore.jinja",
        ".gitkeep.jinja",
        "git/config.jinja",
        ".git .config.jinja",
        ".git\u{a0}.jinja",
        ".git\u{200b}.jinja",
        ".gi\u{200b}t.jinja",
        ".git\u{2029}.jinja",
        ".git\u{2060}.jinja",
        ".git\u{2069}.jinja",
    ] {
        output_relative_path(Path::new(relative)).unwrap();
    }
}

#[test]
fn legacy_go_postgres_render_preserves_a_custom_sqlc_command() {
    let template_root = tempfile::tempdir().unwrap();
    let project_templates = template_root.path().join("templates/project");
    fs::create_dir_all(&project_templates).unwrap();
    fs::write(
        project_templates.join(".jig.toml.jinja"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../templates/project/.jig.toml.jinja"
        )),
    )
    .unwrap();
    let answers_root = tempfile::tempdir().unwrap();
    let answers_path = answers_root.path().join("answers.toml");
    let custom_command = r#"go tool sqlc diff --file="custom sqlc.yaml""#;
    fs::write(
        &answers_path,
        format!(
            "repo_name = \"ExampleProject\"\nbackend_language = \"go\"\ngo_database = \"postgres\"\nsqlx_enabled = false\nschema_dump_enabled = false\nsqlc_check_command = {}\n",
            toml::Value::String(custom_command.into())
        ),
    )
    .unwrap();
    let answers = RenderAnswers::from_answers_file(&answers_path).unwrap();
    let template = PreparedTemplateSource::test_local(
        "fixture".into(),
        template_root.path().to_path_buf(),
        None,
        PrivateAnswerOverrides::default(),
    );
    let destination = answers_root.path().join("rendered");

    render_template_files(&template, &answers, &destination, None, Some(5)).unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    let config = toml::from_str::<toml::Value>(&rendered).unwrap();
    assert_eq!(
        config["commands"]["sqlc_check_command"].as_str(),
        Some(custom_command)
    );
}

#[test]
fn argv_epoch_renders_explicit_shell_and_preserves_authored_runner_choice() {
    let template = live_template_source();
    let answers = rust_render_answers(RepositoryProjectionHint::RustWorkspace);
    let old = render_context(&template, &answers, Some(7)).unwrap();
    let current = render_context(&template, &answers, Some(8)).unwrap();
    for action in current["repository"]["actions"].as_array().unwrap() {
        assert_ne!(action["runner"]["kind"], "command");
    }
    assert!(
        old["repository"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["runner"]["kind"] == "command")
    );
    assert!(
        current["repository"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["runner"]["kind"] == "shell")
    );
    let initial = tempfile::tempdir().unwrap();
    let selected = BTreeSet::from([
        PathBuf::from(".jig.toml"),
        PathBuf::from(".agent/jig-contract.json"),
    ]);
    render_template_files(
        &template,
        &answers,
        initial.path(),
        Some(&selected),
        Some(8),
    )
    .unwrap();
    let path = initial.path().join(".jig.toml");
    let mut source: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let actions = source["repository"]["actions"].as_array_mut().unwrap();
    let action = actions
        .iter_mut()
        .find(|a| a["runner"]["kind"].as_str() == Some("shell"))
        .unwrap();
    action["runner"] = toml::Value::try_from(
        json!({"kind":"argv", "program":"literal program", "args":["literal *", ""]}),
    )
    .unwrap();
    let expected = source["repository"]["actions"].clone();
    fs::write(&path, toml::to_string(&source).unwrap()).unwrap();
    let authored = RenderAnswers::from_answers_file(&path).unwrap();
    assert!(render_context(&template, &authored, Some(7)).is_err());
    let recopy = tempfile::tempdir().unwrap();
    render_template_files(
        &template,
        &authored,
        recopy.path(),
        Some(&selected),
        Some(8),
    )
    .unwrap();
    let copied: toml::Value =
        toml::from_str(&fs::read_to_string(recopy.path().join(".jig.toml")).unwrap()).unwrap();
    assert_eq!(copied["repository"]["actions"], expected);
    let manifest: JsonValue = serde_json::from_str(
        &fs::read_to_string(recopy.path().join(".agent/jig-contract.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["contract_version"], 8);
    assert_eq!(manifest["actions"], serde_json::to_value(expected).unwrap());
}
