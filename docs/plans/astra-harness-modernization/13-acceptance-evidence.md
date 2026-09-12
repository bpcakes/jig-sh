# 13 — Link task acceptance criteria to evidence and completion reporting

## Task identity

- Local task: 13.
- Beads issue: `jig-sh-9wcn.13`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.6`, `jig-sh-9wcn.9`, `jig-sh-9wcn.10`
- Unblocks: 15.
- Status: planned; implementation has not started.

## Context and outcome

Repository checks prove configured policy, not every user-visible acceptance criterion.
Existing goal and finish surfaces can expose this difference without introducing another
task tracker.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `crates/jig/src/runtime/work/goal.rs`
- `crates/jig/src/runtime/work.rs`
- `crates/jig/src/state/`
- `crates/jig/src/command/work.rs`
- `crates/jig/src/cli/work.rs`
- `docs/public-contract.md`

## Scope

- Extend existing work/goal data with stable acceptance criterion identities.
- Support criterion status with evidence references and explicit unverified or waived
  reasons.
- Distinguish machine-checked evidence from human/agent assertions.
- Make new strict acceptance enforcement opt-in for new plans.
- Preserve closure behavior for legacy plans unless repository policy explicitly changes.
- Do not execute free-form validation text as a shell command.
- Reuse existing run/receipt identities instead of creating a competing evidence store.
- Keep criterion amendments and dispositions append-only.

## Implementation sequence

1. Inspect plan/session/decision formats and supported extensions.
2. Define criterion states, evidence reference types, and amendment rules.
3. Add a compatible plan-level policy indicating whether criterion completion is required.
4. Expose unresolved criteria in task 09's completion summary.
5. Validate evidence references for identity, freshness where applicable, and declared
   scope.
6. Treat self-attested explanations as assertions rather than test results.
7. Retain final gate/lease revalidation and add criterion checks only for opted-in plans.
8. Document amendments, rejected evidence, legacy reads, and recovery.

## Acceptance criteria

- New plans can enumerate user-visible acceptance criteria with stable identities.
- Each completed criterion distinguishes executable evidence from assertion.
- Missing or stale evidence remains visible rather than being summarized as passed.
- Strict criteria block finish only when opted in by supported policy.
- Legacy plans and records remain readable and keep existing closure semantics.
- Free-form goal validation text cannot introduce arbitrary command execution.
- Criterion changes are recorded without rewriting historical records.
- The result complements Beads and repository gates rather than replacing either.

## Verification

- Cover legacy plans, optional criteria, strict criteria, and amended criteria.
- Reject missing, foreign-plan, or incompatible evidence references as appropriate to the
  policy.
- Test source drift between acceptance inspection and finish.
- Verify that assertions are never rendered as machine-verified outcomes.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- A criterion cannot be mechanically proven merely because an agent attached a passing
  unrelated test.
- Do not duplicate the separate arbitrary-command-evidence proposal jig-sh-x4n.
- New persisted data needs explicit epoch/schema compatibility and recovery coverage.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Acceptance semantics and writer compatibility

Each criterion declares a stable ID, text revision, and one requirement kind.
`target_evidence` criteria declare a nonempty set of exact required target IDs.
Every member must have current successful linked evidence; a partial pass fails.
Prose assertions cannot satisfy this kind.
Add a two-target partial-pass test as well as an all-targets-pass test.
`attestation` criteria require a recorded actor and rationale and are always
displayed as attested, never machine-verified.
Waivers satisfy strict completion only when that criterion explicitly sets
`allow_waiver=true` at creation or through an authorized recorded amendment.
Default allow_waiver is false; every waiver records actor and rationale.
A text/scope amendment reopens the criterion and supersedes earlier dispositions.
Source drift invalidates target-evidence satisfaction under existing policy.
An unrelated passing target is never sufficient.
Strict plans require a new writer/contract boundary that older runtimes reject
before closure; legacy read compatibility alone is insufficient.
Select an unused epoch above every supported writer epoch at implementation time.
Do not insert strict data into an old epoch.
Extend task 10's typed work results and schemas for all criterion states.

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
