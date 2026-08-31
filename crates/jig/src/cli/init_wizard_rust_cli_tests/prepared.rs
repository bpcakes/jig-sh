use super::*;

#[test]
fn rust_cli_renders_from_one_frozen_answer_input_after_the_file_changes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(
        &answers,
        r#"repo_name = "FrozenCli"
jig_version = "legacy-input"

[dev]
proxy_port = 2455
https_port = 2443
https = true
http2 = false
lan = true
tld = "test"
workspace_discovery = true
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");
    let template = temp.path().join("template");
    copy_tree(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("templates/project"),
        &template.join("templates/project"),
    );
    let mut opts = init_opts(&[
        "jig",
        "init",
        destination.to_str().unwrap(),
        "--preset",
        "rust-cli",
        "--answers-file",
        answers.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);
    opts.template = Some(template.display().to_string());
    let reads = Cell::new(0);
    let mut prepared = bootstrap::PreparedInitAnswers::from_opts_at_with_reader(
        &opts.answers,
        temp.path(),
        |path| {
            reads.set(reads.get() + 1);
            fs::read_to_string(path)
        },
    )
    .unwrap();
    prepared.move_effective_to(&mut opts.answers).unwrap();
    prepare_merged_init_interaction(
        &mut opts,
        &mut prepared,
        InitInteractionPolicy::Strict {
            non_terminal: false,
        },
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(reads.get(), 1);
    fs::write(
        &answers,
        "repo_name = \"ChangedCli\"\nunexpected_shape_authority = true\n",
    )
    .unwrap();

    bootstrap::run_prepared_init(opts, prepared).unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("repo_name = \"frozencli\""));
    assert!(!rendered.contains("ChangedCli"));
    assert!(!rendered.contains("jig_version"));
    let rendered = toml::from_str::<toml::Value>(&rendered).unwrap();
    assert_eq!(rendered["dev"]["proxy_port"].as_integer(), Some(2455));
    assert_eq!(rendered["dev"]["https_port"].as_integer(), Some(2443));
    assert_eq!(rendered["dev"]["https"].as_bool(), Some(true));
    assert_eq!(rendered["dev"]["http2"].as_bool(), Some(false));
    assert_eq!(rendered["dev"]["lan"].as_bool(), Some(true));
    assert_eq!(rendered["dev"]["tld"].as_str(), Some("test"));
    assert_eq!(rendered["dev"]["workspace_discovery"].as_bool(), Some(true));
    assert_eq!(reads.get(), 1);
    assert!(destination.join("crates/frozencli/src/main.rs").is_file());
}

#[test]
fn bootstrap_guard_rejects_rust_cli_before_template_or_publication() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(&answers, "unexpected_shape_authority = true\n").unwrap();
    let destination = temp.path().join("ExampleCli");
    let mut opts = init_opts(&[
        "jig",
        "init",
        destination.to_str().unwrap(),
        "--preset",
        "rust-cli",
        "--answers-file",
        answers.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);
    opts.template = Some("/missing/ExampleProject-template".into());

    let error = bootstrap::run_init(opts).unwrap_err().to_string();

    assert!(error.contains("rust-cli"), "{error}");
    assert!(error.contains("unexpected_shape_authority"), "{error}");
    assert!(
        !error.contains("Failed to inspect template source"),
        "{error}"
    );
    assert!(!destination.exists());
}
