# jig-sqlx crate guide

## Purpose

`crates/jig-sqlx` owns SQLx feature-area contract metadata and SQLx-specific harness checks as they are extracted from `jig-sh`.

## Key entrypoints

- `src/lib.rs`: SQLx command keys, required tool mapping, native tool metadata, and availability messages.
- `src/migrations.rs`: pure SQLx numeric migration-version and reversible-pair validation.

## Edit here for X

- Add an SQLx check exposed through Jig: `src/lib.rs`.
- Change migration or schema gate requirements: `src/lib.rs`.
- Change migration filename identity or conflict rules: `src/migrations.rs`.

## Invariants

- Do not depend on `jig-sh`; keep SQLx metadata usable by the registry.
- Keep SQLx rules separate from general Rust rules even when they operate on Rust projects.
- Validate each migration source independently using parsed `i64` versions; allow one reversible up/down pair per version.
- Keep filesystem mutation and process execution in `jig-sh` until SQLx policy extraction is explicit.

## Common commands

- `cargo test -p jig-sqlx`
- `cargo test -p jig-sh`
