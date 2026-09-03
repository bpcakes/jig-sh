use std::fs;

use tempfile::tempdir;

use super::super::{NoopExecutionObserver, OccurrenceStore, dispatch_workflow, list_workflows};
use crate::context::RepoContext;
use crate::runtime::loops::occurrence::OccurrenceStatus;
use crate::test_env::TestRepoBuilder;

#[test]
fn successful_scheduled_work_requires_attention_when_its_tick_receipt_fails() {
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
"#,
        ),
    )
    .unwrap();
    fs::create_dir_all(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "scheduled-noop")
        .unwrap();
    let mut occurrences = OccurrenceStore::new(&ctx);

    let step = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        super::timestamp("2026-08-21T08:42:30Z"),
        &mut NoopExecutionObserver,
    );

    assert_eq!(step.executed_count, 1);
    assert_eq!(step.failed_count, 0);
    let action = step.action.as_ref().unwrap();
    assert_eq!(action["status"], "needs_attention", "{action:#}");
    assert_eq!(action["occurrence"]["status"], "needs_attention");
    assert!(
        action["occurrence"]["error"]
            .as_str()
            .is_some_and(|error| error.contains("Failed to record loop tick receipt")),
        "{action:#}"
    );
    let occurrence = occurrences.snapshot().unwrap().pop().unwrap();
    assert_eq!(occurrence.status, OccurrenceStatus::NeedsAttention);
}
