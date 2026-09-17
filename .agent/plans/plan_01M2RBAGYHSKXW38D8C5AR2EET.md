# Reuse native target evidence across work plans

A follow-up work plan should reuse a check that already proved the same current inputs, instead of executing it merely because the plan ID changed. This work implements repository-local reuse of original target receipts for contracts with scoped freshness (epoch 8 and supported successors). Legacy checks and reviews retain their existing selection rules. Maintain this plan under `.agent/PLANS.md`.

## Progress

- [x] Inspect receipt selection, original proof validation, prepared native authority and archive protection.
- [x] Create `feat/cross-plan-native-evidence` from `origin/master` at `a5e20d7c34ef8d0abd915349d0ef927d5b6d27a2`.
- [x] Open Bead `jig-sh-lluc` and Jig plan `plan_01M2RBAGYHSKXW38D8C5AR2EET` at that baseline.
- [x] Implement shared repository-wide target selection, original provenance validation and archive protection, excluding plan-bound native closures.
- [x] Verify live cross-plan reuse, newer blockers, changed inputs, dependency provenance, legacy isolation and native baseline sensitivity.
- [x] Update public behavior documentation; run configured gates and final backend verification; close structured work and Bead.

Restart checkpoint: completed on `feat/cross-plan-native-evidence`. All required checks passed, Jig plan `plan_01M2RBAGYHSKXW38D8C5AR2EET` and Bead `jig-sh-lluc` are closed. Changes remain local and uncommitted. The previous feature branch and its unpushed repair commit remain intact.

## Surprises & Discoveries

`OriginalReceiptIndex::open` builds a global latest map but deliberately does not enforce latest selection for historical proof readers. Work reuse needs a distinct mode with a global latest-outcome race guard. Review discovered that global selection of plan-bound native checks would cause plans to invalidate one another; preserve local selection for native closures. Archive protection previously ignored closed plans; reuse requires retaining selected originals and blockers even between work plans. Prepared native file-budget authority includes the original work-plan ID and comparison baseline; retain that conservative identity in this feature.

## Decision Log

2026-09-17: Select the newest plan-independent target outcome across the repository, ordered by `(ended_at_ms, receipt_id)`, before validating it. Never search backward for an older pass when a newer receipt fails, expires, lacks metadata or has different authority. This conservative rule preserves existing newest-outcome semantics while widening eligible plan provenance.

2026-09-17: Reuse whole original target proofs. Validate dependency references against their original execution plan, not the consuming plan. Do not seed foreign dependency receipts into new executions or change persisted proof schemas. Existing scheduling reruns prerequisites when needed.

2026-09-17: Use the existing plan-associated work-check validation receipt as the durable binding: it references original receipt/run IDs and reports each original plan ID. Do not fabricate target execution receipts. Read-only gate inspection may report compatible evidence directly, and finish independently revalidates it.

2026-09-17: Keep native prepared input, repository-wide configuration identity, source authority, expiry, cancellation and finish guards unchanged. No cross-repository/worktree cache or general configuration-invalidation redesign.

## Outcomes & Retrospective

Implemented repository-local reuse for plan-independent targets, with original provenance and conservative native closures. Focused evidence tests passed (60), proof tests passed (19), and receipt/archive tests passed (36). The initial configured run passed 4,206 tests (3 skipped), formatting, contract and file budgets; Clippy flagged one new test at complexity 21/20. Extracted receipt assertions/filtering into small helpers and reran the non-test checks successfully. The final full test run passed all 4,206 tests with 3 skipped. The final work check reused all five original target receipts and launched zero checks. Gates/evidence inspection passed with the supported 30,000 ms freshness budget; the default 2,000 ms inspection budget had expired (read-only unknown, not a failed check). Work finish independently revalidated the gates and closed successfully.

## Context and milestones

`state/receipts/dashboard.rs` reduces receipt history for both individual work queries and dashboard reports. Before epoch 8, target selection stays plan-local. At epoch 8+, share repository-wide newest target outcomes while leaving check/review receipts plan-local. `runtime/work/gates/scoped_freshness.rs` compares these originals to the consuming plan's current default invocations. `repository/freshness/proof.rs` resolves exact originals and dependency references under bounded journal reads; it must validate historical provenance separately from consumption.

Native runners and their transitive dependents retain plan-local selection. This avoids two open plans repeatedly invalidating each other when their native prepared inputs contain different plan IDs. A single repository helper computes this eligibility for gate indexes, proof lookup and archive protection.

First deliver shared selection and proof lookup with deterministic newest-outcome and journal-race guards. Then expose original plan provenance in gate/work-check results and protect global selected receipts and dependency originals during explicit archive maintenance, including when the source plan is closed and no plan is open. Keep the old archive behavior for legacy contracts.

Finally add behavioral tests in `runtime/tests/work/evidence/scoped_freshness/cross_plan.rs` and focused proof/index tests. A live second plan on unchanged authority must launch no check, retain original IDs, and write only its ordinary validation receipt. Relevant edits must execute the affected checks; unrelated exhaustive-input edits may reuse them. Newer failed/expired/unsupported/unverifiable outcomes must block older passes across plans. Native file-budget authority from a different plan must not be promoted, including through dependent proofs. Archive and dashboard inspection must agree with direct gates.

