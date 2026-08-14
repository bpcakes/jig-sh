# Harden Vault TUI recovery state

This ExecPlan follows up the comprehensive review of branch `feat/vault-tui`. It reduces the state-machine surface that produced the recovery bugs, then fixes the independent keyboard and value-file omissions. The work is complete when the TUI never displays an unverified stale snapshot after a potentially state-changing failure, every passphrase character is enterable, loading cannot strand an absent vault on a locked screen, and a corrected value file can be retried without hidden form state.

## Progress

- [x] (2026-08-14) Read the repository and crate guidance plus the complete Fowler Rust refactoring skill and references.
- [x] (2026-08-14) Established a green focused baseline and ran the Fowler heuristic scanner against `master...HEAD`.
- [x] (2026-08-14) Committed this plan and structured-work records as `d17c229`.
- [x] (2026-08-14) Refactored backend completion and failure recovery into closed enums without changing behavior (`9af7762`).
- [x] (2026-08-14) Refactored locked/initializing protected-key routing into explicit handlers without changing behavior (`a04b4e7`).
- [x] (2026-08-14) Fixed failure recovery so state-changing actions refresh after any non-fatal failure and any failed recovery locks the session (`8cc28af`).
- [x] (2026-08-14) Fixed keyboard behavior so lowercase `q` is valid protected input and loading does not expose the unsafe hidden lock transition (`f1a22af`).
- [x] (2026-08-14) Staged value-file loading and validation in a temporary protected buffer so failed validation does not mutate the form (`437b135`).
- [x] (2026-08-14) Measured production-KDF latency and retained the existing credential-only session design.
- [ ] Run the configured Jig gates, inspect receipts and the final diff, and close structured work.

## Surprises & Discoveries

- The two recovery findings are symptoms of one deeper invalid-state design. `BackendCompletion` stores a primary `Result` beside `Option<Result<VaultSnapshot, VaultUiError>>`, so success-plus-recovery and failed-recovery combinations are representable and the consumer can accidentally ignore the second error.
- The 1Password partial-install symptom is not specific to importing. Any action that may change durable state can fail after its side effect or while taking the follow-up snapshot. Recovery policy should therefore follow operation semantics, not a short list of error kinds.
- The lowercase-`q` defect is amplified by a boolean `initializing` flag inside a shared key handler. Locked and initialization screens have distinct control transitions and should delegate only their common protected editing behavior.
- The value-file retry defect is a phase-order problem: file bytes are moved into live form state before validation finishes. A temporary `SecretInput` can encode load -> validate -> consume without exposing plaintext or adding allocation beyond the existing one-MiB protected buffer.
- Scanner reports about file length, exhaustive matches, DTO public fields, and test `unwrap` calls are non-findings for this change. The modules are cohesive, the matches cover closed enums, the records are presentation DTOs, and the panics are test/proven-invariant paths.
- The initial repository-wide `work check` reached the 1,960-test Nextest suite but returned the harness's generic exit 100 after roughly twelve minutes without a vault-related failure in its captured preview. The focused vault/TUI baseline, workspace formatting, targeted all-target check, and targeted strict Clippy run were all green; final acceptance will rerun the configured gates after the changes.
- Production Argon2id parameters are 131,072 KiB memory, 3 iterations, parallelism 4. On this host, an optimized fresh binary measured init at 0.32 s, five metadata snapshots at 0.28/0.29/0.29/0.28/0.28 s (0.28 s median), and five mutations at 0.31/0.33/0.33/0.34/0.32 s (0.33 s median). A TUI mutation plus its snapshot therefore costs roughly 0.61 s median. The unoptimized dev binary took about 5.9-6.0 s per KDF, which is a build-profile artifact rather than deployed latency.

## Decision Log

- Decision: use private closed enums for backend outcome and recovery state rather than another trait or optional field.
  Rationale: the variant set is closed, exhaustive matching is valuable, and invalid success/recovery combinations should be unrepresentable.
  Date/Author: 2026-08-14 / Codex

- Decision: separate behavior-preserving preparation commits from bug-changing commits.
  Rationale: this follows Fowler's two-hats protocol and makes every functional change independently reviewable and revertible.
  Date/Author: 2026-08-14 / Codex

- Decision: attempt metadata recovery after every non-fatal failure from an operation that may alter durable vault or audit state.
  Rationale: error kind does not reliably identify whether a side effect occurred. An extra KDF on an uncommon failure is safer than presenting an unverified snapshot.
  Date/Author: 2026-08-14 / Codex

- Decision: lock on every failed recovery refresh, not only authentication or audit failures.
  Rationale: once a potentially state-changing action and its recovery both fail, the browser cannot prove its metadata is current. Re-unlock is the smallest fail-closed transition.
  Date/Author: 2026-08-14 / Codex

- Decision: remove the undocumented `L` action from `Screen::Loading`.
  Rationale: loading already supports quit, which joins the worker and exits safely. Locking discards the completion and cannot distinguish successful from failed absent-vault initialization or restore.
  Date/Author: 2026-08-14 / Codex

- Decision: retain only the passphrase in the process-local backend session; do not add a derived-key cache in this change.
  Rationale: the optimized production-parameter path keeps ordinary reads near 0.28 s and two-KDF TUI writes near 0.61 s while the operation runs off-thread with visible progress. Caching a derived key would expand the secret lifetime and vault public API for insufficient user-visible benefit.
  Date/Author: 2026-08-14 / Codex

## Outcomes & Retrospective

