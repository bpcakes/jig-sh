use super::*;
use flate2::read::GzDecoder;
use serde_json::json;
use std::fs::{self, File};
use std::io::Read;
use std::process::Command;
use tempfile::tempdir;

use crate::state::jsonl::read_jsonl;
use crate::test_env::TestRepoBuilder;

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
fn receipt_protection_is_limited_to_open_configured_gate_evidence() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let check_gate_tools = BTreeSet::from([tool::TEST.to_string()]);
    let review_gate_ids = BTreeSet::from(["rust-review".to_string()]);
    let evidence_targets = BTreeMap::from([(
        "api-tests".to_string(),
        BTreeSet::from(["api:test".parse().unwrap()]),
    )]);
    let mut index = ReceiptProtectionIndex::with_evidence(&open_plan_ids, &evidence_targets);
    let mut target = test_receipt(
        "receipt_target",
        "plan_open",
        "jig.target_run",
        35,
        json!({}),
    );
    target.run_id = Some("run_1".into());
    target.target = Some("api:test".parse().unwrap());
    let mut closed_target = test_receipt(
        "receipt_closed_target",
        "plan_closed",
        "jig.target_run",
        45,
        json!({}),
    );
    closed_target.run_id = Some("run_2".into());
    closed_target.target = Some("api:test".parse().unwrap());
    let receipts = [
        test_receipt("receipt_direct", "plan_open", tool::TEST, 10, json!({})),
        test_receipt(
            "receipt_batch",
            "plan_open",
            tool::WORK_CHECK,
            20,
            json!({
                "tools": [tool::TEST],
                "receipt_ids": ["receipt_direct"],
            }),
        ),
        test_receipt(
            "receipt_review",
            "plan_open",
            tool::WORK_REVIEW,
            30,
            json!({"gate_id": "rust-review"}),
        ),
        test_receipt("receipt_non_gate", "plan_open", tool::CLIPPY, 40, json!({})),
        target,
        closed_target,
        test_receipt("receipt_closed", "plan_closed", tool::TEST, 50, json!({})),
    ];
    for receipt in &receipts {
        index.observe(receipt, &open_plan_ids, &check_gate_tools, &review_gate_ids);
    }

    let protected = index.protected_receipt_ids().unwrap();

    assert_eq!(
        protected,
        BTreeSet::from([
            "receipt_batch".to_string(),
            "receipt_direct".to_string(),
            "receipt_review".to_string(),
            "receipt_target".to_string(),
        ])
    );
}

#[test]
fn receipt_archive_protection_keeps_only_the_selected_evidence_group_after_overflow() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let evidence_targets = BTreeMap::from([(
        "verify".to_string(),
        BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]),
    )]);
    let mut index = ReceiptProtectionIndex::with_evidence(&open_plan_ids, &evidence_targets);

    for sequence in 0..=1024 {
        let mut partial = test_receipt(
            &format!("receipt_partial_{sequence}"),
            "plan_open",
            "jig.target_run",
            sequence,
            json!({}),
        );
        partial.run_id = Some(format!("run_partial_{sequence}"));
        partial.target = Some("api:lint".parse().unwrap());
        index.observe(&partial, &open_plan_ids, &BTreeSet::new(), &BTreeSet::new());
    }
    for (receipt_id, target) in [
        ("receipt_complete_lint", "api:lint"),
        ("receipt_complete_test", "api:test"),
    ] {
        let mut complete =
            test_receipt(receipt_id, "plan_open", "jig.target_run", 2_000, json!({}));
        complete.run_id = Some("run_complete".into());
        complete.target = Some(target.parse().unwrap());
        index.observe(
            &complete,
            &open_plan_ids,
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
    }

    assert_eq!(
        index.protected_receipt_ids().unwrap(),
        BTreeSet::from([
            "receipt_complete_lint".to_string(),
            "receipt_complete_test".to_string(),
        ])
    );
}

#[test]
fn receipt_archive_refuses_to_drop_protection_when_evidence_indexing_overflows() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let evidence_targets = BTreeMap::from([(
        "verify".to_string(),
        BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]),
    )]);
    let mut index = ReceiptProtectionIndex::with_evidence(&open_plan_ids, &evidence_targets);

    for sequence in 0..=16 * 1024 {
        let mut partial = test_receipt(
            &format!("receipt_partial_{sequence}"),
            "plan_open",
            "jig.target_run",
            sequence,
            json!({}),
        );
        partial.run_id = Some(format!("run_partial_{sequence}"));
        partial.target = Some("api:lint".parse().unwrap());
        index.observe(&partial, &open_plan_ids, &BTreeSet::new(), &BTreeSet::new());
    }

    let error = index.protected_receipt_ids().unwrap_err().to_string();

    assert!(
        error.contains("cannot safely archive target evidence"),
        "{error}"
    );
    assert!(error.contains("incomplete run groups"), "{error}");
}

