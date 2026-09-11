# Astra harness modernization

## Purpose and scope

Modernize Jig's agent guidance and recommended CLI/MCP workflow for capable
reasoning models, using GPT-6 Astra as the evaluation target.
Reduce repeated instructions and unnecessary verification while preserving
deterministic execution, evidence freshness, process supervision, privacy,
and consumer compatibility.

This plan covers every recommendation in the September 11 audit.
It creates one epic with sixteen direct child tasks.
Task 01 is the first implementation task and an independently shippable quick win.
Planning and tasks 01–02 implementation are complete. Further implementation follows
explicit task requests; this document does not authorize starting the whole epic.

## Progress

- [x] Inspect repository and generated guidance, CLI, MCP, work, and evidence paths.
- [x] Establish `feat/astra-harness-modernization` in the main checkout; the initial separate-worktree handoff was superseded.
- [x] Draft sixteen implementation tasks with scope, rationale, acceptance, and tests.
- [x] Complete four sequential reasoning-model plan reviews and integrate findings.
- [x] Run standalone-task, dependency, rationale, and convergence checks after each round.
- [x] Create one Beads epic and sixteen child tasks with real blocking dependencies.
- [x] Review the converted Beads graph and descriptions.
- [x] Run export privacy and planning-artifact verification.
- [x] Record the epic ID, first task, and plan location.
- [x] Implement task 01 and close its Beads issue after verification.
- [x] Implement task 02 with five offline graders and reproducible paired evaluation inputs.
- [ ] Complete the remaining epic tasks and integrated evaluation.

Local planning/reconciliation work ID: `plan_01M28626D98WHJJFH0432ZBEKH`.
Task 01 implementation work ID: `plan_01M28819Z3PAE8YA2MBBBPY57Z` (closed).
Task 02 implementation work ID: `plan_01M28DTQG7Q8TFY7YYH9FPJYV7`; see [acceptance evidence](02-implementation.md).
Beads epic: `jig-sh-9wcn`.
Implementation issues: `jig-sh-9wcn.1` through `jig-sh-9wcn.16`.
Next implementation candidates after task 02 closure: `jig-sh-9wcn.3`, `.4`, `.6`, and `.7`.
Immutable evaluation control: `03e9a9e4e5122b5bc12c66b1f635ae1faac05e15`.
Feature-branch base: `dc68b74497c4f5ae27049425bb63e6d0ec4ff014`.
Restart checkpoint: tasks 01–02 are implemented; tasks 03–16 remain planned.
Its initial implementation run is `run_01M289WM1CWE50CWPBRV7YAAFC`, with validation
receipt `receipt_01M28A9NCY7YHW0VJH0M96MG1M` in the local state journals.
Review follow-up results are recorded in the local planning/reconciliation record.
When continuing implementation, run `br ready --parent jig-sh-9wcn --json` and
verify the requested task's current status before claiming it. Do not restart tasks 01–02.
Use plain `br` from the main checkout. The canonical database owns this epic.
The isolated database handoff has been superseded.

## Surprises & Discoveries

The current harness is not frozen at its GPT-5.2-era design.
It already has component-aware target selection, profiles, immutable run plans,
durable runs, typed effects, current-evidence reuse, and scoped freshness policies.
The planning instructions were revised on September 10.
The opportunity is primarily better exposure and less procedural instruction.

The audited root guide has approximately 1,338 words.
Its managed Jig block is approximately 409 words.
The Beads block contributes approximately 676 words.
Those counts describe this checkout and are not target quality metrics.

Live MCP tools/list returned seventeen tools and approximately 88,727 compact
serialized bytes of descriptors.
Four core repository tools account for approximately 84 KB.
Only those four declared output schemas at the audit baseline.
Descriptor bytes are not a measurement of model context loading or charged tokens.

An old development binary rejected the current source-state configuration.
Rebuilding the binary resolved discovery and contract validation.
The contract check passed.
The check used --no-receipt, but durable run lifecycle records were still appended.
The implementation plan must not describe that flag as a no-write guarantee.

