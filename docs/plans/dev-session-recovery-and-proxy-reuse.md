# Dev session recovery and proxy reuse

## 1. Outcome

Normal development startup should recover eligible stale claims, explain any
remaining blocker accurately, and use a proxy whose listener configuration matches
the request. Operators must be able to inspect and retire an orphan after its
repository moves or disappears, without turning saved PIDs into signaling authority.

This is the canonical cross-task plan for one Beads epic. It authorizes no runtime
implementation in the planning session. Profile: Standard, with explicit durable
state compatibility and recovery gates. Baseline:
`82e6c1c0e0139497ab5e54d10b431695957a9732`.

Beads epic: `jig-sh-3ykb` (Reliable dev-session recovery and shared proxy reuse).

| Canonical task | Beads issue |
| --- | --- |
| T-01 | `jig-sh-3ykb.1` |
| T-02 | `jig-sh-3ykb.2` |
| T-03 | `jig-sh-3ykb.3` |
| T-04 | `jig-sh-3ykb.4` |
| T-05 | `jig-sh-3ykb.5` |

## 2. Scope

In scope: session assessment and diagnostics, safe automatic same-repository claim
retirement, legacy cleanup-evidence compatibility, exact-session recovery without
repository context, and LAN/HTTP2 configuration verification when reusing a proxy.

Non-goals: killing processes from registry PIDs, taking over live cross-repository
sessions, restarting a shared proxy automatically, changing certificate trust,
repairing actual downstream state, changing application code, or redesigning proxy
routing. Use only generic fixtures such as `ExampleProject` and `ExampleMovedProject`.

Retain the shared route/session lock, exact identity checks, owner-only state,
bounded parsing, cancellation, atomic writes, and route-first/session-second failure
ordering. Keep `proxy prune` route-only. No absence inference from a 404, a failed
control connection, an old timestamp, or absence of a route.

## 3. Current-state evidence

- Fact: `crates/jig-dev-proxy/src/dev_sessions.rs::claim_session_interruptible`
  retains every `cleanup_required` record; normal startup then rejects overlapping
  claims without classifying them. `state.rs::read_routes` independently filters
  dead route owners. These paths explain how a claim conflict and proxy 404 coexist.
- Fact: `dev_sessions/management.rs::orphan_recovery_assessment` already distinguishes
  absent, live, uncertain, preflight-pending, pending-spawn, and legacy evidence.
  `retire_orphan` rechecks exact ownership under the shared lock. Recovery notices
  explicitly cannot rule out unrecorded descendants.
- Fact: `dev_sessions.rs::ClaimConflicts::launch_error` calls cross-repository
  claims live without observing liveness. `management.rs::status` counts every
  orphan as aggregate `running`; `docs/public-contract.md` documents that behavior.
- Fact: `CanonicalRepo::resolve` canonicalizes an existing root; stop filters by
  that root identity. `crates/jig/src/cli/run.rs` loads repository context for dev
  commands. Both boundaries prevent recovery from a deleted repository root.
- Fact: `processes/proxy.rs::proxy_ready_interruptible` verifies authenticated PID
  and HTTP/HTTPS ports, but not LAN or HTTP2. `server.rs::run_bound` fixes the bind
  address and HTTP2 behavior when the proxy starts. CLI help promises LAN overrides.
- Fact: session state version 1 defaults omitted preflight evidence to false. The
  test `older_v1_writer_degrades_new_cleanup_evidence_to_legacy_ambiguity` removes
  both preflight and spawn tracking fields. Spawn tracking predates preflight
  evidence (commits `2b4bbc74`, `3dc0f3af`); selective preflight-field loss needs its
  own regression. A writer that preserves spawn tracking but drops preflight
  evidence can otherwise make incomplete cleanup appear recoverable.
- Inference: the reported incident is consistent with a retained orphan claim.
  Its exact original state and runtime version were not established. An analogous
  local stale record is not evidence identifying that incident. Do not copy local
  runtime records into this repository.
- Unknown: compatibility behavior of historical writers is not yet exercised.
  T-02 owns executable fixtures for selective field loss and old-reader rejection;
  automatic recovery cannot ship before that task passes.

