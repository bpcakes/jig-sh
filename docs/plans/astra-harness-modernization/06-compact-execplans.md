# 06 — Allow compact outcome-focused ExecPlans and align goal prompts

## Task identity

- Local task: 06.
- Beads issue: `jig-sh-9wcn.6`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.1`, `jig-sh-9wcn.2`
- Unblocks: 13, 14, 15.
- Status: planned; implementation has not started.

## Context and outcome

The current planning text already handles autonomy and resumption well. Mandatory section
maintenance and generic goal checkpoints can still distract agents from task-specific
acceptance and next actions.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `.agent/PLANS.md`
- `templates/project/.agent/PLANS.md.jinja`
- `crates/jig/src/bootstrap/embedded_template_snapshots/.agent/PLANS.md.jinja`
- `crates/jig/src/runtime/work/goal.rs`
- `crates/jig/src/cli/work.rs`

## Scope

- Preserve acceptance, decisions, evidence, scope, and restart context as required
  information.
- Allow these properties in a compact plan without fixed headings or an eleven-section
  skeleton.
- Keep detailed plans appropriate for risky migrations or multi-stage work.
- State that research/planning requests do not authorize implementation.
- Clarify routine reversible choices and continuing authorized independent work.
- Replace generic goal boilerplate with task-relevant checkpoints when supplied.
- Keep delegation optional and dependent on available client capabilities.
- Do not force model-specific personality or model selection into repository guidance.

## Implementation sequence

1. Compare current root planning text with the shipped template and snapshots.
2. Draft short and extended examples demonstrating equivalent essential information.
3. Review the goal body and generated /goal prompt for unnecessary stopping triggers.
4. Preserve explicit user constraints and missing-authority stops.
5. Avoid turning a missing optional skill into a universal workflow blocker.
6. Keep existing goal CLI fields accepted and avoid silently changing their meaning.
7. Update relevant tests around generated prompt semantics rather than exact formatting.
8. Document resumption using actual source/evidence state after interruption.

## Acceptance criteria

- Small durable work can use a compact plan with acceptance and a restart checkpoint.
- Complex compatibility-sensitive work still records rollout and recovery.
- An implementation agent can infer allowed next actions without recurring milestone
  permission requests.
- Planning remains planning until implementation is authorized.
- Explicit user checkpoints and constraints survive generation unchanged in meaning.
- Optional delegation and reviews are not presented as universal requirements.
- Root/template/snapshot planning text stays aligned.
- Existing goal requests remain accepted.

## Verification

- Exercise goal generation with and without explicit checkpoints.
- Assert preservation of user constraints and scope limitations.
- Check template parity and short/extended examples for completeness.
- Run focused Rust tests if goal generation code changes.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Do not erase durable decision history when shortening active plans.
- The current planning task follows current rules; proposed relaxed rules apply only after
  implementation.
- A compact template must not hide unknown acceptance or unverified completion.
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
