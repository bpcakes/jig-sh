#[cfg(all(test, unix))]
mod preparation_tests {
    use std::ffi::OsStr;
    #[cfg(target_os = "linux")]
    use std::ffi::OsString;
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::os::unix::ffi::OsStringExt as _;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use super::*;
    use crate::runtime::loops::occurrence::{
        OccurrenceAttentionScope, OccurrenceClaim, OccurrenceGuard, OccurrenceStore,
    };
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

    struct CancelWhenPresent(PathBuf);

    impl crate::execution::ExecutionObserver for CancelWhenPresent {}

    impl crate::execution::ExecutionCancellation for CancelWhenPresent {
        fn cancelled(&self) -> bool {
            self.0.exists()
        }
    }

    fn workflow(lease_ttl_seconds: u64) -> ResolvedWorkflow {
        ResolvedWorkflow {
            id: "pr-manager".into(),
            kind: super::super::workflow::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds,
            max_attempts: 2,
            backoff_seconds: 1,
            codex_home_configured: None,
            schedule: None,
            codex_task: None,
        }
    }

    fn item(head_sha: String) -> PrWorkItem {
        PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha,
            reasons: vec!["failing_checks".into()],
        }
    }

    #[test]
    fn pr_worktree_root_rejects_a_symlinked_prs_component() {
        use std::os::unix::fs::symlink;

        let repo = tempdir().unwrap();
        let redirected = tempdir().unwrap();
        TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let worktrees = repo.path().join(LOOP_RUNTIME_DIR).join("worktrees");
        fs::create_dir_all(&worktrees).unwrap();
        symlink(redirected.path(), worktrees.join("prs")).unwrap();
        let ctx = RepoContext::load_from(repo.path()).unwrap();

        let error = prepare_worktree(
            &ctx,
            &workflow(60),
            &item("a".repeat(40)),
            None,
            &mut NoopExecutionObserver,
        )
        .unwrap_err();
        let PrRepairStepError::Failed(error) = error.source else {
            panic!("symlinked PR worktree roots must be a preparation failure");
        };

        assert!(
            format!("{error:#}").contains("component is a symlink"),
            "{error:#}"
        );
        assert!(fs::read_dir(redirected.path()).unwrap().next().is_none());
    }

    #[test]
    fn registered_pr_worktree_rejects_a_forged_gitfile() {
        let _guard = lock_env();
        let repo = tempdir().unwrap();
        TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(repo.path())
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(&["init"]);
        git(&["config", "user.email", "fixture@example.com"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["add", "."]);
        git(&["commit", "-m", "fixture"]);
        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let workflow = workflow(60);
        let item = item(git(&["rev-parse", "HEAD"]));
        let worktree = pr_worktree_path(&ctx, &workflow, &item);
        fs::create_dir_all(worktree.parent().unwrap()).unwrap();
        git(&[
            "worktree",
            "add",
            "--detach",
            worktree.to_str().unwrap(),
            "HEAD",
        ]);
        assert!(
            pr_worktree_is_registered(&ctx, &worktree, &mut NoopExecutionObserver).unwrap()
        );

        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", repo.path().join(".git").display()),
        )
        .unwrap();

        assert!(
            !pr_worktree_is_registered(&ctx, &worktree, &mut NoopExecutionObserver).unwrap()
        );
        assert!(worktree.join(".git").is_file());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registered_pr_worktree_preserves_a_non_utf8_common_git_directory() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let repo = temp
            .path()
            .join(OsString::from_vec(b"common-repo-\xff".to_vec()));
        let scheduler = temp.path().join("scheduler");
        TestRepoBuilder::new(&repo)
            .required_commands(Vec::<String>::new())
            .write();
        let git = |cwd: &Path, args: &[&OsStr]| {
            let output = Command::new("git")
                .current_dir(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
            output.stdout
        };
        git(&repo, &[OsStr::new("init")]);
        git(
            &repo,
            &[
                OsStr::new("config"),
                OsStr::new("user.email"),
                OsStr::new("fixture@example.com"),
            ],
        );
        git(
            &repo,
            &[
                OsStr::new("config"),
                OsStr::new("user.name"),
                OsStr::new("Fixture"),
            ],
        );
        git(&repo, &[OsStr::new("add"), OsStr::new(".")]);
        git(
            &repo,
            &[
                OsStr::new("commit"),
                OsStr::new("-m"),
                OsStr::new("fixture"),
            ],
        );
        git(
            &repo,
            &[
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                scheduler.as_os_str(),
                OsStr::new("HEAD"),
            ],
        );
        let ctx = RepoContext::load_from(&scheduler).unwrap();
        let head = String::from_utf8(git(
            &scheduler,
            &[OsStr::new("rev-parse"), OsStr::new("HEAD")],
        ))
        .unwrap()
        .trim()
        .to_string();
        let worktree = pr_worktree_path(&ctx, &workflow(60), &item(head));
        fs::create_dir_all(worktree.parent().unwrap()).unwrap();
        git(
            &scheduler,
            &[
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                worktree.as_os_str(),
                OsStr::new("HEAD"),
            ],
        );

        assert!(
            pr_worktree_is_registered(&ctx, &worktree, &mut NoopExecutionObserver).unwrap()
        );
    }

    #[test]
    fn cancellation_after_worktree_add_defers_cleanup_to_finalization() {
        let _guard = lock_env();
        let repo = tempdir().unwrap();
        let origin_parent = tempdir().unwrap();
        let origin = origin_parent.path().join("origin.git");
        TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(repo.path())
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(&["init"]);
        git(&["config", "user.email", "fixture@example.com"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["add", "."]);
        git(&["commit", "-m", "fixture"]);
        git(&["branch", "repair/example"]);
        let head_sha = git(&["rev-parse", "repair/example"]);
        let clone = Command::new("git")
            .args([
                "clone",
                "--bare",
                repo.path().to_str().unwrap(),
                origin.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(clone.status.success(), "{clone:?}");
        git(&["remote", "add", "origin", origin.to_str().unwrap()]);

        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        assert!(real_git.status.success(), "{real_git:?}");
        let real_git = String::from_utf8(real_git.stdout).unwrap();
        let wrapper = repo.path().join("git-wrapper.sh");
        fs::write(
            &wrapper,
            r#"#!/bin/sh
"$JIG_TEST_REAL_GIT" "$@"
status=$?
if [ "$status" -eq 0 ]; then
  case "$*" in
    *"worktree add"*) touch "$JIG_TEST_CANCEL_MARKER" ;;
  esac
fi
exit "$status"
"#,
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let marker = repo.path().join("cancel-after-worktree-add");
        let _real_git = EnvVarGuard::set("JIG_TEST_REAL_GIT", OsStr::new(real_git.trim()));
        let _marker = EnvVarGuard::set("JIG_TEST_CANCEL_MARKER", marker.as_os_str());
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, wrapper.as_os_str());
        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let workflow = workflow(60);
        let item = item(head_sha);
        let lease = json!({"owner": "fixture-owner"});
        let mut occurrences = OccurrenceStore::new(&ctx);
        let OccurrenceClaim::Acquired(occurrence) = occurrences
            .claim_scheduled(
                &workflow.id,
                100,
                60,
                OccurrenceAttentionScope::Workflow,
                false,
            )
            .unwrap()
        else {
            panic!("expected occurrence claim");
        };
        let occurrence_guard =
            OccurrenceGuard::start(occurrences.clone(), &occurrence, 60).unwrap();
        let reservation = occurrence_guard.worktree_reservation();
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: Some(&reservation),
        };
        let expected_worktree = pr_worktree_path(&ctx, &workflow, &item);
        let mut observer = CancelWhenPresent(marker);

        let outcome = run_pr_repair(&repair, &json!({}), &mut observer);

        let PrRepairOutcome::Cancelled { worktree, .. } = &outcome else {
            panic!("post-add cancellation should remain an unexecuted cancellation");
        };
        assert_eq!(
            worktree.as_ref().map(PreparedPrWorktree::path),
            Some(expected_worktree.as_path())
        );
        assert!(expected_worktree.exists());
        assert_eq!(
            occurrences.snapshot().unwrap()[0].worktree.as_deref(),
            Some(expected_worktree.to_string_lossy().as_ref())
        );
        let listing = Command::new(real_git.trim())
            .current_dir(repo.path())
            .args(["worktree", "list", "--porcelain"])
            .output()
            .unwrap();
        assert!(listing.status.success(), "{listing:?}");
        let registered_worktree = expected_worktree.canonicalize().unwrap();
        assert!(
            String::from_utf8_lossy(&listing.stdout)
                .lines()
                .any(|line| line == format!("worktree {}", registered_worktree.display()))
        );

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut AttemptStore::new(&ctx),
            outcome,
        )
        .unwrap();

        assert_eq!(action["worktree_retained"], false, "{action:#}");
        assert!(!expected_worktree.exists());
    }

    #[test]
    fn failed_preparation_cleanup_is_retained_without_a_post_release_retry() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let output = Command::new("git")
            .current_dir(temp.path())
            .args(["init"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = workflow(60);
        let item = item("a".repeat(40));
        let worktree = pr_worktree_path(&ctx, &workflow, &item);
        fs::create_dir_all(&worktree).unwrap();
        fs::write(worktree.join("partial-evidence"), "retain me\n").unwrap();
        let lease = json!({"owner": "fixture-owner"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };
        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut AttemptStore::new(&ctx),
            PrRepairOutcome::PreExecutionFailed {
                error: anyhow!("injected preparation failure"),
                worktree: Some(PreparedPrWorktree::Created(worktree.clone())),
                worker_receipt_id: None,
            },
        )
        .unwrap();

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(action["attention_kind"], "worktree_cleanup_failed");
        assert_eq!(action["unexecuted_reason"], "pre_execution_error");
        assert_eq!(action["worktree_retained"], true);
        assert!(worktree.join("partial-evidence").is_file());
    }

    #[test]
    fn preexisting_repair_worktree_survives_an_unexecuted_retry() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = workflow(60);
        let item = item("a".repeat(40));
        let worktree = pr_worktree_path(&ctx, &workflow, &item);
        fs::create_dir_all(&worktree).unwrap();
        fs::write(worktree.join("retained-repair"), "human evidence\n").unwrap();
        let lease = json!({"owner": "fixture-owner"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut AttemptStore::new(&ctx),
            PrRepairOutcome::PreExecutionFailed {
                error: anyhow!("fetch failed before the retry could reuse the worktree"),
                worktree: Some(PreparedPrWorktree::Retained(worktree.clone())),
                worker_receipt_id: None,
            },
        )
        .unwrap();

        assert_eq!(action["status"], "needs_attention", "{action:#}");
        assert_eq!(
            action["attention_kind"],
            "preexisting_repair_worktree_retained"
        );
        assert_eq!(action["worktree_retained"], true);
        assert!(worktree.join("retained-repair").is_file());
    }

    #[test]
    fn preparation_lease_loss_never_cleans_the_reassigned_worktree() {
        let _guard = lock_env();
        let repo = tempdir().unwrap();
        let origin_parent = tempdir().unwrap();
        let origin = origin_parent.path().join("origin.git");
        TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(repo.path())
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(&["init"]);
        git(&["config", "user.email", "fixture@example.com"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["add", "."]);
        git(&["commit", "-m", "fixture"]);
        git(&["branch", "repair/example"]);
        let head_sha = git(&["rev-parse", "repair/example"]);
        let clone = Command::new("git")
            .args([
                "clone",
                "--bare",
                repo.path().to_str().unwrap(),
                origin.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(clone.status.success(), "{clone:?}");
        git(&["remote", "add", "origin", origin.to_str().unwrap()]);

        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let workflow = workflow(1);
        let item = item(head_sha.clone());
        let worktree = pr_worktree_path(&ctx, &workflow, &item);
        let added = Command::new("git")
            .current_dir(repo.path())
            .args([
                "worktree",
                "add",
                "--detach",
                worktree.to_str().unwrap(),
                &head_sha,
            ])
            .output()
            .unwrap();
        assert!(added.status.success(), "{added:?}");

        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        assert!(real_git.status.success(), "{real_git:?}");
        let real_git = String::from_utf8(real_git.stdout).unwrap();
        let trigger = repo.path().join("preparation-fetch-started");
        let reassigned = repo.path().join("branch-lease-reassigned");
        let log = repo.path().join("preparation-git.log");
        let wrapper = repo.path().join("lease-loss-git.sh");
        fs::write(
            &wrapper,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$JIG_TEST_GIT_LOG"
case "$*" in
  *"fetch origin refs/heads/repair/example"*)
    touch "$JIG_TEST_FETCH_STARTED"
    while [ ! -f "$JIG_TEST_LEASE_REASSIGNED" ]; do sleep 0.05; done
    sleep 2
    exit 1
    ;;
esac
exec "$JIG_TEST_REAL_GIT" "$@"
"#,
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let _real_git = EnvVarGuard::set("JIG_TEST_REAL_GIT", OsStr::new(real_git.trim()));
        let _trigger = EnvVarGuard::set("JIG_TEST_FETCH_STARTED", trigger.as_os_str());
        let _reassigned =
            EnvVarGuard::set("JIG_TEST_LEASE_REASSIGNED", reassigned.as_os_str());
        let _log = EnvVarGuard::set("JIG_TEST_GIT_LOG", log.as_os_str());
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, wrapper.as_os_str());

        let mut leases = LeaseStore::new(&ctx);
        let mut replacement_store = leases.clone();
        let replacement_trigger = trigger;
        let replacement_marker = reassigned;
        let replacement = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !replacement_trigger.exists() {
                assert!(Instant::now() < deadline, "preparation fetch never started");
                std::thread::sleep(Duration::from_millis(10));
            }
            replacement_store
                .revoke_for_test("branch:repair/example")
                .unwrap();
            assert!(matches!(
                replacement_store
                    .acquire("branch:repair/example", 60)
                    .unwrap(),
                LeaseAcquire::Acquired(_)
            ));
            fs::write(replacement_marker, "reassigned\n").unwrap();
        });
        let mut observer = NoopExecutionObserver;

        let action = handle_actionable_pr(
            &ctx,
            &workflow,
            &mut leases,
            &mut AttemptStore::new(&ctx),
            &item,
            &json!({}),
            PrManagerExecution {
                codex_home: None,
                worktree_reservation: None,
                observer: &mut observer,
            },
        )
        .unwrap();
        replacement.join().unwrap();

        assert_eq!(
            action["attention_kind"], "branch_lease_lost_after_start",
            "{action:#}"
        );
        assert!(
            action["completed_error"]
                .as_str()
                .is_some_and(|error| error.contains("pre-existing repair worktree was retained")),
            "{action:#}"
        );
        assert_eq!(action["worktree_retained"], true, "{action:#}");
        assert!(worktree.exists());
        let commands = fs::read_to_string(log).unwrap();
        for destructive in ["worktree remove", "reset --hard", "clean -fd"] {
            assert!(
                !commands.contains(destructive),
                "former lease owner ran {destructive}: {commands}"
            );
        }
    }
}
