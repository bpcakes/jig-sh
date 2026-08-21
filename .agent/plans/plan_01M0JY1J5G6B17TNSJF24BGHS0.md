# Harden execution supervision boundaries

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while the work proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

Jig now supervises configured checks and Codex workers in owned process trees and reports their progress to CLI and MCP clients. The current implementation puts too many responsibilities in the observer callback: it delivers transport bytes synchronously, acts as a cancellation source, and is also used to model orchestration phases. Signal ownership is separately acquired by both top-level commands and lower-level probes. Those crossed responsibilities cause a reproducible `agent doctor` deadlock, allow output backpressure to suspend timeout enforcement, leave setup children outside cancellation, create duplicate phase events, and lose receipts for supervised failures.

After this work, every top-level operation has exactly one signal owner, lower layers receive cancellation rather than acquiring it, process supervision never waits on progress consumers, every started phase has one terminal event, worker output is bounded without changing normal successful output, and every configured command that starts records a durable success, exit failure, timeout, cancellation, await failure, or cleanup failure. A user can verify the result by running `scripts/jig agent doctor`, interrupting supervised integration fixtures, exercising MCP progress under output load, inspecting failure receipts, and passing the full repository test, formatting, Clippy, and contract gates.

## Progress

- [x] (2026-08-21 19:51Z) Reproduced the Unix `agent doctor` nested signal-session deadlock with a freshly built binary.
- [x] (2026-08-21 19:51Z) Opened structured work as `plan_01M0JY1J5G6B17TNSJF24BGHS0` and recorded this execution plan.
- [x] (2026-08-21 20:25Z) Slice 1a: centralized signal ownership for runtime agent doctor and setup, passed caller cancellation into nested doctor probes, and added a production-binary regression for the former nested-lock deadlock.
- [x] (2026-08-21 22:17Z) Slice 1b: propagated operation cancellation through state summaries, work gate/evidence/finish scans, and loop status; added production setup interruption and in-process collection regressions; removed superseded blocking-only lifecycle helpers.
- [x] (2026-08-21 20:49Z) Slice 2: separated supervision from transport writes with a bounded 64 KiB CLI event buffer and coalesced 4 KiB-per-stream MCP previews; transport flushes now occur only after supervised work returns.
- [x] (2026-08-21 21:07Z) Slice 3: replaced unbounded configured-command, marketplace, worker-stream, and structured worker-file capture with a shared 4 MiB-per-output limit and explicit incomplete/truncated-output errors.
- [x] (2026-08-21 21:34Z) Slice 4: made phase label/position explicit inputs to tool and worker execution, removed orphan aggregate starts, suppressed nested worker phases inside loop ticks, and added exact sequence tests for checks, review gates, and loop ticks.
- [x] (2026-08-21 21:51Z) Slice 5: converted configured-command supervision errors into failed receipt outcomes, retained fail-fast behavior with receipt IDs, and linked child failure receipts plus diagnostics from work-check batch receipts.
- [x] (2026-08-21 22:31Z) Updated configuration and public-contract documentation for deferred bounded progress, 4 MiB execution capture, and supervised-failure receipt evidence; removed superseded blocking lifecycle helpers.
- [x] (2026-08-21) Built the development binary, ran focused tests after every slice, committed every slice separately, passed formatting and workspace Clippy with warnings denied, passed the configured contract and complete two-stage test gates, and finished structured work successfully.

## Surprises & Discoveries

- Observation: The highest-severity failure is not in process cleanup itself but in nested ownership of a process-wide signal mutex.
  Evidence: `target/debug/jig --json agent doctor` acquired a session in `cli/run.rs`, attempted a second acquisition through `doctor::standalone_codex_support_probe`, emitted no output, and required the diagnostic timeout's SIGKILL with exit status 137.
