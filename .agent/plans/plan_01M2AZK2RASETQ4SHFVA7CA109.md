# 06 — Allow compact outcome-focused ExecPlans and align goal prompts

## Task identity

- Local task: 06.
- Beads issue: `jig-sh-9wcn.6`.
- Priority: P2.
- Parent: `jig-sh-9wcn`, Astra harness modernization.
- Depends on: `jig-sh-9wcn.1`, `jig-sh-9wcn.2`
- Unblocks: 13, 14, 15.
- Status: original implementation verified and work closed on 2026-09-12;
  post-review repairs are tracked separately below.

## Context and outcome

The current planning text already handles autonomy and resumption well. Mandatory section
maintenance and generic goal checkpoints can still distract agents from task-specific
acceptance and next actions.

This is work in the jig-sh Rust CLI and repository-template project.
Read the root AGENTS.md and the nearest existing guide before changing code.
The immutable evaluation control is Git commit 03e9a9e4e5122b5bc12c66b1f635ae1faac05e15.
The implementation baseline is 2c60f635 (resolve the full revision in the work record).
Resolve task-local facts against the actual worktree before implementation.

## Progress

- [x] Research current official Astra guidance and inspect planning and goal paths.
- [x] Verify the issue is open and its blocking dependencies are closed; claim task 06.
- [x] Align root, source template, and embedded planning guidance; add examples.
- [x] Update goal generation and regression coverage.
- [x] Pass configured verification, record acceptance, and close the work and bead.

Restart checkpoint: do not repeat the completed implementation. This work record,
`plan_01M2AZK2RASETQ4SHFVA7CA109`, was closed through `work finish` after passing
all required gates. For the subsequently authorized blocked-goal and journal repairs,
resume from [the repair work record](plan_01M2B5C1NTCDTG3ZZJ03Z8ZT19.md) and its
current evidence. The historical execution notes below are not current next actions.

## Surprises & Discoveries

- Root/template/snapshot currently match. They require four separate history sections
  plus an eleven-topic skeleton even though required properties already describe the
  essential information. Make headings optional while retaining the information.
- Supplied checkpoints already replace defaults; the actual gap is the five generic
  defaults when checkpoints are absent. Use the supplied success condition as the
  fallback checkpoint, with restart state recorded separately.
- CLI and JSON tool requests share `WorkGoalRequest` and the same runtime generator.
  JSON accepts omitted/null optional lists; blank supplied items are rejected. Preserve
  these behaviors, title normalization, response keys, and plan/session persistence.
- The main-checkout Beads database does not contain task 06 in this environment.
  Explicitly opening this worktree's configured database imports its tracked export;
  plain `br info --json` now resolves here. Export diff is limited to task 06.

## Decision Log

2026-09-12: Apply the official guidance as repository-neutral policy: compact required
information, explicit completion and authority, optional delegation, and proportional
verification. Keep configured checks and review gates authoritative. These are design
choices informed by the sources, not measured performance claims.

