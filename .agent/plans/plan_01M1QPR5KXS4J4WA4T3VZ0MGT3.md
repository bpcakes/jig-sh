# Create the unified dashboard shell and migrate status presentation

This ExecPlan implements Task C (`jig-sh-l2x.4`) from `docs/plans/unified-terminal-dashboard.md`. The outcome is an additive, copy-first terminal application inside `jig-ui`: one six-tab shell owns terminal layout and interaction, while Status, Packages, and Blockers preserve the legacy status TUI's behavior. The existing HTTP UI and `jig-status-tui` crate remain compiling and publicly routed until Tasks F and G, so this task introduces no command cutover or deletion.

Implementation baseline: `a4947e59` on branch `jig-sh-l2x`.

## Progress

- [x] Read repository and crate guidance, `.agent/PLANS.md`, Task C, and its referenced architecture, interaction, safety, and test sections.
- [x] Claim `jig-sh-l2x.4`, build the development binary, and open structured work.
- [x] Trace the legacy status model, renderer, runtime, and tests against Task A's typed status and recorder contracts.
- [x] Add terminal dependencies and a public additive dashboard entrypoint to `jig-ui` while retaining the web API.
- [x] Implement the six-tab application model, per-domain state, stable identity reconciliation, detail state, and contextual key handling.
- [x] Migrate Status, Packages, Blockers, and package detail presentation with raw typed identities and separately sanitized display values.
- [x] Implement deterministic wide, standard, compact, and micro layouts plus loading, empty, partial, stale, and error states.
- [x] Migrate legacy assertions and add navigation, hostile-identity, sizing, footer, and selection regression coverage.
- [x] Run focused validation and two comprehensive working-tree review/fix rounds. Round 1 used verified fingerprint `b2da892c`; round 2 used verified fingerprint `004d0d12`. Every actionable finding from both Claude and Codex passes was fixed, and 53 unit plus 21 contract tests now pass.
- [x] Record passing exact-diff evidence, close the bead, flush Beads, finish structured work, and commit Task C.

## Surprises & Discoveries

- The legacy status model sanitizes the complete JSON value before deserialization. That protects rendering but mutates provider, package, and blocker identity, so it cannot be copied unchanged. Task C must decode Task A's typed snapshot first, preserve raw identity fields, and create sanitized display fields separately.
- The legacy status runtime treats every unknown top-level key as a redraw. The unified keymap requires ignored keys to remain true no-ops and release-only events to be filtered through `jig-tui`.
- The old renderer supports only a hard 72-by-20 minimum. Task C must retain the existing standard presentation while adding useful compact and micro fallbacks that never panic on zero-sized areas.
- Tasks D and E deliberately follow this task. Work, Timeline, and Health need real navigable shell states now, but Task C may render honest typed placeholders until their full recorder views and hardened refresh scheduler land.
- The repository's hard 800-line new-file budget required splitting the copied model, renderer, and runtime into focused children while keeping their interfaces private to the terminal namespace.
- The permissive legacy JSON conversion is useful for migration-test parity but unsafe as a production boundary. It is compiled only for tests; production accepts the typed Task A contracts exclusively.
- A Unix pseudo-terminal regression test can verify alternate-screen entry and restoration by launching a filtered child test inside `openpty`; asserting literal rendered text is unreliable because Ratatui emits terminal-diff control sequences.
- The first comprehensive review showed the initial copy preserved wide-screen behavior but let compact rendering bypass selection, detail, and stale-domain state. Routing compact views through the same application state, rather than a second stateless projection, removed that divergence.
- Provider IDs scope package and blocker IDs. Reconciliation must therefore resolve the provider first and reset all child state when it disappears; independently matching child IDs can silently transfer selection and open detail to unrelated data.
- A successful status refresh also supplies a same-epoch recorder projection. Treating that projection as satisfying a queued local refresh avoids a redundant collection, while invalid recorder schema data retains the queued retry and last valid snapshot.
- The second review found that lazy loading was coupled to the broad redraw action. Splitting tab changes from visual redraws prevents resize or held navigation events from creating an unbounded refresh chain after a persistent load failure.
- Ratatui's 65,535-cell diff constraint was already documented and handled in the vault TUI. Moving the clamp into `jig-tui` gives the unified dashboard and vault one safety implementation and makes oversized-area behavior directly testable.
- The final review also exposed that a recorder refresh's paired `status_local` projection was being discarded. Applying it to the loaded status dashboard keeps local status, work counts, loops, errors, repository metadata, and observation time aligned without replacing provider observations.