## Decision Log

2026-09-11: Use one parent epic with direct child tasks.
Rationale: the user requested a single epic and a small task first.
Keep delivery units independently understandable and attach actual blocking edges.

2026-09-11: Preserve the audited Git revision as the evaluation control.
Rationale: task 01 can ship before the evaluation driver exists without losing
the old guidance condition; the driver retrieves immutable baseline bytes.

2026-09-11: Separate guidance changes from verification policy changes.
Rationale: the first task must not weaken required checks accidentally.
Task 03 owns replacing repeated command ordering with equivalent current evidence.

2026-09-11: Extend info/inspect and work instead of creating new command families.
Rationale: existing ownership, gate, and recovery machinery already answers most
of the proposed questions and should remain the single implementation authority.

2026-09-11: Treat compact MCP handles as an experiment before production delivery.
Rationale: retained plans add state and compatibility obligations; byte savings
alone do not justify them. Rejection is an explicit supported outcome.

2026-09-11: Keep full/minimal footprint meanings unchanged.
Rationale: minimal currently omits launcher/MCP files as well as guidance.
Shortening the full default does not require a new adoption profile.

2026-09-11: Preserve legacy work completion and make strict acceptance opt-in.
Rationale: new acceptance criteria cannot retroactively prevent existing plans
from finishing or reinterpret historical records.

2026-09-11: Keep model choice and generic agent personality out of repository defaults.
Rationale: shared guidance should communicate repository facts and policy across
clients. Astra-specific behavior is a measured evaluation concern.

2026-09-11: Use native reasoning-agent reviews for the required review rounds.
Rationale: the available native agents can review repository evidence directly.
No claim is made that GPT Pro or any external reviewer ran.

## Outcomes & Retrospective

The task specifications passed four sequential native reasoning-agent reviews.
The last round found no structural revisions or concrete blockers.
One epic, sixteen children, and thirty-four blocking dependencies are created.
Six conversion checks passed at initial creation, when task 01 was the only ready child.
Beads conversion and verification results are recorded in reviews.md and the
associated planning work record.
No claimed improvement in task performance has been measured.
Task 01 delivered the first guidance changes; task 02 delivered reproducible synthetic
evaluation fixtures and independent graders. Tasks 03–16 remain unimplemented.
The completed epic must distinguish delivered changes from rejected experiments.

## Repository orientation

- `AGENTS.md`: managed root defaults plus repository-local privacy, dogfooding, and Beads rules.
- `agent-map.md`: generated index of local guides.
- `crates/jig/AGENTS.md`: runtime ownership and invariants.
- `templates/project/AGENTS.md.jinja`: shipped root guidance.
- `templates/project/.agent/PLANS.md.jinja`: shipped planning guidance.
- `crates/jig/src/bootstrap/embedded_template_snapshots/`: snapshot parity.
- `crates/jig/src/bootstrap/managed_paths.rs`: ownership and minimal footprint.
- `crates/jig/src/repository/inspect.rs`: typed catalog projection.
- `crates/jig/src/repository/affected.rs`: change ownership and affected selection.
- `crates/jig/src/runtime/work/checks/targets.rs`: evidence reuse and execution.
- `crates/jig/src/runtime/work/gates/recovery.rs`: current recovery projection.
- `crates/jig/src/runtime/work.rs`: plan closure and gate revalidation.
- `crates/jig/src/runtime/work/goal.rs`: goal body and prompt generation.
- `crates/jig/src/tool_defs.rs`: memory/work tool descriptors.
- `crates/jig/src/tool_defs/repository.rs`: repository tool schemas.
- `crates/jig/src/runtime/mcp_repository.rs`: MCP execution and inspection.
- `crates/jig/src/policy/agent_map.rs`: guide/map generation and validation.
- `scripts/beads-sync.py`: required portable Beads export helper.

