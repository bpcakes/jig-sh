Implement docs/plans/astra-harness-modernization/review-repairs.md; cover legacy guides, cleanup failure recovery, classified partial traces, surviving grader descendants, and receipt privacy. Run configured verify gates and backend test evidence.


# Final repair record

This is the completed work record for plan_01M2AT6FARSY5QR7W3NJ37W47Q at baseline 8b43c0df. It supersedes the pending-verification checkpoint in docs/plans/astra-harness-modernization/review-repairs.md. That file retains the investigation and implementation context; final evidence is recorded here in excluded work metadata so recording completion does not alter validated source inputs.

## Progress

- [x] Preserve original guide validation for contract epochs 2–7; enable strict Markdown references at epoch 8.
- [x] Persist unresolved adapter cleanup as excluded cleanup_failed state, retain workspace authority, prohibit replay/finalization, and emit CLI exit 2 without traceback.
- [x] Require exhaustive-checks-v1 classification and boolean trace completeness before reporting repeated checks; retain complete=false for partial measurements.
- [x] Retire successful grader process groups before reaping their leaders, using file-backed output capture.
- [x] Sanitize future preview host roots and migrate exactly three historical stderr_preview fields with a durable decision.
- [x] Pass focused regressions and all six configured verify targets with the worktree stable.

Checkpoint: implementation and verification complete; close this plan using work finish. No implementation work remains. Changes are uncommitted.

## Surprises & Discoveries

The checkout preceded the reviewed source; a clean fast-forward supplied revision 8b43c0df. Epochs 6 and 7 are still supported, so the compatibility guard covers every supported epoch below 8. The same temporary-prefix leak occurred in two earlier receipts as well as the reported Python traceback.

All 4,086 Rust tests and 42 Python tests passed on the first full runs, but editing the narrative progress note while those commands ran invalidated their global worktree-effect proof. The complete verify profile was rerun successfully with source and documentation held stable. Those rejected receipts remain as truthful history.

## Decision Log

Strict links are staged at contract epoch 8, preserving upgrade compatibility. Exhaustive event classification is an explicit adapter contract, separate from whether the observed trace is complete; the bundled partial classifier therefore leaves repeated checks unavailable. Unresolved cleanup preserves an excluded trial and its workspace for operator inspection, without unsafe replay or grading.

The branch's existing state churn follows its required dogfooding workflow: before this repair it appended 43 plan records, 24 sessions, 368 run events, and 227 receipts without changing prior lines. Beads changes add the modernization graph and record completed work. Ordinary history is preserved.

Privacy decision decision_01M2AT6ZD613ST0FKH347AASZD records the migration of receipt_01M23TKHQ14V1N67PRRMZAJMPV, receipt_01M23TSN8X8PG4TTMZAD2CYN9S, and receipt_01M28FCX8QCG1ZVWXVCG7NX4G7. Only stderr preview prefixes changed; IDs and every unaffected field and line were verified against the baseline. No Git history was rewritten.

## Outcomes & Retrospective

All five reported issues are addressed. The final configured verify run run_01M2AVTBB0X8B77G6XKMPBWQMN passed Clippy, formatting, backend tests, contract validation, file budget, and the full harness suite. Target-validation receipt: receipt_01M2AX5P4W7Z7XT87BA6VRHRD2. Backend receipt: receipt_01M2AX59R47QTF0RYPPZMA2KHW. Harness receipt: receipt_01M2AX5B71N5RKA7E9RB63G053.

The backend suite passed 4,086 tests with 3 skipped. The harness suite passed all 42 tests in 1,397 seconds. Focused evidence also includes 13 guide unit tests, 7 guide CLI tests, five new Python regressions, and a successful CLI check of all 19 repository guides. One initial legacy CLI fixture omitted epoch-2 required-command metadata; correcting the fixture made all seven CLI tests pass.

Successful-command cleanup adds process-group inspection overhead, particularly to repeated experiment preparation on this Linux host. It preserves the required retirement guarantee. Runtime fixes were built locally and dogfooded with JIG_DEV_BIN=target/debug/jig. Keep future progress updates in excluded plan/state files during running verification.
