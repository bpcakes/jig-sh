use crate::bootstrap::template_source::PrivateAnswerOverrides;

use super::*;

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
