use std::fs;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

use serde_json::json;
use tempfile::tempdir;

use crate::command::{
    LoopClearAttemptRequest, LoopCommand, LoopRunRequest, LoopStatusRequest, LoopTickRequest,
};
#[cfg(unix)]
use crate::runtime::tests::common::write_codex_stub;
use crate::runtime::tests::common::write_fixture_repo;
use crate::state::now_ms;
#[cfg(unix)]
use crate::test_env::{EnvVarGuard, lock_env};
use crate::tool_defs::LOOP_TICK_TOOL;
#[cfg(unix)]
use crate::tool_defs::WORKER_RUN_TOOL;

use super::*;

mod assertions;
use assertions::*;

#[test]
fn loop_tick_noop_records_idle_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command"], "loop tick");
    assert_eq!(output["status"], "idle");
    assert_eq!(output["idle"], true);
    assert_eq!(output["workflow"]["kind"], "noop_status");
    assert_eq!(output["observed"]["repo"]["name"], "demo");
    assert_eq!(output["observed"]["open_plan_count"], 1);
    assert!(output["receipt_id"].as_str().is_some());

    let receipts = crate::state::receipts_list(
        &ctx,
        crate::state::ReceiptListFilter {
            session_id: None,
            plan_id: None,
            tool_name: Some(LOOP_TICK_TOOL.into()),
            failed_only: false,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(receipts["receipts"].as_array().unwrap().len(), 1);
}

#[test]
fn loop_status_reports_default_workflow_and_empty_runtime_state() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command"], "loop status");
    assert_eq!(output["workflows"][0]["id"], "noop-status");
    assert_eq!(output["workflows"][0]["configured"], false);
    assert!(output["leases"].as_array().unwrap().is_empty());
    assert!(output["attempts"].as_array().unwrap().is_empty());
    assert!(output["waiting_attempts"].as_array().unwrap().is_empty());
    assert!(
        output["needs_attention"]["exhausted_attempts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn loop_status_accepts_noop_kind_alias() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest {
            workflow: Some("noop_status".into()),
        })),
    )
    .unwrap();

    assert_eq!(output["workflows"].as_array().unwrap().len(), 1);
    assert_eq!(output["workflows"][0]["id"], "noop-status");
    assert_eq!(output["workflows"][0]["kind"], "noop_status");
}

#[test]
fn loop_tick_rejects_unknown_workflow() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("missing".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Loop workflow not found: missing"));
}

#[test]
fn loop_tick_rejects_zero_backoff_override() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: Some(0),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("backoff_seconds must be greater than zero"));
}

