# T1: Persist portable Beads work links and tracker-operation intent

This ExecPlan implements Beads issue `jig-sh-x8ow.1`, the first task in epic
`jig-sh-x8ow`. The outcome is an additive, versioned state contract that can
immutably associate an existing Jig plan with a portable Beads issue identity and
can durably record tracker-operation intent before later tasks execute external
side effects. This task does not configure or invoke `br`, add lifecycle CLI/MCP
commands, claim an issue, or change the current repository-rendering epoch.

Acceptance is observable when exact link retries append nothing, conflicting links
are rejected and diagnosed, operation state replays after restart, torn or unknown
authority fails closed, pending operations protect their evidence during archive,
old plan/baseline/receipt bytes remain unchanged, and an epoch-8 runtime cannot
write the new epoch-11 journals.

## Progress

- [x] Read `AGENTS.md`, `agent-map.md`, `crates/jig/AGENTS.md`, `.agent/PLANS.md`,
  the epic delivery plan, and the relevant state/compatibility code.
- [x] Create feature branch `feature/jig-sh-x8ow-beads-evidence`, preserve the
  initial planning residuals in commit `66248573`, claim `jig-sh-x8ow.1`, and open
  structured work.
- [x] Inspect journal append/replay, diagnostics, archive/restore, launcher handoff,
  contract epochs, and receipt freshness compatibility.
- [x] Milestone 1: add epoch-11 journal capability and the immutable work-link
  schema, projection, validation, and append API with focused tests.
- [x] Milestone 2: add tracker-operation event schema, replay/projection, fail-closed
  mutation rules, and restart/corruption tests.
- [x] Milestone 3: integrate the new streams into diagnostics and protect pending
  operation roots during archive/restore without pruning either new journal.
- [x] Milestone 4a: document the durable contract and pass focused journal,
  diagnostics, compatibility, archive, dependency, and restore regressions.
- [x] Milestone 4b: pass Clippy and the required repository verification through
  the rebuilt development binary.
- [x] Run the requested review-fix loop in comprehensive mode at minimum severity
  medium and one bounded continuation. Both bounds were honored; after the loop
  stopped, apply only the separately approved final corrections.
- [x] Obtain a fresh independent Claude/Codex verification of the final source.
  Codex reported no findings; Claude's sole medium report was dismissed because
  the cited launcher-only path is already preceded by the newer-contract preflight.
  Record all remaining low risks as non-blocking Beads residuals.
- [x] On convergence, finish structured work, close and sync the Beads task, and
  commit the complete T1 change before beginning T2.

Restart checkpoint: the current milestone is Milestone 4b. Focused regressions,
formatting, and strict Clippy pass after the separately approved final corrections.
The next action is to rebuild the development binary, execute the configured Jig
gates, and obtain a fresh read-only comprehensive verification. There are no known
blockers. Structured plan ID is
`plan_01M2JAA90VFGKFB1HR2QEH0QEP`; its exact baseline is
`662485730e0c33a74953de5efdd9ef7cbb91fab2`. The worktree contains the complete T1
implementation, structured work state, the Beads claim/export, and this plan.
Build runtime changes with `cargo build -p jig-sh --bin jig` and invoke harness
commands with `JIG_DEV_BIN=target/debug/jig`.

## Surprises & Discoveries

- `PlanEvent` uses a custom legacy serde representation that can discard unknown
  fields. Adding the issue identity there would couple link durability to plan
  projection and violate the requirement to preserve historical plan bytes.
- Versions 9 and 10 are historical receipt-freshness epochs rather than valid
  repository manifest epochs. In particular, authentic epoch-9 receipts omit
  `source_state`, while epoch 8 and epochs at least 10 include it. The next
  repository capability must therefore be 11, and supported repository epochs
  cannot be modeled as one contiguous range.
- The installed launcher cache is keyed by contract epoch. Activating epoch 11 in
  T1 before T2's strict configuration exists could leave a cached T1 runtime
  claiming compatibility with a later T2 repository. T1 must understand epoch 11
  but keep `CURRENT_CONTRACT_VERSION` and generated repository artifacts at 8;
  T3 will perform the coordinated activation after T1 and T2 exist.
