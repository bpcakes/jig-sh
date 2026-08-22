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
    assert_eq!(tick["idle"], false);
}

#[test]
fn loop_clear_attempt_removes_attempt_record_and_records_receipt() {
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

fn write_attempt(temp: &tempfile::TempDir, attempts: u32, next_eligible_ms: u64, exhausted: bool) {
    let cache = temp.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("attempts.json"),
        serde_json::to_vec_pretty(&json!({
            "attempts": {
                "noop-status:item-1": {
                    "key": "noop-status:item-1",
                    "workflow_id": "noop-status",
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