#[test]
fn loop_run_until_idle_stops_after_one_noop_tick() {
    #[derive(Default)]
    struct PhaseObserver(Vec<(String, usize, usize)>);

    impl crate::execution::ExecutionObserver for PhaseObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { label, position } => {
                    self.0.push((
                        format!("started:{label}"),
                        position.current(),
                        position.total(),
                    ))
                }
                crate::execution::ExecutionEvent::PhaseFinished { label, .. } => {
                    self.0.push((format!("finished:{label}"), 0, 0));
                }
                _ => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for PhaseObserver {}

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = PhaseObserver::default();

    let output = crate::runtime::dispatch_with_observer(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Run(LoopRunRequest {
            workflow: Some("noop-status".into()),
            until: "idle".into(),
            max_ticks: 5,
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
        &mut observer,
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command"], "loop run");
    assert_eq!(output["status"], "idle");
    assert_eq!(output["tick_count"], 1);
    assert_eq!(output["ticks"][0]["status"], "idle");
    assert_eq!(
        observer.0,
        [
            ("started:loop tick".into(), 1, 5),
            ("finished:loop tick".into(), 0, 0)
        ]
    );
}

#[test]
fn loop_run_stops_when_tick_is_waiting() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("leases.json"),
        serde_json::to_vec_pretty(&json!({
            "leases": {
                "workflow:noop-status": {
                    "key": "workflow:noop-status",
                    "owner": "other-process",
                    "acquired_at_ms": now_ms(),
                    "expires_at_ms": now_ms() + 60_000,
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Run(LoopRunRequest {
            workflow: Some("noop-status".into()),
            until: "idle".into(),
            max_ticks: 5,
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["status"], "waiting");
    assert_eq!(output["tick_count"], 1);
    assert_eq!(output["ticks"][0]["status"], "waiting");
}

#[test]
fn loop_run_reports_disabled_workflow_instead_of_idle() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "disabled-status"
kind = "noop_status"
enabled = false
"#
        ),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Run(LoopRunRequest {
            workflow: Some("disabled-status".into()),
            until: "idle".into(),
            max_ticks: 5,
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["status"], "disabled");
    assert_eq!(output["tick_count"], 1);
    assert_eq!(output["ticks"][0]["status"], "disabled");
}

#[test]
fn loop_tick_waits_on_live_workflow_lease() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("leases.json"),
        serde_json::to_vec_pretty(&json!({
            "leases": {
                "workflow:noop-status": {
                    "key": "workflow:noop-status",
                    "owner": "other-process",
                    "acquired_at_ms": now_ms(),
                    "expires_at_ms": now_ms() + 60_000,
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["status"], "waiting");
    assert_eq!(output["idle"], false);
    assert_eq!(output["lease"]["owner"], "other-process");
    assert_eq!(output["live_leases"].as_array().unwrap().len(), 1);
}

#[test]
fn loop_tick_waits_on_pending_attempt_backoff() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("attempts.json"),
        serde_json::to_vec_pretty(&json!({
            "attempts": {
                "noop-status:item-1": {
                    "key": "noop-status:item-1",
                    "workflow_id": "noop-status",
                    "item_key": "item-1",
                    "attempts": 1,
                    "max_attempts": 3,
                    "last_attempt_ms": now_ms(),
                    "next_eligible_ms": now_ms() + 60_000,
                    "exhausted": false,
                    "last_status": "failed"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["status"], "waiting");
    assert_eq!(output["idle"], false);
    assert_eq!(output["waiting_attempts"].as_array().unwrap().len(), 1);
    assert!(
        output["needs_attention"]["exhausted_attempts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn loop_status_surfaces_exhausted_attempts_as_needs_attention() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("attempts.json"),
        serde_json::to_vec_pretty(&json!({
            "attempts": {
                "noop-status:item-1": {
                    "key": "noop-status:item-1",
                    "workflow_id": "noop-status",
                    "item_key": "item-1",
                    "attempts": 3,
                    "max_attempts": 3,
                    "last_attempt_ms": now_ms(),
                    "next_eligible_ms": u64::MAX,
                    "exhausted": true,
                    "last_status": "failed"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();
    assert_eq!(
        status["needs_attention"]["exhausted_attempts"][0]["item_key"],
        "item-1"
    );

    let tick = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();
    assert_eq!(tick["status"], "needs_attention");
    assert_eq!(tick["idle"], false);
}

#[test]
fn loop_clear_attempt_removes_attempt_record_and_records_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("attempts.json"),
        serde_json::to_vec_pretty(&json!({
            "attempts": {
                "noop-status:item-1": {
                    "key": "noop-status:item-1",
                    "workflow_id": "noop-status",
                    "item_key": "item-1",
                    "attempts": 3,
                    "max_attempts": 3,
                    "last_attempt_ms": now_ms(),
                    "next_eligible_ms": u64::MAX,
                    "exhausted": true,
                    "last_status": "failed"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::ClearAttempt(LoopClearAttemptRequest {
            workflow: "noop-status".into(),
            item: "item-1".into(),
        })),
    )
    .unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(output["cleared"], true);
    assert!(output["receipt_id"].as_str().is_some());

    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();
    assert!(status["attempts"].as_array().unwrap().is_empty());
    assert!(
        status["needs_attention"]["exhausted_attempts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn loop_clear_attempt_rejects_empty_item_key() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::ClearAttempt(LoopClearAttemptRequest {
            workflow: "noop-status".into(),
            item: " ".into(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--item must not be empty"));
}

#[test]
fn loop_tick_releases_lease_and_records_failed_receipt_on_workflow_error() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::write(temp.path().join(".agent/state/plans.jsonl"), "{not-json\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Loop workflow 'noop-status' failed; receipt"));

    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();
    assert!(status["leases"].as_array().unwrap().is_empty());

    let receipts = crate::state::receipts_list(
        &ctx,
        crate::state::ReceiptListFilter {
            session_id: None,
            plan_id: None,
            tool_name: Some(LOOP_TICK_TOOL.into()),
            failed_only: true,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(receipts["receipts"].as_array().unwrap().len(), 1);
}

#[test]
fn loop_configured_noop_workflow_uses_toml_tuning() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[loop]
lease_ttl_seconds = 120
max_attempts = 4
backoff_seconds = 30

[[loop.workflows]]
id = "status-check"
kind = "noop_status"
"#
        ),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest {
            workflow: Some("status-check".into()),
        })),
    )
    .unwrap();

    assert_eq!(output["workflows"].as_array().unwrap().len(), 1);
    assert_eq!(output["workflows"][0]["id"], "status-check");
    assert_eq!(output["workflows"][0]["configured"], true);
    assert_eq!(output["workflows"][0]["lease_ttl_seconds"], 120);
    assert_eq!(output["workflows"][0]["max_attempts"], 4);
    assert_eq!(output["workflows"][0]["backoff_seconds"], 30);
}

#[cfg(unix)]
#[test]
fn loop_tick_github_pr_status_records_normalized_read_only_snapshot() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_github_pr_status_workflow(temp.path());
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{"nameWithOwner":"acme/widgets","name":"widgets","owner":{"login":"acme"},"url":"https://github.com/acme/widgets","defaultBranchRef":{"name":"main"}}
JSON
    ;;
  "pr list")
    cat <<'JSON'
[{"number":7,"title":"Add widgets","url":"https://github.com/acme/widgets/pull/7","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"feature-base","headRefName":"codex/widgets","headRefOid":"abc123","headRepository":{"nameWithOwner":"acme/widgets"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}]
JSON
    ;;
  "pr checks")
    cat <<'JSON'
[{"bucket":"pass","completedAt":"2026-07-08T10:01:00Z","description":"ok","event":"pull_request","link":"https://github.com/acme/widgets/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"SUCCESS","workflow":"ci"},{"bucket":"pending","completedAt":null,"description":"running","event":"pull_request","link":"https://github.com/acme/widgets/actions/2","name":"lint","startedAt":"2026-07-08T10:02:00Z","state":"PENDING","workflow":"ci"}]
JSON
    exit 8
    ;;
  "api graphql")
    cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":"cursor-1"},"nodes":[{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src/lib.rs","line":42,"startLine":null,"originalLine":40,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{"totalCount":1,"nodes":[{"id":"PRRC_1","url":"https://github.com/acme/widgets/pull/7#discussion_r1","body":"Please add a test","createdAt":"2026-07-08T10:03:00Z","updatedAt":"2026-07-08T10:03:00Z","author":{"login":"reviewer"}}]}}]}}}}}
JSON
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["status"], "idle");
    assert!(output["actions"].as_array().unwrap().is_empty());
    assert_eq!(output["workflow"]["kind"], "github_pr_status");
    assert_eq!(output["observed"]["kind"], "github_pr_status_snapshot");
    assert_eq!(output["observed"]["schema_version"], 1);
    assert_eq!(
        output["observed"]["repository"]["name_with_owner"],
        "acme/widgets"
    );
    assert_eq!(output["observed"]["summary"]["open_pr_count"], 1);
    assert_eq!(output["observed"]["summary"]["pr_list_limit"], 100);
    assert_eq!(output["observed"]["summary"]["pr_list_truncated"], false);
    assert_eq!(output["observed"]["summary"]["pending_check_pr_count"], 1);
    assert_eq!(
        output["observed"]["summary"]["unresolved_review_thread_count"],
        1
    );
    assert_eq!(
        output["observed"]["pull_requests"][0]["checks"]["summary"]["pending"],
        1
    );
    assert_eq!(
        output["observed"]["pull_requests"][0]["review_threads"]["nodes"][0]["id"],
        "PRRT_1"
    );
    assert_eq!(
        output["observed"]["pull_requests"][0]["stack"]["is_stacked"],
        true
    );
}

#[cfg(unix)]
#[test]
fn loop_tick_github_pr_status_records_failed_receipt_when_gh_fails() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_github_pr_status_workflow(temp.path());
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
echo "authentication required" >&2
exit 4
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Loop workflow 'pr-status' failed; receipt"));
    assert!(
        error.contains(
            "gh repo view --json nameWithOwner,name,owner,url,defaultBranchRef failed with status 4"
        ),
        "{error}"
    );

    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();
    assert!(status["leases"].as_array().unwrap().is_empty());

    let receipts = crate::state::receipts_list(
        &ctx,
        crate::state::ReceiptListFilter {
            session_id: None,
            plan_id: None,
            tool_name: Some(LOOP_TICK_TOOL.into()),
            failed_only: true,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(receipts["receipts"].as_array().unwrap().len(), 1);
}
#[cfg(unix)]
#[test]
fn loop_tick_pr_manager_runs_worker_pushes_and_records_attempt() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join(".codex-loop");
    let codex_home_log = temp.path().join("codex-home.log");
    fs::create_dir(&codex_home).unwrap();
    append_pr_manager_workflow_with_home(temp.path(), Some("./.codex-loop"));
    let origin = setup_origin_with_pr_branch(temp.path());
    let head_sha = git_stdout(temp.path(), ["rev-parse", "codex/widgets"])
        .trim()
        .to_string();
    let _gh = fake_gh(
        temp.path(),
        &format!(
            r#"#!/bin/sh
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{{"nameWithOwner":"acme/demo","name":"demo","owner":{{"login":"acme"}},"url":"https://github.com/acme/demo","defaultBranchRef":{{"name":"main"}}}}
JSON
    ;;
  "pr list")
    cat <<'JSON'
[{{"number":7,"title":"Fix widgets","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{{"login":"octo"}},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"{head_sha}","headRepository":{{"nameWithOwner":"acme/demo"}},"headRepositoryOwner":{{"login":"acme"}},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}}]
JSON
    ;;
  "pr checks")
    cat <<'JSON'
[{{"bucket":"fail","completedAt":"2026-07-08T10:01:00Z","description":"tests failed","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"FAILURE","workflow":"ci"}}]
JSON
    ;;
  "api graphql")
    case "$*" in
      *ReviewThreadState*)
        cat <<'JSON'
{{"data":{{"node":{{"id":"PRRT_1","isResolved":false,"comments":{{"nodes":[]}}}}}}}}
JSON
        ;;
      *addPullRequestReviewThreadReply*)
        printf 'reply %s\n' "$*" >> gh-mutations.log
        cat <<'JSON'
{{"data":{{"addPullRequestReviewThreadReply":{{"comment":{{"id":"PRRC_REPLY","url":"https://github.com/acme/demo/pull/7#discussion_r2"}}}}}}}}
JSON
        ;;
      *resolveReviewThread*)
        printf 'resolve %s\n' "$*" >> gh-mutations.log
        cat <<'JSON'
{{"data":{{"resolveReviewThread":{{"thread":{{"id":"PRRT_1","isResolved":true}}}}}}}}
JSON
        ;;
      *)
        cat <<'JSON'
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{{"totalCount":1,"nodes":[{{"id":"PRRC_1","url":"https://github.com/acme/demo/pull/7#discussion_r1","body":"Please fix this failing path","createdAt":"2026-07-08T10:03:00Z","updatedAt":"2026-07-08T10:03:00Z","author":{{"login":"reviewer"}}}}]}}}}]}}}}}}}}}}
JSON
        ;;
    esac
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#
        ),
    );
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
printf '%s' "$CODEX_HOME" > "$JIG_TEST_CODEX_HOME_LOG"
if [ "$*" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral -" ]; then
  echo "old pr manager args should include output schema now" >&2
  exit 2
fi
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// fixed by pr manager\n' >> src.rs
  printf '{"summary":"fixed failing path","review_thread_replies":[{"thread_id":"PRRT_1","body":"Addressed by the pushed fix.","resolve":true},{"thread_id":"PRRT_FOREIGN","body":"Do not post this outside the observed PR.","resolve":true}]}\n' > "$out"
  printf 'worker ok\n'
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _codex_home_log = EnvVarGuard::set("JIG_TEST_CODEX_HOME_LOG", &codex_home_log);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(2),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();

    assert_pr_manager_tick_output(&output, &codex_home, &codex_home_log);
    assert_pr_manager_attempt_and_review_output(&output);
    assert_pr_manager_side_effects(temp.path(), &origin, &ctx);
}

