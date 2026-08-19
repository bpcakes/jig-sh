# Harden Dev Outcome Reporting

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current while the work proceeds.

## Purpose / Big Picture

Foreground `jig dev --replace` must report every orphan recovery it completes, even when replacement later fails or is cancelled, and every failed JSON result must retain the established structured error envelope. After this change, operators and automation can trust one result shape and do not lose audit evidence merely because failure occurred between recovery and construction of a new process runtime.

The root cause is structural rather than a pair of unrelated omissions. Recovery evidence is currently attached by the process-runtime module, but replacement recovery starts in `dev_sessions` before that runtime exists. Separately, `dev_api` sometimes constructs a complete failed result while the CLI constructs ordinary failure envelopes, so adding recovery metadata accidentally bypassed the canonical error shape. The implementation will introduce a command-level recovery-bearing outcome boundary, make replacement's pre-claim phase exhaustive, and centralize dev failure serialization.

## Progress

- [x] (2026-08-19) Reproduced the relevant control flow and reviewed the unpublished branch with independent Codex and Claude passes.
- [x] (2026-08-19) Ran the Fowler Rust refactoring scan and inspected the lifecycle/change topology.
- [x] (2026-08-19) Established a green baseline: `cargo check -p jig-dev-proxy`, `cargo test -p jig-dev-proxy`, and `cargo test -p jig-sh --lib`.
- [x] (2026-08-19) Ran the configured workspace baseline: 2,025 of 2,026 selected tests passed; the sole bootstrap failure passed immediately when rerun in isolation and is tracked below.
- [x] (2026-08-19) Moved recovery-bearing error context to the command/outcome boundary without behavior change; all 562 `jig-dev-proxy` tests pass.
- [ ] Preserve typed recovery evidence through partial stop, cancellation, second-claim conflict, and second-claim error exits.
- [ ] Emit one stable structured error object for every dev failure and keep human output compatible.
- [ ] Run all configured gates with `JIG_DEV_BIN=target/debug/jig`, inspect receipts and diff, and finish structured work.

## Surprises & Discoveries

- Observation: the earlier recovery-retention fix only covers failures after a successful second claim, because `DevSessionRuntime` receives recovery notices only after that claim succeeds.
  Evidence: `DevSessionRuntime::start_interruptible` collects recoveries locally and only transfers them into `Self` in the `Claimed` arm.
- Observation: recovery-bearing ordinary errors are serialized inside `dev_api`, while ordinary errors without recoveries fall through to the CLI JSON serializer.
  Evidence: `normalize_dev_result` returns `Err` for one path and an ad hoc failed JSON `Value` for the other.
- Observation: the initial configured `work check` reported one failure in `update_recopy_normalizes_legacy_schema_dump_true_when_sqlx_disabled` after 2,025 passes; the exact test passed on an immediate isolated rerun.
  Evidence: nextest run `3e7d02d5-d8ca-4005-8c54-7b8d4b79679e` failed only that test, while isolated run `92e03451-c03c-463c-8719-6c5fb0a322bf` passed. Treat this as a pre-existing flaky/environment-sensitive baseline signal and require the final configured gate to pass cleanly.

## Decision Log

- Decision: treat this as a lifecycle/outcome ownership defect, not as isolated conditional omissions.
  Rationale: both failures occur precisely where recovery metadata crosses phase and crate boundaries; adding more local branches would leave future exits vulnerable.
  Date/Author: 2026-08-19 / Codex
- Decision: preserve the public `anyhow::Result<Value>` facade while strengthening internal closed outcome types.
  Rationale: callers need no migration, while the compiler can force every replacement exit to account for accumulated recoveries.
  Date/Author: 2026-08-19 / Codex
- Decision: separate the preparatory refactor, recovery behavior fix, and JSON contract fix into independently verified commits.
  Rationale: this follows the two-hats rule and keeps regressions bisectable.
  Date/Author: 2026-08-19 / Codex

## Outcomes & Retrospective

Pending implementation and final verification.

## Context and Orientation

`crates/jig-dev-proxy/src/dev_sessions.rs` owns registry claims and replacement recovery before a process starts. `crates/jig-dev-proxy/src/processes/dev_session.rs` owns the running dev process and currently attaches recovery evidence only after runtime construction. `crates/jig-dev-proxy/src/dev_api.rs` maps internal outcomes into JSON. `crates/jig/src/cli/output/dev.rs` renders that JSON for humans. Lifecycle regression tests live mainly in `crates/jig-dev-proxy/src/lib/dev_lifecycle.rs` and process-session tests live under `crates/jig-dev-proxy/src/processes/tests/`.

## Plan of Work

First extract the recovery-bearing error wrapper and inspection helpers from the process module into a command-level outcome module. This is a behavior-preserving move and establishes the correct ownership boundary.

Next introduce a closed result for the pre-runtime session-start phase so a cancellation can carry accumulated recoveries. Wrap partial-stop failures, second-claim conflicts, and second-claim errors with the same accumulated evidence. Add deterministic regression coverage for exits that occur after recovery but before runtime construction.

Then make `dev_api` the single serializer for dev failures that need structured metadata, using the same `{kind, message}` error object for ordinary, interrupted, and cleanup-unconfirmed failures. Update the human renderer to read the nested message and document the JSON contract.

Finally build `jig-sh`, set `JIG_DEV_BIN=target/debug/jig`, run `scripts/jig work check`, all configured gates, evidence and receipt inspection, and `scripts/jig work finish`. Review commit boundaries and the final diff.

## Concrete Steps

Run all commands from `/Users/aa/Documents/jig-sh`.

1. Build with `cargo build -p jig-sh --bin jig` and use `JIG_DEV_BIN=target/debug/jig` for every harness command.
2. After each slice, run focused `cargo test` targets and `cargo fmt --check`, inspect `git diff`, and commit only that slice.
3. At the end run `scripts/jig work check --plan-id <plan-id>` and `scripts/jig work gates --plan-id <plan-id>`, followed by evidence, receipts, and status inspection.

## Validation and Acceptance

Acceptance requires regression tests proving that completed recoveries survive a subsequent replacement failure/cancellation and that all failed JSON results expose `error.kind` and `error.message`. Existing human-readable output must remain stable. Every configured repository gate must pass with the freshly built development binary, including the full backend test gate, formatting, Clippy, and contract checks.

## Idempotence and Recovery

Focused tests and configured gates are safe to rerun. Structured state files are append-only and must only be changed through `scripts/jig`. If a slice fails validation, amend the working tree before creating its commit; do not rewrite prior structured state. Each code slice is a separate commit so later work can be reverted independently if necessary.

## Artifacts and Notes

The original review findings identify `dev_sessions.rs` at the post-recovery replacement boundary and `dev_api.rs` at failure normalization. Final receipts and gate evidence will be recorded by the harness.

## Interfaces and Dependencies

Keep the external `jig_dev_proxy::run(...) -> anyhow::Result<serde_json::Value>` contract. Internal types may be added under `jig-dev-proxy`, but avoid new dependencies. Recovery records remain the existing serializable orphan-recovery notices. JSON failure payloads must use an error object with string `kind` and `message` fields.
