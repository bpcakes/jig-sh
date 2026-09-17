# Bound work-link projection and pin tracker authority

This ExecPlan is a living document maintained according to `.agent/PLANS.md`. Its structured work ID is `plan_01M2Q2EAGN5WH0GZQXEM32G0DM`; its exact baseline is commit `5f9b01d35e5ae680dacf2f910d15ddbc5fe4556f` on branch `feature/jig-sh-x8ow-beads-evidence`.

## Purpose / Big Picture

PR #38 establishes Beads-compatible JSONL as a read-only task-data boundary and retains an append-only work-link journal. Two review findings show that the implementation does not yet fully enforce the boundary promises: a work-link scan can retain unbounded aggregate memory, and validation of `.beads` is separated from later ambient pathname opens so a concurrent parent replacement can redirect a read.

After this work, every work-link scan uses memory bounded by explicit unique-event and known-plan ceilings while retaining only compact semantic fingerprints and one folded state per plan. Every tracker read opens the repository and `.beads` as directory capabilities and performs export selection, metadata checks, file opening, and final witnessing relative to the pinned `.beads` handle. Focused tests demonstrate both limits and a deterministic parent-directory replacement race on Linux and macOS.

## Progress

- [x] (2026-09-17) Confirmed both review findings against baseline `5f9b01d3` and identified the wider unbounded projection state beyond the cited full JSON values.
- [x] (2026-09-17) Opened structured work plan `plan_01M2Q2EAGN5WH0GZQXEM32G0DM`.
- [x] (2026-09-17) Replaced full-value/per-record projection retention with compact event fingerprints and folded per-plan authority under 100,000-event and 100,000-plan ceilings.
- [x] (2026-09-17) Replaced ambient tracker child paths with one pinned `.beads` directory capability and descriptor-relative selection/open/re-witness operations.
- [x] (2026-09-17) Added focused limit, folding, semantic-fingerprint, diagnostic-truncation, and parent-replacement race tests; updated the public resource and filesystem authority contracts.
- [x] (2026-09-17) Built the development binary and passed the required Linux test, Clippy, formatting, contract, and file-budget gates against the complete implementation worktree.
- [x] (2026-09-17) Validated exact implementation commit `09ba9d5e` on macOS 26 arm64: all 32 tracker-filtered tests passed, including the directory-replacement race regression.
- [x] (2026-09-17) Re-ran the required gates against committed source authority: 4,126 tests, strict Clippy, formatting, contract, and file-budget checks passed.
- [x] (2026-09-17) Closed structured work successfully after confirming all required evidence was fresh.
- [x] (2026-09-17) Prepared the final evidence-only closure commit after all implementation acceptance criteria passed.

Restart checkpoint: implementation commit `09ba9d5e` is pushed. Focused Linux and macOS validation and the full required committed-source work check pass, structured work is closed successfully, and the final evidence is ready for delivery.

## Surprises & Discoveries

- Observation: the work-link projection has four independent aggregate growth paths: full JSON values keyed by event, full records grouped by plan, unbounded event-ID output for a selected plan, and unbounded diagnostic vectors.
  Consequence: replacing only `serde_json::Value` with a digest would reduce amplification but would not establish a bounded-memory contract.

- Observation: `cap-std` and `cap-fs-ext` are already workspace dependencies, and existing plan/receipt storage uses `Dir::open_ambient_dir`, `open_dir_nofollow`, and descriptor-relative `open_with` calls.
  Consequence: the tracker fix should reuse the repository's capability pattern rather than add platform-specific `openat` wrappers.

- Observation: bounding full JSON values and plan records was insufficient because a formatted diagnostic could itself retain an almost 2-MiB untrusted field, and an inline optional full record would inflate every plan state even when absent.
  Consequence: diagnostic text is truncated when first retained, samples and selected-plan state are boxed, and every non-selected plan keeps only fixed-size digests, counts, and optional bounded diagnostic pointers.

