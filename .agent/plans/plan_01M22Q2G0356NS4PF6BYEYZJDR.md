# Flatten unreleased contract epochs

## Purpose

Release v0.3.0 shipped repository contract v7. The committed bounded-string feature introduced v8, and the staged argv/shell feature introduced v9. Combine these unreleased changes into v8 so users upgrade once from released v7 to a complete argument-and-runner contract. Follow .agent/PLANS.md. This is issue jig-sh-qvi and a follow-up to the completed runner plan.

## Progress

- [x] 2026-09-09: Verified the v0.3.0 tag declares contract v7; identified unreleased v8/v9 split and opened structured work.
- [x] 2026-09-09: Unified bounded arguments and explicit process runners at repository::ACTION_EXECUTION_CONTRACT_VERSION = 8; updated current fixtures and removed intermediate compatibility expectations.
- [x] 2026-09-09: Updated source, launcher, changelog and public documentation; audited other changed schema families against the release baseline.
- [x] 2026-09-09: Fixed check work-plan propagation after reproducing the native file-budget failure; all 25 focused tests pass. Built the dev runtime and verified plan-bound native execution through the CLI.
- [x] 2026-09-09: Staged the reviewed source and refreshed all seven tooling/partition gates; all seven now pass with fresh accepted receipts.
- [x] 2026-09-09: Final plan-bound backend check passed all five verification targets; workspace tests passed 3,925 with 2 skipped. All eight required gates have fresh successful evidence.
- [x] 2026-09-09: Closed structured work with outcome success (receipt_01M22TZAHSFB0HWQ44BWM585VS) and closed issue jig-sh-qvi.

## Surprises & Discoveries

The existing staged changes belong to the previous completed task and must be preserved. Tests from the argument-only commit intentionally used Command under v8; flattening requires converting those current-version fixtures to Argv or Shell, while keeping actual v6/v7 fixtures unchanged. The release already shipped file-budget v7, so v7 must remain a supported compatibility epoch.

All seven tooling/partition checks passed, but gate attestation rejected the partially staged source: the index held v9 while the worktree held v8. Stage the reviewed flattened inputs before refreshing evidence. The final CLI check also exposed a separate planning omission: check --plan-id passed its work ID to execution but used None while preparing native file-budget authority. The existing foreground-run identity regression must cover both run and check so it reproduces this native-path failure before the small propagation fix.

## Decision Log

2026-09-09: Keep v2-v7 runtime compatibility and make v8 the single next contract. Remove any claim that argument-only v8 was released or needs an independent compatibility path. The renderer must migrate released Command sources to explicit Shell in v8 and preserve authored Argv/Shell on recopy. State JSONL records remain append-only; historical receipts are evidence of earlier work and are not rewritten. Do not rewrite commits. Preserve existing staged edits while updating the changed source paths to the reviewed flattened versions, as required for real gate attestation.

2026-09-09: Repair check work-plan propagation because it blocks the required final check from producing valid plan-bound native evidence. Preserve the existing admission guard; supply the requested identity during planning rather than weakening execution validation. No schema change is needed for this correction.

## Outcomes & Retrospective

The source and runtime now use the single combined v8 contract. All 25 focused regression tests passed, including literal argv capture, update/recopy from released v6/v7, native migration compatibility, file-budget authority, check/run work-plan identity and source/launcher epoch agreement. The rebuilt runtime accepts v8 and rejects v9 with support bounded to v2-v8; the installed v2-v7 runtime rejects v8. Source contract and native file-budget checks pass. All seven tooling/partition gates passed with fresh evidence: fast-gate batch receipt_01M22S0PDCT3R0RXCB50REB0M1 and partition batch receipt_01M22T0B3HGEVTVT8HAFQ7ZP4P. Core passed 3,158 tests, frontend 112, vault 443 plus 2 terminal tests, and process 210. The final command `JIG_DEV_BIN=target/debug/jig scripts/jig check --plan-id plan_01M22Q2G0356NS4PF6BYEYZJDR test` exited 0: all five targets passed, with 3,925 workspace tests passed and 2 skipped. Verification run run_01M22T1A9C9FZEZR21FP5GBAND supplies the final profile evidence; work gates and work evidence both report all eight gates passed and fresh.

The other changed schema families do not contain redundant unreleased versions: status JSON moves from released v1 to v2, info commands moves from released v3 to v4, and the new recorder schema starts at v1. Run-plan v3, scheduler v4 and protected-schedule v2 were already present in v0.3.0. No changes to those version numbers or historical state records are required.

## Context and Orientation

crates/jig/src/context.rs bounds supported repository epochs. repository/arguments.rs and repository/runners.rs validate their feature epoch; these must share one v8 boundary. bootstrap/renderer_tests.rs, bootstrap/tests/template_mode/migration_arguments.rs, repository/planner/tests.rs and runtime/tests/mcp/action_arguments.rs contain the version-sensitive fixtures. .agent/jig-contract.json and scripts/jig declare the source contract. README.md and docs/public-contract.md describe the released compatibility surface.

crates/jig/src/runtime.rs::dispatch_repository_check_with_catalog must forward ToolRequest's work-plan ID into PlanRunRequest before native authority is prepared. runtime/tests/mcp/foreground_run.rs exercises both run/check entrypoints with command and native actions, asserting the prepared ID, durable run ID and receipt plan ID agree.

## Plan of Work

Set the latest contract to 8 and make runner validation use the same v8 boundary as argument declarations. Convert current-version test actions to literal argv or explicit shell, preserving v6/v7 command migration tests. Prove v8 jointly supports declared arguments and literal execution, rejects implicit Command, and renders source/manifest/launcher consistently. Replace v9 public documentation with one v8 description containing both features and remove argument-only compatibility prose. Inspect other schema-version changes since v0.3.0; retain independently versioned formats that have only one unreleased change.

## Concrete Steps and Validation

Run focused cargo tests for arguments, runner epoch validation, renderer, planner authority, migration update/recopy, schema snapshots and UI source/launcher epoch agreement. Build with cargo build -p jig-sh --bin jig. Use JIG_DEV_BIN=target/debug/jig for harness commands. Force the seven configured tooling/partition gates with scripts/jig work check --plan-id plan_01M22Q2G0356NS4PF6BYEYZJDR and explicit --gate flags, then run scripts/jig check --plan-id plan_01M22Q2G0356NS4PF6BYEYZJDR test as the required final backend suite and verification-profile evidence. Inspect work gates/evidence/receipts/status and finish only when all required gates have current successful evidence. Run git diff --check and verify historical JSONL prefixes are unchanged.

## Idempotence and Recovery

Use only generic isolated fixtures. Preserve staged work and append-only state. If a test fails, fix the demonstrated fixture or behavior, rerun the relevant focused test, rebuild after runtime edits, and rerun only evidence invalidated by the edit. Poll live test handles until authoritative completion; do not restart on an observation timeout.

## Interfaces and Dependencies

No new DTO, dependency, durable-state format, migration shim, or compatibility alias is needed. Both feature validators must use one constant for the combined v8 contract. Old runtimes that support only v7 must reject the new v8 source before execution. Historical recorded v9 runs remain historical records, not a supported repository schema.
