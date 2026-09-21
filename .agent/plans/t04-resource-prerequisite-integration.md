# T-04: Explicit browser endpoint ownership and current readiness

## Outcome and scope

Implement `jig-sh-ndz2.4` using the approved decision in
`docs/plans/agent-workflow-velocity-measurements.md`. Cargo coordination is
delivered by V06 (`d966205e`); reuse its claims, admission, deadlines, cancellation,
cleanup and publication. Add only the demonstrated generated-Playwright endpoint
ownership policy. Preserve SQLx 0.9's existing per-invocation connection guard.

No second Cargo coordinator, global browser limit, inferred shell ownership,
automatic action enrollment, schema privilege probe, database creation, package
installation, or live benchmark rerun. T-03's three-repetition budget is exhausted.

## Progress

- [x] Verify clean checkout and unblocked T-04 after V06 closure/commit.
- [x] Record the versioned opt-in boundary and compatibility matrix before code.
- [x] Implement policy resolution through the existing resource owner.
- [x] Prove endpoint contention and independent overlap with synchronized fixtures.
- [x] Finish focused readoption/readiness-preservation and Cargo regression evidence.
- [x] Run required gates, stage and native-review all uncommitted changes.
- [ ] Finish structured work, close/sync task, commit and verify clean checkout.

Structured work: `plan_01M31Q8R4TKAW36Z4T4H9909RP`.
Exact baseline: `d966205e3ae7377ee340f2f03b93a5116c756f94`.
Restart: policy, generic dispatcher and runtime integration are implemented.
Strict contract tests (5), planner tests (9), resolver/template-parity tests (4),
and real CLI browser ownership tests (10) pass. Authored readoption, public npm
checker E2E environment and generated SQLx command preservation tests pass.
Existing Cargo process/reuse regression targets pass all15 tests. The additional
mutable-readiness regression passed and the complete browser target passed11/11
(49.574 seconds). Source is frozen for required final validation and native review.
All six required gates now pass. Full backend run
`run_01M31R8NZWFTVZZEVVS9KC77DE` completed successfully (1341.079 seconds),
receipt `receipt_01M31SHN7EP9GRXCD1JFT6KRPK`. The other five gates passed in
`run_01M31R8QTZH6KW0GZ0XAKF6S5A`. Work check reused all six current passes
(`receipt_01M31SKYXZ6AG8KJBHNC92K9KE`), and evidence/gates report fresh passes.
Native Codex review of all uncommitted changes completed with no actionable
defects. Its independent focused resolver run passed all four tests. Next:
finish structured work before tracker closure/export, then commit and verify clean.

## Decision Log

2026-09-21: Add the strict fieldless `playwright_servers_v1` variant to the
existing opt-in `ExecutionResourceV1` declaration. This explicitly attests that
the authored runner follows the generated Playwright environment contract. It
is not discovery or validation of arbitrary Playwright config or shell text.
Existing strict readers reject the unsupported tag; omitted resources and Cargo
declarations retain behavior and authority. Do not change the contract epoch or
released source-runtime pin. Readoption must preserve the complete authored
action and its public `scripts/check-webapps.sh run-script APP test:e2e` runner.

2026-09-21: Resolve `E2E_WEB_PORT` and `E2E_API_PORT` using Node's exact JavaScript
`trim`, `Number` and integer semantics, defaulting to4173/4174. Validate distinct
positive integers no greater than65535, even in external-URL mode, matching the
generated template. A bounded literal Node probe uses the runner's effective
environment/cwd; no shell interpolation or package loading. Unavailable Node,
invalid ports or failed supervision stop admission without running the target.
Never expose URL values or raw probe output in durable diagnostics.

2026-09-21: A trimmed nonempty `E2E_BASE_URL` owns no generated endpoints.
Otherwise acquire two exclusive, role-independent opaque claims for
`127.0.0.1:port`. Claims are individual endpoints, not ordered pairs or repository
IDs, so shared and swapped roles conflict across requests/repositories. Distinct
pairs overlap within one run and across requests. Browser-only resolution performs
no Cargo metadata and adds no repository fallback; explicit Cargo declarations
remain independently supported without forking Cargo identity. The browser policy
does not coordinate its wrapper's internal backend compilation.

2026-09-21: Keep existing read-only process-check/effect requirements. Declaration
is no permission to mutate source or external systems; authors must retain
truthful effects and ordinary approval rules. Do not auto-classify arbitrary E2E
scripts or make effectful actions eligible for concurrent read-only execution.

