use super::*;
use std::process::Output;

#[cfg(unix)]
pub(super) fn prepare_current_template(template: &Path, workspace: &Path) {
    let clone = Command::new("git")
        .args(["clone", "--quiet", "--local", "--no-hardlinks"])
        .arg(workspace)
        .arg(template)
        .status()
        .unwrap();
    assert!(clone.success());
    let patch = Command::new("git")
        .current_dir(workspace)
        .args(["diff", "--binary", "HEAD", "--", "templates"])
        .output()
        .unwrap();
    assert!(patch.status.success());
    if patch.stdout.is_empty() {
        return;
    }
    let mut apply = Command::new("git")
        .current_dir(template)
        .args(["apply", "--binary", "-"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    apply
        .stdin
        .take()
        .unwrap()
        .write_all(&patch.stdout)
        .unwrap();
    assert!(apply.wait().unwrap().success());
    for args in [
        &["config", "user.email", "reviewer@example.invalid"][..],
        &["config", "user.name", "ExampleReviewer"],
        &["add", "templates"],
        &[
            "commit",
            "--quiet",
            "-m",
            "Synthetic current template snapshot",
        ],
    ] {
        assert!(
            Command::new("git")
                .current_dir(template)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[cfg(unix)]
pub(super) fn template_commit(template: &Path) -> String {
    let output = Command::new("git")
        .current_dir(template)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[cfg(unix)]
pub(super) fn remediation_commands(
    inventory: &Output,
    commit: &str,
    portable_source: &str,
) -> (String, String) {
    assert!(inventory.status.success());
    let inventory: Value = serde_json::from_slice(&inventory.stdout).unwrap();
    let next_step = command_by_name(&inventory, "sqlx")["next_step"]
        .as_str()
        .unwrap();
    let mut commands = next_step.split('`');
    let preview = commands.nth(1).unwrap().to_owned();
    let apply = commands.nth(1).unwrap().to_owned();
    assert!(preview.contains("--minimal"));
    assert!(preview.contains("--template"));
    assert!(preview.contains("--template-mode committed"));
    assert!(preview.contains(&format!("--vcs-ref {commit}")));
    assert!(preview.contains(&format!("--template-source-url {portable_source}")));
    assert!(apply.contains("--force --write"));
    (preview, apply)
}

pub(super) fn assert_bootstrap_report(output: &Output, mode: &str, template: &Path) {
    assert!(
        output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["render_mode"], mode);
    assert_eq!(payload["harness_footprint"], "minimal");
    assert_eq!(payload["template"], template.display().to_string());
}

pub(super) fn assert_minimal_template_identity(
    config: &str,
    portable_source: &str,
    template: &Path,
) {
    assert!(config.contains("harness_footprint = \"minimal\""));
    assert!(config.contains(&format!("_src_path = {portable_source:?}")));
    assert!(config.contains(&format!(
        "_template_local_path = {:?}",
        template.display().to_string()
    )));
}

pub(super) fn assert_rust_library_json(output: &Output) {
    assert!(
        output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["scaffold"]["preset"], "rust-library");
    assert_eq!(report["scaffold"]["db"], "none");
    assert_eq!(report["scaffold"]["frontends"], json!([]));
    assert_eq!(
        report["scaffold"]["files_created"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert!(
        report["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step.as_str() != Some("scripts/jig dev"))
    );
    let notes = report["notes"].as_array().unwrap();
    assert!(notes.iter().any(|note| {
        note.as_str()
            .is_some_and(|note| note.contains("Scaffolded project code is project-owned"))
    }));
    assert!(notes.iter().all(|note| {
        !note
            .as_str()
            .is_some_and(|note| note.contains("Scaffolded application code"))
    }));
}

pub(super) fn assert_rust_library_dev_defaults(destination: &Path) {
    let config = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    let config = toml::from_str::<toml::Value>(&config).unwrap();
    assert_eq!(config["dev"]["proxy_port"].as_integer(), Some(1355));
    assert_eq!(config["dev"]["https_port"].as_integer(), Some(1443));
    assert_eq!(config["dev"]["https"].as_bool(), Some(false));
    assert_eq!(config["dev"]["http2"].as_bool(), Some(true));
    assert_eq!(config["dev"]["lan"].as_bool(), Some(false));
    assert_eq!(config["dev"]["tld"].as_str(), Some("localhost"));
    assert_eq!(config["dev"]["workspace_discovery"].as_bool(), Some(false));
}

pub(super) fn assert_rust_library_checks(destination: &Path) {
    for check in ["contract", "agent-map", "agent-guides"] {
        let output = jig()
            .current_dir(destination)
            .args(["check", check])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "jig check {check} failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub(super) fn assert_rust_library_human(output: &Output) {
    assert!(
        output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let human = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(human.contains("scaffold: rust-library for examplelibraryhuman (db: none)"));
    assert!(human.contains("scaffold files: 6 created, 0 modified, 0 unchanged"));
    assert!(human.contains("Scaffolded project code is project-owned"));
    assert!(!human.contains("Scaffolded application code"));
    assert!(!human.contains("frontends:"));
    assert!(!human.contains("scripts/jig dev"));
}
