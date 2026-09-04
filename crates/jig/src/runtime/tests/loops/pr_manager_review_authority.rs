#[cfg(unix)]
#[test]
fn pr_manager_does_not_push_a_repair_based_on_changed_review_feedback() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    let origin = setup_origin_with_pr_branch(temp.path());
    let head_sha = git_stdout(temp.path(), ["rev-parse", "codex/widgets"])
        .trim()
        .to_string();
    let original_remote_src =
        git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]);
    let feedback_changed = temp.path().join("feedback-changed");
    let _gh = fake_gh(
        temp.path(),
        &format!(
            r#"#!/bin/sh
case "$1 $2" in
  "repo view")
    printf '%s\n' '{{"nameWithOwner":"acme/demo","name":"demo","owner":{{"login":"acme"}},"url":"https://github.com/acme/demo","defaultBranchRef":{{"name":"main"}}}}'
    ;;
  "pr list")
    cat <<'JSON'
[{{"number":7,"title":"Fix widgets","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{{"login":"octo"}},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"{head_sha}","headRepository":{{"name":"demo","nameWithOwner":"acme/demo"}},"headRepositoryOwner":{{"login":"acme"}},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"CHANGES_REQUESTED","statusCheckRollup":[],"updatedAt":"2026-09-03T10:00:00Z","createdAt":"2026-09-03T09:00:00Z"}}]
JSON
    ;;
  "pr checks")
    printf '%s\n' '[]'
    ;;
  "api graphql")
    if [ -f "$JIG_TEST_FEEDBACK_CHANGED" ]; then
      cat <<'JSON'
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":1,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{{"totalCount":1,"nodes":[{{"id":"PRRC_1","url":"https://github.com/acme/demo/pull/7#discussion_r1","body":"Please cover the revised edge case","createdAt":"2026-09-03T10:00:00Z","updatedAt":"2026-09-03T11:00:00Z","author":{{"login":"reviewer"}}}}]}}}}]}}}}}}}}}}
JSON
    else
      cat <<'JSON'
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":1,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{{"totalCount":1,"nodes":[{{"id":"PRRC_1","url":"https://github.com/acme/demo/pull/7#discussion_r1","body":"Please cover the original edge case","createdAt":"2026-09-03T10:00:00Z","updatedAt":"2026-09-03T10:00:00Z","author":{{"login":"reviewer"}}}}]}}}}]}}}}}}}}}}
JSON
    fi
    ;;
  "api --method")
    printf '%s\n' '{{"permission":"write"}}'
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
        ),
    );
    let codex_path = temp.path().join("codex-review-drift-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then out="$arg"; fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// repair based on the original feedback\n' >> src.rs
  : > "$JIG_TEST_FEEDBACK_CHANGED"
  printf '%s\n' '{"summary":"addressed original feedback","review_thread_replies":[]}' > "$out"
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _changed = EnvVarGuard::set(
        "JIG_TEST_FEEDBACK_CHANGED",
        feedback_changed.as_os_str(),
    );
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

    let action = &output["actions"][0];
    assert_eq!(output["status"], "needs_attention", "{output:#}");
    assert_eq!(action["status"], "needs_attention", "{output:#}");
    assert_eq!(
        action["attention_kind"],
        "review_feedback_changed_before_push"
    );
    assert_eq!(action["review_thread_revalidation"]["reason"], "review_thread_changed");
    assert_eq!(action["push"]["status"], "not_attempted");
    assert_eq!(action["worktree_retained"], true);
    assert!(Path::new(action["worktree"].as_str().unwrap()).exists());
    assert!(output["attempts"].as_array().unwrap().is_empty());
    assert_eq!(
        git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]),
        original_remote_src
    );
}

#[cfg(unix)]
#[test]
fn pr_manager_does_not_push_when_review_feedback_authority_changes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    let origin = setup_origin_with_pr_branch(temp.path());
    let head_sha = git_stdout(temp.path(), ["rev-parse", "codex/widgets"])
        .trim()
        .to_string();
    let original_remote_src =
        git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]);
    let authority_changed = temp.path().join("review-authority-changed");
    let _gh = fake_gh(
        temp.path(),
        &format!(
            r#"#!/bin/sh
case "$1 $2" in
  "repo view")
    printf '%s\n' '{{"nameWithOwner":"acme/demo","name":"demo","owner":{{"login":"acme"}},"url":"https://github.com/acme/demo","defaultBranchRef":{{"name":"main"}}}}'
    ;;
  "pr list")
    cat <<'JSON'
