# jig-dashboard crate guide

## Purpose

`crates/jig-dashboard` holds the typed data contracts of the unified terminal dashboard: recorder and status snapshots, their bounded collections and text, and the `DashboardSource` interface. `jig-sh` and `jig-loops` produce these snapshots; `jig-ui` renders them. It has no terminal, repository, or state dependencies.

## Key entrypoints

- `src/lib.rs`: module wiring and public re-exports.
- `src/recorder.rs` and `src/status.rs`: recorder and status snapshot contracts and their schema versions.
- `src/source.rs`: the `DashboardSource` interface and refresh results.
- `src/bounded.rs`: bounded rows and text with explicit omission counts.
- `src/parity.rs` and `src/scenarios.rs`: contract parity table and scenario fixtures for tests, behind `cfg(test)` or the `test-support` feature.

## Edit here for X

- Change recorder/status wire contracts or bounds: `src/recorder.rs`, `src/status.rs`, or `src/bounded.rs`. A removed or renamed wire field bumps `RECORDER_SCHEMA_VERSION` or `STATUS_SCHEMA_VERSION`.
- Change how snapshots are rendered: [jig-ui](../jig-ui/AGENTS.md).
- Change how repository state, run history, or loops become snapshots: `crates/jig/src/ui/source/` and [jig-loops](../jig-loops/AGENTS.md).

## Invariants

- Keep this crate data-only: no terminal, `RepoContext`, state storage, or runtime dependencies.
- Keep every collection and text field within its declared bound, preserving explicit omission counts.
- `jig-dashboard` is a CLI-owned internal crate, versioned with the matching `jig-sh` release.

## Common commands

- `cargo test -p jig-dashboard`
- `cargo test -p jig-ui`
- `cargo clippy -p jig-dashboard --all-targets --features test-support -- -D warnings`
