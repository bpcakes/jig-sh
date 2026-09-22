use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::{Compression, write::GzEncoder};
use jig_contract::{
    ActionIntent, ActionRunner, PlannedTarget, RunConclusion, RunPlan, RunStatus, SourceIdentity,
    TargetId, TargetRunResult,
};
use serde_json::{Value, json};
use tempfile::tempdir;

use super::super::state_diagnose;
use super::*;
use crate::command::StateDiagnoseRequest;
use crate::context::RepoContext;
use crate::state::receipts::work_check_targets_evidence;
use crate::state::runs::{
    complete_run, mark_run_running, mark_target_started, record_target_result, start_run,
};
use crate::state::{WORK_CHECK_EVIDENCE_SCHEMA, WORK_CHECK_TARGETS_SCHEMA};
use crate::test_env::TestRepoBuilder;

mod damage;
mod lifecycle;
mod limits;
mod recovery;

const RUN_A: &str = "run_01ARZ3NDEKTSV4RRFFQ69G5FAV";
const RUN_B: &str = "run_01ARZ3NDEKTSV4RRFFQ69G5FB2";

fn fixture_context() -> (tempfile::TempDir, RepoContext) {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .repo_name("ExampleProject")
        .required_commands(["rust_test_command"])
        .write();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    (temp, ctx)
}

fn target_receipt(id: &str, tool: &str, target: &str, run_id: Option<&str>) -> Value {
    let target: TargetId = target.parse().unwrap();
    let mut receipt = json!({
        "id": id,
        "session_id": null,
        "plan_id": "plan_example",
        "tool_name": tool,
        "args": {},
        "started_at_ms": 1,
        "ended_at_ms": 2,
        "exit_status": 0,
        "stdout_preview": "",
        "stderr_preview": "",
        "target": target,
        "changed_paths": [],
        "diff_stat": {"files": 0, "insertions": 0, "deletions": 0},
    });
    if let Some(run_id) = run_id {
        receipt["run_id"] = json!(run_id);
    }
    receipt
}

fn work_check_targets_receipt(id: &str, children: &[(&str, &str, &str)]) -> Value {
    json!({
        "id": id,
        "session_id": null,
        "plan_id": "plan_example",
        "tool_name": "jig.work_check",
        "args": {"plan_id": "plan_example"},
        "started_at_ms": 3,
        "ended_at_ms": 4,
        "exit_status": 0,
        "stdout_preview": "",
        "stderr_preview": "",
        "evidence": work_check_targets_evidence(
            &children.iter().map(|(target, receipt_id, run_id)| json!({
                "target": target.parse::<TargetId>().unwrap(),
                "status": "passed",
                "receipt_id": receipt_id,
                "run_id": run_id,
                "disposition": "executed",
            })).collect::<Vec<_>>()
        ),
        "changed_paths": [],
        "diff_stat": {"files": 0, "insertions": 0, "deletions": 0},
    })
}

fn work_check_gates_receipt(id: &str, tool_receipt_ids: &[&str]) -> Value {
    json!({
        "id": id,
        "session_id": null,
        "plan_id": "plan_example",
        "tool_name": "jig.work_check",
        "args": {"plan_id": "plan_example"},
        "started_at_ms": 3,
        "ended_at_ms": 4,
        "exit_status": 0,
        "stdout_preview": "",
        "stderr_preview": "",
        "evidence": {
            "schema": WORK_CHECK_EVIDENCE_SCHEMA,
            "gates": tool_receipt_ids.iter().map(|receipt_id| json!({
                "gate_id": "tests",
                "tool": "jig.test",
                "status": "executed",
                "applicability": "applies",
                "gate_signature": "sha256:gate",
                "reason": "executed",
                "tool_receipt_id": receipt_id,
            })).collect::<Vec<_>>(),
        },
        "changed_paths": [],
        "diff_stat": {"files": 0, "insertions": 0, "deletions": 0},
    })
}

fn write_records(path: &Path, records: &[Value]) {
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend(serde_json::to_vec(record).unwrap());
        bytes.push(b'\n');
    }
    fs::write(path, bytes).unwrap();
}

