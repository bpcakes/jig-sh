# 04 — Move specialized guidance beside its owning code

## Task identity

- Local task: 04.
- Beads issue: `jig-sh-9wcn.4`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.1`, `jig-sh-9wcn.2`
- Unblocks: 05, 14.
- Status: implemented and verified; see [acceptance evidence](04-implementation.md).

## Context and outcome

Root and crate entry guidance should deliver relevant project knowledge quickly. Installer
internals, the Beads command manual, and detailed process/vault invariants currently
increase unrelated-task reading.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `AGENTS.md`
- `agent-map.md`
- `crates/jig/AGENTS.md`
- `templates/project/AGENTS.md.jinja`
- `crates/jig/src/bootstrap/`
- `crates/jig/src/runtime/`
- `docs/adoption.md`

## Scope

- Move the detailed Beads reference to repository documentation and retain essential
  workflow rules at root.
- Retain robot-only bv, claim verification, sync-helper use, and Git policy in the short
  entry guidance.
- Move frontend installer internals from generated root guidance into relevant maintained
  reference documentation.
- Route process, vault, and bootstrap invariants from the crate guide to their owning module
  guides or docs.
- Preserve every substantive invariant and record its new destination.
- Make the generated agent map a discovery aid rather than a mandatory read before all
  backend work.
- Retain app-specific commands, migration authorities, and unusual setup facts where users
  need them.
- Keep full and minimal adoption footprints unchanged.

## Implementation sequence

1. Build a sentence-level inventory of guidance to retain, move, or retire as obsolete.
2. Choose destination files based on current owning modules and nearest guide scope.
3. Move specialized instructions with explicit links from affected entry guides.
4. Keep uncommon but critical rules reachable before touching the relevant code.
5. Update map generation wording without requiring a map for naturally scoped discovery.
6. Render frontend guidance to verify application commands remain understandable.
7. Check moved references against their actual targets and existing guide validator rules.
8. Provide the movement inventory in the task's final evidence.

## Acceptance criteria

- Root entry guidance no longer contains a Beads command encyclopedia.
- Required sync/privacy/Git rules remain visible without loading the full reference.
- Generated frontend guidance omits installer implementation trivia.
- Critical process, vault, and bootstrap invariants have discoverable owning destinations.
- No guidance is lost merely because it was lengthy.
- The agent map remains useful and compatible with existing generated maps.
- Local user-authored blocks are preserved during updates.
- Resulting guidance works for non-Codex clients as well as Astra.

## Verification

- Validate every new or changed repository-local Markdown link.
- Run existing map/guide checks while their current contracts remain in effect.
- Render frontend and no-frontend variants and review guidance for missing commands.
- Use task 02 fixtures to verify relevant guide discovery paths.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- A moved instruction that becomes undiscoverable is a regression even if byte counts
  improve.
- New nested guides must not accidentally apply vault rules to unrelated runtime modules.
- Do not duplicate the same long rule in both its old and new location.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Completion and handoff

Attach implementation revision, relevant result artifacts, and acceptance disposition to the bead.
Close only after the task's stated outcome is delivered; a plan or expected test result is not proof.
Leave an actionable restart checkpoint if interrupted.
Run `python3 scripts/beads-sync.py` after Beads mutations, using the canonical main-checkout database.
See README.md in this plan directory for shared rollout policy and the dependency matrix.
