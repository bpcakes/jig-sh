# jig-policy crate guide

## Purpose

`crates/jig-policy` owns Jig's repository policy checks: contract validation, agent-guide and agent-map checks and generation, SQLx and schema policy, and migration helpers. The CLI parses `jig check` and `jig agent map` and renders results; runtime dispatches native checks into this crate.

## Key entrypoints

- `src/lib.rs`: `run_check`, `PolicyCheckCommand`, `validate_contract`, and `contract_check`.
- `src/agent_map.rs`: rendering and writing `agent-map.md`.
- `src/guide_check.rs` and `src/agent_guides.rs`: nested `AGENTS.md` discovery, required sections, and references.
- `src/sqlx.rs` and `src/sqlx/`: the SQLx query inventory and unchecked-query policy, parsed with `syn` through `src/rust_syntax.rs`.
- `src/schema.rs` and `src/schema/`: committed schema drift checks in snapshotted worktrees.
- `src/migration_add.rs`: `jig migration add` naming.
- `src/git.rs`: Git commands for policy checks.

## Edit here for X

- Change required `AGENTS.md` sections or guide discovery: `src/guide_check.rs` and `src/agent_guides.rs`.
- Change agent-map output: `src/agent_map.rs`.
- Change SQLx policy: `src/sqlx/`.
- Change schema checks: `src/schema.rs`.

## Invariants

- Policy checks also run during launcher validation, before a `vault` command captures and clears its passphrase. Every Git process starts from `src/git.rs`, which withholds the reserved vault passphrase variables.
- Repository files are written through `jig-repository`'s path helpers, never directly.
- Keep this crate independent from CLI parsing, runtime dispatch, and bootstrap.
- `test_support` is shared with dependents' tests only through the `test-support` feature, enabled from `[dev-dependencies]`.

## Common commands

- `cargo test -p jig-policy`
- `cargo clippy -p jig-policy --all-targets --features test-support -- -D warnings`
- `scripts/jig-dev check agent-guides`
