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
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("schedule.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "occurrences": {
                "noop-status@100": {
                    "occurrence_id": "noop-status@100",
                    "workflow_id": "noop-status",
                    "scheduled_at_ms": 100,
                    "owner": "finished-owner",
                    "claim_expires_at_ms": 200,
                    "started_at_ms": 100,
                    "finished_at_ms": 150,
                    "status": "succeeded"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
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
    assert_eq!(output["scheduled_occurrences"].as_array().unwrap().len(), 1);
    assert_eq!(
        output["scheduled_occurrences"][0]["workflow_id"],
        "noop-status"
    );
}

#[cfg(unix)]
#[test]
fn codex_task_uses_safe_defaults_and_removes_clean_successful_worktree() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let canonical_temp = fs::canonicalize(temp.path()).unwrap();
    write_fixture_repo(temp.path());
    git_ok(temp.path(), ["init"]);
    git_ok(temp.path(), ["config", "user.email", "fixture@example.com"]);
    git_ok(temp.path(), ["config", "user.name", "Fixture"]);
    git_ok(temp.path(), ["add", "."]);
    git_ok(temp.path(), ["commit", "-m", "fixture"]);
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(
        temp.path().join("tasks/nightly.md"),
        "Review the repository.\n",
    )
    .unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "nightly-review"
kind = "codex_task"
schedule = "* * * * *"
timezone = "UTC"
prompt_file = "tasks/nightly.md"
"#
        ),
    )
    .unwrap();
    let invocation_log = temp.path().join("codex-task-invocation.log");
    let prompt_log = temp.path().join("codex-task-prompt.log");
    let codex_path = temp.path().join("codex-task-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
printf '%s\n%s\n' "$PWD" "$*" > "$JIG_TEST_TASK_INVOCATION_LOG"
cat > "$JIG_TEST_TASK_PROMPT_LOG"
printf 'task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _invocation = EnvVarGuard::set("JIG_TEST_TASK_INVOCATION_LOG", &invocation_log);
    let _prompt = EnvVarGuard::set("JIG_TEST_TASK_PROMPT_LOG", &prompt_log);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("nightly-review".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["status"], "acted", "{output:#}");
    assert_eq!(output["actions"][0]["status"], "succeeded");
    assert_eq!(output["actions"][0]["checkout"]["mode"], "worktree");
    assert_eq!(output["actions"][0]["checkout"]["retained"], false);
    let worktree = output["actions"][0]["checkout"]["path"].as_str().unwrap();
    assert!(!Path::new(worktree).exists());
    let invocation = fs::read_to_string(invocation_log).unwrap();
    assert!(
        invocation.contains("--ask-for-approval never exec --sandbox read-only --ephemeral -"),
        "{invocation}"
    );
    let cwd = Path::new(invocation.lines().next().unwrap());
    let suffix = Path::new(worktree).strip_prefix(temp.path()).unwrap();
    let matches = cwd == Path::new(worktree) || cwd == canonical_temp.join(suffix);
    assert!(matches, "{invocation}");
    assert_eq!(
        fs::read_to_string(prompt_log).unwrap(),
        "Review the repository.\n"
    );
}

#[cfg(unix)]
#[test]
fn codex_task_rejects_prompt_symlink_that_escapes_repository() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(outside.path().join("prompt.md"), "outside\n").unwrap();
    symlink(
        outside.path().join("prompt.md"),
        temp.path().join("tasks/nightly.md"),
    )
    .unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "escaped-prompt"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = "tasks/nightly.md"
