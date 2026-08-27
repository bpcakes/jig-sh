use super::*;
use crate::state::privacy::REPOSITORY_ROOT_REDACTION;
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
fn receipt_recording_redacts_the_repository_root_from_all_free_text() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let root = ctx.root().display().to_string();
    let diagnostic = format!("compiled {root}/crates/example");

    record_receipt(
        &ctx,
        ReceiptInput {
            tool_name: tool::TEST,
            args: json!({"cwd": root}),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: 1,
            ended_at_ms: 2,
            exit_status: 1,
            stdout: &diagnostic,
            stderr: &diagnostic,
            evidence: Some(json!({"diagnostic": diagnostic.clone()})),
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: Some(Err(format!(
                "failed beneath {}/.git",
                ctx.root().display()
            ))),
        },
    )
    .unwrap();

    let state = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();
    assert!(
        !state.contains(&ctx.root().display().to_string()),
        "{state}"
    );
    assert_eq!(state.matches(REPOSITORY_ROOT_REDACTION).count(), 5);
    let receipt = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(receipt.args["cwd"], REPOSITORY_ROOT_REDACTION);
    assert!(
        receipt
            .worktree_fingerprint_error
            .as_deref()
            .unwrap()
            .starts_with("failed beneath <repository-root>/")
    );
}

#[test]
fn work_check_batch_serializes_plan_changes_once_for_many_gates() {
    let changed_paths = (0..100)
        .map(|index| format!("src/file-{index:03}.rs"))
        .collect::<Vec<_>>();
    let gates = (0..100)
        .map(|index| WorkCheckGateEvidence {
            gate_id: format!("gate-{index:03}"),
            tool: "jig.test".into(),
            status: "not_applicable".into(),
            applicability: "not_applicable".into(),
            required: true,
            paths: Some(vec![format!("scope-{index:03}/**")]),
            paths_ignore: Vec::new(),
            reuse: false,
            forced: false,
            gate_signature: format!("signature-{index:03}"),
            baseline_oid: Some("baseline".into()),
            reason: "no paths matched".into(),
            changed_paths: Vec::new(),
            changed_path_count: 0,
            changed_paths_truncated: false,
            changed_paths_digest: None,
            matching_paths: Vec::new(),
            matching_path_count: 0,
            matching_paths_truncated: false,
            matching_paths_digest: Some("matching-digest".into()),
            scope_fingerprint: Some(format!("scope-{index:03}")),
            scope_error: None,
            tool_receipt_id: None,
            exit_status: None,
            source_plan_id: None,
            source_batch_receipt_id: None,
            source_tool_receipt_id: None,
        })
        .collect();
    let evidence = WorkCheckBatchEvidence {
        schema: WORK_CHECK_EVIDENCE_SCHEMA.into(),
        changed_paths: changed_paths.clone(),
        changed_path_count: changed_paths.len(),
        changed_paths_truncated: false,
        changed_paths_digest: Some("all-digest".into()),
        gates,
    };

    let encoded = serde_json::to_string(&evidence).unwrap();
    assert_eq!(encoded.matches("\"changed_paths\"").count(), 1);
    assert_eq!(encoded.matches("src/file-099.rs").count(), 1);
    assert!(
        encoded.len() < 50_000,
        "many-gate evidence was {} bytes",
        encoded.len()
    );

    let decoded = serde_json::from_str::<WorkCheckBatchEvidence>(&encoded)
        .unwrap()
        .into_hydrated_gates();
    assert_eq!(decoded.len(), 100);
    assert!(decoded.iter().all(|gate| gate.changed_path_count == 100));
    assert!(
        decoded
            .iter()
            .all(|gate| gate.changed_paths_digest.as_deref() == Some("all-digest"))
    );
}

