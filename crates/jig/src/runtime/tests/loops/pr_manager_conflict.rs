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
[{{"number":9,"title":"Resolve conflict","url":"https://github.com/acme/demo/pull/9","state":"OPEN","isDraft":false,"author":{{"login":"octo"}},"baseRefName":"main","headRefName":"codex/conflict","headRefOid":"{head_sha}","headRepository":{{"name":"demo","nameWithOwner":"acme/demo"}},"headRepositoryOwner":{{"login":"acme"}},"isCrossRepository":false,"mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[],"updatedAt":"2026-07-08T10:00:00Z","createdAt":"2026-07-08T09:00:00Z"}}]
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
