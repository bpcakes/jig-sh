# Durable native file-budget context, results, and contract epoch

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Bead `jig-sh-generic-monorepo-zac.8.3` from baseline `97db14b7677ad277724a4ee14e4378eeb908fd82`. The observable outcome is that contract-v7 repository plans can carry bounded, independently authenticated file-budget policy and comparison authority; accepted runs use the persisted authority; typed native results retain complete finding metadata and calendar validity; and supported v2-v6 repositories, plans, and receipts continue to load with their historical semantics.

## Progress

- [x] 2026-08-30: Claimed the Bead, inspected its dependencies and the universal file-budget design, and opened structured work at the exact Git baseline.
- [x] 2026-08-30: Confirmed contract v6 is current at the baseline, so this task owns contract v7 and must not predeclare the later argv or target-freshness schemas.
- [x] 2026-08-30: Added the serialized DTOs, checked-in native configuration, run-plan schema compatibility, and lazy preparation/replay-authentication service.
- [x] 2026-08-30: Added typed native action context/results and bounded finding/evidence projection with complete diagnostic identity through truncation.
- [x] 2026-08-30: Added generic `valid_until_ms` receipt persistence and enforced it through direct, batch, scoped, reusable, latest, and archival evidence.
- [x] 2026-08-30: Activated target-local non-empty affected matching only for contract v7 while retaining v6 component aggregation.
- [x] 2026-08-30: Passed focused compatibility tests, strict Clippy/formatting, the development-binary contract check, all eight configured Jig gates, and `scripts/jig check test` (3,205 passed, 2 skipped).

## Surprises & Discoveries

- The Task B comparison and scope types are intentionally crate-private and non-serializable. Task C must promote only bounded authority (`ComparisonRequestV1`, `ResolvedComparisonV1`, current view, provenance, and fallback evidence) into `jig-contract`; the potentially large scope stays internal.
- Run-plan acceptance already re-plans an untrusted submitted plan and compares the complete value. Adding prepared input to `PlannedTarget` makes independent replay authentication part of the existing durable acceptance boundary instead of creating a second verifier.
- Repository file-budget execution is deliberately Task D. This task must register and transport the typed operation/context and normalize failed preparation states, while leaving ready-scope materialization and evaluation to the dependent task.
- Existing target receipts already feed work evidence, latest evidence, and archive protection. One optional top-level validity boundary can be projected through those paths without rewriting append-only history.
- The first configured gate run exposed physical-file LOC ceilings in three touched orchestration files. Moving the added helpers behind their existing include/module seams restored the policy without annotations or behavior changes.
- A broad `cargo test -p jig-sh --lib` attempt observed five Git-fixture failures caused by ambient parallel Git environment leakage; immediate isolated comparison-scope execution passed all 19 tests, and the hermetic Nextest gate subsequently passed all 3,205 selected tests.

## Decision Log

- Decision: Allocate contract version 7 and keep versions 2 through 6 readable.
  Rationale: v6 is authoritative at implementation start; the Bead graph reserves the next epoch for file budget and orders argv and freshness after the file-budget cutover.
- Decision: Bump the run-plan schema from 2 to 3 while deserializing schema-2 plans through defaulted new fields.
  Rationale: new plans authenticate prepared native input, but persisted old plans must remain inspectable/readable rather than being rejected by Serde.
- Decision: Store native configuration on the native runner as an optional tagged value and normalize `jig.file_budget` to a fully defaulted `NativeFileBudgetConfigV1` in the v7 catalog.
  Rationale: checked-in repository authority owns ceilings and fallback; invocation arguments cannot raise them.
- Decision: Preserve bounded comparison authority in the run plan and re-resolve it only during plan acceptance.
  Rationale: workers must consume persisted OIDs and must not rediscover symbolic references after durable acceptance.
- Decision: Treat policy invalidity as `Failure` before comparison unavailability; treat valid-policy comparison unavailability as `Blocked`; convert configured fallback to authenticated ready strict inventory with original failure evidence.
  Rationale: this is the deterministic precedence required by the Bead and design.
- Decision: Enforce receipt validity as `now_ms < valid_until_ms`; equality is expired.
  Rationale: `valid_until_ms` is the first invalid instant after an inclusive waiver expiry date.

## Outcomes & Retrospective

Contract v7 now carries fully defaulted, hard-capped native file-budget configuration and bounded `PreparedNativeInputV1` authority in run-plan schema 3. Policy and comparison preparation are independent and replay-authenticated; exact-tree, merge-base, strict-inventory fallback, diagnostic, requested-object, configuration, and work-plan identities participate in durable plan equality. The native dispatch seam returns typed conclusions and preserves finding totals, truncation, complete digests, bounded evidence, evaluation time, and optional validity in run results and append-only target receipts. Ready evaluation remains intentionally blocked with `file_budget.engine_pending` until Task D supplies the engine.

Receipt validity is enforced as `now_ms < valid_until_ms` through direct and target freshness, work-check batches, scoped/reusable/latest evidence, and archive protection. Old plans, runners, target results, and receipts deserialize with absent additive fields. Contract-v7 affected selection is target-local for non-empty inputs, while v6 retains component aggregation and dependency expansion remains ordered afterward.

Validation used the freshly built development binary. Configured work receipt `receipt_01M19QXHVEMA99SF7EM1EQCY5T` records all eight gates passing and fresh; its partitions passed 2,447 core, 107 frontend, 442 vault, and 209 process tests. The required direct `scripts/jig check test` receipt `receipt_01M19R2DYHDF1K18566MYVKHAF` passed 3,205 tests with 2 intentional skips. Task D owns ready-scope materialization/evaluation, Task E owns generated action and policy rendering, and `.1.2` owns migrating this source repository to the new epoch.