#[cfg(unix)]
#[test]
fn invalid_pr_manager_codex_home_does_not_consume_attempt_budget() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow_with_home(temp.path(), Some("./missing-codex-home"));
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(1),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Codex home does not exist"), "{error}");
    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest {
            workflow: Some("pr-manager".into()),
        })),
    )
    .unwrap();
    assert!(status["attempts"].as_array().unwrap().is_empty());

    let receipts = crate::state::receipts_list(
        &ctx,
        crate::state::ReceiptListFilter {
            session_id: None,
            plan_id: None,
            tool_name: Some(LOOP_TICK_TOOL.into()),
            failed_only: true,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(receipts["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(receipts["receipts"][0]["evidence"]["observed"], Value::Null);
    assert!(
        receipts["receipts"][0]["evidence"]["actions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        receipts["receipts"][0]["evidence"]["attempts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn loop_tick_pr_manager_records_partial_review_post_failures() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    let ambient_codex_home = temp.path().join("ambient-codex-home");
    let codex_home_log = temp.path().join("ambient-codex-home.log");
    fs::create_dir(&ambient_codex_home).unwrap();
    let origin = setup_origin_with_pr_branch(temp.path());
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{"nameWithOwner":"acme/demo","name":"demo","owner":{"login":"acme"},"url":"https://github.com/acme/demo","defaultBranchRef":{"name":"main"}}
JSON
    ;;
  "pr list")
    cat <<'JSON'
[{"number":7,"title":"Fix widgets","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"abc123","headRepository":{"nameWithOwner":"acme/demo"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}]
JSON
    ;;
  "pr checks")
    cat <<'JSON'
[{"bucket":"fail","completedAt":"2026-07-08T10:01:00Z","description":"tests failed","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"FAILURE","workflow":"ci"}]
JSON
    ;;
  "api graphql")
    case "$*" in
      *ReviewThreadState*)
        thread_id=""
        case "$*" in
          *threadId=PRRT_1*) thread_id="PRRT_1" ;;
          *threadId=PRRT_2*) thread_id="PRRT_2" ;;
          *threadId=PRRT_3*) thread_id="PRRT_3" ;;
        esac
        printf '{"data":{"node":{"id":"%s","isResolved":false,"comments":{"nodes":[]}}}}\n' "$thread_id"
        ;;
      *addPullRequestReviewThreadReply*)
        case "$*" in
          *threadId=PRRT_1*)
            printf 'reply-1 %s\n' "$*" >> gh-mutations.log
            cat <<'JSON'
{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"PRRC_REPLY_1","url":"https://github.com/acme/demo/pull/7#discussion_r11"}}}}
JSON
            ;;
          *threadId=PRRT_2*)
            printf 'reply-2-failed %s\n' "$*" >> gh-mutations.log
            echo "secondary reply failed" >&2
            exit 4
            ;;
          *threadId=PRRT_3*)
            printf 'reply-3 %s\n' "$*" >> gh-mutations.log
            cat <<'JSON'
{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"PRRC_REPLY_3","url":"https://github.com/acme/demo/pull/7#discussion_r13"}}}}
JSON
            ;;
          *)
            echo "unexpected reply mutation: $*" >&2
            exit 2
            ;;
        esac
        ;;
      *)
        cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{"totalCount":1,"nodes":[{"id":"PRRC_1","url":"https://github.com/acme/demo/pull/7#discussion_r1","body":"First thread","createdAt":"2026-07-08T10:03:00Z","updatedAt":"2026-07-08T10:03:00Z","author":{"login":"reviewer"}}]}},{"id":"PRRT_2","isResolved":false,"isOutdated":false,"path":"src.rs","line":2,"startLine":null,"originalLine":2,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{"totalCount":1,"nodes":[{"id":"PRRC_2","url":"https://github.com/acme/demo/pull/7#discussion_r2","body":"Second thread","createdAt":"2026-07-08T10:04:00Z","updatedAt":"2026-07-08T10:04:00Z","author":{"login":"reviewer"}}]}},{"id":"PRRT_3","isResolved":false,"isOutdated":false,"path":"src.rs","line":3,"startLine":null,"originalLine":3,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{"totalCount":1,"nodes":[{"id":"PRRC_3","url":"https://github.com/acme/demo/pull/7#discussion_r3","body":"Third thread","createdAt":"2026-07-08T10:05:00Z","updatedAt":"2026-07-08T10:05:00Z","author":{"login":"reviewer"}}]}}]}}}}}
