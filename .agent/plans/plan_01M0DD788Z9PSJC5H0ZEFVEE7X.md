# Harden Dev Lifecycle Outcome Composition

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current while the work proceeds.

## Purpose / Big Picture

`jig dev stop` must never lose already-committed orphan-recovery evidence when a later lifecycle operation fails, and a failed development preflight must remain the primary error even if persisting its cleanup confirmation also fails or is interrupted. After this work, both rules are expressed by closed internal outcomes and an explicit result-composition policy instead of relying on callers to inspect an `anyhow::Error` wrapper or on the order of `?` operators.

The public `jig_dev_proxy` command facade remains `anyhow::Result<serde_json::Value>`. Successful JSON shapes remain unchanged. A direct-stop failure after a committed recovery becomes a structured `ok: false` response containing both the standard command error object and the recovery records, so the CLI can emit one truthful report and a nonzero status.

## Progress

- [x] (2026-08-19) Reproduced both review findings and established a green focused baseline during the comprehensive review: all `jig-dev-proxy` and `jig-sh` tests passed.
- [x] (2026-08-19) Read the repository and crate guides, the Fowler Rust refactoring skill and catalog, and ran its diff scanner against `origin/master`.
- [x] (2026-08-19) Built the development `jig` binary and opened structured work with plan `plan_01M0DD788Z9PSJC5H0ZEFVEE7X`.
- [ ] Introduce a closed failed variant at the stop-session phase boundary without changing command behavior.
- [ ] Serialize recovery-bearing direct-stop failures at the owning command boundary and add regression coverage.
- [ ] Compose preflight and cleanup-confirmation results with an explicit primary-error policy and add regression coverage.
- [ ] Run focused checks after every slice, then all configured gates and the full test suite with the development binary.

## Surprises & Discoveries

- Observation: an earlier hardening effort correctly moved recovery context out of the process runner, but the stop phase still returns that context inside an opaque `anyhow::Error` wrapper.
  Evidence: `stop_session_ids_interruptible_with_policy` collects typed `OrphanRecoveryNotice` values, then converts them to `DevErrorWithRecoveries`; only `dev_api::normalize_dev_result` knows to extract them, and `dev_stop` does not call that normalizer.
- Observation: the wrapper forwards its complete source display while also returning that source from `std::error::Error::source`.
  Evidence: alternate chain formatting can therefore print the same source chain twice.
- Observation: `finish_preflight_cleanup` computes the primary preflight result only after a fallible cleanup-confirmation phase.
  Evidence: either `?` in confirmation can return before `normalize_preflight_result`, so statement order silently gives cleanup bookkeeping priority over a preflight failure.
- Observation: the Fowler scanner reports 200 heuristic candidates in the large unpublished diff and is truncated; the relevant confirmed signal is the 71-line `normalize_dev_result` split-phase candidate. Large-file, clone, test-unwrap, and DTO-field findings are not evidence for this repair and are deferred.

## Decision Log

- Decision: classify both findings as manifestations of a structural lifecycle-outcome design weakness, not isolated omissions.
  Rationale: correctness currently depends on unwritten caller obligations at two phase boundaries; the same bug class can recur when a new exit path uses `?` or bypasses one normalizer.
  Date/Author: 2026-08-19 / Codex
- Decision: apply Fowler's **Split Phase** and Rust's closed-enum adaptation of **Replace Error Code with Exception** at the stop-session boundary.
  Rationale: stop already has typed complete and cancelled outcomes; adding a typed failed outcome makes accumulated recovery side effects impossible to ignore in an exhaustive match.
  Date/Author: 2026-08-19 / Codex
- Decision: preserve preflight errors exactly and report a secondary confirmation failure diagnostically when both fail.
  Rationale: preflight is the operation requested by the caller; cleanup confirmation is secondary bookkeeping. The durable session record already preserves the unconfirmed cleanup obligation, while returning the original error preserves its message and downcast/interruption classification.
  Date/Author: 2026-08-19 / Codex
