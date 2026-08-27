#[test]
fn schema_dump_is_an_explicit_utility_and_only_schema_check_is_a_work_gate() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_worktree();

    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(true),
            rust_migration_dir: Some("migrations".into()),
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            schema_dump_enabled: Some(true),
            schema_dump_command: Some(
                "cargo run --locked --package schema-tool --bin dump-schema".into(),
            ),
            schema_docs_dir: Some("artifacts/schema".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(config.contains("id = \"schema\""), "{config}");
    assert!(!config.contains("id = \"schema-dump\""), "{config}");
    assert!(
        config.contains("schema_docs_dir = \"artifacts/schema\""),
        "{config}"
    );
    assert!(config.contains("\"artifacts/schema/**\""), "{config}");
    assert!(!config.contains("\"docs/schema/**\""), "{config}");
    assert!(config.contains("\"crates/**\""), "{config}");
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains("jig.schema_check"));
    assert!(contract.contains("jig.schema_dump"));
}
