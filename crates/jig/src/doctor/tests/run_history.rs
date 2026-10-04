use super::*;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn doctor_guides_existing_journals_to_local_history_without_mutation() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-q"]);
    fs::create_dir_all(root.join(".agent/state")).unwrap();
    let journal = root.join(".agent/state/runs.jsonl");
    fs::write(&journal, "example local history\n").unwrap();
    git(root, &["add", ".agent/state/runs.jsonl"]);
    let index = fs::read(root.join(".git/index")).unwrap();

    let result = super::super::run_history::check_local_history(root, None).unwrap();
    assert_eq!(result.status, "tracked");
    assert!(!result.required);
    assert_eq!(result.data["ignored"], false);
    assert!(result.fix.as_deref().unwrap().contains(".gitignore"));
    assert!(
        result
            .fix
            .as_deref()
            .unwrap()
            .contains("git rm --cached -- .agent/state/runs.jsonl")
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(&journal).unwrap(),
        "example local history\n"
    );

    fs::write(root.join(".gitignore"), ".agent/state/runs.jsonl\n").unwrap();
    let result = super::super::run_history::check_local_history(root, None).unwrap();
    assert_eq!(result.status, "tracked");
    assert_eq!(result.data["ignored"], true);
    git(root, &["rm", "--cached", "--", ".agent/state/runs.jsonl"]);
    let result = super::super::run_history::check_local_history(root, None).unwrap();
    assert!(result.ok);
    assert_eq!(result.status, "local");
    assert_eq!(
        fs::read_to_string(&journal).unwrap(),
        "example local history\n"
    );

    fs::remove_file(root.join(".gitignore")).unwrap();
    let result = super::super::run_history::check_local_history(root, None).unwrap();
    assert_eq!(result.status, "not ignored");
    assert!(!result.fix.unwrap().contains("git rm"));
}

#[test]
fn doctor_history_probe_skips_non_git_roots_and_reports_git_errors() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    assert!(super::super::run_history::check_local_history(temp.path(), None).is_none());
    fs::write(temp.path().join(".git"), "gitdir: missing-git-directory\n").unwrap();
    let result = super::super::run_history::check_local_history(temp.path(), None).unwrap();
    assert_eq!(result.status, "unverified");
    assert!(!result.ok);
}
