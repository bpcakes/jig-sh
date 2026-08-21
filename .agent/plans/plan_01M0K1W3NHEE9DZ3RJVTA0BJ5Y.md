# Harden execution outcome boundaries

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while the work proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

Jig supervises repository commands, Codex workers, and status providers while collecting progress, enforcing cancellation and timeouts, and recording durable receipts. The previous implementation introduced these behaviors but repeatedly converted control-plane facts into presentation values too early: cancellation became exit status 1, capture overflow became a boolean inspected only after process exit, phase completion depended on receiving an ordinary result, and buffered progress was flushed after signal redelivery could terminate the process. Those lossy boundaries create several apparently unrelated bugs.

After this work, termination reasons and output policies remain typed through orchestration, progress is flushed before a recorded signal can terminate the CLI, fatal output overflow terminates owned process trees promptly, diagnostic Codex transcripts may truncate without invalidating authoritative structured results, refinement cancellation stops rather than manufacturing failures for unstarted checks, and status providers run through a bounded scheduler that balances every started phase. The result is observable through focused interruption, overflow, receipt, and provider-lifecycle tests followed by the full configured test gate.

## Progress

- [x] (2026-08-21 20:58Z) Reviewed the merged Claude and Codex findings, traced the affected callers, and opened structured work.
- [x] (2026-08-21 21:04Z) Slice 1: made CLI progress finalization precede signal retirement, made deferred heartbeat wording historical, and covered interruption output in the production CLI.
- [x] (2026-08-21 21:09Z) Slice 2a: added explicit fatal/truncating capture policy, prompt fatal termination on authoritative overflow, and nonfatal schema-backed worker transcript truncation with receipt flags.
- [x] (2026-08-21 21:14Z) Slice 2b: preserved pre-start and in-flight cancellation through command orchestration so collect-all checks stop, started commands retain receipts, and unstarted commands do not manufacture child receipts.
- [x] (2026-08-21 21:20Z) Slice 3: bounded status-provider concurrency at four, balanced panic and cancellation lifecycle events, preserved configured order, made missing batch entries explicit, and removed superseded gate wrappers.
- [x] (2026-08-21 21:22Z) Slice 4: reconciled resource, progress, cancellation, receipt, and bounded-status documentation and repaired the earlier progress plan's stale living sections.
- [ ] Build the development binary, run configured format, Clippy, contract, and full test gates, inspect receipts, and finish structured work.

## Surprises & Discoveries

- Observation: The reviewed defects share one boundary failure rather than one implementation typo.
  Evidence: `OwnedProcessTreeError::Cancelled` is converted into a generic failed JSON result in `runtime/tool_execution.rs`; `OutputDrain` records fatal overflow only as `truncated`; status phases finish only when a normal channel result arrives; and CLI progress flush happens after `DoctorSignalSession::finish` may redeliver a terminating signal.
- Observation: The generic owned-process runner already has legitimate truncating consumers.
  Evidence: status and diagnostic probes intentionally accept bounded partial output, while configured commands require complete output. A single global overflow behavior would break one of those contracts, so the process API needs an explicit per-run overflow policy.
- Observation: The batched open-plan gate evaluator currently has no plan-specific fallible operation after the shared receipt scan except cancellation.
  Evidence: `evaluate_gate` constructs gate states from indexed receipts and only calls the shared cancellation guard. The review's all-or-nothing concern is therefore primarily a future bug-surface issue; this work will make missing map entries explicit without inventing a per-plan error type that current behavior cannot produce.
- Observation: The interruption regression must exercise human output mode because JSON mode deliberately disables buffered progress.
  Evidence: The first focused run used `--json` and correctly produced no progress transcript; switching the production CLI fixture to human mode made the child sentinel observable and the regression pass.
- Observation: Worker stdout has two different meanings depending on invocation shape.
  Evidence: Schema-backed review and refinement use the separately bounded `-o` file as authoritative output, so their provider stdout/stderr may truncate; schema-less worker invocations still use stdout as their result and therefore retain fatal capture overflow.
- Observation: Bounded scheduling also gives cancellation a concrete start boundary.
  Evidence: Workers check shared cancellation before and after claiming an index, so at most the four already-active providers can have started when cancellation arrives; queued providers never emit a phase or launch a process.

## Decision Log

- Decision: Treat cancellation, timeout, output overflow, and ordinary nonzero exit as distinct execution outcomes until orchestration chooses receipt and retry behavior.
  Rationale: Collect-all verification may continue after a normal failed check but must stop after cancellation; a numeric exit status cannot express that distinction.
  Date/Author: 2026-08-21 / Codex
