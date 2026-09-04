# Unified Terminal Dashboard for Jig

Status: implementation-ready delivery plan with audited dependency-linked Beads

Planning baseline: `b80a787ddba0330c033caabb1350e65a66289f14`

Document kind: project-level architecture and delivery plan, not a task-local ExecPlan

## 1. Executive outcome

Jig will retire its loopback web dashboard.

All human-facing flight-recorder functionality will move into one terminal dashboard.

The terminal dashboard will also retain every existing `jig status --tui` capability.

`jig ui` will become the canonical entrypoint for the unified terminal dashboard.

`jig status --tui` will remain as a compatibility entrypoint into the same dashboard engine.

The compatibility entrypoint will start on the status overview.

The canonical entrypoint will start on the work overview.

There will be one feature-specific dashboard crate.

There will be one application model.

There will be one terminal runtime.

There will be one rendering system.

There will not be parallel status and flight-recorder TUI implementations.

The shared `jig-tui` crate will remain presentation-neutral.

Repository reads, state joins, provider execution, and gate evaluation will remain owned by `jig-sh`.

The dashboard crate will consume snapshots through a cancellable source boundary.

The dashboard will remain read-only.

It will not record receipts.

It will not mutate plans.

It will not clear loop attempts.

It will not launch implementation agents.

It will not fetch remotes.

The HTTP listener will be deleted.

The request parser will be deleted.

The bootstrap capability and cookie machinery will be deleted.

The HTML and CSS renderers will be deleted.

The loopback JSON endpoints will be deleted.

Equivalent one-shot JSON inspection will remain available directly from the CLI.

`jig ui --json` will print a dashboard snapshot and exit.

`jig ui --plan PLAN_ID --json` will print one plan-detail snapshot and exit.

Interactive `jig ui --plan PLAN_ID` will open the dashboard with that plan detail selected.

The former `--port` option will be accepted only as a hidden migration diagnostic for one release.

Supplying `--port` will fail before terminal setup with an actionable message.

No ignored compatibility option will pretend that a server was started.

The final implementation will repurpose the generic `jig-ui` crate as the unified terminal application and remove the narrower `jig-status-tui` crate.

This naming makes the application boundary explicit.

The existing `jig-tui` crate will continue to own terminal mechanics shared by all Jig TUIs.

The cutover will preserve the generated launcher root-command classification for `ui` and `status`.

No generated repository contract epoch is required merely for the presentation change.

The generated launcher template already forwards the unchanged `ui` and `status` root commands without describing their presentation behavior.

It therefore requires no content or embedded-snapshot change for this cutover.

## 2. Why this project exists

### 2.1 Current product split

Jig currently exposes two overlapping operator dashboards.

`jig ui` is a browser-based flight recorder.

`jig status --tui` is a terminal-based status dashboard.

The browser dashboard presents structured work history.

The terminal dashboard presents repository and provider status.

Both also expose open-work and loop summaries.

Both are read-only.

Both refresh while running.

Both are owned by the same CLI release.

Both depend on adapters implemented in `crates/jig`.

The split is historical rather than architectural.

It gives users two places to inspect the same repository.

It gives maintainers two presentation stacks.

It gives maintainers two snapshot models for overlapping facts.

It gives maintainers two refresh implementations.

It gives documentation two separate mental models.

### 2.2 The web surface is disproportionate

The browser UI is intentionally small.

Its transport and security boundary is not small.

The current web crate owns a TCP listener.

It owns bounded HTTP request parsing.

It owns worker threads.

It owns request deadlines.

It owns Host validation.

It owns Origin validation.

It owns a random per-run namespace.

It owns a one-time bootstrap capability.

It owns an authenticated cookie.

It owns response security headers.

It owns HTML escaping.

It owns two HTML views.

It owns JSON routes.

Those controls are appropriate for a loopback web service.

They are unnecessary once the browser service no longer exists.

Deleting the transport removes an entire attack and maintenance surface.

It also removes several platform failure modes.

Port conflicts disappear.

DNS-rebinding defenses are no longer needed.

Slow-client handling is no longer needed.

Cookie and capability lifecycle bugs are no longer possible.

Browser discovery and copy-paste are no longer part of the workflow.

### 2.3 The terminal foundation already exists

Jig already depends on Ratatui 0.29.0.

Jig already depends on Crossterm 0.28.1.

`jig-tui` already restores raw mode and alternate-screen state.

`jig-tui` already handles cursor restoration.

`jig-tui` already filters key release events.

`jig-tui` already sanitizes terminal-controlled text.

`jig-tui` already owns cooperative background workers.

`jig-status-tui` already implements refresh without blocking navigation.

`jig-status-tui` already preserves stable selection across refresh.

`jig-status-tui` already supports bounded detail rendering.

The migration therefore reuses a proven local stack.

It does not introduce a new terminal framework.

### 2.4 A direct port is not enough

Copying HTML sections into terminal widgets would preserve duplication.

Adding flight-recorder tabs to `jig-status-tui` without changing its boundary would also preserve conceptual duplication.

The current status crate is explicitly scoped to versioned provider aggregates.

The current web crate is explicitly scoped to recorder snapshots.

The proper fix is one higher-level dashboard application.

That application must understand two data domains.

The first domain is local operational history.

The second domain is repository and provider status.

The domains have different costs.

The domains have different refresh cadences.

The domains have different failure modes.

They should share navigation and rendering without becoming one giant collection transaction.

### 2.5 Existing drift demonstrates the need for typed ownership

The current web model expects exhausted loop attempts to contain `workflow` and `item`.

The loop producer emits `workflow_id` and `item_key`.

Open beads `jig-sh-t9n` and `jig-sh-z9h` describe the same producer-consumer mismatch.

The mismatch can hide the entire loop panel when exhausted attempts exist.

This is not merely a missing field rename.

It demonstrates schema duplication across an untyped `serde_json::Value` boundary.

The migration must remove that class of drift.

The unified dashboard will use producer-owned typed local observations wherever the producer and consumer ship in the same release.

Versioned third-party provider reports will remain wire-decoded because additive compatibility is part of that external contract.

The aggregate shared by `jig status --json` and the dashboard will become one typed `StatusSnapshot`.

The third-party report boundary remains `jig_contract::status_provider::v1::Report`.

The in-process dashboard boundary will no longer pass `serde_json::Value` or redeclare the aggregate wire schema.

## 3. Product thesis

The dashboard is Jig's local operator console.

It answers two related questions.

The first question is, “What is happening in this repository?”

The second question is, “What evidence explains that state?”

Status observations answer the first question.

The flight recorder answers the second question.

One terminal application should let the operator move between them without changing commands or interaction models.

The application will optimize for inspection.

It will not become a mutation console.

Actions remain explicit CLI commands.

The dashboard may display exact recovery commands.

It will never execute those commands implicitly.

## 4. Product principles

### 4.1 One operator surface

There will be one general-purpose Jig dashboard.

Specialized Vault and Codex TUIs remain separate.

They have different trust and mutation models.

The unified dashboard must not absorb them.

### 4.2 Read-only by construction

The dashboard source trait will expose observation requests only.

No mutation callback will cross into the dashboard crate.

No command executor will be stored in the application model.

No key binding will run a shell command.

This keeps accidental action impossible at the presentation layer.

### 4.3 Independent failure domains

Local recorder state can remain useful when a provider fails.

Provider status can remain useful when one plan body is unreadable.

The last successful snapshot remains visible when a refresh fails.

Each pane displays the error for its own domain.

One failed refresh does not blank unrelated tabs.

### 4.4 Bounded presentation

Repository-controlled strings will be sanitized before terminal rendering.

Long plan bodies will remain bounded.

Receipt previews will remain bounded.

Changed-path collections will remain bounded.

Provider extensions will retain their existing bounds.

Every truncation will be visible to the operator.

No view will allocate proportionally to an unbounded terminal field during every frame.

### 4.5 Stable selection

Refreshes will preserve selection by stable identity.

Plans will use plan IDs.

Receipts will use receipt IDs.

Timeline rows will use kind plus durable event identity.

Providers will use provider IDs.

Packages will use package IDs within provider IDs.

Blockers will retain their current stable composite key.

Index-only preservation is not acceptable.

Third-party provider JSON is decoded into raw typed values before display sanitization.

Provider, package, blocker, plan, receipt, decision, session-event, and plan-event identities remain byte-for-byte raw for selection and routing.

Only separately prepared display strings and rendered provider-extension values pass through terminal sanitization.

This intentionally replaces the current status-model behavior that sanitizes the complete JSON value before deserialization.

### 4.6 Honest freshness

Each data domain will show its own observation time.

A spinner cannot imply that stale data is current.

Refresh errors will retain the previous observation timestamp.

The header will distinguish refreshing, current, stale, partial, and failed states using text as well as color.

### 4.7 One source of local truth

Loop status will be collected once per local dashboard refresh.

Open-plan gate status will be collected once per local dashboard refresh.

The flight-recorder builder will not independently reinterpret loop records after a status builder has already projected them.

Typed shared local observations will feed both summary and detail models.

### 4.8 Compatibility with explicit diagnostics

Human workflows should continue to have a useful path.

Old server-specific invocations should fail clearly.

Silently ignoring `--port` would be misleading.

Keeping a dead HTTP server for compatibility would defeat the project.

### 4.9 Testable without a real terminal

The model and rendering layers must remain deterministic.

They must be testable against Ratatui's test backend.

Terminal lifecycle tests will use the existing PTY harness.

Network tests will disappear with the network code.

### 4.10 No speculative action features

The migration will not add plan mutation.

It will not add gate execution.

It will not add attempt clearing.

It will not add agent launch.

It will not add remote refresh.

These actions would require a separate authority design.

## 5. Scope

### 5.1 Required dashboard parity

The unified TUI must display repository name.

It must display configured default branch.

It must display the current branch or detached state.

It must display Jig runtime version.

It must display contract version.

It must display observation timestamps.

It must display session count.

It must display open-plan count.

It must display decision count.

It must display every open plan returned by the snapshot.

It must display each open plan's gate summary.

It must display each gate's ID.

It must display whether each gate is required.

It must display gate status.

It must display gate freshness.

It must display last completion time.

It must display diff summary.

It must display a producer-owned remediation command for a gate when the gate evaluator can prescribe one safely.

It must display recent failed receipts.

It must display failure time.

It must display tool name.

It must display exit status.

It must display linked plan ID when present.

It must expose the bounded stderr preview in a detail view.

It must display recently completed plans.

It must display their resolution.

It must display their duration.

It must display check-health aggregates.

It must display tool run count.

It must display tool failure count.

It must display last status.

It must display last run time.

It must display average duration.

It must display configured loop workflows.

It must display workflow kind.

It must display enabled state.

It must display active leases.

It must display lease expiry.

It must display exhausted attempts.

It must display workflow ID.

It must display item key.

It must display the exact `loop clear-attempt` recovery command.

It must display the merged timeline newest-first.

It must support all timeline kinds.

It must support receipt-only filtering.

It must support failure-only filtering.

It must support plan-only filtering.

It must support session-only filtering.

It must support decision-only filtering.

It must display receipt status and duration.

It must display plan events and resolutions.

It must display session events and outcomes.

It must display decision selection and rationale preview.

It must link plan-associated rows to plan detail through Enter.

It must display open and closed plan details.

It must display the bounded plan body.

It must display the plan baseline reference.

It must display the plan baseline object ID.

It must display baseline errors.

It must display plan gates.

It must display plan decisions.

It must display decision alternatives.

It must display decision rationale.

It must display plan receipts.

It must display stdout previews.

It must display stderr previews.

It must display changed paths.

It must display receipt diff summary.

It must display receipt duration.

### 5.2 Existing status parity

The unified TUI must preserve the status overview.

It must preserve repository cleanliness.

It must preserve upstream ahead and behind state.

It must preserve work and loop summary counts.

It must preserve provider status.

It must preserve provider progress summaries.

It must preserve input freshness.

It must preserve diagnostics.

It must preserve collection errors.

It must preserve provider switching.

It must preserve package selection.

It must preserve blocked-only filtering.

It must preserve package preview.

It must preserve full package detail.

It must preserve dependencies.

It must preserve acceptance checks.

It must preserve evidence.

It must preserve namespaced extensions.

It must preserve blocker queue navigation.

It must preserve blocker details.

It must preserve additive provider-field tolerance.

### 5.3 CLI parity and replacement

`jig ui` remains repository-scoped.

`jig status` remains unchanged in human mode.

`jig status --json` remains schema version 1.

`jig status run RUN_ID` remains unchanged.

`jig status --tui` launches the unified TUI.

`jig status --tui --refresh-seconds N` remains accepted.

`jig ui --refresh-seconds N` configures the local recorder refresh interval.

`jig ui --status-refresh-seconds N` configures provider refresh.

`jig ui --timeline-limit N` selects the initial bounded activity window.

`jig ui --plan PLAN_ID` selects an initial plan.

`jig ui --json` emits one local recorder snapshot and exits.

`jig ui --plan PLAN_ID --json` emits one plan snapshot and exits.

`jig ui --json` does not run configured status providers.

Automation that needs providers continues to use `jig status --json`.

### 5.4 Deletion scope

Delete `crates/jig-ui/src/server.rs`.

Delete `crates/jig-ui/src/html.rs`.

Delete `crates/jig-ui/src/html/dashboard.rs`.

Delete `crates/jig-ui/src/html/plan.rs`.

Delete web-only tests.

Delete the loopback port constant.

Delete web-only random capability dependencies.

Delete web-only constant-time comparison dependencies where no other workspace consumer requires them.

Delete HTTP-specific documentation.

Keep `jig-ui` in the workspace and release list under its existing package identity.

Remove `jig-status-tui` from the workspace, dependency graph, and release list after its code moves.

Move the reusable status TUI implementation into `crates/jig-ui`.

Delete `crates/jig-status-tui` after both CLI entrypoints use `jig-ui`.

Update `agent-map.md` through the repository generator.

Update the new crate guide.

### 5.5 Out of scope

No browser fallback will remain.

No embedded webview will be added.

No localhost API will remain.

No remote dashboard will be added.

No persisted TUI preferences will be added.

No mouse support is required.

No clipboard integration is required.

No command execution palette is required.

No live file watching is required.

No new provider protocol version is required.

No change to `.agent/state` formats is required.

No change to receipt retention is required.

No change to loop mutation semantics is required.

No change to generated application code is required.

No change to Vault or Codex TUIs is required beyond shared regression validation.

### 5.6 Explicit parity matrix