- Observation: `OwnedProcessObserver::output` runs before timeout and cancellation checks on the same supervision thread.
  Evidence: `process/output.rs::OutputDrain::poll` calls `observer.output`, and only after the drain returns does `process.rs::wait_for_owned_process` call `observer.poll`, `observer.cancelled`, and inspect the deadline.
- Observation: The existing focused timeout tests deliberately use `--no-receipt`, so they do not cover the durable-evidence path that currently returns before receipt creation.
  Evidence: `runtime::tests::command_tool_honors_repository_timeout` sets `no_receipt: true`.
- Observation: Unit tests cannot validate the nested signal-session failure because production signal ownership is intentionally excluded under `cfg(test)`.
  Evidence: The new `cli_agent_doctor_reuses_outer_signal_session` integration test invokes `CARGO_BIN_EXE_jig`, bounds the wait to five seconds, and exercises the production Unix configuration that previously hung.
- Observation: The simplest reliable nonblocking transport boundary does not require a background runtime or writer thread.
  Evidence: CLI and MCP observers now retain bounded event previews during execution and flush only after dispatch returns; noisy MCP output is coalesced to one notification per stream, so the supervision thread performs no transport I/O.
- Observation: Limiting only child pipes would leave the Codex structured `-o` file as a second unbounded allocation path.
  Evidence: `run_codex_exec_inner` previously called `fs::read` after checking only that the file was nonempty; it now rejects metadata lengths above the shared execution limit before allocating.
- Observation: A position alone is insufficient phase context for workers because review gates and refinement iterations need caller-owned labels as well as aggregate numbering.
  Evidence: `WorkerPhase` carries label and position together as an optional typed scope; loop-owned workers pass `None`, while review/refine workers receive the caller's exact phase identity.
- Observation: Work-check already had a collect-result mode, so supervision failures did not need a parallel receipt subsystem.
  Evidence: Modeling timeout/cancellation/await/cleanup/capture errors as a synthetic failed tool result lets the existing child receipt, `receipt_ids`, batch verdict, and gate indexing paths remain authoritative.
- Observation: Most long read-only scans already had cancellation-aware implementations for status aggregation, but runtime command dispatch selected their blocking wrappers.
  Evidence: Runtime dispatch now passes `ExecutionControl::cancelled` into state summary, work gate/evidence/finish, and loop status paths; a test that cancels after the dispatch boundary fails inside state collection, not only before or after it.
- Observation: Explicit phase context exposed an over-wide refinement helper during final Clippy validation.
  Evidence: Clippy rejected eight positional arguments; `RefinementIteration` now groups the iteration's plan, gates, findings, refinement profile, and phase position without a lint suppression.

## Decision Log

- Decision: Treat these findings as an abstraction-boundary failure, not independent formatting mistakes.
  Rationale: Signal ownership, observer delivery, cancellation, phase lifecycle, capture policy, and receipt outcome are orthogonal concerns but are currently joined through one callback and several ad hoc wrappers. Fixing individual symptoms would retain the same failure modes at the next call site.
  Date/Author: 2026-08-21 / Codex
- Decision: A top-level operation owns at most one `DoctorSignalSession`; lower layers accept a cancellation capability and never create a nested session.
  Rationale: The session wraps a non-reentrant process-wide mutex and scoped signal-handler restoration. Ownership must therefore be explicit at the transport boundary.
  Date/Author: 2026-08-21 / Codex
- Decision: Process supervision may publish bounded events but must never perform blocking CLI or MCP transport writes itself.
  Rationale: Timeout and cleanup guarantees are meaningful only if their polling thread cannot block behind a progress consumer.
  Date/Author: 2026-08-21 / Codex
- Decision: Orchestration layers own aggregate phase positions; leaf process execution owns output and heartbeat observation, not a second phase with a conflicting position.
  Rationale: One owner per lifecycle produces balanced events and makes exact sequence tests possible.
  Date/Author: 2026-08-21 / Codex
