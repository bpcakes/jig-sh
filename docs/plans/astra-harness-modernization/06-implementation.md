# Task 06 implementation and acceptance

Bead: `jig-sh-9wcn.6`. Work record: `plan_01M2AZK2RASETQ4SHFVA7CA109`.
Implementation baseline: `2c60f6354134a8c49c9d15cb501a3322421b1793`.
Changes are in the working tree; no implementation commit is claimed.

The [revised task](06-compact-execplans.md) records the September 12 audit and its
official sources. OpenAI's current Astra guidance informed the instruction audit,
completion boundaries, and proportional verification. The improve-exec-plan review
identified the existing supplied-checkpoint behavior and missing JSON-path coverage.
No model configuration or API integration changed, and no comparative model result
is claimed; task 15 owns that evaluation.

Root planning guidance, the shipped project template, and its embedded snapshot now
require outcome, scope, acceptance, durable decisions, restart context, and observed
evidence without fixed headings or a section count. Detailed plans remain valid, and
compatibility-sensitive work still needs rollout and recovery. Delegation and optional
skills can be omitted; configured checks and review gates retain their authority.
The [compact and extended examples](06-plan-examples.md) cover the same actual change
and distinguish expected checks from observed evidence.

New goal requests use the normalized success condition as the fallback checkpoint.
Supplied checkpoints keep their content and order. The body and prompt preserve
planning-only scope, explicit approvals, continuation of independent authorized work,
current evidence reuse, and worktree reconciliation on resume. Existing request fields,
response keys, title normalization, mandatory validations, and persistence are unchanged.
Historical plans and journals are not rewritten.

| Acceptance | Evidence |
| --- | --- |
| Compact plans retain required information | Manual review of root/template/snapshot and both examples |
| Rollout and recovery remain required when relevant | Retained Verification and Recovery policy; extended example states the compatibility boundary |
| User checkpoints, constraints, notes, success, and validations survive | Existing CLI dispatch tests plus JSON planning/approval regression |
| Outcome checkpoint replaces five process defaults | CLI no-checkpoint assertion and JSON omitted/null-list regression |
| Invalid supplied values cannot open work | Existing CLI rejection cases and JSON invalid-contract regression |
| Authorization, evidence, and resume instructions remain explicit | Generated-body/prompt regression and manual instruction review |
| Source template and packaged copy match | Byte comparison plus existing embedded-template snapshot test in the configured suite |

Verification status: dev build, byte comparisons, diff whitespace check, and Beads export
privacy check passed. Initial configured run `run_01M2AZT0J9AH3ERQTNJDTZ58GC` failed:
the new rejection test assumed the plans journal was absent, but context loading creates
an empty journal. It now compares journal bytes before and after the rejected request.
The focused goal selection then passed all 12 tests. The initial workspace selection
ran 3,142 tests: 3,141 passed, that one assertion failed, and 947 were not run because
of fail-fast (three configured skips). All 42 offline harness tests passed.

The initial group's otherwise successful commands were rejected because handoff docs
changed while its read-only layer ran. The worktree must stay unchanged throughout
the retry. The first read-only gate inspection also exhausted its default 2000 ms
collection budget; the suggested 30000 ms retry completed. No gate or freshness
policy was weakened.

Stable-worktree run `run_01M2B1ATERGKGCNF1Y9M98KS0F` passed all six targets: Clippy,
formatting, 4,089 Rust tests (five slow; three configured skips), contract validation,
file budgets, and 42 offline harness tests. Its validation receipt is
`receipt_01M2B2P0WH7SJ3HTB2YGPQEK2R`. File budgets report a warning for the existing
large work test module, with no errors. The review found no additional code changes
needed after correcting the rejection-test oracle.

Implementation acceptance is verified. The
[work journal](../../../.agent/plans/plan_01M2AZK2RASETQ4SHFVA7CA109.md) records
current evidence and final closure, including any refresh required after updating
these handoff documents and Beads. On resume, inspect that journal and work gates;
do not repeat completed implementation. Plain
`br` resolves the worktree database imported from this branch's tracked export; the
older main-checkout database did not contain task 06.

Post-review follow-up: the user authorized restoring the explicit blocked exit for
unsatisfiable goals and correcting the completed journal's stale restart state.
The [repair work record](../../../.agent/plans/plan_01M2B5C1NTCDTG3ZZJ03Z8ZT19.md)
records regression results, current required-gate evidence, and repair closure.
The runs above describe the original implementation, not proof of these later edits.

A subsequent independent Claude/Codex/Cursor review found no actionable runtime
defects in the repaired diff. Its remaining findings concerned contradictory Beads
database instructions and a staging statement that had become historical. The
[handoff follow-up record](../../../.agent/plans/plan_01M2B7MN8WX4RXXBENXT4NVE66.md)
records their correction and final verification. Task 06's Beads notes now point
directly to the runtime repair evidence without rewriting the original close reason.