fn append_record(path: &Path, record: &Value) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    serde_json::to_writer(&mut file, record).unwrap();
    file.write_all(b"\n").unwrap();
}

fn write_gzip(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut encoder = GzEncoder::new(fs::File::create(path).unwrap(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap();
}

fn append_gzip_member(path: &Path, bytes: &[u8]) {
    let file = fs::OpenOptions::new().append(true).open(path).unwrap();
    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap();
}

/// Three target receipts plus the work-check batch that references them,
/// exactly as `work check` writes them: the batch carries no top-level run_id.
fn write_orphan_batch(ctx: &RepoContext) {
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[
            target_receipt("receipt_fmt", "jig.fmt_check", "api:fmt", Some(RUN_A)),
            target_receipt("receipt_clippy", "jig.clippy", "api:clippy", Some(RUN_A)),
            target_receipt("receipt_test", "jig.test", "api:test", Some(RUN_A)),
            work_check_targets_receipt(
                "receipt_batch",
                &[
                    ("api:fmt", "receipt_fmt", RUN_A),
                    ("api:clippy", "receipt_clippy", RUN_A),
                    ("api:test", "receipt_test", RUN_A),
                ],
            ),
        ],
    );
}

fn plan() -> RunPlan {
    let target: TargetId = "api:test".parse().unwrap();
    RunPlan::new(
        "run-plan_example",
        "sha256:config",
        SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
        vec![PlannedTarget::new(
            target.clone(),
            ActionIntent::Check,
            ActionRunner::command("test"),
            "sha256:input",
        )],
        vec![vec![target]],
    )
}

fn complete_target(ctx: &RepoContext, run_id: &str) {
    let target: TargetId = "api:test".parse().unwrap();
    mark_target_started(ctx, run_id, target.clone()).unwrap();
    let mut result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
    result.status = RunStatus::Completed;
    result.conclusion = Some(RunConclusion::Success);
    result.started_at_ms = Some(1);
    result.ended_at_ms = Some(2);
    result.exit_code = Some(0);
    record_target_result(ctx, run_id, result).unwrap();
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            let file_type = entry.file_type().unwrap();
            if file_type.is_symlink() {
                files.insert(
                    relative,
                    Some(
                        fs::read_link(&path)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned()
                            .into_bytes(),
                    ),
                );
            } else if file_type.is_dir() {
                files.insert(relative, None);
                pending.push(path);
            } else {
                files.insert(relative, Some(fs::read(&path).unwrap()));
            }
        }
    }
    files
}

fn diagnose(ctx: &RepoContext, deep: bool) -> Value {
    let before = snapshot(ctx.root());
    let output = state_diagnose(ctx, StateDiagnoseRequest { deep });
    assert_eq!(
        before,
        snapshot(ctx.root()),
        "diagnosis must not touch bytes or inventory"
    );
    output
}

fn finding_for<'a>(output: &'a Value, run_id: &str) -> &'a Value {
    output["run_linkage"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["run_id"] == run_id)
        .unwrap_or_else(|| panic!("no finding for {run_id}"))
}

fn recommendation_kinds(output: &Value) -> Vec<&str> {
    output["recommendations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect()
}

fn assert_string_array_contains(value: &Value, expected: &str) {
    assert!(
        value
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().is_some_and(|item| item.contains(expected))),
        "expected {value} to contain a string matching {expected:?}"
    );
}

fn set_backup_created_at(directory: &Path, created_at_ms: u64) {
    let manifest_path = directory.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["created_at_ms"] = json!(created_at_ms);
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
}

