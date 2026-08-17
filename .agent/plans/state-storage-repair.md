# Repair and bound local Jig state

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while implementation proceeds. This document follows `.agent/PLANS.md`.

## Purpose / Big Picture

Jig repositories currently keep sessions, plans, receipts, and decisions as Git-tracked JSON Lines files under `.agent/state`. A historical session-summary bug recursively copied earlier session summaries into later records. Jig no longer creates those recursive records, but one audited repository still has an 861 MB `sessions.jsonl` containing only about 160 real events, and normal readers still buffer that entire file. Receipts also repeat unbounded changed-path arrays and successful command previews, while the existing archive command merely moves uncompressed bytes into another non-ignored directory.

After this work, a developer can diagnose state size without loading it all into memory, compact legacy session summaries safely and idempotently, recover an exact pre-compaction backup, and archive or explicitly export old receipts as compressed local data. Runtime readers will collapse duplicate session event IDs created by line-union Git merges and reject conflicting envelopes. New receipts will exclude `.agent/**`, bound changed-path metadata, and retain smaller output previews. All behavior remains local and offline.

The observable entrypoints are:

    scripts/jig state diagnose
    scripts/jig state diagnose --deep --json
    scripts/jig state compact sessions --dry-run
    scripts/jig state compact sessions
    scripts/jig state restore --backup <path>
    scripts/jig state export receipts --before <date> --output <path>
    scripts/jig state archive --before <date> --dry-run
    scripts/jig state archive --before <date>

## Progress

- [x] (2026-07-28 19:18Z) Audited current state schemas, readers, archive behavior, merge attributes, and an affected repository's pathological session graph.
- [x] (2026-07-28 19:18Z) Recorded the implementation and compatibility contract in this ExecPlan.
- [x] (2026-07-28 21:04Z) Added bounded streaming JSONL scan/rewrite primitives, stable-snapshot fallback, stale-inode reopening, atomic validation, and focused cancellation/failure tests.
- [x] (2026-07-28 21:04Z) Added canonical session event-ID deduplication, divergent-record rejection, deterministic ordering, and a real union-merge regression test.
- [x] (2026-07-28 21:04Z) Added strictly read-only fast/deep `state diagnose`, including maintenance-cache and actionable receipt diagnostics.
- [x] (2026-07-28 21:04Z) Added validated raw-span session compaction, exact gzip backup manifests, dry run, idempotence, and reversible restore.
- [x] (2026-07-28 21:04Z) Bounded future receipt payloads, excluded `.agent/**`, and made work-check batches the single Git-metadata owner even on fail-fast failure.
- [x] (2026-07-28 21:04Z) Replaced active receipt retention with verified compressed local archives, explicit non-mutating export, and an exact pre-archive recovery backup.
- [x] (2026-07-28 21:21Z) Converted summary, gate, list, and UI consumers to streaming folds, bounded reverse scans, canonical maps, or one-scan request-scoped gate indexes.
- [x] (2026-07-28 21:04Z) Updated CLI help, human output, public documentation, compatibility notes, recovery warnings, and local-vs-Git-history policy.
- [x] (2026-07-28 21:21Z) Built the development binary; passed focused and full tests, strict Clippy, formatting, contract and configured test gates; dogfooded read-only diagnostics and compaction against an affected repository; and completed the requirement-by-requirement durability audit.
- [x] (2026-07-29 19:32Z) Closed post-review findings by recursively validating every embedded summary before compaction and by changing successful setup output to the universally available `jig status`; added focused regressions for third-level divergence and repositories without development apps or a `scripts/jig` launcher.
- [x] (2026-07-29 19:32Z) Re-ran focused compaction, maintenance, and setup tests plus formatting, strict Clippy, the development build, and the contract gate. The configured full test gate was attempted but is currently blocked by unrelated macOS listener-owner tests in `jig-dev-proxy`, which also fail in isolation because the spawned process exits before its process group can be read.

## Surprises & Discoveries

- Observation: The affected repository's 861,231,519-byte session file has only 161 unique top-level events. Normalizing direct recent-session references to `summary: null` produces 256,795 bytes, a 99.9702% reduction.
  Evidence: A read-only forensic traversal found 2,063,197 event occurrences, all nested envelopes matching one of the 161 canonical top-level IDs.

- Observation: The current session deserializer deliberately throws away every root summary, so it is safe for queries but unsafe for migration.
  Evidence: `crates/jig/src/state/records.rs` deserializes `SessionEventHeader` and constructs start events with `Value::Null`; its comment says not to use the lossy model to rewrite history.

