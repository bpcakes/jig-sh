# Implement recorder dashboard views and nested details

This ExecPlan implements Task D (`jig-sh-l2x.5`) from `docs/plans/unified-terminal-dashboard.md`. The outcome is complete browser-dashboard presentation parity inside the additive `jig-ui::terminal` shell: Work, Timeline, Health, plan detail, receipt/decision/event/failure detail, and loop-attention recovery detail all consume Task A/B2 typed values. The old HTTP UI remains available until Task G, and Task E retains ownership of hardened scheduling, cancellation, timers, and signal lifecycle.

Implementation baseline: `7b8b3b1be8a8026f27ef5d651a6b0bef1d4717dc` on branch `jig-sh-l2x`.

## Progress

- [x] Read repository/crate guidance, `.agent/PLANS.md`, Task D, and its referenced parity, information-architecture, contract, safety, performance, consistency, and test sections.
- [x] Claim `jig-sh-l2x.5` and open structured work.
- [x] Trace every recorder/plan contract field, bound, error, stable identity, and legacy browser presentation into a terminal parity inventory.
- [x] Build typed, prepared local view models for Work, Timeline, Health, and all detail forms without JSON round-trips or repository I/O.
- [x] Add stable per-view selection, timeline filters, nested detail state, section-local scrolling, and refresh reconciliation by raw identity.
- [x] Render all primary views and details in wide, standard, compact, and micro layouts with explicit errors, limits, omission markers, ages, and inert remediation argv.
- [x] Connect key handling and the transitional Task C worker to plan-detail requests while leaving full scheduling/preemption/timer hardening to Task E.
- [x] Add model, renderer, parity-registry, hostile-text, bounds, partial-error, detail-consistency, and interaction regressions using typed scenarios only.
- [x] Run focused validation and the second comprehensive working-tree review/fix round.
- [x] Close the Bead, record fresh exact-diff gates/evidence, finish structured work, and commit Task D.

## Surprises & Discoveries

- Task C intentionally retained `RecorderSnapshot` directly because its three local tabs were honest placeholders. Task D needs prepared display forms and stable indexes, so recorder acceptance must now convert once at the model boundary while retaining raw routing identities and the recorder epoch.
- The plan assigns the deterministic serialized scheduler to Task E, but Task D's details must be reachable before cutover. Task D will add only the minimal type-matched plan request/result plumbing needed to display detail; Task E will replace its transitional queue/order policy with the specified generation-aware scheduler.
- Timeline rows already carry producer-owned stable identity, while Work and Health require composite identities derived from their typed raw fields. Display sanitization must never participate in any selection or request key.
- Recorder and plan contracts already expose applied limits and omission counts. The renderer must display those values rather than independently truncating producer collections or inferring unknown totals.
- The first comprehensive review found that the existing single-line terminal sanitizer cannot be applied to a bounded text block: it intentionally replaces newlines. Task D now sanitizes each normalized line and expands tabs once at the prepared-model boundary.
- The ten-row history cap is not an existence authority. Closed details remain retained across recorder refreshes and refresh only on explicit request; open details retarget only when the accepted recorder epoch changes, with `NotFound` as the removal authority.
- The second review exposed a starvation loop at the transitional worker boundary: a mismatched or stale plan response could immediately enqueue itself ahead of recorder refreshes. Requests now capture their epoch when queued, domain work wins scheduling ties, and rejected detail responses stop until either the recorder advances or the user retries.
- Ratatui wrapping made the model's logical-line scroll bounds disagree with visual rows. Details now use logical vertical scrolling plus Unicode-width-aware horizontal scrolling; this keeps every byte of bounded content reachable without rebuilding the full gate document per frame.
- Repeated all-CPU local runs exposed unrelated contention-sensitive loop/lock tests. Each observed failure passed immediately in isolation; the final plan-bound run used two bounded per-test retries, reported one recovered flaky core test, and passed every required gate.

## Decision Log

- Keep all Task D code below `jig-ui::terminal`; do not reuse or call the HTTP renderer. Shared source contracts remain the only data boundary.
- Convert `RecorderSnapshot` and `PlanSnapshot` into terminal view models exactly once on acceptance. Store raw IDs/argv separately from sanitized labels and precompute timeline-filter indexes outside the render loop.
- Model Work as one stable selectable sequence spanning open and closed plans while rendering separate open-work, selected-preview, and recently-completed panes. Enter requests detail for either raw plan ID.
- Model Timeline filters as All, Receipts, Failures, Plans, Sessions, and Decisions. Failure is a predicate over nonzero receipt exit status; filtering never reslices or rescans source data.
- Model Health as one navigable sequence over failures, tool aggregates, workflows, leases, exhausted attempts, scheduled occurrences, and loop-state errors, with nonselectable section labels supplied by the renderer.
- Retain at most one base detail and one leaf detail. Plan is a base detail with Summary, Body, Gates, Decisions, and Receipts sections; receipt and decision leaves return to their exact parent section/selection/scroll. Unrelated item detail replaces the base.
- Render producer-owned remediation `argv` as individually quoted inert arguments plus its sanitized display string. No key executes, copies, or shells a command.
- Treat Task D's plan worker plumbing as transitional: one queued raw plan target at most, no overlapping source calls, and last detail retained on error. Task E owns phases, generations, preemption, retry, queue age, refresh cadence, and signal handling.
- Precompute potentially large gate-detail documents once and slice them to the visible viewport during rendering. This avoids rebuilding tens of thousands of Ratatui lines on each keypress at producer ceilings.
- Keep recorder Work gate projections deliberately shallow. Work retains only the six-row headline fields it renders; nested paths, findings, and remediation are converted only for an explicitly requested plan snapshot.

