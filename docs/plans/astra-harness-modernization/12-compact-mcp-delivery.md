# 12 — Deliver the selected compact MCP approach compatibly

## Task identity

- Local task: 12.
- Beads issue: `jig-sh-9wcn.12`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.11`
- Unblocks: 15.
- Status: planned; implementation has not started.

## Context and outcome

A measured prototype should become a supported interface only if its benefit justifies the
lifecycle and migration cost. This task owns that production boundary, including a
documented no-change result if rejected.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `crates/jig/src/mcp.rs`
- `crates/jig/src/tool_defs/repository.rs`
- `crates/jig/src/runtime/mcp_repository.rs`
- `crates/jig/src/state/`
- `crates/jig/src/bootstrap/`
- `docs/public-contract.md`

## Scope

- Implement only the approach selected by task 11's recorded decision.
- If every approach was rejected, document that disposition and retain the existing runtime.
- Make new projections or discovery profiles opt-in initially.
- If retained handles were selected, implement bounded persistence and lookup lifecycle.
- Preserve old request forms, tool names, and exact effect/source validation.
- Use task 07's explicit surface selection before changing advertised schemas.
- Keep full diagnostic results accessible.
- Do not add a remote coordination service or general agent-supplied shell tool.

## Implementation sequence

1. Read task 11's measurements and explicit decision before touching production code.
2. Convert experimental behavior into shared typed runtime logic.
3. Specify compatible discovery and request selection for old and new clients.
4. Integrate retained state with existing journaling and recovery only if selected.
5. Test restart, disconnect, expiry, cancellation, and concurrent callers.
6. Update generated MCP configuration only if necessary and opt-in.
7. Document rollback to the existing interface.
8. Deliver migration fixtures and public contract changes.

## Acceptance criteria

- Only evidence-supported experimental behavior becomes production behavior.
- Existing clients can continue using current full request forms.
- New clients receive bounded compact responses and can request detail.
- State retention is bounded and failure behavior is deterministic.
- Execution still validates exact source/configuration/arguments/effects.
- Explicit surface selection never silently chooses a different operation.
- Opt-in rollout and rollback are documented and tested.
- A rejected experiment closes this task with an explicit no-change decision, not fabricated
  implementation.

## Verification

- Run old-client/new-server and new-client/supported-server fixtures.
- Exercise journal interruption and plan lookup failure if handles are implemented.
- Validate descriptor/output schemas for each advertised profile.
- Run relevant MCP/runtime tests and required repository gates.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Persisted formats require compatibility handling even when model behavior improves.
- Do not weaken validation to make a compact handle inexpensive.
- Do not delete prototype evidence when its production outcome is rejection.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Decision input and conditional acceptance

Read `docs/benchmarks/astra-harness/compact-mcp-decision.md` from task 11.
Missing or inconclusive input leaves implementation blocked.
For reject, record the decision and preserve the current runtime.
Compact-response, retention, and migration criteria are not applicable.
For projection-only, satisfy projection and compatibility criteria;
retained-state, restart, expiry, and persistence criteria are not applicable.
For retained-handles, satisfy all applicable lifecycle and authority criteria.
Report each non-applicable criterion with the selected-branch reason.
Use task 07's explicit surface selection; do not add implicit negotiation.
The standard surface keeps baseline shapes; agent-v1 carries opt-in changes.

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