#[test]
fn receipt_protection_is_limited_to_open_configured_gate_evidence() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let check_gate_tools = BTreeSet::from([tool::TEST.to_string()]);
    let check_gate_ids = BTreeSet::from(["tests".to_string()]);
    let review_gate_ids = BTreeSet::from(["rust-review".to_string()]);
    let mut index = ReceiptProtectionIndex::default();
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
        test_receipt("receipt_closed", "plan_closed", tool::TEST, 50, json!({})),
    ];
    for receipt in &receipts {
        index.observe(
            receipt,
            &open_plan_ids,
            &check_gate_tools,
            &check_gate_ids,
            &review_gate_ids,
        );
    }

    let protected = index.protected_receipt_ids();

    assert_eq!(
        protected,
        BTreeSet::from([
            "receipt_batch".to_string(),
            "receipt_direct".to_string(),
            "receipt_review".to_string(),
        ])
    );
}

#[test]
fn receipt_protection_matches_successful_legacy_batch_lookup() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let check_gate_tools = BTreeSet::from([tool::TEST.to_string()]);
    let check_gate_ids = BTreeSet::from(["tests".to_string()]);
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
            &check_gate_ids,
            &review_gate_ids,
        );
    }

    let protected = index.protected_receipt_ids();

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
    let check_gate_ids = BTreeSet::new();
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
            &check_gate_ids,
            &review_gate_ids,
        );
    }

    let protected = index.protected_receipt_ids();

    assert_eq!(
        protected,
        BTreeSet::from([
            "receipt_review_latest".to_string(),
            "receipt_worker_latest".to_string(),
        ])
    );
}

#[test]
fn newest_v2_gate_evidence_protects_not_applicable_and_reuse_sources() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let check_gate_tools = BTreeSet::from([tool::TEST.to_string()]);
    let check_gate_ids = BTreeSet::from(["sqlx".to_string(), "tests".to_string()]);
    let review_gate_ids = BTreeSet::new();
    let mut not_applicable = test_receipt(
        "receipt_not_applicable",
        "plan_open",
        tool::WORK_CHECK,
        10,
        json!({"plan_id": "plan_open"}),
    );
    not_applicable.evidence = Some(work_check_evidence(json!({
        "gate_id": "sqlx",
        "tool": tool::TEST,
        "status": "not_applicable",
        "applicability": "not_applicable"
    })));
    let mut reused = test_receipt(
        "receipt_reused",
        "plan_open",
        tool::WORK_CHECK,
        20,
        json!({"plan_id": "plan_open"}),
    );
    reused.evidence = Some(work_check_evidence(json!({
        "gate_id": "tests",
        "tool": tool::TEST,
        "status": "reused",
        "applicability": "applicable",
        "source_plan_id": "plan_closed",
        "source_batch_receipt_id": "receipt_source_batch",
        "source_tool_receipt_id": "receipt_source_tool"
    })));
    let mut index = ReceiptProtectionIndex::default();
    for receipt in [not_applicable, reused] {
        index.observe(
            &receipt,
            &open_plan_ids,
            &check_gate_tools,
            &check_gate_ids,
            &review_gate_ids,
        );
    }

    assert_eq!(
        index.protected_receipt_ids(),
        BTreeSet::from([
            "receipt_not_applicable".to_string(),
            "receipt_reused".to_string(),
            "receipt_source_batch".to_string(),
            "receipt_source_tool".to_string(),
        ])
    );
}

