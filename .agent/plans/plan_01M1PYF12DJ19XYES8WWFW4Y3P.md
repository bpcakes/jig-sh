# Harden cancellable readers and plan paths

This ExecPlan implements Task B1 (`jig-sh-l2x.2`) from `docs/plans/unified-terminal-dashboard.md`. The outcome is a shared set of bounded, cooperatively cancellable local readers and a descriptor-relative plan-body store used by both future dashboard reads and existing work mutations. No durable schema changes.

Implementation baseline: `41019c73` on branch `jig-sh-l2x`.

## Progress

- [x] Read repository guidance, Task B1, and every referenced safety/cancellation section.
- [x] Inspect existing JSONL snapshots, reverse receipt traversal, loop JSON cache, plan mutation paths, and cancellation sentinel.
- [x] Claim `jig-sh-l2x.2`, build the development binary, and open structured work.
- [x] Add bounded forward JSONL scanning with stable offsets and cancellable overflow discard.
- [x] Make reverse receipt locking, fallback snapshots, and window traversal cancellable and bounded.
- [x] Lower loop-cache input reads to the planned 8 MiB bound and cover cancellation/growth.
- [x] Add one canonical plan-ID validator and descriptor-relative plan-body read/create/append/lock store.
- [x] Route existing plan mutation and review-prompt callers through validated plan paths without changing valid output.
- [x] Add boundary, race, special-file, lock, no-write, and compatibility tests.
- [x] Run focused validation and two comprehensive working-tree review/fix rounds.
- [x] Record passing exact-diff evidence after two review/fix rounds.
- [x] Close the bead, flush Beads, and finish structured work.
- [x] Commit Task B1.

## Surprises & Discoveries

- The JSONL layer already polls cancellation for forward reads and stable unlocked snapshots, but retains unbounded logical lines and the receipt reverse path still uses blocking locks and whole-file fallback reads.
- Loop coordination reads already use 64 KiB chunks, cancellation, capability-relative no-follow opens, and regular-file verification. Task B1 only needs to tighten the input ceiling from 16 MiB to 8 MiB and strengthen its tests.
- The loop cache already demonstrates the repository's supported `cap_std`/`cap_fs_ext` descriptor-relative pattern. The plan-body store can reuse that audited dependency rather than introducing a second raw-FFI wrapper.
- Existing plan mutations call `ensure_state_layout`, path-join body filenames, and generic JSONL `append_text`; those operations create a time-of-check/time-of-use and symlink surface that must be replaced as one isolated call-site cutover.
- The broad `state::` test filter also selects slow frontend dependency-state fixtures. It passed 210 relevant tests but hit one unrelated, load-sensitive lease timing assertion; that exact test passed immediately in isolation.
- A reverse-scan differential test exposed that preallocating from an unchecked caller limit can panic at `usize::MAX`; the reader now grows only with observed matches.
- Validation initially exhausted the home volume. Removing only this repository's regenerable Cargo build cache recovered space; no source or durable state was removed.
- The local nextest profile expands to all 64 host CPUs. Under concurrent machine load that starved test-internal renewal and process-supervision threads, producing unrelated lease and process-tree failures. Two-worker validation completed both the 3,035-test core partition and 3,801-test workspace target without weakening assertions.
- Two existing lease tests used fixed sleeps as synchronization. One now waits for an actual protected-ledger renewal; the other revokes the owned lease through the existing test seam after an explicit worker-start marker.

## Decision Log

- Preserve existing unbounded JSONL functions for non-dashboard compatibility and add an explicit bounded scanner for future local-epoch consumers. Both share the same chunk loop and typed cancellation sentinel.
- Represent a raw record's stable identity input as its starting byte offset, computed by the scanner before any decode. Oversized records never reach hashing or Serde.
- Treat logical-record overflow as a typed internal error containing path, record offset, and ceiling after discarding to newline cancellably. This lets Task B2 map it to the public `record_too_large` partial error exactly once.
- Implement plan storage with `cap_std::fs::Dir` and `cap_fs_ext` no-follow operations. On Linux and macOS these resolve component-wise relative to held directory descriptors and set close-on-exec internally; final opens additionally request nonblocking behavior and verify the opened handle is regular.
- Keep the body filename convention `{plan_id}.md` and sidecar convention `{plan_id}.md.lock`; preserve legacy body-then-sidecar lock order, append semantics, and `sync_data`.
- Keep legacy unbounded readers for maintenance and compatibility callers, but route every current cancellable status-summary stream through the bounded scanner. Task B2 will consume these primitives for the unified dashboard epoch and map their typed failures to scoped public errors.
- Preserve unbounded `build_summary` and `state_summary` compatibility for mutation receipts and direct legacy callers; only the explicitly cancellable status path receives the 1 MiB ceiling.
- Fail closed when an open plan's body is missing instead of recreating a misleading fragment-only body. The error directs the caller to restore the original, and a regression test proves append creates nothing.
- Enforce the 8 MiB loop-state ceiling before both cache and durable publication so this release cannot write state that its own reader rejects.
- Apply the plan's deliberate 8 MiB ceiling to pre-existing loop state as a fail-closed safety cutover. Existing 8–16 MiB files are rejected with an inspect/repair message rather than being partially decoded or destructively rewritten; the public contract and upgrade regression test record this behavior.
- Shared-lock plan bodies during reads. Writers already acquire the verified body lock before the sidecar, so readers now see either the pre-append or post-append body and remain cancellable while waiting.
- Comprehensive review round 1 completed as a verified single-reviewer Claude pass; native Codex exhausted its account quota before reporting. All nine Claude findings were addressed and covered before round 2.
- Comprehensive review round 2 completed as a verified single-reviewer Claude pass; native Codex remained unavailable from the same quota condition. Legacy web receipt readers stay unbounded until B2 supplies partial-error UI semantics, unlocked bounded scans ignore only a torn final record, plan-body lock waits have a 250 ms deadline, oversized-record recovery is stream-specific, and all reported test gaps were covered where locally reproducible.
- Split new error types and large test blocks into canonical child files so Task B1 and the Task A contracts remain within the repository's exact-tree file budget without waivers or `mod.rs` files.
- Use `NEXTEST_TEST_THREADS=2` for final receipt production on this shared host. This preserves all 3,801 workspace tests while avoiding the 64-way local profile's process pressure and the 30-minute timeout reached by a fully serialized run.

