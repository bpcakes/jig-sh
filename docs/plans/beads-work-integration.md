# Link Beads tasks to Jig execution evidence

## Status and intended outcome

This is a delivery plan for one Beads epic.
It is not an implementation receipt or a task-local ExecPlan.
The user authorized planning and creating the delivery graph on 2026-09-15.
Implementation has not begun.
All interfaces marked proposed below must be implemented before use.

The outcome is a coherent workflow in which an agent selects a Beads issue,
starts a linked Jig execution record, obtains validation evidence, and can
explicitly complete the issue without maintaining two task descriptions.
The same workflow must work in this source repository and in projects using
Jig's full init/adopt harness.
Repositories without Beads keep their existing standalone Jig workflow.

The epic owns the overall outcome and dependencies.
Each delivery bead owns its acceptance criteria and implementation scope.
Complex beads create their own ExecPlan during implementation.
No bead exists solely to plan, review this document, or create more beads.

## Grounding and baseline

Inspected source baseline:
`34b12451e5123bc1c7ec309e2add8e55b79524e6`.
The initial worktree was clean.
The installed tracker reports `br 0.5.7`.
The installed graph viewer reports `bv v0.23.0`.
Neither executable was upgraded by this planning session.

Verified repository entry points:

- `AGENTS.md`: independent Jig and Beads workflow blocks.
- `agent-map.md`: backend guidance index.
- `templates/project/AGENTS.md.jinja`: generated Jig instructions.
- `templates/project/.agent/PLANS.md.jinja`: generated ExecPlan guidance.
- `crates/jig/src/cli/work.rs`: human command arguments.
- `crates/jig/src/command/work.rs`: runtime request structures.
- `crates/jig/src/runtime/work.rs`: shared start/finish behavior.
- `crates/jig/src/tool_defs.rs`: runtime-owned MCP input schemas.
- `crates/jig/src/state/records.rs`: persisted event shapes.
- `crates/jig/src/state/plans.rs`: plan creation and baseline capture.
- `crates/jig/src/state/execution_leases.rs`: repository execution leases.
- `crates/jig/src/context/work_config.rs`: strict work configuration.
- `crates/jig/src/cli/output/work.rs`: human work output.
- `crates/jig/src/status.rs`: repository status aggregation.
- `crates/jig/src/ui/source/`: dashboard sources and retained epochs.
- `crates/jig/src/bootstrap/managed_paths.rs`: managed block ownership.
- `crates/jig/src/bootstrap/answers/`: persisted setup answers.
- `crates/jig/src/bootstrap/repository_model/adoption.rs`: adoption model.
- `crates/jig/src/doctor/`: runtime diagnostics.
- `scripts/beads-sync.py`: project-owned export privacy workflow.
- `.github/workflows/repo-policy.yml`: exported Beads privacy validation.
- `docs/public-contract.md`: gates and metadata freshness ownership.
- `docs/configuration.md`: user-facing runtime configuration.

Relevant observations:

- Work start currently requires a title and accepts a body and Git baseline.
- It creates a session and a plan; it does not create an issue reference.
- Plan event serialization passes through `LegacyPlanEvent` explicitly.
- Unknown event fields are not a safe extension mechanism for old writers.
- Finishing a plan evaluates gates while holding a repository lease.
- `work finish` currently requires the plan to be open.
- Evidence inspection can inspect closed plans.
- Work configuration rejects unknown fields.
- `.agent/state` is append-only repository memory.
- Full adoption preserves root guidance outside Jig's managed block.
- The minimal harness does not manage the full root AGENTS block.
- Existing plan bodies already mention Beads IDs manually.
- Root `.beads` can explicitly be declared receipt metadata.
- That freshness declaration is an ownership decision, not auto-detection.
- This repository validates Beads export content in CI.
- Its export helper clears machine-local source paths through `br` first.
- The helper handles tombstone export redaction as a narrow privacy operation.
- Existing unrelated issues cover generic JSONL recovery and lock bounds.
  This epic must use or improve shared infrastructure without duplicating them.

External source references inspected during planning:

- `https://github.com/Dicklesworthstone/beads_rust`
- `https://github.com/Dicklesworthstone/beads_rust/blob/v0.5.7/src/cli/commands/update.rs`
- `https://github.com/Dicklesworthstone/beads_rust/blob/main/docs/CLI_REFERENCE.md`
- `https://github.com/Dicklesworthstone/beads_rust/blob/main/src/model/mod.rs`
- `https://github.com/Dicklesworthstone/beads_rust/blob/main/src/lib.rs`
- `https://github.com/Dicklesworthstone/beads_rust/blob/main/Cargo.toml`
- `https://github.com/Dicklesworthstone/beads_rust/blob/main/rust-toolchain.toml`

Upstream `main` is moving evidence, not a release guarantee.
Its inspected package version was 0.6.0.
Implementation must pin real executable versions used by compatibility tests.
Do not advertise 0.6.0 support solely because its source was inspected.
The minimum delivery target is a verified profile for installed 0.5.7.
Unknown versions may support read-only diagnostics but must not silently gain
mutation authority from a numerically greater version string.

## Scope

Included:

- An optional external `br` adapter using structured JSON.
- A portable typed issue link for Jig plans.
- Starting and resuming linked execution without retyping the issue.
- Explicit linking of an existing plan without rewriting its baseline.
- A reverse evidence reference in a Beads comment.
- Visibility in CLI, MCP, work status, evidence, and existing dashboard views.
- Explicit opt-in issue claiming at work start.
- Explicit issue completion after Jig gates and acceptance confirmation.
- Recovery of interrupted tracker operations.
- Preservation of project-owned export policy.
- Optional setup and generated combined guidance.
- Full-harness adoption and readoption compatibility.
- A real-Beads integration fixture and source-repository dogfood configuration.

Excluded:

- Reimplementing Beads issue storage or dependency algorithms.
- Forking, vendoring, or embedding `beads_rust`.
- Downloading or bundling a `br` executable in this epic.
- A general issue tracker plugin system or second provider.
- Wrapping all `br` or `bv` commands in Jig.
- Synchronizing task descriptions, priorities, labels, or dependencies.
- Replicating the backlog in `.agent/state`.
- Replacing `bv` triage.
- Closing issues merely because a check command succeeds.
- Automatic issue creation for every user request.
- Automatically rewriting existing historical plan bodies.
- Globally renaming `plan_id` or the public `work` namespace.
- Cross-repository Beads routing and shared multi-repository trackers.
- Distributed locking across independent clones.
- Reopening a closed issue implicitly.
- Automatically editing another tool's marked instructions block.
- General changes to gate freshness semantics.

## Ownership and terminology

A task is a Beads issue with one authoritative task identity.
An execution record is Jig's existing work plan and linked receipts.
An attempt can span several sessions and check runs.
A snapshot is historical issue context captured for an attempt.
An operation is one intended tracker side effect with durable retry identity.

Beads owns:

- Task title and current description.
- Acceptance criteria and task status.
- Priority, labels, assignee, owner, and dependencies.
- Claim admissibility and close policy.
- Database storage, import, export, and recovery semantics.

Jig owns:

- The execution identifier (`plan_id`).
- The immutable Git baseline for an execution record.
- Execution and review receipts.
- Gate evaluation and source freshness authority.
- The typed link and historical issue snapshot.
- Pending adapter operation evidence.
- Whether it has observed its own requested tracker operation complete.

The application has no second authoritative issue status in Jig.
Any displayed tracker status is explicitly an observation with a timestamp.
Local execution status and tracker observation may differ without corruption.
For example, an execution may be closed while its issue remains open.
That can be an intentional execution-only finish or a pending completion.

An issue has zero or more execution records over its life.
An execution record has zero or one immutable issue link.
The same issue may have separate historical attempts and independent worktrees.
One checkout should reuse a unique open linked attempt by default.
Ambiguity must be reported, not resolved by picking the newest record.
An explicit new-attempt flag is required to create an additional attempt.

## Architectural decisions and rationale

### D1: Preserve the task/evidence boundary

Beads already supplies useful task and dependency behavior.
Jig's baseline and gate receipts supply a different form of information.
Linking those records preserves their value without having agents mirror text.
The implementation must not add Jig issue priorities or dependency states.

### D2: Integrate through the executable

Use a bounded child process and parse its supported JSON contract.
This delegates storage, policy, and sync decisions to the installed tracker.
Direct database access would couple Jig to upstream migrations and locking.
Embedding upstream modules would also inherit its toolchain and dependency tree.
The adapter should therefore be narrow and concrete for Beads.

### D3: Keep portable identity separate from local paths

Persist a generated workspace identity in Jig tracker configuration.
Persist issue references using that identity and the canonical Beads issue ID.
Resolve the local database through the configured root and `br info`.
Absolute paths are process-local information and must not enter committed links.
This lets a cloned repository retain links without claiming two unrelated
repositories with the same issue prefix are the same tracker.

### D4: Use additive journals for links and tracker operations

Add dedicated versioned append-only state for links and operation progress.
Do not rewrite existing plan open events or baseline fields.
The current custom legacy event encoder could discard extensions accidentally.
Separate journals also let an existing plan acquire a link explicitly.
Retention, diagnostics, summary, and compatibility checks must recognize them.
This is related evidence, not a second task store.

### D5: Separate linking, claiming, and completing

Starting linked work establishes provenance and may publish its backlink.
Claiming is an explicit request because execution can include review or help
on an issue assigned to someone else.
Issue completion is explicit because accepted gates are only part of done.
Plain work finish must preserve its existing execution-only behavior.
These distinctions prevent implicit task-state changes during inspection.

### D6: Make retries operation-aware

A child can commit a tracker change and then time out before Jig sees output.
Retrying blindly could duplicate comments or overwrite changed task state.
Persist intent before mutation and reconcile observed tracker state afterward.
Use stable comment markers and exact issue IDs to identify previous effects.
Never claim distributed atomicity between Beads and Jig's journals.

### D7: Keep status inspection local

Status and evidence should work when `br` is unavailable or the tracker is busy.
Read local link and operation projections for ordinary inspection.
An explicit refresh can obtain a new tracker observation when needed.
Do not launch `br` for each dashboard row or background render tick.
This also prevents ostensibly read-only UI access from triggering auto-import.

### D8: Honor export policy

The source repository has a privacy-preserving export helper.
An integration that bypasses it would regress an existing invariant.
Persist explicit export behavior and report database versus export completion.
Use an argv vector for an authorized helper, not an interpolated shell string.
The helper remains project-owned and is not copied blindly into consumers.

### D9: Preserve receipt freshness ownership

Do not infer that `.beads` is irrelevant to checks simply because it exists.
Some repositories validate tracker exports or use them as build inputs.
Claiming, backlinks, and export should settle before validation where possible.
Final issue closure can be described as a post-validation tracker change.
Historical gate evidence must retain what it actually authenticated.