| Current capability | Current source | Terminal destination | Acceptance oracle |
|---|---|---|---|
| Repository, harness, and default-branch identity | Browser dashboard header | Shared TUI header | Exact repository, runtime, contract, and default-branch labels render from a producer-derived snapshot |
| Current branch or detached revision | Status repository observation | Shared TUI header | Branch and detached fixtures render without replacing the default-branch label |
| Session, plan, and decision counts | Browser dashboard stats | Work summary | Seeded counts match projection output |
| Open plans | Browser dashboard cards | Work plan list | Every snapshot plan ID is selectable |
| Gate table | Browser plan cards and detail | Work preview and plan Gates section | Every gate field remains reachable |
| Gate remediation command | Producer enhancement at cutover | Gate detail | Producer-owned argv renders as inert shell-safe text and is never executed |
| Gate collection error | Browser plan card hint | Work preview error banner | Error does not erase other plans |
| Recent failures | Browser failure table | Health failures section | Failed receipts appear newest-first with time, tool, exit status, and nullable linked plan ID |
| Failure stderr | Browser expandable details | Failure detail overlay | Bounded stderr remains scrollable |
| Closed work history | Browser history table | Work closed-history subsection | Resolution and duration render |
| Tool statistics | Browser check-health table | Health tool section | Runs, failures, last result, and average render |
| Loop workflows | Browser loops table | Health loop section | Kind and enabled state render |
| Loop leases | Browser loop notes | Health loop section | Key and expiry render |
| Exhausted attempts | Browser attention notes | Health attention detail | Producer-native workflow and item fields render |
| Loop clear-attempt command | Producer enhancement at cutover | Health attention detail | Workflow/item argv with spaces and metacharacters renders as inert shell-safe text |
| Mixed timeline | Browser timeline table | Timeline list | Newest-first rows preserve receipt status/duration, plan event/resolution, session event/outcome, and decision selection/rationale fields |
| Plan-linked timeline navigation | Browser timeline links | Timeline Enter action | Every plan-associated row opens the matching raw plan ID |
| Timeline kind filter | Browser query chips | Timeline `f`/`F` filter | Every filter has a model test |
| Timeline limit | Browser `limit` query | CLI limit plus `+`/`-` | Bounds 1 and 1000 are enforced |
| Plan body | Browser plan page | Plan Body section | Bounded, sanitized body renders |
| Plan body error | Browser plan page hint | Plan Body error banner | Other detail sections remain available |
| Plan baseline | Existing recorder snapshot/JSON producer | Plan Summary section | Cutover enhancement renders requested ref, object ID, and baseline error states |
| Plan decisions | Browser plan-detail page | Plan Decisions section | Selection, alternatives, and rationale render |
| Plan receipts | Browser plan table | Plan Receipts section | Up to 50 rows remain selectable |
| Receipt stdout/stderr | Browser expandable detail | Receipt detail overlay | Both bounded previews render independently |
| Receipt changed paths | Browser expandable detail | Receipt detail overlay | Up to 20 paths plus omission count render |
| Receipt diff and duration | Browser receipt rows/detail | Receipt row and detail | Seeded diff summary and duration remain reachable |
| Dashboard JSON route | Authenticated HTTP endpoint | `jig ui --json` | One document is emitted and process exits |
| Plan JSON route | Authenticated HTTP endpoint | `jig ui --plan ID --json` | Found and not-found behavior is tested |
| Browser auto-refresh | Ten-second meta refresh | Completion-relative local timer | Fake-clock scheduler test proves cadence |
| Status overview structure | Existing status TUI tab 1 | Unified Status tab 1 | Existing section and empty/error renderer assertions migrate |
| Repository cleanliness and revision | Status repository section | Status repository section | Clean, dirty, branch, detached, and head fixtures render exact states |
| Upstream tracking | Status repository section | Status repository section | Ahead, behind, diverged, unavailable, reference, and basis fields render |
| Provider identity | Status provider header | Status provider header | Raw ID, display name, and adapter version remain independently reachable |
| Provider status and duration | Status provider header | Status provider header | Complete, partial, failed, and duration fixtures render |
| Provider failure detail | Status provider error | Provider detail | Error code, message, bounded stderr, and stderr-truncated marker render |
| Provider progress categories | Status progress summary | Status progress summary | Work-package, blocked-package, blocker, acceptance-check, diagnostic-level, specification, implementation, verification, and acceptance totals/categories render |
| Provider input freshness | Status freshness list | Status freshness list/detail | Name, kind, path, expected/observed revisions, dirty state, status, and reason render |
| Provider diagnostics | Status diagnostics list | Status diagnostics list/detail | Level, code, message, work-package link, and source render |
| Aggregate collection errors | Status error section | Status error section | Scope, code, and message render without erasing provider/local data |
| Status local work and loop counts | Status work/loop summaries | Unified Status local summary | Current session ID, open-plan count, gate snapshot/error counts, workflow count, live-lease count, attempt count, waiting-attempt count, and exhausted-attempt count render from `StatusLocalSnapshot` |
| Status partition observation age | Existing aggregate time plus refresh state | Unified Status/header age labels | Local and provider timestamps age independently and never imply a shared observation |
| Provider switching | `[` and `]` | `[` and `]` | Raw provider identity remains selected across reorder and refresh |
| Package list and selection | Existing status TUI tab 2 | Unified Packages tab 2 | IDs, titles, state summaries, and stable selection migrate |
| Blocked-only package filter | Existing `b` filter | Unified `b` filter | Only packages with blockers remain and selection clamps by raw key |
| Package compact preview | Packages split view | Packages preview | Selected package summary remains visible at supported wide sizes |
| Package facet detail | Package detail | Package detail | Native state, normalized category, summary, source path/line/column, and digest render for all three facets |
| Package dependencies | Package detail | Package detail | Every dependency ID remains reachable |
| Package acceptance checks | Package detail | Package detail | Ordinal, ID, state/category, target, and source render |
| Package blockers | Package detail | Package detail | Code, message, related package, and source render |
| Package evidence | Package detail | Package detail | Kind, reference, source, and digest render |
| Package extensions | Package detail | Package detail | Namespaced keys and bounded generic values render without key collision loss |
| Blocker queue navigation | Existing status TUI tab 3 | Unified Blockers tab 3 | Flattened blockers navigate with stable composite raw keys |
| Blocker detail | Blocker detail | Blocker detail | Provider/package identity, code, message, related package, and source render |
| Provider additive fields | Raw-plus-decoded provider boundary | Typed provider-v1 decoder plus retained raw report | Unknown root/nested fields survive status JSON; known extension values and colliding display labels remain reachable in TUI |
| Status refresh lifecycle | Existing status worker | Unified serialized scheduler | Existing semantics plus stronger sequencing tests pass |
| HTTP authentication | Browser server | Removed, no replacement | No listener or HTTP code remains |

The final acceptance task checks this matrix row by row.

A row cannot be waived merely because adjacent information is present.

If implementation intentionally improves a row, the test must prove the new behavior and the plan decision log must explain the change.

## 6. Terminology

### 6.1 Dashboard

The full-screen terminal application launched by `jig ui`.

### 6.2 Local recorder domain

Repository metadata, sessions, plans, decisions, receipts, gates, and loop observations owned by Jig.

### 6.3 Status domain

The versioned aggregate produced by `jig status`, including provider reports.

### 6.4 Snapshot source

The CLI-owned adapter that reads repository state and returns dashboard-owned view data.

### 6.5 Refresh domain

One independently scheduled kind of observation handled by the serialized worker.

### 6.6 Stable identity

A durable identifier used to preserve selection across reordered snapshots.

### 6.7 Detail overlay

A scrollable modal-like TUI layer for one plan, receipt, failure, package, or blocker.

### 6.8 Compatibility entrypoint

An older command spelling that invokes the same implementation without retaining the old transport.

## 7. Current architecture

### 7.1 Browser dashboard

`crates/jig-ui` owns transport, models, and presentation.

`crates/jig/src/ui.rs` implements `SnapshotProvider` for `RepoContext`.

`crates/jig/src/ui/snapshot.rs` scans local state.

It joins open plans with gate snapshots.

It joins loop status.

It builds history, failures, tool statistics, and timeline rows.

Plan detail performs separate plan, decision, receipt, body, and gate reads.

The server refreshes repository context on every request.

The browser refreshes every ten seconds through a meta-refresh tag.

### 7.2 Status terminal dashboard

`crates/jig-status-tui` owns a status-specific model.

It decodes schema version 1 from JSON.

It ignores additive provider fields.

It owns Overview, Packages, and Blockers tabs.

It owns one cancellable refresh worker.

`crates/jig/src/status/tui.rs` supplies the versioned aggregate.

Provider execution and repository observations remain in `crates/jig/src/status.rs`.

### 7.3 Shared terminal mechanics

`crates/jig-tui` owns terminal lifecycle.

It owns terminal text safety.

It owns cooperative worker lifecycle.

It intentionally does not own feature models or rendering.

That invariant remains unchanged.

## 8. Target architecture

### 8.1 Crate layout

`crates/jig-ui` will own the unified terminal application.

It will contain `lib.rs`.

It will replace its web transport modules with `model.rs` and focused model submodules.

It will contain `render.rs` and focused render submodules.

It will contain `runtime.rs` and focused runtime submodules.

It will contain deterministic test fixtures.

It will depend on `jig-tui`.

It will depend on Ratatui.

It will depend on Crossterm.

It will depend on Serde and Serde JSON for status decoding and CLI JSON models.

It will depend on `jig-contract` for the canonical provider-v1 report types.

It will not depend on `RepoContext`.

It will not depend on state persistence.

It will not depend on process supervision.

It will not depend on provider configuration.

It will not depend on HTTP, randomness, or cookie-authentication libraries.

### 8.2 CLI adapter layout

`crates/jig/src/ui.rs` will become the dashboard adapter.

`crates/jig/src/ui/snapshot.rs` will remain the recorder projection owner.

The projection will gain cooperative cancellation.

`crates/jig/src/status/tui.rs` will be removed after compatibility routing moves into `ui.rs`.

`crates/jig/src/status.rs` will remain the status aggregate owner.

The unified source adapter will build typed status and recorder projections from a shared `LocalObservationEpoch`.

Status provider execution remains owned by `status.rs`, but the monolithic status collector will be split into provider observation and typed aggregate projection.

A recorder request refreshes `RepoContext` exactly once before local collection.

A status request refreshes `RepoContext` exactly once, runs providers first, then collects one fresh local epoch from that same request context.

This preserves the existing provider-before-local ordering so repository changes made during a slow provider run are not hidden behind a stale local observation.

Gate collection will use an explicitly named helper that accepts already-collected plan baselines and receipt indexes.

It will not invoke the current wrapper that refreshes authority and rescans local state.

### 8.3 Public source interface

The feature crate will expose one `DashboardSource` trait.

The trait will be `Send + Sync`.

The trait will expose type-matched `recorder`, `status`, and `plan` methods.

Every method will accept a cooperative cancellation callback.

The recorder request will carry the bounded visible timeline limit.

The plan request will carry a plan ID and the recorder epoch from which it was selected.

The status request carries only the current bounded timeline limit needed to project the returned fresh local epoch.

The scheduler, rather than the source trait, will own a `WorkerRequest` enum.

This makes mismatched request and response variants unrepresentable at the source boundary.

The initial interface is conceptually:

```rust
pub trait DashboardSource: Send + Sync {
    fn recorder(
        &self,
        request: RecorderRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RecorderRefresh, SourceError>;

    fn status(
        &self,
        request: StatusRequest,
        phase_changed: &dyn Fn(StatusPhase),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StatusRefresh, SourceError>;

    fn plan(
        &self,
        basis: PlanBasis,
        plan_id: String,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError>;
}

pub enum PlanBasis {
    RecorderEpoch(RecorderEpochId),
    Fresh,
}

pub struct RecorderEpochId(u64);

pub struct RecorderRequest {
    mode: RecorderMode,
    timeline_limit: usize,
}

pub enum RecorderMode {
    Refresh,
    ReuseCurrent,
}

pub struct RecorderRefresh {
    recorder: RecorderSnapshot,
    status_local: StatusLocalSnapshot,
}

pub struct StatusRefresh {
    status: StatusSnapshot,
    recorder: RecorderSnapshot,
}

pub struct StatusRequest {
    timeline_limit: usize,
}

pub enum StatusPhase {
    Providers,
    LocalEpoch,
}

pub enum SourceError {
    Cancelled,
    Collection { domain: Domain, message: String },
    InternalContract { message: String },
}

pub enum PlanSnapshotResult {
    Found(PlanSnapshot),
    NotFound,
    StaleRecorderEpoch,
}
```

Exact private helper names may change during implementation.

The type-matched source boundary and typed cancellation result may not change without updating this plan's decision log.

The source adapter retains one immutable in-process local observation epoch behind a short-held mutex.

The mutex is never held during filesystem, Git, provider, or gate work.

`RecorderEpochId` is an opaque process-local `u64` starting at one and allocated with checked monotonic increment for every publishable complete or partial local collection.

If checked increment would overflow, the request returns `SourceError::InternalContract`, retains the prior epoch, and publishes no replacement; IDs never wrap or repeat.

The retained `LocalObservationEpoch` contains root Git identity, state counts, the distinct-plan index, bounded recent session/plan/decision/receipt projections, typed loop observation, typed open-plan gate observations, and the observation timestamp.

It does not retain complete event streams.

A successful local collection replaces the cached epoch atomically.

Here, successful means the request returned a publishable complete or partial epoch rather than a fatal source error or cancellation.

If its plan observation is unavailable, the partial epoch installs for other sections but cannot prove plan absence.

A TUI plan-detail request names the epoch from which the operator selected the plan.

A one-shot JSON or manually refreshed closed-plan request uses `Fresh`, constructs a transient local epoch, and resolves detail from it without replacing the retained interactive recorder epoch.

The transient epoch still receives a monotonic ID so `basis_epoch` remains meaningful.

If no epoch has been installed, or the requested ID differs from the retained ID, `RecorderEpoch` returns `StaleRecorderEpoch` without reading body or receipts.

If a successful refresh replaces epoch E with E+1, queued and open detail are reconciled by raw plan ID.

If the plan remains in E+1, the runtime retargets the queued detail to E+1.

If an already-running detail returns stale, the runtime retries at most once against the newest accepted epoch.

If the plan is absent from E+1, detail closes with a visible removal notice.

That removal behavior applies only when E+1 contains a successfully decoded plan index.

When E+1 has a plan-section error, existing detail remains visibly stale and a new detail request returns `SourceError::Collection { domain: state.plans, ... }`.

A failed local refresh retains the prior epoch and does not invalidate details based on it.

### 8.4 Why type-matched source methods

Type-matched methods prevent request-response mismatches at compile time.

The runtime still uses one private request enum and one worker abstraction.

It makes background work explicit in tests.

It prevents a generic closure from smuggling mutation into the TUI.

It leaves each request independently cancellable and distinguishes cancellation from collection failure.

### 8.5 Typed status aggregate and external provider boundary

`StatusSnapshot` is one shared Rust DTO consumed directly by the dashboard and serialized by `jig status --json`.

The existing private producer structs and the TUI's duplicate aggregate wire structs will converge on this DTO.

Third-party provider JSON is decoded once into `jig_contract::status_provider::v1::Report`.

That external decoder remains additive-field tolerant and keeps schema version 1 compatibility.

Each accepted provider result is stored as `AcceptedProviderReport { decoded: Report, raw: serde_json::Value }`.

Its custom status serializer emits only `raw` at the existing `report` field; `decoded` is never serialized as a second copy.

The dashboard reads `decoded`; `jig status --json` serializes `raw` in the same location as today so accepted unknown provider properties and extension values survive.

This nested raw value is allowed only at the external provider boundary.

No in-process path serializes and reparses the full status aggregate merely to render it.

For local records within the new 1 MiB safety budget, the migration must differential-test the old and new status producers during coexistence and prove semantic JSON equality, including preservation of accepted unknown provider fields and extensions.

Oversized local records are the single documented differential exception: the new producer emits scoped partial errors and bounded remaining data rather than allocating the legacy record.

`StatusLocalSnapshot` contains `epoch_id`, `observed_at_ms`, typed repository observation, typed work summary and gates, typed loops, and local scoped errors.

`StatusProviderSnapshot` contains `observed_at_ms`, accepted raw-plus-decoded reports, input-freshness projections, durations, and provider scoped errors.

The application model stores these partitions separately even though the custom status-v1 serializer emits the existing flat aggregate.

Accepting `RecorderRefresh` replaces the entire local partition, preserves the complete provider partition—including prior input-freshness projections—and its provider observation timestamp, and recomputes overall `outcome` from both current error partitions.

Provider input freshness may require nested Git observations unavailable to the presentation model, so it remains visibly aged with the provider partition and is recomputed only by the next provider-first `StatusRefresh`.

It never retains a local error from an older epoch beside newer local data.

### 8.6 Why recorder data becomes typed

Recorder producer and consumer ship in the same workspace release.

They are not a third-party compatibility boundary.

Typed data catches renamed local fields at compile time.

Serialization remains derived for CLI JSON output.

The TUI will not deserialize its own recorder data.

### 8.7 Typed loop observation

The loop producer will expose a typed read model.

That read model will serialize to the existing CLI JSON shape.

The field names will be `workflow_id` and `item_key`.

Each exhausted-attempt view will carry a producer-owned, shell-safe recovery command derived from validated workflow and item arguments.

The producer retains argv boundaries until the common shell-display formatter renders the copyable text.

The TUI will display that command but never execute it.

This recovery command is a deliberate cutover enhancement, not existing browser parity.

The recorder snapshot will carry the same typed loop observation.

The typed status snapshot serializes that observation into its existing `loops` JSON field.

Its status-v1 serializer omits the new dashboard-only remediation object so semantic compatibility remains exact.

This removes the current duplicate field-name declaration.

The existing open loop-field beads will be resolved by Task B2 rather than patched in deleted web code.

### 8.8 Typed gate observation

Open-plan gate evaluation already has structured internal data before JSON projection.

Task B2 will make the existing private gate report types available to the `jig` adapter as typed producer data.

The adapter will exhaustively convert those internal producer types into the public dashboard `GateObservation` DTO owned by `jig-ui`.

The DTO carries gate ID, tool, skill, requiredness, status, freshness, end timestamp, diff summary, bounded changed and matching paths with truncation flags, bounded findings with truncation flags, and optional remediation command parts.

The producer-to-view conversion is compile-time checked; it does not round-trip through `Value`.

