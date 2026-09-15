use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

use super::{RECEIPTS_STREAM, RUNS_STREAM};
use crate::context::RepoContext;
use crate::state::jsonl::{
    JsonlScanStats, JsonlWriteGuard, RawJsonlRecord, scan_jsonl_raw, scan_jsonl_raw_locked,
    with_jsonl_write_lock,
};
use crate::state::records::{ReceiptRecord, RunEventRecord};
use crate::state::tracker_operations::PendingTrackerRetentionRoots;

type RecordDigest = [u8; 32];

pub(super) fn unchanged_stream_snapshot(
    path: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
) -> Result<Option<crate::state::compression::GzipReadReport>> {
    with_jsonl_write_lock(path, |_guard| {
        let before = super::sha256_file_or_empty(path)?;
        Ok((before.uncompressed_bytes == expected_bytes
            && before.uncompressed_sha256 == expected_sha256)
            .then_some(before))
    })
}

pub(super) fn ensure_restored_stream_preserves_tracker_roots(
    ctx: &RepoContext,
    stream: &str,
    current_path: &Path,
    restored_path: &Path,
    guard: &JsonlWriteGuard,
    roots: &PendingTrackerRetentionRoots,
) -> Result<()> {
    let has_relevant_roots = match stream {
        RECEIPTS_STREAM => !roots.plan_ids.is_empty() || !roots.receipt_ids.is_empty(),
        RUNS_STREAM => !roots.plan_ids.is_empty() || !roots.run_ids.is_empty(),
        _ => return Ok(()),
    };
    if !has_relevant_roots {
        return Ok(());
    }

    let required = match stream {
        RECEIPTS_STREAM => protected_receipt_digests(ctx, current_path, guard, roots)?,
        RUNS_STREAM => protected_run_digests(current_path, guard, roots)?,
        _ => unreachable!("unsupported streams returned before protection scanning"),
    };
    if required.is_empty() {
        return Ok(());
    }

    let mut missing = required;
    let scan = scan_jsonl_raw(restored_path, &|| false, |record| {
        remove_digest(&mut missing, digest(record.bytes));
        Ok(())
    })?;
    refuse_unterminated(restored_path, scan)?;
    if !missing.is_empty() {
        let missing_records = missing.values().copied().sum::<u64>();
        bail!(
            "Refusing to replace {} because the restored {} backup omits {missing_records} record(s) protected by pending tracker operations",
            current_path.display(),
            stream
        );
    }
    Ok(())
}

fn protected_receipt_digests(
    ctx: &RepoContext,
    path: &Path,
    guard: &JsonlWriteGuard,
    roots: &PendingTrackerRetentionRoots,
) -> Result<BTreeMap<RecordDigest, u64>> {
    let protected_receipt_ids =
        crate::state::receipts::tracker_protected_receipt_ids(ctx, guard, path, roots)?;
    let mut required = BTreeMap::new();
    let scan = scan_jsonl_raw_locked(guard, path, &|| false, |record| {
        let receipt: ReceiptRecord = serde_json::from_slice(record.bytes).with_context(|| {
            format!(
                "Failed to inspect receipt record {} in {} for tracker restore protection",
                record.line_number,
                path.display()
            )
        })?;
        if protected_receipt_ids.contains(&receipt.id) {
            add_digest(&mut required, digest(record.bytes));
        }
        Ok(())
    })?;
    refuse_unterminated(path, scan)?;
    Ok(required)
}

fn protected_run_digests(
    path: &Path,
    guard: &JsonlWriteGuard,
    roots: &PendingTrackerRetentionRoots,
) -> Result<BTreeMap<RecordDigest, u64>> {
    let mut protected_run_ids = roots.run_ids.clone();
    let first_scan = scan_jsonl_raw_locked(guard, path, &|| false, |record| {
        let event = parse_run_event(record, path)?;
        if event
            .work_plan_id
            .as_ref()
            .is_some_and(|plan_id| roots.plan_ids.contains(plan_id))
        {
            protected_run_ids.insert(event.run_id);
        }
        Ok(())
    })?;
    refuse_unterminated(path, first_scan)?;

    let mut required = BTreeMap::new();
    let second_scan = scan_jsonl_raw_locked(guard, path, &|| false, |record| {
        let event = parse_run_event(record, path)?;
        if protected_run_ids.contains(&event.run_id) {
            add_digest(&mut required, digest(record.bytes));
        }
        Ok(())
    })?;
    refuse_unterminated(path, second_scan)?;
    Ok(required)
}