### D10: Deliver optional guidance, not a mandatory tracker

Full init can offer Beads integration; default setup stays tracker-free.
Adoption can recognize an existing tracker and offer an explicit opt-in.
Noninteractive setup requires an explicit option to enable it.
Existing authored instructions remain owned by the project or their tool.
The combined Jig block explains ownership without duplicating a CLI manual.

## Proposed user contract

All commands in this section are proposed additions or extended forms.
The unchanged `--title` form of work start remains supported.

```sh
# Link to an existing issue; obtain its title and acceptance snapshot.
scripts/jig work start --issue beads:example-123

# Also request a claim through Beads policy.
scripts/jig work start --issue beads:example-123 --claim --actor example-agent

# Explicitly make a later attempt after reopening or changing approach.
scripts/jig work start --issue beads:example-123 --new-attempt

# Select an existing open attempt when more than one exists.
scripts/jig work start --issue beads:example-123 --plan-id plan_example

# Attach an existing execution without changing its Git baseline.
scripts/jig work link --plan-id plan_example --issue beads:example-123

# Ordinary execution and evidence remain plan-scoped.
scripts/jig work check --plan-id plan_example
scripts/jig work evidence --plan-id plan_example

# Finish only the execution; leave the issue lifecycle alone.
scripts/jig work finish --plan-id plan_example --resolution 'Implementation validated'

# Explicitly attest acceptance and request issue completion too.
scripts/jig work finish --plan-id plan_example --complete-issue --actor example-agent \
  --resolution 'Acceptance criteria met; implementation validated' --outcome success

# Reconcile a pending backlink, claim, close, or export operation.
scripts/jig work sync --plan-id plan_example
```

`work sync` must only retry previously authorized operation intent.
It must not invent a claim or completion request.
Do not use it to refresh the issue's description into the plan body.
Add an explicit read-only refresh option if the display needs newer observations.
The final interface must define this separately from retry authority.

Issue mode accepts only the canonical `beads:` provider prefix in this epic.
Reject empty IDs, ambiguous abbreviations, and unsupported provider names.
Pass IDs as separate argv values after an option terminator where supported.
Keep a full issue ID in durable state even if upstream can resolve abbreviations.

The issue supplies the default execution title.
An optional `--title` override labels this execution without editing the issue.
Body text or a body file remains optional execution-specific context.
Its existing mutual exclusion still applies in CLI and MCP.
No generated second task specification is required.

Reuse does not silently change a plan's title, body, issue snapshot, or baseline.
If supplied creation arguments differ from the open plan, return a conflict
with the existing plan ID and explicit resume/new-attempt guidance.
No change of baseline is permitted through repeated start.

Proposed configuration is owned by the adapter task.
The shared minimum fields are provider kind and portable workspace ID.
The first implementation supports the repository's root `.beads` only.
Executable resolution must use the normal supported process configuration.
Do not add arbitrary provider scripts or allow issue IDs to select executables.
An export helper is an explicit argv configuration scoped to that workspace.

## Public result semantics

Preserve existing fields for standalone start and finish callers.
Add link and operation information as additive named fields.
Input schemas must reject nonsensical flag combinations before creating state.
Machine callers must never infer completion from human prose.

Linked start returns:

- `plan_id` and the existing plan/session information.
- Whether the execution record was created or reused.
- A typed issue reference.
- Snapshot timestamp and snapshot digest.
- Backlink operation state, when configured.
- Claim operation state, only when requested.
- Export state, independently of database mutation state.
- A bounded recovery instruction if an effect is pending or uncertain.

Finish with issue completion returns:

- Local execution state.
- Whether the request included issue completion.
- Accepted gate evidence references.
- Tracker completion operation identity.
- Observed tracker close status and correlation evidence.
- Export status.
- A clear partial-result classification on any failure.

Top-level success must not hide a requested incomplete mutation.
Manual export deliberately requests no automatic export and can return success
with `export = pending_manual`; the output must say publication is still manual.
An inspection command can succeed while reporting a pending operation.
An effectful completion command with an unresolved requested effect must
return a non-success classification and nonzero CLI status.
The structured payload still includes any durable local success.
Plain execution-only finish retains its established result behavior.

## Consistency and recovery boundaries

The operation journal records intent, observations, acknowledgements, and errors.
It records no executable output wholesale and no machine-local source path.
Each transition refers to one plan, issue, workspace, and operation kind.
Markers are versioned and deterministic from the operation identity.
Incomplete writes use repository-owned journal recovery conventions.
Unknown schema versions must be inspectable without silently allowing mutation.

Use checkout-scoped serialization to avoid duplicate local starts and retries.
Do not claim it coordinates independent clones or bare database clients.
Beads remains responsible for its own write locks and atomic policy checks.
Do not hold a Beads lock manually while invoking `br`.
Avoid holding the repository execution lease around a helper that can invoke Jig.
Define lock ordering and reject recursive tracker helper invocation.

Reopened issues keep previous execution evidence as history.
An old completed operation must not close the newly reopened issue.
A new attempt or explicit new completion decision is required.
Linking an existing closed plan is allowed as historical provenance only.
It does not authorize a claim, new gate acceptance, or task completion.

Task context drift is judged using the acceptance-relevant snapshot fields.
Changing a comment timestamp alone is not a new acceptance contract.
Changing the issue description or acceptance criteria before completion is.
The command must report drift and require a new linked attempt; it must not
silently claim the old snapshot proves the new task.
Use `work start --issue beads:ID --new-attempt`, validate that attempt, and
complete its new plan ID. This epic adds no old-intent reconciliation override.

If closure succeeded but export failed, retry export without a second close.
If closure outcome is unknown, inspect its correlated transition evidence.
If closure did not happen and source/task authority changed, leave it pending.
Do not bypass evidence freshness merely because the plan is already closed.
The implementation may require a fresh linked attempt in this case.

## Delivery graph

All tasks below are concrete implementation outcomes within one epic.
Use `blocks` edges for execution prerequisites and parent-child edges for
membership; parentage must not make every child wait for the epic to close.

| Key | Delivery outcome | Prerequisites |
| --- | --- | --- |
| T1 | Persist portable issue links and recoverable operation records | None |
| T2 | Add bounded Beads adapter, configuration, and doctor diagnostics | None |
| T3 | Start, reuse, and explicitly link execution records | T1, T2 |
| T4 | Publish backlinks and honor export policy with safe retries | T3 |
| T5 | Expose linked work and pending effects across CLI/MCP/status/UI | T3, T4 |
| T6 | Claim linked issues through Beads with interruption recovery | T4 |
| T7 | Complete issues after accepted execution with retryable closure | T5, T6 |
| T8 | Ship optional init/adopt integration and coherent agent guidance | T5, T7 |
| T9 | Validate real-Beads compatibility and dogfood the complete workflow | T7, T8 |

T1 and T2 can begin independently.
T5 and T6 can proceed independently after T4.
T8's setup implementation can be explored earlier, but its deliverable includes
accurate final workflow guidance and therefore depends on T7.
T9 is the final consumer of the full integration.
The epic closes only after all nine outcomes meet acceptance.

Delivery has two useful release checkpoints:

- Linkage checkpoint: T1 through T5 establish provenance and visibility.
- Lifecycle checkpoint: T6 through T9 add explicit mutation and consumer setup.

Do not ship template examples ahead of their runtime support.
This epic does not require the two checkpoints to be separate releases.

## T1 — Persist portable issue links and recoverable operation records

### Outcome

Jig can store and read an immutable issue link for an existing or newly opened
plan, and retain tracker-operation intent across process interruption.
Existing plans, baselines, IDs, and receipts continue to load unchanged.
No installed `br` binary is required to inspect this state.

### Why this task exists

The current plan record lacks an issue reference.
Putting a Beads ID only in prose cannot support reliable lookup or retries.
Adding fields to `LegacyPlanEvent` alone risks older projections dropping them.
Dedicated additive state gives the runtime a typed join without rebuilding
the plan and receipt system or creating an issue database inside Jig.

### Entry points

- Read `agent-map.md` and `crates/jig/AGENTS.md` first.
- Inspect `crates/jig/src/state/records.rs` legacy event serialization.
- Inspect `crates/jig/src/state/plans.rs` and `plan_files.rs`.
- Inspect `crates/jig/src/state/jsonl.rs` and its child modules.
- Inspect `crates/jig/src/state/diagnostics.rs` known journal inventory.
- Inspect `crates/jig/src/state/maintenance.rs`.
- Inspect `crates/jig/src/state/receipts/archive/` protection rules.
- Inspect the launcher compatibility probe and contract validation paths.
- Locate those current paths with `rg`; some guide paths predate reorganizations.

### Proposed data model

Create a versioned `work-links.jsonl` journal under `.agent/state`.
One committed link event contains:

- Event ID and schema version.
- Plan ID.
- Provider kind `beads`.
- Portable tracker workspace ID from committed configuration.
- Canonical full issue ID.
- Repository-relative tracker root `.beads`.
- Snapshot observation timestamp.
- Snapshot title and acceptance-relevant context.
- A deterministic digest of that context.
- Whether the link was established at start or attached later.

Use a separate `tracker-operations.jsonl` journal for side-effect progress.
Record operation identity, kind, issue reference, plan reference, and phase.
Keep enough bounded correlation data to reconcile comments, claims, and close.
The lifecycle tasks own operation-specific payloads and transitions.
T1 supplies the typed journal and projection mechanics they need.

### Invariants

- A plan has at most one distinct immutable issue link.
- Repeating the exact same link is idempotent.
- Linking a different issue to an already linked plan is rejected.
- Old plan event bytes are never rewritten to add a reference.
- Adding a link never resets a baseline or validates old receipts anew.
- Snapshot text is historical context, not a synchronized task description.
- Unknown provider/schema values cannot gain mutation authority.
- Absolute database paths are absent from committed records.
- Duplicate event replay is deterministic and detected.
- Conflicting link events produce an inspectable conflict, not last-write-wins.
- A corrupt link journal does not make an unrelated standalone plan disappear.
- Effectful linked operations refuse ambiguous or corrupted link authority.
- Operation IDs remain stable through retry and export-only recovery.

### Compatibility behavior

Readers accept historical repositories with no new journals.
Standalone start and finish retain existing behavior.
New consumers preserve old event IDs and unknown record bytes in maintenance.
Audit all state listing, diagnosis, archive, and retention inventories.
Protect pending operation intent and its referenced plans/receipts from pruning.
Completed operation history follows documented bounded retention rules only
after its durable correlation and final result remain reconstructible.

Configuration uses strict deserialization today.
Do not claim a new configured tracker can be used by an old runtime.
Wire the existing launcher/contract compatibility mechanism so linked behavior
requires a runtime that understands its journals and configuration.
An old runtime must either reject that configured repository or remain unable
to execute linked lifecycle commands; silently dropping link authority fails.
Do not repurpose an already shipped contract epoch without its migration path.
The implementer records the chosen compatible rollout in the task ExecPlan.

