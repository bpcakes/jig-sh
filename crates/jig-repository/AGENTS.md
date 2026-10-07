# jig-repository crate guide

## Purpose

`crates/jig-repository` owns Jig's repository model: the action catalog built from the contract manifest, run planning and affected-target selection, action runners, Cargo and Playwright resource discovery, and the Git source identity that freshness and comparison scopes are built on.

## Key entrypoints

- `src/lib.rs`: `RepositoryCatalog`, action normalization and validation, and module wiring.
- `src/planner.rs` and `src/planner/`: run plans, plan validation, and resource waves.
- `src/affected.rs` and `src/affected/`: changed-path matching against action inputs.
- `src/runners.rs` and `src/runners/`: argv and native runner validation and literal exec.
- `src/cargo_discovery.rs`, `src/cargo_impact.rs`, `src/cargo_resources.rs`, `src/rust_focus.rs`, `src/playwright_resources.rs`: resource discovery.
- `src/inspect.rs`: `jig repository inspect` data for each `ResponseSurface`.
- `src/source_identity.rs` and `src/source_identity/`: Git-backed source snapshots, worktree fingerprints, and comparison scopes.
- `src/shell.rs`: shell quoting and the optional-Cargo command wrapper shared with bootstrap.

## Edit here for X

- Change how actions are declared or validated: `src/lib.rs`, `src/arguments.rs`, and `src/runners.rs`; contract-epoch rules for inputs live in [jig-context](../jig-context/AGENTS.md) (`inputs_policy.rs`).
- Change affected selection or comparison scopes: `src/affected.rs` and `src/source_identity/`.
- Change run plan construction: `src/planner.rs`.

## Invariants

- Preserve generated-repo compatibility for `.agent/jig-contract.json` action declarations.
- Git commands aimed at the repository go through `jig-git` program selection and environment scrubbing.
- Source identity tests supervise Git process trees; nextest runs them in the `process-signals` group, and some re-run their own test binary by exact test path under `source_identity::tests`.
- `plan_run`, `plan_action_run`, and `validate_run_plan` exist only under `cfg(test)` or the `test-support` feature, which dependents enable from `[dev-dependencies]` only.
- Keep this crate independent from CLI, runtime dispatch, bootstrap, and vault secret handling.

## Common commands

- `cargo test -p jig-repository`
- `cargo clippy -p jig-repository --all-targets --features test-support -- -D warnings`
- `cargo test -p jig-sh`
