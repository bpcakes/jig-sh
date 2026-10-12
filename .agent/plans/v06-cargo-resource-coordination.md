# Coordinate opted-in Cargo checks across Jig processes

This living ExecPlan follows `.agent/PLANS.md`. It implements
`jig-sh-rust-validation-velocity-w0yp.6`, an authorized prerequisite of
`jig-sh-ndz2.4`. Independent Jig requests using a shared Cargo artifact directory
will wait before starting their build, then recheck source and existing evidence.
Ordinary checks keep their existing concurrency. Coordination changes scheduling,
not dependency or receipt authority.

## Progress

- [x] Inspected action/run DTOs, execution layers, owned process boundaries,
  scoped receipt validation, Cargo metadata, and advisory-lock conventions.
- [x] Claimed V06 and opened structured work
  `plan_01M31F7NY5YWBGVTC033Z4MBKY` at baseline
  `911b7b161826dc138179d71c1e7f79028f797ef3`.
- [x] Add strict opt-in resource declarations, authority binding, and resolution.
- [x] Add private process-owned advisory claims and child lifetime protection.
- [x] Integrate admission, one target deadline, post-wait revalidation and reuse.
- [x] Prove cross-process barriers, cancellation, source drift, evidence policy,
  crash recovery, and ordinary-check concurrency.
- [x] Replace singleton scheduling with readiness batches; verify independent
  same-run overlap, conflicting publication order and shared source invalidation.
- [x] Run required gates, stage, native Codex review, resolve findings, finish
  structured work, and close the bead.
- [x] Commit task changes and verify clean checkout (`d966205e`).

Restart checkpoint: V06 is implemented and validated. All4488 backend tests and
all other required gates passed after the two review fixes. Native review cycle2
reported no actionable defects. Structured work finished successfully with
receipt `receipt_01M31Q6N276E13PK4Q46XF6AC2`, then the bead was closed and synced.
The complete task was committed as `d966205e` and clean checkout verified before T-04. The
external workflow findings ledger is never staged or committed.

## Surprises & Discoveries

Execution currently joins an entire parallel layer before publishing receipts.
Holding a build lease in each worker until publication would deadlock two workers
requiring the same resource. Execution must partition a layer without modifying
its dependency graph, and finish coordinated targets before admitting the next.

Cargo metadata can expose separate `target_directory` and `build_directory`.
Both must participate, with identical physical paths deduplicated. The lock key
must not split by feature or profile when those share an artifact directory.

Current receipt dependency references require the original plan identity. A
reused receipt must retain its original run, plan and proof; calling the ordinary
new-receipt recording path would manufacture provenance. Cross-plan reuse of a
prerequisite with a scheduled consumer therefore remains conservative: execute
it again instead of weakening the dependency-proof rules.

End-to-end reuse tests found that default gate invocation construction omitted
the new resource declarations. The gate snapshot now copies them before identity
collection; reuse and forced execution both finish with current passing evidence.
Human progress output, not intentionally silent JSON mode, supplies explicit
waiter-ready barriers; durable run and receipt journals supply result oracles.

DESIGN ESCALATION: `docs/plans/agent-workflow-velocity-measurements.md:113-120`
requires owned browser endpoint coordination for both overlapping requests and
same-run targets while distinct pairs overlap. The current `execution_groups`
implementation in `runtime/run_execution/resources.rs` makes every resource-
bearing target a singleton regardless of disjoint identity. Its conservative
publication boundary is insufficient for that downstream acceptance contract.
Supporting independent same-run resources requires reconciling admission and
receipt publication with the existing layer-wide source postcondition. This is
not a declaration or timeout tweak. The user explicitly required stopping on a
deeper design misalignment rather than performing an uncontrolled redesign.

Read-only T-04 handoff: generated Rust actions are built in
`bootstrap/repository_model.rs::add_adapter_actions`; authored action resources
round-trip through `repository_model/authored.rs`. Before eventual generated
adoption, a regression confirmed `adoption_refresh.rs::generated_action` could
discard nonempty resource declarations during capability/footprint refresh.
V06 now preserves the whole opted-in action and command. The regression failed
before the guard and passed afterward; all six nearby freshness-adoption tests
passed. Browser E2E currently runs through generated
package scripts, not an existing generated Jig E2E action. SQLx 0.9 already owns
the per-invocation database guard; retain it after admission, without duplicate
preflight. The preservation defect is a fixed pre-review integration observation,
not a native review finding. No T-04 implementation or measurement rerun occurred.

