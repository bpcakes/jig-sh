# Remove remaining agent workflow friction

## Outcome

Deliver findings 1–5 from the agent-velocity investigation: usable native retries,
coherent operational-receipt handling, fewer resource/prerequisite failures,
cheaper inspection, and actionable CLI discovery. Success means fewer unnecessary
commands and executions while preserving current completion authority. This is
one implementation epic, not authorization to implement it during planning.

Profile: Standard, with explicit compatibility and receipt-provenance constraints.
Baseline: `853303a9527e8de656779f13ea859326f42b36c3` (2026-09-20).
Planning work: `plan_01M307VR2T32ZNVSAYHVEGCJ8D`.
Beads epic: `jig-sh-ndz2` (Remove remaining agent workflow friction).

## Progress

- [x] Inspect current implementation and existing Beads ownership.
- [x] Define task boundaries, acceptance, compatibility, and external prerequisites.
- [x] Complete focused plan review and structural validation.
- [x] Export and verify one epic and its dependency graph.
- [ ] Implement the delivery tasks; none has been implemented by this planning work.

Execution checkpoint (2026-09-21): T-01 is complete in `0154477a`; all six gates
passed and native Codex review reported no findings. T-02's compatible checkout
diagnostics and validation-context tests are in progress; T-03 remains ready.
The owner also authorized implementation
of the external prerequisite chains needed by T-04 and T-05, retaining their
existing issue ownership. Iteration selection (`fe13c657`) and Cargo discovery
(`350536dc`) are marked delivered in Beads but live on the unmerged
`feat/rust-validation-velocity` branch; integrate and validate that existing work
before implementing its dependent tasks. Use task-local ExecPlans where the risk
warrants them; do not create additional planning beads.

## Scope

In scope: the five remaining findings above, delivered through the existing Jig
CLI, MCP, runtime, worker boundaries, templates, and typed evidence machinery.
Existing ownership of Cargo coordination and compact summaries is retained.

Non-goals: downstream contract upgrades, plugin installation/rollout, another
review controller, generalized task tracking, a machine-wide build daemon,
cross-worktree receipt caching, new dashboards, or weaker required final checks.
Do not move or rewrite `.agent/state` journals. Supporting arbitrary nested
plan-linked writes inside a repo-mode worker is not included; that requires a
separate provenance design, not a filename exemption.

Constraints: use generic `ExampleProject` fixtures and repository-relative
evidence only. Never copy downstream identities, session paths, transcripts, or
private operational details into this plan, Beads, tests, or receipts. Preserve
legacy contracts and existing standard result shapes; version or explicitly opt
into any new public shape. No new external service or library is required.

## Current-state evidence

Facts checked at the baseline:

- `crates/jig/src/runtime/work/tools.rs::selected_checks` filters configured gates
  to `WorkGate::Check`; `WorkGate::Evidence` cannot be selected by `--gate`.
  `cli/work.rs::WorkCheckOpts` nevertheless describes configured gate selection.
  `runtime/work/checks/selection.rs` warns that legacy `--tool` receipts cannot
  satisfy native gates. The mismatch is present in source, not just old traces.
- `runtime/work/gates/recovery.rs::from_report` already returns per-target
  `check TARGET --plan-id PLAN` refresh commands and a reuse/execute preview.
  `runtime/work/check_schedule.rs` handles dependency propagation. Extend these
  authorities instead of implementing another retry resolver.
- `runtime/loops/codex_task/checkout.rs::ReceiptJournalBaseline` checks journal
  identity, prefix, index, append bounds, schema, and the exact parent receipt ID.
  `checkout_tests.rs` covers accepted parent appends and rejected ambiguity.
  `docs/codex-task-operations.md` explicitly forbids nested receipt-producing
  commands in repo mode. `docs/configuration.md` documents that `--no-receipt`
  conflicts with `--plan-id`. An entire `.agent` exclusion is not equivalent.
- Current review-skill reconciliation already addresses some historical receipt
  drift. This is context, not a dependency on an installed plugin cache; the
  reproducible supported integration contract is part of T-02.
- `runtime/run_execution/parallel.rs::MAX_PARALLEL_LAYER_TARGETS` is eight.
  `state/execution_leases.rs` coordinates checkout effects; it is not a shared
  Cargo-resource lock. `repository/planner.rs` and `runtime/run_execution.rs`
  already implement dependency ordering and failed-prerequisite skipping.
