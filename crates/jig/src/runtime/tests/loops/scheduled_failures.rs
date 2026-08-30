#[cfg(unix)]
#[test]
fn scheduled_worker_observes_its_published_running_claim_before_start() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "nightly-review", "checkout = \"repo\"", false);
    let marker = temp.path().join("worker-observed-durable-claim");
    let codex_path = temp.path().join("codex-check-claim.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
set -eu
grep -q '"status": "running"' "$JIG_TEST_TASK_REPO/.agent/runtime/loop/schedule.json"
touch "$JIG_TEST_TASK_COMPLETION_MARKER"
cat >/dev/null
printf 'task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _repo = EnvVarGuard::set("JIG_TEST_TASK_REPO", temp.path().as_os_str());
    let _marker = EnvVarGuard::set("JIG_TEST_TASK_COMPLETION_MARKER", marker.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch_loop(&ctx);

    assert_eq!(output["ok"], true, "{output:#}");
    assert!(marker.exists(), "worker must start after claim publication");
}

#[cfg(unix)]
#[test]
fn scheduled_lease_failure_preserves_worker_receipt_and_retained_worktree() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "nightly-review", "", true);
    let codex_path = temp.path().join("codex-task-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
cat >/dev/null
rm -f "$JIG_TEST_TASK_REPO/.agent/.cache/loop/leases.json"
sleep 1
printf 'task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _repo = EnvVarGuard::set("JIG_TEST_TASK_REPO", temp.path().as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch_loop(&ctx);

    let occurrence = &output["actions"][0]["occurrence"];
    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(occurrence["status"], "needs_attention", "{output:#}");
    assert!(occurrence["worker_receipt_id"].is_string(), "{output:#}");
    let worktree = occurrence["worktree"]
        .as_str()
        .expect("cancelled isolated worker must retain its worktree");
    assert!(Path::new(worktree).exists());
    assert!(
        occurrence["error"]
            .as_str()
            .is_some_and(|error| error.contains("cancelled while the worker was running")),
        "{output:#}"
    );
    assert!(
        output["actions"][0]["tick"]["release_warning"]
            .as_str()
            .is_some_and(|error| error.contains("Loop lease is no longer held")),
        "{output:#}"
    );
}

#[cfg(unix)]
#[test]
fn scheduled_codex_invocation_failure_links_worker_receipt() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "nightly-review", "", false);
    let missing_codex = temp.path().join("missing-codex");
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", missing_codex.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch_loop(&ctx);

    assert_eq!(output["ok"], false, "{output:#}");
    let occurrence_receipt = output["actions"][0]["occurrence"]["worker_receipt_id"]
        .as_str()
        .expect("failed occurrence must link its worker receipt");
    assert_eq!(
        output["actions"][0]["tick"]["actions"][0]["worker_receipt_id"],
        occurrence_receipt
    );
    let retained = output["actions"][0]["occurrence"]["worktree"]
        .as_str()
        .expect("a failed isolated task must retain its worktree");
    assert_eq!(
        output["actions"][0]["tick"]["actions"][0]["checkout"]["path"],
        retained
    );
    assert!(Path::new(retained).exists());
}

#[cfg(unix)]
#[test]
fn post_work_state_failure_preserves_successful_worker_evidence() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "nightly-review", "", false);
    let codex_path = temp.path().join("codex-task-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
set -eu
cat >/dev/null
printf 'retained worker change\n' > worker-result.txt
mkdir -p "$JIG_TEST_TASK_REPO/.agent/.cache/loop"
printf 'not JSON\n' > "$JIG_TEST_TASK_REPO/.agent/.cache/loop/attempts.json"
printf 'task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _repo = EnvVarGuard::set("JIG_TEST_TASK_REPO", temp.path().as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch_loop(&ctx);

    let action = &output["actions"][0];
    let occurrence = &action["occurrence"];
    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["state_errors"][0]["kind"], "attempts", "{output:#}");
    assert_eq!(action["status"], "succeeded", "{output:#}");
    assert_eq!(occurrence["status"], "succeeded", "{output:#}");
    assert!(occurrence["worker_receipt_id"].is_string(), "{output:#}");
    let worktree = occurrence["worktree"]
        .as_str()
        .expect("successful dirty worker must retain its worktree");
    assert!(Path::new(worktree).join("worker-result.txt").is_file());
    assert_eq!(action["tick"]["state_errors"][0]["kind"], "attempts");
}

