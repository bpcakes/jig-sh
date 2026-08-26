use super::*;

#[test]
fn receipts_list_is_read_only() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();
    let before = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();

    let output = receipts_list(&ctx, receipt_list_filter()).unwrap();

    let after = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    assert_eq!(before.len(), after.len());
    assert!(output.get("receipt_id").is_none());
}

#[test]
fn receipts_list_filters_by_tool_and_failure_and_adds_diff_summary() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    append_jsonl(
        &ctx.state_file("receipts.jsonl"),
        &receipt_record(
            "receipt_failed",
            tool::TEST,
            1,
            DiffStat {
                files: 1,
                insertions: 2,
                deletions: 3,
            },
        ),
    )
    .unwrap();
    append_jsonl(
        &ctx.state_file("receipts.jsonl"),
        &receipt_record("receipt_success", tool::CLIPPY, 0, DiffStat::default()),
    )
    .unwrap();

    let output = receipts_list(
        &ctx,
        ReceiptListFilter {
            tool_name: Some(tool::TEST.into()),
            failed_only: true,
            ..receipt_list_filter()
        },
    )
    .unwrap();
    let receipts = output["receipts"].as_array().unwrap();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["id"], "receipt_failed");
    assert_eq!(receipts[0]["diff_summary"], "1 file, +2 -3");
}

fn receipt_list_filter() -> ReceiptListFilter {
    ReceiptListFilter {
        session_id: None,
        plan_id: None,
        tool_name: None,
        failed_only: false,
        limit: 20,
    }
}

pub(super) fn receipt_record(
    id: &str,
    tool_name: &str,
    exit_status: i32,
    diff_stat: DiffStat,
) -> ReceiptRecord {
    ReceiptRecord {
        id: id.into(),
        session_id: Some("session_1".into()),
        plan_id: Some("plan_1".into()),
        tool_name: tool_name.into(),
        args: json!({}),
        invoked_command_key: None,
        started_at_ms: 1,
        ended_at_ms: 2,
        exit_status,
        stdout_preview: String::new(),
        stderr_preview: String::new(),
        evidence: None,
        run_id: None,
        target: None,
        config_digest: None,
        input_digest: None,
        findings: Vec::new(),
        changed_paths: Vec::new(),
        changed_path_count: None,
        changed_paths_truncated: false,
        changed_paths_digest: None,
        diff_stat,
        git_status_error: None,
        git_diff_stat_error: None,
        worktree_fingerprint: None,
        worktree_fingerprint_error: None,
    }
}

#[test]
fn state_summary_is_read_only_and_counts_state_records() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();
    let before = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();

    let output = state_summary(&ctx).unwrap();

    let after = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    assert_eq!(before.len(), after.len());
    assert!(output.get("receipt_id").is_none());
    assert_eq!(output["ok"], true);
    assert_eq!(output["counts"]["sessions"], 1);
    assert_eq!(output["counts"]["plans"], 0);
    assert_eq!(output["counts"]["receipts"], 1);
    assert_eq!(output["counts"]["failed_receipts"], 0);
    assert_eq!(
        output["recent_receipts"][0]["tool_name"],
        tool::SESSION_START
    );
}

#[test]
fn cancellable_state_summary_stops_during_stream_collection() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let plans_path = ctx.state_file("plans.jsonl");
    fs::write(&plans_path, b"").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&plans_path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let reader_ctx = ctx;
    let cancelled = Arc::new(AtomicBool::new(false));
    let reader_cancelled = Arc::clone(&cancelled);
    let (started_tx, started_rx) = mpsc::channel();
    let (summary_tx, summary_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = super::sessions::state_summary_with_cancellation(&reader_ctx, &|| {
            reader_cancelled.load(Ordering::SeqCst)
        });
        summary_tx.send(result).unwrap();
    });

    started_rx.recv().unwrap();
    assert!(summary_rx.recv_timeout(Duration::from_millis(100)).is_err());
    cancelled.store(true, Ordering::SeqCst);
    let result = match summary_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(error) => {
            FileExt::unlock(&lock).unwrap();
            reader.join().unwrap();
            panic!("state summary stayed blocked on a state stream: {error}");
        }
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        "status collection was cancelled"
    );
    FileExt::unlock(&lock).unwrap();
    reader.join().unwrap();
}

