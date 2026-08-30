#[test]
fn loop_tick_waits_on_pending_attempt_backoff() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    write_attempt(&temp, 1, now_ms() + 60_000, false);
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
    assert_eq!(output["status"], "waiting");
    assert_eq!(output["idle"], false);
    assert_eq!(output["waiting_attempts"].as_array().unwrap().len(), 1);
    assert!(
        output["needs_attention"]["exhausted_attempts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn loop_status_surfaces_exhausted_attempts_as_needs_attention() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    write_attempt(&temp, 3, u64::MAX, true);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();
    assert_eq!(
        status["needs_attention"]["exhausted_attempts"][0]["item_key"],
        "item-1"
    );

    let tick = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Tick(LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        })),
    )
    .unwrap();
    assert_eq!(tick["status"], "needs_attention");
    assert_eq!(tick["ok"], false);
    assert_eq!(tick["idle"], false);

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
    assert_eq!(receipts["receipts"][0]["exit_status"], 1);
}

#[test]
fn dispatch_surfaces_machine_global_exhausted_attempt_without_poisoning_occurrence() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "scheduled-noop"
kind = "noop_status"
schedule = "* * * * *"
timezone = "UTC"
"#
        ),
    )
    .unwrap();
    write_attempt_for_workflow(&temp, "pr-manager", 3, u64::MAX, true);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Dispatch(LoopDispatchRequest {})),
    )
    .unwrap();

    assert_eq!(output["status"], "needs_attention", "{output:#}");
    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["needs_attention_count"], 0, "{output:#}");
    assert_eq!(output["exhausted_attempt_count"], 1, "{output:#}");
    assert_eq!(output["actions"][0]["occurrence"]["status"], "succeeded");
    assert_eq!(
        output["actions"][0]["tick"]["status"],
        "needs_attention",
        "machine-global attention remains visible in tick evidence"
    );
}

#[test]
fn dispatch_recovers_unparsable_attempt_state_before_work() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "scheduled-noop"
kind = "noop_status"
schedule = "* * * * *"
timezone = "UTC"
"#
        ),
    )
    .unwrap();
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("attempts.json"), b"not JSON").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Dispatch(LoopDispatchRequest {})),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["status"], "acted", "{output:#}");
    assert_eq!(output["state_error_count"], 0, "{output:#}");
    assert!(output["receipt_id"].as_str().is_some(), "{output:#}");
    assert_eq!(output["actions"][0]["status"], "succeeded", "{output:#}");
    serde_json::from_slice::<Value>(&fs::read(cache.join("attempts.json")).unwrap()).unwrap();
}

#[test]
fn dispatch_recovers_unparsable_attempt_state_once_for_all_workflows() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "first-scheduled-noop"
kind = "noop_status"
schedule = "* * * * *"
timezone = "UTC"

[[loop.workflows]]
id = "second-scheduled-noop"
kind = "noop_status"
schedule = "* * * * *"
timezone = "UTC"
"#
        ),
    )
    .unwrap();
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("attempts.json"), b"not JSON").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Dispatch(LoopDispatchRequest {})),
    )
    .unwrap();

    assert_eq!(output["state_error_count"], 0, "{output:#}");
    assert!(
        output["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["status"] == "succeeded"),
        "{output:#}"
    );
}

#[test]
fn dispatch_recovers_unparsable_cache_when_no_work_is_due() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("attempts.json"), b"not JSON").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Dispatch(LoopDispatchRequest {})),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["status"], "idle", "{output:#}");
    assert_eq!(output["failed_count"], 0, "{output:#}");
    assert_eq!(output["state_error_count"], 0, "{output:#}");
    assert!(output["actions"].as_array().unwrap().is_empty(), "{output:#}");
    assert!(output["receipt_id"].is_string(), "{output:#}");
    serde_json::from_slice::<Value>(&fs::read(cache.join("attempts.json")).unwrap()).unwrap();
}

#[test]
fn loop_clear_attempt_preserves_removed_alias_identity_and_records_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    write_attempt(&temp, 3, u64::MAX, true);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::ClearAttempt(LoopClearAttemptRequest {
            workflow: "noop-status".into(),
            item: "item-1".into(),
        })),
    )
    .unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(output["cleared"], true);
    assert_eq!(output["workflow"]["id"], "noop-status");
    assert_eq!(output["workflow"]["configured"], false);
    assert_eq!(output["workflow"]["removed"], true);
    assert_eq!(output["workflow_id"], "noop-status");
    assert!(output["receipt_id"].as_str().is_some());

    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();
    assert!(status["attempts"].as_array().unwrap().is_empty());
    assert!(
        status["needs_attention"]["exhausted_attempts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn loop_clear_attempt_rejects_empty_item_key() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::ClearAttempt(LoopClearAttemptRequest {
            workflow: "noop-status".into(),
            item: " ".into(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--item must not be empty"));
}

#[test]
fn loop_clear_attempt_rejects_empty_workflow_key() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::ClearAttempt(LoopClearAttemptRequest {
            workflow: " ".into(),
            item: "item-1".into(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--workflow must not be empty"));
}

fn write_attempt(temp: &tempfile::TempDir, attempts: u32, next_eligible_ms: u64, exhausted: bool) {
    write_attempt_for_workflow(
        temp,
        "noop-status",
        attempts,
        next_eligible_ms,
        exhausted,
    );
}

fn write_attempt_for_workflow(
    temp: &tempfile::TempDir,
    workflow_id: &str,
    attempts: u32,
    next_eligible_ms: u64,
    exhausted: bool,
) {
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("attempts.json"),
        serde_json::to_vec_pretty(&json!({
            "attempts": {
                format!("{workflow_id}:item-1"): {
                    "key": format!("{workflow_id}:item-1"),
                    "workflow_id": workflow_id,
                    "item_key": "item-1",
                    "attempts": attempts,
                    "max_attempts": 3,
                    "last_attempt_ms": now_ms(),
                    "next_eligible_ms": next_eligible_ms,
                    "exhausted": exhausted,
                    "last_status": "failed"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
}