#[test]
fn shallow_diagnosis_discloses_that_linkage_was_not_checked() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);

    let output = diagnose(&ctx, false);

    assert_eq!(output["ok"], true);
    assert_eq!(output["run_linkage"]["checked"], false);
    assert_eq!(output["run_linkage"]["verdict"], "not_checked");
    assert_eq!(output["run_linkage"]["complete"], false);
    assert_eq!(output["run_linkage"]["reason"], NOT_CHECKED_REASON);
    assert_eq!(output["integrity"]["run_linkage"], "not_checked");
    assert!(
        output["run_linkage"]["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(recommendation_kinds(&output).is_empty());
}

#[test]
fn unresolved_supported_batch_child_makes_linkage_incomplete() {
    let (_temp, ctx) = fixture_context();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[work_check_gates_receipt(
            "receipt_batch",
            &["receipt_missing"],
        )],
    );

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];

    assert_eq!(linkage["verdict"], "incomplete");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["referenced_runs"], 0);
    assert_eq!(linkage["unresolved_batch_links"], 1);
    assert_eq!(linkage["finding_count"], 0);
    assert_string_array_contains(
        &linkage["incomplete_reasons"],
        "supported batch child receipt link(s) reference missing receipt identities",
    );
}

#[test]
fn unresolved_v1_child_receipt_stays_incomplete_when_its_copied_run_id_resolves() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[work_check_targets_receipt(
            "receipt_batch",
            &[("api:test", "receipt_missing", &run_id)],
        )],
    );

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];

    assert_eq!(linkage["verdict"], "incomplete");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["referenced_runs"], 1);
    assert_eq!(linkage["unresolved_batch_links"], 1);
    assert_eq!(linkage["runs"]["completed"], 1);
    assert_string_array_contains(
        &linkage["incomplete_reasons"],
        "supported batch child receipt link(s) reference missing receipt identities",
    );
}

#[test]
fn deep_diagnosis_reports_wholly_missing_run_history_for_a_linked_batch() {
    for runs_journal in [None, Some(b"".as_slice())] {
        let (_temp, ctx) = fixture_context();
        write_orphan_batch(&ctx);
        if let Some(bytes) = runs_journal {
            fs::write(ctx.state_file("runs.jsonl"), bytes).unwrap();
        }

        let output = diagnose(&ctx, true);

        let expected_authority = if runs_journal.is_some() {
            "verified"
        } else {
            "absent"
        };
        assert_missing_batch_linkage(&output, expected_authority);
        assert_missing_batch_finding(&output);
        assert_preservation_recommendation(&output);
        assert!(!ctx.root().join(".agent/.cache").exists());
    }
}

fn assert_missing_batch_linkage(output: &Value, expected_authority: &str) {
    assert_eq!(
        output["ok"], true,
        "command success is not an integrity verdict"
    );
    let linkage = &output["run_linkage"];
    assert_eq!(linkage["checked"], true);
    assert_eq!(linkage["complete"], true);
    assert_eq!(linkage["verdict"], "findings");
    assert_eq!(linkage["receipts_with_run_id"], 3);
    assert_eq!(linkage["batch_receipts"], 1);
    assert_eq!(linkage["batch_links"], 3);
    assert_eq!(linkage["referenced_runs"], 1);
    assert_eq!(linkage["runs"]["missing"], 1);
    assert_eq!(linkage["finding_count"], 1);
    assert_eq!(linkage["findings_truncated"], false);
    assert_eq!(linkage["journal"]["authority"], expected_authority);
    assert_eq!(linkage["sources"]["archives_scanned"], 0);
    assert_eq!(linkage["sources"]["backups_scanned"], 0);
    assert_eq!(output["integrity"]["run_linkage"], "findings");
    assert_eq!(output["integrity"]["run_linkage_findings"], 1);
    assert!(linkage["guidance"]["never"].is_array());
}

fn assert_missing_batch_finding(output: &Value) {
    let finding = finding_for(output, RUN_A);
    assert_eq!(finding["status"], "missing");
    assert_eq!(
        finding["receipt_ids"],
        json!(["receipt_clippy", "receipt_fmt", "receipt_test"])
    );
    assert_eq!(finding["receipt_count"], 3);
    assert_eq!(finding["batch_receipt_ids"], json!(["receipt_batch"]));
    assert_eq!(finding["lease_file_present"], false);
    assert!(finding["recovery"].is_null());
    assert!(
        finding["detail"]
            .as_str()
            .unwrap()
            .contains("does not prove the events were deleted")
    );
}