## Outcomes & Retrospective

Task B1 now provides bounded, cancellable forward and reverse JSONL primitives; stable record offsets; stream-specific recovery diagnostics; an 8 MiB fail-closed loop-state boundary; and one descriptor-relative, no-follow plan-body store shared by reads and mutations. Existing compatibility readers remain available where B2 still needs to supply public partial-error semantics.

Two comprehensive review/fix rounds were completed before commit. The review-driven additions covered torn tails, invalid UTF-8 beyond visible prefixes, maximum caller limits, lock deadlines, legacy large receipts, supported-host special files, and recovery guidance. Final exact-diff evidence passed contract, formatting, Clippy, file budget, 3,035/3,035 scoped core tests, and 3,801/3,801 workspace tests (2 skipped).

## Context and orientation

`crates/jig/src/state/jsonl.rs` owns forward JSONL scans, data/cache locks, and unlocked snapshot fallback. `state/jsonl/reverse.rs` owns newest-first receipt lookup. `runtime/loops/state/bounded_json.rs` owns bounded loop cache decoding. `state/plans.rs` owns work-plan creation/append/lease validation; `context.rs` currently exposes an unchecked joined plan body path. The new plan-file primitive belongs under `state/` so both state mutations and the later dashboard producer can use it.

The typed cancellation sentinel is `crate::cancellation::StatusCollectionCancelled`; B1 preserves it internally so B2 can map it to the dashboard's `SourceError::Cancelled` at the adapter boundary.

## Plan of work

First strengthen JSONL scanning around a fixed retained-record buffer, stable offsets, and discard mode. Then thread cancellation through the reverse reader's data lock, cache lock, fallback snapshot, seeks, reads, and record parsing. Tighten loop cache size enforcement. Finally add the descriptor-relative plan store and migrate open/append/review path derivation, keeping event body paths byte-compatible.

Tests use synthetic readers and repository fixtures. Unix-only tests exercise symlinked ancestors/finals, FIFO/device rejection without blocking, sidecar attacks, lock serialization, and replacement races. Portable tests cover canonical IDs, exact byte ceilings, cancellation checkpoints, record offsets, and ordinary work workflows.

## Concrete steps

1. Add bounded scanner constants/results/errors and refactor the shared 16 KiB loop without changing existing scans.
2. Add cancellation-aware receipt APIs and replace blocking or whole-file reverse fallbacks.
3. Change loop cache limit to 8 MiB and update exact-boundary tests.
4. Add `state/plan_files.rs`, module exports, validator, descriptor traversal, bounded body read, and safe create/append.
5. Migrate `plans_open_prepared`, `plans_append`, test seeding, and review-prompt path derivation.
6. Run focused state/work/loop tests, formatting, Clippy, and applicable structured gates.
7. Run the requested comprehensive review twice at most, fixing every finding before commit.

## Validation and acceptance

Success means a huge logical JSONL line never grows the retained buffer beyond 1 MiB plus one read chunk; cancellation during discard and reverse traversal yields the typed sentinel; raw offsets remain stable; loop inputs over 8 MiB fail before decode; and plan paths accept exactly 1–128 ASCII alphanumeric/underscore/hyphen bytes.

Supported Unix tests must show no traversal through symlinked `.agent`, `plans`, body, or sidecar components; FIFO and device targets fail without waiting for peers; concurrent appends serialize; missing directories are safely created; and replacement produces either the verified original regular handle or a safe error. Existing valid plan open/append/close/review behavior remains unchanged.

## Idempotence and recovery

Reader additions are backward-compatible and old unbounded APIs remain callable until B2. The plan mutation cutover is isolated in `plans.rs`; reverting its call sites restores the prior implementation without migrating state. Body creation uses exclusive create, append is durable with `sync_data`, and no retry overwrites an existing body.

`.agent/state/*.jsonl` remains append-only. Test fixtures use generic `ExampleProject`/`plan_example` identities.

## Interfaces and dependencies

The new internal interfaces will expose a bounded raw JSONL scanner with record offsets, cancellable reverse receipt lookup, a canonical plan-ID validator, a safe printable relative body path, bounded cancellable body reads, and descriptor-relative body create/append operations. They depend only on existing `anyhow`, `cap-std`, `cap-fs-ext`, `fs4`, `serde_json`, and `libc` workspace dependencies.
