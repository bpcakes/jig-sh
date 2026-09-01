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
{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":1,"pageInfo":{"hasNextPage":false,"endCursor":"cursor-1"},"nodes":[{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src/lib.rs","line":42,"startLine":null,"originalLine":40,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{"totalCount":1,"nodes":[{"id":"PRRC_1","url":"https://github.com/acme/widgets/pull/7#discussion_r1","body":"Please add a test","createdAt":"2026-07-08T10:03:00Z","updatedAt":"2026-07-08T10:03:00Z","author":{"login":"reviewer"}}]}}]}}}}}
JSON
    ;;
  "api --method")
    printf '%s\n' '{"permission":"write"}'
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
    assert_eq!(output["observed"]["budget"]["request_count"], 5);
    assert_eq!(output["observed"]["budget"]["request_limit"], 256);
    assert_eq!(output["observed"]["budget"]["review_item_count"], 2);
    assert_eq!(output["observed"]["budget"]["review_item_limit"], 10_000);
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
        output["observed"]["pull_requests"][0]["review_threads"]["nodes"][0]["comments"]
            ["nodes"][0]["author"]["permission"],
        "write"
    );
    assert_eq!(
        output["observed"]["pull_requests"][0]["review_threads"]["summary"]
            ["trusted_unresolved"],
        1
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
      *ReviewThreadWitnessState*)
        cat <<'JSON'
{{"data":{{"node":{{"id":"PRRT_1","isResolved":false,"comments":{{"totalCount":2,"pageInfo":{{"hasPreviousPage":false,"startCursor":null}},"nodes":[{{"id":"PRRC_1","updatedAt":"2026-07-08T10:03:00Z","body":"Please fix this failing path"}},{{"id":"PRRC_REPLY","updatedAt":"2026-07-08T10:04:00Z","body":"Addressed by the pushed fix."}}]}}}}}}}}
JSON
        ;;
      *ReviewThreadState*)
        if [ -f .agent/.cache/gh-replied ]; then
          cat <<'JSON'
{{"data":{{"node":{{"id":"PRRT_1","isResolved":false,"comments":{{"totalCount":2,"pageInfo":{{"hasPreviousPage":false,"startCursor":null}},"nodes":[{{"id":"PRRC_REPLY"}}]}}}}}}}}
JSON
        else
          cat <<'JSON'
{{"data":{{"node":{{"id":"PRRT_1","isResolved":false,"comments":{{"totalCount":1,"pageInfo":{{"hasPreviousPage":false,"startCursor":null}},"nodes":[{{"id":"PRRC_1"}}]}}}}}}}}
JSON
        fi
        ;;
      *addPullRequestReviewThreadReply*)
        printf 'reply %s\n' "$*" >> gh-mutations.log
        : > .agent/.cache/gh-replied
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
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":1,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{{"totalCount":1,"nodes":[{{"id":"PRRC_1","url":"https://github.com/acme/demo/pull/7#discussion_r1","body":"Please fix this failing path","createdAt":"2026-07-08T10:03:00Z","updatedAt":"2026-07-08T10:03:00Z","author":{{"login":"reviewer"}}}}]}}}}]}}}}}}}}}}
JSON
        ;;
    esac
    ;;
  "api --method")
    printf '%s\n' '{{"permission":"write"}}'
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

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["workflow"]["kind"], "pr_manager");
    assert_eq!(output["workflow"]["codex_home_configured"], "./.codex-loop");
    assert_eq!(output["status"], "waiting");
    assert_eq!(output["actions"][0]["status"], "attempted", "{output:#}");
    assert_eq!(output["actions"][0]["worktree_retained"], false);
    let repair_worktree = output["actions"][0]["worktree"].as_str().unwrap();
    assert!(
        !Path::new(repair_worktree).exists(),
        "completed PR repair worktree was not removed: {repair_worktree}"
    );
    assert_eq!(
        output["actions"][0]["codex_home_resolved"],
        codex_home.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        fs::read_to_string(codex_home_log).unwrap(),
        codex_home.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(output["actions"][0]["push"]["pushed"], true);
    assert_eq!(output["actions"][0]["push"]["force"], true);
    assert_eq!(output["actions"][0]["push"]["force_with_lease"], true);
    assert_eq!(
        output["actions"][0]["push"]["expected_remote_head"],
        head_sha
    );
    assert!(
        output["actions"][0]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "failing_checks")
    );
    assert!(
        output["actions"][0]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "unresolved_review_threads")
    );
    assert!(output["actions"][0]["worker_receipt_id"].as_str().is_some());
    assert_eq!(
        output["actions"][0]["review_thread_posts"][0]["replied"],
        true
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][0]["reply_comment_id"],
        "PRRC_REPLY"
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][0]["is_resolved"],
        true
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][1]["status"],
        "skipped"
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][1]["reason"],
        "unknown_review_thread"
    );
    assert_eq!(output["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(output["attempts"][0]["item_key"], "pr-7");
    assert_eq!(
        output["attempts"][0]["item_version"],
        output["actions"][0]["push"]["final_head"]
    );
    assert_eq!(output["attempts"][0]["last_status"], "attempted");

    let pushed_src = git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]);
    assert!(pushed_src.contains("fixed by pr manager"));
    let gh_mutations = fs::read_to_string(temp.path().join("gh-mutations.log")).unwrap();
    assert!(gh_mutations.contains("addPullRequestReviewThreadReply"));
    assert!(gh_mutations.contains("resolveReviewThread"));
    assert!(!gh_mutations.contains("PRRT_FOREIGN"));

    let worker_receipts = crate::state::receipts_list(
        &ctx,
        crate::state::ReceiptListFilter {
            session_id: None,
            plan_id: None,
            tool_name: Some(WORKER_RUN_TOOL.into()),
            failed_only: false,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(worker_receipts["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(
        worker_receipts["receipts"][0]["evidence"]["purpose"],
        "pr_manager"
    );
    assert_eq!(
        worker_receipts["receipts"][0]["evidence"]["codex_home_resolved"],
        "<repository-root>/.codex-loop"
    );
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
    let actions = receipts["receipts"][0]["evidence"]["actions"]
        .as_array()
        .unwrap();
    assert_eq!(actions.len(), 1, "{receipts:#}");
    assert_eq!(actions[0]["kind"], "pr_manager_pre_execution");
    assert_eq!(actions[0]["status"], "failed");
    assert_eq!(actions[0]["unexecuted_reason"], "pre_execution_error");
    assert!(
        actions[0]["error"]
            .as_str()
            .is_some_and(|error| error.contains("Codex home does not exist")),
        "{receipts:#}"
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
    let head_sha = git_stdout(temp.path(), ["rev-parse", "codex/widgets"])
        .trim()
        .to_string();
    let _head_sha = EnvVarGuard::set("JIG_TEST_HEAD_SHA", &head_sha);
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
    cat <<JSON
[{"number":7,"title":"Fix widgets","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{"login":"octo"},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"$JIG_TEST_HEAD_SHA","headRepository":{"nameWithOwner":"acme/demo"},"headRepositoryOwner":{"login":"acme"},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}]
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
        printf '{"data":{"node":{"id":"%s","isResolved":false,"comments":{"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}\n' "$thread_id"
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
{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":3,"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{"totalCount":1,"nodes":[{"id":"PRRC_1","url":"https://github.com/acme/demo/pull/7#discussion_r1","body":"First thread","createdAt":"2026-07-08T10:03:00Z","updatedAt":"2026-07-08T10:03:00Z","author":{"login":"reviewer"}}]}},{"id":"PRRT_2","isResolved":false,"isOutdated":false,"path":"src.rs","line":2,"startLine":null,"originalLine":2,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{"totalCount":1,"nodes":[{"id":"PRRC_2","url":"https://github.com/acme/demo/pull/7#discussion_r2","body":"Second thread","createdAt":"2026-07-08T10:04:00Z","updatedAt":"2026-07-08T10:04:00Z","author":{"login":"reviewer"}}]}},{"id":"PRRT_3","isResolved":false,"isOutdated":false,"path":"src.rs","line":3,"startLine":null,"originalLine":3,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{"totalCount":1,"nodes":[{"id":"PRRC_3","url":"https://github.com/acme/demo/pull/7#discussion_r3","body":"Third thread","createdAt":"2026-07-08T10:05:00Z","updatedAt":"2026-07-08T10:05:00Z","author":{"login":"reviewer"}}]}}]}}}}}
JSON
        ;;
    esac
    ;;
  "api --method")
    printf '%s\n' '{"permission":"write"}'
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

    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["status"], "failed", "{output:#}");
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
fn loop_tick_pr_manager_resolves_merge_conflict_with_expected_head_lease() {
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
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":0,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[]}}}}}}}}}}
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
    assert_eq!(output["actions"][0]["push"]["force"], true);
    assert_eq!(output["actions"][0]["push"]["force_with_lease"], true);
    assert_eq!(
        output["actions"][0]["push"]["expected_remote_head"],
        head_sha
    );
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