- `append_jsonl_locked` does not by itself establish an invariant across scan and
  append. Each authoritative writer must scan, validate, and append inside one
  `with_jsonl_write_lock` critical section and must refuse an unterminated tail.
- Run archive currently executes before receipt archive. Pending-operation roots
  must be projected once under an operation coordination lock and supplied to both
  archive paths so a newly appended intent cannot race between their snapshots.
- Existing receipt dependency traversal required target-shaped roots. Explicit
  tracker receipt IDs can also name state-tool receipts, so archive now retains an
  exact root first and follows freshness dependencies only when target metadata is
  present.
- The repository's line-union merge policy means physical JSONL order is not a
  stable lifecycle order across branches. Operation replay must therefore validate
  an unordered fact set, while the append boundary separately enforces the next
  sequential transition.
- Caller timestamps and event IDs are not a sequential write clock. The
  lock-protected physical tail is the append transition authority; timestamp/event
  ordering exists only to make a merged read projection deterministic.
- Work-link snapshots are historical observations rather than issue identity.
  Branches may legitimately attach the same plan/issue with different snapshots,
  so those records converge on the lowest event ID instead of conflicting.
- The generic append primitive synced record data but not a newly created file's
  parent-directory entry. Tracker intent durability requires both sync boundaries
  before a caller may proceed to an external side effect.
- A visible exact record is not by itself proof that a prior append completed its
  sync boundary. Retry success must explicitly re-sync that file and its directory.
- Checking record size inside a visitor is too late because the unbounded scanner
  has already buffered the physical line. Both tracker journals now select the
  streaming scanner's own per-record limit for reads and locked append validation.
- A terminal outcome alone cannot dominate facts introduced by another union-merge
  branch. Acknowledgements must name the nonterminal event IDs they resolve, and
  the operation remains pending until that causal closure covers the merged set.
- Bounded semantic readers are insufficient when generic diagnosis scans the same
  streams first. Shallow and deep stream inspection must select the journal-specific
  record limits before decoding or collecting size statistics.
- Requiring one acknowledgement to resolve the entire accumulated history conflicts
  with the record-local 64-reference bound. Partial compatible acknowledgements must
  be appendable while only their collective closure can terminalize the operation.
- Restore protection is a preservation predicate, not a blanket pending-state ban.
  Validated backups can safely replace unrelated history when every protected raw
  receipt or run-event record remains present; exact no-ops replace nothing.
- Sparse repository-epoch support must stay independent of the current/default
  epoch. Otherwise the planned epoch-11 activation would silently admit reserved
  epochs 9 and 10 and produce an inaccurate launcher compatibility label.
- Restore protection must inspect damaged current state only when pending roots make
  it relevant. Without such roots, a valid backup remains the recovery path for a
  malformed or torn receipt/run journal.

## Decision Log

- 2026-09-15: Keep work links separate from `plans.jsonl`. A dedicated
  `.agent/state/work-links.jsonl` preserves plan event IDs, baseline events, plan
  bodies, and receipt interpretation and lets unrelated standalone plans load even
  when link state is damaged.
- 2026-09-15: Reserve repository contract epoch 11 for tracker authority while
  leaving the rendered/current epoch at 8 in T1. Internal repository loading can
  exercise epoch 11 and writer APIs require exactly that capability, but the
  launcher cache probe advertises only active epochs 2 through 8 until cutover.
  Receipt freshness continues to read its historical 8-through-11 semantics.
  Activation and tracker configuration remain T2/T3 work.
- 2026-09-15: Treat both new journals as immutable and unpruned in T1. Completed
  operation compaction is deferred until later lifecycle tasks define a terminal
  record sufficient to reconstruct outcomes. Maintenance must preserve their raw
  bytes, including unknown future records.
- 2026-09-15: A newline is part of the commit boundary. Any unterminated final
  record, even syntactically valid JSON, is uncommitted and blocks mutation or
  destructive maintenance where hidden authority could matter.