- `crates/jig-contract/src/repository.rs::ActionSpec` has dependencies, effects,
  and timeouts, but no generic resource declarations. Its strict deserialization
  makes a new public field a compatibility change, not an incidental refactor.
  `doctor_parts/part_04.rs` probes SQLx driver support using a synthetic URL;
  it does not prove a project's application database permissions.
- `runtime/work/gates/dashboard.rs` already batches receipt indexing and shares
  a fingerprint and plan-change observations within a request. `status.rs` uses
  that path. `repository/freshness/budget.rs` and
  `docs/target-freshness-integration.md` define bounded observation: inspection
  defaults to 2,000 ms, with an explicit maximum of 30,000 ms. Deadline exhaustion
  is unknown, not stale. Do not assume no batching exists.
- `cli/work.rs`, `command/work.rs`, and `cli/structured_error.rs` own argument and
  error handling. The source defines `work start --body`, not `--description`;
  `work status` is a summary without a plan selector. Existing `info`, `status`,
  and MCP inspection surfaces are the discovery boundary.

Inference: these mismatches and conservative boundaries can cause repeated
discovery, invalidation, or unnecessary collateral checks. The investigation
does not establish a global speedup percentage. No private trace is necessary
to execute this plan; generic reproductions and current source are the evidence.

Unknowns: which shared browser resource actually requires coordination, and
whether freshness collection is dominated by journal lookup or source hashing.
T-03 and T-05 respectively own bounded measurements and explicit exit decisions.

Existing owners, verified open with `br show`:

| Issue | Existing delivery contract | New epic boundary |
| --- | --- | --- |
| `jig-sh-rust-validation-velocity-w0yp.6` | Cargo resource coordination, consuming typed Cargo identity from its existing prerequisite | T-04 consumes it; no competing lock implementation |
| `jig-sh-rust-validation-velocity-w0yp.7` | Execution cost/decision diagnostics | Related evidence vocabulary; T-05 owns inspection cost only |
| `jig-sh-9wcn.9` | Shared compact work-check/inspection completion projection | T-05 consumes it; no duplicate summary DTO |
| `jig-sh-3j3m` | Atomic publication of reused evidence under cancellation | Preserve this invariant; do not silently absorb unrelated repair |

## Decisions and design

### D-01 — One selection and evidence authority

Proposed direction: extend `work check --gate` to native evidence gates while
keeping legacy gate behavior. Explicit gate selection retains its documented
force semantics; ordinary `work check` continues to select required applicable
gates and reuse eligible passes. Exact target recovery remains the existing
`check TARGET --plan-id PLAN` path. Do not overload legacy `--tool` into target
syntax. Recognizable misuse gets the correct native command without executing it.

Resolve all requested gates and reject unsupported/review gates before starting
any child. Deduplicate native target execution, preserve prerequisites and plan
comparison authority, and retain current failure/cancellation/lease handling.
Newly supporting a native gate must not convert a partial selection into overall
finish readiness; `work finish` still evaluates every required gate independently.

### D-02 — Preserve receipt ownership; improve supported integration

Keep parent-owned append verification and append-only history. Deliver supported
recipes plus structured distinction between application dirtiness, receipt
ambiguity, and an unverifiable journal. A repo-mode standalone diagnostic can
use supported `--no-receipt` execution; a linked check must retain receipts and
use a supported context instead. Never silently strip `--plan-id` or suppress a
required receipt. Review consumers assess validation applicability; Jig does not
declare arbitrary receipt paths irrelevant to a review.

After a worker has started, recovery must not replay it or its completed checks.
Use retained checkout/receipt inspection and the existing occurrence attention
and acknowledgement workflow. A supported-context recommendation applies to
future invocations, not an automatic retry of an occurrence with possible effects.

No source-of-truth migration is needed. If the initial generic integration matrix
shows an already-fixed historical case, retain its regression/recipe and fix the
remaining actionable diagnostic gap, rather than recreate the old defect.

### D-03 — Separate readiness, resource admission, and validation

Genuine readiness prerequisites use existing dependencies or a same-invocation
preflight in the owning wrapper. Mutable external readiness must be evaluated
before each expensive child it protects, including after resource waiting; a
reused source receipt cannot prove that the database still grants access.
Readiness never satisfies the actual SQLx, test, or browser validation requirement.
Do not create databases, change grants, or install dependencies from read-only
diagnostics. Resource claims control scheduling, not correctness dependencies.