## 4. Decisions and design

### D-01 — Use one assessment, with distinct observation and permission

Status: accepted for this plan. Move the strict assessment into shared owning-crate
logic usable by status, claim, stop, and repair. Return observed activity, recovery
eligibility, retention reason, and relevant exact identities. A persisted phase is
history, not proof of current activity. Keep control-channel probes outside locks.

Preserve the existing JSON `running` field as a documented compatibility field;
add explicit activity (`verified`, `possible`, `none`), cleanup-required and reason
fields. Human output uses these fields to distinguish running, activity uncertain,
cleanup required, and stopped. A status query must not mutate session/route records.
Do not redefine the legacy JSON field silently. Distinguish persisted route presence
from routability if displaying either. Include session IDs and the selected state
directory in recovery guidance; never expose control tokens.

### D-02 — Fence cleanup evidence before broadening automatic recovery

Status: accepted design direction, compatibility proof required in T-02. Use a new
session document version for complete cleanup obligations, with required fields
rather than omission meaning confirmed cleanup. New readers support v1 for inspection
and explicit cleanup; old readers must reject the new version before mutation.

Upgrade/create the document only under the shared lock when no sessions remain.
Never upgrade underneath active old supervisors. If a nonempty legacy store prevents
launch, explain the one-time cleanup/upgrade requirement without stopping sessions.
Include blocking session IDs and saved roots, including unrelated/no-proxy sessions;
if the summary is truncated, direct the operator to contextless `dev status --all`.
Do not label legacy evidence complete based solely on absent fields. Positive
pending evidence remains blocking; uncertain legacy evidence requires explicit
operator repair through the existing stop-only ambiguity override. Keep legacy
inspection and explicit cleanup available until the store is empty. Promotion of an
empty store and subsequent claim must be one serialized operation with safe retry.

Tradeoff: a shared legacy store may require coordinated explicit cleanup across
repositories before new launches. This is preferable to silently losing a cleanup
obligation. T-02 may propose a less disruptive encoding only with equivalent
old-writer exclusion and concrete compatibility proof; record a material design
revision before implementing it. Never downgrade a populated new-version document.

### D-03 — Retire eligible claims on launch, without stopping processes

Status: accepted planned policy change. After T-02, normal launch may retire only
overlapping claims from the same canonical repository whose evidence is complete,
supervisor is absent, and every registered app identity is absent. Reassess under
the shared lock; remove only exact-owned process routes, retire metadata, and claim
the new session without an intervening unlocked ownership window. Preserve aliases,
unrelated sessions, and routes owned by a replacement generation.

This changes the explicit-stop-only policy in the crate guide and developer docs;
update both in T-03. It proves registered identities absent, not all descendants
gone. Automatic recovery never uses the ambiguity override. Keep cross-repository
claims blocked and existing authenticated replacement of live same-repository
sessions unchanged. Preserve recovery notices on later startup failure/cancellation.

### D-04 — Exact-ID repair is explicit and contextless

Status: accepted planned interface. Add `jig dev status --all`,
`jig dev status --session ID`, and
`jig dev recover --session ID`, each accepting `--state-dir`. These modes bypass
repository discovery, even from inside an invalid or unrelated repository. Recovery
is metadata-only and applies the strict assessment; it cannot stop a live supervisor
or app. Validate the full ID, select exactly one record, use its persisted identity,
and revalidate under lock. Missing IDs are idempotent success, with zero retired
records. Prefixes/wildcards are not selectors. Global inspection exposes migration
blockers even when they have no hostname conflict; it has no global mutation partner.
Reject combining --all and --session. Keep repo-scoped status/stop defaults unchanged.

The new recover command has no ambiguity override. For ambiguous deleted-root
records, provide an explicit exact-ID selector on `dev stop` compatible with its
existing `--forget-ambiguous-orphans` repair; that selector must bypass repository
context and retain the existing live/uncertain-identity refusal. Document that stop
can request authenticated shutdown of the selected live session, while recover
cannot. No command implicitly expands an exact ID to other sessions.

### D-05 — Authenticate actual proxy capabilities before reuse

