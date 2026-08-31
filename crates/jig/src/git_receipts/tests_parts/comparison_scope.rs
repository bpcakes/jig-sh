use super::*;

use std::process::Output;

use tempfile::TempDir;

pub(super) fn initialized_repo(files: &[(&str, &str)]) -> TempDir {
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init", "-q"]);
    run_git(temp.path(), &["config", "user.name", "Jig Test"]);
    run_git(
        temp.path(),
        &["config", "user.email", "jig@example.invalid"],
    );
    for (path, contents) in files {
        let full = temp.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, contents).unwrap();
    }
    if !files.is_empty() {
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "fixture", "-q"]);
    }
    temp
}

pub(super) fn git_stdout(root: &Path, args: &[&str]) -> String {
    let output = git_command(root, args);
    assert_git_success(args, &output);
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn git_command(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn assert_git_success(args: &[&str], output: &Output) {
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

pub(super) fn exact_head(root: &Path) -> ResolvedComparisonV1 {
    resolve_comparison_v1(
        root,
        ComparisonRequestV1::ExactTree {
            requested_oid: git_stdout(root, &["rev-parse", "HEAD"]),
            provenance: ExactTreeProvenanceV1::WorkPlan,
        },
    )
    .unwrap()
}

pub(super) fn entry<'a>(snapshot: &'a ScopeSnapshotV1, path: &str) -> &'a ScopeEntryV1 {
    snapshot
        .entries
        .iter()
        .find(|entry| entry.current_path == path)
        .unwrap_or_else(|| panic!("missing scope entry for {path}: {snapshot:#?}"))
}

fn baseline_path(entry: &ScopeEntryV1) -> Option<&str> {
    entry
        .baseline
        .as_ref()
        .map(|baseline| baseline.path.as_str())
}

fn fact<'a>(facts: &'a [ExactCurrentPathFactV1], path: &str) -> &'a ExactCurrentPathFactV1 {
    facts
        .iter()
        .find(|fact| fact.path == path)
        .unwrap_or_else(|| panic!("missing exact-path fact for {path}: {facts:#?}"))
}

#[test]
fn comparison_resolution_preserves_requested_peeled_tree_and_merge_base_identities() {
    let temp = initialized_repo(&[("source.txt", "first\n")]);
    let root = temp.path();
    let first_commit = git_stdout(root, &["rev-parse", "HEAD"]);
    let first_tree = git_stdout(root, &["rev-parse", "HEAD^{tree}"]);
    run_git(root, &["tag", "-a", "fixture-tag", "-m", "fixture tag"]);
    let tag_oid = git_stdout(root, &["rev-parse", "fixture-tag"]);

    std::fs::write(root.join("source.txt"), "second\n").unwrap();
    run_git(root, &["add", "source.txt"]);
    run_git(root, &["commit", "-m", "second", "-q"]);
    let head = git_stdout(root, &["rev-parse", "HEAD"]);

    assert_eq!(
        resolve_comparison_v1(
            root,
            ComparisonRequestV1::MergeBaseRef {
                requested_ref: "fixture-tag".to_string(),
            },
        )
        .unwrap(),
        ResolvedComparisonV1::MergeBase {
            requested_ref: "fixture-tag".to_string(),
            resolved_ref_oid: first_commit.clone(),
            head_oid: head.clone(),
            merge_base_oid: first_commit.clone(),
        }
    );
    assert_eq!(
        resolve_comparison_v1(
            root,
            ComparisonRequestV1::ExactTree {
                requested_oid: tag_oid.clone(),
                provenance: ExactTreeProvenanceV1::Explicit,
            },
        )
        .unwrap(),
        ResolvedComparisonV1::ExactTree {
            requested_oid: tag_oid,
            peeled_commit_oid: Some(first_commit),
            tree_oid: first_tree.clone(),
            provenance: ExactTreeProvenanceV1::Explicit,
        }
    );
    assert_eq!(
        resolve_comparison_v1(
            root,
            ComparisonRequestV1::ExactTree {
                requested_oid: first_tree.clone(),
                provenance: ExactTreeProvenanceV1::UnbornWorktree,
            },
        )
        .unwrap(),
        ResolvedComparisonV1::ExactTree {
            requested_oid: first_tree.clone(),
            peeled_commit_oid: None,
            tree_oid: first_tree,
            provenance: ExactTreeProvenanceV1::UnbornWorktree,
        }
    );
    assert_eq!(
        resolve_comparison_v1(root, ComparisonRequestV1::IndexAgainstHead).unwrap(),
        ResolvedComparisonV1::IndexAgainstHead {
            head_or_empty_oid: head,
        }
    );
    assert_eq!(
        resolve_comparison_v1(
            root,
            ComparisonRequestV1::StrictInventory {
                reason: StrictInventoryReasonV1::ExplicitAudit,
            },
        )
        .unwrap(),
        ResolvedComparisonV1::StrictInventory {
            reason: StrictInventoryReasonV1::ExplicitAudit,
            fallback_from: None,
        }
    );
}