2026-09-21: Browser-bearing targets do not reuse source evidence after a resource
wait: current execution must still run its live validator. Existing ordinary
work-check reuse policy outside admission is unchanged; source freshness is not
a claim of present database or external-server readiness. Explicit execution
always runs the owning validator after admission. SQLx invocation remains the
existing `sqlx prepare --check --workspace -- --workspace --all-targets` wrapper
with its configured offline/cache environment; no duplicate generic preflight.

## Compatibility and acceptance matrix

- Old actions without resources serialize identically and run unchanged.
- Strict browser tag rejects unknown fields; duplicate declarations and typed
  Rust-runner misuse fail planning. Existing Cargo declarations remain valid.
- Authored browser policy, literal wrapper/args and environment round-trip and
  survive readoption; no generated default or CI invocation is rewritten.
- Port parsing matches generated JavaScript, including whitespace, hexadecimal,
  exponent and integral decimal spellings; invalid/equal ports start no child.
- External URL with surrounding whitespace owns no endpoints; blank URL owns
  local endpoints. No URL text enters diagnostics or public identity.
- Same, partial-overlap and swapped endpoint pairs serialize. Distinct pairs
  overlap, including same-run waves and independent requests.
- Cancellation/timeout while waiting starts no child; source edits reject queued
  authority. Existing close-only leases retain crash ownership and publication.
- Existing SQLx connection guard is invoked on each actual validation after
  admission. Prior success never replaces a current explicit invocation; no
  claim about unmeasured query/schema privileges or remote service availability.
- Package-script execution keeps existing ambient npm routing neutralization and
  explicit E2E environment. Use the existing public checker boundary.

## Context and implementation

Contract: `crates/jig-contract/src/resources.rs`; declaration validation:
`crates/jig/src/repository/planner/resources.rs`. Generalize the resolved-resource
container/dispatcher while preserving `repository/cargo_resources.rs` identity
logic. Add a narrow browser resolver. Runtime resource/wave/alias entrypoints
continue through the same owner, with generic diagnostics and browser reuse
exclusion. Avoid changes to the whole executor or its admission algorithm.

Generated ownership authority:
`templates/scaffolds/rust-react/frontend/vite-react/playwright.config.ts.jinja`.
It validates ports before external URL selection and owns two local servers only
without an external URL. There is no generated native E2E action today; document
authored opt-in through the existing wrapper rather than inventing auto-enrollment.

Delegate independent tests or evidence inspection with non-overlapping file
ownership. Main owns contract/resolver/runtime integration and final validation.

## Validation and recovery

Use generic isolated marker/barrier fixtures, real CLI execution and the existing
lease owner. Use real Node for parsing equivalence; do not require a browser or
live database for deterministic regression tests. Preserve recorded T-03 live
SQLx evidence rather than rerunning exhausted measurement matrices. Distinguish
wrapper-invocation assertions from actual database privilege proof.

Run focused contract/planner/resolver and process tests. Then stage complete
source and run `scripts/jig check test --plan-id PLAN` and all other required
gates, `scripts/jig work check`, evidence/gates/receipts inspection, followed by
native `codex review --uncommitted`. No review-fix-loop controller. Every finding
goes into the external workflow ledger with root cause/disposition. Preserve
the user's architectural escalation and non-convergence stopping rules.

Finish work before tracker closure/export so task-tracker mutation cannot stale
the finish proof. Commit all task changes and verify a clean checkout. Rollback
is removal of opt-in configuration; never rewrite historical evidence or delete
live lock files. Unsupported/ambiguous authority blocks rather than guessing.

## Surprises & Discoveries

No generated Jig E2E action exists; package scripts and CI already enter through
the public webapp checker. A whole-run backend Cargo claim would serialize
otherwise independent browser endpoint pairs and is not required by T-03.

## Outcomes & Retrospective

Initial focused validation: five contract tests, four resolver tests and ten
CLI process tests passed. The latter ran in54.043 seconds with no skips; fixtures
use Node marker processes, not actual browsers or databases. They prove selected
admission/ownership behavior, not browser readiness or database privileges.
The resolver comparison executes the generated template's own parsing functions.
The additional readiness test verifies wrapper launches publisher/waiter/repaired,
expensive launches publisher/repaired, receipts0/42/0, and no reused evidence.
After the owner publishes a valid success, the queued wrapper observes a revoked
external fixture prerequisite before expensive work; repair permits a fresh pass.
This is a generic guard model. The SQLx template and its per-invocation guard are
unchanged; its generation regression passes, and live behavior remains supported
by T-03's already-recorded bounded evidence, not a new database experiment.
V06 prerequisite's4488 passing tests do not certify this new browser policy;
required final validation and native review now pass. No new T-04 native review
findings. No live browser/database or macOS qualification is claimed here.
