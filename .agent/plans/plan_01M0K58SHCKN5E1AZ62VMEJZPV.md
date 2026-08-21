# Close execution control boundary gaps

This work hardens two control-plane guarantees introduced by execution supervision: cancellation must retain its typed meaning through Git adapters, and fatal output overflow must remain authoritative through the final output drain after a process exits.

## Progress

- [x] Reviewed both findings and identified their ownership boundaries.
- [ ] Centralize owned-process cancellation classification and update Git adapters with regressions.
- [ ] Make final draining report overflow to the owned-process policy boundary with regressions.
- [ ] Run focused crate checks and all configured repository gates.

## Surprises & Discoveries

- The missed cancellation variants compiled because two adapters used catch-all error arms. Later sticky cancellation checks mask many CLI cases, but the repository explicitly tests non-sticky typed cancellation, so the lower-level classification contract still matters.
- Fatal overflow is currently observed only while waiting. The same drain can first cross its limit during post-exit completion, but that phase discards its `OutputPoll::overflow` value.

## Decision Log

- Treat both findings as abstraction-boundary defects rather than caller-only omissions.
- Put cancellation semantics on `OwnedProcessTreeError` so consumers do not duplicate knowledge of every cancellation variant.
- Keep output-policy enforcement in `jig-owned-process`; higher-level workers and configured commands should not need redundant truncation checks to make `ProcessOutputOverflowPolicy::Error` true.
- Preserve the public output and error shapes; these are correctness repairs, not compatibility migrations.

## Outcomes & Retrospective

Pending implementation and final verification.

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