## Decision Log

- Keep the terminal application under a new focused namespace in `jig-ui`, alongside the existing web modules. Do not rename or delete the root web `model.rs`, `server.rs`, or `html.rs` during the coexistence phase.
- Consume `StatusSnapshot` and `RecorderSnapshot` as typed values. External provider protocol validation remains at the Task A producer boundary; the application model does not serialize and deserialize same-process snapshots.
- Preserve each raw identity in model state and use it for selection reconciliation and future routing. Derive sanitized labels only when constructing view data or Ratatui text.
- Use one top-level `Tab` enum in the specified order: Status, Packages, Blockers, Work, Timeline, Health. Numeric keys retain the first three legacy positions; `jig ui` and `status --tui` initial-tab differences remain a Task F adapter concern.
- Centralize input interpretation in one application event handler. Details consume Escape/Backspace/Enter before top-level quit or navigation; Tab cycles plan-detail sections when applicable and otherwise cycles top-level tabs.
- Model status and recorder domains independently so loading or failure in one cannot blank the other. Retain the last successful snapshot and its observation timestamp when a later refresh fails.
- Use stable identity reconciliation rather than indexes for providers, packages, blockers, plans, and timeline rows. Clamp only after the raw selected identity is absent from the refreshed collection.
- Define renderer size tiers from the plan: wide at 108x24, standard at 72x20, compact at 40x12, and micro below that. Zero-width and zero-height areas return safely.
- Keep ordinary dashboard output inside Ratatui and pass every repository-controlled display string through `jig_tui::sanitize_text`; do not add direct terminal writes, pagers, editors, mutations, or receipt recording.
- Keep Work, Timeline, and Health as honest, typed local-snapshot summaries in Task C. Their complete presentation belongs to Task D, and refresh-generation/preemption/timer hardening belongs to Task E.
- Compile the legacy permissive `serde_json::Value` fixture adapter only under `cfg(test)`. This retains copied assertions without creating a second production decoding path.
- Represent tab selection as a distinct runtime action from redraw. Only an explicit tab change may lazy-load a missing domain; resize and within-view navigation only redraw.
- Put the Ratatui linear-index clamp in `jig-tui` and reuse it from existing vault and new dashboard rendering instead of maintaining per-TUI copies.
- Reproject `RecorderRefresh::status_local` into any loaded status dashboard. Preserve provider observations and their selection while replacing only the paired local projection.

## Outcomes & Retrospective

The additive `jig-ui::terminal` entrypoint now owns a six-tab terminal shell with typed production inputs, provider-scoped stable identity, independent status/recorder state, responsive layouts, contextual controls, and safe terminal lifecycle behavior. Status, Packages, Blockers, and package detail retain the legacy feature set; Work, Timeline, and Health expose honest typed local summaries pending Task D. The old HTTP UI and `jig-status-tui` remain intact for the planned coexistence window.

Two independent Claude+Codex review rounds materially strengthened the result. The fixes eliminated compact-mode state divergence, cross-provider child selection transfer, stale-domain mislabeling, redraw-triggered refresh storms, duplicated provider aggregation, discarded paired local projections, and oversized-terminal risk. Regression coverage now exercises typed rendering, hostile and colliding identities, provider removal, compact and micro selection/detail/error behavior, schema rejection, queue ordering, local reprojection, authoritative fallback summaries, viewport clamping, and a real 80-by-24 pseudo-terminal render/restoration cycle.

## Context and orientation

`crates/jig-ui/src/lib.rs` currently exposes the loopback HTTP server and Task A's `dashboard` contracts. New terminal modules must coexist with `html.rs`, `model.rs`, and `server.rs`. `crates/jig-status-tui/src/{model,render,runtime}.rs` and their focused children are the compatibility implementation to copy and adapt. `crates/jig-tui/src/lib.rs` owns terminal lifecycle, safe text, actionable-key filtering, cancellation, and joined worker mechanics.