Do not deserialize a `Value` produced by the same binary merely to render it.

The status JSON shape and existing field encodings must remain semantically JSON-equal under pre-cutover differential fixtures for supported-size local records.

The typed dashboard observation adds an optional remediation command only to the new recorder schema and interactive model, not to status schema version 1.

The command must be built by the gate owner from canonical selectors and a validated plan ID.

It must be represented as argv until the existing shell-display formatter produces human text.

Authored gate IDs must never be concatenated into a copyable shell command without quoting.

The TUI must not reconstruct CLI syntax from display strings.

Unsupported gates or ambiguous selectors may omit the command and retain their diagnostic reason.

## 9. Information architecture

### 9.1 Top-level tabs

The unified dashboard will expose six top-level tabs.

Tab 1 is `Status`.

Tab 2 is `Packages`.

Tab 3 is `Blockers`.

Tab 4 is `Work`.

Tab 5 is `Timeline`.

Tab 6 is `Health`.

The first three preserve the existing status TUI's numeric positions.

The last three consume local recorder data.

The status data may also contribute repository facts to the shared header after it has loaded.

### 9.2 Status tab

The Status tab preserves the current Overview behavior.

It is the default for `jig status --tui`.

It shows provider progress.

It shows repository state.

It shows work and loop counts.

It shows input freshness.

It shows diagnostics.

It shows partial collection errors.

Provider switching remains `[` and `]`.

### 9.3 Packages tab

The Packages tab preserves current behavior.

`b` toggles blocked-only filtering.

Enter opens full package detail.

Package detail remains scrollable.

Provider switching remains `[` and `]`.

### 9.4 Blockers tab

The Blockers tab preserves current behavior.

It shows a flattened provider-specific queue.

It preserves stable blocker selection.

It shows blocker detail in a secondary pane.

### 9.5 Work tab

The Work tab is the default for `jig ui`.

It contains an open-plan list.

The selected plan has a gate preview.

The lower pane contains recently completed plans.

Enter opens full plan detail.

Selection is preserved by plan ID.

Open plans sort by opened time descending.

Closed history sorts by closed time descending.

The view clearly distinguishes open and closed rows.

### 9.6 Timeline tab

The Timeline tab contains the merged activity list.

Rows sort newest-first.

The active kind filter is visible in the title.

`f` cycles filters forward.

`F` cycles filters backward.

Number keys remain reserved for top-level tabs.

Enter opens a relevant detail when available.

For a plan-associated row, Enter opens the plan.

For a receipt row without a plan, Enter opens receipt detail.

For a decision row without a plan, Enter opens decision detail.

For a session row, Enter opens a bounded event detail.

Failure-only filtering is a predicate over receipt exit status.

The filter is applied to an already bounded recorder snapshot.

### 9.7 Health tab

The Health tab groups recent failures, check health, and loops.

The initial implementation uses one navigable list with recent-failure, per-tool aggregate, and loop/attention section headers.

There is no inactive pane-focus mode and Tab retains its global tab-navigation meaning.

Enter opens the selected failure or loop-attention detail.

The implementation may split panes only if the deterministic renderer tests remain readable at 80 columns.

### 9.8 Detail overlays

Details have one base detail plus at most one leaf detail.

A plan detail is the base detail.

A receipt or decision detail opened from it is the leaf.

Closing the leaf returns to the same plan section, scroll offset, and selected row.

Opening an unrelated base detail explicitly replaces the existing base and its leaf.

Escape closes detail before it quits the application.

Backspace closes detail.

Enter closes non-navigable details.

Plan detail may contain internal sections.

Tab cycles plan-detail sections.

Plan-detail sections are Summary, Body, Gates, Decisions, and Receipts.

Receipt selection exists only in the Receipts section.

Enter on a receipt opens receipt detail.

Scroll offsets are maintained per detail section.

Refresh preserves the open plan by plan ID.

If the plan disappears, the detail closes with a visible notice.

If the plan refresh fails, the last detail remains with an error banner.

### 9.9 Header

The header shows repository name.

It shows branch or detached revision.

The canonical `jig ui` path obtains root checkout identity from the shared local epoch.

It does not start status providers merely to populate the header.

It shows runtime and contract versions.

It shows local observation age.

It shows status observation age when status has loaded.

It shows per-domain refresh activity.

It uses text labels in addition to color.

### 9.10 Footer

The footer shows only bindings valid in the current mode.

It always includes quit.

It includes refresh for the active domain.

It includes navigation keys where rows exist.

It includes detail-close keys when a detail is open.

It includes provider switching only on provider tabs.

It includes filter state only on filterable tabs.

This prevents a permanently dense help bar.

### 9.11 Terminal size tiers

The target comfortable size is 108 columns by 24 rows or larger.

Wide mode uses split tables and previews.

Standard mode begins at 72 columns by 20 rows.

Standard mode preserves the current status TUI's effective minimum while fitting every top-level list.

Compact mode begins at 40 columns by 12 rows.

Compact mode shows a one-line header, one-line tabs, one primary pane, and a contextual footer.

Compact mode hides optional columns and inline previews.

Every hidden detail remains reachable through Enter and scrolling.

Micro mode covers smaller nonzero terminals.

Micro mode shows the current domain's loading, error, or selected-item summary when space permits.

It always shows actual terminal dimensions and `q` or resize guidance when at least one line is available.

At narrow widths, show stable IDs before titles.

At short heights, keep the active list and footer before secondary context.

Do not panic on zero-sized areas.

Do not underflow layout constraints.

Only wide and standard layouts promise simultaneous summary and preview.

Compact layout promises reachability, not simultaneous visibility.

## 10. Keymap

### 10.1 Global keys

`q` quits.

Ctrl-C quits.

Escape closes detail, then quits from the top level.

`r` refreshes the active domain.

`R` refreshes both domains.

Tab selects the next top-level tab when no detail is open.

BackTab selects the previous top-level tab when no detail is open.

`1` through `6` select tabs directly.

Left and Right may also change tabs when focus is not inside a horizontal detail control.

### 10.2 List keys

Up and `k` move up.

Down and `j` move down.

PageUp moves by one visible page.

PageDown moves by one visible page.

Home selects the first row.

End selects the last row.

Enter opens available detail.

### 10.3 Domain keys

`[` and `]` switch status providers.

`b` toggles blocked-only packages.

`f` and `F` cycle timeline filters.

No unmodified key performs a mutation.

### 10.4 Key handling invariants

Release-only events are ignored.

Repeat events remain actionable.

Keys unrecognized in the current context are ignored.

An ignored key does not force a redraw.

This corrects the current status runtime's catch-all redraw behavior.

## 11. Snapshot model

### 11.1 Recorder snapshot

The recorder snapshot carries repository identity.

It carries runtime identity.

It carries current session ID.

It carries state counts.

It carries open-plan summaries.

It carries closed-plan history.

It carries recent failures.

It carries tool statistics.

It carries typed loop observation.

It carries a bounded mixed timeline.

It carries collection errors per subsection.

It carries generated time.

It carries an opaque recorder epoch ID.

### 11.2 Plan snapshot

The plan snapshot carries the plan summary.

It carries bounded body text.

It carries body error independently.

It carries typed gates.

It carries gate error independently.

It carries bounded decisions.

It carries bounded receipts.

It carries the applied limits.

It carries omission counts where available.

It carries `basis_epoch` for the epoch used to resolve plan metadata and open-plan gates.

It carries `detail_observed_at_ms` for the later body and reverse-receipt observations.

It carries `gates_observed_at_ms` because open-plan gates use the basis epoch while closed-plan gates are evaluated during live detail collection.

It carries `decisions_observed_at_ms` because decisions are collected by a live per-plan scan rather than retained proportionally in the epoch.

Plan detail is intentionally a multi-time observation with explicit basis, decision, body/receipt, and gate timestamps, not a point-in-time snapshot.

Each section carries independent error and truncation metadata.

### 11.3 Error representation

Optional subsections use data-plus-error fields.

Internally, independently fallible local sections use `Observation<T> { data: Option<T>, error: Option<SnapshotError> }`.

`SnapshotError` contains stable `scope` and `code` strings plus an exact unsanitized `message`; terminal preparation sanitizes only the displayed copy.

The fatal-versus-partial policy is:

| Input or boundary | Recorder/status behavior | Plan-detail behavior |
|---|---|---|
| Repository discovery or configuration refresh | Fatal `SourceError::Collection`; no source result is published | Fatal `SourceError::Collection` |
| Root Git observation | Publish repository identity with a scoped partial error when the checkout remains identifiable | Reuse basis metadata |
| Missing state file | Treat as empty without creating it | Treat optional section as empty |
| Corrupt session stream | Publish other sections and a `state.sessions` error | Not used for detail |
| Corrupt plan stream | Publish other sections and a `state.plans` error; plan/gate views unavailable | Fatal when no trusted plan membership can be resolved |
| Corrupt decision stream | Publish other sections and a `state.decisions` error | Publish plan metadata with decisions error |
| Corrupt receipt stream | Publish other sections and a `state.receipts` error | Publish metadata/body/decisions with receipts and live-gates errors |
| Loop configuration/cache failure | Publish other sections and `loops` error | Not used for detail |
| One gate evaluation failure | Publish plan with scoped gates error | Publish other detail sections with gates error |
| One provider failure | Publish repository/local data and other providers with `outcome: "partial"` | Not used for detail |
| Every provider failure | Publish repository/local data and provider errors with `outcome: "partial"` | Not used for detail |
| Plan-body failure | Not loaded in summary | Publish other detail sections with body error |
| Cooperative cancellation | Return `SourceError::Cancelled`; publish nothing from that request | Return `SourceError::Cancelled`; retain prior detail |

No fallible all-or-nothing epoch constructor may convert a work, loop, gate, or provider subsection failure into total status loss.

One loop decode problem cannot erase plans and timeline.

One gate failure cannot erase the plan list.

One unreadable plan body cannot erase receipts.

Errors are sanitized only at the terminal boundary.

Exact JSON output retains exact diagnostic text.

### 11.4 Timeline identity

Receipt rows use receipt ID.

Decision rows use decision ID.

Plan rows use the plan-event record ID if available.

Session rows use the session-event record ID if available.

The current projection drops plan and session event record IDs.

The migration will preserve them in `StateStreams`.

This makes selection stable when equal-timestamp events are inserted.

Current session and plan records contain IDs.

If a legacy record is decoded without a usable ID, the cancellable JSONL scanner supplies its stable starting byte offset.

The fallback identity is stream kind, starting byte offset, and a stable digest of the raw record bytes.

Appending preserves existing offsets; rewriting a state stream creates a new recorder epoch and may deliberately reset selection.

The digest prevents selection from attaching to an unrelated replacement record written at the same offset.

Every view model stores raw identity separately from sanitized display text.

Selection, equality, and request routing use only raw identity.

Sanitization may collapse two hostile display strings to the same visible text.

That visual collision must never merge their underlying rows.

### 11.5 Ordering

Timeline ordering is descending timestamp.

Equal timestamps sort by kind order and stable identity.

The kind order is documented in tests.

Open plans sort descending opened time, then plan ID.

History sorts descending closed time, then plan ID.

Failures sort descending ended time, then receipt ID.

Tool health sorts descending last-ended time, then tool name.

Deterministic tie-breaking prevents selection jitter.

### 11.6 Limits

The visible mixed timeline remains capped at 120 rows by default.

The maximum CLI-requested timeline limit remains 1000.

Every local epoch retains at most 1,000 newest candidates per timeline stream while computing full-stream counts through streaming reducers.

Changing the visible timeline limit within 1 through 1,000 does not rescan an already-loaded epoch.

Recorder and rendered open-plan summaries are capped at 1,000 with an omitted count.

The distinct-plan index is proportional to the number of plans because it is required to resolve arbitrary plan IDs and current status.

Compatibility also requires a transient full status-v1 work/gate projection and one raw provider document per configured provider; these proportional structures are listed explicitly in section 15.5.

Rendered and recorder gate rows are capped at 256 per plan with an omitted count.

Rendered and recorder tool-health rows are capped at 256 with an omitted count.

Loop workflow, live-lease, attempt, waiting-attempt, and exhausted-attempt view collections are each capped at 1,000 with omitted counts.

Loop cache input files are capped at 8 MiB before deserialization.

Dashboard and status local-epoch JSONL readers cap each logical record at the repository's existing oversized-record threshold of 1 MiB.

An oversized record stops growing its buffer, is skipped cancellably to the next newline in fixed chunks, and yields a scoped subsection error recommending `scripts/jig state diagnose` and the applicable documented compaction path.

Stable hashing and `serde_json` parsing receive at most one bounded record; parsing is not cooperatively interruptible inside Serde but is independently bounded by 1 MiB.

Closed plan history remains capped at 10.

Recent failures remain capped at 10.

Plan receipts remain capped at 50.

Plan body text remains capped at 20,000 characters.

Plan body input reads are capped at 80,004 bytes, enough to validate and safely truncate any 20,000-character UTF-8 prefix plus one complete four-byte scalar.

Failure preview remains capped at 400 characters.

Receipt output previews remain capped at 1,000 characters each.

Changed paths remain capped at 20.

Decision rationale in the timeline remains capped at 300 characters.

Plan-detail decision collections gain an explicit count cap of 100.

Decision text gains explicit visible truncation markers.

These constants will live next to snapshot construction.

Every new recorder JSON collection and every rendered dashboard collection exposes its applied limit and omitted count where the producer can compute it.

Existing `jig status --json` schema-v1 collection cardinality remains semantically unchanged; it is the explicit compatibility exception to new response limits.

Input streams may be traversed completely, but reducers do not retain complete event vectors.

## 12. Collection and refresh design

### 12.1 Serialized request scheduler

The runtime will own exactly one active worker slot.

The worker accepts local-recorder, status, or on-demand detail requests.

At most one collection request may run at a time.

Pending intent is coalesced by domain.

At most one local refresh may be queued.

At most one status refresh may be queued.

At most one detail request may be queued.

A newer detail request supersedes an older queued detail request.

Every pending request kind retains the sequence number from when that kind first became pending.

Replacing a queued detail changes its target but not its queue age.

When the worker becomes idle, the scheduler starts the pending request with the oldest sequence number.

Automatic refresh is enqueued only when that domain has no pending request.

This makes scheduling deterministic and prevents rapid selection changes from continually moving detail ahead of older work.

User-opened detail is foreground work.

Any explicit local foreground request—initial recorder-tab load, `r` on a recorder-domain tab, or user-opened plan detail—cooperatively preempts an active status request only while its announced phase is `StatusPhase::Providers`.

The runtime cancels and joins, preserves the last successful status snapshot, enqueues one replacement status refresh with its original queue age, then starts the explicit local request or newest detail target.

Once the worker announces `StatusPhase::LocalEpoch`, foreground requests wait for it because that completion supplies the newest local basis.

Foreground detail never preempts an active local refresh.

It waits for that refresh because its completion supplies the newest valid basis epoch.

Initial plan detail likewise waits for the first local epoch because no valid basis exists.

Automatic work never preempts explicit work.

A restarted request can be preempted again only by a newer explicit foreground action; tests must prove normal queued status work eventually resumes after foreground activity stops.

`cancel_and_join` discards the cancelled worker receiver result, and the cancelled generation is unconditionally ineligible for publication.

While the bounded synchronous join completes, the last rendered frame says that provider collection is being cancelled for the named foreground action, such as recorder refresh or plan detail.

The status source announces `Providers` before the first provider operation and `LocalEpoch` immediately before local collection.

Task E creates a dedicated phase channel captured by the worker closure because `CooperativeWorker` exposes only one final-result receiver.

Every phase event carries the worker request generation.

The event loop drains phase events before key handling on each tick, accepts only the active status generation, and ignores stale-generation phase events.

### 12.2 Why domain state remains independent

Status providers may consume configured timeouts of up to one hour.

Local state reads are normally faster.

An operator can navigate already-loaded data while any collection runs.

A newly requested plan detail preempts unrelated provider work through the explicit policy above.

A provider failure should not make the recorder appear failed.

Independent domain state preserves this behavior without multiple simultaneous workers.

Serializing source work removes cross-worker races and simplifies shutdown ordering.

### 12.3 Initial load

`jig ui` starts the local request immediately.

It does not start providers until a status-domain tab is selected.

`jig status --tui` starts the status request immediately.

On normal completion that status request also returns and publishes the fresh local recorder epoch it observed.

If the operator selects a recorder-domain tab before provider completion, the explicit local action preempts and requeues status, then collects local data promptly.