## Decision Log

2026-09-21: Add an optional, bounded `resources` list to `ActionSpec` and
`PlannedTarget`, initially supporting only strict `cargo_v1` declarations. This
allows explicitly declared legacy Cargo wrappers as well as typed nextest,
without inspecting shell text. Fields are included in authority and replay
validation; omitted fields preserve old behavior. The released source launcher
stays pinned to 0.4.0 and this repository is not implicitly opted in.

2026-09-21: Cargo declarations specify a repository-relative workspace manifest,
optional repository-relative execution directory, and the existing structured
Cargo context. Typed nextest declarations must agree with their runner; prepared
feature overrides remain authoritative for execution. Generic command authors
are responsible for declaring their actual Cargo invocation context, including
an inner Cargo working directory different from the outer wrapper's directory. Native
actions and mutating/external actions cannot claim this first resource policy.

2026-09-21: Use one private per-user namespace under the platform temporary root
chosen by the implementation independently of caller `TMPDIR`. Validate directory
ownership, permissions and non-symlink identity. Claims use advisory file locks,
not PID-file liveness or deletion. Names contain opaque hashes, never raw paths.
Canonicalize existing ancestors for not-yet-created artifact directories and
resolve symlink aliases. Existing directories also require physical device/inode
claims. Missing directories have unproved physical alias authority: acquire an
exclusive repository fallback claim, retain available canonical-path bridge
claims and report partial coordination. Other unresolvable authority uses the
same repository fallback. Proven claims also
take a shared repository guard so they conflict with fallback claims in that
repository. Sort all claims deterministically; deduplicate physical resources.

2026-09-21: Hold each admitted lease until owned child cleanup, source
postcondition, receipt publication and target-result publication finish. Target
children inherit the build claim deliberately; unrelated subprocesses do not.
Closing an owner's descriptor must not explicitly unlock a surviving child's
claim. A killed Jig owner must not admit another build while its child still
owns the claim; the last descriptor closing releases it without manual cleanup.
Verify this boundary with a separate-process crash fixture before relying on it.

2026-09-21: Within each existing dependency layer, preserve the existing
uncoordinated parallel group and run coordinated targets individually through
publication. No new `depends_on` edges are introduced. This conservative first
release may serialize distinct opted-in Cargo targets within one Jig request;
separate requests with distinct resources remain independent.

2026-09-21: The preceding singleton decision was escalated and work stopped.
The user subsequently authorized revision and resumption. Supersede singleton
scheduling with readiness batches within each eligible read-only dependency
layer. Attempt each target's entire sorted claim set nonblockingly; retain denied
targets as pending. A nonempty admitted batch executes without waiting for more
claims. Sleep for contention only when no target is admitted. This prevents
hold-and-wait cycles across requests, while disjoint available resources and
ordinary targets can overlap. Keep the existing worker concurrency bound.

Each pending coordinated target owns one budget initialized before its first
resolution, retained across batches and retries. Revalidate configuration,
resource identity and source after admission. Preserve exact post-wait reuse
policy and original provenance. All batch executions must finish and pass the
shared source postcondition before successful receipts/results are published;
hold their claims through publication. Reused results also need the final batch
source/expiry check and must not bypass a sibling's source mutation. Expired reuse
must execute within the original budget rather than publish stale evidence.
Conflicting siblings enter a later batch, without new dependency edges or a
failure dependency. Fail-fast and effectful layers retain sequential semantics.

2026-09-21: Start one monotonic deadline before resource resolution/acquisition.
Resolution, waiting, post-wait source/config/resource/proof checks, and child
execution consume it. Repository execution-lease waiting remains outside it.
Cancellation or timeout before admission records no child start. Explicit forced
gates and direct execution do not acquire new evidence-skipping behavior. Ordinary
work checks may reuse only the exact latest eligible successful invocation under
the existing validator, preserving original provenance and recording no fabricated
child run. Unknown, failed, narrower or stale evidence requires actual execution.

