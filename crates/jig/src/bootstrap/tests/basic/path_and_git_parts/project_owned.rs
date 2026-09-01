#[test]
fn adopt_keeps_project_owned_build_and_lint_configuration() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    let project_owned_files = [
        (
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = []\n",
        ),
        (
            "clippy.toml",
            "# Project-owned Clippy policy\ncognitive-complexity-threshold = 40\n",
        ),
        ("Makefile", "project-owned:\n\t@true\n"),
    ];
    for (relative, contents) in project_owned_files {
        fs::write(repo.join(relative), contents).unwrap();
    }

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

    for (relative, contents) in project_owned_files {
        assert_eq!(fs::read_to_string(repo.join(relative)).unwrap(), contents);
    }
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(!answers.contains("makefile_enabled"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(&format!(
        r#""contract_version": {}"#,
        crate::context::CURRENT_CONTRACT_VERSION
    )));
    assert!(!contract.contains("jig_version"));
    assert!(contract.contains(r#""kind": "command""#));
    assert!(!contract.contains("jig.run_target"));
}