- Observation: canonical `serde_json::Value` serialization preserves the former equality behavior across whitespace and object-key order while still including unknown additive fields.
  Evidence: `event_fingerprint_preserves_complete_json_semantics_not_layout` replays reordered JSON as identical, while the existing unknown-field conflict regression still rejects changed semantics.

- Observation: the real repository export remains readable through the capability-based path.
  Evidence: a temporary generic tracker configuration made the rebuilt development binary's Doctor report `.beads/issues.jsonl`, profile `beads-rust-jsonl-v1`, 205 issues, read-only operation `read_issue_snapshot`, and `write_authority = false`; the temporary configuration was then removed.

## Decision Log

- Decision: keep streaming projection rather than impose a total journal byte ceiling.
  Rationale: the stream is intentionally append-only. A byte ceiling would make age alone invalidate otherwise valid history. Streaming may take time proportional to journal length, but its resident identity state will be explicitly bounded.
  Date/Author: 2026-09-17 / Codex.

- Decision: retain complete duplicate-event semantics through a domain-separated SHA-256 fingerprint of the strictly decoded JSON value, and fold each plan to a link-identity fingerprint plus bounded diagnostics. Retain a full canonical record and event-ID list only for the single plan requested by `project_work_link` or append retry logic.
  Rationale: duplicate detection needs equality across unknown additive fields, while global authority diagnostics do not need every full record. Cryptographic fingerprints preserve that contract without record-sized retention.
  Date/Author: 2026-09-17 / Codex.

- Decision: open `.beads` once without following links and treat that directory handle as the authority for the entire snapshot read.
  Rationale: pathname validation is a transient observation. Descriptor-relative access preserves the identity that was validated even if the repository pathname is concurrently renamed or replaced.
  Date/Author: 2026-09-17 / Codex.

## Outcomes & Retrospective

The two findings shared a missing-authority-model root cause rather than isolated coding mistakes. Journal projection had record-size bounds but no aggregate state contract; tracker reads validated path text but did not preserve the validated parent identity. The resulting implementation folds the journal under explicit cardinality ceilings and makes `.beads` an opened directory capability for the complete read. Implementation commit `09ba9d5e` passes 4,126 repository tests and all other required gates on Linux, plus all 32 tracker-filtered tests on macOS 26 arm64. The structured work item closed successfully with fresh required evidence.

## Context and Orientation

`crates/jig/src/state/work_links/projection.rs` streams `work-links.jsonl` but currently stores every decoded `serde_json::Value` in `seen_events` and every supported `WorkLinkRecordV1` in `records_by_plan`. A unique event ID must be remembered so a later replay can be compared for complete JSON semantics. A plan must be folded so multiple union-merged links either converge on the same issue or become a conflict. These requirements need identity state, but not record-sized state for every line.

`crates/jig/src/state/work_links.rs` exposes `project_work_link`, append retry behavior, and deep-diagnostic summaries. A scan used for one plan may retain that plan's canonical record and event IDs because they are returned to the caller. A diagnostic scan needs only counts, authority classification, and bounded samples.

`crates/jig/src/tracker.rs` currently validates `.beads` through `validate_repository_directory_path`, then later calls `symlink_metadata` and `OpenOptions::open` through ambient paths. `O_NOFOLLOW` protects only the leaf. A directory capability is an already-open directory handle used as the starting point for child operations; it prevents a later pathname replacement from redirecting those operations.

## Plan of Work

First, introduce production projection limits for unique event IDs and known plan IDs. Replace `SeenEvent.value` with a fixed semantic digest. Replace `records_by_plan` and the several diagnostic maps with one compact `PlanState` per known plan. Each plan state will count replays and supported records, retain only bounded diagnostic samples, and compare supported links by a fixed identity digest. When the scan has a selected plan, only that state retains the canonical full record and its distinct event IDs. Crossing an identity ceiling returns a typed projection-limit error before another entry is retained; deep diagnostics classify that as unsupported capacity rather than malformed data.