#[test]
fn comparison_resolution_handles_push_before_unborn_and_replacement_objects_safely() {
    let temp = initialized_repo(&[("source.txt", "first\n")]);
    let root = temp.path();
    let first = git_stdout(root, &["rev-parse", "HEAD"]);
    let first_tree = git_stdout(root, &["rev-parse", "HEAD^{tree}"]);
    std::fs::write(root.join("source.txt"), "second\n").unwrap();
    run_git(root, &["add", "source.txt"]);
    run_git(root, &["commit", "-m", "second", "-q"]);
    let second = git_stdout(root, &["rev-parse", "HEAD"]);
    run_git(root, &["replace", &first, &second]);

    let resolved = resolve_comparison_v1(
        root,
        ComparisonRequestV1::ExactTree {
            requested_oid: first,
            provenance: ExactTreeProvenanceV1::WorkPlan,
        },
    )
    .unwrap();
    assert_eq!(resolved.baseline_oid(), Some(first_tree.as_str()));

    let oid_bytes = git_stdout(root, &["rev-parse", "HEAD"]).len();
    let zeros = "0".repeat(oid_bytes);
    let empty = git_stdout(root, &["mktree"]);
    assert_eq!(
        resolve_comparison_v1(
            root,
            ComparisonRequestV1::ExactTree {
                requested_oid: zeros.clone(),
                provenance: ExactTreeProvenanceV1::PushBefore,
            },
        )
        .unwrap()
        .baseline_oid(),
        Some(empty.as_str())
    );
    assert!(
        resolve_comparison_v1(
            root,
            ComparisonRequestV1::ExactTree {
                requested_oid: zeros,
                provenance: ExactTreeProvenanceV1::Explicit,
            },
        )
        .unwrap_err()
        .to_string()
        .contains("push-before")
    );

    let unborn = initialized_repo(&[]);
    assert_eq!(
        resolve_comparison_v1(unborn.path(), ComparisonRequestV1::IndexAgainstHead)
            .unwrap()
            .baseline_oid(),
        Some(git_stdout(unborn.path(), &["mktree"]).as_str())
    );
    let blob = git_stdout(root, &["rev-parse", "HEAD:source.txt"]);
    assert!(
        resolve_comparison_v1(
            root,
            ComparisonRequestV1::ExactTree {
                requested_oid: blob,
                provenance: ExactTreeProvenanceV1::Explicit,
            },
        )
        .unwrap_err()
        .to_string()
        .contains("unsupported Git type `blob`")
    );
}

#[test]
fn exact_tree_resolution_never_computes_a_merge_base() {
    let temp = initialized_repo(&[("source.txt", "first\n")]);
    comparison::reset_comparison_merge_base_resolution_count();
    let _ = exact_head(temp.path());
    assert_eq!(comparison::comparison_merge_base_resolution_count(), 0);

    let _ = resolve_comparison_v1(
        temp.path(),
        ComparisonRequestV1::MergeBaseRef {
            requested_ref: "HEAD".to_string(),
        },
    )
    .unwrap();
    assert_eq!(comparison::comparison_merge_base_resolution_count(), 1);
}

