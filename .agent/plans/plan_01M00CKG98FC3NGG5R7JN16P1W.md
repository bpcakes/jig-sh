# Harden Vault TUI review boundaries

Apply Fowler-style preparatory refactors and separately committed fixes for the
remaining Vault TUI review findings. The observable outcome is that bulk imports
cannot downgrade redaction policy without a specific acknowledgement, recovery
screens reflect authoritative storage and preserve the operator's selection,
new backend actions require exhaustive handling, and private-output failures keep
typed classifications without parsing display text.

## Progress

- [x] Read repository and crate guidance plus the Fowler Rust refactoring
  principles and catalogs.
- [x] Run the changed-code heuristic scanner and manually reject metric-only
  candidates.
- [x] Classify the five review findings by root cause and identify small seams.
- [x] Record a trustworthy pre-change baseline for the affected crates.
- [ ] Model planned import field transitions without changing commit behavior.
- [ ] Require explicit acknowledgement for concealed-to-text import replacements.
- [ ] Separate success snapshot selection from failure-recovery snapshot selection.
- [ ] Reconcile a vanished vault to the Missing screen.
- [ ] Restore exhaustive matching at the closed Vault action boundary.
- [ ] Replace private-output message parsing with typed policy failures.
- [ ] Run focused checks, configured gates, inspect receipts, and close the work.

## Surprises & Discoveries

- The import plan already binds commits to an authenticated `VaultRevision`, but
  its field observation retains only a presence boolean. That representation
  cannot express the old-to-new kind transition needed by the TUI policy.
- `ImportPreviewRow` repeats `kind` plus `replaces_existing`, and confirmation
  logic reconstructs policy from those primitive facts. This is a real Primitive
  Obsession / Data Class problem because a redaction invariant already escaped it.
- Failure recovery uses operation-name heuristics and reuses `apply_snapshot`,
  whose pending selection hint is success-only state. The cursor jump and stale
  presence are two symptoms of the same conflated transition boundary.
- `PrivateDestinationPrecondition` is a strong typed filesystem capability, but
  its conflict classification is converted to prose and parsed back into a kind.
  The loss of type information happens entirely inside `output.rs` and needs no
  public error-contract change.
- The scanner reported 200 candidates, dominated by cohesive long tests, public
  DTOs, boolean facts, test `unwrap`s, and file-size metrics. They do not create
  change friction for these findings and are explicitly out of scope.
- Change history strengthens the recovery diagnosis: `runtime.rs`, `model.rs`,
  and their tests have co-changed through several recovery fixes, while
  `output.rs` has one cohesive responsibility and should remain one module.
- The source baseline is green: `cargo fmt --all -- --check` passed; all 216
  `jig-vault` and 46 `jig-vault-tui` tests passed; and strict Clippy passed for
  `jig-vault`, `jig-vault-tui`, and `jig-sh` across all targets.

## Decision Log

- Apply Fowler's **Replace Primitive with Object**, **Encapsulate Record**, and
  **Split Phase** to model an import row as a closed `ImportFieldChange` transition
  derived from the core's revision-bound prior kind. Do not add a trait or dynamic
  dispatch for this closed policy.
- Preserve the existing public `VaultImportPrecondition::fields()` presence view
  and add a prior-kind view, following expand -> migrate. The new TUI crate is
  same-release internal, but the core library API remains additive.
- Keep structural import modeling and the new `IMPORT TEXT` behavior in separate
  commits so the refactor and policy change remain independently reviewable.
- Apply **Separate Query from Modifier** / **Move Function** at the App boundary:
  success snapshots may consume a destination hint; recovery snapshots must
  preserve the current selection.
- Model failure recovery as a closed enum owned by `OperationKind`. Reconcile
  Unlock/Refresh absence and a failed recovery refresh through `VaultPresence`
  instead of inferring screens from the original command alone.
- Remove `#[non_exhaustive]` only from `VaultAction`. The action set is a closed,
  same-release backend command protocol; `VaultActionResult` already has an
  exhaustive in-crate consumer and does not create the reported gap.
- Apply Rust's typed-`Result` analogue of Fowler's **Replace Error Code with
  Exception**: create a private conflict enum whose `Display` preserves current
  messages, then classify by downcast through the error chain.
- Commit every preparatory refactor and behavior change separately. Avoid
  opportunistic cleanup from scanner output.

