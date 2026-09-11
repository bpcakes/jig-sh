# 02 — Build reproducible guidance and tool-surface evaluation fixtures

## Task identity

- Local task: 02.
- Beads issue: `jig-sh-9wcn.2`.
- Priority: P1.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.1`
- Unblocks: 03, 04, 06, 07, 11.
- Status: planned; implementation has not started.

## Context and outcome

Shorter guidance is a hypothesis about agent behavior. A fixed baseline and objective task
grading are necessary before attributing improvements to Astra or committing to larger MCP
changes.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `docs/plans/astra-harness-modernization/README.md`
- `scripts/`
- `tests/fixtures/README.md`
- `crates/jig/tests/agent_doctor_mcp.rs`
- `crates/jig/src/tool_defs.rs`

## Scope

- Create a small repository-local evaluation driver with generic synthetic fixtures.
- Pin the audit baseline 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15 as the original guidance
  condition.
- Separate guidance-only, tool-schema-only, and combined treatments.
- Cover a small fix, cross-crate feature, migration, frontend change, and interrupted-task
  resume.
- Record actual model identity, reasoning setting, client, tools, prompt, and trial order.
- Collect correctness and invariant violations independently of agent self-reports.
- Record wall time, tool calls, repeated checks, unnecessary questions, first useful edit,
  and available usage.
- Measure descriptor bytes separately from actual client context or billable tokens.

## Implementation sequence

1. Inspect existing fixture conventions and reuse generic project setup where practical.
2. Define one fixed prompt and an objective grader for each task family.
3. Store baseline instructions and checksums from the audited Git revision.
4. Implement isolated checkout creation and per-trial artifact paths.
5. Make the driver runnable without a model for fixture and grader verification.
6. Add a model-run mode that uses already configured provider access and explicit execution
   selection.
7. Capture unsupported or missing token telemetry as unavailable, never zero.
8. Document paired repetitions and a stopping rule before collecting comparative results.

## Acceptance criteria

- Every trial can be reconstructed from immutable revision and configuration metadata.
- Baseline and treatment see the same task and starting source state.
- Graders reject incorrect implementations even when all reported checks passed.
- Resume trials prove continuation from existing work instead of replaying mutations.
- A fixture-only run needs no external credentials or paid model calls.
- The driver distinguishes bytes, reported tokens, elapsed time, and missing observations.
- Results preserve failures, timeouts, and excluded trials with reasons.
- No performance improvement is claimed by merely counting fewer instructions.

## Verification

- Run all five fixture/grader smoke cases locally without external model execution.
- Deliberately feed a failing outcome to each objective grader.
- Verify separate output directories and deterministic baseline selection.
- Check artifacts for machine-local/private identifiers before committing them.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Historical baseline bytes are part of the experiment and must not be silently regenerated
  from current guidance.
- Model comparisons must use actual available model names; do not silently substitute
  another model.
- Do not add a new cloud provider, billing integration, or recurring evaluation service.
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
