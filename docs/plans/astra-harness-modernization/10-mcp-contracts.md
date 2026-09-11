# 10 — Type work-tool results and measure the MCP contract surface

## Task identity

- Local task: 10.
- Beads issue: `jig-sh-9wcn.10`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.7`, `jig-sh-9wcn.9`
- Unblocks: 11, 13, 15.
- Status: planned; implementation has not started.

## Context and outcome

Only the four repository tools currently declare output schemas, while thirteen work/doctor
tools use older descriptors. Typed compact outputs improve reliability, but schema growth
must be measured alongside it.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `crates/jig/src/tool_defs.rs`
- `crates/jig/src/tool_defs/repository.rs`
- `crates/jig/src/mcp.rs`
- `crates/jig/src/runtime/work.rs`
- `crates/jig/src/runtime/mcp_repository.rs`
- `crates/jig/src/mcp/tests/`

## Scope

- Define output schemas for work/doctor results using shared typed projections.
- Measure descriptor bytes by tool and schema component.
- Add truthful tool annotations only where the operation's actual effects justify them.
- Preserve structuredContent behavior and compatible text rendering.
- Retain current tool names and default exposure during this task.
- Use compact work-summary DTOs from task 09.
- Document protocol errors versus valid command failures.
- Avoid embedding the full catalog schema in every unrelated work-tool response.

## Implementation sequence

New advertised work/doctor output schemas and compact DTOs belong to agent-v1.
Preserve baseline standard descriptors and result shapes.
The requirement for every exposed work tool to have an output schema applies to
agent-v1; standard retains its compatibility shape.
Use separate fixtures for agent-v1 schema validity and standard baseline parity.

1. Capture baseline descriptors using a local initialize/tools-list session.
2. Inventory each memory tool's success and failure envelope.
3. Define shared bounded DTOs with explicit optional and unknown fields.
4. Attach outputSchema descriptors and validate actual tool responses.
5. Audit annotations against state reconciliation and journal writes.
6. Measure total and per-tool schema sizes after the change.
7. Add compatibility fixtures for clients that ignore outputSchema.
8. Document any required response version changes.

## Acceptance criteria

- Every exposed work/doctor tool has a validated output contract.
- Real structured results validate against advertised schemas.
- Tool annotations do not claim pure read-only behavior for state-writing paths.
- Command failures and protocol failures remain distinguishable.
- Descriptor measurements identify large duplicated schema branches.
- Tool names and existing supported request forms continue to work.
- Human-readable output remains useful.
- No numeric performance claim is made from descriptor size alone.

## Verification

- Run MCP schema-validation and protocol regression fixtures.
- Exercise success, ordinary target failure, invalid arguments, and stale authority.
- Test clients omitting optional MCP capabilities.
- Compare per-tool descriptor size reports with the baseline driver.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Output schemas can improve correctness while increasing context size.
- Do not incorrectly mark inspect read-only across its abandoned-run reconciliation case.
- Do not convert error responses to success simply to satisfy a schema.
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
