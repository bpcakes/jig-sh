use super::*;

use super::comparison_scope::{entry, exact_head, git_stdout, initialized_repo};

fn loose_object_file_count(root: &Path) -> usize {
    std::fs::read_dir(root.join(".git/objects"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| !matches!(entry.file_name().to_str(), Some("info" | "pack")))
        .map(|directory| std::fs::read_dir(directory.path()).unwrap().count())
        .sum()
}

#[test]
fn deletion_only_changes_remain_available_to_affected_selection() {
    let temp = initialized_repo(&[("component/input.txt", "base\n")]);
    std::fs::remove_file(temp.path().join("component/input.txt")).unwrap();

    assert_eq!(
        repo_changed_paths_since(temp.path(), "HEAD").unwrap(),
        ["component/input.txt".to_string()]
    );
    let scope = capture_scope_v1(
        temp.path(),
        &exact_head(temp.path()),
        CurrentViewV1::Worktree,
    )
    .unwrap();
    assert!(scope.complete, "{scope:#?}");
    assert!(scope.entries.is_empty());
}

#[test]
fn affected_selection_preserves_committed_changes_reverted_only_in_the_worktree() {
    let temp = initialized_repo(&[("component/input.txt", "base\n")]);
    let base = git_stdout(temp.path(), &["rev-parse", "HEAD"]);
    std::fs::write(temp.path().join("component/input.txt"), "committed\n").unwrap();
    run_git(temp.path(), &["add", "component/input.txt"]);
    run_git(temp.path(), &["commit", "-q", "-m", "change input"]);
    std::fs::write(temp.path().join("component/input.txt"), "base\n").unwrap();
    std::fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    std::fs::write(temp.path().join(".agent/state/private.jsonl"), "ignored\n").unwrap();

    assert_eq!(
        repo_changed_paths_since(temp.path(), &base).unwrap(),
        ["component/input.txt".to_owned()]
    );
}

#[test]
fn broken_symbolic_head_is_not_treated_as_unborn() {
    let temp = initialized_repo(&[]);
    let symbolic_target = git_stdout(temp.path(), &["symbolic-ref", "HEAD"]);
    let loose_ref = temp.path().join(".git").join(&symbolic_target);
    std::fs::create_dir_all(loose_ref.parent().unwrap()).unwrap();
    std::fs::write(&loose_ref, format!("{}\n", "f".repeat(40))).unwrap();

    let error =
        resolve_comparison_v1(temp.path(), ComparisonRequestV1::IndexAgainstHead).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Failed to resolve HEAD for index comparison"),
        "{error:#}"
    );
    assert_eq!(
        resolve_empty_tree_for_unborn_repository(temp.path()).unwrap(),
        None
    );
}

#[test]
fn empty_comparison_resolution_does_not_write_a_git_object() {
    let temp = initialized_repo(&[]);
    assert_eq!(loose_object_file_count(temp.path()), 0);

    let resolved =
        resolve_comparison_v1(temp.path(), ComparisonRequestV1::IndexAgainstHead).unwrap();
    let empty_tree = resolved.baseline_oid().unwrap();

    assert_eq!(loose_object_file_count(temp.path()), 0);
    assert!(matches!(empty_tree.len(), 40 | 64));
}

