use super::*;
use flate2::read::GzDecoder;
use serde_json::json;
use std::fs::{self, File};
use std::io::Read;
use std::process::Command;
use tempfile::tempdir;

use crate::state::jsonl::read_jsonl;
use crate::test_env::TestRepoBuilder;
use crate::tool_defs::tool;

#[test]
fn successful_receipt_previews_are_small_but_failures_keep_diagnostics() {
    let output = "x".repeat(5_000);

    let successful = receipt_output_preview(&output, 0);
    let failed = receipt_output_preview(&output, 1);

    assert_eq!(
        successful.strip_suffix('…').unwrap().len(),
        SUCCESSFUL_RECEIPT_PREVIEW_BYTES
    );
    assert_eq!(failed.strip_suffix('…').unwrap().len(), 4_000);
}

#[test]
fn successful_receipt_preview_preserves_utf8_boundaries() {
    let output = "a".repeat(SUCCESSFUL_RECEIPT_PREVIEW_BYTES - 1) + "é-tail";

    let preview = receipt_output_preview(&output, 0);

    assert!(preview.ends_with('…'));
    assert_eq!(
        preview.strip_suffix('…').unwrap(),
        "a".repeat(SUCCESSFUL_RECEIPT_PREVIEW_BYTES - 1)
    );
}

#[test]
fn receipt_gzip_export_preserves_selected_raw_records_and_unknown_fields() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("receipts.jsonl");
    let destination = temp.path().join("export/receipts.jsonl.gz");
    let second_destination = temp.path().join("export/receipts-copy.jsonl.gz");
    let old = raw_receipt("receipt_old", 10, r#","future":{"nested":true}"#);
    let new = raw_receipt("receipt_new", 100, "");
    let source_bytes = format!("{old}\n{new}\n");
    fs::write(&source, &source_bytes).unwrap();

    let artifact =
        write_receipt_gzip(&source, &destination, |receipt| receipt.ended_at_ms < 50).unwrap();
    let second_artifact = write_receipt_gzip(&source, &second_destination, |receipt| {
        receipt.ended_at_ms < 50
    })
    .unwrap();

    assert_eq!(artifact.receipt_count, 1);
    assert_eq!(artifact.uncompressed_bytes, (old.len() + 1) as u64);
    assert_eq!(fs::read_to_string(&source).unwrap(), source_bytes);
    let mut decoded = String::new();
    GzDecoder::new(File::open(&destination).unwrap())
        .read_to_string(&mut decoded)
        .unwrap();
    assert_eq!(decoded, format!("{old}\n"));
    assert_eq!(
        artifact.sha256,
        sha256_reader(File::open(&destination).unwrap()).unwrap()
    );
    assert_eq!(
        artifact.content_sha256,
        sha256_reader(std::io::Cursor::new(decoded.as_bytes())).unwrap()
    );
    assert_eq!(artifact.sha256, second_artifact.sha256);
    assert_eq!(
        fs::read(destination).unwrap(),
        fs::read(second_destination).unwrap()
    );
}

#[test]
fn receipt_gzip_export_refuses_to_replace_an_existing_output() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("receipts.jsonl");
    let destination = temp.path().join("receipts.jsonl.gz");
    fs::write(&source, format!("{}\n", raw_receipt("receipt_old", 10, ""))).unwrap();
    fs::write(&destination, "keep me").unwrap();

    let error = write_receipt_gzip(&source, &destination, |_| true)
        .unwrap_err()
        .to_string();

    assert!(error.contains("Refusing to replace existing receipt export"));
    assert_eq!(fs::read_to_string(destination).unwrap(), "keep me");
}

#[test]
fn recorded_receipt_persists_bounded_change_set_metadata() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "fixture"]);
    fs::create_dir_all(temp.path().join("changed")).unwrap();
    for index in 0..105 {
        fs::write(
            temp.path().join(format!("changed/file-{index:03}.txt")),
            "changed\n",
        )
        .unwrap();
    }
    fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    fs::write(
        temp.path().join(".agent/state/metadata-noise.jsonl"),
        "noise\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    record_receipt(
        &ctx,
        ReceiptInput {
            tool_name: tool::TEST,
            args: json!({}),
            invoked_command_key: None,
            started_at_ms: 1,
            ended_at_ms: 2,
            exit_status: 0,
            stdout: &"success output ".repeat(100),
            stderr: "",
            evidence: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: false,
        },
    )
    .unwrap();

    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    let receipt = receipts.last().unwrap();
    assert_eq!(receipt.changed_paths.len(), 100);
    assert_eq!(receipt.changed_path_count, Some(105));
    assert!(receipt.changed_paths_truncated);
    assert!(
        receipt
            .changed_paths_digest
            .as_deref()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    assert!(
        receipt
            .changed_paths
            .iter()
            .all(|path| !path.starts_with(".agent/"))
    );
    assert_eq!(
        receipt.stdout_preview.strip_suffix('…').unwrap().len(),
        SUCCESSFUL_RECEIPT_PREVIEW_BYTES
    );
}

#[test]
fn cancelled_git_enrichment_does_not_prevent_durable_receipt_append() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let receipt_id = record_receipt_with_cancellation(
        &ctx,
        ReceiptInput {
            tool_name: tool::TEST,
            args: json!({}),
            invoked_command_key: None,
            started_at_ms: 1,
            ended_at_ms: 2,
            exit_status: 1,
            stdout: "",
            stderr: "cancelled",
            evidence: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
        },
        &|| true,
    )
    .unwrap();

    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    let receipt = receipts.last().unwrap();
    assert_eq!(receipt.id, receipt_id);
    assert_eq!(receipt.exit_status, 1);
    assert!(
        receipt
            .git_status_error
            .as_deref()
            .is_some_and(|error| error.contains("collection was cancelled"))
    );
    assert!(
        receipt
            .worktree_fingerprint_error
            .as_deref()
            .is_some_and(|error| error.contains("collection was cancelled"))
    );
}

fn raw_receipt(id: &str, ended_at_ms: u64, extra: &str) -> String {
    format!(
        r#"{{"id":"{id}","session_id":null,"plan_id":null,"tool_name":"jig.test","args":{{}},"started_at_ms":0,"ended_at_ms":{ended_at_ms},"exit_status":0,"stdout_preview":"","stderr_preview":"","changed_paths":[],"diff_stat":{{"files":0,"insertions":0,"deletions":0}}{extra}}}"#
    )
}

#[test]
fn legacy_check_receipt_fields_are_ignored_on_read() {
    let receipt: ReceiptRecord = serde_json::from_str(&raw_receipt(
        "legacy",
        42,
        r#","run_id":"run_legacy","target":"api:test","config_digest":"sha256:a","input_digest":"sha256:b","findings":[],"finding_count":0,"findings_truncated":false,"evaluated_at_ms":1,"valid_until_ms":2,"target_freshness":{"status":"complete"}"#,
    ))
    .unwrap();
    assert_eq!(receipt.id, "legacy");
    assert_eq!(receipt.run_id.as_deref(), Some("run_legacy"));
    assert_eq!(receipt.ended_at_ms, 42);
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

mod journal_contention;
