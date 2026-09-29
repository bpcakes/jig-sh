use super::*;

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
fn receipts_archive_moves_old_receipts_despite_legacy_open_plans() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    // The fixture receipts link to `plan_1`, which a legacy stream still lists
    // as open; Jig no longer reads it, so the link does not retain them.
    fs::write(
        ctx.state_file("plans.jsonl"),
        "{\"id\":\"event_1\",\"plan_id\":\"plan_1\",\"event\":\"open\",\"timestamp_ms\":1}\n",
    )
    .unwrap();
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
    assert!(output.get("protected_receipts_retained").is_none());
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
