# jig-loops crate guide

## Purpose

`crates/jig-loops` owns Jig's agent loops: workflow scheduling and dispatch, occurrences and their evidence, workflow and branch leases, Codex task workers, and the PR manager. The CLI in `crates/jig` parses `jig loop` commands and renders their results; `crates/jig/src/runtime.rs` dispatches into this crate.

## Key entrypoints

- `src/lib.rs`: `dispatch_with_observer`, `status_with_cancellation`, `typed_status_with_cancellation`, and the `jig loop` request types from `src/request.rs`.
- `src/schedule.rs` and `src/schedule/`: due dispatch, cron schedules, and run policy.
- `src/engine.rs` and `src/engine/`: ticks, status, and attempt maintenance.
- `src/occurrence.rs` and `src/occurrence/`: occurrence claims, attention, history, and persistence.
- `src/evidence.rs`, `src/show.rs`: per-occurrence evidence under Git metadata and `jig loop show`.
- `src/state.rs` and `src/state/`: loop leases, attempts, and JSON caches.
- `src/codex_task.rs`, `src/worker_runner.rs`: Codex task checkout, preflight, and worker execution.
- `src/pr_manager.rs` and `src/pr_manager/`: PR repair, review threads, and pushes.
- `src/dashboard.rs`: typed loop status for `jig-ui`.

## Edit here for X

- Change loop occurrence evidence, its retention, or `jig loop show`: `src/evidence.rs` and `src/show.rs`.
- Change workflow configuration parsing: [jig-context](../jig-context/AGENTS.md) (`loop_config.rs`); resolution against it lives in `src/workflow.rs`.
- Change how `jig loop` is parsed or rendered: `crates/jig/src/cli/loops.rs`.

## Invariants

- Loop leases, occurrence records, and evidence are durable coordination state; preserve compatibility with records written by earlier runtimes.
- Commands aimed at a known repository go through `jig-git` program selection and environment scrubbing.
- Keep this crate independent from CLI parsing, output rendering, and vault secret handling.
- `dispatch_due_at`, `revoke_lease_for_test`, and `evidence_directory_for_test` exist only under `cfg(test)` or the `test-support` feature, which dependents enable from `[dev-dependencies]` only.

## Common commands

- `cargo test -p jig-loops`
- `cargo clippy -p jig-loops --all-targets --features test-support -- -D warnings`
- `cargo test -p jig-sh runtime::tests::loops`