- Observation: Existing `state archive` is not a storage policy. It writes uncompressed files under `.agent/state/archive`, which is neither ignored nor read by normal queries, and protects latest evidence for every historical plan.
  Evidence: `crates/jig/src/state/receipts.rs::receipts_archive`, `templates/project/.gitignore.jinja`, and `crates/jig/src/state/timeline.rs`.

- Observation: A raw-span, one-record-buffer compactor validates the affected repository's live 861,237,444-byte session stream without loading the history graph.
  Evidence: The final development binary's read-only affected-repository dry run found 164 logical records, 64 records with recursive summaries, projected 262,720 bytes, and 860,974,724 reclaimable bytes. It completed in 93.50 seconds with 139,280,384 bytes maximum RSS. The source's size, mtime, inode, and SHA-256 remained unchanged before and after dry-run and deep diagnosis.

- Observation: A selected-record receipt archive alone cannot reconstruct exact physical order when protected and expired records were interleaved.
  Evidence: The archive path now writes a complete manifested pre-rewrite receipt backup and a regression restores the original byte stream exactly after such an interleaved rewrite.

- Observation: Compressed recovery validation must not materialize a second uncompressed copy beside a very large source, and a valid final JSON receipt without its newline is still an unsafe archive boundary.
  Evidence: Backup/archive verification now hashes and validates decompressed content as a bounded stream, restore refuses output beyond the manifest size, and archive/export reject unterminated receipt tails before publishing artifacts or rewriting state.

- Observation: An older Jig process that queued a write on a pre-opened inode cannot be coordinated after an atomic rename by a newer runtime alone.
  Evidence: Current readers and writers coordinate through a repository cache lock and reopen the canonical path after acquisition; mutating command output and documentation explicitly require stopping processes launched with pre-cache-lock runtimes before compact, archive, or restore.

- Observation: The worktree already contains unrelated bootstrap snapshot restructuring and an active session for that work.
  Evidence: `git status --short` on 2026-07-28 showed changes only in bootstrap snapshot files plus the active plan/state records. This plan must not edit or commit those bootstrap files.

- Observation: Comparing only an embedded summary's shallow projection is insufficient for fail-closed compaction because that projection deliberately omits each referenced event's own summary.
  Evidence: A regression that changes only an event summary three embedded levels below the direct reference passed the old check. Validation now descends through every embedded reference and only memoizes an event-ID/raw-summary-digest pair after all descendants validate; cache saturation causes safe revalidation.

- Observation: A successful full-harness setup cannot universally recommend either a development app or the repository launcher.
  Evidence: A full harness may configure no apps, while the supported minimal footprint may omit `scripts/jig`. The installed `jig status` command is available in both shapes.

## Decision Log

- Decision: Keep JSONL as canonical tracked state during this repair; do not introduce canonical SQLite.
  Rationale: SQLite would not solve Git merging or history bloat. A disposable local index can be considered after streaming and record-bound fixes are measured.
  Date/Author: 2026-07-28 / Codex.

- Decision: Compact only nested session summaries and preserve every root write-time summary plus its ordered direct references.
  Rationale: The affected repository's nested graph is exactly redundant, while the root snapshot remains part of the documented durable format.
  Date/Author: 2026-07-28 / Codex.

- Decision: Duplicate session IDs with identical event envelopes collapse at read time; the same ID with a different session ID, event kind, timestamp, or outcome is a hard error.
  Rationale: Git’s line-union merge can retain compact and legacy variants of one logical event. Envelope equality preserves runtime semantics while conflicting identity must never be guessed.
  Date/Author: 2026-07-28 / Codex.

- Decision: Compaction uses stricter duplicate validation than ordinary reads: duplicate roots must have the same normalized complete canonical projection, including unknown fields and root-summary shape.
  Rationale: Read-time envelope equality preserves existing query semantics, but a destructive migration must fail closed instead of discarding forward-compatible data that differs outside the known envelope.
  Date/Author: 2026-07-28 / Codex.

- Decision: Maintenance backups and automatic receipt archives live under ignored `.agent/.cache/state-*`; only explicit exports use a caller-selected destination.
  Rationale: Moving bytes to `.agent/state/archive` does not reduce checkout or Git storage and creates an untracked-or-tracked duplicate with no reader.
  Date/Author: 2026-07-28 / Codex.

- Decision: Use gzip streams implemented in Rust for exact backups and exports.
  Rationale: Recursive JSON and repeated receipt metadata compress well, gzip is portable, and it avoids requiring an external executable.
  Date/Author: 2026-07-28 / Codex.

