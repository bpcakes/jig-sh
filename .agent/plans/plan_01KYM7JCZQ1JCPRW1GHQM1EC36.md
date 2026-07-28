# Separate state records from JSONL storage

Move durable state record schemas and their legacy serde compatibility code out of the JSONL persistence module without changing the append-only stream formats or the crate::state API.

## Progress

- [x] Inspected the current state module and identified the mixed responsibilities in crates/jig/src/state/events.rs.
- [x] Split durable record schemas into crates/jig/src/state/records.rs.
- [x] Split JSONL append, locking, snapshot, parsing, and reverse receipt scans into crates/jig/src/state/jsonl.rs.
- [x] Move shared state filesystem, timestamp, ID, path, and preview helpers into crates/jig/src/state/support.rs.
- [x] Update state feature modules and tests to use the explicit boundaries.
- [x] Run focused state tests, strict Clippy, the full Jig test gate, and formatting checks.

## Surprises & Discoveries

- The top-level state.rs is already a small facade. The remaining coupling is concentrated in the 1119-line events.rs file, whose first 464 lines are record schemas and serde compatibility while the rest is persistence and support code.
- Receipt reverse-window reads are JSONL storage behavior even though they are specialized to ReceiptRecord, so they belong in jsonl.rs and depend inward on records.rs.
- The existing 42 state tests already exercise both sides of the new boundary, including legacy event decoding, exact append behavior, advisory-lock fallbacks, stable snapshots, reverse receipt scans, and archive rewrites.

## Decision Log

- Preserve crate::state call sites and every JSONL field exactly; this is a source reorganization, not a schema migration.
- Use three internal modules: records.rs for durable schemas, jsonl.rs for persistence, and support.rs for non-storage state helpers.
- Keep the known legacy SessionEvent and PlanEvent serialization/deserialization implementations with the record types because they define schema compatibility, not filesystem behavior.
- Keep receipt-specific reverse reads in jsonl.rs because their seek, chunk, and lock behavior is a persistence optimization; jsonl.rs depends on ReceiptRecord only to apply plan filters and deserialize selected rows.

## Outcomes & Retrospective

The state layer now has an explicit dependency direction: `records.rs` owns the durable schemas and compatibility serde code; `jsonl.rs` owns append, lock, snapshot, parse, archive-rewrite, and reverse-window mechanics; and `support.rs` owns IDs, timestamps, layout, relative paths, and preview truncation. The former `events.rs` no longer exists, while the `crate::state` facade and serialized JSONL formats remain unchanged.

Validation passed for all 42 focused state tests, strict all-target/all-feature Clippy, the configured `jig.contract_check` and full `jig.test` work gates, the Jig formatting gate, rustfmt, and `git diff --check`. A mechanical comparison against the parent `events.rs` confirmed that the moved record, storage, and support bodies changed only in module imports and documentation.

## Context and orientation

The Jig CLI persists sessions, plans, receipts, and decisions as append-only streams under .agent/state. crates/jig/src/state.rs exposes the crate-local facade. Feature behavior lives in plans.rs, receipts.rs, sessions.rs, and timeline.rs. events.rs currently owns both serialized record types and JSONL filesystem mechanics, obscuring the dependency direction.

## Plan of work

Create records.rs from the state record enums and structs plus their serde adapters. Create jsonl.rs from append, rewrite-under-lock, shared-lock reads, stable unlocked snapshots, parsing, and receipt reverse scans. Create support.rs for now_ms, truncate, new_id, ensure_state_layout, and rel_path. Update imports so jsonl depends on records, while feature modules import records, jsonl, and support explicitly.

## Concrete steps

1. Split events.rs mechanically at the record/storage and storage/support boundaries.
2. Minimize imports in each new module and remove events.rs.
3. Update state.rs and every state submodule import.
4. Update tests to import record and storage concerns separately.
5. Run cargo fmt and focused state tests, then strict Clippy and repository gates.

## Validation and acceptance

Acceptance requires no production events module, record definitions isolated from filesystem and locking imports, JSONL storage isolated from domain record definitions except its ReceiptRecord dependency, unchanged serialized fixtures and state tests, cargo clippy -p jig-sh --all-targets --all-features -- -D warnings, and JIG_DEV_BIN=target/debug/jig scripts/jig check test plus fmt passing.

## Idempotence and recovery

The source split is repeatable from Git and does not rewrite repository state streams. If compilation fails mid-split, restore module imports from the plan and compare moved blocks to the parent commit. Do not modify or compact .agent/state JSONL files.

## Interfaces and dependencies

No public or crate-visible function signatures change. The only new dependency direction is jsonl.rs importing ReceiptRecord from records.rs. serde, serde_json, fs4, tempfile, and ulid usage remains in the module that owns the corresponding behavior.