JSON
        ;;
    esac
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
    );
    let codex_path = temp.path().join("codex-partial-post-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
printf '%s' "$CODEX_HOME" > "$JIG_TEST_CODEX_HOME_LOG"
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// partial review posting\n' >> src.rs
  printf '{"summary":"fixed code and prepared replies","review_thread_replies":[{"thread_id":"PRRT_1","body":"First thread addressed.","resolve":false},{"thread_id":"PRRT_2","body":"Second thread addressed.","resolve":false},{"thread_id":"PRRT_3","body":"Third thread addressed.","resolve":false}]}\n' > "$out"
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &ambient_codex_home);
    let _codex_home_log = EnvVarGuard::set("JIG_TEST_CODEX_HOME_LOG", &codex_home_log);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(2),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["workflow"]["codex_home_configured"], Value::Null);
    assert_eq!(output["actions"][0]["status"], "failed", "{output:#}");
    assert_eq!(output["actions"][0]["codex_home_resolved"], Value::Null);
    assert_eq!(
        fs::read_to_string(codex_home_log).unwrap(),
        ambient_codex_home.display().to_string()
    );
    assert_eq!(
        output["actions"][0]["error"],
        "one or more review thread update intents failed"
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][0]["status"],
        "posted"
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][1]["status"],
        "failed"
    );
    assert!(
        output["actions"][0]["review_thread_posts"][1]["reply_error"]
            .as_str()
            .unwrap()
            .contains("secondary reply failed")
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][2]["status"],
        "posted"
    );
    assert_eq!(output["attempts"][0]["last_status"], "failed");

    let pushed_src = git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]);
    assert!(pushed_src.contains("partial review posting"));
    let gh_mutations = fs::read_to_string(temp.path().join("gh-mutations.log")).unwrap();
    assert!(gh_mutations.contains("reply-1"));
    assert!(gh_mutations.contains("reply-2-failed"));
    assert!(gh_mutations.contains("reply-3"));
}

