use super::*;

#[test]
fn receipts_archive_rejects_malformed_before_cutoff() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "2026-02-31".into(),
            dry_run: true,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Invalid --before date"));
}

#[test]
fn receipt_archive_and_export_reject_an_unterminated_final_record_before_publication() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let mut receipt = receipt_record("receipt_torn", tool::CLIPPY, 0, DiffStat::default());
    receipt.ended_at_ms = 10;
    let source = serde_json::to_vec(&receipt).unwrap();
    fs::write(ctx.state_file("receipts.jsonl"), &source).unwrap();

    let archive_error = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap_err()
    .to_string();
    let export_path = temp.path().join("export.jsonl.gz");
    let export_error = receipts_export(&ctx, "1000", &export_path)
        .unwrap_err()
        .to_string();

    assert!(archive_error.contains("not newline-terminated"));
    assert!(export_error.contains("not newline-terminated"));
    assert_eq!(fs::read(ctx.state_file("receipts.jsonl")).unwrap(), source);
    assert!(!export_path.exists());
    assert!(!temp.path().join(".agent/.cache/state-backups").exists());
    assert!(!temp.path().join(".agent/.cache/state-archives").exists());
}

#[test]
fn state_tool_receipts_skip_git_metadata_collection() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();

    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    let receipt = receipts
        .iter()
        .find(|receipt| receipt.tool_name == tool::SESSION_START)
        .unwrap();
    assert_eq!(receipt.args["operation"], "session_start");
    assert!(receipt.changed_paths.is_empty());
    assert_eq!(receipt.diff_stat.files, 0);
    assert!(receipt.git_status_error.is_none());
    assert!(receipt.git_diff_stat_error.is_none());
}

#[test]
fn composite_archive_failure_preserves_completed_run_recovery_paths() {
    let runs = json!({
        "runs_archived": 2,
        "runs_recovery_backup_path": "/tmp/example-run-backup",
        "runs_archive_path": "/tmp/example-run-archive.jsonl.gz",
    });

    let error = decorate_receipt_archive_failure(
        anyhow::anyhow!("receipt rewrite failed"),
        Some(&runs),
        false,
    )
    .to_string();

    assert!(error.contains("receipt rewrite failed"));
    assert!(error.contains("2 run(s) were archived"));
    assert!(error.contains("/tmp/example-run-backup"));
    assert!(error.contains("/tmp/example-run-archive.jsonl.gz"));
}
