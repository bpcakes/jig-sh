use std::fs;

use jig_ui::dashboard::{
    DashboardSource, RecorderMode, RecorderRequest, SourceError, TimelineLimit,
};
use serde_json::json;
use tempfile::tempdir;

use crate::context::RepoContext;
use crate::state::{ReceiptInput, record_receipt};
use crate::test_env::TestRepoBuilder;

use super::RepoDashboardSource;

mod edge_cases;
mod limits;

fn source_fixture() -> (tempfile::TempDir, RepoDashboardSource) {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .config(
            r#"
[commands]
custom_check_command = "true"
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
    record_receipt(
        &context,
        ReceiptInput {
            tool_name: "jig.custom_check",
            args: json!({}),
            invoked_command_key: Some("custom_check_command".to_string()),
            started_at_ms: 10,
            ended_at_ms: 20,
            exit_status: 0,
            stdout: "ok",
            stderr: "",
            evidence: None,
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
    assert_eq!(first.recorder.timeline.len(), 1);

    crate::state::reset_dashboard_scan_counts();
    let reused = source
        .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
        .unwrap();
    assert_eq!(reused.recorder.epoch_id, first.recorder.epoch_id);
    assert_eq!(reused.status_local.epoch_id, first.status_local.epoch_id);
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("receipts.jsonl")),
        0,
        "ReuseCurrent must not traverse receipts"
    );
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
fn typed_loop_fields_reach_the_recorder_without_json_reparse() {
    let (_root, source) = source_fixture();
    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
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
fn recorder_status_projection_matches_local_status_command_data() {
    let (root, source) = source_fixture();
    for index in 0..12 {
        record_receipt(
            &source.context,
            ReceiptInput {
                tool_name: "jig.custom_check",
                args: json!({}),
                invoked_command_key: Some("custom_check_command".to_string()),
                started_at_ms: 100,
                ended_at_ms: if index == 11 { 5 } else { 200 },
                exit_status: 0,
                stdout: "ok",
                stderr: "",
                evidence: None,
                collect_git_metadata: false,
                collect_worktree_fingerprint: false,
                worktree_fingerprint_override: None,
            },
        )
        .unwrap();
    }
    let source = RepoDashboardSource::new(RepoContext::load_from(root.path()).unwrap());
    let _clock = crate::state::set_test_now_ms(1_900_000_000_000);
    let legacy = crate::status::snapshot_with_cancellation(&source.context, &|| false).unwrap();
    let typed = source
        .recorder(
            RecorderRequest {
                mode: RecorderMode::Refresh,
                timeline_limit: TimelineLimit::new(25).unwrap(),
            },
            &|| false,
        )
        .unwrap();
    let typed = serde_json::to_value(typed.status_local).unwrap();

    assert_eq!(typed["repository"], legacy["repository"]);
    assert_eq!(typed["loops"], legacy["loops"]);
    assert_eq!(typed["errors"], legacy["errors"]);
    assert!(typed.get("work").is_none());
    assert!(legacy.get("work").is_none());
}

#[test]
fn recorder_status_projection_matches_status_errors() {
    let (root, source) = source_fixture();
    let loop_cache = root.path().join(".agent/.cache/loop");
    fs::create_dir_all(&loop_cache).unwrap();
    fs::write(loop_cache.join("attempts.json"), "not-json").unwrap();
    let _clock = crate::state::set_test_now_ms(1_900_000_000_000);

    let legacy = crate::status::snapshot_with_cancellation(&source.context, &|| false).unwrap();
    let typed = source
        .recorder(
            RecorderRequest {
                mode: RecorderMode::Refresh,
                timeline_limit: TimelineLimit::new(25).unwrap(),
            },
            &|| false,
        )
        .unwrap();
    let typed = serde_json::to_value(typed.status_local).unwrap();

    assert_eq!(typed["errors"], legacy["errors"]);
    let relevant_scopes = typed["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|error| error["scope"].as_str())
        .filter(|scope| *scope != "repository")
        .collect::<Vec<_>>();
    assert_eq!(relevant_scopes, ["loops"]);
}

#[test]
fn local_epoch_traverses_receipts_once_and_ignores_legacy_streams() {
    let (_root, source) = source_fixture();
    let context = &source.context;
    for stream in ["sessions.jsonl", "plans.jsonl", "decisions.jsonl"] {
        fs::write(context.state_file(stream), "{}\n").unwrap();
    }
    crate::state::reset_dashboard_scan_counts();
    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();

    assert_eq!(
        crate::state::dashboard_scan_count(&context.state_file("receipts.jsonl")),
        1,
        "receipts should be traversed exactly once"
    );
    for stream in ["sessions.jsonl", "plans.jsonl", "decisions.jsonl"] {
        assert_eq!(
            crate::state::dashboard_scan_count(&context.state_file(stream)),
            0,
            "{stream} is no longer read"
        );
    }
    assert!(
        refresh
            .recorder
            .errors
            .iter()
            .all(|error| error.scope() == "repository")
    );
}