#[cfg(unix)]
#[test]
fn loop_tick_pr_manager_resolves_merge_conflict_without_force_push() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    let origin = setup_origin_with_conflicting_pr_branch(temp.path());
    let head_sha = git_stdout(temp.path(), ["rev-parse", "codex/conflict"])
        .trim()
        .to_string();
    let _gh = fake_gh(
        temp.path(),
        &format!(
            r#"#!/bin/sh
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{{"nameWithOwner":"acme/demo","name":"demo","owner":{{"login":"acme"}},"url":"https://github.com/acme/demo","defaultBranchRef":{{"name":"main"}}}}
JSON
    ;;
  "pr list")
    cat <<'JSON'
[{{"number":9,"title":"Resolve conflict","url":"https://github.com/acme/demo/pull/9","state":"OPEN","isDraft":false,"author":{{"login":"octo"}},"baseRefName":"main","headRefName":"codex/conflict","headRefOid":"{head_sha}","headRepository":{{"nameWithOwner":"acme/demo"}},"headRepositoryOwner":{{"login":"acme"}},"isCrossRepository":false,"mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}}]
JSON
    ;;
  "pr checks")
    cat <<'JSON'
[{{"bucket":"pass","completedAt":"2026-07-08T10:01:00Z","description":"ok","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"SUCCESS","workflow":"ci"}}]
JSON
    ;;
  "api graphql")
    cat <<'JSON'
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[]}}}}}}}}}}
JSON
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#
        ),
    );
    let codex_path = temp.path().join("codex-conflict-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$*" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral -" ]; then
  echo "old pr manager args should include output schema now" >&2
  exit 2
fi
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  cat >/dev/null
  if ! grep -q '<<<<<<<' src.rs; then
    echo "expected conflict markers" >&2
    exit 3
  fi
  printf 'fn value() -> i32 { 4 }\n' > src.rs
  printf '{"summary":"resolved conflict","review_thread_replies":[]}\n' > "$out"
  printf 'conflict resolved\n'
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(2),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["status"], "waiting");
    assert_eq!(output["actions"][0]["status"], "attempted", "{output:#}");
    assert_eq!(output["actions"][0]["merge"]["conflicts"], true);
    assert_eq!(output["actions"][0]["push"]["pushed"], true);
    assert_eq!(output["actions"][0]["push"]["force"], false);
    assert!(
        output["actions"][0]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "merge_conflict")
    );

    let pushed_src = git_stdout(&origin, ["show", "refs/heads/codex/conflict:src.rs"]);
    assert_eq!(pushed_src, "fn value() -> i32 { 4 }\n");
}

#[cfg(unix)]
#[test]
fn loop_tick_pr_manager_bounds_repeated_ineffective_repairs_until_healthy() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    let origin = setup_origin_with_pr_branch(temp.path());
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
head_sha="$(git --git-dir .tmp-origin.git rev-parse refs/heads/codex/widgets)"
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{"nameWithOwner":"acme/demo","name":"demo","owner":{"login":"acme"},"url":"https://github.com/acme/demo","defaultBranchRef":{"name":"main"}}
JSON
    ;;
  "pr list")
    cat <<JSON
