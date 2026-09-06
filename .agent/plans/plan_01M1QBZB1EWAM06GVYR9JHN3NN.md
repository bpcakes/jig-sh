# Build the shared local dashboard epoch

This ExecPlan implements Task B2 (`jig-sh-l2x.3`) from `docs/plans/unified-terminal-dashboard.md`. The outcome is one bounded, cooperatively cancellable `LocalObservationEpoch` per local refresh, projected directly into Task A's typed recorder and status contracts without duplicate state traversals or in-process JSON round trips. The adapter remains unused by public routing until Task F, and this task changes no persisted state schema.

Implementation baseline: `23a4de31` on branch `jig-sh-l2x`.

## Progress

- [x] Read repository and crate guidance, `.agent/PLANS.md`, Task B2, and all referenced architecture, safety, projection, and test sections.
- [x] Claim `jig-sh-l2x.3`, build the development binary, and open structured work.
- [x] Trace the legacy recorder, status, provider, loop, gate, state, and plan-detail producer paths and resolve ownership questions from source.
- [x] Expose typed loop and gate producer read models while preserving existing command JSON.
- [x] Add the single-pass local epoch reducer, retained epoch store, and typed recorder/status-local projections.
- [x] Split status provider collection into a typed provider partition and compose the shared typed status aggregate.
- [x] Implement retained, stale, not-found, open-plan, closed-plan, and transient `Fresh` detail semantics.
- [x] Add producer-consumer, differential, traversal, limit, phase-ordering, freshness, malformed-provider, raw-round-trip, detail-basis, and cancellation tests.
- [x] Run focused validation and two comprehensive working-tree review/fix rounds.
- [x] Record passing exact-diff evidence, close the bead, flush Beads, finish structured work, and commit Task B2.

## Surprises & Discoveries

- The legacy web recorder performs one state aggregation scan, then evaluates every open plan through a wrapper that independently scans plan and receipt state. Plan detail separately rescans plans, decisions, gates, and receipts.
- The legacy status aggregate already runs providers before local observation, but its local state summary returns `Value`; gate and loop owners also serialize private structured data to `Value`, and downstream code deserializes those values back into presentation structs.
- Task B1 deliberately left bounded cancellable forward and reverse readers available for this adapter. Existing unbounded readers must remain for compatibility until the public cutover.
- The Task A contracts intentionally separate `StatusLocalSnapshot` and `StatusProviderSnapshot`. This permits a local recorder refresh to replace local status facts without aging or recomputing provider input-freshness observations.
- Open-plan gate evaluation already has a batched receipt-index path and baseline preparation cache, but it still discovers baselines by rescanning plans and returns JSON. B2 must add an input form that accepts epoch-owned plan facts and typed output.
- Canonical session handling is not a plain JSON decode: duplicate IDs produced by union merges must collapse, while divergent duplicate envelopes are a scoped stream error. The epoch reducer now preserves that compatibility in its single traversal.
- Review findings were present only inside receipt evidence, not the status-v1 gate DTO. The typed gate owner now produces the recorder view directly so findings remain available without widening or changing status-v1 JSON.
- The initial implementation concentrated new code in already maxed-out legacy modules. Focused dashboard child modules keep existing line debt unchanged and the file-budget gate passing without waivers.
- The first review exposed compatibility drift hidden by a minimal differential fixture: status recent rows are append-order windows of ten, evidence targets remain strings, absent unsupported-gate reasons remain omitted, and status errors retain legacy scopes, codes, ordering, and state-unavailable behavior.
- The second review found that field-level gate assertions were insufficient because snapshot serialization validates declared nested ceilings. Source tests now serialize and deserialize live recorder and plan snapshots, and externally truncated historical findings use an unknown omitted count rather than an invented exact count.
- A hard cap on session event IDs would turn ordinary append-only growth into a permanent dashboard outage. Session reduction now keeps a bounded canonicalization set for merge-duplicate detection and continues streaming valid unique overflow events into exact counts and bounded timeline selection.

## Decision Log

