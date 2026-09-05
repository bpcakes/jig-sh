# jig-contract crate guide

## Purpose

`crates/jig-contract` owns stable shared DTOs, tool names, command keys, and feature-registry contracts used across Jig crates.

## Key entrypoints

- `src/lib.rs`: shared contract types, constants, and feature context traits.

## Edit here for X

- Add or rename tool identifiers: `src/lib.rs`.
- Change manifest DTOs shared across crates: `src/lib.rs`.
- Change feature registry metadata contracts: `src/lib.rs`.

## Invariants

- Keep this crate free of runtime orchestration, filesystem mutation, process execution, and repo loading.
- Keep dependency direction downward: feature and runtime crates may depend on this crate, but this crate must not depend on them.
- Treat public types and constants as semver-sensitive internal workspace API.

## Common commands

- `cargo test -p jig-contract`
- `cargo test -p jig-sh`