- 2026-09-15: T1 validates only bounded syntax and stable identity: provider,
  workspace ID, canonical issue ID, exact repository-relative root `.beads`, and
  snapshot digest. Equality with configured tracker authority belongs to T2/T3.
- 2026-09-15: Pending retention roots include plan IDs plus explicit receipt and
  run IDs. Corrupt, conflicting, truncated, or unsupported operation authority
  aborts relevant archive/restore before any backup or rewrite. A referenced ID
  absent from an active journal has no active bytes to retain and does not block
  archival of unrelated records.
- 2026-09-15: Comprehensive review round 1 verified four medium defects. Repair
  makes operation replay line-order independent with compatible terminal fact
  convergence, shares portable issue validation, makes work-link write safety
  journal-wide, aligns missing receipt and run-root behavior, and separates future
  epoch understanding from launcher cache activation. Focused regressions, strict
  Clippy, formatting, and file-budget validation pass; a fresh full review is next.
- 2026-09-15: Fresh review found that sorted read projection was still being used
  to authorize an append and that same-issue links from separate branches could
  conflict solely because their observation snapshots differed. Repair validates
  each append against the physical journal tail while locked, treats plan/issue as
  the immutable link identity, and keeps lowest-event-ID selection as the merged
  snapshot rule. Capability-only runtime and doctor probes now consistently reject
  internally understood but inactive epoch 11, and diagnostics classify competing
  terminal facts as conflicts.
- 2026-09-15: The next review found two independent retention/durability defects.
  The shared JSONL append now syncs the containing directory after every append,
  and explicitly rooted tracker receipts retain complete transitive proof regardless
  of an expired freshness window. A missing receipt journal remains an empty set of
  active bytes rather than blocking unrelated archive work.
- 2026-09-15: The terminal pass exposed an ambiguous-retry hole in that durability
  repair and unbounded pre-validation line buffering. The final repair round moves
  directory syncing behind a tracker-only append API, re-confirms both sync
  boundaries for exact retries, and routes locked and unlocked reads through the
  streaming per-record limits. Fault injection covers failures before file and
  directory sync; oversized terminated and unterminated records cover both journals.
- 2026-09-15: The continuation review found that a branch-local acknowledgement
  could falsely terminalize an unresolved merged attempt, and that generic state
  diagnosis bypassed the semantic scanners' memory ceilings. Version-1
  acknowledgements now carry `resolves_event_ids`; compatible acknowledgements
  terminalize only when their union covers every attempt, observation, and error.
  Shallow and deep diagnosis enforce the 512-KiB work-link and 64-KiB operation
  limits, with oversized work-link authority classified as corrupt. Projection-order
  and receipt-archive regressions prove an unresolved branch remains protected.
- 2026-09-15: A later continuation separated the existing terminal-safety invariant
  from a new append-liveness defect introduced by its repair. Acknowledgements may
  now resolve bounded subsets, with a 66-fact regression proving two records remain
  pending and then converge safely. Receipt and run restore protection now compares
  the current protected raw-record multiset with the validated backup under the
  existing locks: omission refuses without a recovery rewrite, preservation permits
  unrelated rollback, and an identical no-op remains usable with damaged tracker
  authority.
- 2026-09-15: The post-repair review found two cutover/recovery regressions. Contract
  support, activation, cache retirement, and labels now derive from the sparse
  supported epoch set, with a simulated epoch-11 cutover retaining the 9/10 gap.
  Receipt/run restore skips current-stream protection scans when no relevant pending
  roots exist, while retaining fail-closed parsing when roots require preservation.
- 2026-09-15: After the bounded loop stopped, the user approved a one-off correction
  for its remaining restore defect. Archive and restore now share the exact tracker
  receipt dependency-closure implementation. Restore hashes every current raw record
  in that closure, so a cross-plan dependency cannot disappear while its explicit
  root survives; a focused regression proves refusal and successful preservation.