Second, refactor tracker opening around `cap_std::fs::Dir`. Open the repository root, then `.beads` with `open_dir_nofollow`. Select `issues.jsonl` or `beads.jsonl` through descriptor-relative `symlink_metadata`; open the selected leaf with `FollowSymlinks::No`; validate the opened file and link count; read it twice through the same descriptor; then re-witness selection and leaf identity through the pinned directory before parsing. The returned relative path remains `.beads/<name>` and no absolute or detached path enters durable state.

Third, add unit tests with deliberately small projection limits so the unique-event and known-plan ceilings are exercised without large fixtures. Assert that many large records do not remain in the folded state. Add a Unix race test that pins an empty `.beads`, replaces its repository pathname with a symlink to an outside directory containing a valid export, and proves the outside export is not accepted. Run that test on macOS as well as Linux.

Finally, update `docs/public-contract.md` and the delivery plan with the aggregate projection contract and capability-based snapshot semantics. Build `target/debug/jig`, run focused tests, then run the repository's required work gates. Commit and push the implementation, run focused tracker tests from the exact implementation commit on macOS, record outcomes, and update PR #38.

## Concrete Steps

Run all commands from the repository root.

1. Edit source and tests with `apply_patch`, then run:

       cargo fmt --all
       cargo test -p jig-sh state::work_links --no-fail-fast
       cargo test -p jig-sh tracker --no-fail-fast
       cargo test -p jig-sh doctor::tests::tracker --no-fail-fast

2. Build and run required gates through the changed runtime:

       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M2Q2EAGN5WH0GZQXEM32G0DM
       JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M2Q2EAGN5WH0GZQXEM32G0DM

3. Commit and push, then use a clean temporary checkout of the implementation commit on the configured macOS host and run:

       cargo test -p jig-sh tracker --no-fail-fast

4. Finish the structured plan only after the required receipts and macOS result are recorded.

## Validation and Acceptance

The work-link regression is accepted when a scan retains no full JSON value after observing a line, retains a full `WorkLinkRecordV1` only for the selected plan, rejects another unique event or known plan before crossing its documented production ceiling, and still preserves exact replay, conflicting replay, union-merge convergence, diagnostic classification, and append idempotence behavior.

The tracker regression is accepted when current and legacy exports still parse, ambiguous names still fail, symlinked and hard-linked leaves still fail, and a deterministic replacement of `.beads` after it is opened cannot redirect the reader to an outside valid export. The focused tracker suite must pass on Linux and macOS arm64.

The task is complete when focused tests pass, all required Jig gates pass, `git diff --check` is clean, public documentation matches the implementation, PR #38 contains the committed fix, and the worktree is clean.

## Idempotence and Recovery

All new reader behavior is read-only. Tests use private temporary directories and generic fixture names. A failed projection limit does not alter the journal, and an append performs the complete bounded scan under its existing write lock before writing. Re-running focused tests and gates is safe. The macOS test checkout is temporary and must be removed after validation.

## Interfaces and Dependencies

Use existing `sha2`, `cap-std`, and `cap-fs-ext` dependencies; add no crate. `JournalProjection` should accept an optional selected plan and production `ProjectionLimits`. `SeenEvent` should retain only digest, first line, and optional plan ID. A compact plan state should own authority counts/samples and optionally one selected canonical record plus its event IDs. A typed projection-limit error must name the exhausted dimension and limit without including task content.

Tracker access must use `cap_std::fs::Dir` and `cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt}`. All `.beads` child lookup and open calls must be relative to the pinned directory handle; ambient path use after that handle opens is forbidden.

Plan revision note (2026-09-17): initial plan created after reproducing both review findings and choosing streaming folded projection plus capability-relative tracker access as the holistic corrections.
