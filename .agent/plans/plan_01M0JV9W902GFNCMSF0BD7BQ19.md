# Split the Jig loop runtime by responsibility

This ExecPlan guides a behavior-preserving Fowler refactor of the `jig loop`
runtime. After the change, orchestration, immediate repeat scheduling,
machine-local runtime state, and compiled workflow implementations live in
focused Rust modules. The command surface and every observable runtime behavior
remain unchanged. This prepares the subsystem for a later scheduled Codex task
feature; it does not add scheduling behavior in this plan.

## Progress

- [x] Established a clean formatter, focused loop-test, and strict Clippy baseline.
- [x] Opened structured Jig work as `plan_01M0JV9W902GFNCMSF0BD7BQ19`.
- [x] Extract lease, attempt, and JSON cache persistence into `runtime/loops/state.rs`.
- [x] Extract immediate `run --until idle` policy into `runtime/loops/schedule.rs`.
- [x] Keep each compiled workflow in a workflow-specific module, including noop status.
- [x] Extract tick/status orchestration into `runtime/loops/engine.rs` and leave a thin facade.
- [x] Run focused and project-wide verification, inspect the diff, and finish structured work.

## Surprises & Discoveries

- The scanner flags `loops.rs` by size, but line count alone is not the reason to
  split it. Git history and source inspection show independent reasons to change:
  orchestration, status cancellation, workflow configuration, cache persistence,
  and built-in workflow behavior.
- The exhaustive workflow-kind match is the correct representation for the
  deliberately closed workflow set. This refactor will move it but will not
  introduce a trait or dynamic dispatch.

## Decision Log

- Decision: preserve the existing closed workflow dispatch instead of creating
  an extension trait.
  Rationale: `.jig.toml` intentionally accepts only compiled-in workflow kinds,
  and exhaustive matching makes additions compiler-visible.
- Decision: keep the existing cache records and file helpers together in a
  `state` module.
  Rationale: leases and attempts share the same locked JSON cache protocol and
  machine-local lifecycle.
- Decision: name the immediate repeat wrapper `schedule` now, while adding no
  clock, cron, interval, or due-run semantics.
  Rationale: it owns the current scheduling policy (`run` repeats `tick` until a
  stop state) and becomes the narrow seam for the later scheduling feature.

## Outcomes & Retrospective

The original 972-line facade is now 30 lines. Tick/status orchestration lives in
the 313-line `engine.rs`; the existing immediate repeat policy lives in the
56-line `schedule.rs`; workflow resolution and tuning live in the 182-line
`workflow.rs`; lease, attempt, and locked JSON cache state live in the 428-line
`state.rs`; and the noop implementation lives in its own 23-line workflow
module alongside the existing GitHub and PR-manager modules.

The refactor preserved the closed exhaustive workflow dispatch and introduced
no timer, cron, interval, or Codex task behavior. The 26 focused loop tests
passed after every extraction. Formatting, strict Clippy, 1,419 `jig-sh` unit
tests, integration tests, contract checks, the configured Jig test command, and
both required plan-scoped gates passed. One `codex_launcher` integration test
briefly hit Linux `ETXTBSY` during the first full run and passed immediately in
isolation; no source change was needed.

Future scheduling work now has a narrow policy seam in `schedule.rs` and can
reuse the engine and state boundaries without reopening the facade. Further
splitting the large workflow-specific PR-manager implementation is deliberately
deferred because it is independent of this behavior-preserving preparation.

## Context and orientation

`crates/jig/src/runtime/loops.rs` currently owns command dispatch, tick/status
orchestration, the immediate run loop, workflow resolution, noop workflow
behavior, leases, attempt budgets, cache I/O, and unit tests. GitHub observation
and PR-manager mutation already live in sibling files under
`crates/jig/src/runtime/loops/`. `crates/jig/src/runtime/tests/loops.rs` provides
26 end-to-end runtime characterization tests for the relevant command behavior.

The crate uses Rust edition 2024 with an MSRV of 1.85. The refactor is private to
the `jig-sh` crate and must not change public APIs, serialized receipt evidence,
mutable cache formats, filesystem paths, error strings, blocking behavior, or
platform feature behavior.

## Plan of work

Apply Fowler's Extract Class analogue as focused Rust modules and Move Function
in compiler-guided steps. First move state records and persistence without
changing their fields or serde behavior. Then move the immediate run policy.
Next move noop behavior so every workflow is isolated. Finally move orchestration
and workflow resolution, leaving `loops.rs` as the stable facade used by
`runtime.rs` and status collection.

Do not combine this refactor with schedule configuration, lease renewal, task
definitions, new traits, typed status enums, or behavior fixes. Those are later
changes after this structural seam is green.

## Concrete steps

1. Extract `LeaseStore`, `AttemptStore`, their records, classification, and
   locked JSON helpers into `runtime/loops/state.rs`. Adjust only visibility and
   imports needed by current callers. Run the 26 focused loop tests.
2. Extract `run_until` into `runtime/loops/schedule.rs`. It should call the
   existing tick boundary and return byte-for-byte equivalent JSON values and
   error text. Run focused tests.
3. Extract `noop_status_tick` into `runtime/loops/noop.rs`; retain the existing
   `WorkflowTick` value shape. Keep GitHub and PR manager as concrete modules.
4. Extract command orchestration into `runtime/loops/engine.rs`. Keep the facade
   signatures used by `runtime.rs` unchanged.
5. Format, run focused tests, strict Clippy, all `jig-sh` tests, contract checks,
   and structured work gates. Review the diff for unintended behavior changes.

## Validation and acceptance

Run after each extraction:

    cargo fmt --all -- --check
    cargo test -p jig-sh --locked runtime::tests::loops

Final verification:

    cargo clippy -p jig-sh --all-targets --locked -- -D warnings
    cargo test -p jig-sh --locked
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0JV9W902GFNCMSF0BD7BQ19
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0JV9W902GFNCMSF0BD7BQ19

Acceptance requires all existing loop tests to pass without assertion changes,
the public contract to remain unchanged, and `loops.rs` to become a thin facade
over focused modules with no new scheduling behavior.

## Idempotence and recovery

Each extraction is a pure source move and can be reapplied safely only when the
destination file is absent. If a step does not compile or its focused tests fail,
stop at the previous green module boundary and revert only that extraction via a
targeted patch. Do not delete or rewrite `.agent/state/*.jsonl`; structured Jig
commands append their own events.

## Interfaces and dependencies

No new crate dependency, feature, async runtime, trait, serialization type, or
public item is introduced. Internal modules may use `pub(super)` only where a
sibling needs an existing capability. `runtime::loops::dispatch` and
`runtime::loops::status_with_cancellation` remain the facade consumed outside
the loop subsystem.
