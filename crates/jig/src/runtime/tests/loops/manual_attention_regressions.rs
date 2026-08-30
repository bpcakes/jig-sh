#[cfg(unix)]
#[test]
fn retained_manual_task_is_durable_and_blocks_manual_and_scheduled_reentry() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "manual-task", "checkout = \"worktree\"", false);
    let run_log = temp.path().join(".agent/.cache/manual-task-runs");
    let codex_path = bin.path().join("codex-manual-retention-stub.sh");
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
printf 'manual change\n' > manual-change.txt
printf 'task complete\n' > "$out"
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _run_log = EnvVarGuard::set("JIG_TEST_RUN_LOG", run_log.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let first = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("manual-task".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    let occurrence = &first["manual_occurrence"];
    assert_eq!(occurrence["status"], "succeeded", "{first:#}");
    let retained = occurrence["worktree"]
        .as_str()
        .expect("manual worktree must be linked from durable state");
    assert!(Path::new(retained).exists());

    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest {
            workflow: Some("manual-task".into()),
        })),
    )
    .unwrap();
    assert!(status["scheduled_occurrences"]
        .as_array()
        .is_some_and(|records| records.iter().any(|record| {
            record["occurrence_id"] == occurrence["occurrence_id"]
                && record["worktree"] == retained
        })));

    let second = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("manual-task".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap_err();
    assert!(second.to_string().contains("Retained worktree"));

    let scheduled =
        crate::runtime::loops::dispatch_due_at(&ctx, fixed_dispatch_time()).unwrap();
    assert_eq!(scheduled["executed_count"], 0, "{scheduled:#}");
    assert_eq!(
        scheduled["actions"][0]["reason"],
        "retained_worktree_requires_cleanup"
    );
    assert_eq!(fs::read_to_string(run_log).unwrap(), "run\n");
}

#[cfg(unix)]
#[test]
fn tick_and_run_report_attention_owned_by_another_workflow() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "repo-task", "checkout = \"repo\"", false);
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "observer"
kind = "noop_status"
"#
        ),
    )
    .unwrap();
    git_ok(temp.path(), ["add", ".jig.toml"]);
    git_ok(temp.path(), ["commit", "-m", "add observer workflow"]);
    let codex_path = bin.path().join("codex-global-attention-stub.sh");
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
printf 'scheduled change\n' > scheduled-change.txt
printf 'task complete\n' > "$out"
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let dispatch =
        crate::runtime::loops::dispatch_due_at(&ctx, fixed_dispatch_time()).unwrap();
    assert_eq!(dispatch["needs_attention_count"], 1, "{dispatch:#}");

    let tick = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("observer".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();
    assert_eq!(tick["status"], "needs_attention", "{tick:#}");
    assert_eq!(tick["ok"], false, "{tick:#}");
    assert_eq!(
        tick["needs_attention"]["scheduled_occurrences"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let run = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Run(LoopRunRequest {
            workflow: Some("observer".into()),
            until: "idle".into(),
            max_ticks: 3,
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();
    assert_eq!(run["status"], "needs_attention", "{run:#}");
    assert_eq!(run["ok"], false, "{run:#}");
}