Consume the existing Cargo coordinator. T-03 must demonstrate a distinct browser
resource conflict before T-04 adds browser coordination. Prefer truthful action
effects, existing wrappers, and the smallest applicable bound. New action fields
or cross-process policy require explicit versioned design and compatibility tests
before implementation; the safe default is existing behavior for unopted repos.

### D-04 — Share observations only within a valid request

Use the existing compact projection and gate evaluator. Measure duplicate work,
then reuse source/journal observations only while their identity and authority
remain valid. No persistent cached finish authorization. Revalidate closure under
the existing checkout lease. Keep deadline and resource exhaustion distinct;
increase an inspection allowance only through a documented bounded agent mode,
without silently changing standard defaults or resource ceilings. A timeout must
never generate a command to rerun checks as its first remedy.

Reversible choices: module layout, internal helper design, fixture organization,
and bounded presentation details. Consequential choices: durable state ownership,
public schema/version changes, and new cross-process resource policy. The first
is excluded; the latter two require a recorded task-local design and compatibility
evidence. Escalate to the owner only if the necessary choice exceeds this scope.

## Execution graph

| Plan task | Beads issue | Delivery |
| --- | --- | --- |
| T-01 | `jig-sh-ndz2.1` | Native gate retry selection |
| T-02 | `jig-sh-ndz2.2` | Operational receipt integration |
| T-03 | `jig-sh-ndz2.3` | Bounded resource/prerequisite evidence |
| T-04 | `jig-sh-ndz2.4` | Resource and prerequisite integration |
| T-05 | `jig-sh-ndz2.5` | Economical evidence inspection |
| T-06 | `jig-sh-ndz2.6` | One-step CLI recovery |

Dependencies below are the source of truth; tracker blocker edges mirror them,
including external IDs. All six deliveries remain open.

### T-01 — Make native gate retries selectable and exact
- Outcome: A native gate shown by inspection can be selected, and a single failing target has an executable plan-bound recovery command without rerunning unrelated current passes.
- Context: D-01; `runtime/work/tools.rs`, `checks/selection.rs`, `gates/recovery.rs`, and `work/check_schedule.rs` already own the relevant semantics.
- Changes: Gate resolution and execution dispatch, `command/work.rs`, CLI help/argument conversion where necessary, MCP work-check request handling, human/JSON recovery output, `docs/public-contract.md`, and focused runtime/CLI/MCP fixtures.
- Depends on: none
- Verify: In ExampleProject, exercise native-only and mixed native/legacy selections, repeated gates/shared prerequisites, unknown and review gate IDs, stale inputs, failed prerequisites, cancellation, and closed plans. Count launched targets. Invoke emitted argv and verify ordinary native receipts and retained plan identity. With independent targets, a default retry executes only the failed target. Shared-prerequisite cases execute the required dependency/dependent closure and reuse independent current passes; forced selection still forces. Existing legacy request/output contracts remain accepted.
- Recovery: Preserve original receipts and legacy tool behavior. Revert dispatch changes without rewriting state; never reinterpret old tool receipts as native evidence.
- Done when: Displayed native gates are accepted; invalid selections start nothing; CLI/MCP agree; an unselected required failure still blocks finish; recovery uses the same planner and evidence authority as execution.

### T-02 — Make operational receipt outcomes actionable across supported workflows
- Outcome: Supported validation workflows avoid false dirty-worker failure, and unsupported or ambiguous writes are distinguishable from application changes with a safe, non-replaying recovery route.
- Context: D-02; parent-only receipt attribution is intentional and current review reconciliation already exists. Preserve that boundary.
- Changes: `runtime/loops/codex_task.rs`, `codex_task/checkout.rs` and `checkout_tests.rs`, worker result/recovery rendering, `docs/codex-task-operations.md`, `docs/configuration.md`, and managed task/agent guidance where it recommends validation commands. Add compatible typed reasons for application dirtiness, receipt ambiguity, and journal verification failure with observed paths/parent receipt identity and safe read-only inspection steps.
- Depends on: none
- Verify: Reproduce standalone checks, plan-linked checks, commands used by review validation, repo-mode tasks, and isolated tasks using generic fixtures. The local oracle is Jig's actual exit/result, append, and provenance contract, not simulated external controller reconciliation. Prove parent-only append success and supported receipt-free standalone diagnostics; linked checks still record evidence. Reject application edits, staged journal changes, prefix rewrites, malformed/partial rows, missing parent receipt IDs, and unowned concurrent appends. Verify diagnostics retain observed receipt IDs and original output and never replay a started worker. An optional live review smoke records the controller version; normal tests must not require installed plugins or paid providers.
- Recovery: No migration, truncation, broad ignore rule, or old review-state reinterpretation. Keep ambiguous state and evidence for inspection. Unsupported nested linked checks receive a supported-context recommendation, not an exemption.
- Done when: The local supported matrix has observable success, typed diagnostics distinguish unsupported nesting and actual application dirtiness, and documented recovery inspects retained state/attention without replay or discarded plan evidence. Live external review reconciliation is not a required or claimed local acceptance result.