[{{"number":7,"title":"Fix widgets","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{{"login":"octo"}},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"{head_sha}","headRepository":{{"name":"demo","nameWithOwner":"acme/demo"}},"headRepositoryOwner":{{"login":"acme"}},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"CHANGES_REQUESTED","statusCheckRollup":[],"updatedAt":"2026-09-03T10:00:00Z","createdAt":"2026-09-03T09:00:00Z"}}]
JSON
    ;;
  "pr checks")
    printf '%s\n' '[]'
    ;;
  "api graphql")
    cat <<'JSON'
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":1,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{{"totalCount":2,"nodes":[{{"id":"PRRC_1","url":"https://github.com/acme/demo/pull/7#discussion_r1","body":"Please cover the authorization edge case","createdAt":"2026-09-03T10:00:00Z","updatedAt":"2026-09-03T10:00:00Z","author":{{"login":"reviewer-a"}}}},{{"id":"PRRC_2","url":"https://github.com/acme/demo/pull/7#discussion_r2","body":"Please keep the existing behavior","createdAt":"2026-09-03T10:01:00Z","updatedAt":"2026-09-03T10:01:00Z","author":{{"login":"reviewer-b"}}}}]}}}}]}}}}}}}}}}
JSON
    ;;
  "api --method")
    case "$*" in
      *"/reviewer-a/permission"*)
        if [ -f "$JIG_TEST_REVIEW_AUTHORITY_CHANGED" ]; then
          printf '%s\n' '{{"permission":"read"}}'
        else
          printf '%s\n' '{{"permission":"write"}}'
        fi
        ;;
      *"/reviewer-b/permission"*)
        printf '%s\n' '{{"permission":"write"}}'
        ;;
      *)
        echo "unexpected gh permission args: $*" >&2
        exit 2
        ;;
    esac
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
        ),
    );
    let codex_path = temp.path().join("codex-review-authority-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then out="$arg"; fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// repair based on the authorized feedback\n' >> src.rs
  : > "$JIG_TEST_REVIEW_AUTHORITY_CHANGED"
  printf '%s\n' '{"summary":"addressed authorized feedback","review_thread_replies":[]}' > "$out"
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _changed = EnvVarGuard::set(
        "JIG_TEST_REVIEW_AUTHORITY_CHANGED",
        authority_changed.as_os_str(),
    );
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

    let action = &output["actions"][0];
    assert_eq!(output["status"], "needs_attention", "{output:#}");
    assert_eq!(action["status"], "needs_attention", "{output:#}");
    assert_eq!(
        action["attention_kind"],
        "review_feedback_changed_before_push"
    );
    assert_eq!(
        action["review_thread_revalidation"]["reason"],
        "review_thread_changed"
    );
    assert_eq!(action["push"]["status"], "not_attempted");
    assert_eq!(action["worktree_retained"], true);
    assert!(Path::new(action["worktree"].as_str().unwrap()).exists());
    assert!(output["attempts"].as_array().unwrap().is_empty());
    assert_eq!(
        git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]),
        original_remote_src
    );
}

#[cfg(unix)]
#[test]
fn pr_manager_does_not_push_when_new_trusted_review_feedback_appears() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    let origin = setup_origin_with_pr_branch(temp.path());
    let head_sha = git_stdout(temp.path(), ["rev-parse", "codex/widgets"])
        .trim()
        .to_string();
    let original_remote_src =
        git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]);
    let feedback_added = temp.path().join("feedback-added");
    let _gh = fake_gh(
        temp.path(),
        &format!(
            r#"#!/bin/sh
case "$1 $2" in
  "repo view")
    printf '%s\n' '{{"nameWithOwner":"acme/demo","name":"demo","owner":{{"login":"acme"}},"url":"https://github.com/acme/demo","defaultBranchRef":{{"name":"main"}}}}'
    ;;
  "pr list")
    cat <<'JSON'
