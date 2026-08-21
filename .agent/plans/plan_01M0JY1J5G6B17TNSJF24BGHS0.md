# Harden execution supervision boundaries

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while the work proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

Jig now supervises configured checks and Codex workers in owned process trees and reports their progress to CLI and MCP clients. The current implementation puts too many responsibilities in the observer callback: it delivers transport bytes synchronously, acts as a cancellation source, and is also used to model orchestration phases. Signal ownership is separately acquired by both top-level commands and lower-level probes. Those crossed responsibilities cause a reproducible `agent doctor` deadlock, allow output backpressure to suspend timeout enforcement, leave setup children outside cancellation, create duplicate phase events, and lose receipts for supervised failures.

After this work, every top-level operation has exactly one signal owner, lower layers receive cancellation rather than acquiring it, process supervision never waits on progress consumers, every started phase has one terminal event, worker output is bounded without changing normal successful output, and every configured command that starts records a durable success, exit failure, timeout, cancellation, await failure, or cleanup failure. A user can verify the result by running `scripts/jig agent doctor`, interrupting supervised integration fixtures, exercising MCP progress under output load, inspecting failure receipts, and passing the full repository test, formatting, Clippy, and contract gates.

## Progress

- [x] (2026-08-21 19:51Z) Reproduced the Unix `agent doctor` nested signal-session deadlock with a freshly built binary.
- [x] (2026-08-21 19:51Z) Opened structured work as `plan_01M0JY1J5G6B17TNSJF24BGHS0` and recorded this execution plan.
- [ ] Slice 1: centralize signal ownership and cancellation propagation; add Unix CLI regression tests for doctor, setup interruption, and non-process cancellation.
- [ ] Slice 2: separate supervision from progress transport using bounded/coalesced delivery; cap MCP progress volume and test backpressure.
- [ ] Slice 3: make output capture resource-bounded while preserving complete normal worker results.
- [ ] Slice 4: give orchestration phases balanced scope ownership and test exact event sequences.
- [ ] Slice 5: record configured-command supervision failures and their diagnostic reason in direct and work-check receipts.
- [ ] Update documentation and remove dead compatibility helpers exposed by the refactor where their removal reduces the bug surface.
- [ ] Build the development binary, run focused tests after every slice, commit every slice separately, then run the full configured gates and finish structured work.

## Surprises & Discoveries

- Observation: The highest-severity failure is not in process cleanup itself but in nested ownership of a process-wide signal mutex.
  Evidence: `target/debug/jig --json agent doctor` acquired a session in `cli/run.rs`, attempted a second acquisition through `doctor::standalone_codex_support_probe`, emitted no output, and required the diagnostic timeout's SIGKILL with exit status 137.
- Observation: `OwnedProcessObserver::output` runs before timeout and cancellation checks on the same supervision thread.
  Evidence: `process/output.rs::OutputDrain::poll` calls `observer.output`, and only after the drain returns does `process.rs::wait_for_owned_process` call `observer.poll`, `observer.cancelled`, and inspect the deadline.
- Observation: The existing focused timeout tests deliberately use `--no-receipt`, so they do not cover the durable-evidence path that currently returns before receipt creation.
  Evidence: `runtime::tests::command_tool_honors_repository_timeout` sets `no_receipt: true`.

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

Implementation is in progress. Completion requires all slices to be committed independently, full configured gates to pass, and the plan to record final evidence and remaining limitations.

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
