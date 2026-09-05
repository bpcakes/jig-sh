# Harden serialized dashboard scheduling and terminal lifecycle

This ExecPlan implements Task E (`jig-sh-l2x.6`) from `docs/plans/unified-terminal-dashboard.md`. It replaces Task D's intentionally minimal request ordering with a deterministic generation-tagged scheduler, phase-aware cooperative preemption, completion-relative per-domain clocks, and cancellation-before-terminal-restoration. Real phase production and CLI signal forwarding remain Task F boundaries; Task E proves the runtime against controlled fake sources.

Implementation baseline: `a58a0de1` on branch `jig-sh-l2x`.

## Progress

- [x] Read repository and crate guidance, `.agent/PLANS.md`, Task E, and sections 12.1–12.7 and 17.6–17.7.
- [x] Claim `jig-sh-l2x.6` and open structured work.
- [x] Extract a scheduler state machine with monotonic generations, stable queue ages, coalesced per-kind intents, explicit/automatic priority, and independent completion-relative clocks.
- [x] Extend the sole worker with generation-tagged completion and `StatusPhase` events; implement legal provider-phase preemption, status requeue, join-before-restart, and stale event/result suppression.
- [x] Integrate the scheduler into the event loop without blocking navigation, while preserving loaded domain data and Task D detail state across failures and rebases.
- [x] Add deterministic channel-controlled fake-source tests for serialization, coalescing, ordering, preemption boundaries, anti-starvation, rebasing, stale suppression, timers, and quit cleanup.
- [x] Extend PTY lifecycle coverage for quit during work and restoration ordering, reuse shared panic/startup restoration coverage, and expose the external-cancellation seam without taking Task F's CLI signal ownership.
- [x] Run focused validation and exactly two comprehensive Claude+Codex working-tree review/fix rounds.
- [x] Close the Bead before final exact-diff gates, record evidence, finish structured work, and commit Task E.

## Surprises & Discoveries

- Task D's pending flags are distributed between `App`, `DomainState`, and the event loop. Task E should make scheduling authority a single private state machine while leaving presentation/error data in `App`.
- `DashboardSource::status` already exposes the required provider/local phase callback. `RefreshWorker` currently discards it, so the worker needs a dedicated phase channel alongside `CooperativeWorker`'s final-result channel.
- `TerminalSession` and `CooperativeWorker` already provide RAII restoration and join-on-drop. The runtime must make destruction order explicit: stop scheduling, clear pending intent, cancel and join the worker, then allow the terminal guard to drop.
- The repository's all-CPU local Nextest profile has unrelated contention-sensitive tests. Final Task E validation will close the Bead before the plan-bound check and use bounded retries if the same pre-existing flakes recur; failures remain visible.
- Round 1 exposed a split phase authority: channel delivery alone could both miss a later preemption trigger and race the provider-to-local boundary. The worker now owns an atomic monotonic phase latch used to claim provider cancellation, while the channel remains generation-tagged scheduler observation.
- Completion-relative clocks must begin at the first accepted/attempted domain completion, not scheduler construction. Keeping deadlines disarmed also enforces the Work-first lazy-provider contract and lets a due status request suppress a redundant automatic local traversal until publication succeeds or fails.

## Decision Log

- Represent request identity as `{ generation, kind }`; accept phases and completions only for the exact active identity.
- Give each coalesced pending kind the sequence assigned when it first became pending. Updating a detail target preserves that age. Dispatch always selects the oldest pending explicit intent, while automatic timers enqueue only absent kinds.
- Keep status, recorder, and detail results serialized through one `CooperativeWorker`; the phase channel is observational and cannot publish data.
- Preempt only an active status request whose latest accepted phase is `Providers`, and only for an explicit recorder-domain or detail intent. Cancellation joins synchronously, invalidates the generation, and requeues status at its original age before dispatching again.
- Never preempt `LocalEpoch`, recorder, or detail work. Automatic work never preempts anything.
- Store recorder and status deadlines separately and reset each only after an accepted completion for that domain. A successful status completion resets both because it publishes both observations.
- Treat external cancellation as an injectable observation at the runtime boundary. Task F owns installing/reusing the CLI signal session and passing that observation into the runtime.

## Outcomes & Retrospective

The unified dashboard now runs all status, recorder, and plan collection through one serialized, generation-tagged worker. A pure scheduler owns bounded coalesced intent, lazy completion-relative clocks, deterministic age ordering, status/local publication reconciliation, and status requeue. Provider preemption is linearized through a monotonic atomic phase latch, so explicit foreground work can claim cancellation only before local collection wins and the source observes the claim before starting local work.

Plan detail keeps its request basis and raw target across reconciliation: queued epoch-bound work rebases without losing age, `Fresh` closed-plan reads remain fresh, in-flight stale responses retry once, and a newer same-target request suppresses the older completion. Quit, Ctrl-C, and injected external cancellation clear pending work and join the worker before PTY restoration. Shared terminal tests now also prove panic unwind and startup-failure rollback of terminal attributes.