### Meaningful validation

Load historical plan, session, and receipt fixtures unchanged.
Attach a link and prove all previous baseline and receipt IDs remain identical.
Attempt an identical duplicate and observe no conflicting second link.
Attempt a different issue and observe a conflict with the original reference.
Replay operation events after a process restart and obtain the same projection.
Inject a truncated final line using the existing journal test boundary.
Prove recovery does not invent an acknowledged tracker mutation.
Exercise a future schema version and inspect its unsupported diagnostic.
Exercise maintenance/retention against a pending operation.
Ensure its plan and evidence remain reachable afterward.
Round-trip unknown fields without silently stripping committed history.
Use generic fixture IDs such as `example-123` and `plan_example`.

### Acceptance criteria

- [ ] Typed links persist independently of plan-body prose.
- [ ] A plan's link is immutable and exact retries are harmless.
- [ ] Existing plan IDs, baseline events, and receipt interpretation are preserved.
- [ ] Pending tracker intent survives restart and supported maintenance.
- [ ] Corruption/conflicting identity blocks linked mutations with useful diagnostics.
- [ ] No fixture or new durable record leaks machine-local source paths.
- [ ] Runtime compatibility prevents an unsupported writer losing authority.
- [ ] Focused state regressions and required repository checks pass.

### Dependencies and handoff

No new epic task blocks T1.
It unblocks T3 and supplies state machinery used transitively by T4/T6/T7.
Coordinate shared journal fixes with existing JSONL recovery issues.
Do not create a competing general journal redesign in this bead.
Implementation ends with a documented data contract and tested readers/writers.
It does not create an issue adapter, claim an issue, or expose incomplete CLI.

## T2 — Add the optional external Beads adapter and explicit workspace configuration

### Delivery outcome

An opted-in repository can resolve its Beads workspace and read one issue through a bounded `br` subprocess.
The same adapter supplies the typed primitives later tasks use for comments, claims, and closure.
Repositories without tracker configuration retain their existing operation and installation requirements.
No Beads library, fork, replacement tracker, or bundled executable enters the Jig distribution.

### Ownership and rationale

Beads remains authoritative for issue content, dependencies, assignment, and status.
Jig remains authoritative for execution plans, Git baselines, gate decisions, and evidence.
An external adapter keeps the integration replaceable without coupling Jig to the upstream database schema.
Explicit configuration prevents an incidental parent `.beads` directory from becoming this repository's tracker.
A portable workspace identifier distinguishes identical issue IDs in unrelated repositories without recording absolute paths.
Version profiles express tested command and JSON contracts; they do not assume semver guarantees that upstream has not promised.
Read operations must remain read operations, because doctor and issue previews must not unexpectedly import or export tracker data.

### Dependencies and consumers

Blocked by: no other delivery task in this epic.
Coordinates its configuration and workspace identity value with T1's immutable issue-reference record.
Unblocks T3 linked start, T4 backlinks/export, and the integration-specific parts of T8 setup.
T6 and T7 consume the mutation primitives only through their own explicit runtime workflows.
No planning, research, or review bead is required before implementation.

### Verified implementation entry points

`crates/jig/src/context.rs` owns `RepoConfig` and execution-authority reload behavior.
`crates/jig/src/context/work_config.rs` owns `WorkConfig`, with `deny_unknown_fields` enabled.
The existing `receipt_metadata` option is a separate ownership declaration for evidence freshness.
`crates/jig/src/doctor.rs` owns bounded external-tool probes and imports the owned-process library.
`crates/jig/src/doctor_parts/` contains the split doctor implementation.
`crates/jig/src/doctor/tests/` and `doctor/tests_parts/` contain doctor regression coverage.
`crates/jig/src/runtime/work.rs` owns shared CLI/MCP work behavior.
`crates/jig/src/command/work.rs` owns shared request DTOs.
The crate guide's historical `src/process.rs` pointer is stale; locate current owned-process integration before editing.
Read `crates/jig/AGENTS.md` and the owning process crate's guide if that crate requires changes.

### Proposed configuration contract

All configuration and interfaces in this section are proposed, not currently implemented.
Use `[work.tracker]` with `kind = "beads"` and a required portable `workspace_id`.
Use a generated stable ULID string for `workspace_id`; copy it with the repository across machines.
The v1 tracker root is the repository-local `.beads` directory.
Do not permit an arbitrary external root or cross-repository routing in this version.
Use `export = "manual"` as the conservative default.
T4 defines the optional explicit helper or direct-export policies.
Optional `manual_export_guidance` is display text only, never executable input.
Missing `[work.tracker]` means the adapter is disabled, even when `.beads` exists.
Reject unknown tracker kinds, malformed identifiers, empty helper arguments, and unsupported policy combinations.
Keep the new tracker section optional with serde defaults so old `.jig.toml` files remain accepted.
Update all config serialization, authority fingerprint, and generated-template round-trip paths that carry `WorkConfig`.
The configuration itself belongs in the execution-authority comparison; changing tracker identity mid-operation must be observable.
Never infer the workspace identifier from a checkout basename, private Git remote, or absolute path.
Keep existing `receipt_metadata = ["beads"]` semantics independent and unchanged.

### Proposed adapter contract

Expose normalized internal results rather than upstream JSON values to work orchestration.
Provide discovery, show-issue, list-comments, add-comment, claim-issue, and close-issue operations.
Retain raw successful JSON only in ephemeral diagnostic context when needed; durable records use allowlisted fields.
Represent unsupported binary, unsupported response, issue missing, blocked transition, assignment conflict, timeout, and indeterminate write distinctly.
Normalize issue ID, title, description, acceptance criteria, status, assignee, and update revision when present.
Keep full issue contents out of generic process-error messages and command receipts.
An issue snapshot is copied context, not a second mutable task description.
Derive a deterministic semantic revision from task content when an upstream revision token is unavailable.
The semantic revision includes description and acceptance criteria but excludes Jig's own comments and audit timestamps.
Validate the returned issue ID against the exact requested ID; do not silently accept an alias pointing elsewhere.
Treat tombstoned or deleted issues distinctly from an unavailable tracker.

### Discovery and process behavior

Resolve `br` once at operation start and run it with an argument vector, never through a shell.
Pin the subprocess working directory to the resolved Jig repository root.
Resolve database and JSONL export paths from supported `br info --json` output and local metadata.
Validate both paths belong to the selected local `.beads` store before using them.
Reject redirects or routing that escape that store in v1.
Pass the resolved database explicitly on subsequent supported `br` operations.
Neutralize ambient Beads database-routing overrides that would contradict the pinned store.
Do not scrub unrelated caller identity or authentication variables indiscriminately.
Pass `--no-auto-import` and `--no-auto-flush` to all discovery, read, and primary mutation operations.
Import is an explicit tracker-owned recovery action; claim/close must not import new context silently.
Do not use `--allow-stale` as an automatic recovery strategy.
Treat stale-storage warnings as an actionable tracker state, not permission to import during a read.
Use the supported read-only `br sync --status --json` profile to inspect storage
readiness before linked mutations; do not rely solely on warning prose.
Refuse stale/conflicting storage that cannot establish current task authority.
Use the existing owned-process mechanism for cancellation, output limits, timeout, and descendant cleanup.
Use bounded output even for issue bodies and comment lists; fail explicitly if the required result is truncated.
Separate machine stdout from stderr diagnostics and reject malformed or trailing non-JSON output.
Specify finite read and mutation budgets in one adapter policy rather than scattering magic values across transports.

### Supported Beads profiles

The planning environment contains `br 0.5.7`.
Its help confirms `info`, `show`, `comments list`, `comments add`, `update --claim`, and `close`.
It confirms explicit `--db`, `--actor`, `--no-auto-import`, and `--no-auto-flush` options.
It confirms that terminal issue transitions use `close`, rather than `update --status closed`.
The reviewed upstream 0.6.0 source is a second candidate profile, not proof that its binary passed integration tests.
Implement explicit profile selection based on observed version and necessary capabilities.
Make the 0.5.7 profile's exact response shapes fixture-backed.
If adding a 0.6.0 profile, make its response variants fixture-backed, including wrapped mutation results with warnings where applicable.
Unknown versions must not silently receive mutation support solely because they are numerically newer.
Doctor should explain the observed version, chosen profile, supported operations, and concrete unsupported-contract reason.
T9 must run real binaries before claiming both profiles are release-supported.

### Failure and recovery behavior

A missing `br` must not break unlinked `work start`, existing evidence views, or ordinary gates.
A configured but unusable adapter appears as an explicit doctor finding with installation/configuration recovery guidance.
A malformed `info` result must stop before any issue mutation.
A child exit after a possible write is indeterminate until the owning workflow rereads the same issue.
Do not convert a process timeout into a definite statement that the issue was unchanged.
No mutation primitive automatically retries a write without its caller's reconciliation policy.
Adapter errors must include operation type and generic issue ID when safe, without leaking machine-local store paths into append-only evidence.
Preserve privacy requirements for every fixture, path, and generated receipt.

### Acceptance criteria and validation

With no tracker configuration and no `br` on PATH, all existing unlinked work flows still function.
A configured generic fixture reads its exact issue and returns normalized fields without touching JSONL or issue contents.
A second database injected through ambient routing cannot redirect the operation away from the configured fixture.
An explicit store outside the fixture repository is rejected before a write-capable invocation.
Known response profiles decode their real representative shapes and reject missing required identity fields.
An unknown profile reports unsupported status and cannot reach a write subprocess.
Malformed JSON, output overflow, timeout, cancellation, and nonzero exit each produce the documented result category.
Fake-executable argv tests assert argument boundaries for spaces and shell metacharacters in titles and IDs.
Doctor reports adapter problems without changing configuration, initializing Beads, or importing/exporting its data.
Validate config round trips and legacy config loading with the new section absent.
Run focused adapter/config/doctor tests, then the repository's required backend `scripts/jig check test` through the built dev binary.

### Non-goals

No Beads installation, bootstrap download, remote tracker, or `bv` implementation in this task.
No direct SQLite reads or writes from Jig.
No copying issue graphs into Jig state.
No automatic freshness exclusions or generated policy changes.

## T3 — Start, reuse, and explicitly link execution records

### Outcome

An agent can start work from a canonical Beads issue reference without copying
its title or acceptance criteria into another manually maintained task.
Repeating the request resumes a unique open execution in the checkout.
An existing plan can acquire a typed link while preserving its history.

### Why this task exists

The greatest current duplication occurs when agents author `jig work start`
arguments after already creating or reading the issue.
The adapter and journals become useful when work start consumes them directly.
Explicit reuse prevents retries and resumed chats from multiplying records.
Explicit linking offers a forward migration for in-flight work without rewriting
old JSONL or pretending historical prose had always been structured authority.

