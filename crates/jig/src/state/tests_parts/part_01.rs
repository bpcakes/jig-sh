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
