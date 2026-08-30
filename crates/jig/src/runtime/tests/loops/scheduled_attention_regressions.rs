#[cfg(unix)]
#[test]
fn scheduled_repo_task_blocks_after_leaving_the_shared_checkout_dirty() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "repo-task", "checkout = \"repo\"", false);
    let run_log = temp.path().join(".agent/.cache/repo-task-runs");
    let codex_path = bin.path().join("codex-repo-task-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
set -eu
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "-o" ]; then out="$arg"; fi
  prev="$arg"
done
cat >/dev/null
printf 'run\n' >> "$JIG_TEST_RUN_LOG"
printf 'scheduled change\n' > scheduled-change.txt
printf 'task complete\n' > "$out"
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _run_log = EnvVarGuard::set("JIG_TEST_RUN_LOG", run_log.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let dispatch_at = fixed_dispatch_time();

    let first = crate::runtime::loops::dispatch_due_at(&ctx, dispatch_at).unwrap();

    assert_eq!(first["status"], "needs_attention", "{first:#}");
    assert_eq!(first["needs_attention_count"], 1, "{first:#}");
    assert_eq!(first["actions"][0]["status"], "needs_attention");
    assert_eq!(
        first["actions"][0]["occurrence"]["status"],
        "needs_attention"
    );
    assert_eq!(
        first["actions"][0]["tick"]["actions"][0]["checkout"]["dirty"],
        true
    );
    assert!(
        first["actions"][0]["occurrence"]["worktree"].is_null(),
        "the main repository is not a linked worktree: {first:#}"
    );

    let second = crate::runtime::loops::dispatch_due_at(
        &ctx,
        dispatch_at.saturating_add(60_000),
    )
    .unwrap();

    assert_eq!(second["status"], "needs_attention", "{second:#}");
    assert_eq!(second["executed_count"], 0, "{second:#}");
    assert_eq!(second["actions"][0]["reason"], "occurrence_requires_attention");
    assert_eq!(fs::read_to_string(run_log).unwrap(), "run\n");
}

#[cfg(unix)]
#[test]
fn scheduled_pr_manager_preserves_unconfirmed_push_as_attention() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_scheduled_pr_manager_workflow(temp.path());
    setup_origin_with_pr_branch(temp.path());
    let _gh = fake_gh(
        temp.path(),
        r#"#!/bin/sh
head_sha="$(git --git-dir .tmp-origin.git rev-parse refs/heads/codex/widgets)"
case "$1 $2" in
  "repo view")
    printf '%s\n' '{"nameWithOwner":"example/project","name":"project","owner":{"login":"example"},"url":"https://example.invalid/project","defaultBranchRef":{"name":"main"}}'
    ;;
  "pr list")
    cat <<JSON
[{"number":7,"title":"Repair checks","url":"https://example.invalid/project/pull/7","state":"OPEN","isDraft":false,"author":{"login":"contributor"},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"$head_sha","headRepository":{"nameWithOwner":"example/project"},"headRepositoryOwner":{"login":"example"},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-08-30T10:00:00Z","createdAt":"2026-08-30T09:00:00Z"}]
JSON
    ;;
  "pr checks")
    printf '%s\n' '[{"bucket":"fail","completedAt":"2026-08-30T10:01:00Z","description":"failed","event":"pull_request","link":"https://example.invalid/check/1","name":"test","startedAt":"2026-08-30T10:00:00Z","state":"FAILURE","workflow":"ci"}]'
    ;;
  "api graphql")
    printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}'
    ;;
  *) exit 2 ;;
esac
"#,
    );
    let run_log = temp.path().join(".agent/.cache/pr-manager-runs");
    let codex_path = bin.path().join("codex-pr-manager-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
set -eu
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "-o" ]; then out="$arg"; fi
  prev="$arg"
done
cat >/dev/null
printf 'run\n' >> "$JIG_TEST_RUN_LOG"
printf '\n// scheduled repair\n' >> src.rs
printf '%s\n' '{"summary":"repaired checks","review_thread_replies":[]}' > "$out"
"#,
    );
    let git_path = bin.path().join("git-unconfirmed-push.sh");
    write_codex_stub(
        &git_path,
        r#"#!/bin/sh
case "$*" in
  *" push origin HEAD:refs/heads/codex/widgets") exit 9 ;;
  *" ls-remote --exit-code origin refs/heads/codex/widgets") exit 10 ;;
  *) exec git "$@" ;;
esac
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _git = EnvVarGuard::set("JIG_GIT_BIN", git_path.as_os_str());
    let _run_log = EnvVarGuard::set("JIG_TEST_RUN_LOG", run_log.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let dispatch_at = fixed_dispatch_time();

    let first = crate::runtime::loops::dispatch_due_at(&ctx, dispatch_at).unwrap();

    let action = &first["actions"][0];
    assert_eq!(first["status"], "needs_attention", "{first:#}");
    assert_eq!(first["needs_attention_count"], 1, "{first:#}");
    assert_eq!(action["status"], "needs_attention", "{first:#}");
    assert_eq!(action["occurrence"]["status"], "needs_attention");
    assert!(action["occurrence"]["worker_receipt_id"].is_string());
    let worktree = action["occurrence"]["worktree"]
        .as_str()
        .expect("ambiguous push must retain its diagnostic worktree");
    assert!(Path::new(worktree).exists());
    assert_eq!(action["tick"]["actions"][0]["attention_kind"], "ambiguous_push");
    assert_eq!(action["tick"]["actions"][0]["push"]["status"], "unconfirmed");
    assert!(
        action["occurrence"]["error"]
            .as_str()
            .is_some_and(|error| error.contains("push outcome was not confirmed")),
        "{first:#}"
    );

    let second = crate::runtime::loops::dispatch_due_at(
        &ctx,
        dispatch_at.saturating_add(60_000),
    )
    .unwrap();

    assert_eq!(second["status"], "needs_attention", "{second:#}");
    assert_eq!(second["executed_count"], 0, "{second:#}");
    assert_eq!(second["actions"][0]["reason"], "occurrence_requires_attention");
    assert_eq!(fs::read_to_string(run_log).unwrap(), "run\n");
}

#[cfg(unix)]
fn append_scheduled_pr_manager_workflow(root: &Path) {
    let config = fs::read_to_string(root.join(".jig.toml")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "pr-manager"
kind = "pr_manager"
schedule = "* * * * *"
"#
        ),
    )
    .unwrap();
}

#[cfg(unix)]
fn fixed_dispatch_time() -> u64 {
    u64::try_from(
        chrono::DateTime::parse_from_rfc3339("2026-08-30T12:34:30Z")
            .unwrap()
            .timestamp_millis(),
    )
    .unwrap()
}