## Validation and acceptance

Run focused Rust tests using `cargo test -p jig-sh --lib` with filters for the new cross-plan tests and existing freshness/proof/archive owners. Test live CLI/runtime work-check results and MCP gate inspection through existing fixtures, not just synthetic passing metadata. Preserve original bytes and receipt provenance in assertions. Confirm legacy epoch 6 fixtures remain plan-local.

After runtime edits, run `cargo build -p jig-sh --bin jig`, then `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M2RBAGYHSKXW38D8C5AR2EET`. Inspect `work gates`, `work evidence`, and `work receipts` for that plan. Finish backend verification with `JIG_DEV_BIN=target/debug/jig scripts/jig check test` as required by repository guidance. Run `python3 scripts/beads-sync.py --check` and `git diff --check`. Record actual results here; do not treat this plan as validation evidence.

## Compatibility and recovery

No receipt history is rewritten, no new strict persisted fields or contract epoch are introduced, and no existing run/proof fields change meaning. Additive response provenance belongs to the read model and extensible batch evidence. Older runtimes continue using their own plan-local selection and may rerun checks; they cannot infer a successful target execution from a newer validation batch. Downgrade therefore costs reuse without falsely passing gates. Explicit archive maintenance preserves newest configured target outcomes and recursively required originals. Legacy metadata never grants cross-plan freshness.

Retrying work-check re-evaluates current authority and references existing originals where valid. Failures and cancellation leave work open; source/configuration changes prevent a stale pass. Finish retains its independent authority and expiry checks. All source edits are reversible; repository state journals remain append-only.

Revision: refined shared selection to exclude native closures after review demonstrated repeated cross-plan invalidation; original native execution authority remains unchanged.

The living plan is stored in the structured work plan body at `.agent/plans/plan_01M2RBAGYHSKXW38D8C5AR2EET.md`, so verification outcomes can be recorded as workflow metadata.

Verification logs: `/tmp/jig-cross-plan-proof-final.log` (19 proof tests), `/tmp/jig-cross-plan-evidence.log` (60 evidence tests), `/tmp/jig-cross-plan-receipts.log` (36 receipt tests), `/tmp/jig-cross-plan-tests.log` (10 cross-plan-filtered tests), `/tmp/jig-cross-plan-gates.log` (initial gates), `/tmp/jig-cross-plan-final-checks.log` (final four non-test gates). Final review of native eligibility, selection races and archive agreement found no remaining actionable issue.

Final evidence: `/tmp/jig-cross-plan-final-test.log`, `/tmp/jig-cross-plan-final-work-check.log`, `/tmp/jig-cross-plan-final-gates.json`, `/tmp/jig-cross-plan-final-evidence.json`, `/tmp/jig-cross-plan-final-receipts.json`, and `/tmp/jig-cross-plan-finish.log`. No unresolved implementation findings remain. Native runners and their transitive dependents intentionally retain plan-local evidence, and cross-worktree reuse remains outside this scope.


## PR #41 follow-up: default inspection budget and finish revalidation

2026-09-17: Compared base `a5e20d7c` and PR head `9e071c9c` using the same toolchain/debug profile, checkout, and 28,017,614-byte journal containing 7,511 receipts. Ran four sequential alternating-order base/head pairs for each of `work gates` and `work evidence`, both at the default budget and with `--freshness-timeout-ms 30000`, without concurrent builds/checks. Hashes of tracked contents (including journals) matched before/after, and selected receipt IDs matched in every pair.

| Command | Default-budget median wall time, base/head | Expanded-budget median wall time, base/head | Expanded-budget median freshness time, base/head |
| --- | --- | --- | --- |
| work gates | 2,615.5 / 2,628.1 ms | 3,334.0 / 3,366.2 ms | 2,721.7 / 2,737.4 ms |
| work evidence | 2,613.4 / 2,631.2 ms | 3,328.8 / 3,348.4 ms | 2,714.5 / 2,719.3 ms |

Both revisions exhausted the default deadline in all eight inspections each. All expanded-budget inspections completed without a collection limit and classified the originals as stale against the current checkout; completion is not a passing gate. Original-proof work accounted for roughly 1.5 seconds and worktree collection roughly 0.8 seconds on both revisions. This small, local debug-profile sample demonstrates an existing default-budget limitation, not a regression attributable to this PR; it does not establish release-build, cold-cache, or tail-latency behavior. Track optimization and representative valid-evidence/release measurements in Bead `jig-sh-9y3b`. Keep fail-closed behavior and resource ceilings intact.

Added `cross_plan_finish_rejects_newer_foreign_failure_after_successful_reuse`: plan B successfully reuses plan A's original, a newer failure from another plan is appended, and finishing B must fail while B remains open and its plan journal is unchanged. The gate report must identify the newer failing receipt. This joins the existing reuse and blocker cases as additional workflow assurance; no production defect was demonstrated.

Follow-up validation: the focused finish regression passed; the rebuilt head binary passed `scripts/jig check api:fmt api:clippy repo:file-budget`; final `scripts/jig check test` passed all 4,207 tests with 3 skipped. Beads export privacy and diff whitespace checks passed. Benchmark measurements and raw reports are retained locally under `/tmp/jig-evidence-perf/`; no production behavior or budget was changed.
