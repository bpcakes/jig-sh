# Report partial work-plan closure failures

Expose committed closure explicitly when receipt publication or subsequent session
teardown fails during work finish or retirement. This adopts the review's explicit
partial-completion option; it does not introduce automatic retry or a transaction across
append-only streams. Success behavior and persisted record formats remain compatible.

## Progress

- [x] Trace plan close, receipt publication, session teardown, and CLI/MCP errors.
- [x] Reproduce failure using an invalid receipt lock after work start.
- [x] Add a shared typed partial-completion error and surface its details in CLI/MCP.
- [x] Verify finish and retirement state, receipt certainty, session behavior, and retry.
- [x] Run repository checks, inspect work evidence, and finish the plan.

Checkpoint: plan `plan_01M2RJ7N6WN164N72E7Y0VKQ4E`, baseline
`0a1af959fb1e7fcef6f59b44a9e96d55526bddea`. Both original failure paths reproduced before the fix. All 41 focused lifecycle and transport tests pass. Formatting, Clippy, contract, and file-budget checks pass; the full backend suite passed 4,233 tests with 3 skipped. Work gates and evidence are fresh and passed; work finish closed the plan and ended its owning session.
Preserve unrelated `.beads/issues.jsonl` edits. No commit or push requested for this fix.

## Surprises & Discoveries

The close event, state receipt, and session teardown are separate writes. A failed
receipt append can also be ambiguous after bytes were written, so an error must not
always assert that the receipt is absent. Later session teardown can itself fail after
changing session state. Both cases need truthful partial-completion diagnostics.

## Decision Log

- 2026-09-17: Report partial completion instead of adding implicit retries. A safe retry
  protocol would need receipt deduplication, stored closure identity, and reconciliation
  of ambiguous appends and session cleanup. That is a separate lifecycle feature.
- 2026-09-17: Keep existing failure exit/RPC behavior. Add partial-completion fields to
  error payloads, retain the original cause, and provide read-only inspection commands.
  Normal pre-commit errors must not claim committed closure. A repeated finish/retire
  remains rejected and cannot rewrite closure metadata or append another close event.

## Outcomes & Retrospective

Implemented and verified by 41 focused tests, including nine new regressions. The
original finish/retirement reproductions failed on their missing partial-completion
diagnostics before the fix and passed afterward. Formatting, Clippy, contract, and
file-budget checks pass. The full backend suite passed 4,233 tests with 3 skipped.
Work check reused all five fresh passing target receipts; gates and evidence passed.
Work finish closed this plan and ended its owning session successfully.
Acceptance: both lifecycle commands expose
closed plan state and the close event ID on publication failure; report receipt state
as not_recorded or unknown; identify whether session teardown was attempted; retain
nonzero CLI / MCP error behavior; and preserve all existing ownership and gate rules.

## Validation

Use deterministic filesystem failures for end-to-end runtime and transport tests,
plus a focused test of post-write receipt uncertainty. Build the dev binary and run
`JIG_DEV_BIN=target/debug/jig scripts/jig check test --plan-id plan_01M2RJ7N6WN164N72E7Y0VKQ4E`.
Run required work gates and inspect gates/evidence with the documented extended
freshness budget when necessary. Finish only after all required checks pass.
