# jig-go crate guide

## Purpose

`crates/jig-go` owns Go feature-area contract metadata.

## Key entrypoints

- `src/lib.rs`: Go command keys, required tool mapping, and feature descriptor.

## Edit here for X

- Add or change a Go command-backed check: `src/lib.rs`.

## Invariants

- Do not depend on `jig-sh`; keep this crate reusable by the feature registry.
- Keep process execution in the runtime crate.

## Common commands

- `cargo test -p jig-go`
- `cargo test -p jig-sh`
