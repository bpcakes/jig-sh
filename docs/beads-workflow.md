# Beadroll workflow reference

This repository uses **beadroll (`bead`) as the authoritative tracker**.
`.beadroll.json` records the workspace, canonical coordination remote, and
`refs/beads/journal`. Journals live outside checkouts and sync through that ref.
Every worktree and clone uses the same tracker. Issue IDs retain the `jig-sh`
prefix. A direct request does not require creating an issue.

The former `.beads/` database and JSONL are retired. Do not run `br` or `bd`,
recreate that directory, or follow old mutation commands in historical plans.
Beads JSONL is now an interchange format, not a source of truth. Re-importing
an existing ID does not update its fields, so importing a stale export cannot
reconcile a second writer.

## Start a session

Use a compatible `bead` installation; the cutover was verified with Python
beadroll 0.1.0, `br` 0.5.7, and `bv` 0.25.0. Version 0.1.0 alone does not identify
the exact Python source. The migration archive retains the installed package
and its per-file hashes. The native beadroll 0.2.0 build was re-verified on
2026-10-11: a store recovered from the remote alone pulled all 657 events and
exported the same 275 records as the retired JSONL, with the same 43 ready and
26 blocked issues. Re-run import/export and coordination checks before
replacing that installation with a newer build.

```sh
bead sync --json
bead prime
bead info --json
```

Reads are local and do not fetch. Sync first when current remote state matters.
A fresh clone discovers the tracker through the tracked configuration; use
`bead sync`, not `bead init` or `bead import`. Before this configuration reaches
an older checkout, update that checkout's instructions and configuration before
using its tracker. Never continue writing its old `.beads` database.

## Select and claim work

```sh
python3 scripts/bead-triage.py
bead ready --json
bead show <id> --json
bead claim <id> --json
```

The helper synchronizes beadroll, exports into a temporary directory, rejects
machine-local `source_repo_path` metadata, and runs `bv --robot-triage` there.
It removes command fields from the result because `bv` still generates `br`/`bd`
commands. Use its graph rankings to select a candidate, then verify readiness
and claim through `bead`. Do not execute commands copied from raw `bv` output.
Never run bare `bv`, which launches an interactive TUI.

If `bv` is unavailable, sync and use `bead ready --json` directly. For other
robot reports, export into a disposable directory and pass its file explicitly
with `bv --db FILE --robot-plan` or `--robot-insights`. Those raw reports can
contain obsolete commands. Do not use `bv --robot-diff` against code history as
a view of current journal history; use `bead log <id>`.

Claims belong to the full agent thread and last until release, close, or takeover.
A later session is a different owner. Use `bead takeover <id> --reason "..."`
only after verifying why the prior owner should be replaced. `ALREADY_CLAIMED`
means choose other work; an offline claim is provisional, never exclusive.

## Update work

```sh
bead create "..." --type task --priority 2 --json
bead comments add <id> "Progress and decisions" --json
bead update <id> --notes "..." --json
bead dep add <issue> <depends-on> --json
bead close <id> --reason "Completed and validated" --json
bead release <id> --json
```

Priorities are 0 through 4. `bead update --status in_progress` claims the issue;
it is not a status-only edit. Closing ends the owner's claim. Confirm the result,
including any `superseded` or `unpublished` field, before reporting completion.

Writes normally publish immediately. This is authorized as part of requested
tracker mutations; it does not authorize code commits or pushes. With `--offline`
or a failed publication, local changes remain unpublished until synchronization.
A successful local write alone is not proof that another machine can see it.

Before handoff:

```sh
bead sync --json
bead info --json
```

Resolve `unpublished`, `pending_ownership`, integrity errors, and waiting events
before calling synchronization complete. An unresolved ownership attempt must
be settled by sync before another ownership command. Report remote refusal or
unavailability rather than falling back to the retired tracker.

## Publication, privacy, and automation

Anyone who can read the coordination repository can read the journal. It contains
issue text, opaque machine identity, actor/thread identity, and timestamps.
This repository sets `"where": false` to omit branch and commit locations from
new events. Imported source paths are stripped by beadroll. Issue text and other
metadata must still satisfy the repository's open-source fixture hygiene rules;
there is no post-publication scrubber.

Workers need write access to beadroll's external state directory and Git access
to the coordination remote. A checkout-only sandbox is insufficient. Configure
those capabilities explicitly before scheduling tracker mutations. Tracker-only
tasks leave code branches clean and do not need a `.beads` commit.

## Cutover and recovery

The 2026-10-11 cutover preserved all 275 records, 381 dependency edges, and
44 comments. The initial publication at
`7376aa5fab13dd6973dd1745ac3e8824fe92c9cd` contains 657 journal events.
A fresh store recovered identical records from the remote alone, with the
same 43 ready and 26 blocked issues. The original database has 298 audit
events not represented in JSONL; preserve its migration archive. No issue was
`in_progress` at cutover, so there were no active claims to reconstruct.

The operator's local migration archive under
`~/.local/state/jig-sh/tracker-migrations/` holds the original database family,
a consistent SQLite snapshot, final export, installed Python implementation,
and validation evidence. These backups are outside the repository. Git history
also retains the old tracked export, but cannot recover uncommitted database
history or new beadroll journal events.

Before an intentional rollback, stop all bead writers and export the latest
tracker into a scratch directory. Initialize a fresh `br` database there, import
that export, and compare records and ready/blocked sets before switching every
writer and instruction back together. Do not restore only the pre-cutover
snapshot after new bead mutations. Claims and beadroll journal history have no
Beads JSONL equivalent; preserve the bead store and published ref separately.
Do not delete or reset `refs/beads/journal` as part of routine recovery.
