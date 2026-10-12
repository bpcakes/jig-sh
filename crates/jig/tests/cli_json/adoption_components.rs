use super::*;

#[test]
fn adopt_components_cli_human_and_json_preview_agree() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("ExampleProject");
    fs::create_dir_all(root.join("fixtures/sample")).unwrap();
    fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    fs::write(
        root.join("fixtures/sample/Cargo.toml"),
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let args = [
        "adopt",
        ".",
        "--defaults",
        "--no-input",
        "--no-vault",
        "--include-component",
        "./fixtures/sample/",
    ];
    let json_output = jig()
        .current_dir(&root)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        json_output.status.success(),
        "{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let report: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    let human_output = jig().current_dir(&root).args(args).output().unwrap();
    assert!(
        human_output.status.success(),
        "{}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let human = String::from_utf8(human_output.stdout).unwrap();
    for item in report["adoption_review"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|item| item.starts_with("component "))
    {
        assert!(human.contains(item), "missing {item}: {human}");
    }
    let candidates = report["detection_report"]["component_candidates"]
        .as_array()
        .unwrap();
    assert!(
        candidates
            .iter()
            .any(|c| c["root"] == "fixtures/sample" && c["disposition"] == "included")
    );
    assert!(!root.join(".jig.toml").exists());
    for invalid in ["../escape", "unknown", "fixtures/*"] {
        let result = jig()
            .current_dir(&root)
            .args([
                "adopt",
                ".",
                "--defaults",
                "--no-input",
                "--no-vault",
                "--write",
                "--exclude-component",
                invalid,
                "--json",
            ])
            .output()
            .unwrap();
        assert!(!result.status.success(), "accepted {invalid}");
        assert!(!root.join(".jig.toml").exists());
    }
}

#[test]
fn adopt_write_retains_environment_authorized_vault_setup() {
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
    let root = temp.path().join("ExampleProject");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("README.md"), "# ExampleProject\n").unwrap();
    for args in [
        &["init", "--quiet"][..],
        &["config", "user.email", "fixture@example.com"],
        &["config", "user.name", "Fixture"],
        &["add", "."],
        &["commit", "--quiet", "-m", "fixture"],
    ] {
        assert!(
            Command::new("git")
                .current_dir(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    let output = jig()
        .current_dir(&root)
        .env("JIG_VAULT_HOME", temp.path().join("vault-home"))
        .env("JIG_VAULT_PASSPHRASE", "correct horse battery staple")
        .env_remove("JIG_VAULT_NEW_PASSPHRASE")
        .args([
            "--json",
            "adopt",
            ".",
            "--template",
            template.to_str().unwrap(),
            "--template-mode",
            "committed",
            "--defaults",
            "--no-input",
            "--write",
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
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["vault"]["requested"], true);
    assert_eq!(report["vault"]["initialized"], true);
    assert_eq!(report["vault"]["created"], true);
    assert_eq!(report["vault"]["vault_scope"], "repo");
}