Not complete. Update this section after the implementation, latency measurement, and repository gates.

## Context and orientation

`crates/jig-vault-tui/src/runtime.rs` owns the event loop, worker completion, failure-to-screen transitions, and key routing. `crates/jig-vault-tui/src/model.rs` owns form submission and protected input state. `crates/jig/src/runtime/vault/tui.rs` is the CLI-owned backend that performs core vault calls and may return an error after a durable side effect, notably when an import commits fields before generated dotenv installation fails.

The matching public boundary in `crates/jig-vault-tui/src/lib.rs` is same-release but still treated as a public Rust API. This work must not change `VaultBackend`, `VaultAction`, `VaultActionResult`, persistent vault/audit formats, CLI flags, error kinds, secret lifetimes, lock scope, terminal restoration, or the one-worker ownership contract. All new completion types remain private to `runtime.rs`.

## Plan of work

First apply **Encapsulate Record** and **Replace Type Code with Subclasses** in their Rust form: replace the independent completion fields with `BackendOutcome` and `BackendFailure` enums, move recovery construction behind `BackendCompletion`, and decompose success/failure application into focused functions. Preserve the existing narrow refresh policy in this commit.

Second apply **Remove Flag Argument** and **Extract Function** to protected key handling. Give Locked and Initialize their own control-key handlers and share only backspace, clear, and character insertion. Preserve the current `q` behavior until the later functional commit.

Third change behavior. Give `OperationKind` the authoritative query for whether an operation can change durable state. For any non-authentication/audit error from those operations, attempt refresh. A refreshed snapshot returns to Browse with the primary error; a failed refresh drops the credential and snapshot and reports a combined metadata-only error on Locked. Add characterization tests for secondary authentication/audit failure and import `Io` recovery.

Fourth change keyboard behavior. Treat every unmodified character, including lowercase `q`, as protected input; keep Esc and Ctrl-C as quit controls. Ignore `L` while loading. Update help text and add direct key-routing tests.

Fifth apply **Split Phase** to form value collection. Load a file into a local `SecretInput`, validate the selected kind, and only then consume the bytes into `VaultAction`. Add a test that fails with a three-byte concealed file, rewrites the same file with valid bytes, and succeeds on the next Enter.

Finally measure production Argon2 operations with the freshly built `target/debug/jig` against an owner-only temporary vault. Record init, metadata read, and mutation timings and infer the current TUI mutation cost from its mutation-plus-snapshot calls. Do not add a derived-key session API unless the measurement demonstrates an unusable interaction cost; that security-sensitive optimization is outside a mere cleanup.

## Concrete steps and commit boundaries

1. Commit the ExecPlan and append-only work records. Run `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KZZSQ476RPW0TB5EXTNP03M0` first.
2. Refactor completion types only. Run `cargo test -p jig-vault-tui` and strict crate Clippy. Commit as `refactor(vault-tui): model worker recovery states`.
3. Refactor protected key routing only. Run the same narrow checks. Commit as `refactor(vault-tui): separate protected key modes`.
4. Add recovery behavior and tests. Run `cargo test -p jig-vault-tui` plus `cargo test -p jig-sh --test vault_tui`. Commit as `fix(vault-tui): fail closed after recovery errors`.
5. Add keyboard behavior and tests. Run `cargo test -p jig-vault-tui`. Commit as `fix(vault-tui): preserve protected input semantics`.
6. Add staged file validation and retry tests. Run `cargo test -p jig-vault-tui`. Commit as `fix(vault-tui): stage value file validation`.
7. Measure production KDF cost, update this plan's discoveries/outcome, and commit the evidence-oriented documentation/state slice separately if files change.
8. Build the dev binary, run `scripts/jig work gates`, inspect evidence and receipts, update this plan, finish the work, and commit final append-only state/plan records.

## Validation and acceptance

The focused baseline before edits is green:

    cargo fmt --all -- --check
    cargo check -p jig-vault-tui -p jig-tui -p jig-sh --all-targets
    cargo test -p jig-vault-tui -p jig-tui
    cargo test -p jig-sh --test vault_tui
    cargo clippy -p jig-vault-tui -p jig-tui -p jig-sh --all-targets -- -D warnings

Final acceptance uses the repository contract through the fresh runtime binary:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KZZSQ476RPW0TB5EXTNP03M0
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KZZSQ476RPW0TB5EXTNP03M0
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01KZZSQ476RPW0TB5EXTNP03M0
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01KZZSQ476RPW0TB5EXTNP03M0

Acceptance requires all configured gates green, a clean `git diff --check`, exact slice commits, no plaintext in model/debug/render/error paths, and no changes to public or persistent formats.

## Idempotence and recovery

All refactors are internal and compiler-guided. If a slice fails, restore only that uncommitted slice to its preceding green commit; never rewrite `.agent/state/*.jsonl`. File-based tests use temporary directories. The production-KDF measurement must use a uniquely created owner-only directory under `/tmp` and delete only that exact validated directory after measurement.

If a behavior-changing step cannot be proven green, stop at the last committed behavior-preserving state and record the blocker here. Do not combine a failed slice with the next fix.

## Interfaces and dependencies

No new dependency, feature, trait, dynamic dispatch, allocation policy, unsafe block, async runtime, serialization type, or public API is expected. The private target types are `BackendOutcome`, `BackendFailure`, and explicit locked/initializing key handlers in `crates/jig-vault-tui/src/runtime.rs`; the staged value helper remains private to `crates/jig-vault-tui/src/model.rs`.