## Outcomes & Retrospective

Focused verification so far: 26 contract tests, five declaration/planning tests,
11 lease tests, seven cross-process tests (including a mixed four-target layer
under both success and failure), four receipt-reuse tests, five coordinated-alias
tests and three unchanged alias tests passed. Two expiry/provenance unit tests
also passed. All 12 resolver tests and seven process tests passed after final
repository physical-identity refinement. The single budget is covered by explicit
waiter timeout and deterministic admission-to-child budget tests, not a
fragile small wall-clock threshold. Full backend gates have not completed
successfully and native review has not run for V06. No benchmark improvement or
complete epic delivery is claimed.
The optional test-hermeticity task `jig-sh-7o6` remains authorized only if needed.

Final validation state at escalation: fmt, contract, file-budget and current-
source runtime check passed in run `run_01M31HKJBSYZ4TFSCCTXVM7GP1`. Clippy failed
on three unfixed test-code issues: duplicate dead-code attribute in the shared
resource fixture, cognitive complexity in the mixed-layer test, and a redundant
clone in planner resource tests. Full backend run
`run_01M31HKGZJQFAV654C43YGV60Q` compiled and began 4480 tests (four skipped), then
was gracefully canceled at escalation; it did not produce a full-suite pass.
Native V06 review has not run. Do not treat these partial checks as completion.

The first gate attempts were canceled when a new budget test was found beneath
the globally ignored `target/` directory. It was moved to tracked
`runtime/run_execution/budget_tests.rs` with an explicit module path and staged
before the restarted runs. Coverage was retained; no ignore or lint policy was
weakened. Existing source, tests and all append-only receipts remain preserved.

## Context and orientation

`crates/jig-contract/src/repository.rs` declares actions;
`crates/jig-contract/src/run.rs` contains serialized execution plans and results.
The new resource DTO belongs beside these contracts. Configuration loading and
`crates/jig/src/repository/planner.rs` must reject unsupported declarations and
carry the exact declaration into the run plan. Scheduled-plan validation and
freshness authority must cover the same field, so an old or tampered queued plan
cannot bypass coordination.

`crates/jig/src/runtime/run_execution.rs` owns target scheduling and publication.
Its `target.rs` creates timeout controls and commands, `freshness.rs` records proof,
and `parallel.rs` preserves a shared source postcondition for concurrent targets.
Add resource admission as a separate module, not a dependency edge.

`crates/jig/src/runtime/work/gates/check_snapshot.rs` validates exact prepared
invocations against latest receipts. Expose a bounded internal adapter for
post-wait reuse. Scope collection, original proof validation and latest-outcome
semantics must remain intact. `runtime/work/checks/targets.rs` must represent
reused evidence truthfully rather than calling an unstarted target deferred.

`crates/jig/src/state/execution_leases.rs` provides repository-lease conventions,
but its explicit-unlock Drop is unsuitable for inherited build claims. Use the
existing fs4 dependency for a dedicated close-only resource lease. The Unix
literal-exec hook calls exec itself, so descriptor inheritance must be installed
before that hook.

## Plan of work and milestones

First add strict DTOs, validation, authority binding and focused serialization
tests. Resolve artifact directories with a bounded owned `cargo metadata
--no-deps --locked --offline --format-version 1` invocation using the declared
execution directory and runner environment. Do not modify Cargo.lock or infer
commands from shell strings. Resolution cancellation and timeout are terminal;
other unprovable configuration takes the documented repository-only fallback.
Do not persist raw metadata or absolute artifact paths.

In parallel, a bounded independent implementation may add the private advisory
lease primitive and its direct process tests. Give its agent exclusive write
paths and the interface before delegation. The main agent integrates it with
execution controls. Claims must be held by the actual target process tree, not
accidentally inherited by metadata/freshness subprocesses.

