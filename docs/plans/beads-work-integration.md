# Beads-compatible task integration

This is the long-term delivery plan for linking Jig execution evidence to Beads-compatible tasks. It is a living design document. The architectural decision is JSONL-first: Beads-compatible JSONL is Jig's task-data contract, while `br` is an optional interoperable client during migration.

## Purpose

Jig already owns plans, immutable Git baselines, runs, gates, and evidence. A task tracker owns a different concern: what work exists and how tasks relate. The integration should connect those records without making a replaceable executable or its private database layout part of Jig's runtime authority.

The target shape is:

```text
                 Beads-compatible JSONL
                    /              \
                   /                \
        Jig task operations       beads_rust
                 |
        Jig execution evidence
```

The shared format describes tasks. Jig defines the task operations it supports. Compatibility with Beads data is intentionally smaller than behavioral compatibility with every `br` command.

## Root-cause correction

The first implementation direction treated `br 0.5.7` as Jig's live provider. Before any public linking workflow existed, it had to pin executable bytes, copy SQLite/WAL/lock families, constrain subprocess environments, classify response envelopes, journal uncertain mutations, and protect their evidence across archive and restore.

Those mechanisms were internally careful, but they solved obligations created by the boundary itself. They also pointed away from the intended future in which Jig can own a useful subset of task operations. PR #38 therefore cuts over to a pure JSONL reader and removes the premature mutation journal. Recovery machinery will return only when a concrete cross-file or external side effect requires it.

## Ownership

The Beads-compatible JSONL snapshot owns task identity and current task fields:

- exact issue ID;
- title, description, and acceptance criteria;
- status, priority, type, assignment, timestamps, and relationships;
- additional producer fields that Jig does not yet interpret.

Jig owns execution:

- `plan_id` and immutable Git baseline;
- historical task context captured when an execution is linked;
- run lifecycle and gate receipts;
- evidence and acceptance decisions;
- any future Jig-specific task-operation intent.

A plan has zero or one immutable task link. A task may have multiple execution attempts. Current task state and historical execution state can differ without corruption.

## Current milestone: JSONL foundation

The repository can opt into a task snapshot:

```toml
[work.tracker]
kind = "beads"
workspace_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV"
export = "manual"
manual_export_guidance = "Run the repository's privacy-safe export helper."
```

The current reader:

- accepts exactly one of `.beads/issues.jsonl` and legacy `.beads/beads.jsonl`;
- validates the whole bounded export before returning an issue;
- requires exact issue IDs and distinguishes missing from tombstoned records;
- rejects duplicate object keys and duplicate issue IDs;
- validates known task fields while tolerating unknown fields within the record, export, and nesting bounds;
- pins the real `.beads` directory and performs selection and stable no-follow file reads relative to that directory capability, acquiring the Unix leaf nonblocking before descriptor validation;
- invokes no process, reads no SQLite database, and writes no task data.

Doctor validates this snapshot. It deliberately cannot prove that a local `br` database has no unexported edits.

The existing `work-links.jsonl` design remains staged. It stores portable plan/task identity plus an immutable historical context snapshot whose title, description, and acceptance criteria remain distinct. It never stores an absolute checkout or database path. It is Jig-owned append-only evidence, not another task store. Projection streams the journal with fixed per-record, unique-event, and known-plan ceilings; it retains compact semantic fingerprints globally and one full canonical record only for the plan being requested. A writer admits its candidate to that same projection under the append lock before making it durable, so capacity refusal cannot poison later reads.

## Handoff with br

This repository disables automatic flush and exports through `scripts/beads-sync.py`, which also removes machine-local path data. Preserve that policy.

Initial interoperability is serialized ownership, not unrestricted simultaneous writing:

1. Finish pending `br` edits.
2. Run the repository's authorized export helper.
3. Let Jig read, and eventually atomically update, the JSONL snapshot.
4. Before returning ownership to `br`, let an ordinary command import the changed export.
5. Re-export and verify the intended fields and untouched data.

Automatic import is a real integration mechanism. It is not a promise that ordinary commands perform a three-way merge when both SQLite and JSONL changed. Divergence must be rejected or routed to an explicit merge workflow.

## Delivery sequence

