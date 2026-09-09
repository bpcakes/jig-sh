# Prevent Beads path leaks and preserve Linux argv PATH search

Address the two accepted findings from the single-reviewer follow-up. Preserve the existing staged argv work and contract v8, and do not commit.

## Progress

- [x] Open jig-sh-guo and structured work; inspect repository and runtime guidance.
- [x] Clear machine-local metadata and add a repeatable sync helper, export guard, and CI wiring.
- [x] Add Linux PATH continuation errors with direct-path error preservation and deterministic fault-injection coverage.
- [x] Complete five focused Rust checks and four Python tests; rebuild runtime and stage reviewed changes.
- [x] Collect the seven tooling and partition gates; all passed fresh after the core retry.
- [x] Finish with the full backend test check and inspect all eight gate results.
- [x] Close work/issue, sync Beads through the helper, and stage task metadata.

## Surprises & Discoveries

The first core partition stopped on `wide_parallel_layer_keeps_the_bounded_worker_pool_busy`: a legacy shell runner could not confirm process-tree cleanup. This fixture does not use the changed literal argv lookup. The unchanged test then passed once in isolation and all five consecutive stress iterations (49.215 seconds). No test or cleanup behavior was weakened. The unchanged test passed in the successful core partition retry and final full backend run; the failed receipt remains in history. The original cleanup failure was not reproduced and its root cause is unconfirmed.

The export guard found 17 affected pre-existing records, including four tombstones. Beads rejects updates to tombstones and retains their metadata during import/reconcile. The helper clears ordinary records through br, then removes only source_repo_path from exported tombstones after every flush. Repository sync.auto_flush=false prevents ordinary mutations from immediately exporting new paths. The br list JSON response is paginated even with an unlimited request; the helper accepts both the current envelope and older arrays, and refuses partial results.

## Decision Log

- Keep all record IDs and unrelated fields. Ordinary metadata corrections use br and its normal updated_at change; tombstone exports preserve all other fields and timestamps. Append a durable decision listing affected record IDs without repeating removed values.
- scripts/beads-sync.py is the required local export path; --check is read-only and needs no br installation. Repo Policy CI runs the export guard, including for Beads-only changes. Unit tests use generic fixtures; the real br integration runs when the executable is available.
- Continue past Linux ESTALE, ENODEV, and ETIMEDOUT during PATH search, consistent with glibc. Keep direct executable errors and fatal ENOEXEC. Inject only syscall errors into the same production traversal for deterministic tests.

## Outcomes & Retrospective

Four Python tests passed locally, including real Beads create/close/delete/sync and metadata-only tombstone redaction. The actual export passes the guard across repeated syncs. A semantic comparison proves only source_repo_path and normal updated_at changed on the 17 pre-existing records. Five focused Rust tests passed, including fault-injected PATH traversal, explicit-path errors, ENOEXEC refusal, and descendant cancellation. The runtime build succeeded. Seven tooling/partition gates passed fresh. The retry passed core 3,163, frontend 112, vault 443 plus two additional checks, and process 210. The final full backend check passed: 3,930 tests passed, two skipped (832.262 seconds), and all five configured targets passed. Gates and evidence report all eight required gates passed and fresh; receipts and work status were inspected. All five historical state prefixes are preserved byte-for-byte. Work finished successfully and jig-sh-guo closed; the final helper sync cleared four immutable tombstone export paths and passed its guard. Changes are staged without a commit.

## Context, steps, and validation

The implementation lives in scripts/beads-sync.py, scripts/tests/test_beads_sync.py, .beads/config.yaml, .github/workflows/repo-policy.yml, AGENTS.md, and crates/jig/src/repository/runners/literal_exec.rs. Run python3 -m unittest discover -s scripts/tests and python3 scripts/beads-sync.py --check. Run focused nextest over literal_exec and argv shell-fallback/cancellation behavior. Build cargo build -p jig-sh --bin jig and use JIG_DEV_BIN=target/debug/jig for all harness commands.

Stage the reviewed source before attestation. Run the seven tooling/partition gates for plan_01M2360FDFS7H8MPNFYCHNBTEQ, then JIG_DEV_BIN=target/debug/jig scripts/jig check --plan-id plan_01M2360FDFS7H8MPNFYCHNBTEQ test. Inspect gates, evidence, receipts, and status; all eight gates must be passed and fresh. Finish work successfully, close jig-sh-guo, sync with the new helper, and stage only this task's remaining metadata.

## Recovery and interfaces

Do not rewrite historical .agent/state JSONL. A prefix digest snapshot is kept outside the repo for final verification. If the helper fails, no export is approved: fix the reported cause and rerun it; --check never changes files. Beads tombstone source metadata remains local in its immutable database record, but every repository export removes it. Fix demonstrated test failures before refreshing dependent evidence. No new schema epoch or public runtime interface is added.