fn parse_run_event(record: RawJsonlRecord<'_>, path: &Path) -> Result<RunEventRecord> {
    serde_json::from_slice(record.bytes).with_context(|| {
        format!(
            "Failed to inspect run record {} in {} for tracker restore protection",
            record.line_number,
            path.display()
        )
    })
}

fn digest(bytes: &[u8]) -> RecordDigest {
    Sha256::digest(bytes).into()
}

fn add_digest(digests: &mut BTreeMap<RecordDigest, u64>, digest: RecordDigest) {
    *digests.entry(digest).or_default() += 1;
}

fn remove_digest(digests: &mut BTreeMap<RecordDigest, u64>, digest: RecordDigest) {
    let Some(count) = digests.get_mut(&digest) else {
        return;
    };
    *count -= 1;
    if *count == 0 {
        digests.remove(&digest);
    }
}

fn refuse_unterminated(path: &Path, scan: JsonlScanStats) -> Result<()> {
    ensure!(
        !scan.unterminated_final_record,
        "Refusing tracker restore protection authority because {} has an unterminated final record",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use crate::command::StateRestoreRequest;
    use crate::context::RepoContext;
    use crate::test_env::TestRepoBuilder;

    fn target_receipt(
        id: &str,
        plan_id: &str,
        target: &str,
        dependency: Option<(&str, &str, &str)>,
    ) -> Value {
        let target: jig_contract::TargetId = target.parse().unwrap();
        let target_freshness =
            dependency.map(|(dependency_id, dependency_plan, dependency_target)| {
                let dependency_target: jig_contract::TargetId = dependency_target.parse().unwrap();
                json!({
                    "schema_version": 1,
                    "contract_epoch": 8,
                    "state": "complete",
                    "identity": {
                        "contract_epoch": 8,
                        "schema_version": 1,
                        "digest_domain": jig_contract::freshness::TARGET_IDENTITY_DOMAIN,
                        "target": target,
                        "inputs_policy": "exhaustive",
                        "source_state": "git",
                        "source_digest": "source-example",
                        "authority_digest": "authority-example",
                        "dependency_digest": "dependencies-example",
                        "identity_digest": "identity-example",
                        "configuration_digest": "configuration-example",
                        "runner_digest": "runner-example",
                        "invocation_digest": "invocation-example",
                        "source_preview": [],
                        "source_entry_count": 0,
                        "source_preview_truncated": false,
                        "dependencies": [{
                            "target": dependency_target,
                            "identity_digest": "identity-dependency",
                        }],
                    },
                    "dependency_execution_proof": [{
                        "target": dependency_target,
                        "receipt_id": dependency_id,
                        "run_id": "run_dependency",
                        "plan_id": dependency_plan,
                        "identity_digest": "identity-dependency",
                        "conclusion": "success",
                        "effective_valid_until_ms": null,
                        "effective_requires_time_validity": false,
                    }],
                    "effective_valid_until_ms": null,
                    "effective_requires_time_validity": false,
                    "global_execution_proof": {"state": "unknown"},
                })
            });
        json!({
            "id": id,
            "session_id": "session_example",
            "plan_id": plan_id,
            "tool_name": "jig.target_run",
            "args": {},
            "started_at_ms": 1,
            "ended_at_ms": 2,
            "exit_status": 0,
            "stdout_preview": "",
            "stderr_preview": "",
            "run_id": format!("run_{id}"),
            "target": target,
            "findings": [],
            "changed_paths": [],
            "diff_stat": {"files": 0, "insertions": 0, "deletions": 0},
            "target_freshness": target_freshness,
        })
    }

    fn write_receipts(path: &Path, records: &[&Value]) {
        let mut bytes = Vec::new();
        for record in records {
            bytes.extend_from_slice(&serde_json::to_vec(record).unwrap());
            bytes.push(b'\n');
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn identical_receipt_restore_does_not_require_tracker_authority() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        fs::create_dir_all(ctx.state_dir()).unwrap();
        let receipts_path = ctx.state_file("receipts.jsonl");
        fs::write(&receipts_path, b"").unwrap();
        let (backup, _) =
            super::super::create_receipts_backup(&ctx, &receipts_path, "receipts-identical", None)
                .unwrap();
        fs::write(ctx.state_file("tracker-operations.jsonl"), b"{").unwrap();

        let restored = super::super::restore_backup(&ctx, StateRestoreRequest { backup }).unwrap();

        assert_eq!(restored["changed"], false);
        assert!(fs::read(receipts_path).unwrap().is_empty());
    }

    #[test]
    fn damaged_receipts_without_pending_tracker_roots_can_be_restored() {
        for damaged in [b"{not-json}\n".as_slice(), b"{".as_slice()] {
            let temp = tempdir().unwrap();
            TestRepoBuilder::new(temp.path()).write();
            let ctx = RepoContext::load_from(temp.path()).unwrap();
            fs::create_dir_all(ctx.state_dir()).unwrap();
            let receipts_path = ctx.state_file("receipts.jsonl");
            fs::write(&receipts_path, b"").unwrap();
            let (backup, _) = super::super::create_receipts_backup(
                &ctx,
                &receipts_path,
                "receipts-damaged-recovery",
                None,
            )
            .unwrap();
            fs::write(&receipts_path, damaged).unwrap();

            let restored =
                super::super::restore_backup(&ctx, StateRestoreRequest { backup }).unwrap();

            assert_eq!(restored["changed"], true);
            assert!(fs::read(&receipts_path).unwrap().is_empty());
            let recovery = restored["recovery_backup_path"].as_str().unwrap();
            assert!(Path::new(recovery).join("manifest.json").is_file());
        }
    }

    #[test]
    fn receipt_restore_preserves_explicit_roots_transitive_dependency_proof() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        fs::create_dir_all(ctx.state_dir()).unwrap();
        let receipts_path = ctx.state_file("receipts.jsonl");
        let dependency = target_receipt(
            "receipt_dependency",
            "plan_dependency",
            "example:dependency",
            None,
        );
        let root = target_receipt(
            "receipt_root",
            "plan_root",
            "example:root",
            Some((
                "receipt_dependency",
                "plan_dependency",
                "example:dependency",
            )),
        );
        let unrelated = target_receipt(
            "receipt_unrelated",
            "plan_tracker",
            "example:unrelated",
            None,
        );

        write_receipts(&receipts_path, &[&root]);
        let (missing_backup, _) = super::super::create_receipts_backup(
            &ctx,
            &receipts_path,
            "receipts-missing-dependency",
            None,
        )
        .unwrap();
        write_receipts(&receipts_path, &[&dependency, &root]);
        let protected_bytes = fs::read(&receipts_path).unwrap();
        let (preserving_backup, _) = super::super::create_receipts_backup(
            &ctx,
            &receipts_path,
            "receipts-preserving-dependency",
            None,
        )
        .unwrap();
        write_receipts(&receipts_path, &[&dependency, &root, &unrelated]);
        let current_bytes = fs::read(&receipts_path).unwrap();
        fs::write(
            ctx.state_file("tracker-operations.jsonl"),
            format!(
                "{}\n",
                json!({
                    "schema_version": 1,
                    "event_id": "tracker-event-restore-dependency",
                    "operation_id": "tracker-operation-restore-dependency",
                    "plan_id": "plan_tracker",
                    "issue": {
                        "provider": "beads",
                        "workspace_id": "ExampleProject",
                        "issue_id": "example-123",
                        "tracker_root": ".beads",
                    },
                    "kind": "export",
                    "phase": "intent",
                    "timestamp_ms": 1,
                    "receipt_ids": ["receipt_root"],
                })
            ),
        )
        .unwrap();

        let error = super::super::restore_backup(
            &ctx,
            StateRestoreRequest {
                backup: missing_backup,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("backup omits"), "unexpected error: {error}");
        assert_eq!(fs::read(&receipts_path).unwrap(), current_bytes);

        let restored = super::super::restore_backup(
            &ctx,
            StateRestoreRequest {
                backup: preserving_backup,
            },
        )
        .unwrap();
        assert_eq!(restored["changed"], true);
        assert_eq!(fs::read(&receipts_path).unwrap(), protected_bytes);
    }
}