fn assert_preservation_recommendation(output: &Value) {
    assert_eq!(
        recommendation_kinds(output),
        vec!["preserve_unlinked_receipt_evidence"]
    );
    let recommendation = &output["recommendations"][0];
    assert_eq!(recommendation["affected_run_ids"], json!([RUN_A]));
    assert!(
        recommendation["command"]
            .as_str()
            .unwrap()
            .contains("state export receipts")
    );
    assert!(
        recommendation["alternative_command"]
            .as_str()
            .unwrap()
            .contains("work decide")
    );
}

#[test]
fn reused_batch_evidence_may_reference_several_runs() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let live_run = started.result.run_id;
    drop(lease);
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[
            target_receipt("receipt_test", "jig.test", "api:test", Some(&live_run)),
            target_receipt("receipt_fmt", "jig.fmt_check", "api:fmt", Some(RUN_B)),
            work_check_targets_receipt(
                "receipt_batch",
                &[
                    ("api:test", "receipt_test", &live_run),
                    ("api:fmt", "receipt_fmt", RUN_B),
                ],
            ),
        ],
    );

    let output = diagnose(&ctx, true);

    assert_eq!(output["run_linkage"]["referenced_runs"], 2);
    assert_eq!(output["run_linkage"]["runs"]["active"], 1);
    assert_eq!(output["run_linkage"]["finding_count"], 1);
    let finding = finding_for(&output, RUN_B);
    assert_eq!(finding["receipt_ids"], json!(["receipt_fmt"]));
    assert_eq!(finding["batch_receipt_ids"], json!(["receipt_batch"]));
}

#[test]
fn conflicting_batch_and_child_run_ids_make_linkage_incomplete() {
    let (_temp, ctx) = fixture_context();
    let (first, first_lease) = start_run(&ctx, plan(), None).unwrap();
    let first_run = first.result.run_id;
    complete_target(&ctx, &first_run);
    complete_run(&ctx, &first_run, RunConclusion::Success).unwrap();
    drop(first_lease);
    let (second, second_lease) = start_run(&ctx, plan(), None).unwrap();
    let second_run = second.result.run_id;
    complete_target(&ctx, &second_run);
    complete_run(&ctx, &second_run, RunConclusion::Success).unwrap();
    drop(second_lease);
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[
            target_receipt("receipt_test", "jig.test", "api:test", Some(&second_run)),
            work_check_targets_receipt(
                "receipt_batch",
                &[("api:test", "receipt_test", &first_run)],
            ),
        ],
    );

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];

    assert_eq!(linkage["verdict"], "incomplete");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["conflicting_batch_links"], 1);
    assert_eq!(linkage["referenced_runs"], 2);
    assert_eq!(linkage["runs"]["completed"], 2);
    assert_eq!(linkage["finding_count"], 0);
    assert_string_array_contains(
        &linkage["incomplete_reasons"],
        "supported batch child link(s) carry run IDs that conflict with their receipt histories",
    );
}

#[test]
fn gate_batch_evidence_links_tool_receipts_through_their_own_run_ids() {
    let (_temp, ctx) = fixture_context();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[
            target_receipt("receipt_test", "jig.test", "api:test", Some(RUN_A)),
            work_check_gates_receipt("receipt_gate_batch", &["receipt_test"]),
        ],
    );

    let output = diagnose(&ctx, true);

    let finding = finding_for(&output, RUN_A);
    assert_eq!(finding["status"], "missing");
    assert_eq!(finding["receipt_ids"], json!(["receipt_test"]));
    assert_eq!(finding["batch_receipt_ids"], json!(["receipt_gate_batch"]));
    assert_eq!(output["run_linkage"]["batch_receipts"], 1);
}