- Decision: keep public and persisted compatibility, and separate the preparatory refactor, direct-stop behavior fix, and preflight precedence fix into individual commits.
  Rationale: the crate guide permits the existing public facade, while small commits make behavioral changes independently reviewable and reversible.
  Date/Author: 2026-08-19 / Codex

## Outcomes & Retrospective

Pending implementation and final verification.

## Context and Orientation

`crates/jig-dev-proxy/src/dev_sessions/management.rs` owns stop-session lifecycle work and accumulates orphan recovery notices. `crates/jig-dev-proxy/src/dev_sessions.rs` consumes that phase during replacement. `crates/jig-dev-proxy/src/dev_outcome.rs` carries recovery metadata across the foreground-dev command boundary. `crates/jig-dev-proxy/src/processes/dev_session.rs` closes the caller-owned preflight phase and persists cleanup confirmation. CLI human rendering for stop results lives in `crates/jig/src/cli/output/dev.rs`. Lifecycle regressions live primarily in `crates/jig-dev-proxy/src/lib/dev_lifecycle.rs` and `crates/jig-dev-proxy/src/processes/tests/dev_session.rs`.

## Plan of Work

First add `StopSessionOutcome::Failed { error, recoveries }` and make the stop phase return its closed outcome directly. Adapt replacement and the current direct-stop wrapper path exhaustively, preserving behavior. This is the behavior-preserving ownership refactor.

Next make the direct-stop command consume the typed failed outcome itself and produce a structured failure value containing repository identity, known matched-session count, the command error, and committed recoveries. Update human rendering to include that error and avoid inventing unavailable completion counts. Add deterministic unit and command-boundary regressions, including an error-chain check.

Then split preflight result normalization from cleanup-confirmation persistence and combine the two `Result` values in one focused helper. If preflight failed, return that exact error and only diagnose a secondary confirmation failure; if preflight succeeded, confirmation failure remains fatal. Add characterization tests for all precedence combinations and preserve interruption classification.

Finally rebuild `jig`, run focused tests, formatting, Clippy, the configured full test gate, contract gate, evidence and receipt inspection, and finish the structured work.

## Concrete Steps

Run commands from the repository root.

1. Use `apply_patch` for source and plan edits. After each slice run the narrow `cargo test -p jig-dev-proxy <filter>` checks, `cargo fmt --check`, inspect `git diff`, and commit only that slice.
2. Build with `cargo build -p jig-sh --bin jig`; use `JIG_DEV_BIN=target/debug/jig` for every `scripts/jig` work command.
3. Run `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0DD788Z9PSJC5H0ZEFVEE7X` and `scripts/jig work gates` for the final full suite, then inspect `work evidence`, `work receipts`, and `work status` before `work finish`.

## Validation and Acceptance

Acceptance requires tests proving that a stop-phase failure cannot discard accumulated recovery notices, direct stop renders recovery-bearing failure as one structured report without duplicating its source chain, and preflight failure wins over either confirmation interruption or confirmation I/O failure. Existing stop success, replacement recovery, cancellation, and preflight cleanup tests must remain green. The full configured test, format, Clippy, and contract gates must pass with the freshly built development binary.

## Idempotence and Recovery

Tests, builds, and configured checks are safe to rerun. `.agent/state/*.jsonl` remains append-only and is changed only through `scripts/jig`. Each code slice receives its own commit; if a slice fails verification, repair the uncommitted slice rather than rewriting earlier commits. No persisted dev-session schema or public Rust API migration is required.

## Interfaces and Dependencies

Keep `jig_dev_proxy::{dev, dev_status, dev_stop}` signatures unchanged. Add no dependencies and do not change the persisted dev-session schema. `OrphanRecoveryNotice` remains the typed recovery record. Structured command failures use the existing `{ "kind": "command_failed", "message": ... }` envelope.
