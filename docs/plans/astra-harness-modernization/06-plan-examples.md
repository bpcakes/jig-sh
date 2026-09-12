# Compact and extended plan examples

These two examples cover the same Jig change at the task 06 baseline. They illustrate
valid plan shapes, not executed work or verification evidence. Their restart state is
intentionally before implementation. Use the actual work ID and baseline on execution.

## Compact example

Change new goal plans so omitted checkpoints use the supplied success condition.
Preserve supplied checkpoint order, constraints, notes, mandatory validation, CLI/JSON
request fields, and existing durable plans. Acceptance: a no-checkpoint request produces
one outcome checkbox; supplied checkpoints replace it; blank items still fail.

Edit `GoalHarness::from_request` in `crates/jig/src/runtime/work/goal.rs` and the
checkpoint assertions in `crates/jig/src/runtime/tests/work.rs`. The CLI and JSON
paths share `WorkGoalRequest` in `crates/jig/src/command/work.rs`; cover both paths.
2026-09-12 decision: use the normalized success text directly so generation needs no
model call or invented milestone. No persisted format changes are needed.

Restart: inspected the generator; no edits or checks yet. Next replace the default
list and update coverage. No blocker. From the repository root, build
`cargo build -p jig-sh --bin jig`, then run
`JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id <actual-plan-id>`.
Expected: the configured profile passes, including goal and snapshot tests. Record
actual results and remaining gaps before finishing. On resume, reconcile the worktree
and current receipts before repeating work.

## Extended example

Outcome and acceptance are the same: newly generated plans default to one checkbox
containing the supplied success condition. Explicit checkpoints retain their order
and meaning. Constraints, notes, mandatory validation, CLI/JSON fields, and historical
plan bodies remain intact; blank supplied items are still rejected.

Context: `GoalHarness::from_request` in `crates/jig/src/runtime/work/goal.rs` inserts
a generic list only when checkpoints are empty. `WorkGoalRequest` in
`crates/jig/src/command/work.rs` accepts omitted or null optional lists. The shared
runtime creates the plan and returns its path and prompt; it does not execute the
validation commands. Existing tests in `crates/jig/src/runtime/tests/work.rs` exercise
CLI dispatch and expect the old default checklist.

2026-09-12 decision: normalize the supplied success text for the fallback checkbox.
Keep the original success text in its body section. This preserves a task-specific
outcome without adding model dependencies or new request fields.

1. Replace the fallback list and align the CLI checkpoint help in
   `crates/jig/src/cli/work.rs`. Preserve request validation and response fields.
2. Update CLI assertions and cover the JSON tool path with omitted, null, explicit,
   and invalid lists. Include an explicit user approval checkpoint and a planning-only
   objective to guard their preservation.
3. Build `cargo build -p jig-sh --bin jig` from the repository root. Run
   `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id <actual-plan-id>`;
   expect all configured targets to pass. Record actual results or blockers. Finish
   only after acceptance and required evidence are current.

Compatibility and recovery: this changes generated text for new requests. Do not
rewrite existing plan bodies or append-only journals. Calling goal creation again
opens another plan; resume the recorded plan ID rather than recreating it. No staged
database rollout is needed for this change. Changes to a persisted format in another
task would require mixed-version behavior, rollout order, and data-preserving recovery.

Restart: inspected the generator; no edits or tests yet. Next change the fallback in
`GoalHarness::from_request`. Record the work ID and baseline when opening structured
work, then keep completed steps, decisions, next action, and evidence current. No
blocker or unresolved user decision is known.