Next partition eligible execution layers into readiness batches. Resolve pending
resource identities before holding any batch claims; the admission scan itself
uses only nonblocking claim attempts, so metadata work cannot hold up an already
admitted child's start. Admit at most eight targets per batch and retain each
pending coordinated target's original deadline across batches. Ordinary-only
layers retain their existing executor. Discard pre-wait reusable source
observations. After admission,
recollect source, validate configuration, resolve resource authority again and
reject changed identities before spawning. Run exact receipt reassessment only
where the caller permits reuse. Preserve forced/direct semantics and original
provenance. Keep the claim through success, failure, timeout, cancellation,
source-postcondition failure and publication. Unexpected worker termination must
still use existing durable blocked-run recovery.

The batch coordinator retains claims and owns publication. Workers return
unpublished captures or original-proof reuse candidates. A shared postcondition
checks source after all admitted workers have finished. Source drift invalidates
otherwise successful siblings, including reuse candidates; reuse expiry requires
actual execution under the original budget. Publication precedes claim release
and admission of a conflicting sibling. No new dependency edge is introduced.

Finally add process-level regression tests with explicit entered/waiting/release
barriers. Use generic isolated repositories and no live services. Fix verified
root causes, then run all required validation and the user's native review loop.

## Concrete steps

All commands run at the repository root. Focused validation initially includes
`cargo test -p jig-contract`, relevant new unit/integration targets, and
`cargo build -p jig-sh --bin jig`. Use `scripts/jig-dev` for changed source behavior,
not an implicit launcher pin upgrade. After implementation and focused tests,
stage complete source and run:

    scripts/jig check test --plan-id plan_01M31F7NY5YWBGVTC033Z4MBKY
    scripts/jig check api:clippy api:fmt repo:contract repo:file-budget repo:source-runtime-check --plan-id plan_01M31F7NY5YWBGVTC033Z4MBKY
    scripts/jig work check --plan-id plan_01M31F7NY5YWBGVTC033Z4MBKY
    scripts/jig work gates --plan-id plan_01M31F7NY5YWBGVTC033Z4MBKY
    scripts/jig work evidence --plan-id plan_01M31F7NY5YWBGVTC033Z4MBKY --freshness-timeout-ms 30000
    scripts/jig work receipts --plan-id plan_01M31F7NY5YWBGVTC033Z4MBKY

Expected: passing focused tests and six fresh required gates. Record actual counts
and receipts here when obtained. Stage generated journals and run native
`codex review --uncommitted`, with no review-fix-loop controller and no live
benchmark commands. Follow the external findings ledger workflow for every
finding. Finish structured work before closing and syncing the bead, then commit
all task changes and verify `git status --short` is empty.

## Validation and acceptance

Two independent processes sharing an artifact directory must report a maximum
critical-section holder count of one; distinct ordinary checks must demonstrably
enter before either is released. Symlink aliases and explicit shared directories
must collide. Unknown authority must visibly report partial coordination and
conflict with known same-repository claims.

Two same-run targets with distinct existing artifact directories must both reach
their child-entered barriers before either is released. Same-resource siblings
must not enter until the predecessor's receipt and target-completed event exist,
including when that predecessor fails. A source mutation by either of two
overlapping read-only targets must prevent both from retaining passing evidence.
Fixtures must not assert incidental ordinary-versus-Cargo ordering.

A waiter that signals readiness and then receives cancellation or exhausts its
deadline never creates its child-start marker and never releases the first
owner's claim. Editing tracked source while it waits prevents execution after
admission. Killing the Jig owner while its child is held at a barrier must not
permit overlap; releasing the child must make later admission possible without
deleting a lock file.

An equivalent successful check published during a wait may be reused only when
the request allows it. Assert original receipt/run/plan identity, no extra target
receipt and no child-start marker. Newer failure, narrower scope, source change,
forced gate and direct-run controls must still execute or block as appropriate.
Legacy declarations without resources retain their prior serialized form and
behavior; unsupported resource versions fail closed before starting a child.

## Idempotence and recovery

No persistent daemon, distributed lock, cross-worktree receipt cache, release or
downstream rollout is included. Lock files may remain; ownership is the advisory
claim, not file existence. Do not recommend manual deletion. Retry through normal
planning so configuration, source and proof are checked anew. Durable failures
contain portable reasons and opaque identities only. Preserve all append-only
journals and unrelated user changes. Do not rerun the exhausted T03 live matrix
or the separately budgeted work-inspection benchmark.

