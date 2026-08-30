# jig-file-budget crate guide

## Purpose

`crates/jig-file-budget` owns strict versioned file-budget policy parsing, byte-oriented measurement, path classification, waiver authorization continuity, and deterministic evaluation over supplied facts.

## Key entrypoints

- `src/policy.rs` and `src/policy/`: version 1 policy DTOs, validation, identity, glob matching, and historical-policy parsing.
- `src/measurement.rs`: streaming LF and byte measurement over caller-supplied readers.
- `src/evaluation.rs` and `src/evaluation/`: pure debt and waiver evaluation over explicit current/comparison facts.
- `src/diagnostic.rs`: stable structured diagnostic codes and ordering.

## Edit here for X

- Change policy schema or matching semantics: `src/policy.rs`, `src/policy/`, and `tests/policy.rs`.
- Change physical-line or byte measurement: `src/measurement.rs` and `tests/measurement.rs`.
- Change debt, threshold, or waiver behavior: `src/evaluation.rs`, `src/evaluation/`, and `tests/evaluation.rs`.

## Invariants

- Keep the crate pure: no Git, repository discovery, filesystem traversal, CLI, process, template, journal, or `jig-contract` dependencies.
- Current-view state, measurements, ancestry, calendar date, and comparison policy are explicit inputs.
- Comparison policy is historical waiver authority only; current policy always owns matching and ordinary thresholds.
- Line and byte coordinates are evaluated independently and diagnostics sort deterministically.

## Common commands

- `cargo test -p jig-file-budget`
- `cargo clippy -p jig-file-budget --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
