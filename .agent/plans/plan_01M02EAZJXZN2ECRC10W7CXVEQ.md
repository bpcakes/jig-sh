# Implement high-payoff Rust refactorings

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current as implementation proceeds.

## Purpose

Implement the three accepted Fowler-style refactorings in their recommended dependency order. The result should make work-gate invalid states harder to represent, isolate the unsafe doctor signal lifecycle behind an explicit state machine, and centralize the Unix process-observation primitives shared by the owned-process, vault, and dev-proxy supervisors. This is behavior-preserving work: public CLI JSON, error text, exit behavior, signal restoration, cleanup deadlines, and process-ownership policy must not intentionally change.

Each slice must compile, pass its focused tests, and land in its own commit before work begins on the next slice.

## Progress

- [x] (2026-08-15) Read the repo and crate guides, Fowler Rust refactoring instructions, principles, and catalog.
- [x] (2026-08-15) Built the development `jig` binary and opened plan `plan_01M02EAZJXZN2ECRC10W7CXVEQ` in session `session_01M02EAZCJQ3P96FK27PM827G0`.
- [x] (2026-08-15) Slice 1: replaced internal work-gate JSON reparsing with typed evaluation/report types and explicit wire conversion; `cargo test -p jig-sh --lib --locked` passed 1,399 tests with 2 ignored, and strict crate Clippy passed. Commit pending at this checkpoint.
- [ ] Slice 2: extract the doctor signal lifecycle into a focused module, encode active/retired state without a boolean flag, test, and commit.
- [ ] Slice 3: add shared Unix process-safety primitives to `jig-owned-process`, migrate the specialized supervisors without merging their policy, test, and commit.
- [ ] Run configured repository gates, record evidence, close the plan/session, and summarize the three commits.

## Surprises & Discoveries

- The worktree already contained two append-only successful verification receipts from the preceding read-only audit. They must remain in `.agent/state/receipts.jsonl`; no source files were dirty when this implementation began.
- The harness reports several older open plans unrelated to this work. This plan is independently scoped and must not close or rewrite those records.
- The complete `jig-sh` library suite is intentionally broad and took 662 seconds because many environment-sensitive bootstrap/vault fixtures serialize behind shared locks; it nevertheless passed without failures.
- Review evidence historically projects `freshness_receipt_id` as null even though its review receipt is also the freshness source. The typed projection preserves that wire detail while check evidence continues to expose its batch freshness receipt id.

## Decision Log

- Decision: implement in the accepted order R-02, R-03, R-01.
  Rationale: the typed gate work is local and creates a fast first proof; the doctor split has moderate unsafe/lifecycle risk; the cross-crate process work has the broadest platform and ownership surface and therefore comes last.
- Decision: preserve JSON with explicit conversion methods at CLI/state boundaries rather than deriving a new wire format directly from domain types.
  Rationale: field presence, null values, strings, and error lists are a public compatibility boundary documented in `docs/public-contract.md`.
- Decision: keep dev-proxy and vault ownership policy specialized while sharing only validated identity, wait observation, snapshot classification, and proof-count primitives in `jig-owned-process`.
  Rationale: crate guides explicitly forbid replacing those supervisors with the generic owned-process policy.
- Decision: include the pre-existing append-only audit receipts with the first slice's repository-state changes rather than rewriting or discarding them.
  Rationale: `.agent/state/*.jsonl` is append-only repo memory and the receipts describe valid baseline checks.

## Context and Orientation

Work-gate evaluation lives in `crates/jig/src/runtime/work/gates.rs`. It currently constructs `serde_json::Value` objects and then reparses status strings and fields from those same values in `RequiredGateFailures`, evidence projection, and enforcement. The public JSON shape is exercised by `crates/jig/src/runtime/tests/work.rs` and `crates/jig/src/cli/output_tests.rs` and described in `docs/public-contract.md`.

Doctor logic lives primarily in `crates/jig/src/doctor.rs`, with tests in `crates/jig/src/doctor/tests.rs`. `DoctorSignalSession` installs process-global Unix handlers, tracks a generation, restores previous handlers, and coordinates cancellation. It is also used by Codex/agent launch paths, so the doctor module must continue to expose the same crate-private capability during the move.