The audit checked the existing implementation, not just product documentation.
Where documentation describes an older contract epoch, inspect current types and
tests before choosing compatibility behavior.

## Task index and dependency graph

Each linked task is a standalone implementation specification.
Its complete body is also used as the Beads issue description.
Task NN maps to issue jig-sh-9wcn.N, without zero padding in the issue suffix.
Every task file records its exact issue ID, parent, and blocking issue IDs.

| Task | Priority | Depends on | Outcome |
| --- | --- | --- | --- |
| [01](01-guidance-quick-wins.md) | P1 | None | Shorten default guidance and correct stale workflow wording |
| [02](02-evaluation-baseline.md) | P1 | 01 | Build reproducible guidance and tool-surface evaluation fixtures |
| [03](03-verification-policy.md) | P1 | 01, 02 | Make configured verification evidence the completion authority |
| [04](04-guidance-locality.md) | P2 | 01, 02 | Move specialized guidance beside its owning code |
| [05](05-guide-validation.md) | P2 | 04 | Validate useful guide references without mandatory heading templates |
| [06](06-compact-execplans.md) | P2 | 01, 02 | Allow compact outcome-focused ExecPlans and align goal prompts |
| [07](07-freshness-inspection.md) | P1 | 01, 02 | Expose freshness policy through target inspection |
| [08](08-scoped-discovery.md) | P2 | 05, 07 | Add compact path-scoped context to info and inspect |
| [09](09-completion-summary.md) | P2 | 03, 07 | Unify work-check completion and recovery summaries |
| [10](10-mcp-contracts.md) | P2 | 07, 09 | Type work-tool results and measure the MCP contract surface |
| [11](11-compact-mcp-prototype.md) | P2 | 02, 10 | Prototype compact MCP plans and results with an explicit decision |
| [12](12-compact-mcp-delivery.md) | P2 | 11 | Deliver the selected compact MCP approach compatibly |
| [13](13-acceptance-evidence.md) | P2 | 06, 09, 10 | Link task acceptance criteria to evidence and completion reporting |
| [14](14-setup-guidance.md) | P2 | 04, 06 | Make setup and optional agent tooling guidance task-appropriate |
| [15](15-comparative-evaluation.md) | P2 | 03, 05, 06, 08, 09, 10, 12, 13, 14 | Evaluate the completed guidance and surface changes on Astra tasks |
| [16](16-rollout-integration.md) | P2 | 15 | Validate adoption compatibility and publish the modernized workflow |

All tasks except 01 have a transitive dependency on 01.
This intentionally follows the requested quick-win-first sequence.
After task 02, guidance, planning, and inspection work can proceed independently
where the table allows it.
These are issue dependencies, not action execution prerequisites.
Do not add them to repository.actions.depends_on.

The critical integration chain ends with comparative evaluation (15) and rollout
(16). Parent-child membership is not a replacement for these blocking edges.
Do not add an epic-to-child blocking edge that cycles with parent membership.

## Audit coverage matrix

| Audit recommendation | Owning tasks |
| --- | --- |
| Short everyday workflow and conditional diagnostics | 01, 03, 09 |
| Correct stale generated frontend gate wording | 01 |
| One authoritative verification policy; avoid redundant final tests | 03 |
| Shorten Beads reference and preserve essentials | 04 |
| Move frontend installer and narrow crate invariants | 04 |
| Make agent map optional discovery | 04, 08 |
| Replace rigid guide headings with useful checks | 05 |
| Compact plans with durable outcome and resume context | 06 |
| Add compact task discovery to existing surfaces | 07, 08, 09 |
| Expose inputs_policy and source_state | 07 |
| Complete work-tool output contracts | 10 |
| Measure and reduce large MCP schemas | 02, 10, 11, 12 |
| Prototype retained plan handles without weakening authority | 11, 12 |
| Link acceptance criteria to evidence | 13 |
| Keep setup/delegation/review proportional and conditional | 06, 14 |
| Preserve typed effects, evidence, cancellation, and compatibility | All runtime tasks |
| Preserve current minimal footprint | 01, 04, 16 |
| Compare actual Astra task outcomes | 02, 11, 15 |
| Keep source and generated consumers coherent | 01, 16 |