Status: accepted design direction. Obtain bind scope and effective TLS HTTP2 settings
from the serving runtime through a versioned authenticated loopback capability
response, bound to the existing PID/health-token generation. Preserve the existing
PID health response for older clients. Do not infer settings from the new caller,
service configuration, or stale files. Missing capabilities on an old proxy mean
unknown and require an explicit restart to upgrade; do not silently assume a match.

Reject LAN mismatch in either direction before spawning apps/publishing routes.
Compare HTTP2 when HTTPS is requested. Keep existing HTTP/HTTPS port checks and
allow an extra HTTPS listener when the caller only requests HTTP. Error output
reports requested/actual settings and state directory; restarting a shared proxy
remains an explicit operator action because other sessions depend on it. Preserve
loopback-only management responses and existing LAN alias restrictions.

## 5. Execution graph

### T-01 — Explain session activity and cleanup blockers truthfully
- Outcome: Status and conflict messages distinguish observed activity from retained cleanup obligations and provide actionable exact-session diagnostics.
- Context: D-01; classification exists in management but launch does not use it.
- Changes: `crates/jig-dev-proxy/src/dev_sessions.rs`, `dev_sessions/management.rs`, `dev_sessions/process_identity.rs`, `crates/jig/src/cli/output/dev.rs`, lifecycle/output tests, and `docs/public-contract.md`.
- Depends on: none
- Verify: Table-driven absent/live/uncertain supervisor/app cases, pending preflight/spawn, legacy fields, dead cross-repository claim wording, explicit state-directory guidance, token redaction, no session/route mutation on status, and backward-compatible JSON fields.
- Recovery: Additive output fields preserve existing consumers; human formatting can be reverted independently of the shared assessment.
- Done when: An orphan with no verified activity is never presented as verified running, and every retained claim includes an accurate reason and exact ID without implying a route exists.

### T-02 — Preserve cleanup obligations across runtime versions
- Outcome: Old writers cannot silently erase evidence used to authorize cleanup or automatic retirement.
- Context: D-02; first reproduce selective loss of only preflight evidence, preserving tracked NotStarted apps.
- Changes: `crates/jig-dev-proxy/src/state/dev_sessions.rs`, `state/dev_session_store.rs`, `state/dev_sessions/tests.rs`, session initialization/version-aware read paths, crate guide, and compatibility documentation.
- Depends on: none
- Verify: Historical-shape selective-field-loss fixture, malformed/missing required fields, old-reader rejection of new version, read-only legacy inspection, nonempty-store refusal without mutation and with every blocking ID discoverable, serialized empty-store promotion versus an old concurrent claim, and partial-write retry.
- Recovery: Leave nonempty legacy documents in place; never erase obligations, rewrite version numbers by hand, or downgrade populated state. Drain/explicitly repair with a compatible runtime before promotion.
- Done when: Unknown or dropped evidence cannot become confirmed cleanup, new state excludes legacy writers, and empty-store upgrade has a tested operator path with no automatic stopping of other repositories.

### T-03 — Recover eligible stale claims during normal launch
- Outcome: A normal launch succeeds after a prior supervisor and its registered apps exit, despite retained claims and missing routes, when durable evidence permits retirement.
- Context: D-03 deliberately changes normal-launch policy; use T-01 assessment and T-02 provenance/version guarantees.
- Changes: `crates/jig-dev-proxy/src/dev_sessions.rs`, shared state mutation/retirement helpers, `dev_outcome.rs`, lifecycle and CLI tests, crate guide, and `docs/developer-ux.md`.
- Depends on: T-01, T-02
- Verify: Dead eligible overlapping claim with zero routes; unrelated claims and aliases preserved; pending/legacy/uncertain/live identities refused; PID reuse without signaling; concurrent launches yield one owner; injected route/session-write failure retries safely; recovery notices survive failed or cancelled new startup; live --replace still uses authenticated control.
- Recovery: Preserve conservative records on any incomplete mutation. Reverting auto-retirement restores explicit cleanup while keeping new-version reader support.
- Done when: Generic restart regression succeeds without --replace, strict negative cases remain blocked, and no process-control authority or cross-repository takeover is added.