### Entry points

- `crates/jig/src/cli/work.rs`: work arguments and help examples.
- `crates/jig/src/cli/run/`: locate current CLI-to-runtime conversion.
- `crates/jig/src/command/work.rs`: shared work request types.
- `crates/jig/src/runtime/work.rs`: prepare/start ordering.
- `crates/jig/src/state/plans.rs`: immutable baseline capture.
- `crates/jig/src/tool_defs.rs`: MCP argument schema and descriptions.
- `crates/jig/src/runtime/tests/work.rs` and work test modules.
- `crates/jig/src/runtime/tests/mcp/repository_execution/work_tools.rs`.
- T1's link journal and T2's issue adapter.

### Proposed behavior

Extend work start with `--issue beads:example-123`.
The issue replaces the requirement to supply a manual title.
A caller can still override the execution title explicitly.
An issue plus body/body-file adds execution notes, not a task rewrite.
Existing title-only start remains available with no tracker configured.
The MCP schema requires a title or issue and retains body mutual exclusion.

Resolve the configured workspace and exact issue before durable plan/session
creation, just as existing code validates body/baseline input first.
Reject missing, ambiguous, malformed, routed, or unsupported issue identities.
An existing closed issue can be linked historically through `work link`.
Starting active work against a closed issue requires an explicit reopen in br.
Jig must not reopen it as a side effect of start.

Under a checkout-scoped start lease, inspect open linked plans for that issue.
If none exist, prepare a new plan with the exact current or supplied baseline.
If exactly one exists, return it as reused without touching its baseline.
If more than one exists, report all bounded candidate IDs and require selection.
Selection uses `work start --issue beads:ID --plan-id PLAN` and can include
T6's explicit `--claim --actor ACTOR` when ownership is being requested.
The selected plan must be open and linked to the exact supplied issue/workspace.
Reject a mismatched, closed, unlinked, or missing selected plan.
Reject `--plan-id` combined with `--new-attempt` or without `--issue` in start.
`--new-attempt` explicitly creates a new plan and captures a fresh baseline.
Do not implement an implicit newest-plan or oldest-plan preference.
Do not create a new session merely to return a reused plan unless the existing
session API requires it; define and test that behavior consistently.

Plan open and link publication are separate append operations.
Persist enough start intent to resume an interrupted publication with the same
plan identity, rather than leaving an orphan that a retry duplicates.
Never delete a committed plan event to simulate a transaction rollback.
Before link commit, the new plan must not be reported as fully linked.
T4 later adds backlink publication as a subsequent operation.
The local plan/link join must not depend on a successful remote comment.

### Linking existing plans

Add `work link --plan-id PLAN --issue beads:ID` and equivalent MCP support.
Require the plan to exist and its issue to resolve exactly.
Preserve title, body, current open/closed state, baseline, and receipts.
Record a snapshot and explicitly label the link as attached later.
Exact repeat returns the existing link.
An attempted reassignment to another issue is a conflict.
Do not add a generic unlink/relink history editor in this epic.
For an erroneous link, record a corrective decision and create the appropriate
new execution record; never erase provenance silently.

### Snapshot contract

Capture the issue title, description, acceptance criteria, and observation time.
Use a deterministic semantic digest independent of JSON property ordering.
Normalize absent optional text consistently but preserve meaningful content.
Do not include comments, assignee, priority, or updated_at in acceptance digest.
Those can change without changing the work's acceptance contract.
Do not write the entire `br show` payload to the journal.
Keep unknown upstream fields outside Jig's persisted allowlist.
Snapshot size limits are explicit and oversized context is a validation error,
not a silently truncated acceptance contract.
Implementation-specific size values must follow existing process/state limits
or be documented and tested at their boundary.

### Concurrency and failure behavior

Two local simultaneous starts must not accidentally create two implicit plans.
Explicit new attempts remain possible and visible.
Independent clones may create separate attempts; this is supported history.
A per-checkout lease is not a distributed claim on the Beads issue.
Concurrent evidence execution and linking must preserve plan lease ordering.
Cancellation before state creation leaves no plan or link.
Cancellation after plan publication returns or preserves recoverable identity.
Any retry reconciles the journal before preparing another plan.

### Meaningful validation

Use an adapter fixture with a known exact issue and multiline acceptance text.
Assert start returns the issue's title and durable canonical reference.
Change upstream title afterward and assert reuse does not rewrite the snapshot.
Repeat start with a conflicting baseline and assert clear refusal.
Race two starts in one temporary checkout and count actual open/link events.
Inject interruption between plan and link publication; retry finds the same ID.
Attach to a closed legacy plan and compare its event history byte for byte.
Supply absent, foreign, partial, empty, and malformed IDs before any state write.
Exercise CLI and MCP validation for every issue/title/body combination.

### Acceptance criteria

- [ ] Issue-backed start avoids duplicate manual task text.
- [ ] Exact repeat reuses the unique open record without changing its baseline.
- [ ] New attempts and ambiguous existing attempts are explicit.
- [ ] Existing-plan linking is additive and preserves history.
- [ ] No missing issue or invalid input leaves an orphan session or link.
- [ ] Interrupted publication is recoverable with stable plan identity.
- [ ] CLI and MCP share validation and state behavior.
- [ ] Historical and standalone work regressions remain green.

### Dependencies and handoff

Blocked by T1 and T2.
Unblocks T4, which publishes the reverse link and exports tracker changes.
This task does not claim issues or close them.
Its output must expose pending backlink intent for later integration without
pretending a comment was written before T4 exists.
Add public contract documentation for implemented behavior alongside the code.

## T4 — Publish idempotent backlinks and honor repository export policy

### Delivery outcome

Starting linked execution can attach a discoverable Jig backlink to the owning Beads issue.
Repeating or recovering the operation avoids duplicate comments when the existing comment can be verified.
Tracker mutations follow an explicit export policy and preserve this repository's privacy-aware export helper.
The user can distinguish linked locally, backlink confirmed, export pending, and export failed.

### Ownership and rationale

The immutable Jig issue link is authoritative for the association.
A Beads comment is the reverse pointer that makes the execution discoverable from the task tracker.
Do not take Beads' single `external_ref` field: an issue can have multiple execution records and other integrations may own that field.
An append-only operation journal makes partial success recoverable without pretending that two stores share a transaction.
An explicit export policy avoids bypassing repository-specific privacy and synchronization requirements.
Manual export must be a supported successful state because not every adopted repository wants Jig to run its export process.

### Dependencies and consumers

Blocked by T2 adapter/configuration and T3 linked-start/reuse behavior.
Consumes T1's append-only `work-links.jsonl` and `tracker-operations.jsonl` record contracts.
Unblocks T6 coordinated claim and contributes shared journal/export machinery to T7 completion.
T5 presents the operation states produced here without performing live tracker reads.
T8 teaches the selected export policy and preserves adopted custom instructions.

### Verified implementation entry points

`crates/jig/src/runtime/work.rs` coordinates work start and finish.
`crates/jig/src/state.rs` exposes persistence and execution lease APIs.
`crates/jig/src/state/execution_leases.rs` owns the repository execution lease.
`crates/jig/src/state/records.rs` and `state/plans.rs` hold existing plan records and transitions.
`crates/jig/src/context/work_config.rs` owns tracker configuration alongside work settings.
`scripts/beads-sync.py` is this repository's existing export authority.
That helper clears nonempty `source_repo_path`, exports through `br`, repairs tombstone export metadata, and checks privacy.
`scripts/tests/test_beads_sync.py` supplies existing helper behavior tests.
Do not replace its workflow with a direct `br sync --flush-only` call in this repository.

### Proposed user-visible behavior

All additions below are proposed interfaces.
`jig work start --issue beads:example-123` creates or reuses the link and attempts its backlink.
It does not claim the issue unless the separate T6 `--claim` flag is present.
The response includes `plan_id`, canonical issue reference, backlink state, export state, and retry guidance.
If local link creation succeeds but the backlink fails, return that durable partial result and the same plan ID.
Repeating linked start retries incomplete integration work before considering a new execution record.
Never discard a valid linked plan solely because the tracker comment service is temporarily unavailable.
Do not overwrite the issue's title, body, acceptance criteria, labels, or `external_ref`.
Use existing local evidence commands as the stable destination rather than inventing an unavailable web URL.
The comment includes a repository-relative plan document path and `jig work evidence --plan-id <id>`.

### Proposed backlink marker and journal

Use a versioned marker of the form `jig-link:v1:<workspace_id>:<plan_id>`.
Put the marker in a short comment with plain-language context and the exact plan ID.
Generate only generic, portable paths and identifiers; never include local database paths or Git credentials.
Write a journal intent with operation ID, plan ID, immutable issue identity, kind, and marker before attempting the comment.
List comments on that exact issue before adding a marker that has no confirmed result.
Treat an exact matching marker with matching workspace and plan identity as the existing backlink.
Do not treat substring matches in arbitrary user prose as confirmation.
Append a confirmation record after a successful add or successful reread of an existing marker.
If comment addition may have committed but its response is lost, retry starts with a comment-list reconciliation.
If the list is incomplete, truncated, or ambiguous, leave the operation pending; do not blindly add another comment.
Do not delete or rewrite Beads comments to clean up a retry history.
Record a remote comment ID if provided, but do not require it to be the sole durable identity.
Serialization must preserve all unrelated fields and existing state lines.

### Local serialization and concurrency

Use a repository-local lock for link-plus-operation coordination, with a documented lock acquisition order.
Do not hold a broad worktree write lease during a slow external read unnecessarily.
Before issuing a mutation, verify the linked workspace and exact issue ID again under the operation's authority.
A local lock prevents concurrent Jig invocations in that checkout from creating duplicate intents.
It does not establish exclusion against another clone, another tool, or a direct `br` command.
Remote comments have no assumed server-enforced unique marker constraint.
If two independent clones race to post the same marker, report/reuse recognized duplicates without claiming exactly-once delivery.
Later operations use the durable association even if duplicate comments exist.
Do not automatically delete a duplicate comment because another writer may own its surrounding content.

### Proposed export policies

