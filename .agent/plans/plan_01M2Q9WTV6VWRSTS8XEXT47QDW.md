# Correct tracker extension and Doctor lifecycle boundaries

This change restores the intended JSONL-first boundary. Jig will accept producer-defined
Beads classification values while retaining structural and resource validation, and its
pure tracker check will remain independent of Doctor's process signal session.

Acceptance requires regression tests proving custom `status` and `issue_type` strings are
accepted, malformed or unbounded strings still fail, tracker-only repositories do not
request a process session, and signal-session retirement does not invalidate a completed
tracker result. The relevant Linux checks, configured repository gates, and focused macOS
tests must pass.

## Progress

- [x] Associated Jig plan `plan_01M2Q9WTV6VWRSTS8XEXT47QDW` with baseline `3679cea1d3555dbc3f2eafd0b694d9feec60115f`.
- [x] Inspected the JSONL parser, Doctor orchestration, tests, and public documentation.
- [x] Move producer vocabulary policy out of JSONL structural validation.
- [x] Remove tracker validation from process-control and signal-retirement ownership.
- [x] Add regressions and align the public contract.
- [ ] Run focused and configured checks on Linux and focused tests on macOS.
- [ ] Commit, push, inspect the PR head, and close structured work.

Restart checkpoint: implementation and focused Linux validation are complete. Commit the
implementation, validate that exact commit with configured gates and macOS, then finish
structured work. There are no blockers.

## Surprises & Discoveries

- The earlier JSONL-first ExecPlan already states that Doctor process availability and
  cancellation are irrelevant to tracker validation, but the implementation retained the
  old process-control parameter and session invalidation behavior.
- The generic JSON validation already bounds every string and rejects NUL in known text
  fields, so accepting producer-defined classification values does not require weakening
  input-safety limits.
- The first tracker-only session regression inherited enabled proxy discovery and a default
  Codex marketplace from the generic fixture. The corrected fixture explicitly disables
  every non-tracker process trigger and asserts each precondition before testing session
  admission.

## Decision Log

- Decision: `status` and `issue_type` are producer-owned classification strings at the
  storage boundary; only operations that assign workflow meaning may constrain their
  vocabulary. Rationale: a read-only interoperability layer must not equate its current
  semantic knowledge with the producer's extensible format. Date/Author: 2026-09-17 / Codex.
- Decision: the tracker check has no process-control argument and never participates in
  signal-session admission or retirement invalidation. Rationale: capability ownership
  should follow actual side effects, and this check performs only bounded filesystem reads.
  Date/Author: 2026-09-17 / Codex.

## Outcomes & Retrospective

The parser and Doctor ownership fixes are implemented. The 36 focused tracker tests,
including both regressions, pass on Linux; strict Clippy and formatting also pass. Full
configured gates and macOS validation remain unfinished.

## Context and plan of work

`crates/jig/src/tracker.rs` owns bounded Beads JSONL decoding. It must continue checking
types, NUL, global string/line/file bounds, issue identity, priority, timestamps, and
tombstone semantics while ceasing to enumerate producer-defined classification values.
`crates/jig/src/doctor/tracker.rs` owns the pure Doctor check. `doctor_parts/part_01.rs`
decides whether a signal session is needed, and `doctor_parts/part_02.rs` invalidates
checks whose process evidence becomes unsafe when that session cannot retire.

Milestone 1 updates the parser and regression tests. Milestone 2 removes the unused
process-control parameter, the tracker-only signal-session trigger, and tracker retirement
invalidation, with direct orchestration tests. Milestone 3 updates the public contract,
formats the code, runs focused tests and Clippy, then executes the configured Jig gates and
focused tests in a clean macOS checkout of the exact implementation commit.

## Validation and recovery

Run from the repository root with `JIG_DEV_BIN=target/debug/jig`: focused tracker and Doctor
tests, `cargo fmt --check`, relevant Clippy, then `scripts/jig work check --plan-id
plan_01M2Q9WTV6VWRSTS8XEXT47QDW`. On macOS, test the exact commit in a temporary clean
checkout and remove only that validated temporary directory afterward. These changes alter
no persisted state schema and need no rollout migration; reverting the implementation
commit restores prior behavior.

## Interfaces and dependencies

No new dependency or public CLI shape is introduced. The `beads-rust-jsonl-v1` contract
changes only to classify `status` and `issue_type` as bounded producer-owned strings. The
internal `tracker_check` interface becomes `tracker_check(&RepoContext)`.