- Decision: Preserve complete output for ordinary successful workers through bounded memory or file-backed capture, and fail explicitly at a documented cap rather than allowing OOM.
  Rationale: A timeout cannot protect Jig if output accumulation exhausts memory before the deadline.
  Date/Author: 2026-08-21 / Codex

## Outcomes & Retrospective

The findings came from a structural boundary problem rather than one isolated omission: signal ownership, cancellation, progress transport, phase lifecycle, output capture, and evidence recording had become coupled through callbacks and ad hoc wrappers. The completed slices give each concern one explicit owner and preserve those boundaries with production-path, sequence, overflow, cancellation, and receipt regressions.

The behavior changes were kept in separately reviewable commits. The final development binary passed formatting and workspace Clippy with warnings denied. Structured work check then passed the contract gate (`receipt_01M0JZWTB15Z1D4GJER761SBFZ`) and the complete two-stage test gate (`receipt_01M0K0RFSKBH3EM1Q89RD2Y570`), recorded together by batch receipt `receipt_01M0K0RFV3K0G1N5YQTFMCNQ34`. The first test phase reported 2,180 tests across 24 binaries, followed by the separate vault phase. Both required gates were fresh before work was finished; the close operation recorded plan receipt `receipt_01M0K0VZMS5SXMF3RDRGJRM0BK` and session receipt `receipt_01M0K0VZNBBFMN86Q84DKT3WHA`.

No known correctness limitation remains within the reviewed scope. Deferred progress intentionally retains bounded previews rather than unlimited live output, and execution capture intentionally fails above the documented 4 MiB limit; those are explicit resource contracts rather than silent truncation.

## Context and Orientation

`crates/jig-owned-process/src/process.rs` owns child process-group or Windows Job Object creation, timeout polling, cancellation checks, output draining, and cleanup. Its `OwnedProcessObserver` callback is invoked on the supervision thread. `crates/jig/src/execution.rs` adapts repository execution events and cancellation to that observer. `crates/jig/src/progress.rs` renders CLI progress, while `crates/jig/src/mcp.rs` renders MCP progress notifications. Neither transport may block process supervision after this change.

`crates/jig/src/cli/run.rs` is the ordinary CLI transport boundary. It currently starts `DoctorSignalSession` for every `RuntimeCommand`. `crates/jig/src/doctor/signal_session.rs` implements that session with a process-wide mutex and scoped Unix signal handlers. `crates/jig/src/runtime/agent.rs` reaches `crates/jig/src/doctor_parts/part_04.rs::standalone_codex_support_probe`, which starts another session and deadlocks. `crates/jig/src/cli/setup_run.rs` takes the opposite path and starts no outer session, so newly separated process groups can outlive an interrupted setup. The long-term boundary must let CLI setup, ordinary runtime commands, and standalone doctor callers share the same lower-level cancellation-aware probe without nested ownership.

`crates/jig/src/runtime/tool_execution.rs` runs configured command-backed tools. `crates/jig/src/runtime/worker_runner.rs` runs Codex workers. `crates/jig/src/runtime/work/checks.rs`, `work/review.rs`, and `runtime/loops.rs` add orchestration progress. Receipt persistence lives in `crates/jig/src/state/receipts.rs`. The execution result passed across these layers must distinguish a normal exit status from supervision failures so receipts can record what actually happened.

## Plan of Work

First, introduce an operation-scoped cancellation boundary. Refactor the agent doctor probe so callers that already own signal supervision call the cancellation-aware probe directly. Give setup the same operation boundary while allowing its doctor phases to reuse that cancellation instead of starting nested sessions. Thread cancellation through long non-process state and gate scans that are already equipped with cancellable helpers. Add binary-level Unix tests because unit builds select `cfg(test)` branches that cannot reproduce the production deadlock.

