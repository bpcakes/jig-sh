use super::*;

#[test]
fn init_round_trips_status_provider_configuration() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let answers_path = temp.path().join("answers.toml");
    fs::write(
        &answers_path,
        r#"repo_name = "demo"
default_branch = "main"
sqlx_enabled = false
schema_dump_enabled = false

[[status.providers]]
id = "factorish.rewrite"
argv = ["ruby", "scripts/status.rb", "--jig-v1"]
timeout_seconds = 45
"#,
    )
    .unwrap();

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
            answers_file: Some(answers_path),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("[[status.providers]]"));
    assert!(rendered.contains("id = \"factorish.rewrite\""));
    assert!(rendered.contains("argv = [\"ruby\", \"scripts/status.rb\", \"--jig-v1\"]"));
    assert!(rendered.contains("timeout_seconds = 45"));

    run_update(UpdateOpts {
        path: destination.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        recopy: true,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let recopied = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(recopied.contains("[[status.providers]]"));
    assert!(recopied.contains("id = \"factorish.rewrite\""));
    assert!(recopied.contains("argv = [\"ruby\", \"scripts/status.rb\", \"--jig-v1\"]"));
    assert!(recopied.contains("timeout_seconds = 45"));
}
