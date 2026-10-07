#[cfg(all(test, unix))]
mod review_round37_tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::{TestRepoBuilder, lock_env};

    fn workflow() -> ResolvedWorkflow {
        ResolvedWorkflow {
            id: "pr-manager".into(),
            kind: super::super::workflow::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 2,
            backoff_seconds: 1,
            codex_home_configured: None,
            schedule: None,
            codex_task: None,
        }
    }

    fn item(head_ref: &str, head_sha: String) -> PrWorkItem {
        PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: head_ref.into(),
            head_sha,
            reasons: vec!["merge_conflict".into()],
        }
    }

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn pr_worktree_component_is_bounded_and_collision_resistant() {
        let repo = tempdir().unwrap();
        TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let workflow = workflow();
        let long_prefix = "a".repeat(400);
        let first = pr_worktree_path(
            &ctx,
            &workflow,
            &item(&format!("repair/{long_prefix}-first"), "a".repeat(40)),
        );
        let second = pr_worktree_path(
            &ctx,
            &workflow,
            &item(&format!("repair/{long_prefix}-second"), "a".repeat(40)),
        );
        let slash = pr_worktree_path(
            &ctx,
            &workflow,
            &item("repair/a/b", "a".repeat(40)),
        );
        let dash = pr_worktree_path(
            &ctx,
            &workflow,
            &item("repair/a-b", "a".repeat(40)),
        );

        assert!(first.file_name().unwrap().to_string_lossy().len() < 255);
        assert_ne!(first, second, "the digest must cover the truncated suffix");
        assert_ne!(slash, dash, "sanitization collisions need distinct paths");
    }

    #[test]
    fn authenticated_existing_worktree_is_recreated_without_mutating_repo_identity() {
        let _guard = lock_env();
        let repo = tempdir().unwrap();
        let remote_parent = tempdir().unwrap();
        let remote = remote_parent.path().join("origin.git");
        TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.name", "Fixture User"]);
        git(
            repo.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        fs::write(repo.path().join(".gitignore"), "ignored-cache/\n").unwrap();
        fs::write(repo.path().join("tracked.txt"), "fixture\n").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-m", "fixture"]);
        git(repo.path(), &["branch", "repair/example"]);
        let head = git(repo.path(), &["rev-parse", "repair/example"]);
        git(remote_parent.path(), &["init", "--bare", "origin.git"]);
        git(
            repo.path(),
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(
            repo.path(),
            &["push", "origin", "repair/example:repair/example"],
        );
        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let work_item = item("repair/example", head);

        let first = prepare_worktree(
            &ctx,
            &workflow(),
            &work_item,
            None,
            &mut NoopExecutionObserver,
        )
        .unwrap();
        let worktree = first.path().to_path_buf();
        fs::create_dir(worktree.join("ignored-cache")).unwrap();
        fs::write(worktree.join("ignored-cache/artifact"), "stale\n").unwrap();
        fs::create_dir(worktree.join("nested-repository")).unwrap();
        git(&worktree.join("nested-repository"), &["init"]);

        let second = prepare_worktree(
            &ctx,
            &workflow(),
            &work_item,
            None,
            &mut NoopExecutionObserver,
        )
        .unwrap();

        assert!(second.created_by_current_attempt());
        assert!(!worktree.join("ignored-cache").exists());
        assert!(!worktree.join("nested-repository").exists());
        assert_eq!(
            git(repo.path(), &["config", "--local", "user.name"]),
            "Fixture User"
        );
        assert_eq!(
            git(repo.path(), &["config", "--local", "user.email"]),
            "fixture@example.invalid"
        );
    }

    #[test]
    fn conflicted_merge_validates_only_the_workers_resolution() {
        let _guard = lock_env();
        let repo = tempdir().unwrap();
        let remote_parent = tempdir().unwrap();
        let remote = remote_parent.path().join("origin.git");
        TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.name", "Fixture User"]);
        git(
            repo.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        fs::write(repo.path().join("conflict.txt"), "common\n").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-m", "common"]);
        git(repo.path(), &["branch", "repair/example"]);

        fs::write(repo.path().join("conflict.txt"), "base branch\n").unwrap();
        fs::write(
            repo.path().join("base-whitespace.txt"),
            "incoming whitespace \n",
        )
        .unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-m", "base changes"]);
        git(repo.path(), &["checkout", "repair/example"]);
        fs::write(repo.path().join("conflict.txt"), "repair branch\n").unwrap();
        git(repo.path(), &["commit", "-am", "repair changes"]);
        let observed_head = git(repo.path(), &["rev-parse", "HEAD"]);

        git(remote_parent.path(), &["init", "--bare", "origin.git"]);
        git(
            repo.path(),
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(repo.path(), &["push", "origin", "main:main"]);
        git(
            repo.path(),
            &["push", "origin", "repair/example:repair/example"],
        );
        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let merge = start_base_merge(
            &ctx,
            repo.path(),
            "main",
            &mut NoopExecutionObserver,
        )
        .unwrap();
        assert_eq!(merge["conflicts"], true, "{merge:#}");
        let validation_tree = validation_tree_after_base_merge(
            &ctx,
            repo.path(),
            Some(&merge),
            &mut NoopExecutionObserver,
        )
        .unwrap();
        fs::write(repo.path().join("conflict.txt"), "resolved cleanly\n").unwrap();

        let push = commit_and_push(
            &ctx,
            repo.path(),
            "repair/example",
            &observed_head,
            merge.get("base_head").and_then(Value::as_str),
            &validation_tree,
            &mut NoopExecutionObserver,
        )
        .unwrap();

        assert_eq!(push["pushed"], true, "{push:#}");
        assert_eq!(
            git(repo.path(), &["show", "-s", "--format=%an <%ae>", "HEAD"]),
            "Jig PR Manager <jig-pr-manager@users.noreply.github.com>"
        );
        assert_eq!(
            git(repo.path(), &["config", "--local", "user.name"]),
            "Fixture User"
        );
    }
}
