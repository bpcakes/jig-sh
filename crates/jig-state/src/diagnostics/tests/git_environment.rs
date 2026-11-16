use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use super::{fixture_context, git};
use crate::state_diagnose;

#[test]
fn diagnostics_use_selected_git_and_ignore_ambient_repository() {
    const CHILD: &str = "JIG_TEST_DIAGNOSTIC_ROOT";
    if let Some(root) = std::env::var_os(CHILD) {
        let ctx = fixture_context(std::path::Path::new(&root));
        let output = state_diagnose(&ctx);
        assert_eq!(output["git"]["repository"], true, "{output}");
        assert_eq!(output["git"]["paths"]["runs"]["tracked"], false, "{output}");
        assert_eq!(output["git"]["paths"]["runs"]["ignored"], true, "{output}");
        assert_eq!(
            output["git"]["paths"]["runs"]["merge_attribute"], "union",
            "{output}"
        );
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ExampleProject");
    let foreign = temp.path().join("ExampleOtherProject");
    for path in [&root, &foreign] {
        fs::create_dir_all(path.join(".agent/state")).unwrap();
        git(path, &["init", "-q"]);
    }
    fs::write(root.join(".gitignore"), ".agent/state/runs.jsonl\n").unwrap();
    fs::write(
        root.join(".gitattributes"),
        ".agent/state/*.jsonl merge=union\n",
    )
    .unwrap();
    fs::write(foreign.join(".agent/state/runs.jsonl"), "").unwrap();
    git(&foreign, &["add", ".agent/state/runs.jsonl"]);
    let wrapper = temp.path().join("selected-git");
    let log = temp.path().join("git-invocations");
    fs::write(
        &wrapper,
        "#!/bin/sh\nprintf '%s\n' invoked >> \"$JIG_TEST_GIT_LOG\"\nexec git \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "diagnostics::tests::git_environment::diagnostics_use_selected_git_and_ignore_ambient_repository", "--nocapture"])
        .env(CHILD, &root)
        .env("JIG_GIT_BIN", &wrapper)
        .env("JIG_TEST_GIT_LOG", &log)
        .env("GIT_DIR", foreign.join(".git"))
        .env("GIT_INDEX_FILE", foreign.join(".git/index"))
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fs::read_to_string(log).unwrap().is_empty());
}
