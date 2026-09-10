# Scoped target freshness implementation

Implement `jig-sh-generic-monorepo-zac.4.2`, then `.4.3`, using the Target
Freshness Policy v1 section of `docs/public-contract.md`. A target with explicitly
exhaustive inputs can retain a receipt after an unrelated source edit. Execution
plans and read-only mutation detection continue to cover the entire repository.

## Progress

- [x] Read both issues, their dependencies, and the agreed design.
- [x] Create isolated `feat/scoped-target-freshness` at `55aaecb3`, combining
  the reviewed design and completed targeted-retry prerequisite.
- [x] Claim `.4.2`, build the development runtime, and open structured work.
- [x] Implement explicit input policy, typed bounded fingerprints, and tests.
- [x] Publish deterministic encoding vectors and reproducible generic measurements.
- [x] Review `.4.2`; fix findings and repeat comprehensive review at most twice.
- [x] Run applicable gates and final backend tests, close `.4.2`, and commit.
- [x] Claim `.4.3` after fingerprint commit `6d0859ea`.
- [x] Implement additive receipt metadata and dependency proof.
- [x] Integrate validity, diagnostics, retry reuse, archive, and inspection budgets.
- [ ] Qualify full command performance, then activate the epoch and source schema.
- [ ] Review `.4.3`; fix/review at most twice, validate, close, and commit.
- [ ] Review the full branch; fix/review at most twice and audit all acceptance.

## Surprises & Discoveries

`jig-sh-qh4` is closed in PR #23 (`5a82d428`), but absent from the design
branch's master baseline. Its latest-original-target selection, native plan
binding, and archive protection are included in the implementation baseline.
The original checkout has unrelated tracker edits; isolated implementation and
validation preserve them. Only owned issue records are copied after authoritative
Beads mutation and privacy sync.

The initial 4,000-file implementation measured 3.30–3.49 seconds p95 in the
debug profile. Repeated owned-process Git supervision dominated its cost. Two
fixed supervised batches reduced this to 1.17–1.31 seconds without weakening
cleanup or source revalidation. All 200 baseline/optimized samples completed;
the optimized samples used the 2,000 ms inspection limit. The debug result does
not satisfy the one-second qualification bar. Release, CI, constrained storage,
and full-command qualification remain part of the activation step in `.4.3`.

The first comprehensive review completed with a verified identical scope from
Claude and Codex. Fixes cover executable symlink resolution, missing ignored
glob roots, compatibility for whole-repository runners, nested Git namespaces,
bounded planning diagnostics, cancellation, typed Git overflow, pruning, and
matcher reuse. Unknown scoped observations keep executable plans available.
Added real unmerged-index, unborn/non-Git, Unicode byte-limit, and cancellation
regressions. The first release cold-storage run completed all samples but failed
p95; activation remains deferred, with full before/after revalidation retained
while reducing allocation and repeated directory opens.

Second review: Claude and Codex completed against the same verified fingerprint,
with no exclusions. Fixed PATH execve fallthrough by covering every reachable
repository candidate, removed optional proof from plan identity/equality and
uncancellable prelaunch recollection, scoped unsupported path failures, pruned
shallow globs, counted runner probes, and added safe Git diagnostics. Submitted
proof is discarded before durable acceptance; `.4.3` must prepare it in the live
execution worker. Shell helper declarations remain the agreed author assertion.
The next cold release matrix completed 100/100 but still failed two case p95s.
The third/final review completed with matching complete scope fingerprints.
Final fixes cover ignored working-directory races, the remaining MCP planning
cancellation path, shared pattern match sets, and premature deadline help.
Added a concurrent-writer streaming test and successful-Git-warning regression.
The agreed shell assertion, actual PATH authority, and conservative warning
handling are retained with explicit rationale. Final fixes were not given a
fourth per-task review; the full-branch review will cover them. The third cold
release matrix completed 100/100 but still failed clean p95 at 1,012.732 ms.

## Decision Log

Reserve epoch 9 after argv/shell epoch 8. Normal generation and repository source
stay on 8 until `.4.3` and performance qualification finish. `.4.2` prepares the
implementation and development fixtures; `.4.3` activates it. Keep existing
`target_input_digest`, `input_digest`, global plan identity, and execution safety
unchanged. No persistent cache, remote store, or external-tool attestation.
Use comprehensive-review defaults, identical pinned scopes, and no exclusions.

## Outcomes & Retrospective

Implementation is in progress. Scoped freshness and epoch activation remain
unproven until tests, measurements, integration, and review are complete.

## Context and plan of work