#[test]
fn malformed_superseding_batch_is_retained_as_archive_tombstone() {
    let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
    let check_gate_tools = BTreeSet::from([tool::TEST.to_string()]);
    let check_gate_ids = BTreeSet::from(["tests".to_string()]);
    let review_gate_ids = BTreeSet::new();
    let direct = test_receipt("receipt_direct", "plan_open", tool::TEST, 10, json!({}));
    let mut passed = test_receipt(
        "receipt_passed_batch",
        "plan_open",
        tool::WORK_CHECK,
        20,
        json!({
            "gates": ["tests"],
            "tools": [tool::TEST],
            "receipt_ids": ["receipt_direct"],
        }),
    );
    passed.evidence = Some(work_check_evidence(json!({
        "gate_id": "tests",
        "tool": tool::TEST,
        "status": "executed",
        "applicability": "applicable",
        "tool_receipt_id": "receipt_direct",
        "exit_status": 0
    })));
    let mut malformed = test_receipt(
        "receipt_malformed_batch",
        "plan_open",
        tool::WORK_CHECK,
        30,
        json!({"gates": ["tests"], "tools": [tool::TEST]}),
    );
    malformed.exit_status = 1;
    malformed.evidence = Some(json!({
        "schema": WORK_CHECK_EVIDENCE_SCHEMA,
        "gates": "not-an-array"
    }));

    let receipts = [direct, passed, malformed];
    let mut index = ReceiptProtectionIndex::default();
    for receipt in &receipts {
        index.observe(
            receipt,
            &open_plan_ids,
            &check_gate_tools,
            &check_gate_ids,
            &review_gate_ids,
        );
    }

    let protected_receipt_ids = index.protected_receipt_ids();
    assert_eq!(
        protected_receipt_ids,
        BTreeSet::from([
            "receipt_direct".to_string(),
            "receipt_passed_batch".to_string(),
            "receipt_malformed_batch".to_string(),
        ])
    );

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let retained_stream = receipts
        .iter()
        .filter(|receipt| protected_receipt_ids.contains(&receipt.id))
        .map(|receipt| serde_json::to_string(receipt).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(ctx.state_file("receipts.jsonl"), retained_stream).unwrap();

    let round_trip =
        work_gate_receipt_index(&ctx, "plan_open", &check_gate_tools, &review_gate_ids).unwrap();
    assert!(round_trip.check_gate_receipt("tests").is_none());
}

fn work_check_evidence(gate: Value) -> Value {
    let provided = gate.as_object().unwrap().clone();
    let mut gate = json!({
        "required": true,
        "paths_ignore": [],
        "reuse": true,
        "forced": false,
        "gate_signature": "sha256:gate",
        "reason": "test evidence",
        "changed_paths": [],
        "changed_path_count": 0,
        "changed_paths_truncated": false,
        "matching_paths": [],
        "matching_path_count": 0,
        "matching_paths_truncated": false
    })
    .as_object()
    .unwrap()
    .clone();
    gate.extend(provided);
    json!({
        "schema": WORK_CHECK_EVIDENCE_SCHEMA,
        "gates": [gate],
    })
}

#[test]
fn reusable_scan_tombstones_malformed_selected_batches() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let direct = test_receipt("receipt_direct", "plan_pass", tool::TEST, 10, json!({}));
    let mut passed = test_receipt(
        "receipt_passed_batch",
        "plan_pass",
        tool::WORK_CHECK,
        20,
        json!({"gates": ["tests"]}),
    );
    passed.worktree_fingerprint = Some("sha256:whole".into());
    passed.evidence = Some(work_check_evidence(json!({
        "gate_id": "tests",
        "tool": tool::TEST,
        "status": "executed",
        "applicability": "applicable",
        "scope_fingerprint": "sha256:scope",
        "tool_receipt_id": "receipt_direct",
        "exit_status": 0
    })));
    let mut malformed = test_receipt(
        "receipt_malformed_batch",
        "plan_failed",
        tool::WORK_CHECK,
        30,
        json!({"gates": ["tests"]}),
    );
    malformed.exit_status = 1;
    malformed.evidence = Some(json!({
        "schema": WORK_CHECK_EVIDENCE_SCHEMA,
        "gates": "not-an-array"
    }));
    let stream = [direct, passed, malformed]
        .into_iter()
        .map(|receipt| serde_json::to_string(&receipt).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(ctx.state_file("receipts.jsonl"), stream).unwrap();

    let reusable = reusable_work_check_evidence_batch_with_cancellation(
        &ctx,
        "plan_current",
        &[ReusableWorkCheckQuery {
            gate_id: "tests".into(),
            tool: tool::TEST.into(),
            gate_signature: "sha256:gate".into(),
            scope_fingerprint: "sha256:scope".into(),
        }],
        &|| false,
    )
    .unwrap();

    assert!(reusable.is_empty());
}

#[test]
fn reusable_scan_tombstones_selected_gate_missing_from_batch_evidence() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let direct = test_receipt("receipt_direct", "plan_pass", tool::TEST, 10, json!({}));
    let mut passed = test_receipt(
        "receipt_passed_batch",
        "plan_pass",
        tool::WORK_CHECK,
        20,
        json!({"gates": ["tests"]}),
    );
    passed.worktree_fingerprint = Some("sha256:whole".into());
    passed.evidence = Some(work_check_evidence(json!({
        "gate_id": "tests",
        "tool": tool::TEST,
        "status": "executed",
        "applicability": "applicable",
        "scope_fingerprint": "sha256:scope",
        "tool_receipt_id": "receipt_direct",
        "exit_status": 0
    })));
    let mut incomplete = test_receipt(
        "receipt_incomplete_batch",
        "plan_failed",
        tool::WORK_CHECK,
        30,
        json!({"gates": ["tests"]}),
    );
    incomplete.exit_status = 1;
    incomplete.evidence = Some(json!({
        "schema": WORK_CHECK_EVIDENCE_SCHEMA,
        "gates": []
    }));
    let stream = [direct, passed, incomplete]
        .into_iter()
        .map(|receipt| serde_json::to_string(&receipt).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(ctx.state_file("receipts.jsonl"), stream).unwrap();

    let reusable = reusable_work_check_evidence_batch_with_cancellation(
        &ctx,
        "plan_current",
        &[ReusableWorkCheckQuery {
            gate_id: "tests".into(),
            tool: tool::TEST.into(),
            gate_signature: "sha256:gate".into(),
            scope_fingerprint: "sha256:scope".into(),
        }],
        &|| false,
    )
    .unwrap();

    assert!(reusable.is_empty());
}

#[test]
fn reusable_scan_keeps_direct_proof_across_reuse_and_other_exact_inputs() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let direct = test_receipt("receipt_direct", "plan_pass", tool::TEST, 10, json!({}));
    let batch = |id: &str,
                 plan: &str,
                 ended: u64,
                 status: &str,
                 signature: &str,
                 scope: &str,
                 source: Option<&str>| {
        let mut receipt = test_receipt(
            id,
            plan,
            tool::WORK_CHECK,
            ended,
            json!({"gates": ["tests"]}),
        );
        receipt.worktree_fingerprint = Some("sha256:whole".into());
        receipt.evidence = Some(work_check_evidence(json!({
            "gate_id": "tests",
            "tool": tool::TEST,
            "status": status,
            "applicability": "applicable",
            "gate_signature": signature,
            "scope_fingerprint": scope,
            "tool_receipt_id": (status == "executed").then_some("receipt_direct"),
            "exit_status": (status == "executed").then_some(0),
            "source_plan_id": source
        })));
        if status == "failed" {
            receipt.exit_status = 1;
        }
        receipt
    };
    let passed = batch(
        "receipt_passed_batch",
        "plan_pass",
        20,
        "executed",
        "sha256:gate",
        "sha256:scope",
        None,
    );
    let reused = batch(
        "receipt_reused_batch",
        "plan_reused",
        30,
        "reused",
        "sha256:gate",
        "sha256:scope",
        Some("plan_pass"),
    );
    let other_signature = batch(
        "receipt_other_signature",
        "plan_other_signature",
        40,
        "failed",
        "sha256:other-gate",
        "sha256:scope",
        None,
    );
    let other_scope = batch(
        "receipt_other_scope",
        "plan_other_scope",
        50,
        "failed",
        "sha256:gate",
        "sha256:other-scope",
        None,
    );
    let stream = [direct, passed, reused, other_signature, other_scope]
        .into_iter()
        .map(|receipt| serde_json::to_string(&receipt).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(ctx.state_file("receipts.jsonl"), stream).unwrap();

    let reusable = reusable_work_check_evidence_batch_with_cancellation(
        &ctx,
        "plan_current",
        &[ReusableWorkCheckQuery {
            gate_id: "tests".into(),
            tool: tool::TEST.into(),
            gate_signature: "sha256:gate".into(),
            scope_fingerprint: "sha256:scope".into(),
        }],
        &|| false,
    )
    .unwrap();

    assert_eq!(
        reusable.get("tests").unwrap().source_batch_receipt_id,
        "receipt_passed_batch"
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
