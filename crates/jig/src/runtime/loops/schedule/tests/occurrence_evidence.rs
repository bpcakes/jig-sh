use std::fs;

use jig_context::RepoContext;
use serde_json::Value;
use tempfile::tempdir;

use super::super::dispatch_due_at;
use crate::command::{LoopShowRequest, LoopTickRequest};
use crate::execution::NoopExecutionObserver;
use crate::runtime::loops::{engine, show};
use crate::test_env::TestRepoBuilder;

fn show(ctx: &RepoContext, occurrence_id: &str) -> Value {
    show::show_occurrence(
        ctx,
        LoopShowRequest {
            occurrence: occurrence_id.into(),
        },
        &|| false,
    )
    .unwrap()
}

#[test]
fn dispatched_occurrence_records_its_tick_evidence_instead_of_receipts() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
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
"#,
        ),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch_due_at(&ctx, super::timestamp("2026-08-21T08:42:30Z")).unwrap();

    assert!(output.get("receipt_id").is_none(), "{output:#}");
    let tick = &output["actions"][0]["tick"];
    assert_eq!(tick["command"], "loop tick");
    assert!(tick.get("receipt_id").is_none(), "{tick:#}");
    let occurrence_id = tick["occurrence_id"].as_str().unwrap();
    let shown = show(&ctx, occurrence_id);
    assert_eq!(shown["occurrence"]["status"], "succeeded", "{shown:#}");
    assert_eq!(shown["evidence"]["workflow_id"], "scheduled-noop");
    assert_eq!(shown["evidence"]["tick"]["status"], tick["status"]);
    assert_eq!(
        shown["evidence"]["tick"]["observed"], tick["observed"],
        "evidence keeps the full observation"
    );
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn manual_tick_keeps_its_occurrence_and_evidence_in_history() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = engine::tick_with_observer(
        &ctx,
        LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        },
        &mut NoopExecutionObserver,
    )
    .unwrap();

    let occurrence_id = output["occurrence_id"].as_str().unwrap();
    assert!(
        occurrence_id.starts_with("noop-status@manual:"),
        "{output:#}"
    );
    let shown = show(&ctx, occurrence_id);
    assert_eq!(shown["occurrence"]["status"], "succeeded", "{shown:#}");
    assert_eq!(shown["occurrence"]["worker_invoked"], false);
    assert_eq!(shown["evidence"]["tick"]["status"], output["status"]);
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn unknown_occurrence_is_reported_with_where_to_look() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = show::show_occurrence(
        &ctx,
        LoopShowRequest {
            occurrence: "example@1".into(),
        },
        &|| false,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Loop occurrence not found: example@1"),
        "{error:#}"
    );
    assert!(error.to_string().contains("jig loop status"), "{error:#}");
}
