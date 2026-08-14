# Harden Vault TUI reviewed boundaries

Apply Fowler-style preparatory refactors and separately committed fixes for the
Vault TUI review findings. The observable outcome is that interactive mutations
cannot act on a vault generation the operator did not approve, locking always
drops protected session state, security-sensitive kind changes require deliberate
confirmation, cancelled imports release their plan, invalid item renames retain
their typed error classification, and terminal metadata input is bounded.

## Progress

- [x] Read repository and crate guidance plus the Fowler Rust refactoring
  principles and catalogs.
- [x] Run the Fowler changed-code scanner and manually reject metric-only false
  positives.
- [x] Record a trustworthy pre-change formatting, compile/lint, and test baseline.
- [x] Introduce an opaque snapshot mutation precondition without changing existing
  public mutation behavior.
- [x] Migrate TUI mutations to consume and enforce that precondition.
- [x] Fix poisoned-session credential cleanup.
- [x] Add deliberate concealed-to-text confirmation.
- [x] Discard cancelled import plans.
- [x] Preserve `InvalidInput` for overlong item-rename references.
- [x] Bound metadata and search editing at the model boundary.
- [x] Run configured gates and inspect evidence.

## Surprises & Discoveries

- The import flow already has the correct optimistic-concurrency model:
  `VaultImportPrecondition` binds a preview to `vault_id` and the encrypted state
  nonce. Ordinary snapshot-backed mutations do not share that capability.
- `VaultWriteMode::{Create, Replace}` protects only existence. It prevents stale
  create/remove races but cannot protect replacement, delete/recreate ABA, or
  item operations that widen to newly added fields.
- The scanner reported many large functions, DTO public fields, test unwraps, and
  exhaustive matches. Manual inspection shows these are cohesive tests,
  transparent boundary records, and idiomatic closed-enum dispatch rather than
  refactoring targets for this change.
- The configured baseline passed before source edits: formatting receipt
  `receipt_01M004C9YKS30ST3VHGGPBFGTG`, Clippy receipt
  `receipt_01M004CVZ72EX1T7K29A54PG5M`, contract receipt
  `receipt_01M004D3CPQCFPV6F4Q14DJBVQ`, and the complete two-part nextest receipt
  `receipt_01M00579Z1MCZ4J1BRNN6F6GP9`.
- A single core-owned `VaultMutation` command avoided adding nine parallel
  conditional public methods and removed the backend's duplicate variant
  dispatch. New mutation variants must now pass through the same revision gate.
- Redaction downgrade was not limited to the field-kind form: replacing a
  concealed field as text and converting a concealed legacy entry to text have
  the same risk. Confirmation therefore derives from the typed mutation plus
  the authenticated snapshot rather than from a single key path.
- Item rename crossed from typed `VaultError` into an internal `anyhow::Result`
  through the similarly named public `Result` alias. An explicit conversion
  bridge now preserves error classification at that boundary.
- Import preview cancellation must consume the exact retained capability. A
  generic overlay close could neither release the backend plan nor distinguish
  an older preview from a newer one.

## Decision Log

- Use Fowler's **Replace Primitive with Object** and **Introduce Parameter
  Object** to add one opaque vault-generation value owned by `jig-vault`.
  Generation validation stays in the core under the same lock as each mutation.
- Preserve the existing public non-interactive APIs for compatibility. Add one
  `Vault::mutate_if_unchanged` command boundary and migrate only the TUI backend,
  following expand -> migrate without contracting the existing API.
- Keep structural introduction of the generation type separate from the behavior
  change that rejects stale commands.
- Replace the one-off empty-value screen with a closed mutation-confirmation
  policy. It covers clearing and every current concealed-to-text transition
  without introducing a trait or dynamic dispatch.
- Treat import commit authority as a one-shot capability for both commit and
  discard. A mismatched discard fails without consuming the installed plan.
- Keep raw metadata strings private to the model. Runtime editing uses bounded,
  atomic append methods with a 128 KiB metadata cap and 256-byte search cap.
