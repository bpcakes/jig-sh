use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::tempdir;

#[test]
fn tracked_sources_match_the_supported_host_policy() {
    let output = Command::new(repo_root().join("scripts/check-supported-host-surface.sh"))
        .current_dir(repo_root())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn supported_host_policy_fails_when_content_inventory_fails() {
    let output = run_with_fake_git("exit 86\n");

    assert_eq!(output.status.code(), Some(86));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Failed to inspect tracked source for unsupported-host content")
    );
}

#[test]
fn supported_host_policy_fails_when_path_inventory_fails() {
    let output = run_with_fake_git(
        r#"case "$1" in
  grep) exit 1 ;;
  ls-files) exit 87 ;;
  *) exit 88 ;;
esac
"#,
    );

    assert_eq!(output.status.code(), Some(87));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Failed to inventory tracked paths for unsupported-host artifacts")
    );
}

#[test]
fn supported_host_policy_does_not_treat_window_as_windows() {
    let output = run_with_fake_git(
        r#"case "$1" in
  grep) exit 1 ;;
  ls-files) printf '%s\n' 'crates/example/src/window.rs' ;;
  *) exit 88 ;;
esac
"#,
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_with_fake_git(body: &str) -> Output {
    let temp = tempdir().unwrap();
    let git = temp.path().join("git");
    fs::write(&git, format!("#!/bin/sh\n{body}")).unwrap();
    let mut permissions = fs::metadata(&git).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&git, permissions).unwrap();

    Command::new(repo_root().join("scripts/check-supported-host-surface.sh"))
        .current_dir(repo_root())
        .env("PATH", path_with_prefix(temp.path()))
        .output()
        .unwrap()
}

fn path_with_prefix(prefix: &Path) -> OsString {
    let mut paths = vec![prefix.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