`jig ui --plan PLAN_ID` records a pending application intent, starts the local request, and queues no source detail until that response supplies a valid epoch.

The terminal enters before background collection.

The user sees loading state promptly.

### 12.4 Scheduled refresh

Local refresh defaults to ten seconds after the prior collection completes.

Status refresh defaults to thirty seconds after the prior collection completes.

Detail refresh runs when opened.

An open plan detail refreshes after a successful local refresh.

Detail refresh never overlaps itself.

Plan detail reuses plan metadata and open-plan gates from the requested recorder epoch.

It performs one cancellable forward decision traversal filtered to the selected plan, retaining the newest 100 decisions.

Open-plan detail then performs only body I/O and one bounded reverse receipt lookup.

Closed-plan gates may require one cancellable gate evaluation because the recorder epoch batches only open plans.

For closed plans, one forward receipt traversal simultaneously builds the gate receipt index and retains the newest 50 plan receipts; it does not perform a gate scan followed by a reverse scan.

Completed-plan detail does not auto-refresh unless manually requested.

Intervals are completion-relative.

Slow collection therefore cannot accumulate a backlog.

### 12.5 Manual refresh

`r` queues the active domain when its worker is busy.

Only one queued refresh bit exists per domain.

Repeated keys do not build an unbounded queue.

`R` queues an explicit local refresh first and a status refresh second.

The later status request still performs its required provider-first/local-second observation, so its local epoch supersedes the prompt local result when it completes.

If status has never been requested, `R` deliberately queues it.

The footer labels this behavior.

While open-plan detail is visible, `r` queues a local refresh followed by detail against the accepted new epoch.

While closed-plan detail is visible, `r` queues `PlanBasis::Fresh`.

A leaf detail refreshes its owning base domain and does not discard the base selection.

`R` queues both primary domains without implicitly closing detail.

### 12.6 Cancellation

Every source request receives a cancellation callback.

Cancellation is represented only as `SourceError::Cancelled` and is never inferred from a diagnostic string.

State JSONL scans will poll it before open, between records, and after every bounded read chunk.

When a logical line exceeds 1 MiB, scans discard further bytes in 16 KiB chunks while polling cancellation until newline or EOF rather than allocating proportionally.

Receipt reverse scans will poll it between windows of at most 16 KiB.

They will replace blocking shared-lock acquisition with `try_lock_shared` retry loops.

Lock retries poll cancellation and wait at most 10 milliseconds between attempts.

Stable unlocked-snapshot fallback will use a chunked cancellable reader rather than whole-file `fs::read`.

It will poll before and after each 16 KiB chunk and before each stability comparison.

Gate evaluation will use existing cancellable APIs.

Loop observation will use existing cancellable APIs.

Status provider processes will use existing owned-process cancellation.

Loop-cache reads poll between their existing 64 KiB chunks and gain the 8 MiB input cap.

Plan-body reads poll before every component open and each read chunk.

Except for kernel time spent in a single regular-file operation, a worker performs no more than one 16 KiB state/body read, one 64 KiB loop-cache read, or one 10 millisecond lock wait between cancellation checks.

Quit cancels the active worker.

Quit discards pending work.

Quit joins the active worker.

Terminal restoration begins only after the active worker has joined.

On Unix, the CLI layer extracts the existing `DoctorSignalSession` behavior into a CLI-local reusable signal module.

The policy does not move into `jig-tui` because signal retirement and redelivery belong to process supervision, not terminal presentation mechanics.

The dashboard runtime accepts an external cancellation observation in addition to keyboard input.

SIGINT and SIGTERM request cooperative cancellation rather than bypassing Rust drop.

On external cancellation the runtime stops scheduling, discards pending requests, cancels and joins the worker, restores the terminal by dropping `TerminalSession`, and returns to the adapter.

Only then may the adapter retire or redeliver the original terminating signal according to the shared signal contract.

A panic still restores the terminal through `TerminalSession` drop.

Worker drop remains join-on-drop.

`CooperativeWorker` has no safe forced-detach escape hatch.

Therefore every I/O path reachable by a dashboard worker must be cancellable or independently bounded before the worker integration lands.

### 12.7 Refresh result reconciliation

A successful snapshot replaces data for its domain.

An accepted `StatusRefresh` publishes both its typed status aggregate and the recorder projection from the same fresh local epoch, clears a redundant pending automatic local refresh, and resets both completion-relative timers.

Its `recorder.timeline_limit` exactly matches the `StatusRequest` value.

If the operator changes the visible limit while status is in flight, the runtime accepts and retains the new epoch, immediately issues `RecorderMode::ReuseCurrent` at the current limit, and does not replace visible timeline rows with the stale-limit projection.

A manual local refresh queued while status was active remains pending because operator intent may postdate the status request's local observation boundary.

An accepted `RecorderRefresh` publishes recorder data and reprojects the local repository, work, and loop portions of an already-loaded status model while retaining its independently timestamped provider observations.

The header shows those local and provider timestamps separately.

Every request carries a monotonically increasing request generation.

Every completion echoes that generation.

The model accepts a completion only when its generation is still current for the selected domain and raw identity.

Late superseded detail results are discarded.

When an accepted local completion replaces the basis epoch, queued detail naming the old epoch is discarded and retargeted by raw plan ID if the plan remains present.

An already-running stale detail gets at most one transparent retry against the newest accepted epoch.

Unknown epochs and plans removed from the newest epoch do not retry.

It clears that domain's transient error.

It preserves stable selections.

A failed snapshot preserves prior data.

A failed detail preserves prior detail visibly labeled with its prior `basis_epoch` and observation time.

It records a sanitized error for rendering.

It schedules the next automatic attempt from completion time.

A cancelled snapshot during quit does not flash an error.

An internal source invariant failure uses `SourceError::InternalContract` for that domain.

### 12.8 Context refresh

The source adapter refreshes repository context once per new local-epoch collection.

Every status refresh runs providers first and then collects a fresh local epoch; there is no provider-only refresh over an old local epoch.

`RecorderMode::ReuseCurrent` performs no filesystem, Git, gate, loop, or provider work and only reprojects the retained epoch at the requested timeline limit.

If no retained epoch exists, `ReuseCurrent` returns `SourceError::InternalContract`; the runtime never issues it before accepting a refresh.

Recorder and status source methods reject timeline limits outside 1 through 1,000 as `SourceError::InternalContract` rather than silently clamping them.

This preserves observation of changed repository metadata without cascading duplicate authority and configuration loads.

The refreshed context remains local to the epoch builder and its immediately associated provider request.

The TUI crate never receives `RepoContext`.

## 13. CLI design

### 13.1 Canonical command

The canonical interactive command is:

```sh
scripts/jig ui
```

It requires terminal stdin and stdout.

When redirected, it returns actionable guidance.

The guidance offers `jig ui --json` for recorder data.

The guidance offers `jig status --json` for provider data.

### 13.2 Initial plan selection

The focused form is:

```sh
scripts/jig ui --plan plan_01EXAMPLE
```

Plan-ID syntax is validated before entering the alternate screen.

Existence is resolved by the initial cancellable local epoch rather than a duplicate preflight state scan.

A missing plan becomes a visible startup error inside the interactive dashboard.

JSON mode resolves it before emitting success and uses the standard structured failure path.

### 13.3 Refresh options

The canonical command accepts:

```sh
scripts/jig ui --refresh-seconds 10
scripts/jig ui --status-refresh-seconds 30
```

Both accept integers from 1 through 3600.

The local flag defaults to 10.

The status flag defaults to 30.

The canonical command also accepts:

```sh
scripts/jig ui --timeline-limit 120
```

The value accepts integers from 1 through 1000.

It controls the initially visible mixed timeline; the epoch retains the fixed 1,000-candidate ceiling so interactive growth does not rescan.

`+` grows the visible limit through fixed bounded steps by issuing `RecorderMode::ReuseCurrent` against the epoch's retained 1,000-row candidate ceiling.

`-` shrinks the visible limit model-locally without a source call.

The active limit remains visible on the Timeline tab.

### 13.4 JSON output

Global `--json` changes `jig ui` from interactive mode to one-shot output.

The command does not enter raw mode.

It does not start a server.

It does not emit a URL.

On Unix it uses the same bounded signal-session cancellation policy as other long-running observational commands.

Without `--plan`, it emits `RecorderSnapshot`.

With `--plan`, it emits `PlanSnapshot`.

Both successful documents have this exact common envelope prefix:

```json
{
  "ok": true,
  "command": "ui",
  "schema_version": 1,
  "snapshot_kind": "recorder"
}
```

Plan output uses `"snapshot_kind": "plan"`.

Task A checks in full golden fixtures defining names, nullability, omission rules, limit metadata, and subsection errors.

Schema 1 is a supported machine-readable runtime contract beginning with the cutover release.

Future breaking changes require a new schema version, but do not change the generated harness contract unless launcher or generated-data behavior also changes.

#### 13.4.1 Common JSON rules

Schema-1 objects use snake-case field names.

All declared top-level fields are present.

Optional scalar or object fields serialize as JSON `null`; they are not omitted.

Collections serialize as arrays or objects, including when empty.

Errors serialize as `{ "scope": string, "code": string, "subject_id": string-or-null, "message": string }` and retain raw diagnostic text in JSON.

Snapshot-global bounded collections have a root `limits` entry `{ "applied": integer, "omitted": integer-or-null }`; `null` means the producer cannot safely compute an omitted count.

The only root limit keys are `open_plans`, `history`, `failures`, `tool_stats`, `timeline`, `plan_decisions`, and `plan_receipts`, as applicable to the snapshot kind.

Nested bounded collections use `BoundedRows<T> { "items": [T], "applied": integer, "omitted": integer-or-null }` adjacent to their owner.

Nested bounded text uses `BoundedText { "text": string, "applied_chars": integer, "omitted_chars": integer-or-null }`.

Raw stable identities are serialized; terminal-sanitized display copies are never serialized.

The authoritative recorder/detail limit identifiers and ceilings are:

| Limit key | Applied ceiling |
|---|---:|
| `open_plans` | 1,000 rows |
| `history` | 10 rows |
| `failures` | 10 rows |
| `failure_stderr_chars` | 400 characters |
| `tool_stats` | 256 rows |
| `loop_workflows` | 1,000 rows |
| `loop_leases` | 1,000 rows |
| `loop_attempts` | 1,000 rows |
| `loop_scheduled_occurrences` | 1,000 rows per occurrence collection |
| `loop_waiting_attempts` | 1,000 rows |
| `loop_exhausted_attempts` | 1,000 rows |
| `timeline` | requested 1 through 1,000; default 120 |
| `timeline_decision_rationale_chars` | 300 characters |
| `gate_rows` | 256 rows per plan |
| `gate_changed_paths` | 100 rows per gate |
| `gate_matching_paths` | 100 rows per gate |
| `gate_findings` | 100 rows per gate |
| `plan_body_chars` | 20,000 characters |
| `plan_body_input_bytes` | 80,004 bytes read |
| `plan_decisions` | 100 rows |
| `plan_receipts` | 50 rows |
| `receipt_changed_paths` | 20 rows per receipt |
| `receipt_stdout_chars` | 1,000 characters per receipt |
| `receipt_stderr_chars` | 1,000 characters per receipt |

Recorder output includes the recorder-applicable subset; plan output includes the detail-applicable subset.

The table includes both root and nested ceilings; it does not imply that nested bounds are flattened into the root map.

`plan_body_input_bytes` is internal safety metadata and is not serialized as a root limit or `BoundedText` field.

Loop `workflows`, `leases`, `attempts`, `waiting_attempts`, and `needs_attention.exhausted_attempts` are `BoundedRows`.

`GatesView.gates`, `Gate.changed_paths`, `Gate.matching_paths`, `Gate.findings`, and `Receipt.changed_paths` are `BoundedRows`.

`Failure.stderr_preview`, timeline decision `rationale`, plan `body`, detail decision `rationale`, and receipt `stdout_preview`/`stderr_preview` are `BoundedText`.

The authoritative new `jig ui` error scopes are `repository`, `state.sessions`, `state.plans`, `state.decisions`, `state.receipts`, `loops`, `gates`, and `body`.

The authoritative producer codes are `git_observation_failed`, `stream_open_failed`, `stream_read_failed`, `record_too_large`, `record_decode_failed`, `loop_observation_failed`, `gate_observation_failed`, `body_not_found`, `body_unsafe_path`, `body_unsafe_type`, `body_read_failed`, `body_invalid_utf8`, and `unsupported_platform`.

`subject_id` identifies a raw plan, gate, provider, or other row when the error is scoped to one subject; otherwise it is null.

Adding a new scope or code is additive within schema 1; removing or changing the meaning of one requires a new schema version.

#### 13.4.2 Recorder JSON fields

| Field | Type and meaning |
|---|---|
| `ok` | literal `true` |
| `command` | literal `"ui"` |
| `schema_version` | literal `1` |
| `snapshot_kind` | literal `"recorder"` |
| `generated_at_ms` | unsigned observation completion time |
| `epoch_id` | unsigned `RecorderEpochId` |
| `repo` | object described below |
| `harness` | object described below |
| `current_session_id` | string or null |
| `counts` | sessions, session events, plans, plan events, open plans, and decisions |
| `open_plans` | bounded `OpenPlan` array |
| `history` | bounded `PlanSummary` array |
| `failures` | bounded `Failure` array |
| `tool_stats` | bounded `ToolStat` array |
| `loops` | typed loop object or null |
| `timeline` | bounded tagged timeline array |
| `timeline_show` | literal filter token, `"all"` for one-shot default |
| `timeline_limit` | applied visible output limit |
| `limits` | map of every bounded collection to applied/omitted metadata |
| `errors` | scoped subsection-error array |

`repo` contains `name`, `default_branch`, `source_commit`, `source_path`, `branch`, and `detached`.

`harness` contains nullable legacy `jig_version`, `runtime_version`, and `contract_version`.

`OpenPlan` contains `plan_id`, `title`, nullable `body_path`, nullable `opened_at_ms`, nullable `baseline_ref`, nullable `baseline_oid`, nullable `baseline_error`, nullable typed `gates`, and nullable `gates_error`.

`PlanSummary` contains `plan_id`, `title`, `state`, nullable `opened_at_ms`, nullable `closed_at_ms`, nullable `resolution`, nullable `duration_ms`, nullable `baseline_ref`, nullable `baseline_oid`, and nullable `baseline_error`.

`Failure` contains `id`, `tool_name`, nullable `plan_id`, nullable `ended_at_ms`, `exit_status`, and `stderr_preview: BoundedText`.

`ToolStat` contains `tool`, `runs`, `failures`, `last_exit_status`, `last_ended_at_ms`, and `avg_duration_ms`.

The loop object contains bounded-row `workflows`, `leases`, `attempts`, `waiting_attempts`, and `needs_attention.exhausted_attempts`; recovery-capable exhausted attempts contain raw `workflow_id`, raw `item_key`, and a nullable remediation object.

A remediation object contains `argv` as an array of exact argument strings and `display` as the shell-display formatter result.

`gates` contains `overall` and `gates: BoundedRows<Gate>`.

Each gate contains `id`, nullable `tool`, nullable `skill`, `required`, `status`, nullable `freshness`, nullable `ended_at_ms`, nullable `diff_summary`, `changed_paths: BoundedRows<String>`, `matching_paths: BoundedRows<String>`, `findings: BoundedRows<Finding>`, and nullable remediation.

Timeline objects carry a `kind` discriminator and a `stable_identity` string.

Receipt timeline rows contain `timestamp_ms`, `id`, `tool_name`, nullable `invoked_command_key`, nullable `plan_id`, nullable `session_id`, `exit_status`, nullable `started_at_ms`, nullable `ended_at_ms`, nullable `duration_ms`, nullable `diff_summary`, nullable `changed_path_count`, and nullable `stderr_preview: BoundedText`.

Plan timeline rows contain `timestamp_ms`, raw event-record `id` or fallback identity, `event`, `plan_id`, nullable `title`, and nullable `resolution`.

Session timeline rows contain `timestamp_ms`, raw event-record `id` or fallback identity, `event`, `session_id`, and nullable `outcome`.

Decision timeline rows contain `timestamp_ms`, `id`, nullable `plan_id`, `title`, `selected_option`, and `rationale: BoundedText`.

#### 13.4.3 Plan JSON fields