`export = "manual"` is the default and never runs a flush subprocess.
After a tracker mutation, manual policy records `pending_manual` and displays the repository's configured export guidance.
The optional T2 `manual_export_guidance` field is display-only text.
Confirmed backlink plus `pending_manual` returns success without claiming export
completion. Automatic-export failure returns nonzero with confirmed effects retained.
`export = "helper"` requires a configured `export_argv` array and runs the exact executable/arguments without a shell.
For this repository, the proposed array is `["python3", "scripts/beads-sync.py"]`.
An explicit `export = "br"` may call the selected profile's direct flush only where repository policy permits it.
No setup flow silently chooses direct flush when it finds a custom helper.
All three modes use `--no-auto-import` and `--no-auto-flush` on primary mutations so context and export ordering remain explicit.
Helper mode validates a nonempty executable, bounds runtime/output, and pins cwd to the repository root.
Do not accept a single shell command string as a substitute for the argv array.
Jig does not interpolate issue text, titles, or paths into helper arguments.
Record the export policy identity and effective helper authority at operation start.
If configuration changes before export retry, require explicit reconciliation rather than silently running a different helper.
`work sync --plan-id ID` retries previously authorized pending operations.
For a confirmed mutation with unfinished export only, the explicit
`work sync --plan-id ID --accept-current-export-policy` form can acknowledge a
changed export mode/helper argv before retrying that export.
Record the old and accepted new export authority durably before execution.
Compare export mode, literal helper argv and executable authority, portable
workspace identity, pinned cwd, and selected tracker profile.
This acknowledgement can change export policy only, not issue, workspace,
tracker profile, actor, claim intent, or completion intent.
Restoring the originally recorded authority permits an ordinary retry.
Changing workspace/profile for an unperformed mutation requires restoring the
original authority or starting a new explicit attempt; there is no override.
Manual export remains visibly manual until an explicitly selected automatic
policy completes. This epic adds no unverified manual-success acknowledgement.
Direct flush and helper failures are recorded separately from the already-confirmed issue mutation.
An export retry must not repeat a confirmed comment, claim, or close.
The helper owns its own domain-specific privacy checks; Jig must not partially reproduce them.

### Freshness interaction

Link and operation metadata belong to Jig's state model, with explicit state-store recognition and diagnostics.
Beads export changes remain ordinary repository changes under the existing freshness policy.
Do not add `.beads` to `receipt_metadata` automatically.
This repository validates Beads export content in policy checks, so blanket exclusion would change evidence meaning.
A backlink export before implementation checks can legitimately change the captured working state.
Arrange linked-start capture so its baseline semantics are explicit and stable across a repeated start.
If baseline capture precedes tracker export, display that export as a later source change rather than silently rewriting the baseline.
T7 owns the stricter post-close mutation and terminal-retry policy.

### Acceptance criteria and meaningful tests

Start one linked plan, repeat start, and observe one local link and one recognized backlink marker.
Inject a crash after the remote comment commits but before local confirmation; retry recognizes the existing comment.
Inject a crash before the remote write; retry adds exactly one comment in a serialized local workflow.
Preserve a populated `external_ref` byte-for-byte through all link operations.
A comment-list truncation produces an explicit unresolved state and no speculative add.
Manual policy never invokes any export command and reports its pending state without claiming export success.
Helper policy runs argv with literal metacharacters, the correct cwd, and no shell evaluation.
An export failure retains confirmed backlink state; retry only exports.
Changing helper authority between intent and retry refuses the changed authority with a clear recovery path.
The repository's actual helper still clears private source paths and preserves unaffected fields.
Generic fixture JSONL and all appended evidence contain no machine-local `source_repo_path` values.
Tests prove source-freshness policy remains unchanged when `.beads` is a checked input.
Use fault injection around durable journal append and child completion, not just success-path mocks.
Run focused state/runtime tests and `scripts/beads-sync.py --check` where relevant; finish backend verification through the dev binary.

### Non-goals

No issue claiming or closure in this task.
No global comment uniqueness guarantee.
No tracker sync daemon, background export watcher, or automatic Git commit.
No changes to repository-specific Beads policy outside the integration's explicit configured export hook.

## T5 — Expose linked work and pending effects across CLI/MCP/status/UI

### Outcome

An agent or human can see which issue an execution belongs to, what the recorded
issue context was, and whether a tracker operation still needs attention.
CLI, MCP, status, and the existing dashboard agree on these facts.
Ordinary inspection works without running Beads.

### Why this task exists

A stored link that callers cannot see would not solve the original confusion.
An overall success boolean can also hide that execution finished but issue
completion or export did not.
The interface must make those separate results legible while retaining the
existing evidence model and avoiding a new issue-tracker user interface.

### Entry points

- `crates/jig/src/cli/output/work.rs`.
- `crates/jig/src/tool_defs.rs` schemas/descriptions.
- `crates/jig/src/runtime/work/gates.rs` and its current submodules.
- `crates/jig/src/status.rs` and `status/dashboard.rs`.
- `crates/jig/src/ui/source/epoch/plan.rs`.
- `crates/jig/src/ui/source/epoch/collect.rs`.
- `crates/jig/src/ui/source/tests/details.rs`.
- `crates/jig-ui/AGENTS.md` before editing dashboard-owned types/rendering.
- `docs/public-contract.md` and `docs/configuration.md`.

### Proposed display model

Show the canonical Beads issue ID beside its Jig plan ID.
Show whether the link was captured at start or attached later.
Identify issue text as a snapshot with observation time.
Show local execution status independently of any tracker observation.
Show pending backlink, claim, completion, or export states by operation kind.
Show one exact recovery command for actionable local pending work.
Show unknown/unsupported states honestly when operation data is unavailable.

The default work status path reads only local persisted state.
The default evidence path reads only local receipts and link projections.
The dashboard must not shell out once per issue, row, or refresh cycle.
An explicit refresh request can invoke T2 and append an observation only if
its effectful recording behavior is declared; otherwise return an ephemeral
read-only observation without updating the journal.
Do not call a state-writing refresh an ordinary read-only inspection.

### Public API behavior

Add optional `issue` and `tracker_operations` fields to relevant results.
Preserve established plan and gate field names.
Keep existing `ok` semantics for inspection commands.
Use separate structured flags for execution closure and requested issue effects.
Document which field automation must check before declaring completion.
Never infer issue closure from a plan's `closed` state.
Never infer current issue status from a historical title snapshot.

T6/T7 will extend the operation enum with claim and completion states.
Design the display to render supported generic phase/status fields without
requiring hidden tracker-specific branching in terminal widgets.
Unknown future operation kinds appear as unsupported and remain inspectable.
Do not turn deserialization failure into a blank healthy row.

### Filtering and navigation

Add an issue selector to work status/evidence only where it reduces manual joins.
The selector resolves locally against canonical typed links.
Zero matching records produces an explicit empty result.
One matching record can be displayed directly.
Multiple records are listed with IDs and execution status.
Do not silently choose one attempt for gate evaluation or mutation.
Mutating commands continue to require exact plan selection when ambiguous.
Existing omission rules for `--plan-id` remain compatible.

### Privacy and output boundaries

Render untrusted issue titles as text, not terminal control sequences.
Keep raw child stderr out of committed observation payloads.
Do not display absolute database paths in portable issue references.
Doctor may show a local path interactively, but persistence uses a sanitized
classification and repository-relative location only.
Allow callers to inspect snapshot metadata without dumping a long description.
Use existing bounded detail expansion behavior rather than truncating quietly.

### Failure scenarios

Missing `br` does not prevent local evidence inspection.
A busy tracker does not make the dashboard hang.
A missing issue after linking leaves the historical link visible.
A corrupted operation journal produces attention on the affected plan.
Unlinked legacy plans retain their normal representation.
A closed execution with pending export shows both facts.
A reused open plan remains clearly distinct from a newly created plan.
An externally closed issue remains an observation, not proof Jig completed it.

### Meaningful validation

Exercise equivalent CLI JSON and MCP outputs against the same fixture state.
Assert both include canonical issue identity and matching operation phases.
Run local status with a nonexistent `br` executable and verify success.
Use a sentinel fake executable and prove dashboard rendering never invokes it.
Render a malicious title containing terminal controls through existing sanitizers.
Verify unlinked plans preserve their previous output compatibility.
Verify pending close and pending export do not render as fully completed.
Test multiple attempts for one issue without silently selecting one.
Test unknown operation schema/kind representation and actionable diagnostics.

### Acceptance criteria

- [ ] Issue links are visible in human and machine work output.
- [ ] Execution, tracker observation, and export status are separate fields.
- [ ] Local inspection works without Beads and performs no tracker mutations.
- [ ] Existing dashboard views show linked identity and pending attention.
- [ ] Multiple attempts are navigable without implicit mutation selection.
- [ ] Untrusted tracker text respects output sanitization boundaries.
- [ ] CLI/MCP parity and existing status regressions pass.

### Dependencies and handoff

Blocked by T3 and T4.
Unblocks T7 and feeds T8's final workflow documentation.
Coordinate type ownership with `jig-ui`; keep transport conversion thin.
No new task board, priority editor, graph renderer, or Beads TUI is included.
The final result is an evidence view with task references.

## T6 — Add explicit Beads claim during linked execution start

### Delivery outcome

An agent can request linked execution and an issue claim in one explicit Jig command.
Beads performs the actual assignment/status transition and retains its dependency and ownership policies.
Failures and retries preserve a truthful distinction between the local execution record and the remote claim.

### Ownership and rationale

An existing task should not require a second manually maintained Jig task description.
Claiming belongs to Beads because its issue graph and assignment policy determine whether the task is available.
Jig coordinates the requested transition and records its evidence without implementing a competing claim engine.
An explicit actor prevents an accidental ambient username from claiming on behalf of the wrong agent.
Crash reconciliation is required because a successful database write can outlive the caller's response or local append.
No automatic rollback unclaims an issue, because a subsequent writer may already be working under that assignment.

### Dependencies and consumers

Blocked by T4, which transitively requires T2's adapter and T3's linked-start identity/reuse rules.
Uses T1's operation-journal records and immutable links.
Unblocks T7 lifecycle completion and contributes claim scenarios to T9's integration matrix.
T5 exposes claim observation without issuing a claim from status/evidence commands.
T8's combined guidance must use this optional claim form consistently.

### Verified implementation entry points

`crates/jig/src/cli/work.rs` owns work command argument parsing.
`crates/jig/src/command/work.rs` defines `WorkStartRequest`.
`crates/jig/src/runtime/work.rs` currently validates plan input before opening the session and plan.
Preserve the existing guarantee that invalid request input does not leave an orphan durable session.
`crates/jig/src/runtime/tests/work.rs` and `runtime/tests/work/` contain work integration coverage.
`crates/jig/src/runtime/tests/mcp/repository_execution/work_tools.rs` exercises MCP work tools.
The installed Beads help defines `br update --claim` as setting assignee to actor and status to `in_progress`.
Beads claim exclusivity is configurable upstream; do not infer a global compare-and-set guarantee from the flag name.

### Proposed CLI and transport contract

All new flags below are proposed.
Use `jig work start --issue beads:example-123 --claim --actor ExampleAgent`.
Add optional `claim` and `actor` fields to the shared request and MCP schema.
Require `--issue` when `--claim` is supplied; unlinked claiming is invalid.
Require a nonempty explicit actor or an explicitly configured integration actor whose source is shown in output.
Do not silently derive the claim actor from an unrelated operating-system account.
Reject contradictory issue/plan inputs before any tracker write.
Without `--claim`, linked start performs no assignee/status mutation.
Return the durable plan ID even when a later claim or export step fails.
Expose `not_requested`, `confirmed`, `already_owned`, `conflict`, and `indeterminate` claim outcomes.
Distinguish an externally owned issue from one already assigned to the same actor.