Unix cleanup implementations live in `crates/jig-owned-process/src/lib.rs`, `crates/jig-vault/src/lib.rs`, and `crates/jig-dev-proxy/src/lib.rs`. The latter two deliberately own specialized cleanup policy. The safe shared boundary is lower-level: positive process-group identity, non-reaping child observation, macOS process-group snapshot classification, and consecutive quiescence proof state.

## Plan of Work

### Slice 1: typed work-gate evaluation

Introduce closed internal enums for gate outcome and freshness and typed structs/enums for evaluated check, review, and unsupported gates. Add a typed report that owns required-gate classifications and fingerprint errors. Build this model directly from receipts/config, use it for enforcement and evidence selection, and convert it to the exact existing `serde_json::Value` shape only at public return boundaries. Add characterization tests for every status/freshness mapping and representative full JSON output. Run formatting, the `jig-sh` library tests covering work gates/evidence/output, and a locked crate check. Commit the slice.

### Slice 2: doctor signal lifecycle extraction

Create `crates/jig/src/doctor/signal_session.rs`, move the signal statics, handler installation/restoration helpers, and `DoctorSignalSession` into it, and re-export the needed crate-private items from `doctor.rs`. Replace the independent `retired` boolean plus still-populated generation/actions with an `Option<ActiveDoctorSignalSession>` state so retired sessions cannot retain active restoration state. Preserve install order, rollback-on-partial-install, cancellation generation behavior, and Drop restoration. Move or adapt focused tests without weakening assertions. Run formatting, doctor/signal tests, and a locked `jig-sh` check. Commit the slice.

### Slice 3: shared Unix process-safety primitives

Add a narrowly scoped Unix module in `jig-owned-process` with a validated positive process-group identifier, non-reaping `waitid` observation, macOS snapshot classification, and a consecutive-quiescence proof counter whose required observation count cannot be zero. Migrate `jig-owned-process`, `jig-vault`, and `jig-dev-proxy` one at a time, retaining each caller's ECHILD, error-context, deadline, resignal, and ownership policy. Remove the now-duplicated local classifiers after each migration. Add unit tests for the shared state/classifiers plus focused cleanup tests in all three crates. Run formatting, focused crate tests, strict Clippy for affected crates, and locked checks. Commit the slice.

## Concrete Commands

From `/home/aa/Documents/jig-sh`:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    cargo fmt --all -- --check
    cargo test -p jig-sh --lib runtime::tests::work
    cargo test -p jig-sh --lib doctor::tests
    cargo test -p jig-owned-process -p jig-vault -p jig-dev-proxy
    cargo check --workspace --all-targets --all-features --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    scripts/jig work check --plan-id plan_01M02EAZJXZN2ECRC10W7CXVEQ
    scripts/jig work gates --plan-id plan_01M02EAZJXZN2ECRC10W7CXVEQ
    scripts/jig work evidence --plan-id plan_01M02EAZJXZN2ECRC10W7CXVEQ
    scripts/jig work receipts --plan-id plan_01M02EAZJXZN2ECRC10W7CXVEQ
    scripts/jig work finish --plan-id plan_01M02EAZJXZN2ECRC10W7CXVEQ --outcome success --summary "Implemented and independently committed the three accepted Rust refactoring slices."

Use `JIG_DEV_BIN=target/debug/jig` on each `scripts/jig` invocation if the shell environment is not persistent across commands.

## Validation and Acceptance

Acceptance requires three distinct commits in the stated order. Existing public gate-status and evidence JSON tests must remain byte-for-structure compatible, including missing/null fields and status/freshness strings. Doctor signal tests must demonstrate restoration after normal finish, failed start, and Drop and preserve cancellation isolation across generations. Process tests must cover invalid group IDs, wait-status classification, macOS snapshot classification through portable pure helpers, proof reset/satisfaction, and each specialized supervisor's existing cleanup behavior. The full workspace must format, compile with locked dependencies and all targets/features, pass strict Clippy, and pass configured Jig gates.

## Idempotence and Recovery

All source transformations are ordinary Git edits and each slice ends in a dedicated commit. If a slice fails, fix or revert only the uncommitted edits for that slice; do not rewrite `.agent/state/*.jsonl`. Re-running build, test, check, and `scripts/jig` gate commands is safe, though harness commands append new receipts. The signal and process refactors must proceed through compiling intermediate states so a failed move does not leave both old and new implementations active.

## Outcomes & Retrospective

Pending implementation.
