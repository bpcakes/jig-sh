# Close execution control boundary gaps

This work hardens two control-plane guarantees introduced by execution supervision: cancellation must retain its typed meaning through Git adapters, and fatal output overflow must remain authoritative through the final output drain after a process exits.

## Progress

- [x] Reviewed both findings and identified their ownership boundaries.
- [x] Centralized owned-process cancellation classification and updated both Git adapters with pre-spawn regressions.
- [x] Made fatal overflow authoritative over final capture state and added a post-wait regression while preserving truncating capture.
- [x] Ran focused crate checks and all configured repository gates.

## Surprises & Discoveries

- The missed cancellation variants compiled because two adapters used catch-all error arms. Later sticky cancellation checks mask many CLI cases, but the repository explicitly tests non-sticky typed cancellation, so the lower-level classification contract still matters.
- Fatal overflow is currently observed only while waiting. The same drain can first cross its limit during post-exit completion, but that phase discards its `OutputPoll::overflow` value.
- A forced outer PTY changes terminal-diff behavior in a Vault TUI test, while the repository's normal piped test invocation passes. One earlier full run also encountered a one-shot generated-web lock timing failure whose exact rerun passed. Both failed attempts remain recorded in append-only receipts; the final normal invocation passed every required gate.

## Decision Log

- Treat both findings as abstraction-boundary defects rather than caller-only omissions.
- Put cancellation semantics on `OwnedProcessTreeError` so consumers do not duplicate knowledge of every cancellation variant.
- Keep output-policy enforcement in `jig-owned-process`; higher-level workers and configured commands should not need redundant truncation checks to make `ProcessOutputOverflowPolicy::Error` true.
- Preserve the public output and error shapes; these are correctness repairs, not compatibility migrations.

## Outcomes & Retrospective

Cancellation is now classified exhaustively by the owned-process error boundary, so both Git adapters inherit the same meaning and adding a future error variant requires an explicit owner-side decision. Pre-spawn cancellation regressions cover status probing and non-sticky worktree fingerprinting.

Fatal output overflow is now checked against the durable final capture state after cleanup and draining. Successful processes cannot evade `ProcessOutputOverflowPolicy::Error` merely by crossing the limit between the last active poll and EOF, while `Truncate` continues to return bounded output.

Focused tests and scoped Clippy passed for both slices. The development Jig binary then recorded passing format, Clippy, and contract checks, followed by a final non-PTY `scripts/jig work check` with fresh passing contract and full-test gates.

## Context and orientation

`crates/jig-owned-process/src/process.rs` owns process waiting, cancellation, timeout, output policy, cleanup, and final output capture. `crates/jig-owned-process/src/process/output.rs` implements bounded drains. `crates/jig/src/status/git.rs` and `crates/jig/src/git_receipts.rs` translate owned-process errors into status and fingerprint domain errors.

## Plan of work

First, expose one cancellation classifier on `OwnedProcessTreeError`, update both Git adapters to use it, and add regressions that force cancellation at the pre-spawn boundary. Commit that slice independently.

Second, retain overflow discovered during `OwnedProcessOutputDrains::finish`, combine it with the selected overflow policy before returning an owned-process result, and add a regression for a child that exits with excess buffered output. Commit that slice independently.

Finally, build the development Jig binary, run focused tests and Clippy, run `scripts/jig work check`, all configured gates, inspect receipts/status, update this living plan, and close structured work.

## Concrete steps

1. Change cancellation classification and its two adapters; run focused tests; commit.
2. Change final-drain overflow propagation; run `jig-owned-process` and worker tests; commit.
3. Run formatting, Clippy, contract, and the full configured test suite through `JIG_DEV_BIN=target/debug/jig`.

## Validation and acceptance

Acceptance requires typed pre-start cancellation in both Git adapters, `OutputLimitExceeded` when overflow is first observed during final draining, no regression to truncating capture, and passing configured repository gates.

## Idempotence and recovery

The source changes and tests are repeatable. Structured state files are append-only and must not be rewritten. If a focused or full gate fails, preserve its receipt, repair only the responsible slice, rebuild the development binary, and rerun the affected gate.

## Interfaces and dependencies

No new dependency is required. `OwnedProcessTreeError` remains the crate-level process-control error boundary. `ProcessOutputOverflowPolicy` remains the caller-selected behavior, but its enforcement will cover both active polling and final drain completion.