## Interfaces and architecture boundaries

The repository catalog remains the authority for components, targets, profiles,
runner arguments, and declared effects.
New discovery projections must reuse it.
Affected selection remains component-granular.
A candidate-target list is not proof of all required completion gates.

Work gate evaluation remains the authority for evidence freshness.
A compact summary must distinguish stale, unknown, failed, not-applicable,
and currently passing evidence.
Do not recommend rerunning checks merely because observation failed.

Execution plans remain immutable authority over source, configuration, arguments,
and effects.
A retained plan identity is an address, not authorization.
An effect acknowledgment is scoped to the exact planned operation.
Compact handles must never become an alternate route around current validation.

MCP tools keep existing names and supported requests.
New strict output contracts need an explicit compatibility/version strategy.
Task 07 owns explicit CLI projection and MCP server surface selection.
Omitted selection preserves standard shapes; agent-v1 is opt-in.
The existing server does not negotiate this through initialize capabilities.
Tool annotations must reflect actual behavior, including reconciliation writes.

Acceptance criterion evidence extends existing work/goal structures.
It does not introduce a second issue tracker.
An assertion is different from executable evidence.
New persisted fields and events require a migration/read-compatibility design
before code changes, and legacy plans retain existing closure behavior.

## Verification strategy

For planning deliverables, verify task completeness, path references, graph shape,
audit coverage, and Beads export privacy.
Use the configured work gates and report actual results.
Do not claim runtime behavior has been implemented because its plan is complete.

For implementation, every task has its own meaningful checks.
Runtime changes must use a freshly built development binary.
Run from the repository root:

```sh
cargo build -p jig-sh --bin jig
JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id PLAN_ID
JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id PLAN_ID
JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id PLAN_ID
```

PLAN_ID is the actual implementation work ID, not the planning work ID.
Backend implementation finishes with the repository-required test command until
task 03 explicitly changes that policy while preserving required coverage.
Do not run a copied placeholder identifier.

Snapshot updates use the existing supported refresh mechanism:

```sh
JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
```

Review the resulting diff; do not blindly accept unrelated regenerated changes.

The evaluation protocol must freeze prompts, grader criteria, baseline revisions,
sample count, exclusions, and trial ordering before measurement.
Use at least three paired repetitions per task family for the initial comparison.
This is a deliberately small diagnostic sample, not a statistical quality claim.
Treat every correctness or invariant regression as a finding requiring resolution.
Compare costs only when actual usage and current provider pricing are available.
No hard speedup or token-saving target is assumed.

Fixture preparation and grader smoke tests must work offline.
External model runs use configured access and explicit execution selection.
Missing model access does not authorize another provider or unbounded spending.
Record the limitation and retain the reproducible fixture work.

## Compatibility and rollout

Task 01 can ship independently.
Guidance cleanup must preserve managed-block boundaries and authored local content.
Do not require minimal adopters to acquire new files implicitly.

Tasks 03 through 10 use existing runtime and inspection authority.
Where strict output schemas change, preserve the old projection or introduce an
explicit negotiated/versioned projection.
Do not assume adding a JSON field is compatible for strict consumers.

Task 11 ends with a decision and retained experimental evidence.
Task 12 either delivers the accepted approach behind opt-in compatibility or
records why no production change is warranted.
The epic is not blocked forever by an experiment that conclusively rejects itself.

Task 13 adds optional acceptance enforcement for new plans.
Legacy records remain readable.
History remains append-only.
Any criterion policy change must be explicit and auditable.