Second, change event delivery so `OwnedProcessObserver` only performs bounded in-memory publication. CLI and MCP transport writers must drain that publication independently or receive coalesced events at controlled intervals. Preserve raw CLI output semantics where possible, but bound queued data and make overflow or sink loss explicit. MCP output previews must be rate-limited and coalesced so output size does not map linearly to flushed protocol messages. Tests must fill or stop draining a transport and prove the child still times out and is reaped.

Third, replace `usize::MAX` worker capture with a resource policy. Prefer the existing temporary-file design for Codex stdout and stderr if it can coexist cleanly with live bounded previews; otherwise use a documented finite memory cap large enough for normal structured output. A worker exceeding the cap must be terminated safely, receive an error receipt, and never OOM Jig. Configured command and marketplace behavior should use explicit limits appropriate to their response contracts.

Fourth, make phase scope ownership explicit. Aggregate loops create an `ExecutionPhase` using the aggregate `PhasePosition` and finish it on all paths. Leaf command execution accepts the caller's phase position or emits only output and heartbeat events. Move refine iteration start below the no-findings and no-refinement early exits. Add a recording observer that asserts exact start/output/heartbeat/finish order and equal start/finish counts for native tools, command tools, review gates, refine iterations, and loop ticks.

Fifth, define a recordable supervised execution outcome. Once a configured command has started, timeout, cancellation, await failure, output-limit failure, and cleanup failure must flow through receipt creation before the CLI or MCP error is returned. Work-check batch receipts must include the child receipt identifier when one exists and preserve the failure message in stderr or structured evidence. Keep pre-spawn validation failures distinct because no execution occurred.

Finally, update `docs/configuration.md` and `docs/public-contract.md` to describe the bounded progress/capture and cancellation behavior. Remove superseded dead cancellable gate helper chains if the new operation boundary makes them unnecessary. Review the complete diff for fixture hygiene and append-only state integrity, build `target/debug/jig`, force `JIG_DEV_BIN=target/debug/jig`, run focused tests, then run `scripts/jig check fmt`, `scripts/jig check clippy`, `scripts/jig check contract`, and `scripts/jig check test`.

## Concrete Steps

All commands run from `/home/aa/Documents/jig-sh`.

Build and select the development runtime before Jig workflow commands:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig

After each slice, run its focused unit and integration tests, inspect `git diff --check` and `git status --short`, then create one descriptive commit. Do not mix append-only work evidence into an unrelated behavioral slice; include plan updates that explain that slice in the same commit or in a final evidence commit.

At final verification run:

    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0JY1J5G6B17TNSJF24BGHS0
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0JY1J5G6B17TNSJF24BGHS0
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0JY1J5G6B17TNSJF24BGHS0
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M0JY1J5G6B17TNSJF24BGHS0

Expect every configured gate to report passed and fresh. Then finish the work plan only after the full test gate succeeds.

## Validation and Acceptance

The freshly built `target/debug/jig --json agent doctor` must return a JSON object within the probe timeout instead of hanging. A production-path integration test must fail against the pre-fix code and pass after the signal ownership change.

An interrupted setup fixture must terminate its owned child tree and leave no delayed marker. A long state or receipt scan must observe cancellation without waiting for the entire scan. Normal commands must still restore and redeliver SIGINT, SIGHUP, and SIGTERM according to the existing signal-session contract.

A command producing more output than the progress transport can immediately consume must still time out or finish on schedule, and its child tree must be absent afterward. MCP notification count must remain bounded relative to elapsed time rather than output bytes. JSON mode must emit no progress frames.

Every phase-sequence test must show one start and one finish per phase, with aggregate positions such as `1/3`, `2/3`, and `3/3` preserved and no nested `1/1` duplicate for the same operation.

A timed-out configured check with receipt recording enabled must append a child receipt containing the supervision failure and a work-check batch receipt that points to it. Gate evaluation must treat that receipt as failed rather than missing.

The full acceptance criterion is a clean worktree after the requested commits and successful formatting, Clippy, contract, and complete test gates through the development binary.

