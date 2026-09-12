# Repair guide compatibility, evaluation observations, and receipt privacy

This plan follows `.agent/PLANS.md`. Users upgrading Jig should retain legacy guide-check behavior until adopting contract epoch 8. Evaluation runs must distinguish unavailable metrics and unresolved process cleanup from valid completed observations. Public receipt previews must omit host-specific prefixes.

## Progress

- [x] Locate reviewed revision and inspect findings: clean branch fast-forwarded from d89129a8 to 8b43c0df.
- [x] Confirm all five findings and identify compatible fixes.
- [x] Restore legacy guide validation and cover missing, ignored, and symlinked references.
- [x] Record cleanup failures durably, supervise successful graders, and qualify repeated-check metrics.
- [x] Redact affected receipt prefixes, append a migration decision, and sanitize future previews.
- [ ] Run focused regressions, configured work gates, and backend test verification; finish structured work.

Checkpoint: implementation in progress at baseline 8b43c0df. The guide and Python changes are independently delegated under the repository planning instructions; the main agent owns privacy and integration. Initial dev build failed in native dependency compilation; a bounded four-job retry passed. No build/toolchain failure was reproduced on that retry. Structured plan: `plan_01M2AT6FARSY5QR7W3NJ37W47Q`, baseline 8b43c0df. Focused guide tests (13 unit, 7 CLI) and five Python regressions passed. Clippy, formatting, contract, and file-budget gates passed; full Rust and Python gate retries are pending because a progress-note edit invalidated their global worktree invariant. Privacy decision: `decision_01M2AT6ZD613ST0FKH347AASZD`.

## Surprises & Discoveries

The provided checkout preceded both reviewed commits. Fetching the existing tracking branch supplied the cited files. Contract epochs 6 and 7 are still accepted, so the compatibility guard must cover all epochs below 8. The same temporary-prefix leak occurs in two receipts before the cited Python traceback.

The branch's state streams append work plans, sessions, runs, and verification receipts. Beads changes add the modernization task graph and update task completion. This matches the repository's documented dogfooding workflow; preserve that history except for the explicit privacy migration.

## Decision Log

On 2026-09-12, preserve the previous checker for epochs 2–7 and use strict references at epoch 8. This stages the blocking policy at adoption instead of changing previously valid repositories merely by updating the binary.

On 2026-09-12, require a versioned exhaustive event-classification contract before reporting repeated checks. The adapter must identify checks and nonchecks, provide stable check keys and source digests, and explicitly identify whether its trace is complete. Partial measurements remain partial; unclassified events cannot imply zero checks.

On 2026-09-12, treat unresolved cleanup as an excluded terminal trial with its execution workspace retained. Persist failure before reporting it at the CLI boundary; resume must never grade, release, or replay that workspace automatically. Completed probes must retire their process group before reaping the leader.

On 2026-09-12, limit historical migration to affected preview substrings. Preserve IDs and unaffected fields, append a durable decision identifying all changed records, and retain diagnostic file suffixes. Future output previews redact the current home and temporary roots and standard Homebrew prefixes after repository-root redaction and before truncation.

## Outcomes & Retrospective

All five reported issues are implemented. Guide unit tests passed (13); the CLI suite passed (7) after fixing its epoch-2 required-command fixture. Five focused Python regressions passed in 107 seconds. A 38-test Rust regression selection passed 36 tests before that fixture correction, including privacy checks. Configured Clippy, formatting, contract, and file-budget gates passed. The rebuilt CLI validates all 19 repository guides. The full Rust suite passed 4,086 tests with 3 skipped, but Jig rejected its receipt because this narrative note was updated during execution. The same update invalidated the concurrent harness gate. Both gates must be rerun with all tracked source and documentation held stable; final evidence and completion will be recorded only in the excluded structured work plan and state.

The migration audit compared historical receipt JSON and raw lines against HEAD: exactly three stderr previews changed and all other historical fields and lines remained intact. The branch before this repair appended 43 plan records, 24 sessions, 368 run events, and 227 receipts without changing earlier lines. The full Rust test process took 1,189 seconds. The regression suite exposed substantial Linux setup cost from confirming process groups after every successful grading/setup command; retain the safety behavior and report the observed full-suite timing.

## Context and milestones

First, `crates/jig/src/policy/guide_check.rs` dispatches between legacy heading/entrypoint checks and epoch-8 Markdown validation. Recover the legacy behavior from d89129a8's `policy/agent_map.rs`. Regression tests must prove that legacy root links to an absent map and backend links to missing, ignored, or symlinked targets retain their old result, while modern invalid references block.

Second, `scripts/harness_eval/runner.py` owns trial execution and finalization, `process.py` owns process-group retirement, and `grade.py` launches grading probes. `run_trial` must persist cleanup failure and observations before propagation. Both `run_locked` and direct `finalize_trial` must respect the terminal failure. `scripts/evaluate-harness.py` handles the error without traceback using its documented exit status. Metric validation belongs in `observations`; document the adapter capability in `tests/fixtures/harness-eval/README.md`. Tests cover cleanup failure through the CLI, resumed failure, successful leaders with descendants retaining or closing output pipes, unclassified traces, and partial classifications.

Third, `crates/jig/src/state/privacy.rs` supplies bounded path replacement and `state/receipts/tail.rs` bounds captured output. Extend only preview sanitation, leaving raw command output available to its caller. Test suffix retention, token boundaries, current temporary roots, and sanitation before truncation. Migrate the three affected receipts in `.agent/state/receipts.jsonl` and append `.agent/state/decisions.jsonl`; compare parsed records and raw unaffected lines to the baseline.

## Commands and acceptance

Run commands from the repository root. Build with `cargo build -p jig-sh --bin jig` and select it using `JIG_DEV_BIN=target/debug/jig`. Open structured work using `scripts/jig work start --title "Repair modernization review findings" --body "Follow docs/plans/astra-harness-modernization/review-repairs.md" --print-plan-id`.

Run focused Rust guide/privacy regressions and `python3 -B -m unittest discover -s scripts/tests`. Then run `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id <id>`, inspect gates/evidence/receipts, and finish only when configured verification passes. Backend evidence must include `scripts/jig check test`; the configured verify profile also includes that target. Review `git diff --check` and the final diff for stale docs and accidental fixture identifiers.

## Recovery and interfaces

Source edits are reversible. Never replay a cleanup-failed trial: an operator must inspect and retire surviving processes before manual recovery. Classification capability is additive; older adapters continue to work with repeated checks unavailable. Legacy contracts need no state migration; adopting epoch 8 enables strict validation. Receipt migration changes the current tree only, preserves record identity, and must not be rerun to append duplicate decisions. Do not rewrite Git history, remove ordinary state records, or commit/push without instruction.