[{"number":7,"title":"Still failing","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"$head_sha","headRepository":{"nameWithOwner":"acme/demo"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}]
JSON
    ;;
  "pr checks")
    if [ -f .gh-healthy ]; then
      cat <<'JSON'
[{"bucket":"pass","completedAt":"2026-07-08T10:01:00Z","description":"ok","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"SUCCESS","workflow":"ci"}]
JSON
    elif [ -f .gh-pending ]; then
      cat <<'JSON'
[{"bucket":"pending","completedAt":null,"description":"running","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"PENDING","workflow":"ci"}]
JSON
    else
      cat <<'JSON'
[{"bucket":"fail","completedAt":"2026-07-08T10:01:00Z","description":"still failing","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"FAILURE","workflow":"ci"}]
JSON
    fi
    ;;
  "api graphql")
    cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}
JSON
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
    );
    let codex_path = temp.path().join("codex-ineffective-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$*" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral -" ]; then
  echo "old pr manager args should include output schema now" >&2
  exit 2
fi
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// ineffective repair\n' >> src.rs
  printf '{"summary":"attempted ineffective repair","review_thread_replies":[]}\n' > "$out"
  printf 'worker ok\n'
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let first = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(2),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(first["actions"][0]["status"], "attempted", "{first:#}");
    assert_eq!(first["attempts"][0]["attempts"], 1);
    assert_eq!(first["attempts"][0]["last_status"], "attempted");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(temp.path().join(".gh-pending"), "").unwrap();
    let pending = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(2),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(pending["status"], "waiting", "{pending:#}");
    assert_eq!(pending["actions"][0]["status"], "waiting", "{pending:#}");
    assert_eq!(pending["actions"][0]["reason"], "pending_checks");
    assert_eq!(pending["attempts"][0]["attempts"], 1);
    assert_eq!(pending["attempts"][0]["last_status"], "attempted");

    fs::remove_file(temp.path().join(".gh-pending")).unwrap();
    let second = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(2),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(second["status"], "needs_attention", "{second:#}");
    assert_eq!(second["actions"][0]["status"], "attempted", "{second:#}");
    assert_eq!(second["attempts"][0]["attempts"], 2);
    assert_eq!(second["attempts"][0]["exhausted"], true);
    assert_eq!(
        second["needs_attention"]["exhausted_attempts"][0]["item_key"],
        "pr-7"
    );

    let pushed_src = git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]);
    assert_eq!(pushed_src.matches("ineffective repair").count(), 2);

    fs::write(temp.path().join(".gh-healthy"), "").unwrap();
    let healthy = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(2),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(healthy["status"], "idle", "{healthy:#}");
    assert_eq!(healthy["actions"][0]["kind"], "pr_manager_attempt_clear");
    assert_eq!(healthy["actions"][0]["reason"], "observed_healthy");
    assert!(healthy["attempts"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn loop_tick_pr_manager_resets_attempt_budget_for_new_head_sha() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    let origin = setup_origin_with_pr_branch(temp.path());
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
head_sha="$(git --git-dir .tmp-origin.git rev-parse refs/heads/codex/widgets)"
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{"nameWithOwner":"acme/demo","name":"demo","owner":{"login":"acme"},"url":"https://github.com/acme/demo","defaultBranchRef":{"name":"main"}}
JSON
    ;;
  "pr list")
    cat <<JSON
[{"number":7,"title":"Still failing","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"$head_sha","headRepository":{"nameWithOwner":"acme/demo"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}]
JSON
    ;;
  "pr checks")
    cat <<'JSON'
[{"bucket":"fail","completedAt":"2026-07-08T10:01:00Z","description":"still failing","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"FAILURE","workflow":"ci"}]
JSON
    ;;
  "api graphql")
    cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}
JSON
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
    );
    let codex_path = temp.path().join("codex-reset-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// reset-budget repair\n' >> src.rs
  printf '{"summary":"attempted repair","review_thread_replies":[]}\n' > "$out"
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let first = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(1),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(first["status"], "needs_attention", "{first:#}");
    assert_eq!(first["attempts"][0]["attempts"], 1);
    assert_eq!(first["attempts"][0]["exhausted"], true);
    let first_final_head = first["actions"][0]["push"]["final_head"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(first["attempts"][0]["item_version"], first_final_head);

    let human = temp.path().join(".tmp-human");
    git_ok(
        temp.path(),
        ["clone", origin.to_str().unwrap(), human.to_str().unwrap()],
    );
    git_ok(&human, ["checkout", "codex/widgets"]);
    git_ok(&human, ["config", "user.email", "human@example.com"]);
    git_ok(&human, ["config", "user.name", "Human"]);
    fs::write(human.join("human.txt"), "human retry\n").unwrap();
    git_ok(&human, ["add", "human.txt"]);
    git_ok(&human, ["commit", "-m", "human retry"]);
    git_ok(&human, ["push", "origin", "codex/widgets"]);
    let human_head = git_stdout(&human, ["rev-parse", "codex/widgets"])
        .trim()
        .to_string();
    assert_ne!(human_head, first_final_head);

    let second = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(1),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(second["actions"][0]["status"], "attempted", "{second:#}");
    assert_eq!(second["attempts"][0]["attempts"], 1);
    assert_eq!(second["attempts"][0]["exhausted"], true);
    assert_eq!(
        second["attempts"][0]["item_version"],
        second["actions"][0]["push"]["final_head"]
    );

    let pushed_src = git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]);
    assert_eq!(pushed_src.matches("reset-budget repair").count(), 2);
}

#[cfg(unix)]
#[test]
fn loop_tick_pr_manager_continues_after_blocked_candidate() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    let origin = setup_origin_with_pr_branch(temp.path());
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("leases.json"),
        serde_json::to_vec_pretty(&json!({
            "leases": {
                "branch:codex/blocked": {
                    "key": "branch:codex/blocked",
                    "owner": "other-process",
                    "acquired_at_ms": now_ms(),
                    "expires_at_ms": now_ms() + 60_000,
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
widgets_sha="$(git --git-dir .tmp-origin.git rev-parse refs/heads/codex/widgets)"
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{"nameWithOwner":"acme/demo","name":"demo","owner":{"login":"acme"},"url":"https://github.com/acme/demo","defaultBranchRef":{"name":"main"}}
JSON
    ;;
  "pr list")
    cat <<JSON
[{"number":1,"title":"Blocked PR","url":"https://github.com/acme/demo/pull/1","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"main","headRefName":"codex/blocked","headRefOid":"blockedsha","headRepository":{"nameWithOwner":"acme/demo"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"},{"number":2,"title":"Runnable PR","url":"https://github.com/acme/demo/pull/2","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"$widgets_sha","headRepository":{"nameWithOwner":"acme/demo"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}]
JSON
    ;;
  "pr checks")
    cat <<'JSON'
[{"bucket":"fail","completedAt":"2026-07-08T10:01:00Z","description":"tests failed","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"FAILURE","workflow":"ci"}]
JSON
    ;;
  "api graphql")
    cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}
JSON
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
    );
    let codex_path = temp.path().join("codex-second-pr-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$*" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral -" ]; then
  echo "old pr manager args should include output schema now" >&2
  exit 2
fi
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// fixed after blocked candidate\n' >> src.rs
  printf '{"summary":"fixed after blocked candidate","review_thread_replies":[]}\n' > "$out"
  printf 'worker ok\n'
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(2),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["actions"].as_array().unwrap().len(), 2);
    assert_eq!(output["actions"][0]["status"], "waiting");
    assert_eq!(output["actions"][0]["pr_number"], 1);
    assert_eq!(output["actions"][1]["status"], "attempted", "{output:#}");
    assert_eq!(output["actions"][1]["pr_number"], 2);

    let pushed_src = git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]);
    assert!(pushed_src.contains("fixed after blocked candidate"));
}

#[cfg(unix)]
#[test]
fn loop_tick_pr_manager_cleans_failed_merge_worktree_before_retry() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join(".codex-loop");
    fs::create_dir(&codex_home).unwrap();
    append_pr_manager_workflow_with_home(temp.path(), Some("./.codex-loop"));
    let origin = setup_origin_with_conflicting_pr_branch(temp.path());
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
head_sha="$(git --git-dir .tmp-conflict-origin.git rev-parse refs/heads/codex/conflict)"
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{"nameWithOwner":"acme/demo","name":"demo","owner":{"login":"acme"},"url":"https://github.com/acme/demo","defaultBranchRef":{"name":"main"}}
JSON
    ;;
  "pr list")
    cat <<JSON
[{"number":9,"title":"Resolve conflict","url":"https://github.com/acme/demo/pull/9","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"main","headRefName":"codex/conflict","headRefOid":"$head_sha","headRepository":{"nameWithOwner":"acme/demo"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}]
JSON
    ;;
  "pr checks")
    cat <<'JSON'
[{"bucket":"pass","completedAt":"2026-07-08T10:01:00Z","description":"ok","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"SUCCESS","workflow":"ci"}]
JSON
    ;;
  "api graphql")
    cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}
JSON
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
    );
    let codex_path = temp.path().join("codex-fail-once-conflict-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$*" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral -" ]; then
  echo "old pr manager args should include output schema now" >&2
  exit 2
fi
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  cat >/dev/null
  sentinel="$(dirname "$0")/codex-failed-once"
  if [ ! -f "$sentinel" ]; then
    touch "$sentinel"
    echo "simulated worker failure" >&2
    exit 42
  fi
  if ! grep -q '<<<<<<<' src.rs; then
    echo "expected conflict markers after retry" >&2
    exit 3
  fi
  printf 'fn value() -> i32 { 5 }\n' > src.rs
  printf '{"summary":"resolved conflict after retry","review_thread_replies":[]}\n' > "$out"
  printf 'conflict resolved after retry\n'
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let first = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(3),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(first["actions"][0]["status"], "failed", "{first:#}");
    assert_eq!(
        first["actions"][0]["codex_home_resolved"],
        codex_home.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(first["attempts"][0]["attempts"], 1);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: Some(3),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(second["actions"][0]["status"], "attempted", "{second:#}");
    assert_eq!(second["actions"][0]["merge"]["conflicts"], true);
    assert_eq!(second["actions"][0]["push"]["pushed"], true);

    let pushed_src = git_stdout(&origin, ["show", "refs/heads/codex/conflict:src.rs"]);
    assert_eq!(pushed_src, "fn value() -> i32 { 5 }\n");
}

#[cfg(unix)]
#[test]
fn loop_tick_pr_manager_skips_stacked_prs_without_blocking_idle() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join(".codex-loop");
    fs::create_dir(&codex_home).unwrap();
    append_pr_manager_workflow_with_home(temp.path(), Some("./.codex-loop"));
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
case "$1 $2" in
  "repo view")
    cat <<'JSON'
{"nameWithOwner":"acme/demo","name":"demo","owner":{"login":"acme"},"url":"https://github.com/acme/demo","defaultBranchRef":{"name":"main"}}
JSON
    ;;
  "pr list")
    cat <<'JSON'
[{"number":8,"title":"Stacked work","url":"https://github.com/acme/demo/pull/8","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"feature-base","headRefName":"codex/stacked","headRefOid":"abc123","headRepository":{"nameWithOwner":"acme/demo"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","reviewDecision":"CHANGES_REQUESTED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}]
JSON
    ;;
  "pr checks")
    cat <<'JSON'
[{"bucket":"fail","completedAt":"2026-07-08T10:01:00Z","description":"tests failed","event":"pull_request","link":"https://github.com/acme/demo/actions/1","name":"test","startedAt":"2026-07-08T10:00:00Z","state":"FAILURE","workflow":"ci"}]
JSON
    ;;
  "api graphql")
    cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}
JSON
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["workflow"]["codex_home_configured"], "./.codex-loop");
    assert_eq!(output["status"], "idle");
    assert_eq!(output["idle"], true);
    assert_eq!(output["actions"][0]["status"], "skipped");
    assert_eq!(output["actions"][0]["reason"], "stacked_pr");
    assert!(
        output["actions"][0].get("codex_home_resolved").is_none(),
        "non-repair actions must omit codex_home_resolved: {output:#}"
    );
    assert!(output["attempts"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
fn append_github_pr_status_workflow(root: &Path) {
    let config = fs::read_to_string(root.join(".jig.toml")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "pr-status"
kind = "github_pr_status"
"#
        ),
    )
    .unwrap();
}

#[cfg(unix)]
fn append_pr_manager_workflow(root: &Path) {
    append_pr_manager_workflow_with_home(root, None);
}

#[cfg(unix)]
fn append_pr_manager_workflow_with_home(root: &Path, codex_home: Option<&str>) {
    let config = fs::read_to_string(root.join(".jig.toml")).unwrap();
    let codex_home = codex_home
        .map(|home| format!("codex_home = {home:?}\n"))
        .unwrap_or_default();
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "pr-manager"
kind = "pr_manager"
{codex_home}
"#
        ),
    )
    .unwrap();
}