## Idempotence and Recovery

Build and test commands are safe to rerun. State files under `.agent/state` are append-only and must never be rewritten to erase failed attempts. If a slice fails, keep its changes uncommitted, update this plan with the failure and next experiment, and rerun focused tests after correction. Do not use destructive Git commands. Temporary diagnostic children must be identified exactly and terminated before continuing. If a commit accidentally includes unrelated user changes, stop rather than rewriting or discarding them.

## Artifacts and Notes

Initial deadlock reproduction after `cargo build -p jig-sh --bin jig`:

    timeout --kill-after=1s 3s target/debug/jig --json agent doctor
    exit_status=137

Initial focused tests for the new timeout, worker stdin/output, and MCP progress paths passed, but they do not cover the reported cross-layer failures:

    runtime::tests::command_tool_honors_repository_timeout ... ok
    runtime::worker_runner::tests::worker_supervision_delivers_stdin_and_observes_output ... ok
    mcp::tests::progress_observer_emits_standard_notification_with_call_token ... ok

## Interfaces and Dependencies

Keep using `DoctorSignalSession` as the sole Unix signal-handler owner; do not introduce another signal library. Expose or reuse a lightweight cancellation capability whose `cancelled()` method can be called by process supervision and long-running in-process scans.

Keep `OwnedProcessObserver` transport-neutral. If asynchronous delivery requires a queue, use the standard library's bounded synchronization primitives and atomics; do not add a new runtime dependency. The supervisor-facing callback must have bounded execution time and memory use.

Represent process capture limits with `ProcessOutputLimits` or a file-backed equivalent. Do not overload progress queue limits with receipt capture limits because they protect different resources.

Use `ExecutionPhase` for balanced start/finish publication. It may be extended to accept a caller-provided `PhasePosition`, but no layer may emit a duplicate phase for the same logical work item.

Preserve receipt JSON compatibility. New evidence fields may be additive, but existing fields and append-only record ordering must remain readable by older records and current gate evaluators.

Plan revision note (2026-08-21 19:51Z): Created the initial self-contained plan from the merged Claude and Codex review, the reproduced deadlock, repository guidance, and the user's requirement for separately committed implementation slices and full-suite validation.

Plan revision note (2026-08-21 20:25Z): Recorded the first signal-ownership cut separately from broader in-process cancellation so its deadlock regression and commit remain narrowly reviewable.

Plan revision note (2026-08-21 20:49Z): Recorded bounded deferred transport delivery as the second slice. This deliberately trades live byte-for-byte progress for a fixed resource ceiling and supervision guarantees; complete command results remain part of the normal tool response and receipts.

Plan revision note (2026-08-21 21:07Z): Recorded the shared 4 MiB capture policy and its worker-stream and structured-file regressions. Capture overflow is an explicit execution failure rather than a partial successful result.

Plan revision note (2026-08-21 21:34Z): Recorded explicit phase-scope ownership. Exact observer tests now reject duplicate `1/1` starts, missing finishes, and lost aggregate positions.

Plan revision note (2026-08-21 21:51Z): Recorded supervised failures as ordinary evidence-producing failed outcomes. A timeout regression now asserts both child and batch receipts contain the reason and that the batch references the child ID.

Plan revision note (2026-08-21 22:17Z): Completed the remaining cancellation slice. A production binary setup test now interrupts a live bootstrap and proves its delayed descendant marker is never written.

Plan revision note (2026-08-21 22:31Z): Documented the new resource, progress-delivery, cancellation, and receipt contracts before final repository-wide verification.

Plan revision note (2026-08-21 22:38Z): Recorded and corrected the only initial final-gate failure by replacing positional refinement inputs with a typed request; workspace Clippy then passed with warnings denied.

Plan revision note (2026-08-21): Recorded successful formatting, Clippy, contract, and complete two-stage test validation, linked the final receipts, and closed the structured work session successfully.