#[test]
fn gate_batch_existing_no_run_receipts_are_not_unresolved() {
    let (_temp, ctx) = fixture_context();
    let mut explicit_null_run = target_receipt("receipt_source", "jig.test", "api:test", None);
    explicit_null_run["run_id"] = Value::Null;
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[
            target_receipt("receipt_tool", "jig.test", "api:test", None),
            explicit_null_run,
            json!({
                "id": "receipt_gate_batch",
                "tool_name": "jig.work_check",
                "evidence": {
                    "schema": WORK_CHECK_EVIDENCE_SCHEMA,
                    "gates": [{
                        "tool_receipt_id": "receipt_tool",
                        "source_tool_receipt_id": "receipt_source"
                    }]
                }
            }),
        ],
    );

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];

    assert_eq!(linkage["batch_links"], 2);
    assert_eq!(linkage["unresolved_batch_links"], 0);
    assert_eq!(linkage["referenced_runs"], 0);
    assert_eq!(linkage["verdict"], "clean");
}

#[test]
fn supported_batch_evidence_requires_its_array_container() {
    for (schema, field) in [
        (WORK_CHECK_TARGETS_SCHEMA, "targets"),
        (WORK_CHECK_EVIDENCE_SCHEMA, "gates"),
    ] {
        for (shape, malformed) in [
            ("object", Some(json!({"receipt_id": "receipt_missing"}))),
            ("string", Some(json!("not-an-array"))),
            ("null", Some(Value::Null)),
            ("missing", None),
        ] {
            let (_temp, ctx) = fixture_context();
            let mut evidence = json!({"schema": schema});
            if let Some(malformed) = malformed {
                evidence[field] = malformed;
            }
            write_records(
                &ctx.state_file("receipts.jsonl"),
                &[json!({
                    "id": format!("receipt_{field}_{shape}"),
                    "tool_name": "jig.work_check",
                    "evidence": evidence,
                })],
            );

            let output = diagnose(&ctx, true);

            assert_eq!(
                output["streams"]["receipts"]["deep_analysis_error_count"], 1,
                "schema {schema} with {shape} {field}"
            );
            assert_eq!(
                output["run_linkage"]["complete"], false,
                "schema {schema} with {shape} {field}"
            );
            assert_eq!(
                output["run_linkage"]["verdict"], "incomplete",
                "schema {schema} with {shape} {field}"
            );
        }
    }
}

#[test]
fn optional_and_legacy_links_are_valid_without_run_history() {
    let (_temp, ctx) = fixture_context();
    let mut null_run = target_receipt("receipt_null", "jig.test", "api:test", None);
    null_run["run_id"] = Value::Null;
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[
            target_receipt("receipt_legacy", "jig.test", "api:test", None),
            null_run,
            json!({"id": "receipt_tool", "tool_name": "jig.session_start", "evidence": {"schema": "jig.other/v1", "targets": [{"receipt_id": "x", "run_id": "run_y"}]}}),
            json!({"id": "receipt_string_evidence", "tool_name": "jig.note", "evidence": "run_01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
            work_check_targets_receipt("receipt_empty_batch", &[]),
            work_check_gates_receipt("receipt_empty_gate_batch", &[]),
        ],
    );

    let output = diagnose(&ctx, true);

    let linkage = &output["run_linkage"];
    assert_eq!(linkage["verdict"], "clean");
    assert_eq!(linkage["receipts_with_run_id"], 0);
    assert_eq!(linkage["batch_receipts"], 0);
    assert_eq!(linkage["referenced_runs"], 0);
    assert_eq!(linkage["finding_count"], 0);
    assert_eq!(
        output["streams"]["receipts"]["deep_analysis_error_count"],
        0
    );
}

#[test]
fn large_results_report_counts_and_truncation() {
    let (_temp, ctx) = fixture_context();
    let mut receipts = Vec::new();
    for index in 0..(MAX_FINDINGS + 1) {
        receipts.push(target_receipt(
            &format!("receipt_{index:04}"),
            "jig.test",
            "api:test",
            Some(&format!("run_{index:04}")),
        ));
    }
    for index in 0..(MAX_FINDING_IDS + 1) {
        receipts.push(target_receipt(
            &format!("receipt_shared_{index:04}"),
            "jig.test",
            "api:test",
            Some("run_00-shared"),
        ));
    }
    write_records(&ctx.state_file("receipts.jsonl"), &receipts);

    let output = diagnose(&ctx, true);

    let linkage = &output["run_linkage"];
    assert_eq!(linkage["finding_count"], MAX_FINDINGS as u64 + 2);
    assert_eq!(linkage["findings"].as_array().unwrap().len(), MAX_FINDINGS);
    assert_eq!(linkage["findings_truncated"], true);
    assert_eq!(linkage["runs"]["missing"], MAX_FINDINGS as u64 + 2);
    let shared = finding_for(&output, "run_00-shared");
    assert_eq!(shared["receipt_count"], MAX_FINDING_IDS as u64 + 1);
    assert_eq!(
        shared["receipt_ids"].as_array().unwrap().len(),
        MAX_FINDING_IDS
    );
    assert_eq!(shared["receipt_ids_truncated"], true);
    assert_eq!(
        output["recommendations"][0]["affected_run_ids_truncated"],
        true
    );
    assert!(
        output["recommendations"][0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("At least "),
        "truncated recommendations must mark the retained receipt count as a lower bound"
    );
}

#[test]
fn diagnosis_uses_authoritative_target_and_completion_validation() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);
    let target: TargetId = "api:test".parse().unwrap();
    write_records(
        &ctx.state_file("runs.jsonl"),
        &[
            json!({
                "id": "run_event_queued",
                "run_id": RUN_A,
                "event": "queued",
                "timestamp_ms": 1,
                "plan": plan(),
            }),
            json!({
                "id": "run_event_target_completed",
                "run_id": RUN_A,
                "event": "target_completed",
                "timestamp_ms": 2,
                "target": target,
            }),
            json!({
                "id": "run_event_completed",
                "run_id": RUN_A,
                "event": "completed",
                "timestamp_ms": 3,
                "conclusion": "success",
            }),
        ],
    );

    let output = diagnose(&ctx, true);
    let finding = finding_for(&output, RUN_A);

    assert_eq!(finding["status"], "inconsistent");
    assert_string_array_contains(
        &finding["journal_anomalies"],
        "target_completed event has no result",
    );
    assert_string_array_contains(
        &finding["journal_anomalies"],
        "completed before every target reached a conclusion",
    );
}