#[test]
fn worktree_scope_unifies_staged_unstaged_untracked_and_rename_ancestry() {
    let temp = initialized_repo(&[
        (".gitattributes", "*.txt diff=hostile\n"),
        (".gitignore", "ignored.txt\n"),
        ("modified.txt", "base\n"),
        ("staged.txt", "base\n"),
        ("old.txt", "rename me\n"),
        ("copy-source.txt", "copy me\n"),
        ("deleted.txt", "delete me\n"),
    ]);
    let root = temp.path();
    let comparison = exact_head(root);
    run_git(root, &["config", "diff.renames", "false"]);
    run_git(root, &["config", "diff.hostile.command", "false"]);
    run_git(root, &["config", "diff.hostile.textconv", "false"]);

    std::fs::write(root.join("modified.txt"), "worktree\n").unwrap();
    std::fs::write(root.join("staged.txt"), "staged\n").unwrap();
    run_git(root, &["add", "staged.txt"]);
    run_git(root, &["mv", "old.txt", "renamed.txt"]);
    std::fs::copy(root.join("copy-source.txt"), root.join("copy.txt")).unwrap();
    run_git(root, &["add", "copy.txt"]);
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
    std::fs::write(root.join("ignored.txt"), "ignored\n").unwrap();
    std::fs::remove_file(root.join("deleted.txt")).unwrap();

    let snapshot = capture_scope_v1(root, &comparison, CurrentViewV1::Worktree).unwrap();
    assert!(snapshot.complete, "{snapshot:#?}");
    assert_eq!(
        entry(&snapshot, "modified.txt").kind,
        FileChangeKindV1::Modified
    );
    assert_eq!(
        baseline_path(entry(&snapshot, "modified.txt")),
        Some("modified.txt")
    );
    assert_eq!(
        entry(&snapshot, "staged.txt").kind,
        FileChangeKindV1::Modified
    );
    assert_eq!(
        entry(&snapshot, "renamed.txt").kind,
        FileChangeKindV1::Renamed
    );
    assert_eq!(
        baseline_path(entry(&snapshot, "renamed.txt")),
        Some("old.txt")
    );
    assert_eq!(entry(&snapshot, "copy.txt").kind, FileChangeKindV1::Added);
    assert_eq!(entry(&snapshot, "copy.txt").baseline, None);
    assert_eq!(
        entry(&snapshot, "untracked.txt").kind,
        FileChangeKindV1::Untracked
    );
    assert!(
        !snapshot
            .entries
            .iter()
            .any(|entry| entry.current_path == "ignored.txt")
    );
    assert!(
        !snapshot
            .entries
            .iter()
            .any(|entry| entry.current_path == "deleted.txt")
    );

    let affected = repo_changed_paths_since(root, "HEAD").unwrap();
    for path in [
        "copy.txt",
        "deleted.txt",
        "modified.txt",
        "old.txt",
        "renamed.txt",
        "staged.txt",
        "untracked.txt",
    ] {
        assert!(
            affected.contains(&path.to_string()),
            "missing {path}: {affected:?}"
        );
    }
    assert!(!affected.contains(&"ignored.txt".to_string()));
}

