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
- [ ] Claim `.4.3`; implement additive receipt metadata and dependency proof.
- [ ] Integrate validity, diagnostics, retry reuse, archive, and inspection budgets.
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