#[test]
fn multiple_best_merge_bases_are_rejected_as_ambiguous() {
    let temp = initialized_repo(&[("base.txt", "base\n")]);
    let root = temp.path();
    let base = git_stdout(root, &["rev-parse", "HEAD"]);

    run_git(root, &["switch", "-q", "-c", "side-a"]);
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    run_git(root, &["add", "a.txt"]);
    run_git(root, &["commit", "-q", "-m", "side a"]);
    let side_a = git_stdout(root, &["rev-parse", "HEAD"]);
    let tree_a = git_stdout(root, &["rev-parse", "HEAD^{tree}"]);

    run_git(root, &["switch", "-q", "-c", "side-b", &base]);
    std::fs::write(root.join("b.txt"), "b\n").unwrap();
    run_git(root, &["add", "b.txt"]);
    run_git(root, &["commit", "-q", "-m", "side b"]);
    let side_b = git_stdout(root, &["rev-parse", "HEAD"]);
    let tree_b = git_stdout(root, &["rev-parse", "HEAD^{tree}"]);

    let merge_a = git_stdout(
        root,
        &[
            "commit-tree",
            &tree_a,
            "-p",
            &side_a,
            "-p",
            &side_b,
            "-m",
            "merge a",
        ],
    );
    let merge_b = git_stdout(
        root,
        &[
            "commit-tree",
            &tree_b,
            "-p",
            &side_b,
            "-p",
            &side_a,
            "-m",
            "merge b",
        ],
    );
    run_git(root, &["update-ref", "refs/heads/side-a", &merge_a]);
    run_git(root, &["update-ref", "refs/heads/side-b", &merge_b]);
    run_git(root, &["symbolic-ref", "HEAD", "refs/heads/side-a"]);

    let error = resolve_comparison_v1(
        root,
        ComparisonRequestV1::MergeBaseRef {
            requested_ref: "side-b".to_owned(),
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("comparison is ambiguous"),
        "{error:#}"
    );
}

#[test]
fn staged_deletion_with_ignored_replacement_blocks_measurable_scope() {
    let temp = initialized_repo(&[("ignored-input.txt", "base\n")]);
    std::fs::write(temp.path().join(".gitignore"), "ignored-input.txt\n").unwrap();
    run_git(temp.path(), &["add", ".gitignore"]);
    run_git(temp.path(), &["commit", "-q", "-m", "ignore tracked input"]);
    let comparison = exact_head(temp.path());
    run_git(temp.path(), &["rm", "--cached", "ignored-input.txt"]);
    std::fs::write(temp.path().join("ignored-input.txt"), "replacement\n").unwrap();

    let error = capture_scope_v1(temp.path(), &comparison, CurrentViewV1::Worktree).unwrap_err();
    assert!(
        error.to_string().contains("ignored same-path replacement"),
        "{error:#}"
    );
}

#[test]
fn exact_directory_observation_does_not_expand_tracked_descendants() {
    let temp = initialized_repo(&[("src/one.rs", "one\n"), ("src/nested/two.rs", "two\n")]);
    let requested = ["src".to_owned(), "src/one.rs".to_owned()];
    let facts = observe_exact_paths_v1(temp.path(), CurrentViewV1::Worktree, &requested).unwrap();

    assert_eq!(
        facts,
        [
            ExactCurrentPathFactV1 {
                path: "src".to_owned(),
                state: ExactCurrentPathStateV1::Unsupported {
                    reason: ScopeIssueKindV1::EmbeddedRepository,
                },
            },
            ExactCurrentPathFactV1 {
                path: "src/one.rs".to_owned(),
                state: ExactCurrentPathStateV1::Regular,
            },
        ]
    );
    for view in [CurrentViewV1::Index, CurrentViewV1::Inventory] {
        assert_eq!(
            observe_exact_paths_v1(temp.path(), view, &requested).unwrap(),
            [
                ExactCurrentPathFactV1 {
                    path: "src".to_owned(),
                    state: ExactCurrentPathStateV1::Missing,
                },
                ExactCurrentPathFactV1 {
                    path: "src/one.rs".to_owned(),
                    state: ExactCurrentPathStateV1::Regular,
                },
            ],
            "view {view:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn reverse_symlink_type_change_has_no_regular_file_ancestry() {
    use std::os::unix::fs::symlink;

    let temp = initialized_repo(&[("target.txt", "base\n")]);
    symlink("target.txt", temp.path().join("typed.txt")).unwrap();
    run_git(temp.path(), &["add", "typed.txt"]);
    run_git(temp.path(), &["commit", "-m", "symlink", "-q"]);
    let comparison = exact_head(temp.path());
    std::fs::remove_file(temp.path().join("typed.txt")).unwrap();
    std::fs::write(temp.path().join("typed.txt"), "regular\n").unwrap();

    let scope = capture_scope_v1(temp.path(), &comparison, CurrentViewV1::Worktree).unwrap();
    assert!(scope.complete, "{scope:#?}");
    let typed = entry(&scope, "typed.txt");
    assert_eq!(typed.kind, FileChangeKindV1::TypeChanged);
    assert_eq!(typed.baseline, None);
}

#[test]
fn malformed_records_diagnostics_and_disappeared_untracked_paths_are_typed() {
    assert!(
        change_scope::parse::parse_raw_diff_z(b":bad\0path\0", 10)
            .unwrap_err()
            .to_string()
            .contains("malformed Git raw diff")
    );
    assert!(
        change_scope::parse::parse_index_stage_z(b"100644 bad 0\tpath\0", 10)
            .unwrap_err()
            .to_string()
            .contains("malformed git ls-files")
    );
    assert!(
        change_scope::parse::parse_raw_diff_z(
            b":100644 100644 1111111111111111111111111111111111111111 2222222222222222222222222222222222222222 M\tpath",
            10,
        )
        .unwrap_err()
        .to_string()
        .contains("missing NUL terminator")
    );
    assert!(
        change_scope::parse::parse_index_stage_z(
            b"100644 1111111111111111111111111111111111111111 0\tpath",
            10,
        )
        .unwrap_err()
        .to_string()
        .contains("missing NUL terminator")
    );
    let issues = change_scope::rename_diagnostics(
        b"warning: exhaustive rename detection was skipped due to too many files\n",
    );
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, ScopeIssueKindV1::RenameLimit);
    let issues = change_scope::rename_diagnostics(b"unexpected diagnostic\n");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, ScopeIssueKindV1::GitDiagnostic);
    assert!(issues[0].message.contains("unexpected diagnostic"));
    let issues = change_scope::rename_diagnostics(
        b"warning: exhaustive rename detection was skipped due to too many files\nunexpected diagnostic\n",
    );
    assert_eq!(issues.len(), 2);
    assert!(
        issues
            .iter()
            .any(|issue| issue.kind == ScopeIssueKindV1::RenameLimit)
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.kind == ScopeIssueKindV1::GitDiagnostic)
    );

    let mut entries = Vec::new();
    let mut issues = Vec::new();
    change_scope::append_disappeared_untracked_entry_for_test(&mut entries, &mut issues);
    assert!(entries.is_empty());
    assert!(issues.is_empty());
}
