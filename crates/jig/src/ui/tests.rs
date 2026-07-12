use std::fs;
use std::path::Path;

use jig_ui::{TimelineItem, TimelineShow, UiQuery};
use serde_json::json;
use tempfile::tempdir;

use crate::context::RepoContext;
use crate::state::{ReceiptInput, record_receipt};

use super::snapshot::{snapshot, snapshot_with_query};

fn write_ui_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[commands]
custom_check_command = "printf 'manifest target ran\n'"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["custom_check_command"],
            "tools": [
                {
                    "name": "jig.custom_check",
                    "kind": "command",
                    "description": "Run configured custom check.",
                    "command": "custom_check_command"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();
}

fn seeded_context(root: &Path) -> RepoContext {
    write_ui_fixture_repo(root);
    let ctx = RepoContext::load_from(root).unwrap();
    crate::state::seed_open_plan_for_test(&ctx, "plan_ui", "Ship the <ui>", "# Plan body\n")
        .unwrap();
    record_receipt(
        &ctx,
        ReceiptInput {
            tool_name: "jig.custom_check",
            args: json!({}),
            invoked_command_key: Some("custom_check_command".into()),
            plan_id: Some("plan_ui".into()),
            started_at_ms: 1_000,
            ended_at_ms: 3_500,
            exit_status: 1,
            stdout: "",
            stderr: "check exploded",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();
    ctx
}

#[cfg(unix)]
fn set_tree_directory_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    if !path.exists() {
        return;
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            set_tree_directory_mode(&path, mode);
        }
    }
}

#[cfg(unix)]
fn set_tree_file_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    if !path.exists() {
        return;
    }
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            set_tree_file_mode(&path, mode);
        } else {
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        }
    }
}

#[test]
fn snapshot_on_uninitialized_repo_creates_nothing() {
    let temp = tempdir().unwrap();
    write_ui_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let value = snapshot(&ctx).unwrap();

    assert_eq!(value["ok"], true);
    assert!(value["counts"].get("receipts").is_none());
    assert!(!temp.path().join(".agent/state").exists());
    assert!(!temp.path().join(".agent/.cache").exists());
    assert!(!temp.path().join(".agent/plans").exists());
}

#[cfg(unix)]
#[test]
fn snapshot_reads_existing_read_only_state_without_creating_loop_cache() {
    let temp = tempdir().unwrap();
    let ctx = seeded_context(temp.path());
    let state_dir = temp.path().join(".agent/state");
    let plans_dir = temp.path().join(".agent/plans");
    let cache_dir = temp.path().join(".agent/.cache");
    set_tree_file_mode(&state_dir, 0o444);
    set_tree_file_mode(&plans_dir, 0o444);
    set_tree_file_mode(&cache_dir, 0o444);
    set_tree_directory_mode(&state_dir, 0o555);
    set_tree_directory_mode(&plans_dir, 0o555);
    set_tree_directory_mode(&cache_dir, 0o555);

    let value = snapshot(&ctx).unwrap();

    assert_eq!(value["ok"], true);
    assert!(value["counts"].get("receipts").is_none());
    assert!(!cache_dir.join("loop").exists());
    set_tree_directory_mode(&state_dir, 0o755);
    set_tree_directory_mode(&plans_dir, 0o755);
    set_tree_directory_mode(&cache_dir, 0o755);
}

#[test]
fn snapshot_joins_plans_gates_loops_and_timeline() {
    let temp = tempdir().unwrap();
    let ctx = seeded_context(temp.path());

    let snapshot = snapshot(&ctx).unwrap();

    assert_eq!(snapshot["ok"], true);
    assert_eq!(snapshot["repo"]["name"], "demo");

    let plans = snapshot["open_plans"].as_array().unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0]["plan_id"], "plan_ui");
    assert!(plans[0]["opened_at_ms"].as_u64().is_some());
    let gates = plans[0]["gates"]["gates"].as_array().unwrap();
    assert_eq!(gates[0]["id"], "custom");
    assert_eq!(gates[0]["status"], "failed");
    assert_eq!(plans[0]["gates"]["overall"], "blocked");

    // The default noop-status workflow reports even without loop config.
    let workflows = snapshot["loops"]["workflows"].as_array().unwrap();
    assert!(!workflows.is_empty());

    let timeline = snapshot["timeline"].as_array().unwrap();
    let receipt = timeline
        .iter()
        .find(|entry| entry["kind"] == "receipt" && entry["tool_name"] == "jig.custom_check")
        .expect("timeline should include the recorded receipt");
    assert_eq!(receipt["exit_status"], 1);
    assert_eq!(receipt["duration_ms"], 2_500);
    assert_eq!(receipt["stderr_preview"], "check exploded");
    assert!(
        timeline
            .iter()
            .any(|entry| entry["kind"] == "plan" && entry["event"] == "open"),
        "timeline should include the plan open event"
    );
}

