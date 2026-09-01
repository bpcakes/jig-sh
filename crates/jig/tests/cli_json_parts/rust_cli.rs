#[test]
fn rust_cli_init_has_exact_json_and_human_process_summaries() {
    let template_parent = tempdir().unwrap();
    let template = template_parent.path().join("ExampleProject-template");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let clone = Command::new("git")
        .args(["clone", "--quiet", "--local", "--no-hardlinks"])
        .arg(&workspace)
        .arg(&template)
        .status()
        .unwrap();
    assert!(clone.success());

    let destinations = tempdir().unwrap();
    let json_destination = destinations.path().join("ExampleCliJson");
    let json_output = jig()
        .args([
            "--json",
            "init",
            json_destination.to_str().unwrap(),
            "--preset",
            "rust-cli",
            "--template",
            template.to_str().unwrap(),
            "--template-mode",
            "committed",
            "--no-input",
            "--no-vault",
        ])
        .output()
        .unwrap();
    assert!(
        json_output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        json_output.status,
        String::from_utf8_lossy(&json_output.stdout),
        String::from_utf8_lossy(&json_output.stderr)
    );
    assert!(json_output.stderr.is_empty());
    let json_report: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    assert_eq!(json_report["ok"], true);
    assert_eq!(json_report["scaffold"]["preset"], "rust-cli");
    assert_eq!(json_report["scaffold"]["db"], "none");
    assert_eq!(json_report["scaffold"]["frontends"], json!([]));
    assert_eq!(
        json_report["scaffold"]["files_created"],
        json!([
            "Cargo.toml",
            "README.md",
            "clippy.toml",
            "crates/exampleclijson/Cargo.toml",
            "crates/exampleclijson/AGENTS.md",
            "crates/exampleclijson/src/main.rs"
        ])
    );
    assert!(
        json_report["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step.as_str() != Some("scripts/jig dev"))
    );
    let notes = json_report["notes"].as_array().unwrap();
    assert!(notes.iter().any(|note| {
        note.as_str()
            .is_some_and(|note| note.contains("Scaffolded project code is project-owned"))
    }));
    assert!(notes.iter().all(|note| {
        !note
            .as_str()
            .is_some_and(|note| note.contains("Scaffolded application code"))
    }));

    let human_destination = destinations.path().join("ExampleCliHuman");
    let human_output = jig()
        .args([
            "init",
            human_destination.to_str().unwrap(),
            "--preset",
            "rust-cli",
            "--template",
            template.to_str().unwrap(),
            "--template-mode",
            "committed",
            "--defaults",
            "--no-vault",
        ])
        .output()
        .unwrap();
    assert!(
        human_output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        human_output.status,
        String::from_utf8_lossy(&human_output.stdout),
        String::from_utf8_lossy(&human_output.stderr)
    );
    let human = String::from_utf8(human_output.stdout).unwrap();
    assert!(human.contains("scaffold: rust-cli for exampleclihuman (db: none)"));
    assert!(human.contains("scaffold files: 6 created, 0 modified, 0 unchanged"));
    assert!(human.contains("Scaffolded project code is project-owned"));
    assert!(!human.contains("Scaffolded application code"));
    assert!(!human.contains("frontends:"));
    assert!(!human.contains("scripts/jig dev"));
}

#[test]
fn rust_cli_init_retains_environment_authorized_vault_setup() {
    let template_parent = tempdir().unwrap();
    let template = template_parent.path().join("ExampleProject-template");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let clone = Command::new("git")
        .args(["clone", "--quiet", "--local", "--no-hardlinks"])
        .arg(&workspace)
        .arg(&template)
        .status()
        .unwrap();
    assert!(clone.success());
    let temp = tempdir().unwrap();
    let destination = temp.path().join("ExampleCliVault");

    let output = jig()
        .env("JIG_VAULT_HOME", temp.path().join("vault-home"))
        .env("JIG_VAULT_PASSPHRASE", "correct horse battery staple")
        .args([
            "--json",
            "init",
            destination.to_str().unwrap(),
            "--preset",
            "rust-cli",
            "--template",
            template.to_str().unwrap(),
            "--template-mode",
            "committed",
            "--no-input",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["scaffold"]["preset"], "rust-cli");
    assert_eq!(report["vault"]["requested"], true);
    assert_eq!(report["vault"]["initialized"], true);
    assert_eq!(report["vault"]["created"], true);
    assert_eq!(report["vault"]["vault_scope"], "repo");
}

#[test]
fn forbidden_rust_cli_answers_fail_before_template_vault_and_publication() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(&answers, "unexpected_shape_authority = true\n").unwrap();
    let destination = temp.path().join("ExampleCli");

    let output = jig()
        .env_remove("JIG_VAULT_PASSPHRASE")
        .args([
            "--json",
            "init",
            destination.to_str().unwrap(),
            "--preset",
            "rust-cli",
            "--answers-file",
            answers.to_str().unwrap(),
            "--template",
            "/missing/ExampleProject-template",
            "--no-input",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    let message = error["error"]["message"].as_str().unwrap();
    assert!(message.contains("rust-cli"), "{message}");
    assert!(message.contains("unexpected_shape_authority"), "{message}");
    assert!(!message.contains("JIG_VAULT_PASSPHRASE"), "{message}");
    assert!(
        !message.contains("Failed to inspect template source"),
        "{message}"
    );
    assert!(!destination.exists());
}
