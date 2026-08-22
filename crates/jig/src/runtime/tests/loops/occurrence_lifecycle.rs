#[test]
fn loop_acknowledge_occurrence_resolves_attention_without_reopening_the_schedule() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    write_occurrence(&temp, "needs_attention");
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = acknowledge_occurrence(&ctx).unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(output["changed"], true);
    assert_eq!(output["occurrence"]["status"], "acknowledged");
    assert!(output["receipt_id"].as_str().is_some());

    let repeated = acknowledge_occurrence(&ctx).unwrap();
    assert_eq!(repeated["changed"], false);

    let status = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
    )
    .unwrap();
    assert_eq!(status["scheduled_occurrences"][0]["status"], "acknowledged");
    assert!(
        status["needs_attention"]["scheduled_occurrences"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn loop_acknowledge_occurrence_rejects_non_attention_state() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    write_occurrence(&temp, "succeeded");
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = acknowledge_occurrence(&ctx).unwrap_err().to_string();

    assert!(error.contains("only needs_attention occurrences can be acknowledged"));
}

fn acknowledge_occurrence(ctx: &RepoContext) -> anyhow::Result<serde_json::Value> {
    crate::runtime::dispatch(
        ctx,
        RuntimeCommand::Loop(LoopCommand::AcknowledgeOccurrence(
            LoopAcknowledgeOccurrenceRequest {
                occurrence: "nightly@100".into(),
            },
        )),
    )
}

fn write_occurrence(temp: &tempfile::TempDir, status: &str) {
    let runtime_dir = temp.path().join(".agent/runtime/loop");
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::write(
        runtime_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 2,
            "occurrences": {
                "nightly@100": {
                    "occurrence_id": "nightly@100",
                    "workflow_id": "nightly",
                    "scheduled_at_ms": 100,
                    "owner": "owner",
                    "claim_expires_at_ms": 200,
                    "started_at_ms": 100,
                    "finished_at_ms": 200,
                    "status": status,
                    "error": (status == "needs_attention")
                        .then_some("ambiguous worker shutdown")
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
}
