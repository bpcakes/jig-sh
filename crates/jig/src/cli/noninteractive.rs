use super::*;

#[test]
fn no_input_accepts_answers_file_frontends_as_an_explicit_decision() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"[[frontend_apps]]
name = "portal"
dir = "portal"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--answers-file",
        answers_file.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);

    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();

    assert_eq!(opts.answers.frontend_apps[0].name, "portal");
    assert!(!opts.scaffold.has_frontends());
}

#[test]
fn harness_only_is_explicit_and_rejects_scaffold_options() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "harness-only",
        "--no-input",
        "--no-vault",
    ]);
    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(opts.answers.sqlx_enabled, Some(false));

    let mut invalid = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "harness-only",
        "--db",
        "none",
        "--no-input",
        "--no-vault",
    ]);
    let error = prepare_init_interaction_with_io(
        &mut invalid,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("harness-only cannot be combined with --db"));
}

#[test]
fn empty_migration_dir_is_omitted_consistently_in_noninteractive_harness_init() {
    for mode in ["--defaults", "--no-input"] {
        let mut opts = init_opts(&[
            "jig",
            "init",
            "demo",
            "--preset",
            "harness-only",
            "--rust-migration-dir",
            "",
            mode,
            "--no-vault",
        ]);

        prepare_init_interaction_with_io(
            &mut opts,
            &mut Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
        )
        .unwrap();

        assert_eq!(opts.answers.sqlx_enabled, Some(false), "mode {mode}");
        assert_eq!(opts.answers.rust_migration_dir, None, "mode {mode}");
    }

    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        "repo_name = \"demo\"\nrust_migration_dir = \"\"\n",
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "harness-only",
        "--answers-file",
        answers_file.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);

    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
    assert_eq!(opts.answers.rust_migration_dir, None);
}

#[test]
fn implicit_non_terminal_mode_is_strict_but_accepts_resolved_input() {
    let mut unresolved = init_opts(&["jig", "init", "demo", "--no-vault"]);
    let error = prepare_init_interaction_with_terminal(
        &mut unresolved,
        false,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("stdin or stderr is not a terminal"));
    assert!(error.contains("--preset harness-only"));

    let mut resolved = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--frontends",
        "web",
        "--no-vault",
    ]);
    prepare_init_interaction_with_terminal(
        &mut resolved,
        false,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
}

#[test]
fn minimal_answers_resolve_to_harness_only_in_every_interaction_mode() {
    let temp = tempdir().unwrap();
    let minimal = temp.path().join("minimal.toml");
    fs::write(
        &minimal,
        "repo_name = \"demo\"\nharness_footprint = \"minimal\"\n",
    )
    .unwrap();

    for flags in [
        vec![],
        vec!["--defaults"],
        vec!["--no-input"],
        vec!["--preset", "harness-only", "--no-input"],
    ] {
        let mut args = vec![
            "jig",
            "init",
            "demo",
            "--answers-file",
            minimal.to_str().unwrap(),
            "--no-vault",
        ];
        args.extend(flags);
        let mut opts = init_opts(&args);
        let mut output = Vec::new();

        prepare_init_interaction_with_io(
            &mut opts,
            &mut Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .unwrap();

        assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::HarnessOnly));
        assert_eq!(opts.answers.sqlx_enabled, Some(false));
        assert!(output.is_empty());
    }

    let mut non_terminal = init_opts(&[
        "jig",
        "init",
        "demo",
        "--answers-file",
        minimal.to_str().unwrap(),
        "--no-vault",
    ]);
    prepare_init_interaction_with_terminal(
        &mut non_terminal,
        false,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(
        non_terminal.scaffold.preset,
        Some(ScaffoldPreset::HarnessOnly)
    );
}

#[test]
fn malformed_answers_and_minimal_rust_react_conflicts_fail_during_preflight() {
    let temp = tempdir().unwrap();
    let malformed = temp.path().join("malformed.toml");
    fs::write(&malformed, "repo_name = [\n").unwrap();
    let destination = temp.path().join("destination");
    let mut malformed_opts = init_opts(&[
        "jig",
        "init",
        destination.to_str().unwrap(),
        "--answers-file",
        malformed.to_str().unwrap(),
        "--defaults",
        "--no-vault",
    ]);
    let error = prepare_init_interaction_with_io(
        &mut malformed_opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Failed to parse"));
    assert!(!destination.exists());

    let minimal = temp.path().join("minimal.toml");
    fs::write(&minimal, "harness_footprint = \"minimal\"\n").unwrap();
    let mut conflict = init_opts(&[
        "jig",
        "init",
        destination.to_str().unwrap(),
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--frontends",
        "web",
        "--answers-file",
        minimal.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);
    let error = prepare_init_interaction_with_io(
        &mut conflict,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("cannot combine harness_footprint = \"minimal\""));
    assert!(error.contains("Rust React scaffold"));
    assert!(!destination.exists());
}
