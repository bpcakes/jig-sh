# jig-core crate guide

## Purpose

`crates/jig-core` owns base Jig harness feature metadata and shared dev environment naming rules.

## Key entrypoints

- `src/lib.rs`: core feature descriptor, base harness tool requirements, and dev environment prefix normalization and uniqueness.

## Edit here for X

- Add or change a base harness command key: `src/lib.rs`.
- Change core required tool rules: `src/lib.rs`.
- Change core native tool metadata: `src/lib.rs`.
- Change dev environment prefix naming or collision validation: `src/lib.rs`.

## Invariants

- Keep this crate narrowly scoped to base harness feature metadata and pure dev environment naming rules.
- Do not add runtime orchestration, state handling, MCP transport, bootstrap implementation, or process execution here.
- Depend only downward on `jig-contract`; aggregation belongs in `jig-features`.

## Common commands

- `cargo test -p jig-core`
- `cargo test -p jig-features`
- `cargo test -p jig-sh`
