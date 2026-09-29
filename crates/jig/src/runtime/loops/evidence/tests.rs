use std::process::Command;

use tempfile::tempdir;

use super::*;
use crate::runtime::loops::occurrence::{OccurrenceClaim, ScheduleOccurrence};

fn fixture(git: bool) -> (tempfile::TempDir, RepoContext) {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    if git {
        let output = Command::new("git")
            .current_dir(temp.path())
            .args(["init", "-q"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    (temp, ctx)
}

fn claim(ctx: &RepoContext, scheduled_at_ms: u64) -> ScheduleOccurrence {
    match OccurrenceStore::new(ctx)
        .claim("example", scheduled_at_ms, 60)
        .unwrap()
    {
        OccurrenceClaim::Acquired(occurrence) => occurrence,
        _ => panic!("expected a new occurrence"),
    }
}

fn evidence_for(occurrence: &ScheduleOccurrence, tick: Value) -> OccurrenceEvidence {
    OccurrenceEvidence::new(&occurrence.occurrence_id, "example", 10, 20, tick)
}

#[test]
fn evidence_round_trips_outside_the_checkout() {
    for git in [true, false] {
        let (temp, ctx) = fixture(git);
        let occurrence = claim(&ctx, 1_000);
        let evidence = evidence_for(&occurrence, json!({"status": "acted", "actions": []}));

        record(&ctx, &evidence).unwrap();

        assert_eq!(
            read(&ctx, &occurrence.occurrence_id, &|| false).unwrap(),
            Some(evidence)
        );
        let path = path_for_test(&ctx, &occurrence.occurrence_id).unwrap();
        let expected_parent = if git {
            temp.path().join(".git/jig/loop/evidence")
        } else {
            temp.path().join(".agent/runtime/loop/evidence")
        };
        assert_eq!(path.parent(), Some(expected_parent.as_path()));
        assert!(read(&ctx, "example@2000", &|| false).unwrap().is_none());
    }
}

#[test]
fn evidence_is_dropped_once_its_occurrence_leaves_the_schedule() {
    let (_temp, ctx) = fixture(true);
    let abandoned = claim(&ctx, 1_000);
    record(&ctx, &evidence_for(&abandoned, json!({"status": "acted"}))).unwrap();
    OccurrenceStore::new(&ctx)
        .abandon_unexecuted(&abandoned.occurrence_id, &abandoned.owner)
        .unwrap();
    let retained = claim(&ctx, 2_000);

    record(&ctx, &evidence_for(&retained, json!({"status": "acted"}))).unwrap();

    assert!(
        read(&ctx, &abandoned.occurrence_id, &|| false)
            .unwrap()
            .is_none()
    );
    assert!(
        read(&ctx, &retained.occurrence_id, &|| false)
            .unwrap()
            .is_some()
    );
}

#[test]
fn oversized_evidence_drops_the_observed_snapshot_before_action_detail() {
    let (_temp, ctx) = fixture(true);
    let occurrence = claim(&ctx, 1_000);
    let tick = json!({
        "status": "acted",
        "observed": {"pull_requests": "x".repeat(MAX_EVIDENCE_BYTES)},
        "actions": [{"kind": "pr_manager_worker", "status": "attempted", "item_key": "pr-1"}],
    });

    record(&ctx, &evidence_for(&occurrence, tick)).unwrap();

    let stored = read(&ctx, &occurrence.occurrence_id, &|| false)
        .unwrap()
        .unwrap();
    assert_eq!(stored.tick["observed"]["omitted"], true);
    assert!(stored.tick["observed"]["bytes"].as_u64().unwrap() > MAX_EVIDENCE_BYTES as u64);
    assert_eq!(stored.tick["actions"][0]["item_key"], "pr-1");
    assert!(stored.tick["actions"][0].get("omitted").is_none());
}

#[test]
fn only_hash_named_json_files_are_evidence() {
    assert!(is_evidence_file_name(&file_name("example@1000")));
    for name in ["schedule.json", "example.json", ".tmp-evidence.json"] {
        assert!(!is_evidence_file_name(OsStr::new(name)), "{name}");
    }
    let upper = format!("{}.json", "A".repeat(64));
    assert!(!is_evidence_file_name(OsStr::new(&upper)));
}
