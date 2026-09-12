# 07 — Expose freshness policy through target inspection

## Task identity

- Local task: 07.
- Beads issue: `jig-sh-9wcn.7`.
- Priority: P1.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.1`, `jig-sh-9wcn.2`
- Unblocks: 08, 09, 10.
- Status: planned; implementation has not started.

## Context and outcome

Target inspection shows runner and input paths but omits inputs_policy and source_state.
Agents cannot explain evidence reuse reliably without the effective values and whether they
were defaulted.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `crates/jig/src/repository/inspect.rs`
- `crates/jig/src/tool_defs/repository.rs`
- `crates/jig-contract/src/freshness.rs`
- `crates/jig/src/repository/freshness.rs`
- `docs/public-contract.md`
- `docs/target-freshness-integration.md`

## Scope

- Expose effective inputs_policy and source_state in target inspection.
- Represent provenance/defaulting where supported by the authored model.
- Preserve the distinction between whole-repository and exhaustive inputs.
- Preserve the distinction between Git-sensitive and worktree-sensitive identity.
- Expose policy, not a claim that a particular receipt is currently fresh.
- Keep CLI JSON and MCP inspection derived from the same typed projection.
- Handle legacy contract epochs explicitly.
- Document response evolution before changing strict output schemas.

## Implementation sequence

1. Read current ActionSpec fields, defaults, provenance, and epoch validation.
2. Define an additive typed freshness-policy inspection object.
3. Resolve effective defaults using the same authority as runtime freshness checks.
4. Update target, component, and workspace inspection projections consistently.
5. Regenerate MCP output schemas and review descriptor size impact.
6. Add examples showing identical input globs with different source-state policies.
7. Cover old manifests lacking the new fields.
8. Update public schema/version documentation according to current compatibility rules.

## Acceptance criteria

- A target inspection reports effective input and source-state policy.
- Omitted defaults are distinguishable from explicit declarations when provenance exists.
- Legacy targets remain inspectable with truthful compatibility metadata.
- The response never labels evidence fresh without running the evidence evaluator.
- CLI and MCP expose equivalent values.
- No evidence invalidation or input-selection semantics change.
- Strict output-schema consumers have a documented compatible path.
- Examples explain why input lists alone are insufficient.

## Verification

- Use inspection fixtures for defaults, explicit exhaustive, explicit worktree, and legacy
  cases.
- Compare CLI JSON and MCP structured results.
- Validate generated schemas against real outputs.
- Run focused inspection/freshness tests and required backend verification.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Default resolution must track contract epoch rather than assume the newest policy
  everywhere.
- Adding output fields can affect strict clients and must follow documented schema
  evolution.
- Do not include environment values or secrets in inspection.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Shared response evolution

This task owns explicit selection used by tasks 08–13.
Proposed CLI inspection option: `--projection standard|agent-v1`.
Proposed MCP server option: `jig mcp --surface standard|agent-v1`.
Omission preserves the baseline standard response and descriptor shapes.
The opt-in agent-v1 surface advertises its own strict result schemas.
Unknown selections fail before serving or executing an operation.
Implement and test the shared selection plumbing here.
Do not assume connection negotiation exists in MCP initialize.
These flags are proposed interfaces, not existing commands.

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