#[test]
fn index_inventory_and_exact_observations_use_the_requested_current_view() {
    let temp = initialized_repo(&[
        ("staged.txt", "base\n"),
        ("unstaged.txt", "base\n"),
        ("unchanged[1].txt", "base\n"),
    ]);
    let root = temp.path();
    std::fs::write(root.join("staged.txt"), "index\n").unwrap();
    run_git(root, &["add", "staged.txt"]);
    let index_blob = git_stdout(root, &["rev-parse", ":staged.txt"]);
    std::fs::write(root.join("staged.txt"), "worktree after index\n").unwrap();
    std::fs::write(root.join("unstaged.txt"), "worktree\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();

    let comparison = resolve_comparison_v1(root, ComparisonRequestV1::IndexAgainstHead).unwrap();
    let index_scope = capture_scope_v1(root, &comparison, CurrentViewV1::Index).unwrap();
    assert!(index_scope.complete, "{index_scope:#?}");
    assert_eq!(index_scope.entries.len(), 1);
    assert_eq!(
        entry(&index_scope, "staged.txt").current_source,
        CurrentSourceV1::IndexBlob { oid: index_blob }
    );
    assert_eq!(
        baseline_path(entry(&index_scope, "staged.txt")),
        Some("staged.txt")
    );

    let paths = [
        "staged.txt".to_string(),
        "unstaged.txt".to_string(),
        "unchanged[1].txt".to_string(),
        "untracked.txt".to_string(),
        "missing.txt".to_string(),
    ];
    let index_facts = observe_exact_paths_v1(root, CurrentViewV1::Index, &paths).unwrap();
    assert_eq!(
        fact(&index_facts, "staged.txt").state,
        ExactCurrentPathStateV1::Regular
    );
    assert_eq!(
        fact(&index_facts, "unstaged.txt").state,
        ExactCurrentPathStateV1::Regular
    );
    assert_eq!(
        fact(&index_facts, "unchanged[1].txt").state,
        ExactCurrentPathStateV1::Regular
    );
    assert_eq!(
        fact(&index_facts, "untracked.txt").state,
        ExactCurrentPathStateV1::Missing
    );
    assert_eq!(
        fact(&index_facts, "missing.txt").state,
        ExactCurrentPathStateV1::Missing
    );

    let worktree_facts = observe_exact_paths_v1(root, CurrentViewV1::Worktree, &paths).unwrap();
    assert_eq!(
        fact(&worktree_facts, "untracked.txt").state,
        ExactCurrentPathStateV1::Regular
    );
    assert_eq!(
        fact(&worktree_facts, "missing.txt").state,
        ExactCurrentPathStateV1::Missing
    );

    let inventory = resolve_comparison_v1(
        root,
        ComparisonRequestV1::StrictInventory {
            reason: StrictInventoryReasonV1::ExplicitCheck,
        },
    )
    .unwrap();
    let inventory_scope = capture_scope_v1(root, &inventory, CurrentViewV1::Inventory).unwrap();
    assert!(inventory_scope.complete, "{inventory_scope:#?}");
    assert_eq!(inventory_scope.entries.len(), 4);
    assert_eq!(
        entry(&inventory_scope, "untracked.txt").kind,
        FileChangeKindV1::Untracked
    );
    assert!(inventory_scope.entries.iter().all(|entry| {
        entry.current_path == "untracked.txt" || entry.kind == FileChangeKindV1::Unchanged
    }));
    let inventory_facts = observe_exact_paths_v1(root, CurrentViewV1::Inventory, &paths).unwrap();
    assert_eq!(
        fact(&inventory_facts, "untracked.txt").state,
        ExactCurrentPathStateV1::Regular
    );
}

#[test]
fn intent_to_add_sparse_gitlink_and_symlink_states_fail_closed() {
    let temp = initialized_repo(&[("tracked.txt", "base\n")]);
    let root = temp.path();
    let comparison = exact_head(root);
    std::fs::write(root.join("intent.txt"), "intent\n").unwrap();
    run_git(root, &["add", "-N", "intent.txt"]);
    let facts =
        observe_exact_paths_v1(root, CurrentViewV1::Worktree, &["intent.txt".to_string()]).unwrap();
    assert_eq!(
        facts[0].state,
        ExactCurrentPathStateV1::Unsupported {
            reason: ScopeIssueKindV1::IntentToAdd,
        }
    );
    let scope = capture_scope_v1(root, &comparison, CurrentViewV1::Worktree).unwrap();
    assert!(!scope.complete);
    assert!(
        scope
            .issues
            .iter()
            .any(|issue| issue.kind == ScopeIssueKindV1::IntentToAdd)
    );
    assert_eq!(
        repo_changed_paths_since(root, "HEAD").unwrap(),
        ["intent.txt".to_string()]
    );

    let sparse = initialized_repo(&[("tracked.txt", "base\n")]);
    run_git(
        sparse.path(),
        &["update-index", "--skip-worktree", "tracked.txt"],
    );
    let facts = observe_exact_paths_v1(
        sparse.path(),
        CurrentViewV1::Worktree,
        &["tracked.txt".to_string()],
    )
    .unwrap();
    assert_eq!(
        facts[0].state,
        ExactCurrentPathStateV1::Unsupported {
            reason: ScopeIssueKindV1::Sparse,
        }
    );
    let inventory = resolve_comparison_v1(
        sparse.path(),
        ComparisonRequestV1::StrictInventory {
            reason: StrictInventoryReasonV1::ExplicitAudit,
        },
    )
    .unwrap();
    let scope = capture_scope_v1(sparse.path(), &inventory, CurrentViewV1::Inventory).unwrap();
    assert!(!scope.complete);
    assert!(
        scope
            .issues
            .iter()
            .any(|issue| issue.kind == ScopeIssueKindV1::Sparse)
    );

    let gitlink = initialized_repo(&[("tracked.txt", "base\n")]);
    let commit = git_stdout(gitlink.path(), &["rev-parse", "HEAD"]);
    let cache = format!("160000,{commit},module");
    run_git(
        gitlink.path(),
        &["update-index", "--add", "--cacheinfo", &cache],
    );
    let index =
        resolve_comparison_v1(gitlink.path(), ComparisonRequestV1::IndexAgainstHead).unwrap();
    let scope = capture_scope_v1(gitlink.path(), &index, CurrentViewV1::Index).unwrap();
    assert!(!scope.complete);
    assert!(
        scope
            .issues
            .iter()
            .any(|issue| issue.kind == ScopeIssueKindV1::Gitlink)
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let symlinks = initialized_repo(&[("target.txt", "base\n")]);
        symlink("target.txt", symlinks.path().join("link.txt")).unwrap();
        let facts = observe_exact_paths_v1(
            symlinks.path(),
            CurrentViewV1::Worktree,
            &["link.txt".to_string()],
        )
        .unwrap();
        assert_eq!(
            facts[0].state,
            ExactCurrentPathStateV1::Unsupported {
                reason: ScopeIssueKindV1::Symlink,
            }
        );
        assert_eq!(
            repo_changed_paths_since(symlinks.path(), "HEAD").unwrap(),
            ["link.txt".to_string()]
        );

        let type_change = initialized_repo(&[("typed.txt", "base\n"), ("target.txt", "base\n")]);
        let comparison = exact_head(type_change.path());
        std::fs::remove_file(type_change.path().join("typed.txt")).unwrap();
        symlink("target.txt", type_change.path().join("typed.txt")).unwrap();
        let scope =
            capture_scope_v1(type_change.path(), &comparison, CurrentViewV1::Worktree).unwrap();
        assert!(!scope.complete);
        assert!(
            scope
                .issues
                .iter()
                .any(|issue| issue.kind == ScopeIssueKindV1::Symlink)
        );
    }
}

#[test]
fn unmerged_index_non_utf8_and_special_paths_are_typed_instead_of_flattened() {
    let temp = initialized_repo(&[("conflict.txt", "base\n")]);
    let root = temp.path();
    let main_branch = git_stdout(root, &["symbolic-ref", "--short", "HEAD"]);
    run_git(root, &["checkout", "-q", "-b", "other"]);
    std::fs::write(root.join("conflict.txt"), "other\n").unwrap();
    run_git(root, &["add", "conflict.txt"]);
    run_git(root, &["commit", "-m", "other", "-q"]);
    run_git(root, &["checkout", "-q", &main_branch]);
    std::fs::write(root.join("conflict.txt"), "main\n").unwrap();
    run_git(root, &["add", "conflict.txt"]);
    run_git(root, &["commit", "-m", "main", "-q"]);
    let merge = git_command(root, &["merge", "other"]);
    assert!(!merge.status.success());

    let comparison = resolve_comparison_v1(root, ComparisonRequestV1::IndexAgainstHead).unwrap();
    let scope = capture_scope_v1(root, &comparison, CurrentViewV1::Index).unwrap();
    assert!(!scope.complete, "{scope:#?}");
    assert!(
        scope
            .issues
            .iter()
            .any(|issue| issue.kind == ScopeIssueKindV1::Unmerged)
    );
    let facts =
        observe_exact_paths_v1(root, CurrentViewV1::Index, &["conflict.txt".to_string()]).unwrap();
    assert_eq!(
        facts[0].state,
        ExactCurrentPathStateV1::Unsupported {
            reason: ScopeIssueKindV1::Unmerged,
        }
    );

    #[cfg(unix)]
    {
        use std::os::unix::net::UnixListener;

        let special = initialized_repo(&[("tracked.txt", "base\n")]);
        let _listener = UnixListener::bind(special.path().join("socket.input")).unwrap();
        let facts = observe_exact_paths_v1(
            special.path(),
            CurrentViewV1::Worktree,
            &["socket.input".to_string()],
        )
        .unwrap();
        assert_eq!(
            facts[0].state,
            ExactCurrentPathStateV1::Unsupported {
                reason: ScopeIssueKindV1::Special,
            }
        );

        std::fs::create_dir(special.path().join("embedded.input")).unwrap();
        let facts = observe_exact_paths_v1(
            special.path(),
            CurrentViewV1::Worktree,
            &["embedded.input".to_string()],
        )
        .unwrap();
        assert_eq!(
            facts[0].state,
            ExactCurrentPathStateV1::Unsupported {
                reason: ScopeIssueKindV1::EmbeddedRepository,
            }
        );
    }

    #[cfg(all(unix, not(target_vendor = "apple")))]
    {
        use std::os::unix::ffi::OsStringExt;

        let non_utf8 = initialized_repo(&[("tracked.txt", "base\n")]);
        let name = OsString::from_vec(vec![b'n', b'o', b'n', b'-', 0xff]);
        std::fs::write(non_utf8.path().join(name), "bytes\n").unwrap();
        let scope = capture_scope_v1(
            non_utf8.path(),
            &exact_head(non_utf8.path()),
            CurrentViewV1::Worktree,
        )
        .unwrap();
        assert!(!scope.complete);
        assert!(
            scope
                .issues
                .iter()
                .any(|issue| issue.kind == ScopeIssueKindV1::NonUtf8Path)
        );
        assert!(repo_changed_paths_since(non_utf8.path(), "HEAD").is_err());
    }
}

#[test]
fn tracked_inventory_missing_from_the_worktree_is_incomplete_and_exactly_missing() {
    let temp = initialized_repo(&[("tracked.txt", "base\n")]);
    std::fs::remove_file(temp.path().join("tracked.txt")).unwrap();
    let inventory = resolve_comparison_v1(
        temp.path(),
        ComparisonRequestV1::StrictInventory {
            reason: StrictInventoryReasonV1::ExplicitCheck,
        },
    )
    .unwrap();
    let scope = capture_scope_v1(temp.path(), &inventory, CurrentViewV1::Inventory).unwrap();
    assert!(!scope.complete);
    assert!(
        scope
            .issues
            .iter()
            .any(|issue| issue.kind == ScopeIssueKindV1::MissingWorktreeEntry)
    );
    let facts = observe_exact_paths_v1(
        temp.path(),
        CurrentViewV1::Inventory,
        &["tracked.txt".to_string()],
    )
    .unwrap();
    assert_eq!(facts[0].state, ExactCurrentPathStateV1::Missing);
}

#[test]
fn exact_observation_validates_inputs_and_all_primitives_are_cancellable_and_bounded() {
    let temp = initialized_repo(&[("tracked.txt", "base\n")]);
    let root = temp.path();
    assert!(
        observe_exact_paths_v1(root, CurrentViewV1::Worktree, &["../outside".to_string()])
            .unwrap_err()
            .to_string()
            .contains("traversal")
    );
    assert!(
        observe_exact_paths_v1(root, CurrentViewV1::Worktree, &[".agent/state".to_string()])
            .unwrap_err()
            .to_string()
            .contains("protected")
    );
    for (path, message) in [
        ("/absolute", "repository-relative"),
        ("nested/C:/escape", "repository-relative"),
        ("duplicate//segment", "empty, dot, or traversal"),
        ("nul\0path", "NUL"),
    ] {
        assert!(
            observe_exact_paths_v1(root, CurrentViewV1::Worktree, &[path.to_string()])
                .unwrap_err()
                .to_string()
                .contains(message)
        );
    }
    assert!(
        observe_exact_paths_v1(
            root,
            CurrentViewV1::Worktree,
            &["tracked.txt".to_string(), "tracked.txt".to_string()],
        )
        .unwrap_err()
        .to_string()
        .contains("duplicate")
    );
    assert!(
        observe_exact_paths_v1(
            root,
            CurrentViewV1::Worktree,
            &["x".repeat(MAX_EXACT_CURRENT_PATH_BYTES_V1 + 1)],
        )
        .unwrap_err()
        .to_string()
        .contains("byte limit")
    );
    assert!(
        observe_exact_paths_v1(
            root,
            CurrentViewV1::Worktree,
            &vec!["x".to_string(); MAX_EXACT_CURRENT_PATH_FACTS_V1 + 1],
        )
        .unwrap_err()
        .to_string()
        .contains("permits at most")
    );
    let cancelled = || true;
    let error = resolve_comparison_v1_with_cancellation(
        root,
        ComparisonRequestV1::IndexAgainstHead,
        &cancelled,
    )
    .unwrap_err();
    assert!(is_git_receipt_collection_cancellation(&error));
    let comparison = exact_head(root);
    let error =
        capture_scope_v1_with_cancellation(root, &comparison, CurrentViewV1::Worktree, &cancelled)
            .unwrap_err();
    assert!(is_git_receipt_collection_cancellation(&error));
    let error = observe_exact_paths_v1_with_cancellation(
        root,
        CurrentViewV1::Worktree,
        &["tracked.txt".to_string()],
        &cancelled,
    )
    .unwrap_err();
    assert!(is_git_receipt_collection_cancellation(&error));

    std::fs::write(root.join("tracked.txt"), "changed\n").unwrap();
    let bounded = GATE_SCOPE_DIFF_OUTPUT_LIMIT_OVERRIDE.with(|limit| {
        let previous = limit.replace(Some(1));
        let result = capture_scope_v1(root, &comparison, CurrentViewV1::Worktree);
        limit.set(previous);
        result
    });
    assert!(
        bounded
            .unwrap_err()
            .to_string()
            .contains("output limit of 1 bytes")
    );
}

#[test]
fn successful_git_rename_limit_degradation_marks_the_scope_incomplete() {
    let temp = initialized_repo(&[
        ("old-a.txt", "alpha alpha alpha\n"),
        ("old-b.txt", "bravo bravo bravo\n"),
    ]);
    let comparison = exact_head(temp.path());
    std::fs::remove_file(temp.path().join("old-a.txt")).unwrap();
    std::fs::remove_file(temp.path().join("old-b.txt")).unwrap();
    std::fs::write(temp.path().join("new-a.txt"), "charlie charlie charlie\n").unwrap();
    std::fs::write(temp.path().join("new-b.txt"), "delta delta delta\n").unwrap();
    run_git(temp.path(), &["add", "new-a.txt", "new-b.txt"]);

    let scope = change_scope::with_scope_rename_limit(1, || {
        capture_scope_v1(temp.path(), &comparison, CurrentViewV1::Worktree)
    })
    .unwrap();
    assert!(!scope.complete, "{scope:#?}");
    assert!(
        scope
            .issues
            .iter()
            .any(|issue| issue.kind == ScopeIssueKindV1::RenameLimit)
    );
}