Task A fixtures live in `crates/jig-ui/src/dashboard/scenarios.rs`; Task B2's source adapter implements `DashboardSource` in `crates/jig/src/ui/source.rs` but remains unused until Task F. Task C tests should construct typed snapshots from these shared scenarios rather than inventing a second wire schema.

## Plan of work

First establish an additive terminal namespace and public entrypoint without disturbing the web API. Port the legacy status data transformations into typed conversions that preserve raw IDs, then introduce the six-tab application state and stable selection logic. Port status rendering and package detail into focused render modules, add size-tier shells and honest placeholders for the three recorder tabs, and centralize contextual key handling. Finally migrate the legacy behavioral assertions, add Task C's new navigation/identity/layout tests, and run the required review and validation loop before committing.

## Concrete steps

1. Add `crossterm`, `ratatui`, and `jig-tui` dependencies to `jig-ui`; add a terminal module tree and an additive public run/configuration API without altering `UiServer` or `SnapshotProvider`.
2. Adapt the legacy status view model to accept `StatusSnapshot` directly. Preserve raw provider/package/blocker identifiers and separately sanitize titles, descriptions, diagnostics, extensions, and other rendered strings.
3. Add a six-tab `App` with independent status/recorder domain state, stable selected identities, package detail state, notices, refresh flags/errors, and deterministic selection reconciliation.
4. Port Status, Packages, Blockers, and package-detail rendering. Add common header, tabs, contextual footer, and loading/error/partial states.
5. Add Work, Timeline, and Health shell states with honest Task C placeholders derived only from available typed snapshot facts; defer their complete view behavior to Task D.
6. Implement wide, standard, compact, and micro layout dispatch. Keep primary lists and contextual controls visible before optional previews, and make all zero/nonzero terminal sizes safe.
7. Centralize key handling for six-tab navigation, provider switching, filtering, list movement, details, quit, and refresh intents. Return an explicit no-op for unrecognized or release-only keys.
8. Port legacy status tests without weakening assertions and add focused tests for tab order, initial selection, raw-identity collisions, stable refresh reconciliation, hostile text, contextual footer, key handling, and all four size tiers.
9. Run formatting, Clippy, `cargo test -p jig-ui`, relevant legacy parity tests, PTY/terminal lifecycle coverage, and configured Jig gates; perform the requested comprehensive review at most twice and address every finding before commit.

## Validation and acceptance

Success means `jig-ui` exposes one six-tab terminal shell in the exact Status, Packages, Blockers, Work, Timeline, Health order while the old HTTP and status-TUI APIs still compile. The first three tabs retain legacy content, filtering, provider switching, package detail, and stable blocker behavior. The latter three have explicit domain state and honest pre-Task-D presentation rather than fake data.

Tests must prove stable selection by raw identity across reorder and shrink, including distinct hostile IDs with identical sanitized display forms. They must cover every global and Task C-relevant domain key, ignored and release events, contextual footer changes, detail precedence, and rendering at 120x36, 80x24, 60x15, below 40x12, and zero-sized areas. Existing status renderer assertions must move with equal or stronger oracles.

## Idempotence and recovery

This task is additive. The existing web server and `jig-status-tui` remain compiled and publicly routed; reverting the new terminal modules, dependencies, and entrypoint restores the baseline without persistence changes. The new application is read-only and does not record receipts, launch subprocesses, or mutate repository state.

## Interfaces and dependencies

The new public terminal entrypoint accepts the Task A `DashboardSource` boundary (or a narrowly equivalent typed source facade), an initial tab/configuration, and refresh timing needed by the copied runtime. It depends on `jig-tui`, Ratatui, Crossterm, Serde, Serde JSON, and `jig-contract`, but never on `RepoContext`, state storage, process supervision, HTTP internals, or provider configuration.

Revision note (2026-09-05): replaced the work-start stub with a self-contained Task C execution plan after tracing the legacy status implementation and the typed Task A/B2 boundaries.