#[cfg(unix)]
fn setup_origin_with_pr_branch(root: &Path) -> std::path::PathBuf {
    let origin = root.join(".tmp-origin.git");
    git_ok(root, ["init"]);
    git_ok(root, ["checkout", "-b", "main"]);
    git_ok(root, ["config", "user.email", "fixture@example.com"]);
    git_ok(root, ["config", "user.name", "Fixture"]);
    fs::write(root.join("src.rs"), "fn main() {}\n").unwrap();
    git_ok(root, ["add", "."]);
    git_ok(root, ["commit", "-m", "initial"]);
    git_ok(root, ["init", "--bare", origin.to_str().unwrap()]);
    git_ok(root, ["remote", "add", "origin", origin.to_str().unwrap()]);
    git_ok(root, ["push", "-u", "origin", "main"]);
    git_ok(root, ["checkout", "-b", "codex/widgets"]);
    fs::write(root.join("src.rs"), "fn main() { println!(\"widget\"); }\n").unwrap();
    git_ok(root, ["add", "src.rs"]);
    git_ok(root, ["commit", "-m", "widget work"]);
    git_ok(root, ["push", "-u", "origin", "codex/widgets"]);
    git_ok(root, ["checkout", "main"]);
    origin
}

#[cfg(unix)]
fn setup_origin_with_conflicting_pr_branch(root: &Path) -> std::path::PathBuf {
    let origin = root.join(".tmp-conflict-origin.git");
    git_ok(root, ["init"]);
    git_ok(root, ["checkout", "-b", "main"]);
    git_ok(root, ["config", "user.email", "fixture@example.com"]);
    git_ok(root, ["config", "user.name", "Fixture"]);
    fs::write(root.join("src.rs"), "fn value() -> i32 { 1 }\n").unwrap();
    git_ok(root, ["add", "."]);
    git_ok(root, ["commit", "-m", "initial"]);
    git_ok(root, ["init", "--bare", origin.to_str().unwrap()]);
    git_ok(root, ["remote", "add", "origin", origin.to_str().unwrap()]);
    git_ok(root, ["push", "-u", "origin", "main"]);
    git_ok(root, ["checkout", "-b", "codex/conflict"]);
    fs::write(root.join("src.rs"), "fn value() -> i32 { 2 }\n").unwrap();
    git_ok(root, ["add", "src.rs"]);
    git_ok(root, ["commit", "-m", "conflicting branch"]);
    git_ok(root, ["push", "-u", "origin", "codex/conflict"]);
    git_ok(root, ["checkout", "main"]);
    fs::write(root.join("src.rs"), "fn value() -> i32 { 3 }\n").unwrap();
    git_ok(root, ["add", "src.rs"]);
    git_ok(root, ["commit", "-m", "main conflict"]);
    git_ok(root, ["push", "origin", "main"]);
    origin
}

#[cfg(unix)]
fn git_ok<const N: usize>(cwd: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .current_dir(cwd)
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

#[cfg(unix)]
fn git_stdout<const N: usize>(cwd: &Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
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
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[cfg(unix)]
fn fake_gh(root: &std::path::Path, body: &str) -> EnvVarGuard {
    use std::os::unix::fs::PermissionsExt;

    let bin = root.join("fake-gh");
    fs::write(&bin, body).unwrap();
    let mut permissions = fs::metadata(&bin).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&bin, permissions).unwrap();
    EnvVarGuard::set("JIG_GH_BIN", bin.as_os_str())
}