#[test]
fn state_summary_on_uninitialized_repo_creates_nothing() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert!(!ctx.state_dir().exists());
    assert!(!temp.path().join(".agent/.cache").exists());

    let output = state_summary(&ctx).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["counts"]["sessions"], 0);
    assert_eq!(output["counts"]["receipts"], 0);
    assert!(!ctx.state_dir().exists());
    assert!(!temp.path().join(".agent/.cache").exists());
    assert!(!temp.path().join(".agent/plans").exists());
}

#[cfg(unix)]
#[test]
fn state_summary_reads_existing_read_only_state() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    session_start(&ctx).unwrap();
    let state_dir = ctx.state_dir();
    let cache_dir = temp.path().join(".agent/.cache");
    let lock_dir = cache_dir.join("state-locks");
    for path in [
        ctx.state_file("sessions.jsonl"),
        ctx.state_file("receipts.jsonl"),
        ctx.current_session_path(),
        lock_dir.join("sessions.jsonl.lock"),
        lock_dir.join("receipts.jsonl.lock"),
    ] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o444)).unwrap();
    }
    for path in [&state_dir, &lock_dir, &cache_dir] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    }

    let output = state_summary(&ctx).unwrap();

    assert_eq!(output["counts"]["sessions"], 1);
    assert_eq!(output["counts"]["receipts"], 1);
    for path in [&cache_dir, &lock_dir, &state_dir] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn receipts_archive_moves_old_unprotected_receipts() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let mut old_receipt = receipt_record("receipt_old", tool::CLIPPY, 0, DiffStat::default());
    old_receipt.ended_at_ms = 10;
    let mut new_receipt = receipt_record("receipt_new", tool::CLIPPY, 0, DiffStat::default());
    new_receipt.ended_at_ms = 2_000;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_receipt).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &new_receipt).unwrap();
    let original = fs::read(ctx.state_file("receipts.jsonl")).unwrap();

    let output = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(output["receipts_archived"], 1);
    assert_eq!(output["receipts_retained"], 1);
    let retained = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].id, "receipt_new");
    let archive_path = output["archive_path"].as_str().unwrap();
    let archived = read_gzip_receipts(Path::new(archive_path));
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, "receipt_old");
    assert!(
        archive_path.contains(".agent/.cache/state-archives/"),
        "{archive_path}"
    );
    let recovery_path = Path::new(output["recovery_backup_path"].as_str().unwrap()).to_path_buf();
    let restored = restore_backup(
        &ctx,
        StateRestoreRequest {
            backup: recovery_path.clone(),
        },
    )
    .unwrap();
    assert_eq!(restored["stream"], "receipts");
    assert_eq!(restored["changed"], true);
    assert_eq!(
        fs::read(ctx.state_file("receipts.jsonl")).unwrap(),
        original
    );
    fs::remove_file(ctx.state_file("receipts.jsonl")).unwrap();
    let restored_missing = restore_backup(
        &ctx,
        StateRestoreRequest {
            backup: recovery_path,
        },
    )
    .unwrap();
    assert_eq!(restored_missing["changed"], true);
    assert!(restored_missing["recovery_backup_path"].is_null());
    assert_eq!(
        fs::read(ctx.state_file("receipts.jsonl")).unwrap(),
        original
    );
    assert!(!ctx.state_dir().join("archive").exists());
}

#[test]
fn receipts_archive_preserves_latest_gate_evidence_and_supporting_receipts() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[commands]
rust_test_command = "true"

[[work.gates]]
id = "tests"
kind = "check"
tool = "jig.test"