- Decision: Retention protects gate-evidence closure only for currently open plans; all records newer than the cutoff remain active.
  Rationale: Closed plans no longer need evidence for `work finish`. Preserving every historical plan/tool forever prevents bounded active state.
  Date/Author: 2026-07-28 / Codex.

- Decision: Every receipt archive rewrite first creates a complete gzip backup plus manifest, while the selected-record archive remains a compact cold-data artifact.
  Rationale: Selected records do not encode their former positions among retained records. A full preimage is the simplest exact, validated recovery path and lets the existing `state restore` command recover either sessions or receipts.
  Date/Author: 2026-07-28 / Codex.

## Outcomes & Retrospective

Implementation and verification are complete. The final affected-repository dry run proves the migration is practical and bounded by one physical record rather than total history size: 861,237,444 bytes project to 262,720 bytes without mutating the repository, in 93.50 seconds and 139,280,384 bytes maximum RSS. Deep diagnosis reports the same 860,974,724-byte opportunity plus receipt-storage categories and completed in 17.96 seconds.

Local repair, diagnostics, exact recovery, receipt bounding, compressed archive/export, canonical Git-union reads, and one-scan gate evaluation no longer depend on a hosted service. The full direct suite passed with 1,238 library tests, 12 integration tests, zero failures, and two deliberate ignores; formatting, strict all-target/all-feature Clippy, `jig.contract_check`, and `jig.test` also passed. The remaining operational limitation is explicit rather than hidden: stop processes launched with older pre-cache-lock Jig runtimes before a mutating state rewrite, and copy any recovery artifact that must be durable beyond the ignored local maintenance-cache lifecycle.

The post-review pass closed two additional correctness edges without changing the compaction metric contract: direct recursive references are still counted as before, while canonical validation now covers arbitrarily deep embedded summaries with bounded memoization. Successful setup now recommends `jig status`, which works for no-app and minimal harness repositories. All focused checks, formatting, strict Clippy, the development build, and the contract gate pass on the final sources. A fresh configured `jig.test` run is not green because three unrelated `jig-dev-proxy` macOS listener-owner tests fail while reading a short-lived spawned process group; the representative test reproduces in isolation and no proxy code was changed in this review fix.

## Context and Orientation

`crates/jig/src/state.rs` is the internal facade. `crates/jig/src/state/records.rs` owns durable serde schemas. `crates/jig/src/state/jsonl.rs` owns append, locks, full-file reads, atomic rewrites, and reverse receipt scans. `crates/jig/src/state/sessions.rs`, `plans.rs`, and `receipts.rs` implement state behavior. `crates/jig/src/state/timeline.rs` adapts records for the local UI.

CLI parsing lives in `crates/jig/src/cli/state.rs`; transport-neutral command DTOs live in `crates/jig/src/command/state.rs`; conversion is in `crates/jig/src/cli/command_conversion.rs`; dispatch is in `crates/jig/src/runtime.rs`; and terminal rendering is in `crates/jig/src/cli/output.rs`.

A JSON Lines file stores one JSON value per physical line. A canonical event is the unique top-level session event bearing an event ID. A shallow session reference retains the referenced event’s envelope but sets its nested `summary` to null. An envelope consists of event ID, session ID, event kind, timestamp, and outcome. Gate-evidence closure means the latest receipt used for an open plan gate plus the work-check receipts and direct tool receipts linked to that evidence.

Current `read_jsonl` locks a file, reads every byte into a `Vec<u8>`, and then deserializes each line. Current `write_jsonl_locked` writes a same-directory temporary file and renames it over the source, but does not preserve every unknown raw field, fsync the parent directory, or provide recovery metadata. Current receipt reverse scanning in `jsonl.rs` demonstrates that bounded reads are already feasible.

The repository used for the forensic baseline must not be mutated during development. Tests must create temporary fixture repositories with the same recursive shape at a smaller default size plus an ignored stress test capable of generating a record above 100 MB.

## Plan of Work

First, extend `crates/jig/src/state/jsonl.rs` with raw, locked streaming primitives. A scanner will visit one nonblank record at a time, report byte and line statistics, validate line termination, and check cancellation between chunks. A rewriter will use the existing exclusive locks, preserve untouched raw lines, write transformed records to a same-directory temporary file, preserve file permissions, sync output, validate it, replace the source once, and fsync the parent directory. Mutation must fail closed when stable locking is unavailable. Existing generic readers can then use the scanner rather than an all-file byte buffer on supported filesystems.

