use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;
use crate::state::jsonl::with_jsonl_write_lock;
use crate::state::receipts::target_receipt_status;
use crate::state::records::ReceiptRecord;

fn receipt(id: usize, dependency: Option<usize>) -> Value {
    let target: jig_contract::TargetId = format!("example:check-{id}").parse().unwrap();
    json!({
        "id": format!("receipt_example_{id}"), "target": target,
        "plan_id": "plan_example", "run_id": "run_example", "tool_name": "jig.target_run",
        "args": {}, "started_at_ms": 1, "ended_at_ms": 2, "exit_status": 0,
        "stdout_preview": "", "stderr_preview": "", "changed_paths": [],
        "diff_stat": {"files": 0, "insertions": 0, "deletions": 0},
        "target_freshness": {
            "schema_version": 1, "contract_epoch": 8, "state": "complete",
            "identity": {
                "contract_epoch": 8, "schema_version": 1, "digest_domain": TARGET_IDENTITY_DOMAIN,
                "target": target, "inputs_policy": "exhaustive", "source_state": "git", "source_digest": "source",
                "authority_digest": "authority", "dependency_digest": "dependencies",
                "identity_digest": "identity", "configuration_digest": "configuration",
                "runner_digest": "runner", "invocation_digest": "invocation", "source_preview": [],
                "source_entry_count": 0, "source_preview_truncated": false, "dependencies": [],
            },
            "dependency_execution_proof": dependency.into_iter().map(|id| json!({
                "target": format!("example:check-{id}").parse::<jig_contract::TargetId>().unwrap(), "receipt_id": format!("receipt_example_{id}"),
                "run_id": "run_example", "plan_id": "plan_example", "identity_digest": "identity",
                "conclusion": "success", "effective_valid_until_ms": null,
                "effective_requires_time_validity": false,
            })).collect::<Vec<_>>(),
            "effective_valid_until_ms": null, "effective_requires_time_validity": false,
            "global_execution_proof": {"state": "unknown"},
        },
    })
}

fn protect(records: &[Value], root: &Value) -> Result<(BTreeSet<String>, usize)> {
    let bytes = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    protect_bytes(bytes.as_bytes(), root)
}

fn protect_bytes(bytes: &[u8], root: &Value) -> Result<(BTreeSet<String>, usize)> {
    let temp = tempdir()?;
    let path = temp.path().join("receipts.jsonl");
    fs::write(&path, bytes)?;
    let root: ReceiptRecord = serde_json::from_value(root.clone())?;
    let root = target_receipt_status(&root, root.target.as_ref().unwrap());
    let mut protected = BTreeSet::new();
    originals::RECORD_VISITS.set(0);
    with_jsonl_write_lock(&path, |guard| {
        protect_dependencies(guard, &path, std::iter::once(root), &mut protected, 100)
    })?;
    assert_eq!(fs::read(path)?, bytes, "protection only reads the journal");
    Ok((protected, originals::RECORD_VISITS.get()))
}

#[test]
fn deep_chain_reads_journal_once_and_each_required_original_once() {
    const DEPTH: usize = 1_000;
    const HISTORY: usize = 4_000;
    let records = (0..HISTORY)
        .map(|id| receipt(id, (id + 1 < DEPTH).then_some(id + 1)))
        .collect::<Vec<_>>();
    let (protected, visits) = protect(&records, &records[0]).unwrap();
    let expected = (0..DEPTH)
        .map(|id| format!("receipt_example_{id}"))
        .collect();
    assert_eq!(protected, expected);
    assert_eq!(visits, HISTORY + DEPTH);
}

#[test]
fn cycles_load_each_original_once() {
    let records = [receipt(0, Some(1)), receipt(1, Some(0))];
    let (protected, visits) = protect(&records, &records[0]).unwrap();
    assert_eq!(protected.len(), 2);
    assert_eq!(visits, 4);
}

#[test]
fn conflicts_are_sticky_only_for_required_ids_including_unknown_fields() {
    for conflicting_id in [0, 1, 2] {
        let root = receipt(0, Some(1));
        let original = receipt(conflicting_id, (conflicting_id == 0).then_some(1));
        let mut conflicting = original.clone();
        conflicting["future_field"] = json!("different authority");
        let records = [
            root.clone(),
            receipt(1, None),
            original.clone(),
            conflicting,
            original,
        ];
        let result = protect(&records, &root);
        if conflicting_id == 2 {
            assert_eq!(result.unwrap().0.len(), 2);
        } else {
            assert!(format!("{:#}", result.unwrap_err()).contains("conflicting duplicate IDs"));
        }
    }
}

#[test]
fn whitespace_equivalent_duplicates_are_valid_originals() {
    let root = receipt(0, None);
    let bytes = format!("{root}\n  {root}  \n");
    assert_eq!(protect_bytes(bytes.as_bytes(), &root).unwrap().0.len(), 1);
}

#[test]
fn missing_unsupported_changed_and_unterminated_originals_refuse_protection() {
    let root = receipt(0, Some(1));
    assert!(
        format!(
            "{:#}",
            protect(std::slice::from_ref(&root), &root).unwrap_err()
        )
        .contains("missing")
    );
    let mut unsupported = receipt(1, None);
    unsupported["target_freshness"]["schema_version"] = json!(99);
    assert!(
        format!(
            "{:#}",
            protect(&[root.clone(), unsupported], &root).unwrap_err()
        )
        .contains("unsupported freshness schema")
    );
    let mut changed = root.clone();
    changed["ended_at_ms"] = json!(3);
    assert!(
        format!(
            "{:#}",
            protect(&[changed, receipt(1, None)], &root).unwrap_err()
        )
        .contains("Selected original receipt changed")
    );
    assert!(
        format!(
            "{:#}",
            protect_bytes(root.to_string().as_bytes(), &root).unwrap_err()
        )
        .contains("unterminated")
    );
}

#[test]
fn expired_original_does_not_pin_expired_dependency_proof() {
    let mut root = receipt(0, Some(1));
    root["target_freshness"]["effective_valid_until_ms"] = json!(99);
    root["target_freshness"]["effective_requires_time_validity"] = json!(true);
    let (protected, visits) = protect(std::slice::from_ref(&root), &root).unwrap();
    assert_eq!(protected, BTreeSet::from(["receipt_example_0".into()]));
    assert_eq!(visits, 2);
}