| Field | Type and meaning |
|---|---|
| `ok` | literal `true` |
| `command` | literal `"ui"` |
| `schema_version` | literal `1` |
| `snapshot_kind` | literal `"plan"` |
| `generated_at_ms` | unsigned completion time |
| `basis_epoch` | unsigned `RecorderEpochId` |
| `detail_observed_at_ms` | unsigned live detail observation time |
| `gates_observed_at_ms` | unsigned gate observation time |
| `decisions_observed_at_ms` | unsigned per-plan decision observation time |
| `plan` | `PlanSummary` object |
| `body` | `BoundedText` or null |
| `gates` | typed gates object or null |
| `decisions` | bounded `Decision` array |
| `receipts` | bounded `Receipt` array |
| `limits` | applied/omitted metadata map |
| `errors` | scoped subsection-error array |

`Decision` contains `id`, nullable `session_id`, nullable `plan_id`, `timestamp_ms`, `title`, `selected_option`, `alternatives`, and `rationale: BoundedText`.

`Receipt` contains `timestamp_ms`, `id`, `tool_name`, nullable `invoked_command_key`, nullable `plan_id`, nullable `session_id`, `exit_status`, nullable `started_at_ms`, nullable `ended_at_ms`, nullable `duration_ms`, nullable `diff_summary`, `changed_paths: BoundedRows<String>`, `stdout_preview: BoundedText`, and `stderr_preview: BoundedText`.

A missing plan emits no success document.

`NotFound` is returned only after a successfully decoded plan index proves absence.

An unavailable plan index returns scoped collection failure and never closes or retargets prior detail as though absence were known.

It uses the repository's standard JSON failure envelope with `ok: false`, `command: "ui"`, `error.kind: "command_failed"`, and a diagnostic that names only the sanitized plan ID supplied through the validated CLI argument.

An unknown plan returns the standard structured command failure.

Refresh interval flags conflict with JSON mode.

`--timeline-limit` remains valid in JSON mode because it selects bounded output.

### 13.5 Status compatibility command

The following remains valid:

```sh
scripts/jig status --tui
```

It launches the same dashboard runtime.

It selects the Status tab.

It begins provider collection immediately.

Its `--refresh-seconds` continues to control provider refresh.

Its local recorder refresh uses the canonical ten-second default.

The existing `--json` conflict remains.

Status subjects remain incompatible with `--tui`.

### 13.6 Former port option

`UiOpts.port` becomes `UiOpts.retired_port: Option<u16>` for one release.

The option remains hidden from help.

When present, `post_parse_usage_error` returns a usage-class error before dispatch.

The error says the browser server was removed.

The error recommends `jig ui` for the TUI.

The error recommends `jig ui --json` for one-shot data.

The option is never ignored.

The hidden parser shim ships in the planned 0.3.0 cutover and is removed in 0.4.0.

### 13.7 Why `jig ui` remains the name

Generated launchers already classify `ui` as repository-scoped.

The command already means “open Jig's general operator interface.”

Keeping the name avoids a new root command.

It avoids launcher compatibility work.

It avoids teaching users a third dashboard spelling.

The term UI does not promise HTTP.

### 13.8 Why `status --tui` remains

It is documented and already used.

Removing it would create migration cost without reducing implementation surface.

Both commands route to one function.

The alias changes only initial application state and refresh defaults.

## 14. Terminal safety

### 14.1 Repository-controlled text

Plan titles are untrusted display text.

Plan bodies are untrusted display text.

Receipt output is untrusted display text.

Changed paths are untrusted display text.

Decision text is untrusted display text.

Provider fields are untrusted display text.

Loop IDs are untrusted display text.

Every such string passes through `jig_tui::sanitize_text` before becoming a Ratatui `Text`, `Line`, or `Span`.

Machine JSON retains original values.

### 14.2 Bidi and control handling

Control characters render as the replacement character.

Unsafe Unicode formatting characters render as the replacement character.

Newlines are preserved only where the view intentionally supports multiline text.

Tabs are expanded deterministically in multiline detail text.

Terminal escape sequences cannot reach Crossterm as raw writes.

### 14.3 Direct output

The dashboard does not need direct output for ordinary views.

It will not copy receipt output to the terminal outside Ratatui.

It will not launch a pager.

It will not launch an editor.

This keeps all untrusted output inside sanitized rendering.

### 14.4 Error leakage

The dashboard may show repository-local paths already exposed by existing CLI diagnostics.

It must not expose vault plaintext.

It must not add receipt recording.

It must not log plan bodies to stderr on render failures.

Panic messages in tests must use generic fixtures.

## 15. Performance and resource model

### 15.1 Existing costs

Recorder collection scans sessions and plan events.

It reads decisions.

It reads a bounded reverse window of receipts.

It evaluates gates for open plans.

It observes loop status.

Status collection runs configured providers.

It observes Git.

It scans state summary.

It evaluates open-plan gates.

It observes loop status.

### 15.2 Avoiding accidental duplication

The unified dashboard must not run full status collection on every ten-second recorder refresh.

One recorder refresh scans sessions once.

It scans plan events once.

It scans decisions once.

It traverses receipts once and retains bounded reducers for timeline candidates, failures, tool statistics, plan-linked detail, and gate inputs.

It builds the plan index, counts, history, failures, tool health, and timeline from that observation.

It evaluates all open-plan gates through the existing batched gate collector.

Gate collection cost must not scale through one independent setup pass per open plan.

The provider domain remains lazy for canonical `jig ui`.

Every status request runs providers first, then performs exactly one local epoch collection and projects both status and recorder results from it.

That returned epoch clears a redundant pending automatic local request; a manual local request queued during status remains pending.

A later `RecorderMode::ReuseCurrent` request reuses that epoch and its original timestamp; automatic and manual local refresh use `RecorderMode::Refresh`.

### 15.3 Shared local observation epoch

One request-scoped builder produces `LocalObservationEpoch` for both recorder and status projections.

Receipt counts, bounded recent rows, failures, tool statistics, plan-linked receipt indexes, and gate inputs are derived during one receipt traversal.

Plan baselines and receipt indexes are passed into gate evaluation; gate evaluation does not reopen plan or receipt streams.

One immutable in-process local epoch is retained for cross-domain projection and lazy detail consistency.

It is discarded when the dashboard source is dropped.

It is not written to disk.

Do not reuse data across observation timestamps without labeling it stale.

Do not share mutable snapshot state between workers.

Because the scheduler admits only one source worker, the epoch swap cannot race another active source request.

### 15.4 Render cost

The model precomputes filtered row references only when snapshots or filters change.

The render loop does not parse JSON.

The render loop does not sort full collections.

The render loop does not sanitize the same stable text every frame.

Prepared display strings live in view models.

Detail line construction is bounded.

### 15.5 Memory bounds

The new recorder response and dashboard-view constants in section 11.6 are hard retention bounds.

Streaming state traversal may visit every record to compute counts and current state.

It does not retain complete event vectors.

The documented proportional-memory compatibility structures are the distinct-plan index, the transient full status-v1 work/gate projection, and one bounded raw-plus-decoded report pair per configured provider.

Status provider documents remain subject to the existing 8 MiB stdout cap.

The typed status aggregate preserves existing schema-v1 collection cardinality rather than silently truncating machine output.

The implementation derives the full status projection transiently from prepared epoch inputs and does not retain a duplicate full copy inside the epoch.

Each raw provider report is individually bounded by the existing 8 MiB provider stdout ceiling and its typed decode is retained beside it; the number of configured providers remains proportional.

Provider extension detail remains subject to existing row and field bounds.

The application retains at most one successful snapshot per domain.

It retains at most one base plan detail and one leaf detail.

It retains at most one transient error per domain.

## 16. Data consistency

### 16.1 Snapshot semantics

A recorder refresh is a best-effort local epoch with one observation timestamp and independently fallible sections.

A status refresh deliberately runs providers first and local collection second, so it contains separately timestamped provider and local partitions and claims no atomic point in time.

Later recorder refreshes replace only the local partition; provider reports and freshness retain their earlier timestamp until the next status refresh.

The dashboard displays these ages separately rather than presenting mixed-age data as current together.

Open-plan detail combines metadata and gates from `basis_epoch` with live decisions at `decisions_observed_at_ms` and body plus reverse receipts at `detail_observed_at_ms`.

Closed-plan detail combines metadata from its transient `basis_epoch` with live decisions at `decisions_observed_at_ms` and body, gates, plus the newest 50 matching receipts from one forward traversal at `detail_observed_at_ms`; `gates_observed_at_ms` equals that live observation time.

Those timestamps remain separately visible and no combined point-in-time coherence is claimed.

### 16.2 Concurrent state mutation

Append-only streams may grow during scanning.

Existing state readers define safe behavior for that condition.

The migration must reuse those readers.

It must not open and parse raw JSONL independently in the TUI crate.

### 16.3 Plan disappearance

A plan cannot normally disappear from append-only state.

State restore or corruption can still change observable history.

The detail model handles missing data without panicking.

It reports that the selected plan is no longer available.

### 16.4 Partial state

Missing state files represent empty state.

Opening the dashboard on an uninitialized state directory creates nothing.

Read-only observation preserves file bytes and modes.

This invariant already has UI tests and must survive the move.

### 16.5 Plan-body path and read safety

Persisted plan IDs are repository-controlled state, not trusted filesystem paths.

The repository currently has only a private lease-filename validator.

The current general helper blindly joins a formatted plan ID below `.agent/plans`.

The current body loader reads the complete file before character truncation.

Task B1 extracts one pure canonical plan-ID filesystem-component validator and uses it for every plan-body path derivation in work commands and dashboard reads.

Accepted IDs contain 1 through 128 ASCII alphanumeric, underscore, or hyphen bytes.

Dashboard detail additionally requires plan membership in its selected epoch.

Separators, parent components, absolute paths, NUL, and alternate platform separators are rejected.

On supported Linux and macOS targets, the reader opens the repository root directory, then `.agent`, then `plans`, then the validated filename through component-by-component `openat` calls using `O_NOFOLLOW` and `O_CLOEXEC`.

Jig's product support is already limited to Linux and macOS.

Any future target must provide an equivalent descriptor-relative no-follow primitive or return a typed plan-local unsupported-platform error; it must never fall back to ordinary path open.

Directory components additionally require `O_DIRECTORY`.

The final file additionally uses `O_NONBLOCK`, preventing a FIFO from blocking before type verification.

The reader verifies the opened handle with `fstat` and accepts only a regular file.

It never performs a metadata-check-then-path-open sequence.

Symlinks and non-regular files are rejected atomically at the opened-handle boundary.

Because the canonical helper is shared by work commands, Task B1 also closes the corresponding mutation race.

Missing `.agent` and `plans` components are created descriptor-relatively with `mkdirat`, followed by no-follow directory open and verification from the already verified parent descriptor.

Work-command body creation and append do not call the current path-based `ensure_state_layout` before this secured traversal.

New plan bodies use descriptor-relative `O_CREAT | O_EXCL | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC` with the existing umask-compatible creation mode.

Body append uses descriptor-relative `O_WRONLY | O_APPEND | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC`, then verifies the opened handle is regular before writing.

Sidecar locks use descriptor-relative `O_RDWR | O_CREAT | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC`.

Every body and sidecar descriptor is verified as a regular file before locking or writing.

Legacy body-lock serialization uses the already verified body and sidecar descriptors rather than reopening by path.

The change preserves current lock ordering, append serialization, `sync_data` durability, and error context; it does not claim an atomic replacement that the current direct body creation lacks.

Tests cover symlinked `.agent`, `plans`, and final body components for create, append, and read paths.

They cover FIFO and device body and sidecar targets, including bounded completion with no FIFO peer.

They also cover a missing-directory create and adversarial replacement of each ancestor between `mkdirat` and verified open.

The reader uses chunks of at most 16 KiB.

It polls cancellation between chunks.

It reads at most 80,004 bytes and never allocates from untrusted file length.

For a file at or below 80,000 bytes, the complete bounded input must be valid UTF-8.

For a larger file, the reader validates the greatest complete UTF-8 prefix at or below 80,000 bytes, rejects invalid data before a trailing partial scalar, truncates at 20,000 characters, and marks the body truncated.

The result records whether the body was truncated.

Invalid, missing, unreadable, unsafe-type, or non-UTF-8 bodies remain plan-local errors.

Oversized valid bodies return the safe prefix plus explicit truncation metadata rather than failing the whole section.

They do not erase plan metadata, gates, decisions, or receipts.

## 17. Testing strategy

### 17.1 Pure recorder projection tests

Retain the uninitialized-repository no-write test.

Retain the read-only file-mode test.

Retain repository metadata refresh coverage.

Retain plans, gates, loops, and timeline joining coverage.

Retain passing-receipt stderr omission coverage.

Retain history, failure, and tool-stat coverage.

Retain timeline-kind filtering coverage at the model layer.

Retain non-UTF-8 plan body error coverage.

Retain old-plan receipt reverse-scan coverage.

The authoritative cancellable-boundary inventory is session JSONL open and scan, plan JSONL open and scan, decision JSONL open and scan, receipt JSONL forward scan, receipt reverse data-lock acquisition, reverse cache-lock acquisition, reverse window scan, stable-fallback chunk read and stability comparison, gate iteration, loop workflow observation, loop lease and attempt-cache chunks, plan-directory component opens, plan-body open, and plan-body chunks.

Add cancellation before and during every boundary in that inventory.

Primitive cases expect the existing internal typed cancellation sentinel; adapter tests expect its one-time mapping to `SourceError::Cancelled`, no partial success publication, and no user-visible refresh error on quit.

Add bounded plan-body read coverage.

Reject path traversal, separators, symlinks, FIFOs, devices, and non-UTF-8 body files without blocking.

Accept oversized valid bodies only as bounded, marked-truncated prefixes.

Add a replacement-race test that swaps the target after directory traversal and prove the opened handle is either the verified original regular file or a safe error, never the replacement special file.

Add cancellation during batched gate evaluation.

Add cancellation during loop observation.

Add deterministic equal-timestamp ordering coverage.

Add stable event identity coverage.

Prove an append preserves fallback identity for unchanged prefix records.

Prove a stream rewrite placing different raw bytes at the same offset does not preserve selection onto the replacement record.

Add omitted-count coverage for every bounded collection.

At 1,000 recorder-visible plans and at status cardinality above that limit, measure retained model counts and prove recorder/view caps apply without truncating status-v1 machine output or duplicating the full status projection in the epoch.

### 17.2 Producer-consumer contract tests

Construct a non-empty typed loop observation.

Serialize it through `loop status`.

Embed it through `status --json`.

Render it through the dashboard model.

Assert `workflow_id` and `item_key` survive every boundary.

Assert recovery-command rendering preserves argv boundaries for whitespace and shell metacharacters.

This test resolves the current exhausted-attempt drift.

Construct gate data from the real gate producer.

Project it into recorder and status outputs.

Load recorder and status projections from one local epoch and instrument state opens/traversals.

Assert each state file is traversed once and gate evaluation performs no hidden plan or receipt rescan.

Assert open-plan detail performs one forward decision scan, one bounded reverse receipt lookup, and no gate rescan.

Assert closed-plan detail performs one forward decision scan and one forward receipt traversal total for both gate inputs and the newest 50 matching receipts.

Assert required, status, freshness, command, timestamps, and summaries agree.

Assert an authored gate ID with shell metacharacters cannot alter the displayed argv structure.

Do not use independently hand-authored JSON fixtures as the only oracle.

While old and new status producers coexist, feed the same deterministic provider outputs and supported-size local state to both and assert semantic JSON equality.

Inject the same deterministic clock and fake provider-duration source into both producer paths so equality requires no timestamp, duration, or field stripping.

Include unknown provider root and nested properties plus extension-key collisions, proving the raw accepted document survives while the dashboard renders the decoded report.

Add a differential fixture containing a valid local record above 1 MiB and assert the old producer succeeds while the new producer returns the specifically documented `record_too_large` partial result without proportional allocation.

### 17.3 Application-model tests

Consume typed `StatusSnapshot` without an aggregate serialization round-trip.

Decode provider protocol version 1, ignore additive provider fields, and reject unsupported provider protocols at the external boundary.

Accept recorder snapshots without serialization round-trips.

Preserve tab selection across refresh.

Preserve plan selection by ID.

Preserve timeline selection by event ID.

Preserve provider selection by ID.

Preserve package selection by ID.

Preserve blocker selection by stable key.

Decode hostile provider, package, and blocker IDs without changing their raw identity.

Sanitize separate display forms and prove two raw IDs that render identically remain independently selectable and route distinct requests.

Preserve open plan detail after refresh.