- Keep `LocalObservationEpoch` and its short-held retained cache in `crates/jig/src/ui/`; `jig-ui` remains the runtime-independent contract and future terminal application crate.
- Refresh `RepoContext` exactly once per source request. Status requests announce `Providers`, run provider work, announce `LocalEpoch`, and then collect local state from that same refreshed request context.
- Make producer ownership explicit: `status.rs` returns Task A's typed provider partition; the loop owner returns a typed loop report; the gate owner returns typed reports or a typed conversion surface. Existing CLI command wrappers serialize those producer values but the dashboard adapter never reparses them.
- Build sessions, plans, decisions, and receipts with one forward reducer per stream. Retain only distinct plan facts and the bounded newest candidates required by recorder/status/detail projections, plus the prepared gate receipt index required for batched open-plan evaluation.
- Treat a stream failure as a scoped partial observation when the remaining epoch is publishable. Cancellation and failures that prevent construction of a trustworthy basis return `SourceError` and never replace the retained epoch.
- Allocate nonzero epoch IDs with checked monotonic increment. Publishable complete and partial local collections replace the retained epoch atomically only after all blocking work finishes; the mutex is never held during I/O, Git, loop, or gate work.
- `RecorderMode::ReuseCurrent` reprojects the retained immutable epoch at the requested timeline limit without I/O or a new ID. `Refresh` constructs and installs a new epoch.
- `PlanBasis::RecorderEpoch` first validates the exact retained ID without I/O. Unknown or superseded IDs return `StaleRecorderEpoch`; a successfully decoded plan index may return `NotFound`; an unavailable plan index returns its scoped collection error.
- Open-plan detail reuses epoch plan metadata and gate results, then performs a single decision traversal, bounded reverse receipt lookup, and safe body read. Closed and `Fresh` detail use a transient one-pass forward receipt reducer for both gate inputs and append-order matching receipts. `Fresh` allocates a monotonic transient basis but collects only the requested plan, and never replaces the retained interactive epoch.
- Keep status schema-v1 cardinality and encodings unchanged for supported-size state. Accepted provider reports retain both the decoded `Report` and exact raw `Value`; serialization emits only the raw value. Oversized local records are the documented differential exception and become scoped partial errors.
- Stable timeline identity is derived from stream kind plus Task B1's raw record start offset and a digest of the raw record bytes, so appends preserve identity while replacement bytes at the same offset do not.
- Repository identity and provider freshness share one root Git observation per status refresh. Bounded newest-row selection uses an ordered fixed-size reducer rather than rescanning the full retained window for every input record.
- Use injected clocks/provider-duration seams and traversal counters only in tests. Differential tests compare semantic JSON rather than weakening assertions by stripping dynamic fields.

## Outcomes & Retrospective

Task B2 delivered a single-pass, typed local observation epoch and retained source adapter without changing public command routing. Recorder, local-status, provider-status, loop, gate, and plan-detail projections now share producer-owned typed data instead of performing duplicate state traversals or JSON round trips. The retained epoch boundary keeps refresh publication atomic and monotonic, `ReuseCurrent` free of I/O, and `Fresh` scoped to the requested plan.

Two comprehensive working-tree review rounds found and closed compatibility and resource-safety gaps that focused happy-path tests had missed: append-order status windows, exact status-v1 error behavior, unsupported-field serialization, bounded gate findings, live snapshot serialization, high-cardinality session behavior, duplicate plan histories, cancellation inside gate indexing, and source-error taxonomy. Regression tests now exercise those boundaries directly, including semantic equality against the legacy status aggregate.

Final exact-diff evidence passed all configured gates. The source partitions ran 3,061 core tests, 443 vault tests plus 2 vault CLI tests, and 209 process tests; the aggregate repository target ran 3,827 tests with 2 skipped. Contract, formatting, Clippy, and file-budget checks also passed with no waivers or unresolved gates.

## Context and orientation