- 2026-09-15: Fresh read-only verification of that correction found two related
  authority gaps. Pending plan roots now use the same configured gate/evidence
  selection and transitive dependency closure in archive and restore, rather than
  pinning every receipt carrying the plan ID. Replay also rejects a terminal
  acknowledgement of an attempted side effect when the merged history has no
  observation or error evidence. Regressions prove cross-plan dependencies survive,
  arbitrary same-plan receipts remain replaceable, malformed raw authority blocks
  maintenance without mutation, and valid merged branches remain supported.
- 2026-09-15: Final independent review covered the complete 44-path working-tree
  scope at fingerprint
  `1ca4a7d6fbca936cdf00c16c30657bd948618ac8e54b81fa67fee792c502c468`.
  Codex reported no actionable findings. Claude's medium launcher-only finding was
  not actionable because `prepare_update` calls `reject_newer_declared_contract`
  before dispatching either update mode, so epoch 11 cannot reach rendering or
  cache seeding while epoch 8 is current. The no-version private repository probe
  intentionally follows the documented internal-loading path; generated launchers
  provide an explicit active epoch. Low residuals are tracked by `jig-sh-ofk5`,
  `jig-sh-gqpf`, `jig-sh-mz14`, and `jig-sh-gcnw`.

## Outcomes & Retrospective

Implementation, repository verification, independent review, structured-work
finalization, and Beads closure are complete. The T1 commit is the remaining
handoff step.
Focused evidence currently includes 17 work-link tests, 18 operation-journal tests,
8 state-diagnostics tests, compatibility/freshness/probe tests, pending and terminal
receipt/run archive tests, corrupt/future/torn no-mutation archive tests, transitive
freshness retention, and protected receipt/run restore refusal and preservation.
`cargo check -p jig-sh`
passes without warnings after intentionally marking the T1-to-T3 API bridge. The
work-link tests prove closed-plan attachment, byte-identical plan/receipt history,
strict duplicate-key rejection, and eight-thread idempotent append. Review
round 1 repairs also prove union-merge convergence, journal-wide write blocking,
portable operation identities, missing-root archive progress, and inactive epoch-11
cache compatibility. The second repair adds regressions for backdated append facts,
same-issue snapshot retries and branch unions, implicit inactive-epoch capability
probes, doctor reporting, and terminal-conflict diagnostics. Review convergence and
final Jig gate receipts remain unfinished. The continuation repair additionally
proves that acknowledgement closure is independent of merged physical order, that
an unresolved branch receipt survives real archive, and that both generic and
semantic diagnostic paths reject oversized tracker records with bounded memory.
The latest continuation additionally proves multi-record acknowledgement closure,
byte-exact protected receipt/run survival across restore, omission refusal, and
identical restore no-op behavior when tracker authority is damaged. The approved
one-off corrections also prove that an explicitly rooted receipt and a dependency
from another plan must both survive restore; pending plan roots preserve the same
configured evidence closure used by archive without overprotecting unrelated
same-plan receipts; and a raw Attempt-to-Acknowledgement shortcut cannot discard
retention authority without reconciliation evidence. The rebuilt development
binary passed the required Jig test gate with 4,125 tests passed and 3 skipped
(`run-plan_sha256:8e094e6f5e45ff912610c2d9c1892309b887a941479f8eda7646f1e9dc73cde9`).
Jig formatting, strict Clippy, contract, focused archive, and file-budget checks
also pass.

## Context and orientation

`crates/jig/src/state.rs` owns runtime state modules and coordinates archive.
`state/records.rs` contains common durable record types, `state/jsonl.rs` supplies
raw bounded JSONL scanning and locked append helpers, and `state/plans.rs` supplies
the existing plan-existence check. New modules `state/work_links.rs` and
`state/tracker_operations.rs` own their streams rather than changing legacy plan
serialization.

`state/diagnostics.rs` has a hard-coded `STATE_STREAMS` inventory used for stream
and Git-policy facts. `state/runs/archive.rs` and `state/receipts/archive.rs` decide
which historical evidence survives archive; `state/maintenance.rs` implements
restore. These paths must learn about pending-operation authority while leaving
the new journals byte-identical.

