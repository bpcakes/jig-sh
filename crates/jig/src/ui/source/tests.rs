use std::fs;
use std::sync::Mutex;

use jig_ui::dashboard::{
    DashboardSource, PlanBasis, PlanSnapshotResult, RecorderMode, RecorderRequest, SourceError,
    StatusOutcome, StatusPhase, StatusRequest, TimelineLimit,
};
use serde_json::json;
use tempfile::tempdir;

use crate::context::RepoContext;
use crate::state::{ReceiptInput, record_receipt, seed_open_plan_for_test};
use crate::test_env::TestRepoBuilder;

use super::RepoDashboardSource;

mod details;
mod edge_cases;
mod gates;
mod limits;

fn source_fixture() -> (tempfile::TempDir, RepoDashboardSource) {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .config(
            r#"
[commands]
custom_check_command = "true"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
        )
        .required_commands(["custom_check_command"])
        .tool(json!({
            "name": "jig.custom_check",
            "kind": "command",
            "description": "Run configured custom check.",
            "command": "custom_check_command"
        }))
        .write();
    let context = RepoContext::load_from(root.path()).unwrap();
    seed_open_plan_for_test(&context, "plan_example", "Example plan", "# Example plan\n").unwrap();
    record_receipt(
        &context,
        ReceiptInput {
            tool_name: "jig.custom_check",
            args: json!({}),
            invoked_command_key: Some("custom_check_command".to_string()),
            plan_id: Some("plan_example".to_string()),
            started_at_ms: 10,
            ended_at_ms: 20,
            exit_status: 0,
            stdout: "ok",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();
    (root, RepoDashboardSource::new(context))
}

fn recorder_request(mode: RecorderMode) -> RecorderRequest {
    RecorderRequest {
        mode,
        timeline_limit: TimelineLimit::new(50).unwrap(),
    }
}

fn relative_tree_paths(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    fn collect(
        root: &std::path::Path,
        directory: &std::path::Path,
        paths: &mut Vec<std::path::PathBuf>,
    ) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            paths.push(path.strip_prefix(root).unwrap().to_path_buf());
            if path.is_dir() {
                collect(root, &path, paths);
            }
        }
    }

    let mut paths = Vec::new();
    collect(root, root, &mut paths);
    paths.sort();
    paths
}

#[test]
fn recorder_on_uninitialized_state_creates_no_runtime_directories() {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path()).write();
    let source = RepoDashboardSource::new(RepoContext::load_from(root.path()).unwrap());

    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();

    assert!(refresh.recorder.ok);
    assert!(!root.path().join(".agent/state").exists());
    assert!(!root.path().join(".agent/plans").exists());
    assert!(!root.path().join(".agent/.cache").exists());
}

#[cfg(unix)]
#[test]
fn recorder_reads_existing_read_only_state_without_creating_loop_cache() {
    use std::os::unix::fs::PermissionsExt;

    fn set_file_modes(root: &std::path::Path, mode: u32) {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                set_file_modes(&path, mode);
            } else {
                fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
            }
        }
    }

    fn set_directory_modes(root: &std::path::Path, mode: u32) {
        if !root.exists() {
            return;
        }
        fs::set_permissions(root, fs::Permissions::from_mode(mode)).unwrap();
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                set_directory_modes(&path, mode);
            }
        }
    }

    let (root, source) = source_fixture();
    let agent = root.path().join(".agent");
    let paths_before = relative_tree_paths(&agent);
    set_file_modes(&agent, 0o444);
    set_directory_modes(&agent, 0o555);

    let result = source.recorder(recorder_request(RecorderMode::Refresh), &|| false);

    set_directory_modes(&agent, 0o755);
    set_file_modes(&agent, 0o644);
    let refresh = result.unwrap();
    assert!(refresh.recorder.ok);
    assert!(!agent.join(".cache/loop").exists());
    assert_eq!(relative_tree_paths(&agent), paths_before);
}

#[test]
fn recorder_refreshes_repository_metadata_after_source_construction() {
    let (root, source) = source_fixture();
    let config_path = root.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace("repo_name = \"demo\"", "repo_name = \"RenamedExample\""),
    )
    .unwrap();

    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();

    assert_eq!(refresh.recorder.repo.name, "RenamedExample");
}