### T-03 — Establish the resource and prerequisite remedy with bounded evidence
- Outcome: Reproducible generic cases select the smallest justified resource/readiness change, with a recorded implementation decision for T-04.
- Context: D-03; Cargo coordination has an existing owner. Browser contention is a hypothesis, and SQLx driver detection is not a project readiness check.
- Question: Which execution resource or missing prerequisite causes avoidable waiting, child launches, or failures after accounting for current effects and dependencies?
- Method: Compare isolated, same-run parallel, and two-request overlap for a shared Cargo target, a browser resource/ownership pattern traced from an existing generated or configured browser wrapper, and a missing/insufficient database prerequisite. Reproduce supported browser ownership with generic fixtures; an artificially colliding resource alone cannot authorize coordination. Distinguish observed queue time, process time, launches, and failure stage; use existing execution events where available.
- Boundary: One generic fixture per hypothesis, at most three measured repetitions per condition after setup; no customer environments, scheduler daemon, or benchmark framework.
- Output: A compact repository-local result with commands, contamination, accepted/rejected hypotheses, and the concrete T-04 behavior. A non-reproduction means no browser scheduler change, not permission to invent one.
- Changes: Focused fixtures/measurement helper only where existing tests cannot express the experiment, and the task-local decision/evidence in this plan.
- Depends on: none
- Verify: Another executor can reproduce the child-launch and ordering observations; reported wall time does not sum parallel target durations; no fixed percentage speedup is promised.
- Recovery: Discard disposable resources and instrumentation; no production or durable-state changes in this task.
- Done when: Each hypothesis is demonstrated or explicitly bounded as unconfirmed, and T-04's prerequisite/preflight and any justified browser-resource behavior have a falsifiable acceptance contract.

### T-04 — Prevent demonstrated resource contention and late prerequisite failures
- Outcome: Actual expensive checks start only after current prerequisites and justified resource admission, while unrelated checks retain concurrency.
- Context: D-03 and T-03's decision. Reuse the existing Cargo resource implementation; do not fork its identity, wait, or cancellation protocol.
- Changes: The existing executor/resource integration from `jig-sh-rust-validation-velocity-w0yp.6`; owning SQLx/frontend wrappers and generated action configuration; `runtime/run_execution.rs`, `parallel.rs`, and readiness helpers only where the demonstrated boundary requires it. Public contract changes are conditional on the decision gate, not assumed.
- Depends on: T-03, jig-sh-rust-validation-velocity-w0yp.6
- Verify: Failed readiness prevents expensive child launch; repaired readiness allows the real check. Mutating the external prerequisite after a successful source receipt still requires a current preflight before execution. Supported competing resources obey the selected bound while independent checks overlap. Waiting cancellation spawns nothing, timeout budgets do not reset, source changes while waiting are rejected, and failure/crash cleanup retains ownership and accurate outcomes. Use synchronized event/launch assertions rather than sleep-only timing tests.
- Recovery: New policy is opt-in where compatibility requires it. Restore prior configuration to contain regressions; keep historical receipts unchanged. Missing or ambiguous resource authority follows the existing conservative path, never guessed equivalence.
- Done when: The demonstrated avoidable overlap or late failure is removed with unchanged validation coverage, no hidden environment mutation, and no competing Cargo coordinator. If browser contention was unconfirmed, readiness plus Cargo integration is the delivered scope and the limitation is explicit.

