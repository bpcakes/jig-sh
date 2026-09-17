# Close JSONL admission and open races

This plan tracks work from baseline `206af4eeddab8faa616b99b027eb0a797b9c6adf` on `feature/jig-sh-x8ow-beads-evidence`.

## Purpose

Complete the JSONL authority boundary before the staged linking workflow builds on it. A successful append must leave the journal projectable, and observing an untrusted export must not block before its descriptor type is known. Restore durable-state documentation that was unrelated to the Beads re-scope.

## Progress

- [x] Reproduced the writer omission: aggregate limits were applied only to existing records, not the candidate being committed.
- [x] Added candidate-inclusive projection admission under the existing journal write lock.
- [x] Added actual append-path regressions for event and plan capacity, unchanged bytes, readable existing links, and exact retry at capacity.
- [x] Added nonblocking Unix export acquisition and a deterministic metadata-to-open FIFO replacement regression.
- [x] Restored the complete pre-rework Runtime State contract, then added only the new work-link and Beads sections.
- [x] Passed focused work-link and tracker suites, formatting, whitespace validation, and strict all-target Clippy.
- [ ] Run required repository gates, exact-commit macOS tracker validation, and finish delivery.

## Decisions

- Reuse `JournalProjection::observe` for admission rather than duplicate capacity arithmetic in the writer. This makes future projection invariants candidate-inclusive by construction.
- Keep exact retries on the existing-link path. They reconfirm durability but consume no event or plan identity capacity.
- Add `O_NONBLOCK` only on Unix, alongside `O_NOFOLLOW`, and retain all post-open regular-file and identity checks.
- Restore unrelated documentation verbatim from `origin/master`; do not rewrite those contracts as part of this feature.

## Validation

Focused acceptance requires 24 work-link tests and 33 tracker-filtered tests to pass. The FIFO regression must replace a regular export only after the production metadata check and must remain bounded if nonblocking acquisition regresses. Final acceptance also requires formatting, strict all-target Clippy, configured Jig gates, macOS tracker validation, a clean diff, and an updated PR.
