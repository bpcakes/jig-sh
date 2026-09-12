# 09 — Unify work-check completion and recovery summaries

## Task identity

- Local task: 09.
- Beads issue: `jig-sh-9wcn.9`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.3`, `jig-sh-9wcn.7`
- Unblocks: 10, 13, 15.
- Status: planned; implementation has not started.

## Context and outcome

Agents currently have several overlapping evidence/status views. A concise result showing
what passed, what was reused, and what still blocks finish can remove redundant diagnostic
calls.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `crates/jig/src/runtime/work/checks/targets.rs`
- `crates/jig/src/runtime/work/gates/report.rs`
- `crates/jig/src/runtime/work/gates/recovery.rs`
- `crates/jig/src/cli/output/work.rs`
- `crates/jig/src/runtime/work.rs`

## Scope

- Add one shared compact completion projection to work-check and work inspection output.
- Reuse existing gate reports and recovery commands as the source of truth.
- Show executed, reused, failed, stale, unknown, and not-applicable states distinctly.
- Show unresolved review or external requirements without automatically executing them.
- Return a finish-ready result only when current required policy is satisfied.
- Keep full evidence and receipts available for detailed diagnosis.
- Avoid automatic work-plan creation or closure.
- Document the three-command workflow using the improved output.

## Implementation sequence

Use task 07's shared projection type and explicit selection convention.
Add the corresponding work CLI projection option here; info owns it in task 07.
Compact defaults apply within agent-v1 only.
Omitted selection preserves standard result and descriptor shapes.
Verify standard parity separately from the compact work summary.

1. Inventory overlapping fields in work check, gates, evidence, and status.
2. Define a typed compact projection over the current gate evaluator.
3. Reuse observation results safely within one request instead of independently rescanning
   for each rendering.
4. Retain final closure revalidation and checkout lease behavior.
5. Show bounded reasons and concrete next commands for unresolved requirements.
6. Avoid recommending execution when observation itself is unavailable.
7. Update CLI human output and MCP response shapes consistently.
8. Keep old command names and detailed modes available.

## Acceptance criteria

- A successful check explains whether targets executed or reused evidence.
- Remaining review gates are visible even if check targets passed.
- Unknown freshness is not reported as stale or as success.
- Finish readiness is clearly a current observation, not a durable authorization token.
- work finish still revalidates required evidence under its existing lease.
- Full evidence remains reachable from the compact view.
- Existing automation has a compatible response transition.
- The normal workflow does not require gates plus evidence plus receipts after every check.

## Verification

- Cover all-pass, mixed reuse, failed checks, review-blocked, stale, and unknown cases.
- Exercise changes between summary and finish.
- Check that recovery preserves target dependency closure.
- Run existing work finish/freshness regressions and backend verification.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- A cached summary cannot authorize later closure after source drift.
- Read-only annotations must reflect any existing reconciliation side effects.
- This task does not add work finish --check or a new completion command.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Completion and handoff

Attach implementation revision, relevant result artifacts, and acceptance disposition to the bead.
Close only after the task's stated outcome is delivered; a plan or expected test result is not proof.
Leave an actionable restart checkpoint if interrupted.
Follow the [shared Beads database-selection rule](README.md#beads-database-selection):
verify `br info --json` before mutations or export, then run
`python3 scripts/beads-sync.py` from the verified checkout after mutations.
See README.md in this plan directory for shared rollout policy and the dependency matrix.

Revision note (2026-09-12, task 06 handoff review): replaced the unconditional
main-checkout database instruction with verified checkout-local discovery. The sync
helper uses that checkout's `br` discovery; task scope and status are unchanged.
