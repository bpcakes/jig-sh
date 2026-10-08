//! The CLI records its build script's official-template pin before any
//! command runs, so an unreleased build renders `jig init` from the templates
//! embedded in the binary instead of the official release tag.

use std::fs;
use std::process::Command;

#[test]
fn unreleased_build_initializes_from_embedded_templates() {
    if option_env!("JIG_BUILD_OFFICIAL_TEMPLATE_PIN") != Some("unreleased") {
        // Released and packaged builds resolve the official remote template,
        // which this offline test does not exercise.
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("ExampleProject");
    let output = Command::new(env!("CARGO_BIN_EXE_jig"))
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .arg("init")
        .arg(&destination)
        .args(["--preset", "harness-only", "--no-input", "--no-vault"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(
        answers
            .lines()
            .any(|line| line == r#"_src_path = "embedded:jig-sh""#),
        "{answers}"
    );
}
