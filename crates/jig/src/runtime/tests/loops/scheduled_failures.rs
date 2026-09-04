#[cfg(unix)]
#[test]
fn scheduled_worker_observes_its_published_running_claim_before_start() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "nightly-review", "checkout = \"repo\"", false);
    let marker = bin.path().join("worker-observed-durable-claim");
    let codex_path = bin.path().join("codex-check-claim.sh");
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
fn scheduled_pr_manager_rejects_corrupt_branch_authority_before_claiming() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_scheduled_pr_manager_workflow(temp.path());
    git_ok(temp.path(), ["init"]);
    let authority = temp.path().join(".git/jig/loop");
    fs::create_dir_all(&authority).unwrap();
    fs::write(authority.join("branch_leases.json"), b"{").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::loops::dispatch_due_at(&ctx, fixed_dispatch_time()).unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["due_count"], 1, "{output:#}");
    assert_eq!(output["executed_count"], 0, "{output:#}");
    assert!(
        output["actions"][0]["error"]
            .as_str()
            .is_some_and(|error| error.contains("branch_leases.json")),
        "{output:#}"
    );
    let schedule: serde_json::Value =
        serde_json::from_slice(&fs::read(authority.join("schedule.json")).unwrap()).unwrap();
    assert_eq!(schedule["occurrences"], json!({}), "{schedule:#}");
}

#[cfg(unix)]
#[test]
fn manual_pr_manager_rejects_corrupt_branch_authority_before_claiming() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    append_pr_manager_workflow(temp.path());
    git_ok(temp.path(), ["init"]);
    let authority = temp.path().join(".git/jig/loop");
    fs::create_dir_all(&authority).unwrap();
    fs::write(authority.join("branch_leases.json"), b"{").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("pr-manager".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap_err();

    assert!(format!("{error:#}").contains("branch_leases.json"));
    assert!(!authority.join("schedule.json").exists());
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
rm -f "$JIG_TEST_LEASE_PATH"
sleep 1
printf 'task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _repo = EnvVarGuard::set("JIG_TEST_TASK_REPO", temp.path().as_os_str());
    let lease_path = temp.path().join(".git/jig/loop/leases.json");
    let _lease_path = EnvVarGuard::set("JIG_TEST_LEASE_PATH", lease_path.as_os_str());
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
            .is_some_and(|error| error.contains(
                "Loop state migration marker exists without protected state"
            )),
        "{output:#}"
    );
}

#[cfg(unix)]
#[test]
fn scheduled_codex_start_failure_links_retry_receipt_without_consuming_occurrence() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    configure_scheduled_task(&temp, "nightly-review", "", false);
    let missing_codex = temp.path().join("missing-codex");
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", missing_codex.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch_loop(&ctx);

    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(
        output["actions"][0]["reason"],
        "pre_execution_error",
        "{output:#}"
    );
    assert_eq!(output["actions"][0]["retryable"], true, "{output:#}");
    assert_eq!(
        output["actions"][0]["occurrence_state_persisted"],
        false,
        "{output:#}"
    );
    let worker_receipt = output["actions"][0]["tick"]["actions"][0]["worker_receipt_id"]
        .as_str()
        .expect("pre-start worker failure must link its diagnostic receipt");
    assert!(worker_receipt.starts_with("receipt_"));
    let checkout = &output["actions"][0]["tick"]["actions"][0]["checkout"];
    assert_eq!(checkout["retained"], false, "{output:#}");
    let removed = checkout["path"]
        .as_str()
        .expect("worktree checkout path must be reported");
    assert!(!Path::new(removed).exists());
    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();
    assert_eq!(status["scheduled_occurrences"], json!([]));
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
printf 'not JSON\n' > "$JIG_TEST_ATTEMPTS_PATH"
printf 'task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _repo = EnvVarGuard::set("JIG_TEST_TASK_REPO", temp.path().as_os_str());
    let attempts_path = temp.path().join(".git/jig/loop/attempts.json");
    let _attempts_path =
        EnvVarGuard::set("JIG_TEST_ATTEMPTS_PATH", attempts_path.as_os_str());
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
fn scheduled_dispatch_ignores_worker_forged_checkout_schedule_replica() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin = tempdir().unwrap();
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
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace("schedule = \"* * * * *\"", "schedule = \"0 0 1 1 *\""),
    )
    .unwrap();
    git_ok(temp.path(), ["add", ".jig.toml"]);
    git_ok(temp.path(), ["commit", "-m", "stabilize fixture schedule"]);
    let codex_path = bin.path().join("codex-task-stub.sh");
    let completion_marker = bin.path().join("scheduled-worker-completed");
    let start_log = bin.path().join("scheduled-worker-starts");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
set -eu
cat >/dev/null
printf 'started\n' >> "$JIG_TEST_TASK_START_LOG"
durable_schedule="$JIG_TEST_TASK_REPO/.git/jig/loop/schedule.json"
baseline_schedule="$(mktemp "${TMPDIR:-/tmp}/jig-schedule.XXXXXX")"
trap 'rm -f "$baseline_schedule"' EXIT
cp "$durable_schedule" "$baseline_schedule"
printf '%s\n' '{"schema_version":4,"occurrences":{}}' \
  > "$JIG_TEST_TASK_REPO/.agent/runtime/loop/schedule.json"
printf '%s\n' 'not valid marker JSON' \
  > "$JIG_TEST_TASK_REPO/.agent/runtime/loop/schedule.initialized"
rm -f "$JIG_TEST_TASK_REPO/.agent/.cache/loop/schedule.json"
attempt=0
while cmp -s "$durable_schedule" "$baseline_schedule"; do
  attempt=$((attempt + 1))
  test "$attempt" -lt 100
  sleep 0.05
done
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
    let _starts = EnvVarGuard::set("JIG_TEST_TASK_START_LOG", start_log.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch_loop(&ctx);

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["failed_count"], 0, "{output:#}");
    assert_eq!(output["executed_count"], 2, "{output:#}");
    assert_eq!(output["actions"][0]["workflow_id"], "broken-state");
    assert_eq!(output["actions"][0]["status"], "succeeded");
    assert_eq!(output["actions"][0]["occurrence"]["status"], "succeeded");
    assert_eq!(output["actions"][1]["workflow_id"], "healthy-noop");
    assert_eq!(output["actions"][1]["status"], "succeeded");
    assert!(
        completion_marker.exists(),
        "checkout-local schedule forgery must not interrupt the worker"
    );
    assert_eq!(fs::read_to_string(&start_log).unwrap(), "started\n");

    let second = dispatch_loop(&ctx);
    assert_eq!(second["ok"], true, "{second:#}");
    assert_eq!(
        fs::read_to_string(&start_log).unwrap(),
        "started\n",
        "the protected Git-metadata ledger must prevent a duplicate worker start"
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