`crates/jig/src/context.rs` defines current and maximum contract versions.
`context/runtime.rs` and `cli/run.rs` implement capability probing and launcher
handoff. `crates/jig-contract/src/freshness.rs` separately interprets receipt
epochs. Repository epoch support must be explicit because 9 and 10 are not valid
manifest epochs even though they remain meaningful to receipt validation.

The delivery contract and rationale are in
`docs/plans/beads-work-integration.md`, section "T1". Public durable-state behavior
is documented in `docs/public-contract.md`; `docs/repo-intent.md` contains the
stream inventory. Existing `.gitattributes` already covers every
`.agent/state/*.jsonl` file.

## Plan of work and milestones

### Milestone 1: compatibility and immutable link authority

Introduce named constants/helpers for supported repository epochs so runtime
capability accepts 11 but not reserved 9/10, while current rendering stays at 8.
Extend compatibility tests to prove the exact set and receipt freshness at 11.

Define a bounded V1 link envelope containing an event ID, plan ID, provider
(`beads` is the sole authoritative T1 value), portable workspace ID, canonical
issue ID, exact `.beads` tracker root, observation timestamp, snapshot title and
acceptance context, deterministic digest, and `start` or `attach` origin. The
digest uses a documented domain-separated, length-prefixed canonical encoding.

The reader scans raw newline-terminated values strictly. Projection distinguishes
unlinked, one supported link, exact duplicate replay, immutable conflict,
unsupported authority, and corruption. Complete semantic JSON, including unknown
fields, determines whether a duplicate event ID is exact or divergent. The writer
requires an existing plan and epoch-11 capability, then scans and appends under one
lock. An exact retry returns the existing link without writing; every distinct
link for that plan fails without changing bytes.

### Milestone 2: recoverable operation facts

Define a bounded V1 operation envelope with stable event and operation IDs, kind,
the same immutable plan/issue identity, fact phase, timestamp, attempt/cause
correlation, explicit referenced receipt/run IDs, and acknowledgement resolution
IDs that causally close attempt, observation, and error facts. Initially supported kinds
cover the epic's future backlink, claim, complete-issue, and export operations;
supported phases distinguish intent, attempt, observation, acknowledgement, and
error. Later lifecycle tasks own command-specific payloads and transition policy.

Projection groups complete event histories, rejects identity drift and same-event
ID divergence, and determines whether an operation remains pending. Error and
uncertain observation facts remain pending until durable acknowledgement or a
definitive no-effect fact. Unknown schema/provider/kind/phase and malformed known
records are visible but never authoritative. Append uses a stable caller-supplied
event ID, is idempotent for an exact replay, and fails closed on conflict or an
unterminated/malformed/unsupported journal.

### Milestone 3: diagnosis and maintenance safety

Add both files to the state-stream inventory and emit bounded semantic diagnostic
facts for supported, unsupported, conflicting, corrupt, and torn records. Missing
files remain a read-only empty state and create nothing.

Project one `PendingTrackerRetentionRoots` before archive mutation. Union its plan
IDs into run/receipt plan protection and seed exact referenced receipts/runs before
dependency closure. Hold the tracker coordination lock through both archive
operations, with lock order tracker operations, then runs, then receipts. Abort
before backup/rewrite when operation authority is ambiguous. Refuse receipt restore
that would replace protected pending evidence. Do not archive or restore either new
journal in T1.

### Milestone 4: contract documentation and verification

Document filenames, V1 fields, immutability, commit boundary, compatibility gate,
unknown-schema behavior, and no-pruning policy in `docs/public-contract.md`; update
the exact stream inventory in `docs/repo-intent.md`. Do not add tracker config,
adapter commands, templates, or end-user lifecycle surfaces.

Run focused tests while implementing, then format, Clippy, contract, and repository
tests through the development binary. Rebuild the binary after runtime edits before
structured Jig commands. Inspect the final diff for fixture hygiene and ensure new
records contain no absolute or machine-local paths.

## Concrete steps

All commands run from the repository root.

1. Edit compatibility and state modules with focused unit tests, then run the
   narrowest affected packages/tests with `cargo test -p jig-sh <filter>` and
   `cargo test -p jig-contract <filter>`. Expected: supported/unsupported epoch,
   link replay, operation replay, and corruption cases pass.