Preserve selected receipt within plan detail.

Close detail when the selected plan disappears.

Retain last successful data after a refresh error.

Clear only the successful domain's error.

Queue at most one refresh per domain.

Apply timeline filters correctly.

Apply failure filter only to nonzero receipts.

Clamp all selections after collection shrinkage.

### 17.4 Keymap tests

Cover every global key.

Cover every tab-selection key.

Cover list movement.

Cover page movement based on visible height.

Cover detail open and close.

Cover nested plan receipt detail.

Cover timeline filters.

Cover provider switching.

Cover blocked-only filtering.

Cover ignored keys.

Cover resize redraw.

Cover release-event filtering through shared helpers.

Assert unrecognized keys do not trigger redraw.

### 17.5 Renderer tests

Render each tab at 120 by 36.

Render each tab at 80 by 24.

Render each tab's primary data path at 60 by 15.

Render the safe micro fallback below 40 by 12.

Prove compact-hidden preview data remains reachable through detail.

Render empty recorder state.

Render empty provider state.

Render partial errors.

Render refresh-in-progress labels.

Render stale data plus refresh failure.

Render open-plan gates.

Render closed history.

Render recent failures.

Render tool health.

Render loop attention with real producer fields.

Render every timeline kind.

Render plan body detail.

Render plan decisions.

Render plan receipts and changed paths.

Render provider packages.

Render provider blockers.

Render package extensions.

Assert text labels accompany color status.

Assert truncation markers appear.

Assert dangerous control and bidi characters do not survive rendered buffers.

### 17.6 Runtime tests

Use fake sources with controllable channels.

Prove navigation remains responsive during local collection.

Prove navigation remains responsive during provider collection.

Prove local, status, and detail collection never overlap.

Prove the maximum active source-call count is one.

Prove manual refresh coalesces.

Prove oldest-pending request order and detail-target replacement.

Prove user-opened detail cancels and joins active provider collection before detail starts.

Prove initial recorder-tab load and recorder-domain `r` cancel and join during `StatusPhase::Providers`, preserve and requeue status, and start their local request.

Prove the same three foreground actions wait rather than cancel during `StatusPhase::LocalEpoch`.

Prove the prior status snapshot remains visible while its replacement is queued.

Prove preempted status refresh eventually resumes after foreground activity stops.

Prove automatic work never preempts explicit work.

Prove queued detail is rebased after epoch replacement and an in-flight stale result receives at most one transparent retry.

Prove failed refresh retains the prior epoch, unknown epochs fail stale, removed plans close detail, and `Fresh` uses a transient epoch without replacing the retained one.

Prove automatic refresh does not duplicate a pending manual refresh.

Prove automatic refresh is completion-relative.

Prove entering a status tab lazily starts status collection.

Prove `jig status --tui` eagerly starts status collection.

Prove state changed while a provider is blocked appears in the provider-completing `StatusRefresh` local epoch.

Prove a later local refresh reprojects loaded status work and loop facts while preserving provider reports, input-freshness projections, and their older timestamp unchanged.

Prove a timeline-limit change during status collection accepts the epoch but publishes timeline rows only after `ReuseCurrent` uses the latest limit.

Prove quit cancels and joins the sole active worker.

Prove quit discards every queued request.

Prove a newer queued detail supersedes an older queued detail.

Prove late results are rejected by request identity.

Prove cancelled quit does not surface a false error.

Prove source panics become joined worker errors without leaking terminal state where supported by shared machinery.

### 17.7 PTY lifecycle tests

Retain the shared terminal panic-unwind regression.

Add one dashboard launch-and-quit PTY test.

Assert alternate screen is entered.

Assert raw mode is restored.

Assert cursor visibility is restored.

Assert bracketed paste is disabled if enabled.

Add one quit-during-provider-refresh PTY test with deterministic synchronization.

Add SIGINT and SIGTERM cases using the CLI signal-session contract already used by non-TUI `jig status`.

Assert the process exits within a bounded generous deadline.

Use a fake-source side channel to prove cancellation was observed and worker cleanup completed before the captured `LeaveAlternateScreen` sequence.

Add an end-to-end hostile-text PTY case covering plan body, receipt output, changed paths, provider errors, extension keys, and colliding sanitized IDs.

Decode the terminal stream or isolate payload markers and assert hostile payload bytes cannot create an OSC command or alter terminal state; ordinary Crossterm/Ratatui CSI output remains expected.

Avoid millisecond timing races.

### 17.8 CLI parsing tests

Parse bare `jig ui`.

Parse both refresh flags.

Parse `--timeline-limit` at both bounds.

Reject zero refresh values.

Reject values above 3600.

Parse `--plan`.

Parse global JSON before and after the subcommand where supported.

Reject refresh flags with JSON.

Parse hidden `--port`.

Assert hidden `--port` produces the migration diagnostic.

Assert help does not advertise `--port`.

Assert help describes terminal requirements.

Retain status TUI conflicts.

Assert `status --tui` routes to the unified startup mode.

### 17.9 JSON contract tests

Assert `jig ui --json` emits exactly one JSON document.

Assert it exits after collection.

Assert it does not bind a port.

Assert it does not run status providers.

Assert `jig ui --plan ID --json` emits plan detail.

Assert recorder and plan outputs match checked-in full schema-1 golden fixtures and use distinct `snapshot_kind` values.

Freeze status compatibility fixtures from the pre-cutover producer before replacement and differentially compare them to the typed serializer; the new serializer must not author its own sole oracle.

Assert missing plans use the standard JSON error envelope.

Assert `--json ui --port 0` emits one usage error object and exits with status 2.

Assert human `ui --port 0` fails before terminal setup.

Assert human interactive failures do not emit partial JSON.

### 17.10 Deletion tests

Assert the workspace contains one `jig-ui` package and no `jig-status-tui` package.

Assert no production code contains `TcpListener` for this feature.

Assert no docs recommend `jig ui --port` except the migration note.

Assert no docs describe cookie or browser setup as current behavior.

Assert release scripts publish `jig-ui` and do not publish `jig-status-tui`.

Use manifest inspection and `cargo metadata` to assert the dashboard has no web-only direct dependencies; do not infer direct ownership from `Cargo.lock`.

### 17.11 Regression tests for specialized TUIs

Run `cargo test -p jig-tui`.

Run `cargo test -p jig-codex-tui`.

Run `cargo test -p jig-vault-tui`.

The shared terminal foundation must remain behaviorally compatible.

No specialized keymap should change.

## 18. Documentation migration

### 18.1 README

Replace “status and flight recorder” split wording.

Describe one terminal operator dashboard.

Show `scripts/jig ui`.

Retain `scripts/jig status --tui` as compatibility syntax.

Remove the port example.

Describe JSON alternatives.

### 18.2 Developer UX

Rename “Flight Recorder UI” to “Terminal Dashboard.”

Document the six tabs.

Document the keymap.

Document independent refresh behavior.

Document plan detail.

Document failure and receipt detail.

Document lazy provider collection.

Document one-shot JSON.

Document the `--port` migration error.

### 18.3 Status provider documentation

Remove language that calls the TUI distinct from `jig ui`.

State that the unified dashboard consumes the typed aggregate while providers continue to use protocol version 1.

State that `status --tui` starts on the provider overview.

Retain provider protocol guarantees unchanged.

### 18.4 Public contract

State that interactive dashboard output is not machine-stable.

State that terminal stdin and stdout are required.

State that `jig ui --json` is one-shot recorder output.

State that `jig status --json` remains the provider aggregate.

Remove the loopback authentication description.

### 18.5 Repository intent and agent guides

Describe `jig-ui` as the terminal feature application.

Describe `jig-tui` as shared mechanics.

Point recorder projection changes to `crates/jig/src/ui/snapshot.rs`.

Point provider aggregate changes to `crates/jig/src/status.rs`.

Point dashboard navigation to the new crate.

Regenerate `agent-map.md`.

### 18.6 Changelog

Call out the browser UI removal as a breaking workflow change.

Call out the retained `jig ui` command.

Call out the `status --tui` compatibility entrypoint.

Call out one-shot JSON semantics.

Call out the hidden `--port` migration diagnostic.

## 19. Compatibility and rollout

### 19.1 Compatibility classification

The browser transport is a public CLI workflow.

The current `jig ui --json` output is also externally consumable.

The cutover is therefore observable and must be documented.

The internal snapshot Rust API is release-coupled and may change directly.

The published web-facing `jig-ui` Rust API, including `UiServer`, `SnapshotProvider`, and HTTP query types, reaches explicit end of support in the planned 0.3.0 cutover.

Future releases also stop publishing `jig-status-tui`; previously published crate versions remain available, but neither crate surface is a supported cross-version integration boundary.

The status provider v1 schema must not change.

The `.agent/state` schema must not change.

The 1 MiB dashboard/status local-reader record ceiling is a deliberate 0.3.0 safety tightening, not a state-format change.

Oversized legacy records remain diagnosable and repairable through state diagnosis and compaction, but the unified dashboard will not allocate them unboundedly.

Consequently `jig status --json` retains schema version 1 and supported-size semantics but may become partial for a valid oversized legacy record that the previous implementation attempted to allocate.

The 0.3.0 Breaking notes name this exception explicitly.

Repository policy requires a contract bump or an explicit end of support for a breaking CLI or JSON change.

This project chooses an explicit end of support for the browser transport and the old `jig ui --json` URL envelope across every otherwise supported repository epoch.

That choice is authorized by the product requirement to retire the web UI.

It will be called out under a Breaking heading in the release notes.

It does not require contract v8 because no generated repository data or launcher behavior changes.

### 19.2 Generated launchers

Existing launchers already pass `ui` and `status` as repository-scoped commands.

The canonical root command names remain unchanged.

Old launchers therefore work with the new runtime.

New launchers retain both names.

No compatibility epoch is required.

### 19.3 One-release diagnostic shim

The runtime accepts `--port` syntactically for one release.

It rejects it semantically with migration guidance.

This produces a better error than an unknown argument.

The shim is hidden from current help.

`post_parse_usage_error` produces the diagnostic so human mode receives Clap's usage status and JSON mode receives `error.kind = "usage"`.

Dispatch is not used because it would misclassify the failure as `command_failed`.

The shim ships in the planned 0.3.0 cutover release and is removed in 0.4.0.

The changelog records both versions.

### 19.4 JSON semantic cutover

Old `jig ui --json` printed a server URL and continued serving.

New `jig ui --json` prints recorder data and exits.

This cannot be made behaviorally compatible without retaining the server.

The direct cutover is intentional.

The previous URL-envelope schema is explicitly end-of-support rather than silently redefined as compatible.

The top-level output includes a schema version.

The first terminal-recorder JSON schema version is 1.

The command field becomes `ui`.

Plan output also declares schema version 1.

### 19.5 Recovery

Before the deletion commit, the web crate can be restored independently.

After the cutover, users needing historical browser behavior can run the prior Jig release.

No persisted state migration is involved.

Rolling back the binary restores the prior reader without changing state.

## 20. Delivery architecture

### 20.1 Epic: unified terminal dashboard

The epic owns the browser retirement and one-dashboard outcome.

It owns the cross-task dependency graph.

It does not own implementation progress detail.

Each substantial child task will maintain a task-local ExecPlan while active.

### 20.2 Task A: dashboard contracts and parity registry

Outcome:

Define the typed dashboard request, response, identity, error, limit, and startup-mode contracts.

Scope:

Add raw stable identities separate from sanitized display text.

Define recorder epoch and stale-detail semantics.

Define local, status, and plan-detail request variants.

Define `RecorderRequest`, `RecorderRefresh`, `StatusRefresh`, `StatusLocalSnapshot`, and dual raw/decoded `AcceptedProviderReport` contracts.

Define the nonblocking `StatusPhase::{Providers, LocalEpoch}` progress contract.

Define per-domain partial-failure behavior.

Define every collection and text limit.

Create focused scenario builders for status, recorder, plan detail, partial errors, hostile text, and collection limits.

Create a parity registry mapping every parity-matrix row to at least one named behavioral test.

Keep status JSON schema version 1 unchanged.

Keep current recorder JSON fields unless section 13 explicitly changes them.

Tests:

Serialization compatibility tests.

Identity collision tests.

Limit boundary tests.

Parity-registry completeness tests.

No single hand-authored fixture may serve as its own behavioral oracle.

Why first:

Runtime and presentation tasks need one agreed contract before they can proceed independently.

Depends on:

None.

Blocks:

Tasks B2, C, D, and E.

Recovery boundary:

Task A changes only additive types, builders, fixtures, and tests; revert them together before consumers merge if the contract is rejected.

References and acceptance oracle:

Sections 4.3–4.9, 5.6, 8.3–8.8, 11, 13.4, 14, and 17.2–17.3; `cargo test -p jig-ui` must prove schema goldens, raw-identity collisions, every limit boundary, and a one-to-one parity-registry-to-test mapping.

### 20.3 Task B1: cancellable readers and canonical plan-path hardening

Outcome:

Make every I/O primitive reachable from the dashboard cooperatively cancellable and read-only, and close unsafe plan-body path derivation for dashboard and existing work-command callers through one canonical validator.

Scope:

Add cancellable state-stream and plan-detail reads.

Add the 1 MiB logical-record budget and cancellable oversized-record skipping to dashboard/status local-epoch readers.

Add cancellable reverse receipt reads.

Add validated, confined, bounded, cancellable plan-body reads.

Replace path-based plan-body directory creation, body create/append, and sidecar-lock opens with the descriptor-relative mutation path in section 16.5 while preserving lock order and `sync_data`.

Add cancellable and size-bounded loop-cache reads.

Expose stable JSONL record offsets needed for legacy event identity.

Preserve or extract an internal typed cancellation sentinel without depending on dashboard presentation types.

Use the complete boundary inventory in section 17.1.

Tests:

Cancellation tests for every reachable I/O boundary.

Path-confinement and bounded-read tests.

No-write state tests.

Lock-contention and stable-fallback tests.

Synthetic-reader tests for a multi-hundred-megabyte logical line without proportional allocation, including cancellation during discard-to-newline.

Replacement-race tests.

Compatibility tests proving all generated and documented plan IDs remain accepted while traversal-like legacy IDs fail before filesystem access.

Why separate:

No worker integration may land before every joined I/O path has a testable cancellation contract.

Depends on:

None.

Blocks:

Task B2.

References:

Sections 8.3, 12.6, 16.5, 17.1, 21.3, and 24.9.

Recovery boundary:

Keep old non-dashboard readers callable until the new primitives pass equivalence and no-write tests; the validator cutover is isolated with compatibility fixtures and can be reverted without state migration.

Acceptance oracle:

Targeted state and work tests must pass every cancellation boundary, no-write read, 1 MiB record budget with bounded huge-line discard and cancellation, 8 MiB cache limit, canonical-ID compatibility case, and Linux/macOS descriptor-relative read/create/append/sidecar symlink, FIFO, device, no-peer, and replacement-race case.

### 20.4 Task B2: shared local epoch and typed producer projections

Outcome:

Build one trustworthy `LocalObservationEpoch` per successful local refresh and project recorder and status data without duplicate state scans.

Scope:

Consume Task A's `StatusSnapshot`, `RecorderSnapshot`, `PlanSnapshot`, identity, limit, and error contracts.

Consume Task B1's cancellable readers and safe plan-body primitive.

Map the internal cancellation sentinel to `SourceError::Cancelled` once at the dashboard adapter boundary without string matching.

Add stable raw event identities to timeline projections.

Stream session, plan, decision, and receipt reducers once per epoch.

Retain the distinct-plan index and only the bounded candidates listed in section 11.6.

Expose typed loop status with exact `workflow_id` and `item_key` fields.

Expose typed internal gate reports and exhaustively map them to dashboard `GateObservation`.

Pass collected plan baselines and receipt indexes to one batched open-plan gate evaluation without hidden rescans.

Project the same epoch into recorder and typed status aggregates.

Return `StatusRefresh` with both the status aggregate and recorder projection after provider-first, local-second collection.

Emit generation-tagged `StatusPhase::Providers` before provider work and `StatusPhase::LocalEpoch` before local collection.

Return `RecorderRefresh` with recorder and status-local projections so loaded status views can accept new local facts without rerunning providers.

Keep status JSON schema version 1 unchanged.

Implement monotonic epoch allocation, retained-epoch replacement, `Fresh`, stale, missing-plan, and explicit multi-time detail semantics.

Tests:

Producer-consumer contract tests.

Combined-domain one-scan instrumentation tests.

Source-level phase-ordering tests proving both announcements precede their corresponding operations.

