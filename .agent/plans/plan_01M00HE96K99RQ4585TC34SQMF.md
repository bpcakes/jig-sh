# Close Vault TUI review findings

Apply Fowler-style preparatory refactors and separately committed fixes for the
remaining Vault TUI review findings. The observable outcome is that no
plaintext/private output can replace backend-owned vault state, a durable action
is never reported as wholly failed merely because its trailing metadata refresh
failed, every operation reconciles a vanished vault consistently, and closed TUI
commands cannot silently acquire incomplete dispatch behavior.

## Progress

- [x] Read repository and crate guidance plus the Fowler Rust refactoring
  principles and catalogs.
- [x] Run the changed-code heuristic scanner and reject metric-only candidates.
- [x] Record a clean formatting, focused-test, and strict-Clippy baseline.
- [ ] Centralize vault-owned output-path policy without changing behavior.
- [ ] Enforce that policy at every private/plaintext file sink.
- [ ] Model durable completion separately from post-operation snapshot refresh.
- [ ] Report committed actions accurately when snapshot refresh fails.
- [ ] Centralize missing-vault reconciliation for all backend operations.
- [ ] Make tool dispatch exhaustive and require unmodified preview shortcuts.
- [ ] Run focused checks, configured gates, and inspect final evidence.

## Surprises & Discoveries

- `PreparedPrivateFile` correctly hardens generic destination installation, but
  it cannot know which paths a `VaultStore` owns. Backup contains a local
  source-path guard while reveal, template injection, export, and import do not.
  The repeated use of raw `PathBuf` therefore loses a domain invariant rather
  than merely missing one call-site check.
- Durable operations and their presentation refresh are currently flattened into
  one `Result`. Import, mutation, migration, backup, export, and passphrase
  rotation can commit before `refresh()` fails, so the type cannot state what
  happened and the runtime necessarily reports the wrong outcome.
- Missing-vault recovery is selected from an operation-name table. Read-only
  activity/audit/import-preview paths can therefore retain authenticated UI state
  after storage disappears even though all failures pass through one completion
  boundary.
- The modifier and tool-dispatch findings are local omissions. They still benefit
  from closed helper/enum APIs so the compiler and focused tests prevent repeats.
- The Fowler scanner reported its cap of 200 candidates across the branch. Most
  are cohesive long tests, test `unwrap`s, transparent DTO fields, boolean facts,
  and file-size metrics. Manual inspection found no current change pressure that
  justifies broad cleanup outside these reviewed boundaries.
- The pre-change baseline is green: `cargo fmt --all -- --check`; all 219
  `jig-vault` and 49 `jig-vault-tui` tests; and strict Clippy for `jig-vault`,
  `jig-vault-tui`, and `jig-sh` across all targets.

## Decision Log

- Treat output ownership as a high-impact cross-module **Primitive Obsession** /
  **Shotgun Surgery** problem. Apply Fowler's **Move Function** and **Replace
  Primitive with Object** in two hats: first establish one store-owned policy
  behind existing backup behavior, then separately reject vault-home sinks from
  reveal/export/import paths.
- Treat operation completion as an invalid-state model. Apply **Split Phase** and
  **Replace Type Code with Subclasses** in Rust form: a closed enum distinguishes
  a current snapshot from a committed action whose refresh failed. Preserve
  plaintext-free public results and avoid success encoded as an error string.
- Centralize missing-vault recovery at the completion boundary using a closed
  policy enum and authoritative `presence()` query. A field-level `NotFound`
  remains an ordinary failure when the vault itself is still present.
- Keep structural changes and behavior fixes in separate commits. Preserve the
  existing core public APIs through additive expand -> migrate steps where a new
  boundary is required.
- Do not change the vault format, audit schema/order, CLI JSON contract, dependency
  graph, Rust 1.85 MSRV, terminal secrecy boundary, or platform hardening policy.

## Outcomes & Retrospective

Work is in progress. This section will record the final root-cause assessment,
commit boundaries, verification receipts, compatibility effects, and any
deferred risks once every slice is green.

## Context and Orientation

