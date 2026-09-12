# 08 — Add compact path-scoped context to info and inspect

## Task identity

- Local task: 08.
- Beads issue: `jig-sh-9wcn.8`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.5`, `jig-sh-9wcn.7`
- Unblocks: 15.
- Status: planned; implementation has not started.

## Context and outcome

An agent should be able to discover owners, relevant guidance, candidate checks, and
uncertainties for the task without reading the entire catalog or reimplementing ownership
logic.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `crates/jig/src/repository/inspect.rs`
- `crates/jig/src/repository/affected.rs`
- `crates/jig/src/repository/planner.rs`
- `crates/jig/src/runtime/mcp_repository.rs`
- `crates/jig/src/tool_defs/repository.rs`
- `crates/jig/src/cli.rs`
- `agent-map.md`

## Scope

- Extend existing info/inspect discovery rather than add a new top-level product.
- Accept explicit repository-relative paths or a Git comparison, with clear mutual
  exclusion.
- Return owners, applicable guide paths, candidate targets, and selection reasons.
- Return bounded unknown/unclaimed-input diagnostics.
- Include current work/evidence only when an explicit work-plan identity is supplied.
- Resolve paths and affected sets using existing repository authority.
- Keep the view observational: it must not execute checks or open work plans.
- Provide compact defaults with an explicit detailed projection.

## Implementation sequence

Use task 07's CLI `--projection standard|agent-v1` and MCP server
`--surface standard|agent-v1` selection.
Compact defaults apply within agent-v1 only.
Omitted selection preserves standard result and descriptor shapes.
Verify standard parity separately from the new compact discovery behavior.

1. Define the CLI spelling under info and the corresponding typed inspect request.
2. Specify stable ordering, output bounds, truncation flags, and unknown states.
3. Reuse affected selection while preserving component-granular semantics.
4. Resolve guide scope for the requested paths, including applicable ancestor guides.
5. Separate candidate checks from policy-required gates in the response.
6. Join work evidence only through the existing evaluator and its budgets.
7. Document that runtime inspections may reconcile abandoned runs where current APIs already
   do so.
8. Update examples to route common task discovery through this view.

## Acceptance criteria

- Explicit paths resolve to the same owners as existing affected-selection authority.
- Unclaimed inputs are visible and retain conservative behavior.
- Guide paths include applicable local scope without a mandatory full-map read.
- Candidate targets are not mislabeled as an exhaustive completion requirement.
- Output is bounded, deterministic, and explicit about partial results.
- No target executes and no work plan opens during discovery.
- CLI and MCP views share a typed result.
- Invalid paths, unknown refs, and unavailable evidence produce actionable diagnostics.

## Verification

- Cover nested owners, shared inputs, ignored paths, unclaimed paths, and renames.
- Verify guide ancestry and explicit owner-guide handling.
- Exercise output bounds and observation timeouts.
- Compare selected candidates with existing affected planner fixtures.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Do not implement another independent ownership graph.
- Deleted paths need comparison-aware ownership without assuming files still exist.
- Concurrent source changes should report uncertainty instead of promising an exact plan.
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