[[work.gates]]
id = "rust-review"
kind = "codex_review"
skill = "rust-review"
"#,
        )
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": tool::TEST,
            "kind": "command",
            "description": "Run tests.",
            "command": "rust_test_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    seed_open_plan_for_test(&ctx, "plan_1", "Open plan", "# Plan\n").unwrap();
    seed_open_plan_for_test(&ctx, "plan_2", "Closed plan", "# Plan\n").unwrap();
    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::close(
            "plan-event-close-2".into(),
            "plan_2".into(),
            1,
            Some("done".into()),
        ),
    )
    .unwrap();
    ensure_state_layout(&ctx).unwrap();
    let mut old_direct = receipt_record("receipt_direct", tool::TEST, 0, DiffStat::default());
    old_direct.ended_at_ms = 10;
    let mut old_batch = receipt_record("receipt_batch", tool::WORK_CHECK, 0, DiffStat::default());
    old_batch.ended_at_ms = 20;
    old_batch.args = json!({
        "plan_id": "plan_1",
        "tools": [tool::TEST],
        "receipt_ids": ["receipt_direct"],
    });
    let mut old_review_worker = receipt_record(
        "receipt_review_worker",
        crate::tool_defs::WORKER_RUN_TOOL,
        0,
        DiffStat::default(),
    );
    old_review_worker.ended_at_ms = 25;
    let mut old_review =
        receipt_record("receipt_review", tool::WORK_REVIEW, 1, DiffStat::default());
    old_review.ended_at_ms = 30;
    old_review.args = json!({
        "plan_id": "plan_1",
        "gate_id": "rust-review",
    });
    old_review.evidence = Some(json!({
        "worker_receipt_id": "receipt_review_worker",
    }));
    let mut unrelated_old = receipt_record(
        "receipt_unrelated_old",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    unrelated_old.plan_id = Some("plan_2".into());
    unrelated_old.ended_at_ms = 40;
    let mut unrelated_new = receipt_record(
        "receipt_unrelated_new",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    unrelated_new.plan_id = Some("plan_2".into());
    unrelated_new.ended_at_ms = 2_000;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_direct).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_batch).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_review_worker).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_review).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &unrelated_old).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &unrelated_new).unwrap();
    let original = fs::read(ctx.state_file("receipts.jsonl")).unwrap();

    let output = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(output["receipts_archived"], 1);
    assert_eq!(output["protected_receipts_retained"], 4);
    let retained = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
        .unwrap()
        .into_iter()
        .map(|receipt| receipt.id)
        .collect::<Vec<_>>();
    assert!(retained.contains(&"receipt_direct".into()));
    assert!(retained.contains(&"receipt_batch".into()));
    assert!(retained.contains(&"receipt_review_worker".into()));
    assert!(retained.contains(&"receipt_review".into()));
    assert!(!retained.contains(&"receipt_unrelated_old".into()));
    assert!(retained.contains(&"receipt_unrelated_new".into()));
    let recovery = Path::new(output["recovery_backup_path"].as_str().unwrap()).to_path_buf();
    restore_backup(&ctx, StateRestoreRequest { backup: recovery }).unwrap();
    assert_eq!(
        fs::read(ctx.state_file("receipts.jsonl")).unwrap(),
        original,
        "receipt recovery must restore the exact interleaved physical stream"
    );
}

#[test]
fn receipts_archive_dry_run_does_not_rewrite_state() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let mut old_receipt = receipt_record("receipt_old", tool::CLIPPY, 0, DiffStat::default());
    old_receipt.ended_at_ms = 10;
    let mut new_receipt = receipt_record("receipt_new", tool::CLIPPY, 0, DiffStat::default());
    new_receipt.ended_at_ms = 2_000;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_receipt).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &new_receipt).unwrap();
    let before = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();

    let output = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: true,
        },
    )
    .unwrap();

    assert_eq!(output["receipts_archived"], 1);
    assert_eq!(output["protected_receipts_retained"], 0);
    assert!(output["archive_path"].is_null());
    assert_eq!(
        fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap(),
        before
    );
    assert!(!ctx.state_dir().join("archive").exists());
}

#[test]
fn receipts_export_writes_exact_gzip_without_mutating_active_state() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let mut old_receipt = receipt_record("receipt_old", tool::CLIPPY, 0, DiffStat::default());
    old_receipt.ended_at_ms = 10;
    let mut new_receipt = receipt_record("receipt_new", tool::CLIPPY, 0, DiffStat::default());
    new_receipt.ended_at_ms = 2_000;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_receipt).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &new_receipt).unwrap();
    let before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
    let output_path = temp.path().join("exports/old-receipts.jsonl.gz");

    let output = receipts_export(&ctx, "1000", &output_path).unwrap();

    assert_eq!(output["receipts_exported"], 1);
    assert_eq!(output["output_path"], output_path.display().to_string());
    assert!(
        output["sha256"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    assert_eq!(fs::read(ctx.state_file("receipts.jsonl")).unwrap(), before);
    let exported = read_gzip_receipts(&output_path);
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].id, "receipt_old");
}

fn read_gzip_receipts(path: &Path) -> Vec<ReceiptRecord> {
    let mut contents = String::new();
    GzDecoder::new(fs::File::open(path).unwrap())
        .read_to_string(&mut contents)
        .unwrap();
    contents
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
