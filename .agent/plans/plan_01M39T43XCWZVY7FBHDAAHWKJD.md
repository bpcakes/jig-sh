# Reduce local validation reruns

## Outcome and scope

Deliver a source-repository entrypoint that runs cheap checks before workspace
tests, preserves Jig's existing freshness decisions, and avoids duplicate final
test invocations. The user requested implementation and a PR with open-PR overlap
cataloging on 2026-09-24. This work is based on `df97666c`.

## Progress

- [x] Catalog open PRs and inspect existing iteration/final reuse.
- [x] Add preflight profile, local wrapper, and source-local guidance.
- [x] Verify early failure, passing-receipt reuse, source invalidation and cancellation.
- [x] Commit implementation, complete final checks, finish before evidence commit, open PR.

## Surprises & Discoveries

`crates/jig/build.rs` reads HEAD, tags and cleanliness for display version and
template policy. `api:test` and `api:clippy` cannot simply assert Git independence.
The repository's cached 0.4.0 runtime exists in the main checkout; the new
worktree lacks that cache and the PATH binary is 0.5.0. Initial work commands use
an explicit path to the verified 0.4.0 release. The first contract check
then demonstrated that 0.4.0 cannot parse `work.iteration_profile`; advance the
source pin to the installed 0.5.0 release, whose help and parser support both
phases. Edited-source validation builds and uses this worktree's binary.

## Decision Log

- Use existing `work.iteration_profile` and phase reuse instead of runtime or
  schema changes. The new source-local `preflight` contains every required
  target except `api:test`; the existing final `verify` remains unchanged.
- Do not add artificial target dependencies or remove tests. Preserve Git
  identity for source compilation; prevent avoidable reruns by final validation
  after source commit and work finish before evidence commit.
- Leave generated cadence and review-loop guidance to PR #48. No independent
  review loop is required by this task. This avoids duplicating open work.

## Open PR overlap catalog (2026-09-24)

| PR | Scope | Overlap and disposition |
| --- | --- | --- |
| [#49](https://github.com/bpcakes/jig-sh/pull/49) | Scheduled worktree preparation | Separate runtime path; only append-only state can overlap. |
| [#48](https://github.com/bpcakes/jig-sh/pull/48) | Generated iteration profiles and focused repair guidance | Directly related; retain its generic/template ownership. This PR adopts a source-local preflight profile and wrapper. Root AGENTS.md cadence edits require integration if both land. |
| [#46](https://github.com/bpcakes/jig-sh/pull/46) | Ready-dependent scheduling/resource coordination | Complementary performance work; no scheduler changes here. |
| [#37](https://github.com/bpcakes/jig-sh/pull/37) | Codex fleet usage projection | No implementation overlap. |
| [#33](https://github.com/bpcakes/jig-sh/pull/33) | Generated agent guidance | Potential AGENTS.md and repository-config merge overlap; preserve preflight selection when integrating. |
| [#24](https://github.com/bpcakes/jig-sh/pull/24) | Conservative freshness design | Preserve its Git-sensitive semantics; no freshness runtime/schema edits here. |

[#50](https://github.com/bpcakes/jig-sh/pull/50), CI timing, is already merged into
the chosen baseline. #47, Git-only freshness diagnostics and safe closure, is
also already included. No open PR is merged or modified by this work.

## Tasks and verification

T1 (complete): inspect catalog and authoritative input policy. T2: implement the
wrapper/profile and real-Jig process regressions; depends on T1. T3: validate,
commit, finish and deliver PR; depends on T2. Execution is sequential.

From the repository root, run `cargo fmt --all`, then
`cargo test -p jig-sh --test local_validation`. The tests must prove that a failed
preflight starts no full test, passing preflight executes once across both phases,
worktree-policy evidence survives an evidence-only commit, real input edits rerun
checks, final failures propagate, invalid options spawn nothing, and cancellation
reaches the running child. The fixture's content-only policy is explicit; it does
not assert Git independence for Jig's own test command.

After source is committed, build the current binary and run
`scripts/check-local --plan-id plan_01M39T43XCWZVY7FBHDAAHWKJD` with that binary
selected through `JIG_DEV_BIN`. Inspect `work gates` and `work evidence`, then
`work finish` before committing records. No new required gates. Reverting the
wrapper/profile/guide changes restores the previous workflow; old receipts remain
readable, and the configuration change requires new evidence once.

## Outcomes & Retrospective

Five real-process regressions pass, along with focused strict Clippy, all 14
source-runtime launcher tests, contract, file budget and the Beads privacy guard.
The initial regression fixture lacked required `_commit` and failed during
setup; the fixture was corrected and the full focused target passed.

On implementation commit `76368755`, `scripts/check-local` passed its five-target
preflight and final verification reused all five original receipts, executing
only `api:test` (637.776 seconds). Test receipt:
`receipt_01M39VBKCM8XKKA7RF7V64M94B`. Both gate and evidence inspection reported
all six targets fresh and `finish_ready: true`; `work finish` succeeded before
this evidence commit. No second full suite was run for closure.

[PR #51](https://github.com/bpcakes/jig-sh/pull/51) delivers the implementation
and open-PR catalog. CI was still running with no reported failure at this
checkpoint. This is not a claim that every test is Git-independent or that all
review time can be removed. The configured final gate and every action policy
were compared with the baseline and are unchanged. Historical state records
remain byte-for-byte prefixes of the updated journals.
