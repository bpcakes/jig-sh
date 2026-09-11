# 15 — Evaluate the completed guidance and surface changes on Astra tasks

## Task identity

- Local task: 15.
- Beads issue: `jig-sh-9wcn.15`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.3`, `jig-sh-9wcn.5`, `jig-sh-9wcn.6`, `jig-sh-9wcn.8`, `jig-sh-9wcn.9`, `jig-sh-9wcn.10`, `jig-sh-9wcn.12`, `jig-sh-9wcn.13`, `jig-sh-9wcn.14`
- Unblocks: 16.
- Status: planned; implementation has not started.

## Context and outcome

The epic should demonstrate task outcomes and characterize tradeoffs. Aggregate token or
tool-count reductions alone do not show that agents complete useful work more effectively.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `scripts/`
- `tests/fixtures/README.md`
- `docs/benchmarks/`
- `docs/plans/astra-harness-modernization/README.md`

## Scope

- Run the task 02 protocol against original and completed treatments.
- Use the same Astra model identity, reasoning settings, tools, and task starts within each
  comparison.
- Evaluate guidance-only, schema-only, and combined changes separately where possible.
- Grade correctness, invariant violations, missed acceptance, unnecessary questions,
  repeated checks, time, and usage.
- Include interrupted work and compatibility-sensitive migration scenarios.
- Preserve failed and timed-out trials.
- Report uncertainty and sample size rather than claiming universal speedups.
- Recommend final defaults using the observed results.

## Implementation sequence

1. Verify the evaluation driver and pinned baseline artifacts have not drifted.
2. Record the exact treatment revisions and active client configuration.
3. Randomize or counterbalance paired trial ordering.
4. Run the predeclared repetitions using configured authorized model access.
5. Grade artifacts independently from final agent prose.
6. Inspect regressions and attribute them to guidance, tools, or implementation defects.
7. Repeat only when a fix, failed trial condition, or declared protocol requires it.
8. Publish the measurements and a clear retain/revise/reject decision for each treatment.

## Acceptance criteria

- All five task families have reproducible paired results.
- Environment limitations are retained progress evidence, not completion.
- Actual model/client identities and sample counts accompany reported numbers.
- Correctness and invariant checks remain primary acceptance measures.
- Unavailable usage metrics are labeled unavailable.
- Failures and exclusions remain in the result set with reasons.
- No production recommendation relies solely on raw descriptor byte counts.
- Regressions have a resolved mitigation or a rollback/default decision.
- The report supports task 16's rollout decision.

## Verification

- Run the driver self-checks before external model trials.
- Review fixture validity and grader independence.
- Recompute summary metrics from retained raw result records.
- Check published artifacts for private identifiers and secrets.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- A small evaluation cannot establish a population-wide quality claim.
- Do not spend against an unconfigured provider or silently switch models to finish the
  report.
- An unavailable paid execution environment leaves this implementation task incomplete, with
  fixture work retained.
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