`crates/jig-contract/src/repository.rs` owns `ActionSpec`; `run.rs` owns plans.
Add the optional input policy, reject its presence before epoch 9, normalize
defaults, and reject empty exhaustive inputs. Add separate versioned identity
types with bounded reasons and previews. Implement collection under
`crates/jig/src/repository/freshness/`, using supervised Git and a shared deadline
and accounting for source, dependency, and runner observation. Integrate plan
preparation without narrowing its global source authority. Publish encoding
vectors and a generic benchmark driver.

In `.4.3`, extend `runtime/run_execution/target_result.rs` receipt recording and
`state/receipts/target_evidence.rs` read models additively. Bind the invocation
before launch and after cleanup. Reference original successful same-plan
dependency receipts and inherit their earliest expiry. Update
`runtime/work/gates/target_evidence.rs` and `runtime/work/checks/targets.rs` while
preserving latest-target selection and original provenance. Update archive,
dashboard, CLI/MCP budgets and diagnostics, and file-budget adoption together.
Activate schema, source, renderer, loader, launcher, and migration docs only
after the new integration and qualification pass.

## Validation and acceptance

Cover every row of the design's acceptance table with isolated generic fixtures:
direct/transitive/generated/runner/configuration/invocation changes; unrelated
edits and commits; default fallback; absent matches; ignored trees/dotenv;
symlinks/submodules; races, cancellation, and limits; epochs and legacy receipts;
original dependency proof; retries/newer blockers/cross-plan records; expiry;
status precedence; global mutation and adoption safety; archive and deadlines.

Run at least 20 independent invocations for each documented roughly 4,000-file
clean/narrow-dirty/wide-dirty/staged/untracked case, narrow/broad inputs, and a
shared dependency graph. Record first/median/p95, entries/bytes/projection costs,
and full command baseline. Qualify CI and one-CPU/20-MiB/s storage profiles with
cold/warm conditions and near-ceiling bounded outcomes. Require phase p95 below
one second, no two-second deadline failures, and full-command p95 within two
seconds of the whole-repository baseline. If qualification fails, improve the
collector and repeat before activation.

Build `cargo build -p jig-sh --bin jig`; force the built runtime through
`JIG_DEV_BIN` for `scripts/jig work check --plan-id <owned-plan>`. Inspect gates,
evidence, receipts, and status. Finish backend changes with `scripts/jig check
test`. Verify Beads privacy and append-only journal integrity. Freeze source
during configured checks and comprehensive review.

## Idempotence, recovery, and interfaces

Do not rewrite historical receipts or fabricate IDs. Retain failed/cancelled
evidence, and resume live processes by their existing handles. Recheck scope
after all reviewers terminate, then fix findings. Disclose any final fixes made
after reaching the review repeat limit. Preserve unrelated checkout changes.

Collection returns complete typed identity or explicit bounded reasons; receipt
conclusion remains separate. One budget covers dependency traversal, source
observation, and proof resolution. Inspection defaults to 2,000 ms with an
explicit 1..=30,000 ms override; recording, post-check, and finish use 30,000 ms,
subject to earlier caller deadline/cancellation. `.4.3` owns consumer activation.

### Fingerprint implementation validation

The final development runtime passed the configured `verify` gate: Clippy,
formatting, contract, file-budget, and all 3,984 workspace tests (three skipped).
Gate status, evidence, receipts, and work status were inspected; all required
evidence was fresh. The separately required `scripts/jig check test` also passed
all 3,984 tests. Focused checks passed 46 tests. Beads privacy, diff whitespace,
and append-only journal checks passed. `.4.2` is closed; this work plan remains
open for `.4.3` and the full-branch review. The final contended cold run completed
200/200 samples but failed three p95 cases, so activation is still deferred.

### Receipt integration staging

`.4.3` is now claimed. Keep normal source/renderer epoch 8 while implementing
and qualifying epoch 9 in explicit development fixtures. A development-only
build feature may admit epoch 9 for CLI/CI qualification; it must not change
normal rendering or the source epoch. Gate invocation reconstruction uses
configured defaults and the exact work-plan baseline. Keep one shared bounded
source observation for gate closures and preserve original dependency receipt
references, global launch/mutation guards, and inherited validity.

