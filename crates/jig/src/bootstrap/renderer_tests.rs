use crate::backend::BackendLanguage;
use crate::bootstrap::AnswerOpts;
use crate::bootstrap::answers::AnswerResolution;
use crate::bootstrap::repository_model::RepositoryProjectionHint;
use crate::bootstrap::template_source::PrivateAnswerOverrides;

use super::*;

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
        "before Rust work",
        "## Rust Defaults",
        "For Rust changes",
        "## Crate Guide Conventions",
    ] {
        assert!(initial_guide.contains(expected), "missing {expected}");
    }
    for absent in [
        "Keep transport logic thin",
        "- `scripts/jig dev`",
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
fn existing_rust_backend_guidance_branch_remains_unchanged() {
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
        "before backend work",
        "## Backend Defaults",
        "Keep transport logic thin and business logic in the owning crate.",
        "- `scripts/jig dev`",
        "For backend changes",
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
