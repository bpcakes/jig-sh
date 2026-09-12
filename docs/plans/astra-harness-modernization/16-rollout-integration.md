# 16 — Validate adoption compatibility and publish the modernized workflow

## Task identity

- Local task: 16.
- Beads issue: `jig-sh-9wcn.16`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.15`
- Unblocks: Epic closure.
- Status: planned; implementation has not started.

## Context and outcome

The source repository and generated consumers must receive coherent guidance and runtime
behavior. A final integration slice prevents a successful local experiment from shipping
stale templates or breaking existing adopters.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The plan baseline is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
Resolve task-local facts against the actual worktree before implementation.

## Relevant entrypoints

- `README.md`
- `AGENTS.md`
- `docs/adoption.md`
- `docs/public-contract.md`
- `templates/project/`
- `crates/jig/src/bootstrap/tests/`
- `crates/jig/src/bootstrap/embedded_template_snapshots/`

## Scope

- Reconcile all shipped examples with the final supported workflow.
- Validate init, full adoption, minimal adoption, update, and recopy.
- Preserve unmanaged guidance and current minimal-footprint semantics.
- Use evaluated defaults; keep unproven compact MCP features opt-in or rejected.
- Exercise legacy/current contract and persisted work compatibility.
- Document current evidence, acceptance, setup, and diagnostic behavior together.
- Map every audit recommendation to implementation evidence or an explicit rejected
  experiment.
- Close the epic only after child outcomes and required integration checks are complete.

## Implementation sequence

1. Review task 15's results and determine the supported default behavior.
2. Run the generated-consumer compatibility matrix.
3. Verify root guidance, template source, and embedded snapshots agree.
4. Check links to moved specialized guidance from generated and source contexts.
5. Review public response schemas and persisted-format migration documentation.
6. Run the configured repository gates through a freshly built development binary.
7. Inspect evidence and finish the implementation work plan only when policy passes.
8. Publish a concise workflow guide and epic outcome summary.

## Acceptance criteria

- Source and generated guidance teach the same supported workflow.
- Full adoption preserves local user content outside managed boundaries.
- Minimal adoption does not gain previously omitted launcher/MCP files implicitly.
- Existing clients and legacy work records retain supported behavior.
- No accepted recommendation is silently dropped.
- Rejected experiments include their evidence and rationale.
- All required verification passes on the integrated implementation.
- The epic's first quick win remains independently understandable and the final state is
  documented.

## Verification

- Run existing init/adopt/update/recopy and managed-block preservation suites.
- Exercise Rust-only, Go, frontend, and migration-enabled generated consumers.
- Run MCP compatibility, target/work evidence, and planning-template parity tests.
- Finish backend implementation with current evidence satisfying the then-active
  backend-test policy.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Record observed results, including failures and tests not run.

## Compatibility and boundaries

- Do not run broad recopy over unrelated authored files without reviewing ownership.
- Avoid coupling release of simple guidance changes to an experimental feature that failed
  its decision criteria.
- No historical .agent/state record may be rewritten as part of this rollout.
- Preserve unrelated worktree changes and unmanaged generated-file sections.
- Keep .agent/state JSONL history append-only; use explicit migrations for new persisted formats.
- Use generic fixtures such as ExampleProject and ExampleVault.
- This task does not authorize unrelated account setup, publishing, or external messages.

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
