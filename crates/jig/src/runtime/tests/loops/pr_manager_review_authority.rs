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
    case "$*" in
      *ReviewThreadWitnessState*)
        [ -f "$JIG_TEST_FEEDBACK_CHANGED" ] || exit 9
        cat <<'JSON'
{{"data":{{"node":{{"id":"PRRT_1","isResolved":false,"pullRequest":{{"headRefOid":"{head_sha}"}},"comments":{{"totalCount":1,"pageInfo":{{"hasPreviousPage":false,"startCursor":null}},"nodes":[{{"id":"PRRC_1","updatedAt":"2026-09-03T11:00:00Z","body":"Please cover the revised edge case"}}]}}}}}}}}
JSON
        ;;
      *)
        cat <<'JSON'
{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"totalCount":1,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{{"id":"PRRT_1","isResolved":false,"isOutdated":false,"path":"src.rs","line":1,"startLine":null,"originalLine":1,"originalStartLine":null,"subjectType":"LINE","diffSide":"RIGHT","startDiffSide":null,"viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":false,"resolvedBy":null,"comments":{{"totalCount":1,"nodes":[{{"id":"PRRC_1","url":"https://github.com/acme/demo/pull/7#discussion_r1","body":"Please cover the original edge case","createdAt":"2026-09-03T10:00:00Z","updatedAt":"2026-09-03T10:00:00Z","author":{{"login":"reviewer"}}}}]}}}}]}}}}}}}}}}
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