#[cfg(unix)]
#[test]
fn scheduled_dispatch_fails_closed_after_durable_ledger_loss() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(
        &temp,
        "broken-state",
        r#"checkout = "repo"

[[loop.workflows]]
id = "healthy-noop"
kind = "noop_status"
schedule = "* * * * *""#,
        true,
    );
    let codex_path = temp.path().join("codex-task-stub.sh");
    let completion_marker = temp.path().join("scheduled-worker-completed");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
cat >/dev/null
rm -f "$JIG_TEST_TASK_REPO/.agent/runtime/loop/schedule.json"
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

    let output = dispatch_loop(&ctx);

    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["failed_count"], 2, "{output:#}");
    assert_eq!(output["executed_count"], 1, "{output:#}");
    assert_eq!(output["actions"][0]["workflow_id"], "broken-state");
    assert_eq!(output["actions"][0]["status"], "failed");
    assert_eq!(output["actions"][0]["occurrence_state_persisted"], false);
    assert_eq!(output["actions"][0]["occurrence"]["status"], "running");
    assert_eq!(
        output["actions"][0]["state_error"],
        output["actions"][0]["error"]
    );
    assert!(
        output["actions"][0]["error"]
            .as_str()
            .is_some_and(|error| error.contains("Failed to finish scheduled occurrence")),
        "{output:#}"
    );
    assert_eq!(output["actions"][1]["workflow_id"], "healthy-noop");
    assert_eq!(output["actions"][1]["status"], "failed");
    assert!(
        output["actions"][1]["error"]
            .as_str()
            .is_some_and(|error| error.contains("Initialized loop schedule state is missing")),
        "{output:#}"
    );
    assert!(
        !completion_marker.exists(),
        "worker should be terminated as soon as occurrence renewal fails"
    );
}

#[cfg(unix)]
#[test]
fn missing_durable_ledger_behind_migration_marker_blocks_worker_launch() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "nightly-review", "", false);
    let legacy_dir = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&legacy_dir).unwrap();
    fs::write(
        legacy_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 3,
            "migrated_to": ".agent/runtime/loop/schedule.json",
            "occurrences": {}
        }))
        .unwrap(),
    )
    .unwrap();
    let marker = temp.path().join("worker-invoked");
    let codex_path = temp.path().join("codex-must-not-run.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\ntouch \"$JIG_TEST_TASK_COMPLETION_MARKER\"\n",
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _marker = EnvVarGuard::set("JIG_TEST_TASK_COMPLETION_MARKER", marker.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Dispatch(LoopDispatchRequest {})),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("migration marker exists without durable state"),
        "{error}"
    );
    assert!(!marker.exists(), "worker must not start without a durable claim");
}

#[cfg(unix)]
fn configure_scheduled_task(
    temp: &tempfile::TempDir,
    id: &str,
    extra_config: &str,
    one_second_lease: bool,
) {
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(
        temp.path().join("tasks/nightly.md"),
        "Review the repository.\n",
    )
    .unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    let lease = if one_second_lease {
        "lease_ttl_seconds = 1"
    } else {
        ""
    };
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "{id}"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = "tasks/nightly.md"
{lease}
{extra_config}
"#
        ),
    )
    .unwrap();
    git_ok(temp.path(), ["init"]);
    git_ok(temp.path(), ["config", "user.email", "fixture@example.com"]);
    git_ok(temp.path(), ["config", "user.name", "Fixture"]);
    git_ok(temp.path(), ["add", "."]);
    git_ok(temp.path(), ["commit", "-m", "fixture"]);
}

#[cfg(unix)]
fn dispatch_loop(ctx: &RepoContext) -> serde_json::Value {
    crate::runtime::dispatch(
        ctx,
        RuntimeCommand::Loop(LoopCommand::Dispatch(LoopDispatchRequest {})),
    )
    .unwrap()
}