## Outcomes & Retrospective

Pending implementation and final verification.

## Context and Orientation

`crates/jig-vault/src/vault.rs` owns authenticated vault observations and atomic
import commits. `crates/jig-vault-tui/src/lib.rs` defines the typed backend
boundary. `model.rs` owns import confirmation and selection state, `render.rs`
renders transition warnings, and `runtime.rs` owns worker failure recovery.
`crates/jig/src/runtime/vault/tui.rs` adapts the fixed-scope CLI backend to those
types. `crates/jig-vault/src/output.rs` owns hardened private-file preparation and
installation.

The workspace uses Rust 2024 with Rust 1.85 MSRV. The core crate is a public
library, the TUI boundary is described as same-release internal, and the vault
serialization, audit ordering, plaintext boundaries, blocking behavior, Unix
filesystem guarantees, and Windows rejection behavior must not change. No unsafe
contract, async task, FFI, persistent format, or dependency change is planned.

## Plan of Work

1. Characterize the existing import observation, then add prior-kind metadata
   behind the existing presence API. Verify `jig-vault` tests and commit.
2. Introduce a closed import transition enum and migrate adapter/model/render
   consumers without changing the required `IMPORT` phrase. Verify both affected
   crates and commit.
3. Add downgrade-specific rendering and require exact `IMPORT TEXT` for any
   concealed-to-text replacement. Add mixed-batch regression tests and commit.
4. Add an explicit recovery-snapshot App operation that preserves the current
   selection, use it only on failed-operation refreshes, test create/rename
   failures, and commit.
5. Centralize failure policy in a closed recovery enum, then reconcile missing
   storage after Unlock/Refresh and failed recovery refreshes. Test state and
   credential cleanup, and commit the behavior change separately.
6. Remove the open-set marker from `VaultAction`, delete the adapter wildcard,
   compile-check the exhaustive protocol, and commit.
7. Introduce private typed output-policy conflicts while preserving messages,
   then replace substring classification and add context/path regressions in a
   following commit.
8. Build the development Jig binary, run configured checks and gates with
   `JIG_DEV_BIN=target/debug/jig`, inspect the final diff and receipts, update this
   plan, and close structured work.

## Concrete Steps

- Baseline with `cargo fmt --all -- --check`, focused tests for `jig-vault` and
  `jig-vault-tui`, and the existing current-HEAD Jig receipts.
- For every slice, use `apply_patch`, run the narrow owning-crate tests, inspect
  `git diff --check` and the staged diff, then create one non-interactive commit.
- Finish with `cargo build -p jig-sh --bin jig`, `scripts/jig work check`,
  `scripts/jig work gates`, `scripts/jig work evidence`, and
  `scripts/jig work receipts` using plan `plan_01M00CKG98FC3NGG5R7JN16P1W`
  and `JIG_DEV_BIN=target/debug/jig`.

## Validation and Acceptance

- A preview reports create, same-kind replacement, and kind-changing replacement
  from one revision-bound source of truth.
- Any import batch containing `Concealed -> Text` requires exact `IMPORT TEXT`;
  ordinary imports still require exact `IMPORT` and dry runs remain non-committable.
- A failed atomic mutation keeps the prior item and field selection after refresh.
- Removing the vault before Unlock or Refresh transitions to `Missing`; a present
  but unreadable vault remains locked/fail-closed.
- Adding a `VaultAction` variant without handling it in the adapter fails to
  compile.
- Private-output policy conflicts retain exact current messages and classify by
  type even through context or when paths contain classifier phrases.
- Formatting, focused tests, Clippy, contract, configured tests, and work gates
  pass with no uncommitted source changes beyond final work evidence.

## Idempotence and Recovery

All source edits are ordinary patches. Each commit is a green revert boundary.
The core import API uses expand -> migrate, so work can stop after the additive
observation method without changing behavior. If a behavior slice fails, revert
only that commit and retain the last passing preparatory refactor. Structured
state files are append-only and must not be rewritten or truncated.

## Interfaces and Dependencies

No dependency is added. Planned interfaces are an additive prior-kind iterator on
`VaultImportPrecondition`, a closed `ImportFieldChange` TUI boundary type, private
snapshot/recovery policy enums, and a private output-conflict error enum. No vault
format, CLI flag, JSON contract, audit schema, or external service interface changes.
