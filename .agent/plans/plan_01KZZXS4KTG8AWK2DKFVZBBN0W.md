# Reduce Vault TUI bug surfaces after comprehensive review

This work closes the actionable branch-review findings without treating every symptom as an architectural rewrite. The observable outcome is that an operator commits exactly the import they previewed, lifecycle failures reconcile against authoritative vault presence, audit degradation remains visible, destructive edit intent is explicit, platform file opening honors the documented security boundary, and local policy omissions are removed.

## Progress

- [x] Establish the pre-change baseline and run the Fowler Rust heuristic scanner.
- [x] Introduce exact destination and vault-field preconditions behind compatible APIs.
- [x] Replace the import preview/commit flag protocol with an opaque one-shot plan.
- [x] Reconcile initialize and restore failures against a non-creating presence query.
- [x] Preserve audit verification in the activity result.
- [x] Skip no-op kind mutations before audit/save and require explicit empty replacement intent.
- [x] Harden protected file opening and TUI credential environment consumption.
- [x] Correct test partition/platform coverage and remove unused/dead surface.
- [x] Run focused checks after every slice and finish with configured Jig gates.

## Surprises & Discoveries

- The branch starts green: `scripts/jig check fmt`, scoped `cargo check`, scoped strict Clippy, all 37 `jig-vault-tui` tests, and all 209 `jig-vault` tests pass.
- The scanner reported 200 capped heuristic candidates. DTO public fields, exhaustive closed-enum matches, test-only `unwrap`s, and file length alone are explicit non-findings for this work.
- Import approval currently spans two independently mutable resources: vault fields and the generated dotenv destination. Exact authorization therefore needs preconditions at both owning boundaries, not only a source-file digest in the presentation layer.
- `PreparedPrivateFile` already rechecks ordinary path safety at installation, but its boolean overwrite policy means “replace the previewed file” and “upsert whatever is there now” are not distinguishable.
- The TUI activity API discarded `AuditVerification` after successfully deriving it, so a recoverable torn audit tail became invisible precisely at the presentation boundary.
- The Windows workspace already pins the required reparse-point APIs; the omission was local to the new TUI crate rather than a missing platform abstraction across the repository.

## Decision Log

- Treat import drift as a **Split Phase** / **Replace Flag Argument** design gap. Keep parsed protected source material in a backend-owned one-shot plan; expose only an opaque token and metadata to the TUI.
- Apply **Replace Primitive with Object** at mutation boundaries: an opaque vault import precondition and an exact private-file destination precondition make stale approval reject instead of widening overwrite permission.
- Treat lifecycle recovery as **Move Function** / **Separate Query from Modifier**: the backend owns authoritative non-creating presence checks; the model consumes a typed presence result.
- Treat empty replacement as a real state transition, not an empty `SecretInput`: an explicit confirmation screen distinguishes accidental empty submission from deliberate clearing.
- Keep behavior-changing commits separate from preparatory structural commits, with a narrow green check before each commit.
- Preserve existing public compatibility wrappers where a new exact API can be added without forcing unrelated callers to migrate.
- Keep verification and projected audit records in one `VerifiedVaultActivity` result so future consumers cannot accidentally forget verification state.
- Add a conditional audited-edit seam first, then use it for field-kind no-ops; this preserves the audit-before-state invariant while avoiding false mutation events and nonce churn.

## Outcomes & Retrospective

Implementation is complete in fifteen independently verified commits after the planning commit. The deeper issues were concentrated at four boundaries—split-phase authorization, lifecycle state inference, incomplete verification results, and ambiguous edit intent. Platform environment/file handling, test partitioning, an unused dependency, and an inert marker were local omissions. No persisted vault or audit format changed, and compatibility entrypoints remain for non-TUI callers.

The final `jig work check` passed the contract and complete two-part nextest suite with batch receipt `receipt_01M001PYSE04ZWKQPWV2TS6Q91`; `jig work gates` reported both required gates fresh with no receipt diff. Final formatter and workspace Clippy checks passed with receipts `receipt_01M001QH4J5DSWRHQ72WQXXSSC` and `receipt_01M001R9BGXS96KCXGF9MH5DQ3`. The Windows reparse test remains as archival implementation coverage. The later platform-support policy superseded the temporary cross-platform CI matrix described here, so supported-host CI no longer selects that test.

## Context and orientation

The presentation API is in `crates/jig-vault-tui/src/lib.rs`; the state machine is in `model.rs` and `runtime.rs`; the matching fixed-scope adapter is `crates/jig/src/runtime/vault/tui.rs`. Core lock-scoped vault mutations live in `crates/jig-vault/src/vault.rs`, private output installation in `crates/jig-vault/src/output.rs`, and TUI value-file capture in `crates/jig-vault-tui/src/secret_input.rs`.

The branch is internal release-coupled code at workspace version 0.2.0. Persisted vault/audit formats, CLI compatibility, secret redaction, lock ordering, and the Rust 1.85 MSRV must remain unchanged. Linux, macOS, and Windows compilation remain supported; private output remains fail-closed where its existing platform contract is unsupported.

## Plan of work

1. Add destination and vault import precondition types behind existing compatibility entrypoints. Characterize stale state rejection at the owning crates.
2. Migrate only the TUI import flow to a one-shot backend-owned plan. Source bytes are parsed once; destination and field state are revalidated by the core APIs; stale or reused tokens require a new preview.
3. Add a typed backend presence query and make initialize/restore failure completion reconcile before choosing `Missing` or `Locked`.
4. Return audit verification together with projected activity records and render torn-tail degradation explicitly.
5. Centralize no-op mutation skipping in the audited edit boundary, then add an explicit empty-text replacement confirmation in the model.
6. Make protected file opening atomic against Windows reparse traversal, consume TUI passphrase environment variables before parsing, and add platform/negative tests.
7. Update the crypto-heavy nextest filters and platform job to include `jig-vault-tui`; remove the unused dependency and inert marker separately.

## Concrete steps

Each numbered implementation slice receives one commit after its focused formatter, compile, Clippy, and relevant unit tests are green. Preparatory API refactors and subsequent behavior changes remain separate commits when both are required. If a slice cannot be proven green, restore only that slice to its preceding commit and continue from the last verified state.

## Validation and acceptance

- Focused: `cargo test -p jig-vault`, `cargo test -p jig-vault-tui`, and affected `jig-sh` module tests.
- Static: `cargo fmt --all -- --check` and `cargo clippy -p jig-vault -p jig-vault-tui -p jig-sh --all-targets --locked -- -D warnings`.
- Repository: build `target/debug/jig`, then run `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KZZXS4KTG8AWK2DKFVZBBN0W` and configured gates/evidence.
- Acceptance tests cover source drift, collision drift, destination identity drift, plan reuse, lifecycle presence races, torn audit tails, deliberate empty clearing, no-op kind changes, invalid passphrase encoding, and platform link refusal where supported.

## Idempotence and recovery

All new preview plans are process-local and one-shot; dropping or locking the backend clears protected plan material. No migration or persisted format change is introduced. Every commit is intended to be independently revertible, and existing compatibility methods remain available for non-TUI callers.

## Interfaces and dependencies

No new third-party dependency is expected. Existing `ulid`, `windows-sys`, `SecretBytes`, lock-scoped vault APIs, and private output primitives are sufficient. Any Windows-specific flags must use the workspace-pinned `windows-sys` crate and compile at the declared MSRV.