### T-04 — Inspect and recover exact sessions without a repository
- Outcome: An operator can inspect and repair the exact orphan identified by a conflict even after the old repository root is gone.
- Context: D-04; both CLI dispatch and library canonicalization currently require a repository.
- Changes: `crates/jig/src/cli/proxy.rs`, `cli/run.rs`, `command/proxy.rs`, `dev_proxy/commands/dev.rs`, `crates/jig-dev-proxy/src/types.rs`, `dev_api.rs`, session management, CLI/lifecycle tests, public command inventory and developer documentation.
- Depends on: T-01, T-02
- Verify: Moved/deleted root; non-repository cwd and malformed unrelated config; global discovery of an unrelated deleted-root/no-proxy legacy migration blocker; mutually exclusive selectors; full versus partial IDs; missing ID idempotence; alternate state directory; exact target isolation; strict recover refuses live/uncertain/pending evidence; exact stop preserves authenticated control and the existing explicit ambiguity-repair restrictions; outputs omit credentials.
- Recovery: New selectors are opt-in; existing repo-scoped commands retain their scope. Metadata-only recovery uses route-first retirement and retryable exact identity rechecks.
- Done when: The documented conflict command works without the old path, affects only the selected session, and reports metadata retirement separately from process stopping.

### T-05 — Refuse incompatible shared proxy configuration
- Outcome: Dev/proxy startup cannot silently ignore LAN or HTTPS HTTP2 requirements when reusing a running proxy.
- Context: D-05; ports and PID are checked today, effective listener capabilities are not.
- Changes: `crates/jig-dev-proxy/src/server.rs`, `ports.rs`, `processes/proxy.rs`, proxy status/output as needed, proxy integration tests, CLI help, and developer documentation.
- Depends on: none
- Verify: Existing loopback proxy plus --lan; existing LAN proxy plus --no-lan; both HTTP2 mismatch directions under HTTPS; matching reuse; HTTP-only request behavior; old server without capability endpoint; token/PID-generation mismatch; capability data denied to LAN clients; mismatch occurs before app spawn or route publication and leaves other sessions intact.
- Recovery: Never restart the shared proxy automatically. Keep legacy health probes working; unknown capabilities produce an explicit upgrade/restart action preserving state-directory and desired settings.
- Done when: Matching reuse succeeds, incompatible/unknown configurations fail with actionable diagnostics, and unrelated sessions are not stopped or reconfigured.

The dependency table is each task's `Depends on` field. The gating chain is T-02
(and T-01) before T-03/T-04; T-05 has no dependency. Start T-01, T-02, or T-05 after
claiming its Beads issue. Prefer T-02 first to settle compatibility risk. No duration
estimate is asserted. T-01/T-02 and T-03/T-04 overlap session files: serialize their
edits even where the graph permits concurrency. T-05 can be isolated at crate-level
proxy surfaces, but coordinate shared CLI/docs edits. Tracker edges are not locks.

## 6. Verification

During implementation, use isolated private temporary state and unprivileged ports.
No tests may consume the operator's real registry. Run focused crate tests for each
changed behavior (`cargo test -p jig-dev-proxy` with an appropriate filter) and CLI
tests (`cargo test -p jig-sh` with the applicable test target/filter). Before closing
implementation work, run configured `scripts/jig work check --plan-id ID`, inspect
gates/receipts, and finish backend work with `scripts/jig check test` as required by
AGENTS. Launcher/runtime changes require the configured source-runtime check.

Epic acceptance requires all task regressions, a generic stale-claim restart across
the new state version, legacy upgrade/repair coverage, deleted-root exact recovery,
and two-direction LAN mismatch coverage. Document macOS and Linux observations
separately; an unrun platform test is not a pass. Each task owns its implementation
tests; there is no separate generic review/testing issue.

Planning verification uses the skill's `scripts/validate_plan.py`, a reread of Beads
parent/blocking edges and ready roots, and `python3 scripts/beads-sync.py --check`.

## 7. Rollout and recovery