Both required comprehensive-review rounds completed with Claude and Codex against verified unchanged fingerprints. Their actionable findings drove the atomic phase authority, lazy timers, publication outcomes, queued-detail rebasing, Fresh-basis preservation, and same-target generation suppression. Focused `jig-ui` and `jig-tui` tests, strict Clippy, formatting, diff checks, and file budgets are clean. Final batch `receipt_01M1R7JWJQ89XY4ZZ0K867AG3X` passed every applicable configured gate: 3,151 core tests, 443 vault tests, 2 vault PTY tests, and 3,917 target tests passed. The target run reported one bounded-retry recovery alongside two slow dependency-scope tests; no test remained failed.

## Context and orientation

`crates/jig-ui/src/terminal/runtime.rs` owns terminal entry, event polling, key intents, and worker lifecycle. `runtime/worker.rs` adapts `DashboardSource` calls to one cooperative worker. `runtime/scheduler.rs` is Task D's minimal queue selector and is the intended replacement point. `terminal/model/app.rs` owns visible data, selections, and detail reconciliation. `dashboard/source.rs` defines `DashboardSource`, `StatusPhase`, request modes, bases, and typed errors. `jig-tui::TerminalSession` and `CooperativeWorker` provide the shared RAII primitives.

## Plan of work

First make scheduling a pure state machine whose behavior can be exhaustively tested without threads or clocks. Then make the worker carry generated identities and a dedicated phase receiver. Integrate both into a small runtime driver that turns keys, phases, completions, and timer observations into scheduler transitions. Finally add blocking fake sources and PTY fixtures to prove cancellation/join/restoration order and interaction responsiveness.

## Concrete steps

1. Define request kinds, priorities, pending entries, generations, active phase, deadlines, and transition outputs in `runtime/scheduler.rs` or focused children that remain within file budgets.
2. Make `RefreshWorker::spawn` receive a generated request, send status phases tagged with that generation, and return a completion tagged with the same identity.
3. Add queue APIs for initial mode, tab-driven lazy load, `r`, `R`, open/closed plan detail, automatic deadlines, and epoch-driven detail rebase. Preserve first-pending sequence on coalescing and replacement.
4. Implement phase draining before input, legal preemption with cancel-and-join, cancelled-generation invalidation, status requeue, and oldest-pending dispatch.
5. Apply only current completions, reset the correct clocks on accepted completion, preserve data on errors, and bound transparent detail retry to one newest-epoch attempt.
6. On quit or injected external cancellation, clear pending work, cancel and join the active worker, and return only after cleanup; keep terminal RAII outside the worker owner.
7. Add pure scheduler tables and channel-controlled fake-source tests for every Task E ordering, overlap, coalescing, phase, rebase, timer, and stale-result rule.
8. Extend PTY fixtures with deterministic side channels/files for quit-during-provider cleanup and terminal-sequence ordering. Reuse shared panic/startup restoration coverage where it already proves the same invariant; add missing runtime-specific cases only.
9. Run `cargo fmt`, strict Clippy, focused `jig-ui` tests, fake-source concurrency tests, PTY tests, file budget, and the configured Jig gates. Run exactly two comprehensive review rounds and fix every actionable finding.

## Validation and acceptance

At most one source call may be active. Pending local, status, and detail intent is bounded to one entry each; newer detail targets replace without losing queue age. Explicit recorder/detail work preempts and joins status only during `Providers`; `LocalEpoch` and all local/detail work are non-preemptible. Status eventually resumes, stale generations cannot publish phases or results, timers are completion-relative, and automatic work cannot duplicate or outrank explicit work.

Normal quit, Ctrl-C key handling, injected external cancellation, worker panic/error, and startup failure must leave no worker alive when terminal restoration begins. PTY output must prove alternate screen exit, cursor restoration, and bracketed-paste disablement where emitted. Task E does not install Unix signal handlers or wire real source phases; Task F owns that adapter work.

## Idempotence and recovery

The scheduler and tests are private to `jig-ui::terminal`. Reverting Task E restores Task D's minimal serial worker without changing source contracts, persistent state, public CLI routing, or the still-available HTTP rollback path. Fake sources use only channels and generic temporary fixtures.

## Interfaces and dependencies

No new production dependency is expected. The implementation uses `DashboardSource`, `StatusPhase`, `PlanBasis`, `RecorderMode`, existing typed refreshes/errors, standard channels/time, `CooperativeWorker`, and `TerminalSession`. Task F may add a public/internal runtime entrypoint accepting the CLI cancellation observation, but must not alter the scheduler invariants proven here.

Revision note (2026-09-05): replaced the work-start stub with the Task E execution plan after tracing the final scheduler and lifecycle contracts.
