# 14 — Make setup and optional agent tooling guidance task-appropriate

## Task identity

- Local task: 14.
- Beads issue: `jig-sh-9wcn.14`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.4`, `jig-sh-9wcn.6`
- Unblocks: 15.
- Status: planned; implementation has not started.

## Context and outcome

A fresh-machine instruction to follow every doctor next step can pull an agent into optional
Codex marketplace setup. Guidance should distinguish required repository dependencies from
optional client extensions.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `templates/project/AGENTS.md.jinja`
- `AGENTS.md`
- `docs/adoption.md`
- `crates/jig/src/runtime/agent.rs`
- `crates/jig/src/doctor.rs`
- `crates/jig/src/cli/agent.rs`

## Scope

- Clarify when doctor and repository bootstrap are needed.
- Distinguish repository toolchain readiness from optional client marketplace availability.
- Preserve explicitly configured required review/skill dependencies.
- Do not install or modify global agent tooling during ordinary unrelated repository work.
- Explain the difference between registering a marketplace and enabling its plugins.
- Keep agent bootstrap an explicit operation with existing command semantics.
- Keep review/refine and delegation optional unless task or policy requires them.
- Avoid model-specific defaults or duplicated harness personality instructions.

## Implementation sequence

1. Inspect current doctor readiness fields and actual bootstrap behavior.
2. Classify missing prerequisites as repository-required, policy-required, or
   optional-client setup.
3. Update guidance and help to communicate the classification.
4. Preserve existing machine-readable readiness fields and add compatible distinctions if
   needed.
5. Ensure repositories with no marketplace requirement remain usable.
6. Ensure required review gates report their missing dependency clearly.
7. Exercise fresh-machine and already-configured fixture paths.
8. Document setup without implying authorization for unrelated external changes.

## Acceptance criteria

- Ordinary coding tasks do not require unrelated optional agent setup.
- Configured required review dependencies still block the relevant operation truthfully.
- The guidance accurately describes marketplace registration versus plugin enablement.
- No automatic global installation is added.
- Doctor diagnostics remain actionable for missing required tools.
- Codex and non-Codex clients can use the same repository checks.
- Review loops and subagents are conditional, not mandatory.
- Existing bootstrap invocations retain their documented effects.

## Verification

- Use agent doctor fixtures with absent, optional, and required marketplace configurations.
- Verify bootstrap help against the command it actually executes.
- Check legacy JSON consumers and relevant generated guidance.
- Run backend tests if readiness classification changes runtime code.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Do not silently downgrade an authored required dependency to optional.
- Model-specific prompting belongs in the client unless it expresses real repository policy.
- This task changes setup clarity, not provider authentication or account management.
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