`crates/jig-vault/src/store.rs` owns the vault home and reserved filesystem
entries. `output.rs` owns generic hardened private-file installation,
`backup.rs` owns encrypted backup creation, and `vault.rs` owns audited reveal
and template-injection lifecycles. `crates/jig-vault-tui/src/lib.rs` defines the
metadata-only backend protocol; `runtime.rs` owns worker completion and UI state;
`model.rs` owns closed tool transitions. `crates/jig/src/runtime/vault/tui.rs`
adapts fixed-scope TUI actions to the core vault and performs import/export work.

The workspace uses Rust 2024 with Rust 1.85 MSRV. `jig-vault` is a public library;
the TUI crate is same-release internal but exposes public Rust types. Serialized
vault state, audit ordering, error secrecy, blocking behavior, Unix filesystem
guarantees, Windows rejection behavior, and the one-worker/credential lifecycle
are compatibility-sensitive. There is no async, FFI, or new unsafe work in scope.

## Plan of Work

1. Move backup's reserved-source validation to the store owner behind unchanged
   backup behavior, add characterization coverage, verify `jig-vault`, and commit.
2. Introduce a validated external-destination boundary and migrate reveal,
   template injection, CLI/TUI export, and import destination paths. Add regression
   tests proving `vault.json`, `audit.jsonl`, `vault.lock`, and paths beneath the
   home remain unchanged; verify and commit the behavior change separately.
3. Introduce a closed operation-completion value at the TUI backend boundary while
   preserving current behavior. Migrate the worker/runtime exhaustively, verify,
   and commit the refactor.
4. Make the adapter return committed metadata when only the trailing refresh
   fails. Test mutation and file-producing actions, verify, and commit.
5. Route every backend error through one authoritative presence reconciliation.
   Preserve entity-level `NotFound` when storage is present; test read-only and
   peek paths after vault removal; verify and commit.
6. Replace wildcard/impossible tool dispatch with closed enum methods and require
   `KeyModifiers::NONE` for preview toggles, using separate commits for refactor
   and behavior.
7. Build the development Jig binary, run configured checks through `JIG_DEV_BIN`,
   inspect receipts and diffs, update this living plan, and close structured work.

## Concrete Steps

- For each slice, add or identify the smallest characterization test, use
  `apply_patch`, run the narrow owning-crate test, run `cargo fmt --all -- --check`,
  inspect `git diff --check` and the staged diff, then commit non-interactively.
- For runtime changes, run `cargo build -p jig-sh --bin jig` before repository
  harness commands and set `JIG_DEV_BIN=target/debug/jig`.
- Finish with `scripts/jig work check`, `scripts/jig work gates`,
  `scripts/jig work evidence`, `scripts/jig work receipts`, and
  `scripts/jig work finish` for `plan_01M00HE96K99RQ4585TC34SQMF`.

## Validation and Acceptance

- Every plaintext/private destination under the selected vault home is rejected
  before installation; reserved vault, audit, and lock bytes/identity remain
  intact even with overwrite enabled.
- External destinations preserve existing private-file overwrite, symlink,
  identity, permission, and atomicity behavior.
- A committed operation followed by refresh failure displays a committed/success
  outcome plus refresh warning, reconciles session state safely, and is never
  offered as if retrying the primary action were harmless.
- Removing the vault before any read-only or mutating backend completion drops the
  credential and authenticated snapshot and shows `Missing`; a missing field in a
  present vault remains an ordinary action error.
- `Ctrl`/`Alt` variants of import-preview toggle keys have no effect.
- Adding a new closed tool/action variant without dispatch behavior fails to
  compile.
- Formatting, focused tests, strict Clippy, configured tests, contract, and work
  gates pass.

## Idempotence and Recovery

All source edits are ordinary patches and each commit is a green revert boundary.
The structural commits preserve behavior and may remain even if a later behavior
slice is reverted. Filesystem regressions use disposable temporary homes and
sentinel bytes; they never target a real vault. Append-only `.agent/state/*.jsonl`
files must not be rewritten or truncated.

## Interfaces and Dependencies

No dependency or persistent-format change is planned. Core API additions, if
needed, follow expand -> migrate and preserve existing methods. The likely new
interfaces are a store-owned external-destination validator and a closed
metadata-only TUI operation outcome. No plaintext enters those types.