Sources checked 2026-09-12:
- [OpenAI Astra prompting guidance](https://developers.openai.com/api/docs/guides/latest-model#prompting-best-practices)
  recommends auditing instruction conflicts, clear follow-through, and calibrated tests.
- [Rethinking skills and prompts for GPT-6 Astra](https://developers.openai.com/blog/rethinking-skills-and-prompts-for-gpt-6-astra)
  (2026-09-11) recommends contextual instructions and defining completion before work.

2026-09-12: Update only newly generated goal bodies/prompts. Existing durable plans and
append-only journals retain their content. A requested review checkpoint remains a
stop; ordinary progress checkpoints do not introduce approval requirements.

## Outcomes & Retrospective

Original audit, implementation, and acceptance are complete. Final run
`run_01M2B2RXQMBNQ6GMXFZ98HVK5X` passed all six configured targets, including 4,089
Rust tests and 42 offline harness tests, with three configured Rust skips; validation
receipt: `receipt_01M2B42X8J1RK2FQK92J3MSFH4`. Work and bead closure followed.
These results cover the original implementation, not subsequent review repairs;
the repair work record carries their verification and closure. No model comparison
has run; comparative behavior evaluation belongs to task 15.

## Relevant entrypoints

- `.agent/PLANS.md`
- `templates/project/.agent/PLANS.md.jinja`
- `crates/jig/src/bootstrap/embedded_template_snapshots/.agent/PLANS.md.jinja`
- `crates/jig/src/runtime/work/goal.rs`
- `crates/jig/src/cli/work.rs`
- `crates/jig/src/command/work.rs`: shared request compatibility.
- `crates/jig/src/runtime/tests/work.rs`: goal generation and input validation tests.
- `crates/jig/src/bootstrap/embedded_templates.rs`: existing snapshot parity test.

## Scope

- Preserve acceptance, decisions, evidence, scope, and restart context as required
  information.
- Allow these properties in a compact plan without fixed headings or an eleven-section
  skeleton.
- Keep detailed plans appropriate for risky migrations or multi-stage work.
- State that research/planning requests do not authorize implementation.
- Clarify routine reversible choices and continuing authorized independent work.
- Preserve supplied checkpoints; use the success condition when none are supplied.
- Preserve mandatory validations and configured gates. Reuse qualifying evidence;
  repeated checks need changed inputs, a concrete unresolved concern, or repo policy.
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
- CLI and JSON requests preserve constraints, explicit approval checkpoints, notes,
  success text, and validation commands after existing whitespace normalization.
- Omitted/null checkpoints produce an outcome checkpoint, without the five generic
  process steps. Blank supplied checkpoints/constraints/validations still fail.
- Resume guidance reconciles actual worktree and evidence before choosing the next
  action; goal generation itself does not execute validation or close the plan.

## Verification

- Exercise goal generation with and without explicit checkpoints.
- Assert preservation of user constraints and scope limitations.
- Check template parity and short/extended examples for completeness.
- Cover the shared JSON path as well as existing CLI dispatch tests; include a
  planning-only objective and an explicit approval checkpoint.
- Manually review compact and extended examples against all required information.

Run commands from the repository root.
For runtime changes, build with `cargo build -p jig-sh --bin jig`.
Select that binary with `JIG_DEV_BIN=target/debug/jig` for harness commands.
Run change-appropriate checks and all applicable repository-required gates.
Use `scripts/jig work check --plan-id <id>` for the configured `verify` profile:
Rust clippy, formatting, tests, contract, file-budget, and harness-eval-test. A passing
profile must cover current inputs before `scripts/jig work finish --plan-id <id>`.
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
Run `python3 scripts/beads-sync.py` after Beads mutations. Verify `br info --json`
resolves the worktree database containing this task; do not export the older main
database over this branch's issue history.
See README.md in this plan directory for shared rollout policy and the dependency matrix.

Revision note (2026-09-12): distinguished the evaluation control from the implementation
baseline; corrected the already-supported supplied-checkpoint behavior; added the JSON
request path, concrete fallback and compatibility checks, configured verification, and
the observed Beads resolution. Scope remains task 06.


## Historical execution notes

The following entries preserve observations made during the original run. Their
pending actions are superseded by the current Progress and Outcomes above.

Implementation added compact plan guidance in root/source/embedded copies, outcome checkpoint defaults, explicit authorization and evidence guidance, CLI help, and JSON/CLI regression coverage. Canonical evolving specification: docs/plans/astra-harness-modernization/06-compact-execplans.md. Examples: docs/plans/astra-harness-modernization/06-plan-examples.md. Build passed; configured six-target verification is running. Beads resolves this worktree database; main-checkout database lacks task 06.

Implemented and verified task 06. Stable run run_01M2B1ATERGKGCNF1Y9M98KS0F passed all six targets: 4089 Rust tests, 42 harness tests, three configured Rust skips. Initial failure and corrected test oracle are recorded in docs/plans/astra-harness-modernization/06-implementation.md. Bead closed and final handoff documents/export updated. Next inspect current evidence and execute only the required refresh before work finish; keep the source worktree unchanged during verification.

Task 06 implementation and acceptance are complete. Final frozen-worktree run run_01M2B2RXQMBNQ6GMXFZ98HVK5X passed all six configured targets, including 4089 Rust tests and 42 offline harness tests, with three configured Rust skips. Validation receipt: receipt_01M2B42X8J1RK2FQK92J3MSFH4. Gate inspection passed with no missing, failed, stale, or unknown requirements. Bead jig-sh-9wcn.6 is closed and its export is synchronized. Final closure follows through work finish; no implementation work remains.

Revision note (2026-09-12, post-review): corrected stale status, unchecked progress,
restart instructions, and unverified outcomes against the recorded passing run and
work closure. Historical observations remain intact; follow-up repairs have their
own work record so the original evidence is not presented as proof of later edits.