Plan created 2026-09-21 after repository inspection. Updated the same day for
implemented declaration/lease/executor paths, honest missing-directory fallback,
alias routing, expiry revalidation and observed focused-test results. Native
review and all required gates remain pending.

Revision 2026-09-21: after explicit user authorization, replace the escalated
singleton scheduler with readiness batches. Preserve admission budgets, shared
source validation and publication-owned claims while adding the independent
same-run concurrency required by the downstream T04 acceptance decision.

Resumed focused validation: `cargo test -p jig-sh --test cargo_resources --test
cargo_resource_reuse -- --nocapture` passed all 14 tests (10 resource/process,
four reuse). The new fixtures demonstrate same-run distinct-resource overlap,
available-target publication while an earlier conflicting resource remains held,
healthy-first/later source mutation rejecting both results, and mixed ordinary/
shared-resource execution without synthetic dependency edges. Final scheduler
refinements, deterministic publication tests and all required gates remain pending.

The final scheduler refinements are now complete. Two deterministic resource-wave
unit tests passed: shared source changes reject successful captures and original
proof reuse; expiry at the exact boundary forces execution without replacing the
admission budget. `cargo fmt --all -- --check` and diff whitespace checks passed.
The executor remains below its 800-line limit after extracting result recording
into its existing owning module. Full required gates and native review remain.

First resumed gate cycle passed fmt, contract, file-budget and source-runtime,
but strict Clippy found three scheduler code-shape issues. Fixed them with boxed
reuse payloads, a named admission-result type and a combined conditional; no
policy suppression. The exact workspace/all-target/all-feature Clippy command
then passed. The concurrent full suite was gracefully canceled before these edits
at 180 seconds (4486 tests started, four skipped); this is not passing evidence.
Restart all required validation on the revised staged source.

Final validation passed: run `run_01M31KER2YGSRK0JHDV7PXA3T8` executed all 4486
backend tests successfully (four existing skips; 1463.265 seconds test runtime).
Receipt: `receipt_01M31MWKV13Y82P9C3FWZX7TAR`. All five other targets passed in
`run_01M31KESGNPVVZRJRAM0CJXQT0`. `work check` then reused all six current passes,
recording `receipt_01M31MX5EHA8S9MAZ5QEY73TT0` without rerunning checks. Read-only
evidence with a 30-second observation budget reports fresh/passed required
verification and no missing, failed, stale or unknown required gates. Native
review of all staged and unstaged changes is next; do not close or commit yet.

Native review cycle 1 reported two P2 findings. Mixed waves applied publication
deadlines to ordinary targets, allowing a fast successful command to time out
while waiting for a slow peer; restore the ordinary execution timeout path.
Metadata resolution also downgraded post-spawn supervisor failures to partial
coordination; stop admission when probe cleanup/capture is not confirmed. These
are bounded compatibility/safety fixes, not a new scheduler redesign. Required
gates and native review must run again after both fixes; earlier passing receipts
describe the reviewed checkpoint, not the repaired source.

Both cycle-1 findings are repaired with fail-before/pass-after checks. The new
metadata classifier regression rejected the reviewed fallback, then all13 resolver
unit tests passed. The ordinary-timeout process regression reproduced a timed-out
result despite a successful362ms child, then all11 resource integration tests
passed after restoring the ordinary executor path. Shared source validation and
resource-owner deadlines remain intact. Full gates and native re-review pending.

Post-repair required validation passed: 4488 backend tests, four existing skips,
run `run_01M31NQCJRKJ1VJVAYJ8GJM9HQ`, receipt
`receipt_01M31PWP85RFSA92Z8P5RXEKWK` (1186.803 seconds test runtime). All five
other gates passed in `run_01M31NQEDFRGBR18KB1VG95DPK`. Work check reused all six
current passes, recording `receipt_01M31PX3SYG9H8THGE8BQAK5WV`; read-only evidence
and gates report passed/fresh with no missing, stale or unknown required gates.
Native review cycle 2 reported no actionable defects. Its optional focused test
attempt failed to compile with ENOSPC; this does not replace or invalidate the
preceding successful full suite and required gates. No conditional jig-sh-7o6
repair was needed. Finish structured work, close V06, commit and verify clean.