#[test]
fn snapshot_passing_receipt_omits_stderr_preview() {
    let temp = tempdir().unwrap();
    write_ui_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    record_receipt(
        &ctx,
        ReceiptInput {
            tool_name: "jig.custom_check",
            args: json!({}),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: 1_000,
            ended_at_ms: 1_200,
            exit_status: 0,
            stdout: "fine",
            stderr: "noise",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();

    let snapshot = snapshot(&ctx).unwrap();

    let timeline = snapshot["timeline"].as_array().unwrap();
    let receipt = timeline
        .iter()
        .find(|entry| entry["kind"] == "receipt")
        .unwrap();
    assert!(receipt.get("stderr_preview").is_none());
}

#[test]
fn snapshot_reports_history_failures_and_tool_stats() {
    let temp = tempdir().unwrap();
    let ctx = seeded_context(temp.path());
    crate::state::plans_close(
        &ctx,
        crate::state::PlanCloseRequest {
            plan_id: "plan_ui".into(),
            resolution: Some("Landed the change".into()),
        },
    )
    .unwrap();

    let snapshot = snapshot(&ctx).unwrap();

    assert!(snapshot["open_plans"].as_array().unwrap().is_empty());
    let history = snapshot["history"].as_array().unwrap();
    assert_eq!(history[0]["plan_id"], "plan_ui");
    assert_eq!(history[0]["state"], "closed");
    assert_eq!(history[0]["resolution"], "Landed the change");
    assert!(history[0]["closed_at_ms"].as_u64().is_some());

    let failures = snapshot["failures"].as_array().unwrap();
    assert_eq!(failures[0]["tool_name"], "jig.custom_check");
    assert_eq!(failures[0]["stderr_preview"], "check exploded");

    let stats = snapshot["tool_stats"].as_array().unwrap();
    let row = stats
        .iter()
        .find(|row| row["tool"] == "jig.custom_check")
        .expect("tool stats should aggregate command receipts");
    assert_eq!(row["runs"], 1);
    assert_eq!(row["failures"], 1);
    assert_eq!(row["last_exit_status"], 1);
    assert_eq!(row["avg_duration_ms"], 2_500);
    assert_eq!(snapshot["harness"]["contract_version"], 3);
}

#[test]
fn snapshot_timeline_filter_narrows_kinds() {
    let temp = tempdir().unwrap();
    let ctx = seeded_context(temp.path());

    let failures_only = snapshot_with_query(
        &ctx,
        UiQuery {
            show: TimelineShow::Failures,
            limit: 5,
        },
    )
    .unwrap();
    let timeline = &failures_only.timeline;
    assert!(!timeline.is_empty());
    assert!(
        timeline.iter().all(
            |entry| matches!(entry, TimelineItem::Receipt(receipt) if receipt.exit_status != 0)
        )
    );
    assert_eq!(failures_only.timeline_show, "failures");

    let plans_only = snapshot_with_query(
        &ctx,
        UiQuery {
            show: TimelineShow::Plans,
            limit: 5,
        },
    )
    .unwrap();
    assert!(
        plans_only
            .timeline
            .iter()
            .all(|entry| matches!(entry, TimelineItem::Plan(_)))
    );
}

#[test]
fn plan_snapshot_reports_non_utf8_body_error() {
    let temp = tempdir().unwrap();
    let ctx = seeded_context(temp.path());
    fs::write(ctx.plan_body_path("plan_ui"), [0xff, 0xfe]).unwrap();

    let plan = super::snapshot::plan_snapshot(&ctx, "plan_ui")
        .unwrap()
        .unwrap();

    assert!(plan.body.is_none());
    assert!(plan.body_error.as_deref().is_some_and(|error| {
        error.contains("Failed to read plan body")
            && error.contains("stream did not contain valid UTF-8")
    }));
}

#[test]
fn plan_snapshot_finds_old_receipts_beyond_global_dashboard_window() {
    let temp = tempdir().unwrap();
    let ctx = seeded_context(temp.path());
    for index in 0..401 {
        record_receipt(
            &ctx,
            ReceiptInput {
                tool_name: "jig.unrelated",
                args: json!({}),
                invoked_command_key: Some("unrelated".into()),
                plan_id: Some("another_plan".into()),
                started_at_ms: 10_000 + index,
                ended_at_ms: 10_001 + index,
                exit_status: 0,
                stdout: "",
                stderr: "",
                evidence: None,
                session_override: None,
                collect_git_metadata: false,
                collect_worktree_fingerprint: false,
                worktree_fingerprint_override: None,
            },
        )
        .unwrap();
    }

    let plan = super::snapshot::plan_snapshot(&ctx, "plan_ui")
        .unwrap()
        .unwrap();

    assert_eq!(plan.receipts.len(), 1);
    assert_eq!(plan.receipts[0].plan_id.as_deref(), Some("plan_ui"));
    assert_eq!(plan.receipts[0].stderr_preview, "check exploded");
}