- Commit every independently verifiable slice separately and avoid unrelated
  scanner-driven cleanup.

## Outcomes & Retrospective

The reviewed failures split into one architectural family and four local
omissions. Snapshot drift was the architectural issue: authorization metadata,
commands, and execution were separate, so destructive actions could operate on
state the operator never saw. `VaultRevision`, core-owned `VaultMutation`, and
`Vault::mutate_if_unchanged` now make that invariant one atomic core boundary.

The other fixes are deliberately narrower but close their escape hatches:
poison recovery is permitted only for session erasure; mutation confirmations
are policy-driven; import abandonment consumes only its matching plan; typed
errors retain their kind across the dynamic-error layer; and runtime code no
longer receives mutable metadata strings.

Each slice was committed and verified independently. Final configured work
check `receipt_01M008Q0TSG1HEJXRQG69M1MS9` passed, containing contract receipt
`receipt_01M007V9YSGY4MV742A73BHAHT` and complete test receipt
`receipt_01M008Q0S9X54NYXCKS32SD32M`. Focused validation also passed all 215
`jig-vault` tests, all 46 `jig-vault-tui` tests, the backend TUI regressions, and
strict Clippy for the affected crates.

## Context and Orientation

`crates/jig-vault/src/vault.rs` owns encrypted state, mutation locking, and the
state nonce. `crates/jig-vault-tui/src/lib.rs` defines the metadata-only backend
contract and typed actions. `crates/jig-vault-tui/src/model.rs` owns snapshots,
forms, and confirmations. `crates/jig-vault-tui/src/runtime.rs` owns terminal input
and worker coordination. `crates/jig/src/runtime/vault/tui.rs` adapts typed TUI
actions to the core vault.

The public crate and persistent vault format are compatibility-sensitive. This
work must not alter serialized state, audit ordering, plaintext boundaries,
existing CLI upsert behavior, the Rust 1.85 MSRV, or Unix/Windows terminal and
filesystem policies.

## Plan of Work

1. Characterize stale snapshot behavior and introduce an opaque generation type
   sourced from authenticated vault state.
2. Add conditional core mutation entry points behind existing mutation logic,
   then migrate TUI actions to carry the snapshot generation and reject drift.
3. Address each remaining finding locally with its own regression test and commit.
4. Build the development `jig` binary, run configured checks through
   `JIG_DEV_BIN=target/debug/jig`, inspect receipts, and finish structured work.

## Concrete Steps

- Run `cargo fmt --all -- --check`, project Clippy policy, and the configured test
  command before code changes.
- For each slice: add the smallest failing/characterization test, apply one focused
  patch, run the owning crate's tests, inspect `git diff`, and commit.
- Run `cargo build -p jig-sh --bin jig`, then `scripts/jig work check`,
  `scripts/jig work gates`, `scripts/jig work evidence`, and
  `scripts/jig work receipts` with this plan ID and `JIG_DEV_BIN` set.

## Validation and Acceptance

- A mutation carrying a stale snapshot generation fails before audit append or
  state write, refreshes the TUI, and requires a new operator action.
- Item delete/rename cannot include fields added after the approved snapshot.
- A poisoned session mutex can still be entered for erasure and retains no
  credential or pending import.
- Concealed-to-text cannot be committed through the ordinary one-Enter form path.
- Escaping an import preview clears only its matching pending plan.
- Combined item/field reference overflow reports `InvalidInput`.
- Oversized metadata/search input is rejected atomically and cannot become
  persistent render work.
- Formatting, Clippy, configured tests, contract, and work gates pass.

## Idempotence and Recovery

All code changes are ordinary source edits and tests. Each slice is a separate
commit and can be reverted independently. The new conditional APIs are additive,
so work may stop after their introduction without changing existing behavior. If
a behavior-changing step fails validation, revert only that slice and retain the
last green commit.

## Interfaces and Dependencies

No dependency was added. The additive interfaces are the opaque `VaultRevision`,
the core-owned `VaultMutation` command, and `Vault::mutate_if_unchanged`.
Existing public mutation methods remain available and retain their semantics.
