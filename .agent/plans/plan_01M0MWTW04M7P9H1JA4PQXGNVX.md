# Close execution finalization boundaries

This ExecPlan is a living document. Maintain it in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

The execution-supervision work now preserves typed cancellation for configured commands, Codex workers, and GitHub commands, but the PR manager still launches Git through raw blocking `Command::output`. Cancellation can therefore become an ordinary failed repair, consume retry budget, or obscure a push that already reached the remote. Separately, CLI progress is bounded in memory but synchronously flushed before the Unix signal session retires, so a stalled stderr sink can indefinitely delay restored-signal redelivery.

After this work, every PR-repair child process uses the same typed owned-process boundary, an interrupted push is reconciled against the remote before its result is classified, and presentation output cannot indefinitely block command or signal finalization. Each concern lands in its own commit with focused regression coverage, followed by the complete configured repository gates.

## Progress

- [x] (2026-08-22) Reviewed repository and crate guidance, built the development Jig binary, and opened structured work.
- [x] (2026-08-22) Implemented typed supervision and post-cancellation reconciliation for every PR-manager Git command; focused PR-manager tests and strict crate Clippy pass.
- [ ] Add bounded CLI progress finalization that cannot indefinitely delay signal retirement.
- [ ] Run focused tests and commit each implementation slice independently.
- [ ] Run format, Clippy, contract, and the complete configured test suite through `JIG_DEV_BIN=target/debug/jig`; inspect receipts and close the work.

## Surprises & Discoveries

- Observation: The Git omission is more than a missing cancellation check.
  Evidence: `run_pr_repair` treats all ordinary errors as failed attempts, while `git_output` erases termination reason through `Command::output`; `git push` also has a remote commit point whose outcome may be ambiguous after interruption.
- Observation: The progress buffer is bounded but its transport is not.
  Evidence: `CliExecutionObserver::finish_with` synchronously writes up to 64 KiB of child preview plus 16 KiB of structural output before `DoctorSignalSession::finish` restores and redelivers a pending termination signal.
- Observation: Push ambiguity applies to ordinary client failures as well as cancellation.
  Evidence: A server can accept the single ref update before the client sees EOF, timeout, or cancellation; the implementation now reconciles every non-successful push outcome and the focused fixture proves a completed remote update survives cancellation classification.

## Decision Log

- Decision: Keep cancellation typed through a PR-repair-specific step result rather than detecting cancellation from error strings.
  Rationale: Retry accounting and commit-point reconciliation need control-plane facts, not presentation text.
  Date/Author: 2026-08-22 / Codex
- Decision: Reconcile an interrupted push with a short, owned, cancellation-independent `git ls-remote` probe.
  Rationale: Once push may have mutated the remote, cancellation cannot safely be classified until the final head is compared with the remote ref. Failure to reconcile remains cancellation/unknown and must not consume attempt budget.
  Date/Author: 2026-08-22 / Codex
- Decision: Make final progress delivery deadline-bounded and best-effort while preserving immediate I/O diagnostics.
  Rationale: Presentation must never own execution or signal lifecycle. A bounded background write allows ordinary sinks to preserve output while a stalled sink cannot hold signal retirement indefinitely.
  Date/Author: 2026-08-22 / Codex

## Outcomes & Retrospective

Pending implementation and final verification.

## Context and Orientation

`crates/jig/src/runtime/loops/pr_manager.rs` owns PR worktree preparation, worker execution, commit/push, review-thread updates, retry accounting, and the test-only Git program override. `crates/jig/src/execution.rs` adapts commands to `jig-owned-process` and retains cancellation, timeout, output, and cleanup outcomes. The PR manager should use that boundary instead of growing another process supervisor.

`crates/jig/src/progress.rs` owns the bounded human progress preview. `crates/jig/src/cli/run.rs` and `crates/jig/src/cli/setup_run.rs` flush that preview before retiring `DoctorSignalSession`. The buffer limit prevents memory growth; a separate final-delivery deadline is required to prevent transport backpressure from becoming lifecycle backpressure.

## Plan of Work

First, introduce a typed PR-repair step error that distinguishes cancellation from ordinary failure. Route every PR-manager Git invocation through `run_authoritative_execution_command` with null stdin, bounded captured output, the repository command timeout, and the active observer. Map typed cancellation to `PrRepairOutcome::Cancelled`, so leases release and attempt budget remains unchanged. For an interrupted push, run one short supervised `ls-remote` reconciliation without the already-latched cancellation source. If the remote ref equals the intended final head, report the push as reconciled and allow the existing post-push cancellation path to stop thread updates. Otherwise return a cancellation detail that records whether the remote was unchanged or could not be confirmed. Add tests for cancellation before Git spawn, in-flight Git cancellation, attempt preservation, and push-reconciliation parsing. Commit this slice.

Second, separate progress rendering from progress delivery. Drain pending chunks into one owned bounded payload, deliver it on a short-lived writer thread, and wait only for a fixed deadline. Preserve an I/O error that arrives within the deadline, but treat a stalled sink as dropped presentation and continue signal retirement. Make rendering consuming so repeated finalization cannot duplicate progress. Add focused tests proving prompt return from a slow writer, preservation of immediate write errors, and single emission. Update public documentation to describe best-effort bounded final delivery. Commit this slice.

Finally, build the development binary and run the configured format, Clippy, contract, and complete two-stage test gate. Inspect work gates, evidence, receipts, and the final diff before closing the plan.

## Concrete Steps

All commands run from the repository root:

    cargo build -p jig-sh --bin jig
    cargo test -p jig-sh runtime::loops::pr_manager
    cargo test -p jig-sh progress::tests
    cargo fmt --all -- --check
    cargo clippy -p jig-sh --all-targets --locked -- -D warnings
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0MWTW04M7P9H1JA4PQXGNVX
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0MWTW04M7P9H1JA4PQXGNVX
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0MWTW04M7P9H1JA4PQXGNVX
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M0MWTW04M7P9H1JA4PQXGNVX

## Validation and Acceptance

Cancellation before or during any PR-manager Git command must release the branch lease, return a cancellation diagnostic, and leave the attempt store unchanged. An interrupted push that can be confirmed on the remote must not be reported as a failed push. Every spawned Git tree must obey timeout, output, and cleanup policy.

A slow or non-reading progress sink must not keep `finish_with` blocked beyond its delivery deadline. Immediate write failures must remain attached to the operation result, ordinary sinks must receive the same bounded transcript, and pending termination signals must reach signal-session retirement after finalization returns.

All focused tests and the repository's configured format, Clippy, contract, and complete test gates must pass. The final worktree must contain separate commits for the Git and progress slices plus only the normal structured-work evidence updates.

## Idempotence and Recovery

Builds and tests are safe to rerun. Test Git programs and progress writers are temporary fixtures. If push reconciliation cannot confirm remote state, return cancellation without consuming attempt budget; the next PR snapshot is authoritative. Do not rewrite append-only `.agent/state/*.jsonl` evidence after failed gates.

## Interfaces and Dependencies

Add no dependency. Reuse `ExecutionCommandError`, `ExecutionControl`, `NoopExecutionObserver`, `CommandTimeout`, and `run_authoritative_execution_command`. Keep the generic owned-process crate unchanged unless focused implementation proves a missing generic primitive.