"#
        ),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("escaped-prompt".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("prompt must resolve inside the repository"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn loop_run_rejects_scheduled_codex_task_without_invoking_worker() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(temp.path().join("tasks/nightly.md"), "Run once.\n").unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "single-task"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = "tasks/nightly.md"
"#
        ),
    )
    .unwrap();
    let marker = temp.path().join("worker-invoked");
    let codex_path = temp.path().join("codex-must-not-run.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
touch "$JIG_TEST_TASK_COMPLETION_MARKER"
exit 0
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _marker = EnvVarGuard::set("JIG_TEST_TASK_COMPLETION_MARKER", marker.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Run(LoopRunRequest {
            workflow: Some("single-task".into()),
            until: "idle".into(),
            max_ticks: 10,
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("does not support `loop run`"), "{error}");
    assert!(!marker.exists(), "loop run must not start the Codex worker");
}

#[cfg(unix)]
#[test]
fn codex_task_retains_clean_worktree_when_worker_commits() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(
        temp.path().join("tasks/nightly.md"),
        "Update the repository.\n",
    )
    .unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "nightly-update"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = "tasks/nightly.md"
sandbox = "workspace-write"
"#
        ),
    )
    .unwrap();
    git_ok(temp.path(), ["init"]);
    git_ok(temp.path(), ["config", "user.email", "fixture@example.com"]);
    git_ok(temp.path(), ["config", "user.name", "Fixture"]);
    git_ok(temp.path(), ["add", "."]);
    git_ok(temp.path(), ["commit", "-m", "fixture"]);
    let initial_head = git_stdout(temp.path(), ["rev-parse", "HEAD"]);
    let codex_path = temp.path().join("codex-task-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
cat >/dev/null
printf 'committed task output\n' > committed-task-output.txt
git add committed-task-output.txt
git commit -m 'scheduled task change' >/dev/null
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("nightly-update".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    assert_eq!(output["actions"][0]["status"], "succeeded", "{output:#}");
    assert_eq!(output["actions"][0]["checkout"]["retained"], true);
    assert_eq!(output["actions"][0]["checkout"]["dirty"], false);
    assert_eq!(output["actions"][0]["checkout"]["head_changed"], true);
    let worktree = Path::new(output["actions"][0]["checkout"]["path"].as_str().unwrap());
    assert!(worktree.exists());
    assert!(
        worktree.starts_with(temp.path().join(".agent/runtime/loop/worktrees/tasks")),
        "retained worktree must not live under disposable cache: {}",
        worktree.display()
    );
    let cache = temp.path().join(".agent/.cache");
    if cache.exists() {
        fs::remove_dir_all(cache).unwrap();
    }
    assert!(
        worktree.exists(),
        "discarding cache must not remove retained task work"
    );
    assert_ne!(git_stdout(worktree, ["rev-parse", "HEAD"]), initial_head);
}

#[cfg(unix)]
#[test]
fn codex_task_reports_checkout_cleanup_failure_without_losing_worker_result() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(
        temp.path().join("tasks/nightly.md"),
        "Review the repository.\n",
    )
    .unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "nightly-review"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = "tasks/nightly.md"
"#
        ),
    )
    .unwrap();
    git_ok(temp.path(), ["init"]);
    git_ok(temp.path(), ["config", "user.email", "fixture@example.com"]);
    git_ok(temp.path(), ["config", "user.name", "Fixture"]);
    git_ok(temp.path(), ["add", "."]);
    git_ok(temp.path(), ["commit", "-m", "fixture"]);
    let git_failure_flag = temp.path().join("fail-worktree-git");
    let git_path = temp.path().join("git-task-stub.sh");
    write_codex_stub(
        &git_path,
        r#"#!/bin/sh
case "$PWD" in
  */.agent/runtime/loop/worktrees/tasks/*)
    if [ -f "$JIG_TEST_GIT_FAILURE_FLAG" ]; then
      echo 'injected worktree git failure' >&2
      exit 93
    fi
    ;;
esac
exec git "$@"
"#,
    );
    let codex_path = temp.path().join("codex-task-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
out=""
previous=""
for argument in "$@"; do
  if [ "$previous" = "-o" ]; then out="$argument"; fi
  previous="$argument"
done
cat >/dev/null
touch "$JIG_TEST_GIT_FAILURE_FLAG"
printf 'diagnostic task transcript\n'
printf 'authoritative task result\n' > "$out"
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _git = EnvVarGuard::set("JIG_GIT_BIN", git_path.as_os_str());
    let _git_failure = EnvVarGuard::set("JIG_TEST_GIT_FAILURE_FLAG", &git_failure_flag);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("nightly-review".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();

    let action = &output["actions"][0];
    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(action["status"], "failed");
    assert!(action["worker_receipt_id"].is_string());
    assert_eq!(action["output"], "authoritative task result\n");
    assert_eq!(action["provider_stdout"], "diagnostic task transcript\n");
    assert_eq!(action["checkout"]["retained"], true);
    assert!(action["checkout"]["dirty"].is_null());
    assert!(
        action["error"]
            .as_str()
            .is_some_and(|error| error.contains("Failed to inspect task worktree status")),
        "{output:#}"
    );
}

#[cfg(unix)]
#[test]
fn codex_task_cannot_succeed_after_workflow_lease_renewal_fails() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(
        temp.path().join("tasks/nightly.md"),
        "Review the repository.\n",
    )
    .unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "nightly-review"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = "tasks/nightly.md"
checkout = "repo"
lease_ttl_seconds = 1
"#
        ),
    )
    .unwrap();
    git_ok(temp.path(), ["init"]);
    git_ok(temp.path(), ["config", "user.email", "fixture@example.com"]);
    git_ok(temp.path(), ["config", "user.name", "Fixture"]);
    git_ok(temp.path(), ["add", "."]);
    git_ok(temp.path(), ["commit", "-m", "fixture"]);
    let codex_path = temp.path().join("codex-task-stub.sh");
    let completion_marker = temp.path().join("worker-completed");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
cat >/dev/null
rm -f "$JIG_TEST_TASK_REPO/.agent/.cache/loop/leases.json"
sleep 1
touch "$JIG_TEST_TASK_COMPLETION_MARKER"
printf 'task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _repo = EnvVarGuard::set("JIG_TEST_TASK_REPO", temp.path().as_os_str());
    let _completion = EnvVarGuard::set(
        "JIG_TEST_TASK_COMPLETION_MARKER",
        completion_marker.as_os_str(),
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("nightly-review".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("Loop workflow lease renewal or release failed"),
        "{error}"
    );
    assert!(
        !completion_marker.exists(),
        "worker should be terminated as soon as lease renewal fails"
    );
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
                crate::execution::ExecutionEvent::PhaseStarted { label, position } => self.0.push((
                    format!("started:{label}"),
                    position.current(),
                    position.total(),
                )),
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