Limit and omission-count tests.

No-per-plan-gate-setup tests.

Stale-epoch matrix covering older epoch after success, failed refresh retention, unknown epoch, removed plan, and `Fresh`.

Why separate:

This is the state and projection architecture; it depends on proven safe primitives but can be reviewed independently from terminal layout.

Depends on:

Tasks A and B1.

Blocks:

Task F.

References:

Sections 4.7, 8.2–8.8, 11, 12.7–12.8, 15.2–15.3, 16.1, 17.1–17.2, and 21.2–21.3.

Recovery boundary:

The epoch builder and projections land behind the unused dashboard source adapter; they can be reverted before Task F without changing persisted state or public command routing.

Acceptance oracle:

Targeted source tests must prove one traversal per state stream, no hidden gate rescan, provider-first/local-second status freshness, paired refresh publication, every limit/omission count, typed loop and gate contracts, and the complete stale/Fresh epoch matrix.

### 20.5 Task C: unified dashboard shell and status migration

Outcome:

Repurpose `jig-ui` as the unified terminal application and migrate the existing status TUI into it.

Scope:

Add terminal dependencies, modules, and the public dashboard entrypoint to `jig-ui` while the old web modules and dependencies still compile.

Copy the status model, renderer, runtime, and tests into focused `jig-ui` modules for coexistence.

Copy the status implementation and tests into `jig-ui` while retaining the compiling old status crate and web surface until Task F routes both commands to the new runtime.

Task G deletes the duplicated transitional source after cutover; Task C does not move files out from under the compatibility implementation.

Replace whole-value pre-deserialization sanitization with typed raw identities and separately sanitized display fields.

Preserve Status, Packages, and Blockers behavior.

Add six-tab navigation.

Add per-domain application state.

Add stable selection reconciliation.

Add contextual footer behavior.

Keep deterministic rendering.

Tests:

Existing status tests migrate without weaker assertions.

New navigation and small-terminal tests.

Why separate:

It preserves a working TUI while creating the host for recorder views.

Depends on:

Task A.

Blocks:

Tasks D and E.

Recovery boundary:

The migrated modules coexist with the compiling web and status crates; revert the new entrypoint and modules together while existing commands remain routed to old implementations.

References and acceptance oracle:

Sections 4.5, 8.1, 9.1–9.3, 9.8–9.11, 10, 14, and 17.3–17.5; migrated status tests must retain their prior assertions and `cargo test -p jig-ui` must cover six-tab navigation, raw-identity collisions, and wide/standard/compact/micro rendering.

### 20.6 Task D: recorder, health, timeline, and detail views

Outcome:

Implement all web-dashboard presentation parity in the unified TUI.

Scope:

Work tab.

Timeline tab and filters.

Health tab.

Plan detail sections.

Receipt detail.

Decision detail.

Loop attention detail.

Bounded text and omission markers.

Tests:

Model tests for every section.

Renderer tests at multiple sizes.

Dangerous text sanitization tests.

Parity matrix assertions.

Depends on:

Tasks A and C.

Blocks:

Task F.

Recovery boundary:

Views are reachable only through the not-yet-cut-over dashboard entrypoint and can be reverted independently of runtime state readers.

References and acceptance oracle:

Sections 5.1, 5.6, 9.4–9.9, 11, 14, and 17.3–17.5; every recorder parity-registry row must name a passing model test and renderer assertion, including baseline, remediation, loop recovery, linked navigation, and receipt diff/duration.

### 20.7 Task E: serialized refresh scheduling and lifecycle hardening

Outcome:

Run local, provider, and detail collection through one serialized worker without blocking interaction or leaking work.

Scope:

Implement one request scheduler with per-domain timers and error state.

Implement lazy provider and recorder loading according to startup mode.

Implement completion-relative timers.

Implement coalesced refresh.

Implement queued-intent coalescing, result generations, quit cancellation, and joining.

Implement foreground detail preemption, status requeue, and anti-starvation behavior.

Consume fake `StatusPhase` events so explicit local work preempts only provider phase and waits for local-epoch phase.

Integrate external SIGINT and SIGTERM handling without bypassing terminal restoration.

Implement refresh reconciliation.

Tests:

Channel-controlled concurrency tests.

Quit-during-refresh tests.

PTY restoration tests.

Depends on:

Tasks A and C.

Blocks:

Task F.

Recovery boundary:

The scheduler is proven against fake sources before real integration; revert it and its runtime tests without touching Task B2's source or persisted state.

References and acceptance oracle:

Sections 12.1–12.7 and 17.6–17.7; channel-controlled tests must prove single-call serialization, coalescing, foreground preemption, anti-starvation, epoch rebasing, cleanup-before-terminal-restore, and completion-relative timers without real providers.

### 20.8 Task F: source integration and CLI cutover

Outcome:

Route `jig ui` and `jig status --tui` into the unified dashboard and provide one-shot JSON.

Scope:

Update `UiOpts`.

Update argument conflicts.

Update CLI dispatch.

Add startup modes.

Add JSON output.

Add the hidden port diagnostic.

Set the workspace product version to 0.3.0 and refresh the lockfile and release metadata as part of the public CLI/API cutover.

Wire the real shared local epoch and provider projection into the scheduler and views.

Forward producer `StatusPhase` events into the runtime before the corresponding blocking phase begins.

Publish both domains from accepted `StatusRefresh` results and reproject loaded status-local fields from accepted `RecorderRefresh` results.

Update launcher-facing tests.

Tests:

Parsing tests.

Help tests.

JSON envelope tests.

Compatibility entrypoint tests.

Depends on:

Tasks B2, D, and E.

Blocks:

Task G.

Recovery boundary:

This is the first public routing cutover; before Task G, revert dispatch and adapter wiring to restore the still-present web and status implementations.

References and acceptance oracle:

Sections 8.2–8.5, 12.8, 13, 17.8–17.9, and 19.1–19.4; CLI tests must prove both entrypoints, source integration, exact schema-1 recorder/plan envelopes, semantic old-vs-new status JSON equality inside the new record budget, the explicit oversized-record partial delta, usage-class port errors, and no listener creation. `cargo metadata` must report product version 0.3.0 while the generated repository contract epoch remains unchanged.

### 20.9 Task G: delete web transport and obsolete crate surface

Outcome:

Remove the browser server, security machinery, HTML renderers, and obsolete status crate wiring.

Scope:

Keep the repurposed `jig-ui` package and dependency.

Delete `jig-status-tui`.

Remove `jig-status-tui` workspace membership and the direct dependency from `jig-sh`.

Remove `jig-status-tui` from release publication.

Remove web-only dependencies where unused.

Remove the transitional web modules and dependencies deliberately retained by Task C.

Remove web tests.

Add negative source checks.

Tests:

Workspace metadata succeeds.

No production source retains server behavior; documentation removal remains Task H and converges in Task I.

Depends on:

Task F.

Blocks:

Task H.

Recovery boundary:

Perform deletion in one isolated commit after Task F passes; reverting that commit restores sources and manifests because no state format changes occur.

References and acceptance oracle:

Sections 5.4, 17.10, 19.5, 21.5, and 23.5; `cargo metadata` and manifest inspection must prove the obsolete crate and web-only direct dependencies are absent, and production-source searches must prove the server surface is gone.

### 20.10 Task H: documentation and guide migration

Outcome:

Align public docs, help, guides, and changelog with the TUI cutover.

Scope:

README.

Developer UX.

Public contract.

Status provider guide.

Repository intent.

Crate guides.

Agent map.

Record that generated launcher command scope remains unchanged and requires no template rewrite.

Changelog.

Record 0.3.0 as the cutover and 0.4.0 as the earliest hidden-port shim removal.

Tests:

Agent guide and map checks.

Documentation grep assertions.

Depends on:

Task G.

Blocks:

Task I.

Recovery boundary:

Documentation changes are isolated from generated templates and can be reverted without changing runtime or repository contracts.

References and acceptance oracle:

Sections 18 and 19; guide checks and focused searches must prove current docs describe the terminal workflow, the 0.3.0 cutover, the 0.4.0 shim removal, and unchanged launcher/contract-epoch behavior.

### 20.11 Task I: parity acceptance and release validation

Outcome:

Demonstrate that the TUI contains every supported browser and status feature and that the server is gone.

Scope:

Run the parity matrix.

Run targeted crate tests.

Run CLI tests.

Run shared TUI tests.

Run workspace Clippy.

Run workspace tests.

Build the development binary.

Dogfood `jig ui` in a PTY.

Dogfood `jig status --tui` in a PTY.

Dogfood recorder JSON, plan-detail JSON, and status JSON.

Inspect generated artifacts.

Verify product semver remains separate from the unchanged generated repository contract epoch.

Depends on:

Task H.

Terminal task:

Task I has no dependents and closes the epic only after every layer-level acceptance criterion passes.

Recovery boundary:

Task I is validation-only and changes no version, test definition, implementation, product documentation, or release file.

Its task-local ExecPlan and append-only validation evidence are the only expected repository mutations.

Any failure reopens its owning predecessor rather than weakening an oracle or patching the terminal task.

References and acceptance oracle:

Sections 5.6, 17.11, and 21–23; all targeted tests, `scripts/jig check clippy`, `scripts/jig check test`, three JSON paths, two PTY entrypoints, deletion searches, release metadata, and the row-by-row parity registry must pass.

### 20.12 Dependency graph

```text
A dashboard contracts and parity registry ──► C unified shell and status migration
B1 cancellable readers and plan-path hardening ──┐
A dashboard contracts and parity registry ──────┴──► B2 shared local epoch and typed projections
    A + C ──► D recorder and detail views
    A + C ──► E refresh and lifecycle
    B2 + D + E ──► F source integration and CLI cutover
    F ──► G web/status-crate deletion
    G ──► H docs and guide migration
    H ──► I parity and release validation
```

The graph is acyclic.

Every task except I has a dependent.

I consumes both deletion and documentation outcomes.

### 20.13 Parallelism

Tasks A and B1 can begin in parallel.

Task C begins after Task A.

Task B2 begins after both Task A's contracts and Task B1's safe primitives exist.

Task D begins after contracts and the shell exist and uses typed scenario builders.

Task E begins after Tasks A and C and uses fake sources independently of Task B1.

Task F integrates the real source and performs the command cutover after B2, D, and E finish.

Task G removes the obsolete topology after Task F.

Task H regenerates guides and maps from that final topology.

Task I begins after H finishes.

### 20.14 Why there are no planning beads

The plan itself is not a delivery outcome.

Review rounds are not delivery outcomes.

Beads conversion is not a delivery outcome.

Only Tasks A, B1, B2, and C through I become child delivery beads.

## 21. Acceptance criteria by layer

### 21.1 Product acceptance

One command opens one terminal dashboard containing both domains.

Every feature listed in sections 5.1 and 5.2 is discoverable.

No browser is required.

No port is opened.

The dashboard remains usable while providers run.

### 21.2 Architecture acceptance

One feature-specific dashboard crate exists.

`jig-tui` remains domain-neutral.

The dashboard crate does not depend on repository internals.

The CLI adapter owns repository access.

The status provider v1 decoder remains additive-field tolerant.

Local producer-consumer fields are typed.

### 21.3 Safety acceptance

All repository-controlled terminal text is sanitized.

All new recorder output and rendered dashboard collections and text are bounded, with the distinct-plan index and existing status-v1 machine cardinality documented as compatibility exceptions.

All workers are cancelled and joined before terminal restoration.

No dashboard path mutates state.

No dashboard path records a receipt.

No dashboard key executes a command.

### 21.4 Compatibility acceptance

Existing generated launchers can invoke `ui` and `status`.

`status --tui` retains its current status capabilities.

`status --json` remains schema version 1.

Existing `.agent/state` remains readable.

`--port` gives explicit migration guidance.

The JSON semantic change is documented.

### 21.5 Deletion acceptance

The existing `jig-ui` package remains under the same package and library identity.

No `jig-status-tui` package remains.

No browser dashboard server remains.

No HTTP routes remain.

No dashboard cookies remain.

No dashboard capability tokens remain.

No dashboard HTML or CSS remains.

No release entry publishes the removed `jig-status-tui` crate.

### 21.6 Test acceptance

Targeted dashboard tests pass.

Targeted CLI and snapshot tests pass.

Shared terminal tests pass.

Specialized TUI tests pass.

Workspace Clippy passes.

Workspace tests pass.

Generated template checks pass.

## 22. Validation commands

Build the runtime before dogfooding:

```sh
cargo build -p jig-sh --bin jig
export JIG_DEV_BIN=target/debug/jig
```

Run focused tests:

```sh
cargo test -p jig-ui
cargo test -p jig-tui
cargo test -p jig-sh ui
cargo test -p jig-sh status
cargo test -p jig-sh cli::help_tests
cargo test -p jig-sh cli::status_tests
```

Run specialized regression tests:

```sh
cargo test -p jig-codex-tui
cargo test -p jig-vault-tui
```

Run lints:

```sh
cargo clippy -p jig-ui --all-targets -- -D warnings
cargo clippy -p jig-sh --all-targets -- -D warnings
```

Run repository-contract checks:

```sh
JIG_DEV_BIN=target/debug/jig scripts/jig check contract
JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides
JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map
```

Run repository gates:

```sh
JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
JIG_DEV_BIN=target/debug/jig scripts/jig check test
```

Dogfood interactive commands in a real PTY:

```sh
JIG_DEV_BIN=target/debug/jig scripts/jig ui
JIG_DEV_BIN=target/debug/jig scripts/jig status --tui
```

Dogfood JSON commands:

```sh
recorder_json="$(JIG_DEV_BIN=target/debug/jig scripts/jig --json ui)"
printf '%s\n' "$recorder_json" | jq -e '.snapshot_kind == "recorder"'
plan_id="$(printf '%s\n' "$recorder_json" | jq -er '.open_plans[0].plan_id // .history[0].plan_id')"
JIG_DEV_BIN=target/debug/jig scripts/jig --json ui --plan "$plan_id"
JIG_DEV_BIN=target/debug/jig scripts/jig status --json
```

Task I first verifies `jq` is available for this read-only extraction.

If the dogfood repository has no open or historical plan, Task I uses the existing temporary integration-fixture helper to seed a generic `ExampleProject` plan outside the source checkout; it does not mutate this repository merely to obtain an ID.

Confirm the retired option diagnostic:

```sh
JIG_DEV_BIN=target/debug/jig scripts/jig ui --port 0
```

Expected result:

The command fails without opening a port.

It points to `jig ui` and `jig ui --json`.

## 23. Idempotence and recovery

### 23.1 Task-local recovery

Each child bead uses a task-local ExecPlan.

Each ExecPlan records its Git baseline.

Each task keeps the workspace buildable at its commit boundary.

### 23.2 Crate consolidation recovery

Repurpose `jig-ui` while status behavior still has direct parity tests.

Do not delete `jig-status-tui` until its model, renderer, runtime, and tests have moved.

Do not publish a commit with both feature crates wired as separate dashboard implementations.

### 23.3 Web deletion recovery

Delete web code only after CLI parity tests pass.

Keep deletion in its own reviewable commit.

If a parity gap appears, restore the specific view behavior into the TUI rather than retaining the server.

### 23.4 Generated launcher stability

Do not modify the generated launcher solely for this cutover.

Both `ui` and `status` remain repository-scoped root commands.

If implementation discovers a real launcher-content dependency, record it as a surprise and reassess the compatibility decision before editing templates.

### 23.5 State safety

No migration writes `.agent/state`.

Rolling back the runtime is safe.

The old and new readers consume the same persisted records.

## 24. Risks and mitigations

### 24.1 Risk: one giant application model

Mitigation:

Partition state by domain and feature module.

Use one root coordinator with focused submodels.

Keep provider wire decoding isolated.

Keep detail states isolated.

### 24.2 Risk: refresh complexity

Mitigation:

Use exactly three named request kinds.

Use one serialized scheduler state machine and one active worker.

Tag every request and completion with a generation.

Use channel-controlled tests.

Do not add a general async runtime.

### 24.3 Risk: terminal output injection

Mitigation:

Sanitize at view-model construction.

Test control and bidi characters.

Never use direct output for records.

### 24.4 Risk: hidden parity loss

Mitigation:

Maintain a feature parity matrix.

Require one model test and one render assertion per feature group.

Delete the web crate only after parity passes.

### 24.5 Risk: provider latency degrades recorder use

Mitigation:

Use independent domain snapshots behind one active worker.

Load providers lazily for `jig ui`.

