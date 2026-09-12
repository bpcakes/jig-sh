# 03 — Make configured verification evidence the completion authority

## Task identity

- Local task: 03.
- Beads issue: `jig-sh-9wcn.3`.
- Priority: P1.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.1`, `jig-sh-9wcn.2`
- Unblocks: 09, 15.
- Status: planned; implementation has not started.

## Context and outcome

The default verify profile already includes tests, formatting, Clippy, contract checks, and
file budgets. Requiring another final test command encourages redundant execution and
conflicts with evidence reuse.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `.jig.toml`
- `AGENTS.md`
- `templates/project/AGENTS.md.jinja`
- `templates/project/.jig.toml.jinja`
- `crates/jig/src/runtime/work/checks/targets.rs`
- `crates/jig/src/runtime/work/gates/recovery.rs`
- `crates/jig/src/cli/work.rs`

## Scope

- Replace procedural repeat-test wording with current passing evidence covering required
  checks.
- Preserve every mandatory test/check obligation in the relevant configured profile or gate.
- Define routine edits, durable handoff work, and policy-required structured work
  separately.
- Keep plain check execution and work-check reuse semantics distinct.
- Document that --affected selects candidates and does not waive required work gates.
- Prefer the existing recovery projection for diagnostics instead of inventing another gate
  ledger.
- Keep authored custom profiles and review gates authoritative.
- Correct examples that imply a legacy --tool receipt satisfies a native target gate.

## Implementation sequence

1. Inventory required prose checks against configured native and legacy gate requirements.
2. Identify supported generated cases with additional SQLx, migration, or frontend
   requirements.
3. Add any missing mandatory coverage to generated policy before removing corresponding
   prose.
4. Update root, templates, and work help to explain current evidence instead of last-command
   ordering.
5. Document the small-change workflow and when an explicit plan ID is required.
6. Exercise a current pass, changed input, failed target, and dependent target.
7. Check that review gates remain unresolved until their configured evidence exists.
8. Refresh affected snapshots and documentation together.

## Acceptance criteria

- Every preexisting required verification obligation remains represented or explicitly
  preserved.
- A second work check reuses qualifying passing target receipts.
- Changed relevant inputs still invalidate evidence under the declared freshness policy.
- A plain check command is not advertised as a universal evidence cache.
- An affected no-op is not treated as proof that every required work gate passed.
- Routine investigation is not instructed to manufacture implementation plans or run
  unrelated tests.
- Legacy and native receipt distinctions remain clear.
- No gate is weakened solely to improve evaluation timing.

## Verification

- Reuse existing work-check receipt scheduling and freshness tests.
- Add regressions only for policy branches whose required coverage changes.
- Render Rust, Go, SQLx, and frontend policy examples and inspect selected targets.
- Run required repository gates and finish backend work with the then-applicable test
  policy.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Unrelated component changes and incomplete observations can still force conservative
  verification.
- Authored gate preservation must survive adoption and recopy.
- This task changes guidance and policy deliberately; task 01 must not preempt it.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

## Repository-local dogfooding

Also deliberately update the source repo's unmanaged Dogfooding This Harness
section in AGENTS.md.
Retain the development-binary requirement.
Make repeated gates/evidence/receipts/status calls conditional there too.
Task 01 only changes managed defaults; this task removes the local contradiction.

## Completion and handoff

Attach implementation revision, relevant result artifacts, and acceptance disposition to the bead.
Close only after the task's stated outcome is delivered; a plan or expected test result is not proof.
Leave an actionable restart checkpoint if interrupted.
Run `python3 scripts/beads-sync.py` after Beads mutations, using the canonical main-checkout database.
See README.md in this plan directory for shared rollout policy and the dependency matrix.
