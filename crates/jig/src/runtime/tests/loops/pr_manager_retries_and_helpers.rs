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
fn loop_run_pr_manager_preserves_failure_and_cleans_worktree_before_retry() {
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

    let run = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Run(LoopRunRequest {
            workflow: Some("pr-manager".into()),
            until: "idle".into(),
            max_ticks: 3,
            lease_ttl_seconds: None,
            max_attempts: Some(3),
            backoff_seconds: Some(1),
        })),
    )
    .unwrap();
    assert_eq!(run["ok"], false, "{run:#}");
    assert_eq!(run["status"], "failed", "{run:#}");
    assert_eq!(run["tick_count"], 2, "{run:#}");
    assert_eq!(run["ticks"][0]["actions"][0]["status"], "failed");
    assert_eq!(run["ticks"][1]["status"], "waiting");
    assert_eq!(
        run["ticks"][0]["actions"][0]["codex_home_resolved"],
        codex_home.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(run["ticks"][0]["attempts"][0]["attempts"], 1);

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