#[test]
fn recorder_refresh_pairs_one_epoch_and_reuse_performs_no_refresh() {
    let (_root, source) = source_fixture();
    let first = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert_eq!(first.recorder.epoch_id, first.status_local.epoch_id);
    let encoded = serde_json::to_value(&first.recorder).unwrap();
    let _: jig_ui::dashboard::RecorderSnapshot = serde_json::from_value(encoded).unwrap();
    assert_eq!(first.recorder.open_plans.len(), 1);
    assert_eq!(
        first
            .status_local
            .work
            .state
            .as_ref()
            .unwrap()
            .open_plans
            .len(),
        1
    );

    crate::state::reset_dashboard_scan_counts();
    let reused = source
        .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
        .unwrap();
    assert_eq!(reused.recorder.epoch_id, first.recorder.epoch_id);
    assert_eq!(reused.status_local.epoch_id, first.status_local.epoch_id);
    for stream in [
        "sessions.jsonl",
        "plans.jsonl",
        "decisions.jsonl",
        "receipts.jsonl",
    ] {
        assert_eq!(
            crate::state::dashboard_scan_count(&source.context.state_file(stream)),
            0,
            "ReuseCurrent must not traverse {stream}"
        );
    }
}

#[test]
fn reuse_before_the_first_refresh_is_a_modeled_empty_state() {
    let (_root, source) = source_fixture();
    assert_eq!(
        source
            .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
            .unwrap_err(),
        SourceError::NoCurrentEpoch
    );
}

#[test]
fn status_announces_provider_then_local_and_returns_paired_epoch() {
    let (_root, source) = source_fixture();
    crate::status::git::reset_git_observation_counts();
    let phases = Mutex::new(Vec::new());
    let refresh = source
        .status(
            StatusRequest {
                timeline_limit: TimelineLimit::new(25).unwrap(),
            },
            &|phase| phases.lock().unwrap().push(phase),
            &|| false,
        )
        .unwrap();

    assert_eq!(
        *phases.lock().unwrap(),
        vec![StatusPhase::Providers, StatusPhase::LocalEpoch]
    );
    assert_eq!(refresh.recorder.open_plans[0].plan_id, "plan_example");
    assert_eq!(
        refresh.status.work.state.as_ref().unwrap().open_plans[0].plan_id,
        "plan_example"
    );
    assert_eq!(
        crate::status::git::git_observation_count(source.context.root()),
        1,
        "status must share the root Git observation between repository and provider freshness"
    );
}

#[test]
fn stale_missing_fresh_and_failed_refresh_retention_are_distinct() {
    let (root, source) = source_fixture();
    let first = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let first_id = first.recorder.epoch_id;
    assert!(matches!(
        source
            .plan(
                PlanBasis::RecorderEpoch(first_id),
                "plan_example".to_string(),
                &|| false,
            )
            .unwrap(),
        PlanSnapshotResult::Found(_)
    ));

    let cancelled = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| true)
        .unwrap_err();
    assert_eq!(cancelled, SourceError::Cancelled);
    let retained = source
        .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
        .unwrap();
    assert_eq!(retained.recorder.epoch_id, first_id);

    let second = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert!(matches!(
        source
            .plan(
                PlanBasis::RecorderEpoch(first_id),
                "plan_example".to_string(),
                &|| false,
            )
            .unwrap(),
        PlanSnapshotResult::StaleRecorderEpoch
    ));
    assert!(matches!(
        source
            .plan(
                PlanBasis::RecorderEpoch(second.recorder.epoch_id),
                "missing_plan".to_string(),
                &|| false,
            )
            .unwrap(),
        PlanSnapshotResult::NotFound
    ));

    assert!(matches!(
        source
            .plan(PlanBasis::Fresh, "plan_example".to_string(), &|| false)
            .unwrap(),
        PlanSnapshotResult::Found(_)
    ));
    let after_fresh = source
        .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
        .unwrap();
    assert_eq!(after_fresh.recorder.epoch_id, second.recorder.epoch_id);

    fs::write(root.path().join(".agent/state/plans.jsonl"), "").unwrap();
    let third = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert!(matches!(
        source
            .plan(
                PlanBasis::RecorderEpoch(third.recorder.epoch_id),
                "plan_example".to_string(),
                &|| false,
            )
            .unwrap(),
        PlanSnapshotResult::NotFound
    ));
}

#[test]
fn typed_gate_and_loop_fields_reach_the_recorder_without_json_reparse() {
    let (_root, source) = source_fixture();
    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let plan = &refresh.recorder.open_plans[0];
    let gate = &plan.gates.as_ref().unwrap().gates.items()[0];
    assert_eq!(gate.id, "custom");
    assert_eq!(gate.tool.as_deref(), Some("jig.custom_check"));
    assert_eq!(gate.remediation.as_ref().unwrap().argv[4], "plan_example");
    let loops = refresh.recorder.loops.as_ref().unwrap();
    assert!(!loops.workflows.items().is_empty());
}