### Claim workflow

Resolve and validate tracker, exact issue identity, actor, and local linked-start inputs first.
Read current issue content and assignment before creating a claim operation intent.
Refuse to claim closed, tombstoned, or unavailable issues through linked start.
Treat another actor's assignment as a conflict and do not add `--force` to bypass it.
If the issue is already assigned to the selected actor and is `in_progress`,
return an `already_owned` observation after linking/backlinking without creating
a claim intent or invoking `update --claim` again.
Preserve any older pending claim intent for its normal history reconciliation.
If assigned to that actor in another nonterminal status, report the need for an
explicit tracker transition; do not substitute an unrequested status update.
Create or reuse the linked plan using T3's local lock and baseline rules.
Create the backlink/export state through T4 without using comment success as proof of claim authority.
Append a claim intent identifying the actor, issue, plan, and original task-content revision.
Invoke the selected Beads profile's `update --claim` with explicit actor and exact database.
Pass no force, policy-bypass, or direct status-update fallback.
Require an atomic transition comment with a versioned claim-operation marker for supported automated claims.
Derive that comment from the plan ID and marker; do not use a vague unconditional completion statement.
Read the same issue after the command and verify assignee and `in_progress` status.
Only then append a confirmed claim result and perform the configured export step.
If Beads refuses a blocked task, surface the refusal; Jig must not compute its own override from stale `bv` output.

### Exclusivity boundary

Document whether the supported profile exposes Beads' exclusive-claim configuration and how doctor observes it.
If verified exclusive claims are required by the integration, fail before mutation when that capability is absent or disabled.
Do not rewrite Beads' own exclusivity configuration during a claim.
Where exclusivity is not verifiable, report that limitation and do not describe the adapter as a global scheduler.
The initial supported workflow should require the verified exclusive mode for automated claims.
Pre-read and post-read checks supplement upstream enforcement but do not replace it.
The repository-local lock serializes local Jig starts only.
Another clone or direct `br` client remains governed by Beads' storage and synchronization behavior.
Two clones using separate local databases cannot obtain a distributed exclusive claim merely by sharing a workspace identifier.
No lock-file name, lease record, or advisory comment should imply otherwise.

### Retry and crash recovery

Retry a recorded claim intent by first reading current issue state under the same workspace identity.
If it is already assigned to the intended actor and remains `in_progress`, record `already_owned` without another mutation.
If it is unassigned/open, inspect the exact atomic claim-operation marker first.
An existing marker means the earlier claim occurred and was subsequently cleared;
record that reversal and do not reclaim under the old intent.
Only retry when complete supported history proves the marker absent and the
original intent is still valid. Incomplete history leaves the operation indeterminate.
If another actor owns it, record a conflict and preserve the local plan for diagnosis or explicit reuse.
If it is now closed, stop; do not reopen it as part of recovery.
If task description or acceptance criteria changed since intent, return task-context drift before issuing a new claim.
A lost successful response can reconcile from current ownership without fabricating an exact historical claim timestamp.
Do not automatically clear assignment when plan creation, backlinking, or export fails afterward.
An export failure after confirmed claim retries only export.
Retain enough journal detail to distinguish a fresh request from replay of the same intent.
Report uncertainty honestly when ownership changed twice and current state cannot identify the original outcome.

### Acceptance criteria and meaningful tests

A valid unassigned generic issue becomes assigned to the requested actor and `in_progress` through Beads itself.
The result refers to the same plan as a subsequent linked-start retry.
Without `--claim`, the same workflow preserves issue assignment and status.
Invalid actor or missing issue arguments produce no session, plan, comment, or claim mutation.
An issue assigned to another actor is refused without `--force` or fallback status writes.
A blocked issue is refused according to Beads policy, even if an earlier triage snapshot listed it as available.
An issue already owned by the same actor can reuse its suitable execution record without duplicate claims.
Inject a lost response after successful claim and prove retry observes ownership rather than assuming failure.
Inject successful claim, lost acknowledgement, explicit unclaim, then retry;
the old operation must not claim the issue again.
Inject plan/backlink/export failures and prove no automatic unclaim occurs.
Tests distinguish one-checkout serialization from separate-clone behavior without asserting global exclusivity.
CLI and MCP reject the same invalid combinations and return the same normalized claim states.
At least one real supported `br` test validates exclusivity behavior; mocks alone cannot establish upstream claim semantics.
Run focused workflow/CLI/MCP tests, then required backend verification through `JIG_DEV_BIN`.

### Non-goals

No implementation of `bv`, claim queues, distributed locking, or task scheduling.
No new Jig-owned assignee/status authority.
No automatic issue creation, reopen, unclaim, or reassignment.
No agent-launch changes merely to add the claim adapter.

## T7 — Complete linked issues explicitly after accepted Jig execution

### Delivery outcome

An explicit completion request can close a linked Beads issue after Jig accepts required evidence.
Ordinary `work finish` continues to finish execution only.
A failed tracker close or export can be retried after the Jig plan is terminal without closing the plan or rerunning successful gates again.
The result clearly separates accepted execution, Beads closure, and export publication.

### Ownership and rationale

Passing required checks is evidence about the execution, not proof that every issue acceptance criterion is fulfilled.
The explicit completion flag is the caller's declaration that issue-level acceptance is satisfied.
Jig verifies its existing gates and records that declaration before asking Beads to close the issue.
Beads retains closure policies, dependency checks, and the final task status.
There is no distributed transaction between Jig's JSONL and Beads' database.
Durable intent and reconciliation preserve recoverability without undoing accepted execution or inventing success.

### Dependencies and consumers

Blocked by T5's complete CLI/MCP/output exposure and T6's integrated identity/claim workflow.
Consumes T1's journal and T4's backlink/export primitives.
Unblocks T9 lifecycle and supported-binary acceptance validation.
T8 final lifecycle examples must be checked against this proposed interface before the epic is complete.

### Verified implementation entry points

`crates/jig/src/command/work.rs` defines `WorkFinishRequest` with `plan_id`, resolution, and outcome.
`crates/jig/src/runtime/work.rs` currently rejects an already-closed plan before gate evaluation.
`finish_with_cancellation` retains a shared repository execution lease through durable plan closure.
`finish_after_required_gates_passed` rechecks source fingerprint and time validity, then closes the plan and ends the current session.
`crates/jig/src/state/plans.rs` retains the state-layer open/closed invariant.
The new retry path must be a specific runtime integration path, not a relaxation of generic `plans_close` invariants.
`crates/jig/src/cli/work.rs`, `cli/output/work.rs`, and MCP work schemas carry the public surface.
Installed Beads `close` supports explicit reason and transition comment and owns terminal-state policy enforcement.

### Proposed public contract

All new options and response fields below are proposed.
Use `jig work finish --plan-id <id> --complete-issue --actor ExampleAgent --resolution "Acceptance criteria verified"`.
Require an immutable linked issue when `--complete-issue` is requested.
Require a nonempty completion resolution for this form so the caller's acceptance declaration is durable.
Require outcome `success` if supplied; default this explicit form to success.
Reject failure/cancelled/custom outcomes combined with `--complete-issue`.
Execution-only finish retains its existing free-form outcome compatibility.
Keep existing unlinked and linked finish behavior when the flag is absent.
Add `complete_issue` to the shared request and MCP schema with a backward-compatible false default.
Return separate execution, tracker-close, and export states and a concrete retry invocation.
Use a nonzero/error result for requested closure that remains incomplete, while retaining the successful execution result in structured output.
Manual export may remain `pending_manual` without falsely claiming an automatic export failed.
Repeating the exact completion request resumes the existing operation by plan ID.
`work sync --plan-id <id>` also resumes only previously recorded operations.
An identical finish retry must retain the original resolution, outcome, actor,
workspace, issue, and profile authority; conflicting values are rejected.
An omitted actor uses only an explicitly configured integration actor, as in T6.
Allow completion for an unassigned issue or one assigned to that selected actor.
Reject an issue assigned to another actor; do not silently claim or reassign it.
Freeze selected actor and observed assignee in completion intent.
Require that observation to remain compatible before an unconfirmed close.

Completion result matrix:

| Tracker close | Export policy/result | CLI result |
| --- | --- | --- |
| Confirmed correlated close | Automatic export succeeded | Success |
| Confirmed correlated close | Manual, pending publication | Success, explicit pending_manual |
| Confirmed correlated close | Automatic export failed | Nonzero, preserve confirmed close |
| Refused, conflicting, or indeterminate | Any | Nonzero, preserve accepted execution |
| Closed externally without correlation | Any | Nonzero, already_closed_externally |

CLI JSON and MCP error signaling retain the structured partial payload, including
plan/session facts and distinct execution, tracker-close, and export states.
Do not lose accepted execution details by propagating a generic anyhow error.

### Preflight and durable sequence

Validate issue link, tracker profile, actor policy, resolution, and export configuration before running a write.
Read the exact issue and compare task-content revision against the linked execution context.
If acceptance criteria or task description drifted, stop before closure and require a new execution.
The exact recovery is `jig work start --issue beads:ID --new-attempt`, then
`jig work check --plan-id NEW_ID`, then explicit completion for `NEW_ID`.
The new start snapshots current acceptance scope and captures its own baseline.
There is no reconciliation override that blesses an old completion intent.
Do not treat Jig's own comments or audit timestamps as changed acceptance criteria.
Run the existing required-gate evaluation and freshness/time-validity checks unchanged.
Persist completion intent with linked identity, issue-content revision, resolution, and proof of accepted execution.
Prepared intent is not committed finish authority until the matching plan-close event exists.
Allocate and freeze the intended plan-close event ID in completion intent.
Add a scoped prepared-close helper that passes that exact ID through the
existing PlanEvent encoding without changing its wire shape or public close API.
Only that exact event commits this completion intent. Another ordinary finish
or low-level close event for the plan cannot authorize the pending tracker close.
If interruption leaves the plan open, reevaluate current gates and authority
before finishing; prepared intent must not bypass final checks.
Close the Jig plan exactly once through the existing state transition under its existing source-authority lease.
Persist the confirmed execution-closure step before issuing the external close operation.
If plan close exists but operation acknowledgement is missing, reconcile the
exact close event and frozen evidence before continuing.
Freeze the finishing session ID; recovery must not end a different session
that became current after interruption. Preserve generic session semantics.
If a different close event already finished the plan, report completion conflict;
keep that history and require a new linked attempt rather than adopting its proof.
Release or transition leases in a documented order before allowing a worktree-mutating export.
Reread acceptance-relevant context and assignee immediately before the first
external close and before each retry, including after gate evaluation.
Stop on observed drift or reassignment.
The verified br close surface has no expected-revision/assignee compare-and-set.
Concurrent direct tracker writers can therefore race the final read and close;
document this limit and never claim a local Jig lease excludes that race.
Call Beads `close` for the exact issue, with the selected profile's actor, reason, and supported transition comment.
Do not pass `--force`, `--bypass-policy`, or an `update --status closed` fallback.
Reread issue state and record confirmed closure, then run or defer export according to T4.
Require the supported profile to put a versioned completion marker in the atomic transition comment so a lost response can be reconciled.
If that atomic marker contract is unsupported, refuse completion before intent
or plan closure; linking and execution-only finish can still work.

