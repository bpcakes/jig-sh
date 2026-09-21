#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::{Seek as _, SeekFrom, Write as _};
    use std::thread;

    use tempfile::tempdir;

    use super::*;
    use crate::bootstrap::GIT_BIN_ENV;
    use crate::state::{ReceiptInput, now_ms, record_receipt};
    use crate::test_env::{EnvVarGuard, lock_env};

    fn fixture() -> (tempfile::TempDir, RepoContext, PathBuf) {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = std::process::Command::new("git")
            .current_dir(temp.path())
            .arg("init")
            .output()
            .unwrap();
        assert!(git.status.success(), "{git:?}");
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let path = temp.path().join(WORKER_RECEIPT_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        (temp, ctx, path)
    }

    fn append_runtime_receipt(ctx: &RepoContext) -> Result<String> {
        record_receipt(ctx, receipt_input())
    }

    fn receipt_input() -> ReceiptInput<'static> {
        let now = now_ms();
        ReceiptInput {
            tool_name: "worker_run",
            args: json!({"purpose": "scheduled_codex_task"}),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: now,
            ended_at_ms: now,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: Some(json!({"kind": "worker_run"})),
            session_override: Some("session-example".into()),
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        }
    }

    #[test]
    fn short_journal_windows_accept_the_worker_receipt() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _path) = fixture();

        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let receipt_id = append_runtime_receipt(&ctx).unwrap();
        baseline.verify(&ctx, Some(&receipt_id)).unwrap();
    }

    #[test]
    fn concurrent_runtime_receipt_makes_worker_append_provenance_ambiguous() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        let competing_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let competing_writer =
            thread::spawn(move || record_receipt(&competing_ctx, receipt_input()));
        let competing_id = competing_writer.join().unwrap().unwrap();
        let receipt_id = append_runtime_receipt(&ctx).unwrap();
        let error = baseline.verify(&ctx, Some(&receipt_id)).unwrap_err();

        assert!(
            error.to_string().contains("only appended record"),
            "{error:#}"
        );
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 2);
        let diagnostic = serde_json::to_value(CheckoutDiagnostics::inspect(
            &ctx, ctx.root(), &Ok(false), &Ok("example-head".into()), &Err(error), Some(&receipt_id),
        )).unwrap();
        assert_eq!(diagnostic["reasons"], json!(["receipt_ambiguity"]));
        assert_eq!(diagnostic["parent_receipt_id"], receipt_id);
        assert_eq!(diagnostic["observed_receipt_ids"], json!([competing_id, receipt_id]));
        assert_eq!(diagnostic["receipt_ids_incomplete"], false);
    }

    #[test]
    fn receipt_append_without_a_reported_worker_receipt_is_rejected() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _path) = fixture();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        append_runtime_receipt(&ctx).unwrap();

        let error = baseline.verify(&ctx, None).unwrap_err();

        assert!(
            error.to_string().contains("expected no receipt append"),
            "{error:#}"
        );
    }

    #[test]
    fn partial_worker_receipt_append_is_rejected() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .unwrap()
            .write_all(b"{\"id\":\"receipt-worker\"}")
            .unwrap();

        let error = baseline
            .verify(&ctx, Some("receipt-worker"))
            .unwrap_err();

        assert!(
            error.to_string().contains("appended receipt record is unterminated"),
            "{error:#}"
        );
    }

    #[test]
    fn prefix_capture_starts_at_zero_and_ignores_a_concurrent_partial_append() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("receipts.jsonl");
        fs::write(&path, b"{}\npartial-append").unwrap();
        let mut file = File::open(&path).unwrap();
        file.seek(SeekFrom::End(0)).unwrap();

        capture_prefix(&path, &file, 3).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn git_index_probe_does_not_hold_the_receipt_writer_lock() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let _env_lock = lock_env();
        let (temp, ctx, _path) = fixture();
        let marker = temp.path().join("git-probe-started");
        let release = temp.path().join("release-git-probe");
        let git = temp.path().join("git-probe-stub");
        fs::write(
            &git,
            format!(
                "#!/bin/sh\ntouch '{}'\nwhile [ ! -e '{}' ]; do sleep 0.01; done\n",
                marker.display(),
                release.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
        let capture_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let capture = thread::spawn(move || ReceiptJournalBaseline::capture(&capture_ctx));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !marker.exists() {
            fs::write(&release, b"").unwrap();
            let detail = match capture.join().unwrap() {
                Ok(_) => "probe completed without creating its marker".to_string(),
                Err(error) => format!("probe failed: {error:#}"),
            };
            panic!("Git index probe did not start: {detail}");
        }

        let writer_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (sent, received) = mpsc::channel();
        let writer = thread::spawn(move || {
            let result = append_runtime_receipt(&writer_ctx);
            let _ = sent.send(());
            result
        });
        let wrote_during_probe = received.recv_timeout(Duration::from_secs(1)).is_ok();
        fs::write(&release, b"").unwrap();
        let baseline = capture.join().unwrap().unwrap();
        let receipt_id = writer.join().unwrap().unwrap();

        assert!(
            wrote_during_probe,
            "receipt append was blocked by the Git index probe"
        );
        baseline.verify(&ctx, Some(&receipt_id)).unwrap();
    }

    #[test]
    fn receipt_prefix_capture_does_not_hold_the_writer_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        let first_receipt = append_runtime_receipt(&ctx).unwrap();
        let capture_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let capture = thread::spawn(move || {
            let mut first_snapshot = true;
            ReceiptJournalBaseline::capture_with(&capture_ctx, || {
                if first_snapshot {
                    first_snapshot = false;
                    snapshot_tx.send(()).unwrap();
                    resume_rx.recv().unwrap();
                }
            })
        });
        snapshot_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let writer_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (writer_tx, writer_rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            let receipt = append_runtime_receipt(&writer_ctx);
            writer_tx.send(()).unwrap();
            receipt
        });
        writer_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        resume_tx.send(()).unwrap();

        let baseline = capture.join().unwrap().unwrap();
        let second_receipt = writer.join().unwrap().unwrap();
        baseline.verify(&ctx, None).unwrap();
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains(&first_receipt));
        assert!(contents.contains(&second_receipt));
    }

    #[test]
    fn receipt_baseline_exhausts_its_bounded_retry_budget() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _path) = fixture();
        append_runtime_receipt(&ctx).unwrap();
        let mut attempts = 0;

        let error = ReceiptJournalBaseline::capture_with(&ctx, || {
            attempts += 1;
            append_runtime_receipt(&ctx).unwrap();
        });
        let error = match error {
            Ok(_) => panic!("every receipt snapshot should be invalidated"),
            Err(error) => error,
        };

        assert_eq!(attempts, RECEIPT_SNAPSHOT_ATTEMPTS);
        assert!(format!("{error:#}").contains("changed during all 3 snapshot attempts"));
    }

    #[test]
    fn receipt_append_verification_does_not_hold_the_writer_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _path) = fixture();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let worker_receipt = append_runtime_receipt(&ctx).unwrap();
        let verify_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let verify = thread::spawn(move || {
            baseline.verify_with(&verify_ctx, Some(&worker_receipt), || {
                snapshot_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            })
        });
        snapshot_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let writer_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (writer_tx, writer_rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            let receipt = append_runtime_receipt(&writer_ctx);
            writer_tx.send(()).unwrap();
            receipt
        });
        writer_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        resume_tx.send(()).unwrap();

        let error = verify.join().unwrap().unwrap_err();
        writer.join().unwrap().unwrap();
        assert!(
            error
                .to_string()
                .contains("changed while it was being verified"),
            "{error:#}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn absent_receipt_journal_still_requires_a_successful_index_probe() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = lock_env();
        let (temp, ctx, _path) = fixture();
        let git = temp.path().join("git-fails");
        fs::write(&git, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());

        let Err(error) = ReceiptJournalBaseline::capture(&ctx) else {
            panic!("an index probe failure must reject an absent receipt journal");
        };

        assert!(
            error
                .to_string()
                .contains("Git command failed with status 1"),
            "{error:#}"
        );
    }

    #[test]
    fn worker_injection_before_the_runtime_receipt_is_rejected() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();

        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{}\n")
            .unwrap();
        let receipt_id = append_runtime_receipt(&ctx).unwrap();
        let error = baseline.verify(&ctx, Some(&receipt_id)).unwrap_err();

        assert!(error.to_string().contains("durable receipt schema"));
    }

    #[test]
    fn oversized_worker_append_is_rejected_before_record_allocation() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();

        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&vec![b'x'; MAX_VERIFIED_RECEIPT_APPEND_BYTES as usize + 1])
            .unwrap();
        let error = baseline.verify(&ctx, None).unwrap_err();

        assert!(error.to_string().contains("byte verification limit"));
    }

    #[test]
    fn oversized_receipt_baseline_requires_archival_before_worker_start() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .unwrap();
        file.set_len(MAX_VERIFIED_RECEIPT_BASELINE_BYTES + 1)
            .unwrap();

        let Err(error) = ReceiptJournalBaseline::capture(&ctx) else {
            panic!("an oversized active receipt journal must be rejected");
        };

        assert!(error.to_string().contains("archive old receipts"));
    }

    #[test]
    fn receipt_baseline_rejects_same_length_prefix_rewrite() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        fs::write(&path, b"{\"id\":\"old-a\"}\n").unwrap();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();

        fs::write(&path, b"{\"id\":\"old-b\"}\n{\"id\":\"receipt-worker\"}\n").unwrap();

        let error = baseline.verify(&ctx, Some("receipt-worker")).unwrap_err();
        assert!(error.to_string().contains("prefix was rewritten"));
    }

    #[test]
    fn directly_copied_schema_valid_receipt_is_rejected() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        append_runtime_receipt(&ctx).unwrap();
        let forged_record = fs::read(&path).unwrap();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&forged_record)
            .unwrap();
        let worker_receipt = append_runtime_receipt(&ctx).unwrap();

        let error = baseline.verify(&ctx, Some(&worker_receipt)).unwrap_err();

        assert!(
            error.to_string().contains("only appended record"),
            "{error:#}"
        );
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn unknown_final_repository_head_requires_attention_and_a_dispatch_stop() {
        use std::os::unix::fs::PermissionsExt as _;

        let _env_lock = lock_env();
        let (temp, ctx, _path) = fixture();
        let receipt_journal = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let git = temp.path().join("git-final-head-fails.sh");
        fs::write(
            &git,
            r#"#!/bin/sh
case " $* " in
  *" status --porcelain=v1 "*) exit 0 ;;
  *" ls-files --stage "*) exit 0 ;;
  *" rev-parse HEAD "*) exit 7 ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&git).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&git, permissions).unwrap();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
        let checkout = PreparedCheckout::Repo {
            path: temp.path().to_path_buf(),
            initial_head: "initial-head".into(),
            receipt_journal,
        };

        let completion = checkout.finish(TaskOutcome::Succeeded, &ctx, None);

        assert_eq!(
            completion.report.repository_revision_state(),
            RepositoryRevisionState::Unknown
        );
        assert!(completion.report.repository_requires_attention());
        assert!(
            completion
                .error
                .as_deref()
                .is_some_and(|error| error.contains("checkout HEAD"))
        );
        assert!(
            completion
                .report
                .repository_revision_state()
                .requires_dispatch_stop()
        );
    }

    #[test]
    fn absent_receipt_journal_remains_valid_without_a_worker_receipt() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _path) = fixture();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();

        baseline.verify(&ctx, None).unwrap();
    }

    #[test]
    fn legacy_receipt_free_review_validation_preserves_parent_only_success() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (temp, _, path) = fixture();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .contract_version(2)
            .config("rust_test_command = \"printf 'focused validation passed\\n'\"")
            .required_commands(["rust_test_command"])
            .tool(json!({"name": "jig.test", "kind": "command", "command": "rust_test_command", "description": "Test"}))
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let before = fs::read(&path).unwrap_or_default();
        let result = crate::runtime::dispatch(&ctx, crate::command::RuntimeCommand::Check(
            crate::command::CheckCommand::Test(crate::command::ToolRequest::new(None, false)),
        )).unwrap();
        assert_eq!(result["ok"], true, "{result:#}");
        assert!(result["receipt_id"].is_null());
        assert_eq!(fs::read(&path).unwrap_or_default(), before);
        assert!(!ctx.state_file("runs.jsonl").exists());
        let id = append_runtime_receipt(&ctx).unwrap();
        baseline.verify(&ctx, Some(&id)).unwrap();
    }

    #[test]
    fn staged_receipt_append_remains_unverifiable_not_owned() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _) = fixture();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let id = append_runtime_receipt(&ctx).unwrap();
        git_stdout(&ctx, ctx.root(), ["add", "--", WORKER_RECEIPT_PATH], &mut NoopExecutionObserver).unwrap();
        let result = baseline.verify(&ctx, Some(&id));
        assert!(result.as_ref().unwrap_err().to_string().contains("Git index entry changed"));
        let diagnostic = serde_json::to_value(CheckoutDiagnostics::inspect(
            &ctx, ctx.root(), &Ok(false), &Ok("example-head".into()), &result, Some(&id),
        )).unwrap();
        assert_eq!(diagnostic["reasons"], json!(["journal_unverifiable"]));
        assert_eq!(diagnostic["receipt_ids_incomplete"], true);
    }

    #[test]
    fn operational_state_is_reported_but_never_exempted_from_dirty_status() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _) = fixture();
        fs::write(ctx.root().join(".gitignore"), ".agent/.cache/\n").unwrap();
        git_stdout(&ctx, ctx.root(), ["add", "."], &mut NoopExecutionObserver).unwrap();
        git_stdout(&ctx, ctx.root(), ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.com", "commit", "-m", "fixture"], &mut NoopExecutionObserver).unwrap();
        let initial_head = git_stdout(&ctx, ctx.root(), ["rev-parse", "HEAD"], &mut NoopExecutionObserver).unwrap();
        let receipt_journal = ReceiptJournalBaseline::capture(&ctx).unwrap();
        fs::write(ctx.state_file("runs.jsonl"), "example operational change\n").unwrap();
        let id = append_runtime_receipt(&ctx).unwrap();
        let result = PreparedCheckout::Repo {
            path: ctx.root().into(), initial_head, receipt_journal,
        }.finish(TaskOutcome::Succeeded, &ctx, Some(&id));
        assert!(result.report.repository_requires_attention());
        let value = result.report.value();
        assert_eq!(value["dirty"], true);
        assert_eq!(value["receipt_append_valid"], true);
        assert_eq!(value["diagnostics"]["reasons"], json!(["operational_state_changes"]), "{value:#}");
        assert_eq!(value["diagnostics"]["observed_paths"], json!([".agent/state/runs.jsonl"]));
    }

    #[test]
    fn isolated_task_retains_validation_receipts_without_dirtying_shared_checkout() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _) = fixture();
        git_stdout(&ctx, ctx.root(), ["add", "."], &mut NoopExecutionObserver).unwrap();
        git_stdout(&ctx, ctx.root(), ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.com", "commit", "-m", "fixture"], &mut NoopExecutionObserver).unwrap();
        let initial_head = git_stdout(&ctx, ctx.root(), ["rev-parse", "HEAD"], &mut NoopExecutionObserver).unwrap();
        let isolated = tempdir().unwrap();
        let path = isolated.path().join("example-task");
        git_stdout(&ctx, ctx.root(), ["worktree", "add", "--detach", path.to_str().unwrap(), "HEAD"], &mut NoopExecutionObserver).unwrap();
        let task_ctx = RepoContext::load_from(&path).unwrap();
        let id = append_runtime_receipt(&task_ctx).unwrap();
        let result = PreparedCheckout::Worktree {
            repo_root: ctx.root().into(), path: path.clone(), initial_head,
        }.finish(TaskOutcome::Succeeded, &ctx, Some(&id));
        assert!(!result.report.repository_requires_attention());
        let value = result.report.value();
        assert_eq!(value["mode"], "worktree");
        assert_eq!(value["retained"], true);
        assert_eq!(value["dirty"], true);
        assert!(fs::read_to_string(path.join(WORKER_RECEIPT_PATH)).unwrap().contains(&id));
        assert!(!ctx.root().join(WORKER_RECEIPT_PATH).exists());
        assert!(!git_is_dirty(&ctx, ctx.root(), &mut NoopExecutionObserver).unwrap());
    }
}