#[test]
fn real_loop_attempt_identity_and_recovery_argv_survive_the_source_boundary() {
    let (root, source) = source_fixture();
    let workflow_id = "workflow with space;printf injected";
    let item_key = "item $(touch nope)";
    let key = format!("{workflow_id}:{item_key}");
    let cache = root.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("attempts.json"),
        serde_json::to_vec(&json!({
            "attempts": {
                key.clone(): {
                    "key": key,
                    "workflow_id": workflow_id,
                    "item_key": item_key,
                    "attempts": 3,
                    "max_attempts": 3,
                    "last_attempt_ms": 10,
                    "next_eligible_ms": 20,
                    "exhausted": true,
                    "last_status": "failed"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let attempt = &refresh
        .recorder
        .loops
        .as_ref()
        .unwrap()
        .needs_attention
        .exhausted_attempts
        .items()[0];
    assert_eq!(attempt.workflow_id, workflow_id);
    assert_eq!(attempt.item_key, item_key);
    assert_eq!(
        attempt.remediation.as_ref().unwrap().argv,
        vec![
            "scripts/jig",
            "loop",
            "clear-attempt",
            "--workflow",
            workflow_id,
            "--item",
            item_key,
        ]
    );
    assert!(attempt.remediation.as_ref().unwrap().display.contains("'"));
    assert!(!root.path().join("nope").exists());
}

#[test]
fn local_epoch_traverses_each_state_stream_once_and_gates_do_not_rescan() {
    let (_root, source) = source_fixture();
    crate::state::reset_dashboard_scan_counts();
    crate::state::reset_work_gate_receipt_index_scan_count();
    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let context = &source.context;

    for stream in [
        "sessions.jsonl",
        "plans.jsonl",
        "decisions.jsonl",
        "receipts.jsonl",
    ] {
        assert_eq!(
            crate::state::dashboard_scan_count(&context.state_file(stream)),
            1,
            "{stream} should be traversed exactly once"
        );
    }
    assert_eq!(crate::state::work_gate_receipt_index_scan_count(), 0);
    assert_eq!(refresh.recorder.open_plans.len(), 1);
}

#[test]
fn typed_status_source_is_semantically_equal_to_legacy_status_for_supported_state() {
    let (root, source) = source_fixture();
    for index in 0..12 {
        record_receipt(
            &source.context,
            ReceiptInput {
                tool_name: "jig.custom_check",
                args: json!({}),
                invoked_command_key: Some("custom_check_command".to_string()),
                plan_id: Some("plan_example".to_string()),
                started_at_ms: 100,
                ended_at_ms: if index == 11 { 5 } else { 200 },
                exit_status: 0,
                stdout: "ok",
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
    let decisions = (0..12)
        .map(|index| {
            format!(
                "{}\n",
                json!({
                    "id": format!("decision-{index:02}"),
                    "session_id": null,
                    "plan_id": "plan_example",
                    "timestamp_ms": if index == 11 { 5 } else { 200 },
                    "title": format!("Decision {index}"),
                    "selected_option": "A",
                    "alternatives": [],
                    "rationale": "because"
                })
            )
        })
        .collect::<String>();
    fs::write(root.path().join(".agent/state/decisions.jsonl"), decisions).unwrap();
    let plans_path = root.path().join(".agent/state/plans.jsonl");
    let mut plans = fs::read_to_string(&plans_path).unwrap();
    plans.push_str(&format!(
        "{}\n",
        json!({
            "id": "plan-append-different-path",
            "plan_id": "plan_example",
            "event": "append",
            "timestamp_ms": 300,
            "body_path": ".agent/plans/different.md"
        })
    ));
    fs::write(plans_path, plans).unwrap();
    let config_path = root.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "/tmp/template",
        &root.path().join("template").display().to_string(),
    );
    fs::write(config_path, config).unwrap();
    let source = RepoDashboardSource::new(RepoContext::load_from(root.path()).unwrap());
    let _clock = crate::state::set_test_now_ms(1_900_000_000_000);
    let legacy = crate::status::snapshot_with_cancellation(&source.context, &|| false).unwrap();
    let typed = source
        .status(
            StatusRequest {
                timeline_limit: TimelineLimit::new(25).unwrap(),
            },
            &|_| {},
            &|| false,
        )
        .unwrap();
    let typed = serde_json::to_value(typed.status).unwrap();

    assert_eq!(typed, legacy);
    assert_eq!(
        typed["work"]["state"]["recent_receipts"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    assert_eq!(
        typed["work"]["state"]["recent_decisions"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    assert_eq!(
        typed["work"]["state"]["repo"]["source_path"],
        "<repository-root>/template"
    );
}

#[test]
fn typed_status_preserves_legacy_gate_then_loop_error_order() {
    let (root, source) = source_fixture();
    let plans_path = root.path().join(".agent/state/plans.jsonl");
    let mut plans = fs::read_to_string(&plans_path).unwrap();
    plans.push_str(&format!(
        "{}\n",
        json!({
            "id": "plan-open-duplicate-status",
            "plan_id": "plan_example",
            "event": "open",
            "timestamp_ms": 50,
            "title": "Duplicate",
            "body_path": null,
            "baseline": null
        })
    ));
    fs::write(plans_path, plans).unwrap();
    let loop_cache = root.path().join(".agent/.cache/loop");
    fs::create_dir_all(&loop_cache).unwrap();
    fs::write(loop_cache.join("attempts.json"), "not-json").unwrap();
    let _clock = crate::state::set_test_now_ms(1_900_000_000_000);

    let legacy = crate::status::snapshot_with_cancellation(&source.context, &|| false).unwrap();
    let typed = source
        .status(
            StatusRequest {
                timeline_limit: TimelineLimit::new(25).unwrap(),
            },
            &|_| {},
            &|| false,
        )
        .unwrap();
    let typed = serde_json::to_value(typed.status).unwrap();

    assert_eq!(typed, legacy);
    let relevant_scopes = typed["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|error| error["scope"].as_str())
        .filter(|scope| scope.starts_with("work.gates") || *scope == "loops")
        .collect::<Vec<_>>();
    assert_eq!(relevant_scopes, ["work.gates.plan_example", "loops"]);
}

#[cfg(unix)]
#[test]
fn provider_raw_extensions_round_trip_and_provider_mutations_precede_local_epoch() {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .config(
            r#"
[[status.providers]]
id = "example.provider"
argv = ["sh", "provider.sh"]
timeout_seconds = 2
"#,
        )
        .write();
    let report = json!({
        "protocol": "jig.status-provider/v1",
        "provider": {
            "id": "example.provider",
            "adapter_version": "1.0.0",
            "future_provider_field": {"kept": true}
        },
        "observed_at_ms": 1_785_142_200_000_u64,
        "outcome": "complete",
        "inputs": [],
        "work_packages": [],
        "diagnostics": [],
        "future_report_field": ["kept", 7]
    });
    fs::write(
        root.path().join("provider-report.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    let appended = json!({
        "id": "plan-event-provider",
        "plan_id": "plan_from_provider",
        "event": "open",
        "timestamp_ms": 42,
        "title": "Provider-observed plan",
        "body_path": null,
        "baseline": null
    });
    fs::write(
        root.path().join("provider-plan.json"),
        format!("{}\n", serde_json::to_string(&appended).unwrap()),
    )
    .unwrap();
    fs::write(
        root.path().join("provider.sh"),
        "#!/bin/sh\ncat provider-report.json\ncat provider-plan.json >> .agent/state/plans.jsonl\n",
    )
    .unwrap();
    fs::create_dir_all(root.path().join(".agent/state")).unwrap();
    fs::write(root.path().join(".agent/state/plans.jsonl"), "").unwrap();
    let source = RepoDashboardSource::new(RepoContext::load_from(root.path()).unwrap());
    let phases = Mutex::new(Vec::new());
    let refresh = source
        .status(
            StatusRequest {
                timeline_limit: TimelineLimit::new(25).unwrap(),
            },
            &|phase| phases.lock().unwrap().push(phase),
            &|| false,
        )
        .unwrap();

    assert_eq!(
        *phases.lock().unwrap(),
        vec![StatusPhase::Providers, StatusPhase::LocalEpoch]
    );
    assert_eq!(
        refresh.status.providers[0].report.as_ref().unwrap().raw(),
        &report
    );
    assert!(
        refresh
            .status
            .work
            .state
            .as_ref()
            .unwrap()
            .open_plans
            .iter()
            .any(|plan| plan.plan_id == "plan_from_provider")
    );
    assert!(
        refresh
            .recorder
            .open_plans
            .iter()
            .any(|plan| plan.plan_id == "plan_from_provider")
    );
}

#[cfg(unix)]
#[test]
fn malformed_provider_is_typed_partial_data_not_a_local_epoch_failure() {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .config(
            r#"
[[status.providers]]
id = "example.provider"
argv = ["printf", "not-json"]
timeout_seconds = 2
"#,
        )
        .write();
    let source = RepoDashboardSource::new(RepoContext::load_from(root.path()).unwrap());
    let refresh = source
        .status(
            StatusRequest {
                timeline_limit: TimelineLimit::new(25).unwrap(),
            },
            &|_| {},
            &|| false,
        )
        .unwrap();

    assert_eq!(refresh.status.outcome, StatusOutcome::Partial);
    assert_eq!(refresh.status.providers[0].status, "failed");
    assert_eq!(
        refresh.status.providers[0].error.as_ref().unwrap().code,
        "invalid_json"
    );
    assert!(refresh.status.work.state.is_some());
}