### T1 — Portable links and historical snapshots

Keep the versioned `work-links.jsonl` contract, deterministic projection, strict record bounds, durable append, diagnostics, and configuration preservation. Do not introduce an external-operation journal while the tracker boundary is read-only.

Acceptance:

- records contain only portable task identity and historical context;
- exact retries are idempotent and union merges are deterministic;
- conflicting plan/task identity fails closed;
- archive and restore do not rewrite the link journal;
- older repository epochs cannot append future link authority.

### T2 — Bounded JSONL reader and Doctor

Replace the executable/SQLite adapter with the pure `beads-rust-jsonl-v1` reader.

Acceptance:

- valid current and legacy exports work on Linux and macOS without `br`;
- ambiguous filenames, unsafe paths, unstable reads, malformed records, duplicate keys or IDs, and resource-limit violations fail closed;
- unknown fields remain compatible within bounds;
- Doctor reports snapshot path, profile, issue count, and read-only capability;
- no production path can invoke `br` or inspect its database.

### T3 — Read-only linking

Expose `work start --issue beads:ID` and explicit link attachment. Resolve the exact task from JSONL, use its title as the default execution title, and store the historical description/acceptance snapshot in `work-links.jsonl`.

Acceptance:

- standalone work remains unchanged;
- repeated start reuses the unique compatible open attempt;
- ambiguity requires an explicit plan or new attempt;
- linking never changes the task export;
- CLI, MCP, status, evidence, and dashboard views agree on the portable link.

### T4 — Narrow native JSONL writer

Add only the first task mutation required by a concrete workflow. The writer edits a task snapshot, not an event journal.

Its contract must:

- preserve every field it does not own;
- maintain Beads identity and timestamp semantics;
- reject malformed, ambiguous, stale, or divergent input;
- acquire a repository-local writer lock for Jig writers;
- write a complete temporary file, sync it, atomically replace the export, and sync the directory;
- state clearly that the lock does not coordinate an independent `br` process.

Required interoperability matrix:

```text
br creates/updates
    -> authorized export
    -> Jig reads/updates JSONL
    -> ordinary br command imports
    -> authorized export again
    -> verify intended and untouched fields
```

Also test unexported database edits, disabled automatic import, conflicts, unknown fields, interruption before publication, and interruption after atomic publication.

### T5 — Evidence backlink

If backlinks are still useful, define them as an explicit Jig task operation rather than calling `br comments add`. Decide whether the interoperable JSONL profile supports the required comment shape without loss. Introduce operation recovery only for the actual atomic boundary that remains after the native writer design.

### T6 — Claim

A native claim may update assignment and status under Jig's stated policy. It need not emulate every `br update --claim` rule. Document and test Jig's admissibility, stale-snapshot handling, and handoff behavior.

### T7 — Completion

Plain `work finish` remains execution-only. A separate explicit completion request can update a linked task after required evidence and caller acceptance. Define Jig's completion policy directly; do not advertise it as identical to `br close` unless equivalence is tested.

### T8 — Setup and guidance

Full init/adopt can offer Beads-compatible JSONL integration while defaulting to no tracker. Preserve project-owned export helpers and authored instructions. Detection must not silently enroll a repository or enable mutations.

### T9 — Interoperability and dogfood

Exercise the complete supported workflow with a pinned real `br` version, the privacy-safe export helper, Linux and macOS reader coverage, conflict fixtures, and a generated consumer. Support claims name the versions and operations actually tested.

## Non-goals

This plan does not require:

- vendoring or embedding `beads_rust`;
- reading or writing its SQLite database;
- wrapping all `br` or `bv` commands;
- reproducing all Beads dependency or policy behavior;
- simultaneous independent writers;
- a general tracker plugin system;
- changing gate freshness merely because `.beads` exists;
- closing a task because checks alone pass.

## Validation policy

Every milestone uses generic fixtures and the development Jig binary. Runtime changes finish with the configured test, formatting, clippy, contract, agent-guide, and file-budget gates. JSONL writer milestones additionally run the real interoperability matrix and the repository privacy check.

The architecture is successful when Jig can eventually replace `br` for the task operations this project actually uses, while the JSONL remains portable and `br` remains a tested optional client rather than a runtime dependency.
