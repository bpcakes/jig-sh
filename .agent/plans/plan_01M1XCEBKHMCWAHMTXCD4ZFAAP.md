Audit every active issue against master, compare closed history for duplicate and completed scope, update canonical issue scope and acceptance criteria, encode actual dependencies, and validate the resulting Beads graph and export. Documentation and issue metadata only; no runtime implementation.

Baseline: `4ef2e90280d053a6fc5a38477429dd3c0a61de44`.

The [per-issue audit](../notes/beads-audit-2026-09-07.md) records every original active issue and its current source evidence. Applied all 69 dispositions through `br`: 15 duplicate closures, nine already-addressed closures, and 45 updated implementation contracts. The retained backlog contains 27 ready implementation items, five waiting on local dependencies, four tracking parents, and nine deferred Jury cutover records. Previously closed records remain unchanged.

Read-only inspection of Jury's committed cutover plan confirmed its explicit post-0.x activation requirement. Preserved the deferred migration, TUI, platform, support-window and reverse-dependency prerequisites instead of presenting these as immediately claimable work.

Validation completed before repository gates:

- Every original issue ID remains present; all 69 original active issues have exactly one disposition.
- All 68 previously closed issue records remain unchanged.
- Retained descriptions and acceptance fields match the applied updates.
- All 167 source links in the audit report resolve.
- `br lint --status open --json` and `br lint --status deferred --json` report zero warnings.
- `br dep cycles --json` reports no cycles.
- Triage against the current export agrees with the live ready queue; AGENTS.md now explains how to avoid stale legacy export selection.
- `git diff --check` passes.

The configured `work check` also runs the repository-wide `verify` profile. Its original receipts and final gate status belong to this plan; they are separate from the source-based issue audit and do not imply that every historical flake was freshly reproduced.

The required profile completed with formatting, Clippy, contract and file-budget targets passing. `api:test` failed in `runtime::tests::loops::scheduled_dispatch_ignores_worker_forged_checkout_schedule_replica` after its one-second occurrence lease expired. The test source is unchanged by the audit. Nextest reported 2,797 passed, one failed, two skipped, and 1,054 not run after fail-fast. Original failure receipt: `receipt_01M1XDTZNDAQKRJ28HTPPQV5WG`, run `run_01M1XD7MQZM0EBE9AM6AJ1TCA6`.

The fresh failure is now recorded in `jig-sh-7o6`, including its protected-ledger and no-duplicate-worker acceptance obligations. That follow-up metadata update occurred after the failed profile, so the profile receipts describe their original source fingerprint. The audit must not claim a passing `verify` gate or waive it to close this plan.

The exact failing test passed in a focused package-library run with one test thread (8.895 seconds). Command: `cargo nextest run -p jig-sh --lib -E 'test(=runtime::tests::loops::scheduled_dispatch_ignores_worker_forged_checkout_schedule_replica)' --test-threads 1`. This unreceipted focused pass does not replace the failed profile or close the timing issue.

The audit Bead `jig-sh-y03` is closed for completed metadata scope. `work finish` correctly rejected plan closure because `verify` failed; no gate was waived. Final checks confirmed 167 source links, zero open/deferred lint warnings, unchanged historical records, and append-only state changes. All edits remain uncommitted.