### T-05 — Make compact evidence inspection bounded and economical
- Outcome: One existing compact inspection gives current completion/recovery information without redundant source/journal work or an automatic full-suite retry after observation timeout.
- Context: D-04. `jig-sh-9wcn.9` owns the shared summary projection; this task owns measured collection cost and bounded inspection behavior, not a second projection.
- Changes: `runtime/work/gates/dashboard.rs`, `gates/collection.rs`, `gates/target_evidence.rs`, `repository/freshness/budget.rs`, `status.rs`, existing compact CLI/MCP projections, and `docs/target-freshness-integration.md`. Keep any observation metrics separate from proof identity.
- Depends on: jig-sh-9wcn.9
- Verify: First profile journal lookup versus source collection at 1 and 20 plans with 1,000 and 10,000 generic receipts, cold/warm, up to three trials each. Instrument repeated scans and observe total elapsed time separately. Reuse existing index/baseline caches; optimize the demonstrated duplicate phase. Assert one request shares eligible observations, source/journal/config changes invalidate them, cancellation/resource ceilings still stop collection, and deadline exhaustion yields unknown with read-only recovery. Standard defaults/shapes remain compatible and finish independently revalidates after a source edit.
- Recovery: Request-local optimizations can be reverted without state migration. Keep bounded standard mode and detailed evidence available; do not persist a summary as closure authority.
- Done when: The existing compact path covers the ordinary decision without gates/evidence/receipts round trips; demonstrated duplicate scans are removed or a measured absence is recorded; timeout handling and any explicit agent budget are bounded and truthful. Timing claims require observed data, not a guaranteed latency threshold.

### T-06 — Make invalid CLI attempts recover in one step
- Outcome: Common argument/selector mistakes produce the exact supported command or scoped help without executing a guessed operation.
- Context: D-01 and D-04; native recovery and compact inspection must be settled before documenting their canonical path. Keep the existing info/status discovery family.
- Changes: `cli/work.rs`, `cli/structured_error.rs`, CLI argument parsing/conversion, runtime selector diagnostics, shared recovery rendering, `templates/project/AGENTS.md.jinja`, and command examples. Synchronize generated snapshots through the established template workflow when needed.
- Depends on: T-01, T-05
- Verify: Generic CLI/JSON cases for `work start --description`, `work status --plan-id`, unsupported `--summary`, top-level `contract`, and target syntax supplied to legacy `--tool` return accurate next commands. Apply each suggestion against its fixture and assert no silent execution on the invalid attempt. Quote argv safely for names with spaces; avoid claiming a unique active plan when several exist. Test old supported invocations and standard structured-error envelopes.
- Recovery: Prefer contextual hints; add an alias only when its semantics are exactly equivalent and conflicts are tested. Do not remove old valid commands or auto-retry mutating operations.
- Done when: Each selected common mistake has a correct one-step recovery or truthful ambiguity, native target/tool/gate vocabulary is consistent, and generated guidance uses the supported compact workflow without adding another command family.

The first ready tasks are T-01, T-02, and T-03. Dependency-limited tracks run through
the existing Cargo work into T-04 and the existing compact-summary work into
T-05/T-06. No duration-based critical-path estimate is justified yet. Preserve
external owners and their upstream dependencies; do not create duplicate issues
or silently reparent them. The structural validator covers local task IDs; the
Beads graph check additionally covers external edges and parent relationships.

T-01 and T-02 mostly own separate runtime boundaries. T-03 is fixture-only.
Do not concurrently edit public command/result types, generated manifests, or
templates. In particular, integrate the existing owners before T-04/T-05 and
serialize T-06's shared CLI/template changes. Graph readiness is not a file lock.

## Verification

During execution, run the smallest falsifiable task checks while developing, then
the repository-required gates at a stable delivery boundary. Backend deliveries
finish with `scripts/jig check test`; runtime/template changes also exercise
`scripts/jig-dev` and the configured `repo:source-runtime-check`. Current-source
validation must not accidentally use only the released launcher implementation.

End-to-end generic acceptance: open work, run a profile with a controlled single
failure, inspect once, apply the emitted exact recovery, and finish only after
all required evidence is current. Count unnecessary launches and inspection
calls. Repeat with a source change, an unknown freshness observation, an
unselected required failure, and ambiguous worker receipt mutation: none may
produce a false success. Resource cases also prove independent work can overlap.

Planning-only validation: run the planning-workflow structural validator on this
file, review the high-risk receipt and scheduling boundaries, check every exported
task's acceptance/recovery and blocker direction, then run
`python3 scripts/beads-sync.py` and `python3 scripts/beads-sync.py --check`.
Use structured work checks/evidence/finish for this planning deliverable; do not
claim implementation acceptance from a valid plan or tracker export.

## Rollout and recovery

Deliver tasks incrementally through their existing interfaces. Start with generic
fixtures and Jig dogfooding; downstream adoption and plugin distribution remain
separate work. Preserve old readers and omitted-option behavior. Any new action
metadata requires authoring/render/runtime agreement and an explicit supported
contract version, with rejection or safe fallback for older runtimes.