## Outcomes & Retrospective

Task D now provides typed Work, Timeline, Health, plan, leaf, and loop-attention views at every layout tier. Two independent comprehensive review rounds were completed and every actionable finding was addressed. The principal hardening changes were epoch-bound one-shot detail requests, domain-first scheduling, non-wrapping reachable detail navigation, lightweight Work gate summaries, visible section-local collection errors, stable duplicate identities, and snapshot-age/staleness labels for standalone details. Focused and full `jig-ui` tests, strict Clippy, formatting, file budgets, and all seven exact-diff repository gates passed; post-closure batch receipt `receipt_01M1R3Y1KGGM1X6J27EWEVADND` records the final acceptance evidence.

## Context and orientation

`crates/jig-ui/src/dashboard/recorder.rs` and `source.rs` define the typed recorder, plan, timeline, loop, gate, bound, error, epoch, and request/result contracts. `crates/jig-ui/src/dashboard/scenarios.rs` provides producer-shaped nonempty recorder and plan fixtures. `crates/jig-ui/src/terminal/model.rs`, `model/app.rs`, `render.rs`, `render/responsive.rs`, and `runtime.rs` are Task C's additive six-tab shell. The legacy browser renderer under `crates/jig-ui/src/html/` remains a presentation reference only.

Task D must not import `RepoContext`, open `.agent`, parse JSONL, evaluate gates, or launch subprocesses. All local information arrives through `DashboardSource::{recorder,plan}`. Task B2 owns the one-pass/reverse-scan semantics; Task D proves their observable basis/live timestamps, caps, omissions, and errors survive the consumer boundary.

## Plan of work

First create prepared recorder and detail view models with exact raw identities, sanitized display values, producer limit metadata, and deterministic row indexes. Extend `App` with Work, Timeline, Health, base-detail, and leaf-detail state plus reconciliation methods. Then implement the three local renderers and a common detail overlay across every size tier. Finally connect key intents to typed plan requests, port browser parity assertions to typed scenarios, and validate all behavior before the mandated review loop.

## Concrete steps

1. Inventory the recorder/plan DTOs and browser fields; add focused local-model modules with bounded text/row helpers, omission labels, time/duration preparation, and inert argv formatting.
2. Replace the recorder domain's raw stored value with a prepared local dashboard that retains epoch, timestamps, counts, repository/harness identity, open/history rows, gates, failures, tools, loops, timeline rows, limits, and errors.
3. Add stable Work, Timeline, and Health selections; recompute only filtered/indexed references on snapshot/filter changes; reconcile raw identities before clamping.
4. Add base/leaf detail state. Accept typed `PlanSnapshotResult`, preserve a prior detail on request error, close on trusted absence, expose stale epoch explicitly, and keep per-section scroll/selection.
5. Render Work lists and gate preview, Timeline kinds/filters/limit, Health sections, and every plan/item detail field with exact bounds, partial errors, text status labels, and safe remediation commands.
6. Extend compact/micro rendering so selected local items and every hidden detail remain reachable. Keep IDs before titles and the contextual footer limited to valid bindings.
7. Extend input handling for local list movement, `f`/`F`, detail section cycling, leaf open/close, and plan request intents. Extend the transitional sole worker for type-matched plan calls without implementing Task E's final scheduler.
8. Add typed scenario tests covering every parity row: baseline, gates/remediation/findings, errors, history, failures, tools, loops/attention/recovery, all timeline kinds/filter/navigation, body, decisions, receipts/output/paths/diff/duration, caps/omissions, stable raw identities, and hostile text.
9. Run format, Clippy, focused crate/compatibility tests, PTY coverage, file budget, configured Jig gates, and up to two comprehensive Claude+Codex working-tree reviews; address every actionable finding before commit.

## Validation and acceptance

Success means no browser-only recorder fact remains inaccessible from the TUI. Work, Timeline, and Health are fully navigable at supported sizes. Every producer limit/error/basis timestamp is honest. Plan-linked rows route by raw plan ID and epoch; standalone receipt, decision, session, failure, and loop-attention rows open bounded details. All repository-controlled strings are sanitized only in prepared display forms, and remediation commands remain inert.

Tests must consume typed Task A scenarios without aggregate JSON round-trips, cover raw-identity collisions/reordering/shrinkage, assert every filter and global/domain key, and render data/empty/partial/stale paths at 120x36, 80x24, 60x15, and micro sizes. Exact omission counts and unknown omissions must remain distinguishable. Source-call tests may use a fake `DashboardSource`, but Task D code performs no direct source I/O.

## Idempotence and recovery

The views are additive behind the uncut-over terminal entrypoint. Reverting Task D's local model/render/detail modules and transitional plan-result plumbing restores Task C's honest placeholders. No persistent state or public routing changes occur. The old HTTP UI remains the rollback surface until Tasks F and G.

## Interfaces and dependencies

`jig-ui::terminal` consumes `RecorderSnapshot`, `PlanSnapshot`, `PlanSnapshotResult`, `PlanBasis`, `RecorderEpochId`, `BoundedRows`, `BoundedText`, `SnapshotError`, timeline variants, loop DTOs, gate DTOs, and the existing `DashboardSource` trait. Ratatui and `jig-tui` own rendering and terminal-safe primitives. The crate directly uses the workspace-pinned `unicode-width` version already required by Ratatui so horizontal bounds match terminal cell widths. Task E may replace private transitional worker/app request types without changing these public source contracts.

Revision note (2026-09-05): replaced the work-start stub with a self-contained Task D execution plan after tracing the epic and typed producer boundaries.
