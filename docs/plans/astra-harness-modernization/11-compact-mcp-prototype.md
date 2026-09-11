# 11 — Prototype compact MCP plans and results with an explicit decision

## Task identity

- Local task: 11.
- Beads issue: `jig-sh-9wcn.11`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.2`, `jig-sh-9wcn.10`
- Unblocks: 12.
- Status: planned; implementation has not started.

## Context and outcome

The four core MCP descriptors account for almost all measured schema bytes. Compact
projections and plan references may reduce overhead, but they add lifecycle and
compatibility complexity that needs evidence.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `crates/jig/src/tool_defs/repository.rs`
- `crates/jig/src/runtime/mcp_repository.rs`
- `crates/jig/src/repository/planner.rs`
- `crates/jig/src/state.rs`
- `docs/public-contract.md`
- `scripts/`

## Scope

- Compare current full plans against compact response projections.
- Prototype server-retained immutable plans addressed by opaque identity in an isolated
  experimental path.
- Keep exact config/source/argument/effect validation before execution.
- Measure serialized sizes, actual client loading where observable, latency, and task
  correctness.
- Specify retention, lookup, expiry, process restart, and plan-not-found behavior.
- Compare opt-in discovery profiles without removing current tools.
- Finish with a documented ship-or-reject decision.
- Do not enable the prototype for existing consumers by default.

## Implementation sequence

1. Define the smallest experimental request/response contract needed for measurement.
2. Use synthetic repositories and temporary local state.
3. Bind each handle to one repository and immutable planned authority.
4. Reject tampered, expired, foreign-repository, and stale-source requests.
5. Run schema-only and paired task trials from task 02.
6. Measure both the benefits and added requests/round trips.
7. Assess state retention and compatibility costs.
8. Select the smallest approach supported by the evidence; rejection is a valid task
   outcome.

## Acceptance criteria

- The experiment preserves exact execution authority and explicit effect acknowledgment.
- Plan handles cannot cross repository boundaries or bypass source checks.
- Full existing plan requests continue to work in the experiment.
- Measurements report missing client telemetry honestly.
- Task correctness and invariant failures are evaluated alongside size and timing.
- Retention and restart behavior are specified before any production proposal.
- A decision explains why the chosen option earns its added complexity.
- No production cutover occurs in this task.

## Verification

- Exercise stale, unknown, expired, tampered, and foreign-repository handles.
- Test disconnected clients and unavailable retained state.
- Run deterministic schema-size and fixture-only measurements.
- Run authorized paired model trials when the configured environment permits them.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Smaller descriptors can be offset by more discovery round trips.
- A handle is not permission and cannot waive approved_effects.
- If evidence does not support retained handles, prefer compact results without new durable
  state.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Required decision artifact

Write `docs/benchmarks/astra-harness/compact-mcp-decision.md`.
Name the selected option: reject, projection-only, or retained-handles.
Include measurement paths, exact interface, selected compatibility mode,
retention/restart policy when relevant, and unresolved prerequisites.
An inconclusive result is not a ship decision.
This artifact is the explicit input consumed by task 12.

## Completion and handoff

Attach implementation revision, relevant result artifacts, and acceptance disposition to the bead.
Close only after the task's stated outcome is delivered; a plan or expected test result is not proof.
Leave an actionable restart checkpoint if interrupted.
Run `python3 scripts/beads-sync.py` after Beads mutations, using the canonical main-checkout database.
See README.md in this plan directory for shared rollout policy and the dependency matrix.
