# Bound receipt publication across session and journal locks

Receipt publication must not wait indefinitely for session attribution before reaching
its existing journal timeout. Preserve ordinary cancellation receipts and prompt rollback
for cancellation-sensitive loop maintenance. No persisted record schema changes.

## Progress

- [x] Trace receipt entry points, session readers, journal locks, and cancellation callers.
- [x] Reproduce ordinary check SIGINT hanging behind the session lock.
- [x] Centralize publication policy and share one deadline across its lock acquisitions.
- [x] Verify deadline composition, cancellation receipt persistence, and rollback behavior.
- [x] Run configured work gates, inspect evidence, and finish structured work.

Restart checkpoint: plan `plan_01M2R99CDNHNHWWZHE0HTTFYNT`, baseline
`a3cbda637f577ec0b55ebf9dbcf74f045bba6642`. Implementation complete; 45 focused tests and fmt/clippy/contract passed. Full backend tests passed (4,199 passed, 3 skipped); all required gates and evidence are fresh, and structured work is closed. Changes remain local for review.
Unrelated `.beads/issues.jsonl` modifications must remain untouched.

## Surprises & Discoveries

The cancellation callback on ordinary receipt recording cancels optional Git enrichment;
it intentionally does not abort durable recording of a cancelled operation. The journal
already gives such recording a 30-second lock budget, but session attribution occurs
before that budget exists. The earlier fix only joined the explicit-deadline path to
session-pointer locking. Read-only inspection and dashboard cancellation are already fixed. Journal locking also
previously tried to acquire an available lock before checking deadline expiry; the shared
deadline contract now rejects expired publication even when the journal is uncontended.

## Decision Log

- 2026-09-17: Keep the session writer lock: compare-and-clear ownership requires
  synchronization, and removing reader locking alone would expose partial pointer writes.
- 2026-09-17: Express finalization versus cancellation-sensitive publication explicitly.
  Every receipt entry point uses the same bounded session-read and journal-append path.
  Ordinary finalization starts one 30-second lock budget after optional enrichment;
  transactional publication retains the caller's absolute deadline and cancellation.
  Never silently discard session attribution on timeout or suppress recording solely
  because the command was cancelled.

## Outcomes & Retrospective

The CLI regression failed before the fix after 35 seconds with the session lock held;
with the fix it exits at the 30-second finalization budget and preserves SIGINT status.
All 45 focused receipt and signal tests pass, including recording cancelled outcomes
after temporary session/journal contention, retaining the journal deadline, and loop
rollback. An additional expired-deadline test covers explicit session attribution, which
bypasses the session reader. Format, strict Clippy, contract, and file-budget checks pass. The required `JIG_DEV_BIN=target/debug/jig scripts/jig check test --plan-id
plan_01M2R99CDNHNHWWZHE0HTTFYNT` passed: 4,199 tests passed and 3 skipped. Default work check reused all five passing targets. Gate and evidence inspection
passed with `--freshness-timeout-ms 30000` after the default two-second inspection
budget was exceeded. Work finish closed the plan and its owning session successfully.

## Execution and validation

From the repository root, run focused `cargo nextest run -p jig-sh -P local` receipt
and runtime signal tests. Use the actual 30-second budget in the CLI regression to prove
the public entry point, and short deadlines for unit tests of sequential lock waits.
Build `cargo build -p jig-sh --bin jig`, then use `JIG_DEV_BIN=target/debug/jig` for
`scripts/jig work check --plan-id plan_01M2R99CDNHNHWWZHE0HTTFYNT`, inspect `work gates`
and `work evidence`, and finish only after required gates pass. Backend verification
must include `scripts/jig check test`. Review the final diff and document actual results.

## Root-cause assessment

The ownership synchronization itself is necessary: an unrelated session must not be
cleared between a read and a later mutation. The regression came from exposing that
synchronization through the previously cheap current-session query without carrying
caller resource and progress constraints. Reusing writer acquisition first added write
permissions to inspection; unqualified blocking acquisition then hid cancellation.
Those reader issues were corrected earlier. Receipt publication still had two separate
compositions, only one of which supplied a deadline to session lookup.

Ordinary cancellation has two phases: stop optional enrichment, then attempt to persist
the outcome. Passing the same cancelled callback indiscriminately into both phases
would erase cancellation receipts. The local architecture repair is to make publication
policy explicit and select it once for the common session-read and journal-write path.
The storage schema, ownership protocol, and read-only observer path need no migration.

Validation notes: Initial focused reruns exposed two test expectations that needed to
reflect the contract: an already-expired budget now fails before the first journal lock,
and SIGINT redelivery does not render the ordinary timeout error. The held-sidecar test
now supplies a live deadline; the CLI regression asserts timely signal exit while the
lock remains held, while unit tests assert timeout errors and absence of appended receipts.
