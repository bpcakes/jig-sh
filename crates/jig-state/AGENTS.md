# jig-state crate guide

## Purpose

`crates/jig-state` owns the append-only repository state under `.agent/state`: run history, repository execution and resource leases, and the summary, diagnosis, archive, and restore maintenance over them.

## Key entrypoints

- `src/lib.rs`: public facade and the `state archive`/`state restore` request types.
- `src/runs.rs` and `src/runs/`: durable run lifecycle events, folding, cancellation requests, and archiving.
- `src/jsonl.rs` and `src/jsonl/`: locked and streaming JSONL reads and appends.
- `src/execution_leases.rs`, `src/resource_leases.rs`: repository execution and resource claims.
- `src/summary.rs`, `src/diagnostics.rs`, `src/maintenance.rs`: `jig state summary`, `diagnose`, and `restore`.
- `src/cancellation.rs`: the status-collection cancellation error shared with status and dashboard readers.

## Edit here for X

- Change run history records or folding: `src/records.rs` and `src/runs.rs`.
- Change JSONL locking, streaming, or size limits: `src/jsonl.rs`.
- Change lease semantics: `src/execution_leases.rs` or `src/resource_leases.rs`.
- Change state summary, diagnosis, archive, or restore: `src/summary.rs`, `src/diagnostics.rs`, `src/runs/archive.rs`, or `src/maintenance.rs`.
- Change how the CLI presents state commands: `crates/jig/src/cli/state.rs`.

## Invariants

- Treat `.agent/state/*.jsonl` as append-only unless a migration path is explicit, and preserve generated-repo compatibility for existing records.
- Keep this crate independent from CLI, runtime execution, bootstrap, and vault secret handling.
- The test clock (`set_test_now_ms`), dashboard scan counters, and blocking lease helpers exist only under `cfg(test)` or the `test-support` feature, which dependents enable from `[dev-dependencies]` only. Production `now_ms` always reads the system clock.
- Resource-lease tests re-run their own test binary by exact test path; keep those paths in `src/resource_leases/tests.rs` in step with module moves.

## Common commands

- `cargo test -p jig-state`
- `cargo clippy -p jig-state --all-targets --features test-support -- -D warnings`
- `cargo test -p jig-sh`