- Decision: Add an explicit fatal-versus-truncating output-overflow policy to owned-process execution rather than changing all bounded capture to terminate.
  Rationale: configured commands need immediate failure, while status probes and non-authoritative Codex transcripts need bounded truncation and continued execution.
  Date/Author: 2026-08-21 / Codex
- Decision: Keep structured Codex output fatal at 4 MiB but make stdout/stderr transcript truncation nonfatal and visible in worker receipt evidence.
  Rationale: the schema-backed `-o` file is authoritative for review/refine; diagnostic transcript volume must not discard an otherwise-valid result.
  Date/Author: 2026-08-21 / Codex
- Decision: Use a small bounded status-provider worker pool and channel lifecycle messages back to the observer-owning thread.
  Rationale: the observer is intentionally single-threaded, while a bounded pool limits simultaneous processes and makes phase start/finish ownership explicit.
  Date/Author: 2026-08-21 / Codex
- Decision: Preserve `.agent/state/*.jsonl` append-only history.
  Rationale: stale plan prose may be corrected directly, but durable historical events must only be clarified through new append-only decisions or plan updates, never rewritten.
  Date/Author: 2026-08-21 / Codex

## Outcomes & Retrospective

Implementation is pending. The intended outcome is a smaller execution bug surface: one operation finalizer for progress and signal ordering, one typed process termination model, one explicit output policy, and one bounded provider scheduler with balanced phase events.

## Context and Orientation

`crates/jig-owned-process/src/process.rs` owns process-tree creation, waiting, termination, and cleanup. `crates/jig-owned-process/src/process/output.rs` drains stdout and stderr into bounded buffers. An output policy is the rule applied when one buffer reaches its byte limit: truncating policy keeps draining and discards later bytes, while fatal policy terminates the process tree and returns a typed overflow error.

`crates/jig/src/execution.rs` adapts owned-process events to Jig's execution observer. `crates/jig/src/progress.rs` buffers human CLI progress. `crates/jig/src/doctor/signal_session.rs` temporarily owns SIGINT, SIGHUP, and SIGTERM and redelivers them only after child cleanup. `crates/jig/src/cli/run.rs` and `crates/jig/src/cli/setup_run.rs` compose these pieces.

`crates/jig/src/runtime/tool_execution.rs` runs configured commands and records their receipts. `crates/jig/src/runtime/work/checks.rs` runs one or more configured checks; its collect-all mode is used after refinement to report ordinary check failures without aborting early. `crates/jig/src/runtime/worker_runner.rs` runs Codex and has both non-authoritative stdout/stderr transcripts and an authoritative schema-backed output file.

`crates/jig/src/status.rs` starts configured status-provider processes. `crates/jig/src/runtime/work/gates.rs`, `crates/jig/src/runtime/work.rs`, and `crates/jig/src/runtime.rs` expose batched gate snapshots used by status aggregation.

## Plan of Work

Milestone 1 creates a single operation-finalization order for CLI execution. Add a helper that composes the operation result with progress flushing before it retires the signal session. Use it from ordinary runtime dispatch, status, and setup. Reword or remove deferred heartbeat text so post-completion output cannot claim a process is still running. Extend the production-binary setup interruption fixture to emit a child-output sentinel and assert that the sentinel and phase transcript reach stderr before SIGINT is redelivered. Commit this slice independently.

Milestone 2 restores typed execution outcomes. Extend the owned-process drain result so it can report which stream exceeded its limit. Add an explicit overflow policy to the observer-backed runner while keeping existing callers on truncating behavior. Fatal policy must return a new typed output-limit error and terminate/reap the process tree immediately. Configure repository commands and marketplace registration to use fatal policy. Keep Codex transcript capture truncating, carry truncation flags into worker receipt evidence, and keep the separately bounded structured output file fatal. Represent cancellation-before-spawn separately from cancellation of a started tree so unstarted commands do not create child receipts. Thread the typed cancellation outcome through work-check collection so both fail-fast and collect-all modes stop after cancellation while retaining the active child's receipt. Add regressions for a process that overflows then sleeps, a valid structured result with oversized transcript, and refinement cancellation with multiple remaining checks. Commit this slice independently.

Milestone 3 replaces full status-provider fan-out with a bounded scheduler. Extract a testable scheduler that starts at most four workers. Worker threads send typed started, finished, and panicked lifecycle messages; only the main thread calls the execution observer. Catch a provider-task panic, emit a failed terminal event for its started phase, cancel remaining queued work, join every worker, and return an error. Preserve configured output order. Add tests for maximum concurrency, cancellation leaving queued providers unstarted, and balanced panic events. Remove the dead single-plan cancellable gate wrapper chain and unnecessary dead-code suppressions. Make a missing entry in the batched status map an explicit error rather than `snapshot: null, error: null`; retain the shared scan because current per-plan evaluation has no independent non-cancellation failure. Commit this slice independently.