2. Integrate diagnostics and archive/restore. Run focused state diagnostics,
   receipt archive, run archive, and maintenance tests. Expected: pending roots
   survive, terminal state restores normal eligibility, and ambiguous authority
   produces no backup or source-byte change.
3. Run `cargo fmt --all -- --check`, rebuild with
   `cargo build -p jig-sh --bin jig`, then run
   `JIG_DEV_BIN=target/debug/jig scripts/jig check test`,
   `... scripts/jig check fmt`, `... scripts/jig check clippy`, and
   `... scripts/jig check contract`. Expected: all applicable checks pass and
   receipts attach to this plan where configured.
4. Run `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id
   plan_01M2JAA90VFGKFB1HR2QEH0QEP`, followed by `work gates`, `work evidence`,
   and `work receipts`; record observed results here.
5. Execute the requested context-free review-fix loop over the task's working-tree
   scope with exact Beads-state exclusions returned by its preflight. Address all
   verified findings at severity medium or above, rerun affected checks, and obtain
   a fresh converged review. Log verified residual low findings only through the
   loop's Beads helper. If the bounded loop fails to converge, run one continuation;
   stop and report rather than closing T1 if that also fails.
6. On convergence, finish the Jig work plan, close `jig-sh-x8ow.1`, run
   `python3 scripts/beads-sync.py`, verify no private fixture data, and commit T1.

## Validation and acceptance

Focused regressions must prove:

- reading absent journals is side-effect free;
- open and closed existing plans can be linked without changing plan, plan-body,
  baseline, or receipt bytes;
- exact retry, concurrent retry, a different issue, and same-ID divergent unknown
  fields have deterministic outcomes;
- malformed, interior-corrupt, future-schema, and well-formed-but-unterminated
  records have distinct fail-closed behavior and useful diagnostics;
- replay after constructing a fresh context reproduces pending and acknowledged
  operation projection without inventing success;
- pending operations protect closed-plan runs, exact receipts, and transitive
  receipt dependencies; terminal records restore normal archive eligibility;
- ambiguous operation authority aborts archive before any backup/rewrite, and
  restore cannot replace pending evidence;
- epochs 2 through 8 and 11 have intended internal capability behavior, while the
  active cache probe advertises only 2 through 8, repository epochs 9 and 10 are
  rejected, and old/current epoch 8 cannot write new authority;
- generic fixtures such as `ExampleProject`, `ExampleVault`, `example-123`, and
  `plan_example` are used everywhere.

Repository completion requires the configured Jig test, fmt, Clippy, and contract
checks plus a clean requested review-fix convergence. Generated structured-work
receipts are evidence only after their commands actually run; expected checks in
this plan are not proof.

## Idempotence and recovery

Journal append is retry-safe only with a stable event ID and semantically identical
record. Writers do no repair: a torn tail, malformed known record, unsupported
schema, or identity conflict leaves bytes untouched and returns an actionable
error. Operators preserve and inspect the raw journal before any manual recovery.

Because both journals are append-only and T1 maintenance never rewrites them,
interrupted implementation can resume after reconciling this checkpoint with Git
status and current files. Archive must compute all pending roots before creating a
backup or rewriting runs/receipts; failure before that boundary is safe to retry.
Never call `br` while a state lock is held.

## Interfaces and dependencies

The state layer will expose typed link commit/projection and operation
append/projection APIs plus a `PendingTrackerRetentionRoots` value consumed by
archive/restore. Concrete Rust names may follow existing module conventions, but
callers must supply stable IDs and typed portable identity rather than paths or raw
tracker JSON. T1 depends only on existing serde/JSONL, hashing, IDs, time, context,
and state modules; it must not introduce a Beads library or subprocess dependency.

T3 consumes the link and operation interfaces after T2 supplies committed config
identity and the external adapter. Mutation authority will ultimately require a
supported link projection, exact T2 configuration identity, and a supported adapter
profile. T5 consumes diagnostic/projection state, and T4/T6/T7 add specific
operation payloads and terminal transitions.