`crates/jig-ui/src/dashboard/{source,recorder,status}.rs` owns the Task A public DTOs, limits, source requests, epochs, and typed errors. `crates/jig/src/ui.rs` is the future dashboard adapter; `ui/snapshot.rs` is the legacy web recorder and remains routed until Task F. `crates/jig/src/status.rs` owns provider execution and the status-v1 aggregate. `runtime/loops/engine/status.rs` and `runtime/work/gates.rs` own loop and gate producer truth. `state/jsonl.rs`, `state/jsonl/reverse.rs`, and `state/plan_files.rs` provide Task B1's bounded cancellable primitives.

The typed cancellation sentinel is `crate::cancellation::StatusCollectionCancelled`. Only the source adapter maps it to `SourceError::Cancelled`; internal producers keep returning their existing `anyhow::Result` boundary.

## Plan of work

First replace the loop and gate JSON-only producer seams with typed internal reports while preserving their command serializers. Then add an epoch reducer that streams each state file once, computes bounded projections and gate inputs, and obtains repository/loop/gate observations without retaining full streams. Add the retained source adapter and typed status provider partition. Finally implement plan detail against epoch-owned facts and add instrumentation and differential tests before the required reviews.

## Concrete steps

1. Introduce typed loop status output in `runtime/loops/engine/status.rs`; keep `loop status` serialization shape-compatible and derive dashboard remediation from validated argv parts.
2. Expose a crate-private typed gate report/conversion surface and a batched entrypoint accepting epoch-collected plan facts and receipt indexes, with no plan or receipt rescan.
3. Add `ui/source.rs` and focused child modules for epoch collection/projection/detail, using bounded reducers and stable raw-record identities.
4. Refactor status provider collection to return `StatusProviderSnapshot` and compose `StatusSnapshot` from provider and local partitions; retain the old public route until Task F and prove JSON equality.
5. Implement `DashboardSource for RepoContext` (or a small repo-backed adapter if cache ownership requires construction) with monotonic allocation, short-held cache mutation, `ReuseCurrent`, `Fresh`, and typed error mapping.
6. Add focused source tests for phase order, paired results, one traversal, no gate rescans, exact limits/omissions, stale/Fresh behavior, provider raw preservation, compatibility, and cancellation.
7. Run format, Clippy, relevant crate tests, and applicable Jig gates; run the requested comprehensive review at most twice, addressing every finding before commit.

## Validation and acceptance

Success means every publishable local refresh has one epoch ID and exactly one traversal of sessions, plans, decisions, and receipts; open-plan gate projection performs no hidden state scan; and recorder/status-local output is derived from the same epoch. Status announces provider and local phases before their work and reflects mutations made while providers run. `ReuseCurrent` performs no I/O, failed or cancelled refreshes retain the prior epoch, `Fresh` does not replace it, and the complete stale/not-found matrix is deterministic.

Typed loop data must preserve producer `workflow_id` and `item_key`, including shell-safe recovery argv. Typed gate data must preserve status, freshness, timestamps, summaries, paths, findings, and canonical remediation argv. Status-v1 JSON must remain semantically equal for supported records, including unknown provider extensions and exact accepted raw reports. Every collection reports its applied limit and computable omitted count.

## Idempotence and recovery

The adapter is not routed by any public command during B2. Reverting its new modules and the typed producer seams restores the prior web/status implementation without migrating files or durable records. Epoch publication is an in-memory atomic replacement; unsuccessful work has no side effects. `.agent/state/*.jsonl` remains append-only.

## Interfaces and dependencies

The primary interface is Task A's `jig_ui::DashboardSource`. New crate-private producer interfaces return typed loop, gate, provider, and epoch observations, plus prepared batched-gate inputs. They depend only on existing workspace crates and Task B1's state primitives; no terminal, HTTP, or new persistence dependency is introduced.

Revision note (2026-09-05): replaced the work-start stub with a self-contained B2 execution plan after tracing the existing producer and scan boundaries.

Revision note (2026-09-05): recorded the two review/fix rounds, final compatibility and resource-safety decisions, and passing exact-diff evidence before closing Task B2.