Keep navigation synchronous and data-free.

Allow explicit local foreground requests to cooperatively preempt provider work, requeue the provider request, and prove anti-starvation after foreground activity stops.

### 24.6 Risk: command semantic surprise

Mitigation:

Keep `jig ui` name.

Document terminal requirement.

Provide a port-specific migration error.

Provide one-shot JSON.

### 24.7 Risk: stale selection after refresh

Mitigation:

Use durable identities.

Define deterministic ordering.

Test insertions, removals, and equal timestamps.

### 24.8 Risk: overcoupling status protocol to UI needs

Mitigation:

Keep status JSON schema version 1 unchanged while sharing one typed in-process aggregate.

Keep recorder data release-coupled and typed.

Do not add TUI-only fields to provider contracts.

### 24.9 Risk: unsafe or blocking plan-body input prevents terminal restoration

Mitigation:

Validate plan IDs centrally.

Use component-wise no-follow directory traversal and opened-handle regular-file verification.

Use nonblocking no-follow open for body and sidecar descriptors, post-open regular-file verification, bounded chunks, exact byte limits, and cooperative cancellation.

Test special files and replacement races on Linux and macOS.

### 24.10 Risk: foreground preemption starves provider refresh

Mitigation:

Requeue exactly one cancelled status intent with its original sequence age.

Never let automatic work preempt explicit work.

Retain only the newest detail target and prove the requeued provider eventually runs once explicit activity stops.

### 24.11 Risk: one oversized JSONL record defeats memory and cancellation bounds

Mitigation:

Cap dashboard/status local-epoch records at 1 MiB.

Discard overflow to newline in cancellable fixed chunks without retaining it.

Return a scoped partial error with state-diagnosis and compaction guidance, and test a synthetic huge logical record without constructing it in memory.

## 25. Decisions already made

### 25.1 Retire, do not maintain, the browser implementation

Decision:

Delete the server and browser renderers.

Rationale:

Keeping them as a fallback retains the full maintenance and security burden.

### 25.2 Use Ratatui and Crossterm already pinned by the workspace

Decision:

Do not introduce another TUI framework.

Rationale:

Existing Jig TUIs and lifecycle tests already validate this stack.

### 25.3 Keep `jig ui` canonical

Decision:

Change transport semantics without changing the root command.

Rationale:

This minimizes launcher and user migration while matching the command's generic name.

### 25.4 Keep `status --tui` as a compatibility entrypoint

Decision:

Route it into the same dashboard.

Rationale:

The spelling is already public and retaining it costs no second implementation.

### 25.5 Preserve the generic feature crate identity

Decision:

Repurpose `jig-ui` and retire `jig-status-tui`.

Rationale:

`jig-ui` matches the canonical `jig ui` command without committing to a browser transport.

Keeping it avoids package and release identity churn.

### 25.6 Preserve `jig-tui` boundaries

Decision:

Do not move feature state or widgets into `jig-tui`.

Rationale:

Vault and Codex consumers need mechanics without dashboard coupling.

### 25.7 Independent refresh domains with serialized work

Decision:

Schedule recorder, status, and detail work independently through one worker slot.

Rationale:

Their costs and failures differ materially, while serialization minimizes race and shutdown surfaces.

### 25.8 One-shot JSON instead of HTTP JSON

Decision:

Expose snapshots directly from CLI JSON mode.

Rationale:

This preserves inspectability without retaining a server or authentication protocol.

### 25.9 No mutation actions

Decision:

Keep the dashboard observational.

Rationale:

Mutation would require authority, confirmation, receipt, and recovery design beyond this cutover.

### 25.10 Keep the status compatibility entrypoint permanently

Decision:

`jig status --tui` remains a supported spelling rather than a timed deprecation.

Rationale:

It communicates a useful status-first intent and costs no second implementation.

### 25.11 Preserve selected-provider status semantics

Decision:

Status, Packages, and Blockers remain selected-provider-centric.

Rationale:

Provider IDs are independent namespaces and the current interaction already makes the selected provider explicit.

Provider blockers will not be mixed with gate failures, failed receipts, or loop attention in one ambiguous queue.

### 25.12 Refresh the active domain by default

Decision:

`r` refreshes the active domain and `R` queues both primary domains.

Rationale:

Running configured providers can be materially slower than reading local recorder state.

The explicit capital binding keeps the expensive combined action available without making it accidental.

### 25.13 Refresh open and closed detail differently

Decision:

Open plan detail follows successful local epochs; closed detail refreshes only manually or when reopened.

Rationale:

Open work is expected to gain gates, decisions, and receipts while closed work is normally stable.

### 25.14 Promise compact reachability at 40 by 12

Decision:

Compact terminals retain access to every detail through navigation even when previews and optional columns collapse.

Rationale:

A TUI replacement should not require the wide browser layout, while micro terminals still need only a safe non-panicking fallback.

### 25.15 Use typed status inside the process

Decision:

Share one `StatusSnapshot` between the producer, JSON serializer, and dashboard; retain JSON decoding only for third-party provider-v1 reports.

Rationale:

The loop field defect demonstrates that duplicate in-process aggregate schemas are a bug surface, while external additive compatibility remains isolated at the actual plugin boundary.

### 25.16 Share one local observation epoch across domains

Decision:

Status and recorder projections consume the same typed local epoch rather than independently scanning state, gates, loops, and Git.

Rationale:

Repeated reads can disagree and multiply latency; immutable epoch sharing preserves timestamps, removes hidden rescans, and keeps ownership in `jig-sh`.

### 25.17 Permit bounded foreground preemption

Decision:

Explicit local foreground requests may cooperatively cancel and requeue provider work while automatic work never preempts explicit work.

Rationale:

Providers may legally run for an hour, so non-preemptive serialization would make local detail operationally unavailable despite a responsive render loop.

### 25.18 Keep signal policy in the CLI layer

Decision:

Extract the reusable signal session beside existing CLI supervision and pass external cancellation into the dashboard runtime.

Rationale:

Signal retirement and redelivery are process policy rather than terminal mechanics, so moving them into `jig-tui` would blur its domain-neutral boundary.

### 25.19 Secure plan bodies at the file-descriptor boundary

Decision:

Use canonical component validation, component-wise no-follow opens, nonblocking final open, post-open `fstat`, and bounded chunk reads on supported Unix targets.

Rationale:

Path joining and pre-open metadata checks leave traversal, special-file blocking, and replacement-race surfaces that become shutdown hazards inside a joined worker.

## 26. Source grounding

The browser crate boundary is documented in `crates/jig-ui/AGENTS.md`.

The browser model is defined in `crates/jig-ui/src/model.rs`.

The browser server is defined in `crates/jig-ui/src/server.rs`.

The recorder projection is defined in `crates/jig/src/ui/snapshot.rs`.

The browser adapter is defined in `crates/jig/src/ui.rs`.

The status TUI boundary is documented in `crates/jig-status-tui/AGENTS.md`.

The status model is defined in `crates/jig-status-tui/src/model.rs`.

The status renderer is defined in `crates/jig-status-tui/src/render.rs`.

The status runtime is defined in `crates/jig-status-tui/src/runtime.rs`.

The status adapter is defined in `crates/jig/src/status/tui.rs`.

The versioned aggregate producer is defined in `crates/jig/src/status.rs`.

The shared terminal invariants are documented in `crates/jig-tui/AGENTS.md`.

The shared implementation is defined in `crates/jig-tui/src/lib.rs`.

The CLI contract is defined in `crates/jig/src/cli.rs` and `crates/jig/src/cli/run.rs`.

Generated launcher command scope is defined in `templates/project/scripts/jig.jinja`.

Public behavior is documented in `docs/developer-ux.md`.

Provider behavior is documented in `docs/status-provider.md`.

Human-versus-machine output is documented in `docs/public-contract.md`.

The current workspace pins Ratatui 0.29.0 and Crossterm 0.28.1.

Closed beads `jig-sh-t9n` and `jig-sh-z9h` independently record the loop field mismatch and are superseded by the typed producer regression in Task B2.

## 27. Planning workflow revision log

### Initial architecture synthesis

The initial draft was grounded in current source, docs, manifests, tests, generated launcher templates, Git history, and open beads.

It selected an in-place `jig ui` cutover.

It selected one unified feature crate.

It separated recorder and provider refresh domains while serializing source work.

It retained status schema version 1.

It replaced HTTP JSON with one-shot CLI JSON.

It made web deletion depend on tested TUI parity.

### Review round 1

Two independent reviewers and one dependency validator found structural issues.

The revision replaced the untyped status aggregate and generic request-response enum with typed contracts.

It split safe cancellable readers from the shared local epoch so worker integration cannot outrun shutdown safety.

It made one local observation mandatory across recorder and status projections.

It defined foreground provider preemption, epoch rebasing, multi-time plan detail, exact JSON envelopes, raw identity preservation, secure plan-body opening, explicit limits, signal ownership, and recovery boundaries.

It corrected the delivery graph and replaced the self-confirming exhaustive fixture with focused scenario builders plus a parity registry.

### Review round 2

Three independent reviewers checked the revised design against current status ordering, provider preservation, terminal behavior, and the DAG.

The revision made status collection provider-first and local-second, introduced paired `StatusRefresh` and `RecorderRefresh` results, preserved raw accepted provider JSON beside decoded reports, and defined local reprojection of loaded status.

It completed parity rows, fatal-versus-partial publication rules, schema-1 field tables, closed-plan single-scan detail, digest-qualified fallback identities, proportional-memory exceptions, non-self-confirming differential tests, deterministic provider-only preemption, descriptor-relative mutation hardening, and self-contained task oracles.

It decoupled Task B1 from presentation contracts and let Tasks A and B1 start in parallel.

### Review round 3

Three independent reviewers found the last structural ambiguities in scheduling, partial epochs, memory accounting, and task topology.

The revision added status phase events, generalized provider-phase preemption to explicit local work, defined local/provider status partitions and merge semantics, made partial epochs publishable without falsely proving plan absence, and moved plan decisions to bounded live scans.

It added the 1 MiB record ceiling, secured descriptor-relative directory creation and mutation locks, fixed release-version ownership, expanded status parity into field-level feature groups, completed error/limit registries, and made G precede topology-derived documentation.

Round 3 required structural edits, so a final independent round remains mandatory.

### Review round 4

The nominal final round found three implementation blockers rather than optional refinements.

The revision replaced impossible worker-result phase delivery with a generation-tagged side channel, preserved provider freshness until provider refresh, made nested limit metadata structurally representable, and made body/sidecar mutation opens nonblocking and type-verified.

It also resolved stale multi-time semantics, status age/count parity, and the deliberate oversized-record compatibility exception.

Because these were structural corrections, one confirmation round is required before conversion.

### Review round 5

Three independent confirmation reviewers found no remaining blockers or architectural contradictions.

Their remaining comments were marginal: name all explicit-local preemption sites, clarify phase-channel ownership, preserve aged provider freshness, make nested bounds representable, tighten status parity rows, and align the final task metadata.

Those corrections are integrated.

The reviewers agreed the DAG is acyclic and orphan-free, the tasks are executable, the tests are independently grounded, and Beads conversion is safe.

### Delivery graph polish rounds 1–6

After Beads conversion, three independent reviewers audited the live issue descriptions, structured acceptance fields, and stored dependency edges rather than relying on the plan alone.

The first three graph rounds found execution-level ownership leaks that the abstract DAG did not expose.

Task D incorrectly claimed package/blocker presentation and live filesystem scans despite depending only on Tasks A and C.

Its bead now owns only recorder-facing Work, Timeline, Health, and detail presentation from typed Task A scenarios; Task B2 owns the live body, decision, receipt, and gate collection.

Task E incorrectly implied ownership of real phase production and low-level I/O cancellation.

Its bead now consumes fake phase events and owns only scheduler/runtime cancellation and joining; Tasks B1, B2, and F retain low-level I/O, source-phase production, and real integration respectively.

Task G used an overbroad "JSON endpoint" deletion phrase that could have removed the permanent CLI recorders.

Its bead now deletes only HTTP JSON routes and handlers and explicitly preserves and retests all three CLI JSON paths.

The graph audit also strengthened Task B2's before-operation phase oracle and non-empty exhausted-attempt regression, assigned supported-size differential and oversized-record compatibility testing to Task F, made Task I's task-local ExecPlan and append-only evidence exception explicit, and copied exact gate commands into its acceptance criteria.

Every child description now states its prerequisites and direct consumers, and its embedded acceptance section exactly mirrors the structured Beads acceptance field.

Three independent confirmation rounds then re-read the corrected live graph and returned ready with no remaining blockers or marginal changes.

The live graph is lint-clean, acyclic, orphan-free, and exposes exactly Tasks A and B1 as its unblocked parallel roots; Task I remains its sole terminal delivery task.

## 28. Beads conversion

Conversion occurred after five review rounds reached structural steady state.

The epic references this plan path and planning baseline.

Tasks A, B1, B2, and C through I are concrete child issues:

| Plan task | Bead | Title |
| --- | --- | --- |
| Epic | `jig-sh-l2x` | Retire web UI into a unified terminal dashboard |
| A | `jig-sh-l2x.1` | Define dashboard contracts and parity registry |
| B1 | `jig-sh-l2x.2` | Harden cancellable readers and plan-body paths |
| B2 | `jig-sh-l2x.3` | Build shared local epoch and typed producer projections |
| C | `jig-sh-l2x.4` | Create unified dashboard shell and migrate status TUI |
| D | `jig-sh-l2x.5` | Implement recorder, work, timeline, health, and detail views |
| E | `jig-sh-l2x.6` | Harden serialized refresh scheduling and terminal lifecycle |
| F | `jig-sh-l2x.7` | Integrate source and cut CLI over to the unified TUI |
| G | `jig-sh-l2x.8` | Delete web transport and obsolete status crate |
| H | `jig-sh-l2x.9` | Migrate dashboard documentation and repository guides |
| I | `jig-sh-l2x.10` | Run parity acceptance and release validation |

Their blocking dependencies reproduce section 20.12 exactly, and `br dep cycles` reports no cycles.

The two existing loop-field mismatch beads, `jig-sh-t9n` and `jig-sh-z9h`, are closed as superseded by Task B2.

Task B2 explicitly carries their real-producer `workflow_id` and `item_key` regression acceptance criteria, so the fix happens at the typed producer boundary rather than in UI-side tolerant decoding that Task G would delete.

No bead will be created for plan writing.

No bead will be created for plan review.

No bead will be created for beads conversion.

## 29. Planning completion checklist

- [x] Every delivery task has a concrete outcome, dependency set, acceptance oracle, and recovery boundary.

- [x] Existing-code claims are source grounded.

- [x] Framework choices are grounded in the pinned workspace.

- [x] The product scope is explicit.

- [x] The non-goals are explicit.

- [x] The command migration is explicit.

- [x] The data contracts are explicit.

- [x] The refresh lifecycle is explicit.

- [x] The terminal safety model is explicit.

- [x] Every web feature appears in the parity list.

- [x] Every status TUI feature appears in the parity list.

- [x] Every delivery task names dependencies.

- [x] The dependency graph is acyclic.

- [x] Every non-final task has a dependent.

- [x] Five review rounds are integrated.

- [x] Six live delivery-graph polish rounds are integrated.

- [x] The final review is marginal rather than structural.

- [x] Delivery beads exist.

- [x] Delivery bead dependencies match the plan.

- [x] Beads JSONL is flushed.

## 30. Implementation prompt contract

Each child bead is also the detailed execution prompt for its delivery task.

Its description must include the task outcome, exact scope, referenced plan sections, dependency prerequisites, recovery boundary, and named acceptance oracle from section 20.

Its acceptance criteria must restate the externally observable result and commands or tests that prove it.

Every executing agent must read the root `AGENTS.md`, `agent-map.md`, the nearest crate guides, this plan, and `.agent/PLANS.md` before creating the task-local ExecPlan required by repository policy.

The agent must claim only an unblocked bead, record its exact Git baseline, preserve unrelated worktree changes, use `JIG_DEV_BIN=target/debug/jig` for runtime dogfooding, and stop for input if implementation evidence contradicts a product or compatibility decision in this plan.

The agent must not weaken a differential, PTY, security, parity, or cancellation oracle to make a task pass.

The agent must not pull work from a successor bead merely because adjacent code is convenient; required cross-task changes update the plan and dependency graph first.

On completion, the agent runs the task's focused tests, `scripts/jig work check` for applicable gates, records evidence, closes the bead with a concrete reason, and flushes Beads JSONL.