Milestone 4 reconciles documentation and durable plan prose. Update configuration/public-contract text for fatal versus truncating output streams, bounded provider concurrency, cancellation receipts, and deferred progress flush ordering. Bring the earlier closed progress ExecPlan's required living sections up to date without editing append-only JSONL history. Update this plan's outcomes and record any design deviations discovered during implementation. Commit this slice independently.

Finally, build `target/debug/jig`, force the launcher through `JIG_DEV_BIN=target/debug/jig`, run the configured format, Clippy, contract, and full test gates, inspect gate status and receipts, and finish plan `plan_01M0K1W3NHEE9DZ3RJVTA0BJ5Y` only when every required gate is fresh and passing.

## Concrete Steps

All commands run from the repository root.

Build and select the development runtime before Jig workflow commands:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig

After each milestone, run its focused tests, `cargo fmt --all -- --check`, `git diff --check`, and inspect `git status --short`. Stage only that milestone plus the living plan update, then create one descriptive commit.

For final verification run:

    JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
    JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0K1W3NHEE9DZ3RJVTA0BJ5Y
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0K1W3NHEE9DZ3RJVTA0BJ5Y
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0K1W3NHEE9DZ3RJVTA0BJ5Y
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M0K1W3NHEE9DZ3RJVTA0BJ5Y

The full test gate is the repository's configured two-stage test command. A successful final work check records fresh passing receipts for every required gate.

## Validation and Acceptance

An interrupted production CLI command must reap its child tree, flush already-buffered progress to stderr, restore signal handlers, and then preserve the original signal behavior. The interruption integration test must fail before Milestone 1 and pass afterward.

A configured command that writes more than 4 MiB and then sleeps must be terminated promptly with an output-limit receipt rather than waiting for the repository timeout. A Codex review/refine worker with valid bounded structured output and more than 4 MiB of transcript must remain successful while its receipt reports transcript truncation. Cancelling refinement during verification must stop queued checks and must not append failed receipts for commands that never spawned.

Status aggregation must start no more than four providers at once, retain configured result order, stop starting queued providers after cancellation, and emit exactly one finished event for every started event even when a provider task panics. Batched gate status must never produce a null snapshot with a null error for a requested plan.

All existing tests must remain green. Final acceptance requires passing format, workspace Clippy with warnings denied, contract, and the complete configured test gate through the freshly built development binary.

## Idempotence and Recovery

Build, format, Clippy, contract, and test commands are safe to rerun. Focused tests use temporary repositories and process markers and must reap every child on failure. If a milestone fails, leave its edits uncommitted, update this plan with the observed failure and next experiment, and continue without rewriting prior commits or append-only state. Never delete or rewrite `.agent/state/*.jsonl` records to conceal a failed gate.

## Artifacts and Notes

Initial focused review verification passed `git diff --check`, 30 `jig-owned-process` tests, three execution-event tests, two worker supervision tests, and the configured-command timeout test. Those tests do not cover a long-lived overflow, signal-time progress flushing, provider panic balance, or multi-check refinement cancellation; the new regressions must cover those gaps.

## Interfaces and Dependencies

Keep using the standard library, `jig-owned-process`, and the existing `DoctorSignalSession`; add no runtime dependency. The owned-process API must expose a typed overflow policy and a typed stream-specific overflow error while preserving the existing truncating wrappers for current diagnostic consumers. Jig's configured-command adapter must select fatal overflow explicitly. Worker receipt evidence must add boolean transcript truncation fields without removing existing fields.

The status scheduler must use scoped standard-library threads, atomics or a synchronized work index, and an `mpsc` lifecycle channel. It must not call `ExecutionObserver` from worker threads. The concurrency limit must be a named constant and covered by a test.

Plan revision note (2026-08-21 20:58Z): Created the initial self-contained plan from the merged review, repository guidance, and the user's requirement for separately committed implementation slices and full-suite validation.

Plan revision note (2026-08-21 21:04Z): Completed Milestone 1 and recorded the JSON-versus-human progress test constraint discovered during focused validation.

Plan revision note (2026-08-21 21:09Z): Split Milestone 2 at its natural API boundary so output-policy hardening and cancellation orchestration remain independently reviewable commits.

Plan revision note (2026-08-21 21:14Z): Completed the cancellation half of Milestone 2 and verified the entire work-test module plus the owned-process crate.

Plan revision note (2026-08-21 21:20Z): Completed Milestone 3 with scheduler-level concurrency, cancellation, order, and panic-balance tests plus the full status test module.

Plan revision note (2026-08-21 21:22Z): Completed Milestone 4; documentation now distinguishes authoritative output from diagnostic transcripts and pre-spawn cancellation from interruption of a started command.