Second, add session-specific canonical read helpers in `state/sessions.rs` or a focused new `state/session_stream.rs`. They will key events by ID, compare envelopes, collapse identical duplicates, reject conflicts, and order query results by timestamp and ID rather than merged line position. Every current session consumer in `sessions.rs` and `timeline.rs` must use the canonical helper. Add a temporary Git repository test that merges a compacted branch with a stale append branch under `merge=union`, then prove logical counts remain stable and conflicting envelopes fail.

Third, add `crates/jig/src/state/diagnostics.rs` and CLI plumbing for `state diagnose`. The fast report uses metadata and bounded newline scanning. Deep mode parses stream-specific fields to report recursive session records, projected compacted bytes, receipt payload byte categories, oversized records, malformed records, archive bytes, Git tracking, ignore, and merge-attribute facts. It is strictly read-only and creates no directories, locks, receipts, or cache files in an uninitialized repository.

Fourth, add `crates/jig/src/state/compaction.rs`. Define a lossless migration-only session model separate from `SessionEvent`. In a validation pass, index canonical root envelopes and normalized root summaries, then verify every recursively embedded event resolves to the same canonical envelope and normalized summary. In a rewrite pass, preserve root fields and replace only nested summaries under direct recent-session references. Dry run writes only to an in-memory counter or disposable temporary outside the state directory and leaves no artifact. Apply mode first creates a gzip backup plus a JSON manifest under `.agent/.cache/state-backups/<id>/`, then writes and validates the compacted stream before replacing the source. Add `state restore --backup` to verify the manifest checksum, decompress to a temp file, validate, and restore under the same locks. A second compaction must be a byte-for-byte no-op.

Fifth, change `crates/jig/src/git_receipts.rs` and `state/receipts.rs`. Changed-path and diff-stat Git commands must exclude `.agent/**`. Persist at most 100 deterministic changed-path entries with total count, truncation flag, and a digest over the full ordered list. Keep failed stdout/stderr previews at the existing bounded size; reduce successful previews to a small diagnostic prefix. Batch `work check` child receipts should not repeat change-set metadata when the batch receipt owns the fingerprint and diff facts. Schema additions must use serde defaults so old records remain readable.

Sixth, refactor receipt retention. `state archive` will select records older than the cutoff, protect only evidence needed by currently open plans, write selected raw records to an exact gzip file under `.agent/.cache/state-archives`, and only then rewrite the active file. Add `state export receipts --before ... --output ...` as a non-mutating exact gzip export. Keep old `.agent/state/archive` files untouched and diagnose them as legacy cold storage. Raw-line export preserves unknown fields. JSON and human output must distinguish exported, archived, retained, protected, compressed bytes, checksum, and destination.

Finally, replace avoidable whole-stream consumers with folds or reverse scans. `build_summary` needs only recent sessions, receipts, and decisions plus open plans. `state_summary` needs counters and bounded recent windows. `receipts_list` needs a reverse limited scan. Gate evaluation should build one request-scoped receipt index rather than rereading the file per gate. UI receipt reads are already bounded, while session, plan, and decision scans can use streaming folds.

## Concrete Steps

Work from `/Users/aa/Documents/jig-sh`.

1. Add the gzip dependency to the workspace and `jig-sh` crate, then implement and test streaming storage helpers.

       cargo test -p jig-sh state::tests --no-fail-fast

2. Implement canonical session reads and merge tests. Run the focused session and Git tests.

       cargo test -p jig-sh session --no-fail-fast
       cargo test -p jig-sh union --no-fail-fast

3. Add CLI DTOs, conversion, dispatch, human output, diagnostics, compaction, backup, and restore. Exercise help and JSON output using the development binary.

       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig state --help
       JIG_DEV_BIN=target/debug/jig scripts/jig state diagnose --json

4. Add receipt bounds, local archive, and explicit export. Validate round trips with temporary repositories.

       cargo test -p jig-sh receipt --no-fail-fast
       cargo test -p jig-sh archive --no-fail-fast
       cargo test -p jig-sh export --no-fail-fast

5. Update documentation and run formatter, strict Clippy, focused tests, full crate tests, and repository gates. Because unrelated bootstrap snapshot changes exist, do not refresh or stage embedded snapshots as part of this plan.

       cargo fmt --all -- --check
       cargo clippy -p jig-sh --all-targets --all-features -- -D warnings
       cargo test -p jig-sh
       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract --no-receipt
       JIG_DEV_BIN=target/debug/jig scripts/jig check test --no-receipt

Expected diagnostics on a recursive fixture include a nonzero `recursive_session_records`, a projected size much smaller than the source, and a `compact sessions --dry-run` recommendation. Expected apply output reports the exact backup path, unchanged logical event count, and reduced byte count. Restore returns the original checksum. Running compact a second time reports zero changed records.

