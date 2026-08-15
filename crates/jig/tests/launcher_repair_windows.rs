#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::tempdir;

fn assert_success(output: &Output, action: &str) {
    assert!(
        output.status.success(),
        "{action} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn git_bash() -> PathBuf {
    let mut candidates = Vec::new();
    for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(key) {
            candidates.push(PathBuf::from(&root).join("Git/bin/bash.exe"));
            candidates.push(PathBuf::from(root).join("Git/usr/bin/bash.exe"));
        }
    }
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(root).join("Programs/Git/bin/bash.exe"));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .expect("Git Bash should be installed for Windows launcher tests")
}

fn initialize_embedded_repo(jig: &Path, destination: &Path, repo_name: &str) {
    let init = Command::new(jig)
        .arg("init")
        .arg(destination)
        .args([
            "--preset",
            "harness-only",
            "--repo-name",
            repo_name,
            "--sqlx-enabled",
            "false",
            "--no-input",
            "--no-vault",
        ])
        .output()
        .unwrap();
    assert_success(&init, "fixture init");
}

#[test]
fn git_bash_launcher_only_repair_seeds_a_runnable_cache() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repair");
    let jig = Path::new(env!("CARGO_BIN_EXE_jig"));
    initialize_embedded_repo(jig, &destination, "windows-repair");

    let manifest_path = destination.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = 3.into();
    manifest["jig_version"] = "0.2.0-beta.1".into();
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap() + "\n",
    )
    .unwrap();

    let answers_path = destination.join(".jig.toml");
    let answers = fs::read_to_string(&answers_path).unwrap().replacen(
        "template_source_url =",
        "jig_version = \"0.2.0-beta.1\"\ntemplate_source_url =",
        1,
    );
    fs::write(answers_path, answers).unwrap();

    let competing_bin = temp.path().join("competing-bin");
    fs::create_dir(&competing_bin).unwrap();
    fs::copy(jig, competing_bin.join("bash.exe")).unwrap();
    let path = std::env::join_paths(std::iter::once(competing_bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();

    let repair = Command::new(jig)
        .arg("update")
        .arg(&destination)
        .args(["--launcher-only", "--force"])
        .env("PATH", path)
        .output()
        .unwrap();
    assert_success(&repair, "Windows launcher-only repair");

    let version = Command::new(git_bash())
        .arg(destination.join("scripts/jig"))
        .arg("--version")
        .env_remove("JIG_DEV_BIN")
        .current_dir(&destination)
        .output()
        .unwrap();
    assert_success(&version, "repaired Git Bash launcher");
}

#[test]
fn git_bash_path_reuse_accepts_a_compatible_pe_jig_binary() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("path-reuse");
    let jig = Path::new(env!("CARGO_BIN_EXE_jig"));
    initialize_embedded_repo(jig, &destination, "windows-path-reuse");

    let path = std::env::join_paths(std::iter::once(jig.parent().unwrap().to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let version = Command::new(git_bash())
        .arg(destination.join("scripts/jig"))
        .arg("--version")
        .env("PATH", path)
        .env("JIG_INSTALL_ALLOW_PATH_BINARY", "1")
        .env_remove("JIG_DEV_BIN")
        .current_dir(&destination)
        .output()
        .unwrap();

    assert_success(&version, "Git Bash PE PATH runtime reuse");
    assert!(
        String::from_utf8_lossy(&version.stderr)
            .contains("Using explicitly allowed PATH Jig binary"),
        "PATH reuse did not report the selected binary: {}",
        String::from_utf8_lossy(&version.stderr)
    );
}