Ship diagnostics and compatible readers before enabling automatic retirement. Fence
new evidence with the new document version, and require explicit legacy cleanup
before an empty-store upgrade. Never run old workers against upgraded populated
session state. Release gating: do not enable new-version writes/nonempty-legacy
launch refusal until T-04's contextless discovery and repair commands are available;
T-02 can implement/test the fence earlier without releasing that cutover alone.
Preserve the route format, existing health probe, and existing JSON
fields; new proxy capability checks detect old daemons and request an explicit
restart. Do not silently restart a proxy that other repositories are using.

Record recovery notices on success and on later failure, with exact session ID,
reason, and affected apps but no tokens. Stop rollout if compatibility fixtures lose
obligations, a new client accepts unknown listener capabilities, or an exact repair
touches another session. Roll forward using a version-aware runtime; rollback must
retain readers for any already-written state version. These changes cannot resurrect
a retired session or prove an unrecorded descendant never existed.

## 8. Risks and open decisions

| Risk or unknown | Owner and resolution |
| --- | --- |
| Selective legacy evidence loss creates false eligibility | T-02 executor owns reproduction and version fencing; blocks automatic retirement and exact repair delivery. |
| Legacy upgrade requires other repositories to stop | T-02 documents the explicit one-time migration; no automatic shutdown. Revise D-02 only with equivalent compatibility proof. |
| Human status fix breaks JSON clients | T-01 keeps legacy fields and adds explicit observation fields; schema/output regressions. |
| Dead recorded leaders do not prove descendant absence | T-03/T-04 retain strict evidence checks and precise recovery language; never signal numeric registry PIDs. |
| Shared proxy upgrade affects unrelated sessions | T-05 refuses incompatible reuse; operator controls restart. |
| Actual original incident differs from the source-derived mechanism | All tasks use reproducible generic fixtures; no claim of reproducing the original incident. |

Local helper names and module splits are reversible executor choices. The policy
changes and compatibility choices in D-02 through D-05 are explicit plan decisions;
record any changed invariant before implementation. No unresolved user preference
blocks the first safe tasks. This plan does not authorize actual runtime repair.

## Progress

- [x] Investigated source paths and existing regression coverage.
- [x] Reviewed process-ownership and mixed-version risks independently.
- [x] Validate the canonical plan and export its five tasks under one Beads epic.
- [ ] Implement T-01 through T-05 (not authorized by the planning request).

Restart checkpoint: planning/export delivered; implementation remains open in Beads. Jig work ID
`plan_01M33ZWXV3JY4KHASHNDJ31PAQ`; baseline above. Existing unrelated work records
and a pre-existing Beads diff were preserved. This temporary structured-work wrapper
is superseded by the canonical plan and epic; implementation tasks must open their
own work at their actual execution baseline. First safe action: inspect/claim
`jig-sh-3ykb.2` and reproduce selective legacy evidence loss in an isolated fixture.
Runtime behavior remains unmodified.

## Surprises & Discoveries

Selective preflight-field loss was not covered by the existing broad legacy test.
It adds a necessary compatibility prerequisite before enabling automatic recovery.
The initial investigation's local stale record was analogous, not the reported
repository's exact record; no private identifying details are retained here.

## Decision Log

2026-09-22: Chose five delivery tasks, including version-safe cleanup evidence as a
prerequisite discovered during focused review. Preserved legacy JSON semantics,
selected explicit exact-ID repair, and rejected automatic shared-proxy restart.
Automatic same-root retirement is a planned policy change, not existing behavior.
Focused review added global read-only discovery for unrelated deleted-root/no-proxy
migration blockers and a release gate tying version cutover to available repair.

## Outcomes & Retrospective

Planning/export is complete. The structural validator passed with zero errors; its
one isolated-task warning is intentional because T-05 has no session-recovery
dependency. Beads has one epic, five open children, four blocking edges, and ready
roots T-01/T-02/T-05. Each issue retains acceptance, verification, recovery, and its
canonical task link. The sanitized export check passed. Focused independent review
and self-review covered G1-G6; the material migration-discovery gap was corrected.
No implementation tests or runtime repairs were performed; all implementation
acceptance remains future work. Existing workspace-wide implementation receipts are
not evidence that this proposed work has passed its implementation gates.
