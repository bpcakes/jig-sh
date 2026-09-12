# 01 — Shorten default guidance and correct stale workflow wording

## Task identity

- Local task: 01.
- Beads issue: `jig-sh-9wcn.1`.
- Priority: P1.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: None; this is the first implementation task.
- Unblocks: 02, 03, 04, 06, 07.
- Status: implemented; `jig-sh-9wcn.1` is closed. Review follow-up is tracked in the local planning/reconciliation record below.

## Context and outcome

Agents currently encounter overlapping checklists and frontend gate descriptions that no
longer match the generated native profile. A small editorial change provides the first
usable result while leaving verification enforcement intact.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `AGENTS.md`
- `templates/project/AGENTS.md.jinja`
- `crates/jig/src/bootstrap/embedded_template_snapshots/AGENTS.md.jinja`
- `templates/project/.jig.toml.jinja`
- `crates/jig/src/bootstrap/tests/committed.rs`

## Scope

- Edit the root managed block and the corresponding source template together.
- Teach work start, work check, and work finish as the normal structured-work sequence.
- Make evidence, gates, receipts, and status conditional diagnostics instead of a mandatory
  sequence.
- Explain that work check reuses current passing target evidence.
- Replace the obsolete four-atomic-gates-per-app statement with the configured-profile
  model.
- Mention info targets and check --explain without printing a full CLI catalog.
- Leave the current required backend-test rule unchanged until task 03 updates verification
  policy deliberately.
- Preserve local privacy rules, JIG_DEV_BIN instructions, managed markers, and every
  unmanaged section.

## Implementation sequence

1. Read the root and crate guidance and inspect the active template conditions.
2. Record the baseline commit and retain it for task 02's control condition.
3. Draft a concise managed block using only commands supported by current help.
4. Keep compatibility and migration rules; remove redundant introductions and duplicated
   command lists.
5. Change frontend gate wording only where contract-version branching warrants it.
6. Render representative Rust-only, Go, and frontend variants through existing fixture
   helpers.
7. Refresh the embedded template snapshot through the repository-provided mechanism.
8. Review the root/template diff for unmanaged content changes and stale examples.

## Acceptance criteria

- The first deliverable is a bounded documentation/template change that can ship
  independently.
- Root and generated guidance describe start → check → finish accurately.
- Optional diagnostics are discoverable but no longer phrased as required steps.
- Current passing evidence reuse is explained without promising reuse after arbitrary
  changes.
- Frontend guidance reflects native profile gates while any retained legacy rendering
  remains accurate.
- All compatibility, privacy, and mandatory verification requirements remain effective.
- Full/minimal footprint selection and generated runtime behavior are unchanged.
- Changes include root guidance, template source, and matching embedded snapshot.

## Verification

- Use existing generated-guide tests for supported variant rendering.
- Check managed-block preservation using existing adoption fixtures.
- Run template snapshot parity and relevant contract checks through the dev binary.
- Inspect the diff; do not create tests whose only assertion is arbitrary prose wording.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- This task must not remove required checks indirectly through nicer wording.
- Whitespace cleanup must not cross managed-block ownership boundaries.
- Treat the old baseline as the evaluation control even after this task ships.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Completion and handoff

Attach the implementation identity, result artifacts, and acceptance disposition to the bead.
Use the commit when available, or the exact Jig run/receipt and recorded worktree fingerprint
for uncommitted work; record the eventual commit without implying that a receipt is a commit.
Close only after the task's stated outcome is delivered; a plan or expected test result is not proof.
Leave an actionable restart checkpoint if interrupted.
Run `python3 scripts/beads-sync.py` after Beads mutations, using the canonical main-checkout database.
See README.md in this plan directory for shared rollout policy and the dependency matrix.

## Delivered state and evidence

Task 01's implementation work record is
`.agent/plans/plan_01M28819Z3PAE8YA2MBBBPY57Z.md` (closed).
Run `run_01M289WM1CWE50CWPBRV7YAAFC` and receipt
`receipt_01M28A9NCY7YHW0VJH0M96MG1M` record the initial verified worktree:
all five gates passed, including 4,082 tests with 3 skipped.
Review follow-up results belong to the local planning/reconciliation record
`.agent/plans/plan_01M28626D98WHJJFH0432ZBEKH.md`.
These records cover their exact source states, not later edits.
Task 02 is next; use `br ready --parent jig-sh-9wcn --json` before claiming it.
