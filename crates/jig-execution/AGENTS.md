# jig-execution crate guide

## Purpose

`crates/jig-execution` runs Jig-owned commands under supervision: bounded output capture, repository-configured timeouts and output limits, cooperative cancellation, heartbeats, and phase events for observers. It builds on `jig-owned-process` for process-tree ownership.

## Key entrypoints

- `src/progress.rs`: human-mode CLI progress (`CliProgress`, `CliExecutionObserver`), buffered and delivered to stderr with a bounded best effort.
- `src/lib.rs`: `run_supervised_execution_command`, `run_authoritative_execution_command`, the `ExecutionObserver`/`ExecutionCancellation`/`ExecutionControl` traits, `ExecutionPhase`, and `HeartbeatSchedule`.

## Edit here for X

- Change how a supervised command reports timeout, cancellation, or output overflow: `SupervisedExecutionError` and `execution_command_error` in `src/lib.rs`.
- Change heartbeat cadence or phase events: `HEARTBEAT_INTERVAL`, `HeartbeatSchedule`, and `ExecutionEvent` in `src/lib.rs`.
- Change process-tree ownership or raw output bounding: [jig-owned-process](../jig-owned-process/AGENTS.md).

## Invariants

- Exceeding an output limit is an explicit failure that still reports the captured output; it is never partial success.
- Internal Git and GitHub protocol commands use the fixed `EXECUTION_OUTPUT_CAPTURE_LIMIT`; configured commands use the repository's limits from `jig-context`.
- Keep this crate independent from CLI, state, runtime dispatch, and vault secret handling.

## Common commands

- `cargo test -p jig-execution`
- `cargo clippy -p jig-execution --all-targets -- -D warnings`
- `cargo test -p jig-sh`