Diagnostics and request-local collection changes need no journal migration.
Preserve original record IDs and bytes, and never retroactively turn a failed or
ambiguous worker/review outcome into success. Revert opt-in scheduling/configuration
or new presentation behavior to contain regressions while retaining evidence.

## Risks and open decisions

| Risk or decision | Owner and resolution |
| --- | --- |
| Native gate support accidentally changes force/reuse semantics or broadens closure | T-01 owner; resolve before dispatch with launch-counter and required-gate negative tests |
| Receipt path classification launders unowned writes | T-02 owner; retain exact-parent provenance and explicit unsupported nested-write boundary |
| Browser flakiness is misattributed to scheduling | T-03 owner; bounded reproduction selects or rejects coordination before T-04 |
| Readiness receipt outlives external privileges/state | T-04 owner; same-invocation preflight or equivalent current proof before child start, including after waiting |
| New shared-resource declarations exceed current strict schema | T-04 owner; design/version gate before adding public fields; default to existing behavior otherwise |
| Cached inspection outlives source/journal authority | T-05 owner; request-local identity checks and independent finish revalidation |
| External prerequisite work delays delivery | Epic integrator; T-01/T-02/T-03 can proceed; preserve visible blocker edges, do not clone implementations |
| Existing implementation makes a historical symptom obsolete | Owning task; verify current behavior, retain regression, narrow the task to remaining integration instead of manufacturing work |

## Surprises & Discoveries

- Cargo coordination and compact summaries already have open owners; the new
  epic adds their consumers and specific remaining fixes, not duplicate engines.
- Repo-mode workers intentionally accept only their exact parent receipt append.
  Nested plan-linked receipts are not safe to whitelist as an optimization.
- Gate recovery and dashboard inspection already have typed recovery and batching.
  Remaining work must extend or measure these paths before introducing new ones.

## Decision Log

- 2026-09-20: Scope is findings 1–5 only. Keep downstream adoption and rollout of
  already-delivered skill fixes outside this implementation epic.
- 2026-09-20: Retain existing issue ownership via explicit external prerequisites.
  Split resource diagnosis from implementation because the browser hypothesis
  materially changes the implementation boundary.
- 2026-09-20: Do not relocate append-only state or authorize unowned nested appends.
  Improve supported integration and actionable diagnostics first.
- 2026-09-20: Focused review clarified non-replaying worker recovery and the local
  Jig acceptance oracle, required a real supported browser ownership pattern
  before scheduling changes, and preserved required-dependent retry propagation.
  No material review findings remain. The structural validator's isolated T-02
  warning is intentional: receipt integration needs no artificial predecessor.

## Outcomes & Retrospective

Planning and export delivered epic `jig-sh-ndz2` with six open tasks. Focused
review covered native retry authority, receipt provenance, resource boundaries,
and graph/inspection compatibility; all material findings were integrated.
Structural validation passes with one intentional warning for independent T-02.
Tracker verification checks task mapping, acceptance/recovery, parent membership,
external blockers, and ready roots T-01/T-02/T-03. Existing unrelated tracker
cycles are not repaired or claimed absent by this epic.

The required export first rejected a stale local database. A read-only reconcile
preview identified two source-only issues; non-destructive reconciliation imported
those exact IDs, changed no existing issue, deleted none, and retained all seven
new records. The repository sync helper then exported and passed privacy checks.
The sync also preserves an already-newer database record rather than replacing it
with an older export. This is tracker maintenance, not a new epic delivery.

No product code or downstream configuration has changed. Implementation behavior,
performance effects, and delivery acceptance remain unverified until task-specific
checks run. Planning work receipts retain the configured gate results separately;
passing those checks cannot certify the planned runtime features.

Planning verification limitation: the configured `work check` ran the full
profile, and nextest reported 3,145 passes, one launch failure, three skips, and
1,087 tests not run. The launched test executable disappeared from
`target/debug/deps`; its cause is not established. Independently, source authority
was rejected as `execution_mutated`/`source_raced`, including this plan: the
planning agent started verification before finishing plan/export edits. This was
an execution sequencing error, not evidence of a product regression. The final
plan, graph, privacy, and whitespace checks pass, but broad verification does not.
The structured planning record remains open because required evidence cannot
authorize finish. Do not treat it as a completed implementation or rerun the
broad suite merely to certify this document; future code delivery must validate
a stable candidate and resolve build-artifact interference first.