Task 16 validates new init, full adoption, minimal adoption, update, and recopy.
Exercise Rust-only, Go, frontend, and migration-enabled consumers.
Verify examples against actual supported commands.
Retire obsolete guidance only after its replacement is discoverable.

## Recovery and idempotence

On resume, inspect the actual branch, task status, and work evidence first.
Do not rerun completed fixture mutations blindly.
Keep experimental outputs separate from implementation source.
Use generic temporary fixture roots and keep private data out of committed evidence.

Beads creation should be retried by inspecting existing task titles and parent IDs.
Do not create duplicate epics after a partial tool failure.
Add blocking edges only after both issue IDs exist.
Validate every child belongs to the single intended epic.
Export through the repository helper after mutations.

A rejected feature experiment does not justify deleting prior measurements.
A failed implementation gate does not justify editing historical receipts.
A stale run plan must be resolved again against current authority.

## Existing work coordination

The existing monorepo epic contains fixture-consolidation work.
Reuse shared fixture foundations when available without making this epic depend on
unrelated implementation progress.
The existing arbitrary-command evidence issue `jig-sh-x4n` is separate.
Acceptance linkage here references supported Jig evidence and assertions; it does
not implement arbitrary shell execution or duplicate that issue.
The Go adoption epic remains separately owned.
Task 16 tests supported Go behavior without expanding into its inference features.

## Plan review and Beads conversion

Complete at least four sequential reviews using strong reasoning agents.
Integrate each round before the next review.
Record findings, accepted/rejected changes, rationale, and convergence in reviews.md.
For each round check a standalone difficult task, the dependency graph,
five architectural decisions, and whether changes remain structural.

After convergence, create one epic and sixteen children from the linked task files.
Use br create --description-file so all sections survive conversion.
Use real blocks edges for the dependency table.
Make task 01 the only ready implementation child at initial creation.
At initial conversion, keep the epic and implementation tasks open and unclaimed.
During implementation, keep
the status and restart checkpoint synchronized with the canonical Beads records.

Perform six focused conversion checks:
1. Identity, parent membership, priorities, and first-task readiness.
2. Full description/acceptance preservation.
3. Blocking edges and cycle detection.
4. Audit recommendation coverage and absence of duplicate ownership.
5. Cross-task interface and compatibility consistency.
6. Export privacy, graph stability, and final handoff references.

These checks are verification passes, not claims that six external models ran.

## Source grounding

Repository facts above were inspected at the pinned baseline.
The model guidance was fetched during the September 11 audit:
[OpenAI GPT-6 Astra guidance](https://developers.openai.com/api/docs/guides/latest-model).

The relevant documented behaviors are increased sensitivity to instructions,
a tendency to clarify consequential ambiguity, and potentially excessive testing
for small changes.
Those motivate an experiment; they do not prove a particular Jig change is faster.
Do not copy a whole model prompting guide into AGENTS.md.

Related repository documents:
- [Agent-native repository model](../../agent-native-repository-model.md)
- [Public contract](../../public-contract.md)
- [Adoption](../../adoption.md)
- [Target freshness integration](../../target-freshness-integration.md)
- [Task specifications](01-guidance-quick-wins.md)

## Handoff correction

The separate-worktree handoff was a mistake. The plan and epic now live in the main
jig-sh checkout on feat/astra-harness-modernization, created from updated
origin/master at dc68b744. Plain br uses the canonical database; no BEADS_DIR
override is needed. Existing implementation edits were preserved.
The audit baseline remains the evaluation control. It differs from the branch-base
tree, including the upstream prompt-library removal. Original work records retain
their captured baseline, so their comparisons include those intervening changes;
they are not isolated feature-only comparisons. Future work captures its own start.
The abandoned worktree's journals were not imported. Planning reviews and conversion
checks are documented in reviews.md; local reconciliation and follow-up verification
use plan_01M28626D98WHJJFH0432ZBEKH. Task 01's initial implementation has its own
closed local work record. Earlier evidence covers only its recorded source state.
