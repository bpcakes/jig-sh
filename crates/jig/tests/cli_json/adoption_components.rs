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