[{{"number":7,"title":"Fix widgets","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{{"login":"octo"}},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"{head_sha}","headRepository":{{"name":"demo","nameWithOwner":"acme/demo"}},"headRepositoryOwner":{{"login":"acme"}},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":null,"statusCheckRollup":[],"updatedAt":"2026-09-03T10:00:00Z","createdAt":"2026-09-03T09:00:00Z"}}]
JSON
    ;;
  "pr checks")
    printf '%s\n' '[{{"bucket":"fail","completedAt":"2026-09-03T10:01:00Z","description":"failed","event":"pull_request","link":"https://example.invalid/check/1","name":"test","startedAt":"2026-09-03T10:00:00Z","state":"FAILURE","workflow":"ci"}}]'
    ;;
  "api graphql")
    if [ -f "$JIG_TEST_FEEDBACK_ADDED" ]; then
      cat <<'JSON'
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":1,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{{"id":"PRRT_NEW","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{{"totalCount":1,"nodes":[{{"id":"PRRC_NEW","url":"https://github.com/acme/demo/pull/7#discussion_new","body":"Please cover the new edge case","createdAt":"2026-09-03T11:00:00Z","updatedAt":"2026-09-03T11:00:00Z","author":{{"login":"reviewer"}}}}]}}}}]}}}}}}}}}}
JSON
    else
      printf '%s\n' '{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":0,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[]}}}}}}}}}}'
    fi
    ;;
  "api --method")
    printf '%s\n' '{{"permission":"write"}}'
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
        ),
    );
    let codex_path = temp.path().join("codex-new-feedback-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then out="$arg"; fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// repair for the failed check\n' >> src.rs
  : > "$JIG_TEST_FEEDBACK_ADDED"
  printf '%s\n' '{"summary":"repaired the failed check","review_thread_replies":[]}' > "$out"
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _added = EnvVarGuard::set("JIG_TEST_FEEDBACK_ADDED", feedback_added.as_os_str());
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

    let action = &output["actions"][0];
    assert_eq!(output["status"], "needs_attention", "{output:#}");
    assert_eq!(action["status"], "needs_attention", "{output:#}");
    assert_eq!(
        action["attention_kind"],
        "review_feedback_changed_before_push"
    );
    assert_eq!(
        action["review_thread_revalidation"]["reason"],
        "review_thread_membership_changed"
    );
    assert_eq!(
        action["review_thread_revalidation"]["thread_id"],
        "PRRT_NEW"
    );
    assert_eq!(action["push"]["status"], "not_attempted");
    assert_eq!(action["worktree_retained"], true);
    assert!(Path::new(action["worktree"].as_str().unwrap()).exists());
    assert!(output["attempts"].as_array().unwrap().is_empty());
    assert_eq!(
        git_stdout(&origin, ["show", "refs/heads/codex/widgets:src.rs"]),
        original_remote_src
    );
}

#[cfg(unix)]
#[test]
fn pr_manager_does_not_commit_after_the_remote_pr_head_changes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
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
    printf '%s\n' '{{"nameWithOwner":"acme/demo","name":"demo","owner":{{"login":"acme"}},"url":"https://github.com/acme/demo","defaultBranchRef":{{"name":"main"}}}}'
    ;;
  "pr list")
    cat <<'JSON'
[{{"number":7,"title":"Fix widgets","url":"https://github.com/acme/demo/pull/7","state":"OPEN","isDraft":false,"author":{{"login":"octo"}},"baseRefName":"main","headRefName":"codex/widgets","headRefOid":"{head_sha}","headRepository":{{"name":"demo","nameWithOwner":"acme/demo"}},"headRepositoryOwner":{{"login":"acme"}},"isCrossRepository":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":null,"statusCheckRollup":[],"updatedAt":"2026-09-03T10:00:00Z","createdAt":"2026-09-03T09:00:00Z"}}]
JSON
    ;;
  "pr checks")
    printf '%s\n' '[{{"bucket":"fail","completedAt":"2026-09-03T10:01:00Z","description":"failed","event":"pull_request","link":"https://example.invalid/check/1","name":"test","startedAt":"2026-09-03T10:00:00Z","state":"FAILURE","workflow":"ci"}}]'
    ;;
  "api graphql")
    printf '%s\n' '{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":0,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[]}}}}}}}}}}'
    ;;
  *)
    echo "unexpected gh args: $*" >&2
    exit 2
    ;;
esac
"#,
        ),
    );
    let codex_path = temp.path().join("codex-head-change-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
if [ "$1 $2 $3 $4 $5 $6 $7" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral --output-schema" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then out="$arg"; fi
    prev="$arg"
  done
  cat >/dev/null
  printf '\n// repair for the failed check\n' >> src.rs
  git --git-dir "$JIG_TEST_ORIGIN" update-ref refs/heads/codex/widgets refs/heads/main
  printf '%s\n' '{"summary":"repaired the failed check","review_thread_replies":[]}' > "$out"
  exit 0
fi
echo "unexpected codex args: $*" >&2
exit 2
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _origin = EnvVarGuard::set("JIG_TEST_ORIGIN", origin.as_os_str());
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

    let action = &output["actions"][0];
    assert_eq!(output["status"], "needs_attention", "{output:#}");
    assert_eq!(action["status"], "needs_attention", "{output:#}");
    assert_eq!(action["attention_kind"], "pr_head_changed_before_push");
    assert_eq!(
        action["review_thread_revalidation"]["reason"],
        "pr_head_changed"
    );
    assert!(
        action["review_thread_revalidation"]["thread_id"].is_null(),
        "{output:#}"
    );
    assert_eq!(action["push"]["status"], "not_attempted");
    assert_eq!(action["worktree_retained"], true);
    assert!(Path::new(action["worktree"].as_str().unwrap()).exists());
    assert!(output["attempts"].as_array().unwrap().is_empty());
    assert_ne!(
        git_stdout(&origin, ["rev-parse", "refs/heads/codex/widgets"])
            .trim(),
        head_sha
    );
}