### Terminal retry contract

Only a terminal plan with a matching durable completion intent can resume an unfinished tracker completion.
A previously finished plan with no completion intent does not retroactively gain evidence by supplying the flag.
Do not reopen or reclose the Jig plan, start another session, or regenerate a baseline on retry.
Reuse the frozen accepted-execution proof for its original source state.
Revalidate tracker workspace identity, issue ID, task-content revision, and operation authority before an unconfirmed close.
If the issue is already closed with the matching completion marker, record success and retry only unfinished export.
If the issue is already closed by another actor without that marker, report `already_closed_externally`; do not attribute their action to Jig.
Inspect the exact operation's atomic transition marker before every retry,
regardless of current issue status or local acknowledgement.
If the marker exists and the issue is open, record reopened-after-operation
and refuse another close, even when the original acknowledgement was lost.
If history cannot distinguish never-applied from applied-then-reversed, leave
the operation indeterminate and require a new explicit attempt.
If a new claimant or changed acceptance scope is observed, stop rather than completing their changed task.
A lost close response remains indeterminate until current state and marker can be inspected.
Export failure after confirmed close cannot trigger another close request.

### Source freshness and changed-checkout policy

Evidence continues to authenticate the source state accepted at Jig closure.
Beads closure and export can subsequently change `.beads` files that ordinary gates inspect.
The completion output must identify those as post-acceptance tracker changes, without describing earlier receipts as validating the new bytes.
Do not activate `.beads` freshness exclusion to make the completion command appear fully fresh.
Persist the source/proof identity before external mutation and report observed export completion afterward.
For a terminal retry whose external close has not occurred, require the current source proof and execution authority to match the recorded intent.
Do not invent exceptions that subtract assumed operation-owned tracker changes from the source fingerprint.
If source attribution is ambiguous after a crash, stop with a recovery diagnosis instead of silently accepting `.beads` changes.
Once tracker close is confirmed, export-only recovery may publish the current tracker state under the configured helper's own checks.
Export-only recovery must not assert that the old Jig gates checked subsequently changed source.
A meaningful application/source change before an unconfirmed close requires a new linked execution and current evidence.
These boundaries intentionally preserve valid historical proof while preventing it from authorizing changed work.

### Acceptance criteria and meaningful tests

Default finish closes the plan and session exactly as before and performs no Beads close.
The explicit form refuses an unlinked plan or empty resolution before any mutation.
Failed, missing, stale, or expired required gates prevent both execution closure and issue closure.
Fresh accepted evidence plus explicit completion closes the linked issue through Beads policy.
A Beads policy refusal leaves execution accepted and reports tracker completion failed without bypassing policy.
Inject a crash after plan closure but before Beads close and prove retry does not close the plan twice.
Inject interruption after intent, plan-close append, session end, and before
local closure acknowledgement; prove a different current session is untouched.
Prepare completion intent, interrupt, then close through ordinary finish;
retry must reject that different close event and leave the issue unchanged.
Inject a lost successful close response and prove marker reconciliation does not duplicate a transition/comment.
Inject export failure and prove retry only reruns the configured export step.
Issue-content drift and reassignment prevent an unconfirmed close from completing changed work.
An externally closed issue is reported accurately without fabricated Jig ownership of its transition.
A reopened issue after confirmed completion is not reclosed by a replayed command.
Successful close followed by lost acknowledgement, reopen, and retry must
also refuse another close under the old intent.
Task edits and reassignment between preflight and the final preclose read
must be observed before any external close invocation.
Source changes before an unperformed close require a new linked execution with current evidence.
CLI, JSON, MCP, status, and evidence distinguish each partial state and preserve backward-compatible default behavior.
Use real Beads policy/transition tests in T9 in addition to fault-injected runtime coverage here.
Review the generated diff for stale guidance and run required backend verification through the built dev binary.

### Non-goals

No automatic closure from gates, review acceptance, or ordinary `work finish`.
No automatic verification of prose acceptance criteria.
No force closure, automatic reopen, remote rollback, or exactly-once distributed transaction claim.
No changing the meaning of old receipts or rewriting existing append-only records.

## T8 — Ship optional init/adopt integration and coherent agent guidance

### Outcome

Full Jig setup offers an optional Beads workflow and adoption recognizes an
existing tracker without silently changing project ownership decisions.
Generated guidance teaches one combined task/execution workflow.
Existing repositories retain authored instructions and export policy.

### Why this task exists

The present generated block tells every substantial task to create Jig work.
A separately installed Beads block then supplies another complete lifecycle.
Even a correct runtime adapter would remain unused or misused if setup keeps
shipping those independent instructions without explaining their relationship.
Integration must therefore reach templates, persisted answers, and adoption.

### Entry points

- `templates/project/AGENTS.md.jinja`.
- `templates/project/.agent/PLANS.md.jinja`.
- `crates/jig/src/bootstrap/managed_paths.rs`.
- `crates/jig/src/bootstrap/answers/raw_answers.rs`.
- `crates/jig/src/bootstrap/answers/serialization.rs`.
- `crates/jig/src/bootstrap/answers/adoption.rs`.
- `crates/jig/src/bootstrap/repository_model/adoption.rs`.
- `crates/jig/src/bootstrap/repository_model/adoption_refresh.rs`.
- `crates/jig/src/cli/init_wizard.rs` and relevant tests.
- `crates/jig/src/bootstrap/tests/basic/` adoption ownership tests.
- `crates/jig/src/bootstrap/embedded_template_snapshots/`.
- `docs/adoption.md`, `docs/configuration.md`, and `docs/public-contract.md`.
- Root `AGENTS.md` when adopting the combined source-repository workflow in T9.

### Proposed setup options

Offer tracker selection `none` or `beads` for the full harness.
Default new projects to `none`.
Expose the same choice as a noninteractive init/adopt argument.
Preserve explicit stored choices during readoption.
Do not enable tracking solely because `.beads` exists during noninteractive run.
Interactive adoption can present a detected candidate and explain its effect.
An explicit command-line choice takes precedence over detection.

When enabling against an existing root tracker, obtain its identity through T2.
Persist a portable Jig workspace ID once; preserve it on repeated adoption.
Do not infer workspace identity from absolute checkout path or issue prefix alone.
If existing configuration names another store, report the conflict.
Do not migrate, merge, replace, or reinitialize an existing Beads database.

When explicitly enabling Beads for a new project with no tracker, use the verified
`br init` command only after the user selected that setup mode.
Require the executable to be available; report installation instructions if not.
Do not download `br`, install global skills, or append upstream guidance behind
the user's back as a side effect of Jig setup.
Treat Beads initialization as an external setup effect in init's transaction
report; do not promise rollback can undo an arbitrary tracker command.
On failure preserve the initialized store and report retryable setup state.

### Minimal harness behavior

The current minimal footprint deliberately omits the full managed root guide.
Keep that ownership boundary.
If tracker configuration is explicitly requested with a minimal harness,
configure supported runtime behavior and provide a concise output pointer to
the workflow documentation without taking ownership of root AGENTS.
Do not silently expand minimal setup into a full harness.
The setup report must identify which guidance was or was not installed.

### Generated workflow text

State that Beads owns tasks and Jig owns execution evidence.
Use `bv --robot-triage` only when choosing backlog work.
Verify candidate status through `br` before claiming it.
For a specific user-assigned issue, work on that issue instead of replacing it
with whichever item a global triage command currently ranks first.
Start linked substantial implementation with `work start --issue ...`.
Use `--claim` only when beginning ownership of the task.
Doctor must report when the verified exclusive-claim policy is disabled.
Document `br config set claim_exclusive true` as the explicit tracker-owned
setup needed for automated claims; preserve existing policy on adoption.
Do not advertise --claim as ready until that policy is verified.
Use one task-local ExecPlan when the owning delivery bead is complex.
Record implementation decisions and evidence in the linked execution.
Complete only after acceptance is met and required evidence passes.
Explain execution-only finish and explicit issue completion in one short example.
Use the configured export workflow after mutations.
Do not ask agents to duplicate acceptance criteria into another issue or plan.
Do not require creating records for investigation or routine small edits.

Keep detailed Beads command reference outside the managed AGENTS summary.
Reference installed upstream documentation or an appropriate generated guide.
Do not replicate the entire robot flag catalogue inside the Jig block.
This reduces contradictory lifecycle instructions and maintenance drift.

### Existing instruction blocks

Preserve content outside Jig's block exactly under normal adoption rules.
Recognize an existing Beads/bv marker for diagnostic purposes only.
If it describes a conflicting close/sync workflow, emit a concrete warning
identifying the conflicting section and proposed project-owner reconciliation.
Do not delete or rewrite another tool's block automatically.
For this repository, T9 may edit its authored block because the epic explicitly
includes source-repository dogfooding and guidance reconciliation.
Consumer fixtures must prove normal adoption preserves authored bytes.

### Export and freshness ownership

Preserve explicitly configured helper argv and export mode.
For existing trackers whose policy is unknown, require an explicit policy
selection or retain manual export; never bypass an existing helper silently.
Never set `receipt_metadata = ["beads"]` from tracker detection.
Explain that it is valid only when no gated command consumes that store.
Leave this source repository's existing privacy validation effective.
Do not introduce broad freshness path exclusions as part of setup.

### Meaningful validation

Render full init with no tracker and compare standalone workflow behavior.
Render explicitly enabled Beads with a stubbed supported binary.
Exercise missing binary with no partially claimed successful setup report.
Adopt an existing tracker with custom AGENTS content outside Jig's markers.
Readopt twice and prove workspace ID, helper argv, and authored bytes persist.
Exercise noninteractive detection without explicit enablement: remain disabled.
Exercise minimal footprint with explicit tracker configuration.
Ensure no root guidance ownership is added unexpectedly.
Test conflicting or routed tracker identity as a clear setup diagnostic.
Verify rendered CLI examples exist and validate against actual command parsing.

### Acceptance criteria