## Validation and Acceptance

The implementation is accepted only when all six requested outcomes have direct evidence.

Streaming is proven by a test that scans multiple large records while retaining at most one record buffer and by cancellation during scan. Mutation tests prove malformed input, lock failure, temporary-write failure, validation failure, replacement failure, and parent-sync failure never silently lose the source.

Diagnosis is proven by exact byte, record, maximum-line, recursive-record, reclaimable-byte, payload-category, archive, tracking, ignore, and merge-attribute assertions. Running diagnose in an uninitialized fixture creates nothing.

Compaction is proven by a recursive graph fixture. Before and after have identical canonical root IDs, order, envelopes, root summary fields, and ordered direct references. Orphan nested IDs, divergent envelopes, divergent normalized summaries, divergent duplicate root IDs, malformed middle records, and torn tails refuse mutation; semantically identical duplicate roots collapse deterministically. Dry run is non-mutating, apply creates a verified backup, restore recovers the original checksum, and repeat apply is a no-op.

Merge behavior is proven in a real temporary Git repository using `merge=union`: a stale branch and compacted branch may produce duplicate physical lines, but canonical reads expose one logical event. A duplicate ID with a different envelope produces a clear error naming the ID.

Receipt bounds are proven by a fixture with more than 100 paths, `.agent` changes, successful and failed output, and nested `work check` receipts. The stored record contains no `.agent` path, preserves total count and digest, bounds the preview, and retains enough fingerprint and relationship data for gates.

Retention is proven by open and closed plans on both sides of a cutoff. New records remain active; old open-plan evidence closure remains active; old closed-plan and unrelated receipts enter the gzip archive exactly once; explicit export does not mutate active state; archive and restore/export checksums validate; and no new file appears under `.agent/state/archive`.

The full `jig-sh` test suite, strict Clippy, formatting, contract check, and configured test gate must pass with `JIG_DEV_BIN=target/debug/jig`. The final diff must not contain unrelated bootstrap snapshot changes attributable to this plan.

## Idempotence and Recovery

Diagnosis and export never mutate canonical state. Session compaction is deterministic; rerunning it after success reports no changes. Archive selection is based on receipt IDs and cutoff, so an immediate repeat archives nothing. Backup and archive filenames contain unique IDs, but a manifest checksum prevents treating partial output as valid.

Every mutating operation writes and syncs its complete recovery artifact before replacing active state. It validates the new stream before publication, preserves original permissions, performs one replacement, and syncs the parent directory. If publication fails, the original path remains authoritative. If a failure is reported after publication, `state restore --backup <manifest-or-directory>` verifies and restores the exact original under exclusive locks.

The maintenance commands must warn that working-tree compaction does not remove reachable Git blobs. They must never invoke Git history rewriting. Migration of the audited repository must happen only after this code passes synthetic tests and its active branches/worktrees are coordinated.

## Artifacts and Notes

Anonymized forensic baseline:

    sessions.jsonl bytes:       861,237,444
    physical records:                    164
    recursive records:                    64
    worst record bytes:          118,752,584
    recursive references:                   93
    compacted projection:             262,720
    projected saving:                   99.9702%

Receipt baseline:

    receipts.jsonl bytes:        about 77.4 MB
    changed-path JSON:           about 48.1 MB
    stdout/stderr previews:      about 23.0 MB
    args plus evidence:          below 0.2 MB

Plan revision note, 2026-07-28: Initial plan created from the completed local-state and anonymized forensic audit. It intentionally avoids the unrelated bootstrap snapshot restructuring already present in the worktree.

## Interfaces and Dependencies

`crates/jig/src/state/jsonl.rs` will expose crate-internal raw scan and rewrite helpers plus statistics structs. Callers must not bypass their locking and durability behavior.

`crates/jig/src/state/diagnostics.rs` will expose:

    pub(crate) fn state_diagnose(
        ctx: &RepoContext,
        request: StateDiagnoseRequest,
    ) -> anyhow::Result<serde_json::Value>;

`crates/jig/src/state/compaction.rs` will expose session compact and restore functions using command DTOs from `crates/jig/src/command/state.rs`.

`crates/jig/src/state/receipts.rs` will expose archive and export functions. Exact gzip output uses `flate2` with a deterministic gzip header whose timestamp is zero, and SHA-256 uses the repository’s existing or newly added Rust digest dependency. If no digest crate exists, add `sha2` at workspace scope rather than shelling out.

All new persisted receipt fields are optional/defaulted for backward compatibility. Maintenance commands remain runtime-owned CLI features and are not added to `.agent/jig-contract.json` or MCP tool definitions.