## Context and orientation

`crates/jig-contract/src/repository.rs` owns checked-in action DTOs. `crates/jig-contract/src/run.rs` owns serialized run plans and results. `crates/jig/src/repository/planner.rs` creates plans and replay-authenticates submitted plans. `crates/jig/src/git_receipts/comparison.rs` and `change_scope.rs` own bounded Git authority supplied by completed Task B. `crates/jig-file-budget` parses pure policy and returns deterministic diagnostics without reading repository state. `crates/jig/src/runtime/run_execution.rs` dispatches planned targets and records target receipts. `crates/jig/src/state/records.rs`, `state/receipts.rs`, and `runtime/work/gates*` own durable evidence and freshness. `crates/jig/src/repository/affected.rs` owns affected selection. `crates/jig/src/context.rs` and rendered templates own the current repository contract epoch.

Prepared input is bounded authority, not an evaluation snapshot. It contains no file contents, measurements, or unbounded path inventory. A planner prepares policy and comparison independently for a selected built-in `jig.file_budget` target. Durable acceptance repeats that preparation and requires the whole plan to match. Only then may a worker materialize a scope from persisted OIDs; Task D owns that materialization and evaluation.

## Plan of work

First, add serde/schema-stable contract DTOs for current view, comparison request/resolution, preparation failures and previews, strict-inventory fallback, native file-budget configuration, prepared input, and complete native result metadata. Normalize and validate configuration at catalog load, including immutable hard caps and operation/configuration pairing. Bump only the current repository epoch and run-plan writer while leaving historical readers default-compatible.

Second, promote Task B's bounded authority types to the shared DTOs and add a `file_budget` preparation module in the runtime. It reads the canonical policy from the selected current view within the policy byte cap, parses at one UTC instant, bounds and digests diagnostics, resolves the requested comparison, records typed failures and attempted identities, and produces authenticated strict-inventory fallback when configured. Planning remains lazy: only selected built-in file-budget targets need policy or comparison authority. Work checks pass their plan ID during planning, MCP accepts the same typed request, and execute-time IDs may only confirm equality/link receipts.

Third, introduce `NativeActionContext` and `NativeActionResult` at repository dispatch. Failed preparation returns typed `Failure` or `Blocked` without process-exit laundering. Ready file-budget input reaches the typed operation boundary that Task D will complete. Map total finding count, truncation, digest, bounded findings/output/evidence, evaluation time, and optional validity into `TargetRunResult` and target receipts.

Fourth, add optional `valid_until_ms` to append-only receipt records and every in-memory evidence status. Freshness rejects a receipt at or after the boundary even when source bytes match. Reuse scans cannot reuse expired evidence; scoped/latest projections show the boundary and enforced reason; archive protection does not preserve expired evidence as current proof. Historical receipts without the field retain old behavior, except file-budget evidence that proves active waivers but omits validity is unknown/stale.

Fifth, switch affected selection by catalog contract version. v7 matches each action with non-empty inputs directly, retains component-root behavior for actions without inputs, then expands component/action dependency policy. v6 and older native catalogs keep existing component aggregation. Pin mixed-component and hidden-path cases.

## Concrete steps

1. Edit `jig-contract` DTOs and tests, then run `cargo test -p jig-contract`.
2. Add `jig-file-budget` as a runtime dependency, implement preparation, integrate it into planning and plan validation, and run repository planner/Git focused tests.
3. Integrate native context/result capture and receipt metadata; run repository execution and state receipt tests.
4. Integrate validity into work-gate evaluation/reuse/archive and run focused work/state tests with fixed boundaries.
5. Implement the v7 affected switch and compatibility tests for v6/v7.
6. Advance `CURRENT_CONTRACT_VERSION`, update public contract documentation and current-version fixtures without migrating the Jig source repository contract (owned by `.1.2`).
7. Run formatting and focused crate tests, build `cargo build -p jig-sh --bin jig`, export `JIG_DEV_BIN=target/debug/jig`, and run `scripts/jig work check`, `work gates`, `work evidence`, `work receipts`, `check contract`, and the required backend test gate.

## Validation and acceptance

Tests must prove: exact request/peeled/tree/config/policy/diagnostic/work-plan modifications make an untrusted plan stale; a worker-facing ready context contains persisted identities; invalid policy wins over comparison failure; block and strict-inventory fallback diverge as configured; old plan/receipt JSON loads; finding totals and digests survive preview truncation; validity expires at equality across every projection and reuse/archive decision; v7 target-local matching does not select sibling actions while v6 retains it; and v7 is the only new contract schema.

Success for focused commands is zero exit status with the named regression tests executed. Success for final gates is every applicable required gate reported passed/reused/not-applicable according to repository policy, `scripts/jig check test` passing through the freshly built development binary, and no unreviewed stale docs or compatibility assumptions in the final diff.

## Idempotence and recovery

Preparation and validation are read-only and deterministic at a supplied instant. Re-running planning after source, policy, ref, or config movement produces a new plan rather than mutating an accepted plan. Receipt additions remain append-only. The contract bump changes generated current templates but does not rewrite this repository's v6 authored contract; a failed implementation can be resumed from the open plan and current worktree. Do not delete or rewrite historical state.

## Interfaces and dependencies

The shared contract exposes versioned bounded authority and configuration only; it does not depend on Git or `jig-file-budget`. The runtime depends on `jig-file-budget` to parse policy and map pure diagnostics. Existing bounded Git execution remains the only Git process boundary. Task D will consume `NativeActionContext` and return `NativeActionResult` for ready inputs; Task E will render the action/configuration and policy; `.1.2` will migrate this repository to v7; `.8.6` will remove the temporary Bash checker after dogfood.