The additive metadata, bounded original-receipt location index, iterative proof
validation, and epoch-9 gate reader are implemented in development fixtures.
Original proof validation checks canonical dependency/identity encoding, every
reference field, same-plan provenance, execution ordering, global safety, and
inherited expiry. Gate inspection reconstructs default invocations, validates
the shared closure, detects newer blockers during lookup, and revalidates source
and configuration after resolving originals. Typed CLI/MCP/dashboard projections
preserve separate outcome and freshness precedence and bounded reason arrays.
The focused freshness suite passed 82 tests, including live sequential and
parallel receipt recording, archive retention of original cross-run dependencies,
request timeout validation, inherited expiry in status/evidence/check summaries,
and final finish expiry checks; one explicit benchmark remains ignored. A
separate native execution regression also passes: a parent inherits its native
file-budget dependency's waiver boundary and work-check reuse preserves both
original receipts. Inspection budgets are wired through CLI/MCP/status/dashboard;
work-check and finish evaluations use 30 seconds. Archive refuses mutation when
protected dependency proof is missing or unsupported. Additional validity and
native adoption regressions are being verified. Performance qualification,
epoch activation, comprehensive reviews, and final gates remain outstanding.
Normal source and renderer epoch remains 8. The latest matching optimization
compiled, but its default-parallelism run encountered 15 Git supervision and
cleanup failures on the shared host; that failed run is retained and is being
diagnosed separately before review. The cold full-command matrix and subsequent
clean/narrow probe returned complete proof but missed the one-second p95 bar.

The first integration comprehensive review completed with matching complete
fingerprints from Claude and Codex and no exclusions. Confirmed findings concern
archive maintenance borrowing inspection quotas, fabricated time requirements
for incomplete metadata, and missing-deadline aggregation. Fixes use a locked
streaming archive frontier and preserve time constraints independently of proof
completeness. Recording now carries cumulative observation time as well as
resource counters. Shared closure/aggregate limits, original append-race refusal,
and unsupported archive-proof refusal remain required by the agreed policy.
The parent also corrected phase measurement to exclude the existing plan-change
scan; all new proof/source work and full-command measurements remain included.
Regression and performance qualification of these fixes are in progress.

The review-fix freshness suite passed all 88 tests (one explicit benchmark
ignored), including a real archive/rewrite of 250,001 old records that preserved
both required originals, missing-time profile and transitive proofs, and
cumulative observation accounting. The first compile attempt exposed one old
earliest-deadline helper call; it was migrated to the same conservative fold.

The corrected constrained cold command matrix passed all 600 invocations and
all 15 phase/full-command p95 comparisons; maximum phase p95 was 873.346 ms.
All five constrained limit cases passed. Full raw local reports are checked in;
actual hosted-CI qualification remains pending. The second integration review
completed against an unchanged complete fingerprint, with both reviewers and no
exclusions. Fixes preserve known time constraints when original proof lookup
fails, admit complete native failures without accepting blocked/cancelled work,
retain bounded push-before fetch provenance during local native revalidation,
and correct the development maximum-version fixture. A final whole-repository
source check covers whole-policy dependencies after journal lookup. Shared
inspection/run budgets and unknown archive-dependency refusal remain explicit
policy. Workflow paths now target the implementation's dependencies while
keeping performance qualification mandatory.

The preactivation configured work check passed Clippy (all features), formatting,
contract, file-budget, and all 4,026 workspace tests (three skipped). Gates and
evidence reported passed/fresh; receipts and work status were inspected. The
separate feature-enabled contract regression passed. Final local corrections
place journal revalidation after the whole-source guard and distinguish missing
global authority from a detected source race; focused validation follows these
changes before the third per-task review. The required final backend test and
configured gate run will follow epoch activation.

The final source-ordering change passed its focused regressions. The focused
suite passed 92 cases; the native gate case passed separately after correcting
the test to distinguish a blocked report from a failed gate. It asserts the
native target remains fresh, the gate fails, and the original receipt ID remains
visible. The push-before test separately rejects a changed policy, accepts its
byte-for-byte restoration, and rejects a newly available comparison object.
Final Clippy checks passed. No additional runtime behavior changes followed.

The third integration review completed with both reviewers against fingerprint
`1cfdbb89805b733eb357e3a20a87cf37c8b53e98188077ef9f0f62eb0b665eb5`, complete
before/after captures and no exclusions. Claude attested all 113 evidence pages;
Codex reported no actionable findings. An initial native invocation had a stray
command argument and exited before inspection; the corrected invocation supplied
the completed report. Final fixes clarify resource-limit remedies, align staged
native policy size validation, consistently count diagnostic occurrences, and
include the affected consumers in CI qualification triggers. Explicit shared
inspection/journal/run budgets remain the agreed policy; their aggregate-order
and journal-growth effects are now documented. Deep archive chains and multi-plan
exhaustion remain unmeasured scale cases. These fixes will be covered by the
full-branch review, without a fourth per-task round.

After those fixes, all 98 focused freshness tests passed with the development
feature enabled, including both sides of the staged policy size boundary.
Clippy passed for the runtime, contract and dashboard crates with all targets
and warnings denied. Benchmark script syntax, Beads export privacy, diff checks,
and byte-for-byte append-only receipt/run journal checks passed. The candidate
is ready for hosted CI qualification; epoch 8 remains the normal runtime/source
contract until those measurements pass.