#[test]
fn receipt_protection_matches_successful_legacy_batch_lookup() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let check_gate_tools = BTreeSet::from([tool::TEST.to_string()]);
    let review_gate_ids = BTreeSet::new();
    let direct = test_receipt("receipt_direct", "plan_open", tool::TEST, 10, json!({}));
    let successful_legacy = test_receipt(
        "receipt_legacy_success",
        "plan_open",
        tool::WORK_CHECK,
        20,
        json!({"tools": [tool::TEST]}),
    );
    let mut failed_legacy = test_receipt(
        "receipt_legacy_failed",
        "plan_open",
        tool::WORK_CHECK,
        30,
        json!({"tools": [tool::TEST]}),
    );
    failed_legacy.exit_status = 1;
    let unrelated_exact_schema = test_receipt(
        "receipt_exact_other",
        "plan_open",
        tool::WORK_CHECK,
        40,
        json!({"tools": [tool::TEST], "receipt_ids": []}),
    );
    let mut index = ReceiptProtectionIndex::default();
    for receipt in [
        direct,
        successful_legacy,
        failed_legacy,
        unrelated_exact_schema,
    ] {
        index.observe(
            &receipt,
            &open_plan_ids,
            &check_gate_tools,
            &review_gate_ids,
        );
    }

    let protected = index.protected_receipt_ids().unwrap();

    assert_eq!(
        protected,
        BTreeSet::from([
            "receipt_direct".to_string(),
            "receipt_legacy_success".to_string(),
        ])
    );
}

#[test]
fn newest_review_protects_its_worker_receipt_by_physical_order() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let check_gate_tools = BTreeSet::new();
    let review_gate_ids = BTreeSet::from(["rust-review".to_string()]);
    let old_worker = test_receipt(
        "receipt_worker_old",
        "plan_open",
        crate::tool_defs::WORKER_RUN_TOOL,
        400,
        json!({}),
    );
    let mut old_review = test_receipt(
        "receipt_review_old",
        "plan_open",
        tool::WORK_REVIEW,
        500,
        json!({"gate_id": "rust-review"}),
    );
    old_review.evidence = Some(json!({"worker_receipt_id": "receipt_worker_old"}));
    let latest_worker = test_receipt(
        "receipt_worker_latest",
        "plan_open",
        crate::tool_defs::WORKER_RUN_TOOL,
        200,
        json!({}),
    );
    let mut latest_review = test_receipt(
        "receipt_review_latest",
        "plan_open",
        tool::WORK_REVIEW,
        100,
        json!({"gate_id": "rust-review"}),
    );
    latest_review.exit_status = 1;
    latest_review.evidence = Some(json!({"worker_receipt_id": "receipt_worker_latest"}));
    let mut index = ReceiptProtectionIndex::default();
    for receipt in [old_worker, old_review, latest_worker, latest_review] {
        index.observe(
            &receipt,
            &open_plan_ids,
            &check_gate_tools,
            &review_gate_ids,
        );
    }

    let protected = index.protected_receipt_ids().unwrap();

    assert_eq!(
        protected,
        BTreeSet::from([
            "receipt_review_latest".to_string(),
            "receipt_worker_latest".to_string(),
        ])
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
            plan_id: None,
            started_at_ms: 1,
            ended_at_ms: 2,
            exit_status: 0,
            stdout: &"success output ".repeat(100),
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
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
            plan_id: None,
            started_at_ms: 1,
            ended_at_ms: 2,
            exit_status: 1,
            stdout: "",
            stderr: "cancelled",
            evidence: None,
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
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

fn test_receipt(
    id: &str,
    plan_id: &str,
    tool_name: &str,
    ended_at_ms: u64,
    args: Value,
) -> ReceiptRecord {
    ReceiptRecord {
        id: id.to_string(),
        session_id: None,
        plan_id: Some(plan_id.to_string()),
        tool_name: tool_name.to_string(),
        args,
        invoked_command_key: None,
        started_at_ms: 0,
        ended_at_ms,
        exit_status: 0,
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
        diff_stat: crate::git_receipts::DiffStat::default(),
        git_status_error: None,
        git_diff_stat_error: None,
        worktree_fingerprint: None,
        worktree_fingerprint_error: None,
    }
}

fn raw_receipt(id: &str, ended_at_ms: u64, extra: &str) -> String {
    format!(
        r#"{{"id":"{id}","session_id":null,"plan_id":null,"tool_name":"jig.test","args":{{}},"started_at_ms":0,"ended_at_ms":{ended_at_ms},"exit_status":0,"stdout_preview":"","stderr_preview":"","changed_paths":[],"diff_stat":{{"files":0,"insertions":0,"deletions":0}}{extra}}}"#
    )
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