- [ ] Beads remains optional and new setup defaults to standalone Jig.
- [ ] Explicit init/adopt configuration persists across readoption.
- [ ] Existing trackers are detected without replacement or implicit enrollment.
- [ ] Generated guidance describes one coherent task/evidence workflow.
- [ ] Consumer-owned instructions and export policy survive adoption.
- [ ] Minimal footprint keeps its root-guidance ownership boundary.
- [ ] Metadata freshness exclusions remain explicit ownership decisions.
- [ ] Template snapshots, CLI help, and public docs match implemented behavior.

### Dependencies and handoff

Blocked by T5 and T7 so all shipped workflow examples are executable.
Unblocks T9's full consumer and source-repository dogfood verification.
No dependency on unrelated adoption feature epics is needed unless actual code
conflicts reveal a concrete prerequisite during implementation.
This task is setup behavior plus guidance, not a standalone planning deliverable.

## T9 — Validate real-Beads compatibility and dogfood the complete workflow

### Outcome

A supported real `br` executable and generated Jig consumer complete the linked
workflow with correct issue references, evidence, retries, and export handling.
The Jig source repository adopts the optional integration and coherent guidance.
The final public compatibility claims are backed by exercised behavior.

### Why this task exists

Mocked JSON can accidentally describe the adapter implementation rather than
the actual tracker contract.
The current tracker already has policy-dependent response envelopes and export
semantics that make a real executable check valuable.
This task validates the cross-component boundary and ships the repository's
actual adoption; it does not substitute for each task's focused regressions.

### Entry points

- `crates/jig/tests/`: use the existing CLI/consumer fixture organization.
- `crates/jig/src/bootstrap/tests/`: generated harness fixtures.
- `crates/jig/src/runtime/tests/work/`: linked lifecycle regressions.
- `scripts/beads-sync.py` and `scripts/tests/test_beads_sync.py`.
- `AGENTS.md`, `.jig.toml`, and generated managed blocks for source dogfooding.
- `.github/workflows/repo-policy.yml` if a bounded compatibility job is added.
- `docs/public-contract.md`, `docs/configuration.md`, and `docs/adoption.md`.

### Real executable policy

Pin the tested Beads version and record its checksum/source provenance.
Do not fetch mutable latest releases inside ordinary offline unit tests.
Separate hermetic fixture tests from an opt-in or provisioned integration lane.
The release-support claim requires the real lane to run in its configured CI.
If the executable is absent in an optional local run, report the skip honestly.
The mandatory compatibility lane must fail if provisioning is missing.
Start with verified 0.5.7 and add 0.6.0 only after the same suite passes.
Do not claim all later versions are compatible without profile validation.

### End-to-end fixture

Create a temporary `ExampleProject` with an isolated HOME/config environment.
Initialize a real tracker using the supported command surface.
Create a generic issue with multiline acceptance criteria.
Initialize or adopt the full Jig harness with explicit tracker enablement.
Start linked work and verify the canonical issue ID in its local journal.
Verify the issue contains exactly one correlated backlink for the start.
Repeat start and confirm the same open plan ID returns.
Run a small real passing gate and inspect evidence.
Finish execution only and confirm the issue remains open.
Create a fresh attempt and explicitly request claim and completion.
Confirm the expected assignee/status changes and correlated close evidence.
Confirm export uses the configured policy and produces portable metadata.

### Recovery fixture

Use the real tracker for semantic checks and controlled process wrappers for
fault injection around its observable invocation boundary.
Do not replace actual tracker mutation with a fabricated success response.
Interrupt after a real comment/claim/close commits but before Jig acknowledgement.
Retry and verify no duplicate correlated comment or second unintended mutation.
Force export-helper failure after a confirmed database close.
Retry after restoring the helper; do not rerun gates or close again.
Change acceptance criteria before a pending uncommitted close retry.
Observe refusal to apply stale completion intent.
Reopen after a confirmed completion and verify old retry cannot close it again.
Exercise a second checkout with the same portable tracker identity and separate
execution history without pretending local leases are distributed locks.

### Freshness fixture

In one consumer, explicitly classify tracker data as receipt metadata.
In another, make a gate inspect the tracker export and leave exclusion unset.
Verify start-time claim/backlink/export is settled before checks.
Verify final close is reported as a post-validation tracker change.
Do not automatically bless a tracker edit that affects a declared gate input.
Confirm execution receipts retain their original source identity after close.
Historical finish evidence and current freshness must be distinguishable.
Use the existing freshness engine; this epic adds no path-normalization bypass.

### Source repository adoption

Enable the new optional tracker configuration in `.jig.toml`.
Use the existing canonical root Beads store; do not choose a DB by filename glob.
Configure the existing `python3 scripts/beads-sync.py` helper as argv.
Enable and verify the source repository's explicit Beads exclusive-claim policy
through `br config set claim_exclusive true`; retain its project-owned config.
Preserve its privacy invariant and tombstone handling.
Reconcile root Jig/Beads guidance into the implemented combined workflow.
Keep useful `bv` triage guidance without retaining a contradictory close path.
Do not set the metadata freshness exclusion automatically.
Link only the delivery work actually being executed; do not mass-convert history.
Use one real delivery issue as a documented dogfood example when implementing.
Do not invent a production issue solely to generate acceptance screenshots.

### Verification commands

Read nearest crate instructions before runtime or UI changes.
Build the development runtime after implementation:

```sh
cargo build -p jig-sh --bin jig
```

Use the development runtime for receipt-producing harness verification:

```sh
JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
JIG_DEV_BIN=target/debug/jig scripts/jig check contract
JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id PLAN_ID
JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id PLAN_ID
JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id PLAN_ID
JIG_DEV_BIN=target/debug/jig scripts/jig check test
```

`PLAN_ID` is the actual implementation execution record, never a literal fixture.
Run focused fixture targets using the names added by their owning tasks.
Record real commands and outcomes in the implementation ExecPlan/receipts.
Use `python3 scripts/beads-sync.py --check` for exported privacy verification.
Run the existing sync-helper unit tests if its implementation is changed.
Do not present these expected commands as already executed by planning.

### Acceptance criteria

- [ ] A pinned supported real br passes the generated-consumer workflow.
- [ ] Tests observe actual tracker state rather than only adapter return values.
- [ ] Interrupted mutations reconcile without duplicate logical effects.
- [ ] Close success/export failure has a tested export-only recovery path.
- [ ] Reopened and acceptance-changed tasks reject stale completion replay.
- [ ] Both tracker-freshness ownership modes behave as documented.
- [ ] This repository uses the integration with its existing privacy helper.
- [ ] Root guidance and shipped guidance agree on ownership and lifecycle.
- [ ] Required repository checks pass with the development runtime.
- [ ] Public support claims match the real executable versions exercised.

### Dependencies and handoff

Blocked by T7 and T8; transitively consumes every preceding task.
The task closes the epic's final integration gap.
Close the epic only after all child acceptance outcomes are verified.
Keep any unsupported additional Beads version out of the completion claim.
Create follow-up delivery work only for concrete newly discovered scope that
cannot be resolved within the agreed integration, not for more planning rounds.

## Review and conversion record

This section records planning evidence only.
It must not claim implementation tests ran.
Round 1: full 1,596-line draft reviewed by an independent reasoning agent.
The graph and five sampled rationales passed; lifecycle recovery needed revision.
Accepted revisions cover lost acknowledgement followed by reopen/unclaim,
local plan/session-close crash boundaries, final issue-context reread, and the
absence of an upstream atomic expected-revision check.
An isolated T7 reader identified missing actor, retry, drift, and exit contracts.
Those now require a new attempt on drift, explicit actor rules, exact retries,
atomic markers, and a partial-result matrix including intentional manual export.
The optional 0.6.0 profile remains conditional on real executable validation.
Round 1 is not steady state; subsequent full-plan review is required.
Round 2: independent full-plan review retained the nine-task graph and accepted
five sampled rationales. Added exact local close-event correlation, an existing
attempt selector, the already-owned claim branch, and callable export-only
authority acknowledgement. The isolated T4 check led to explicit manual-export
result/guidance rules and a bounded helper-policy recovery command.
Round 2 required targeted contract revisions; another full review is required.
Round 3: an independent agent read all 1,725 lines and found no remaining
material defects or required corrections. DAG and rationale samples D2/D4/D7/
D8/D10 passed against repository evidence. The isolated T3 reader found its
start/reuse/link specification self-contained with no missing user decisions.
No delivery contract changed after round 3; the fourth review checks steady state.
Round 4: a new independent agent read the full 1,730-line plan and passed it
without corrections. DAG and rationale samples D1/D2/D4/D8/D9 passed against
repository evidence. A fresh isolated T8 reader confirmed setup/adoption scope
is self-contained. Rounds 3 and 4 reached steady state with no contract changes.
The local graph validator also passed after each revision: nine deliverables,
acyclic prerequisites, T1/T2 roots, T9 final consumer, and acceptance/rationale/
entry-point sections on every task.
The reviews used the available reasoning agents; no GPT Pro web review is claimed.
Delivery graph conversion follows this reviewed version.

### Created Beads delivery graph

Epic: `jig-sh-x8ow` — Integrate Beads tasks with Jig execution evidence.
All records are open at P2; none has been claimed for implementation.

| Plan key | Beads delivery ID |
| --- | --- |
| T1 | `jig-sh-x8ow.1` |
| T2 | `jig-sh-x8ow.2` |
| T3 | `jig-sh-x8ow.3` |
| T4 | `jig-sh-x8ow.4` |
| T5 | `jig-sh-x8ow.5` |
| T6 | `jig-sh-x8ow.6` |
| T7 | `jig-sh-x8ow.7` |
| T8 | `jig-sh-x8ow.8` |
| T9 | `jig-sh-x8ow.9` |

Each child contains its full reviewed delivery section and a separate acceptance
criteria field. The epic contains the shared architecture and dependency graph.
No planning/review/conversion-only child was created.

Graph polish and verification completed through tracker APIs:

1. Inventory: exactly one epic and nine open P2 delivery tasks under its label.
2. Content: every child body and acceptance field matches its reviewed section.
3. Membership: each child has exactly the intended parent-child edge.
4. Prerequisites: all blocking edges match the plan; no cycle or unintended orphan.
5. Readiness: `br ready --epic jig-sh-x8ow --json` returns only `.1` and `.2`.
6. Viewer/export: scoped `bv --robot-plan` reads the current export and shows
   the expected two ready delivery tasks and seven blocked delivery tasks.
   It also lists the summary epic as a candidate; authoritative child readiness
   is taken from `br ready`, not that summary row.

The graph needed no structural revision after conversion.
The repository-mandated `python3 scripts/beads-sync.py` completed successfully.
Its `--check` privacy guard passed and `git diff --check` reported no whitespace errors.
That export also published pre-existing tracker database changes absent from the
checked-in JSONL; those were preserved rather than discarded to narrow the diff.
No application implementation, build, or runtime acceptance test was performed
by this planning session. Runtime tests remain the delivery beads' responsibility.