#[test]
fn structurally_invalid_queued_plan_is_inconsistent_and_not_recoverable() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);
    let mut invalid_plan = plan();
    invalid_plan.execution_layers.clear();
    let target: TargetId = "api:test".parse().unwrap();
    let mut result = TargetRunResult::queued(
        target.clone(),
        invalid_plan.config_digest.clone(),
        invalid_plan.targets[0].input_digest.clone(),
    );
    result.status = RunStatus::Completed;
    result.conclusion = Some(RunConclusion::Success);
    result.started_at_ms = Some(1);
    result.ended_at_ms = Some(2);
    result.exit_code = Some(0);
    write_records(
        &ctx.state_file("runs.jsonl"),
        &[
            json!({
                "id": "run_event_queued",
                "run_id": RUN_A,
                "event": "queued",
                "timestamp_ms": 1,
                "plan": invalid_plan,
            }),
            json!({
                "id": "run_event_target_completed",
                "run_id": RUN_A,
                "event": "target_completed",
                "timestamp_ms": 2,
                "target": target,
                "result": result,
            }),
            json!({
                "id": "run_event_completed",
                "run_id": RUN_A,
                "event": "completed",
                "timestamp_ms": 3,
                "conclusion": "success",
            }),
        ],
    );

    let journal = diagnose(&ctx, true);
    let finding = finding_for(&journal, RUN_A);
    assert_eq!(finding["status"], "inconsistent");
    assert_string_array_contains(
        &finding["journal_anomalies"],
        "execution layers omit planned target(s): api:test",
    );

    let runs_path = ctx.state_file("runs.jsonl");
    crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "invalid-plan-recovery", None)
        .unwrap();
    fs::write(&runs_path, b"").unwrap();

    let backup = diagnose(&ctx, true);
    let finding = finding_for(&backup, RUN_A);
    assert_eq!(finding["status"], "unverifiable");
    assert!(finding["recovery"].is_null());
    assert_string_array_contains(
        &backup["run_linkage"]["sources"]["errors"],
        "execution layers omit planned target(s): api:test",
    );
}
