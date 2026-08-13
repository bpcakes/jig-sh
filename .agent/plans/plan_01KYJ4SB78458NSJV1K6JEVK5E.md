# Add an interactive status TUI

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while implementation proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

Jig already runs project-specific `jig.status-provider/v1` executables and combines their reports with local Git, Jig work-plan, gate, and loop state. Today an operator can read a compact text summary or inspect the full JSON, but neither form is convenient for navigating 100 or more rewrite work packages. After this change, an operator can run `scripts/jig status --tui` and interactively inspect rewrite progress, stale or dirty source inputs, package blockers, provider diagnostics, and Jig-owned operational state. The screen refreshes without losing the current selection, remains responsive while a provider runs, and safely cancels an in-flight provider process tree when the operator quits.

The implementation-agent launcher discussed as a later product milestone is deliberately out of scope. This plan establishes a read-only inspection surface and does not add launch policy, prompts, subprocess authority, remote fetching, cached status, or repository writes.

## Progress

- [x] (2026-07-27 16:00Z) Audited the existing status aggregate, CLI dispatch, process-tree supervisor, hocr2 provider payload, workspace Rust floor, and terminal-library compatibility.
- [x] (2026-07-27 16:00Z) Started structured Jig plan `plan_01KYJ4SB78458NSJV1K6JEVK5E` and recorded the initial TUI scope and architecture.
- [x] (2026-07-27 16:20Z) Added `StatusOpts`, the `--tui` and bounded `--refresh-seconds` modes, the Jig-to-TUI adapter, and cancellation propagation through the existing provider process-tree supervisor while preserving default text and `--json` paths.
- [x] (2026-07-27 16:27Z) Implemented the typed view model, stable navigation/filter state, three Ratatui views, single background refresh worker, and guarded terminal lifecycle in the separate `jig-status-tui` crate.
- [x] (2026-07-27 16:31Z) Added focused model, rendering, empty/failure, CLI, in-flight cancellation, terminal-requirement, and additive-schema tests; verified both the TUI crate and Jig status suite.
- [x] (2026-07-27 16:32Z) Updated status-provider, public-contract, configuration, developer-UX, repository-intent, README, crate-guide, release, installer, and generated template documentation/integration.
- [x] (2026-07-27 16:43Z) Passed formatting, focused and full workspace tests, Clippy, Rust 1.85/no-default compatibility, crate packaging, repository policy checks, and both required plan gates. Full-work-check receipt: `receipt_01KYJ7FMPTNRWXBJJ7R5745DB6`.
- [x] (2026-07-27 16:29Z) Dogfooded the development binary against `/Users/aa/Documents/hocr2` at 80x24: inspected 130 packages, 25 blockers, native/normalized progress, and current target/legacy inputs; quitting during the active Ruby provider returned in 0.036 seconds with no provider descendant remaining.
- [x] (2026-07-27 16:44Z) Recorded fresh evidence for every changed path and closed the structured plan successfully; the final repository commit carries the implementation and closure records together.
- [x] (2026-08-13) Superseded the Ratatui 0.28.1 dependency decision with Ratatui 0.29.0 after reproducing the 65,535-cell failure at 608x113; aligned dependency rationale and compatibility pins while retaining Rust 1.85.

## Surprises & Discoveries

- Observation: hocr2's current private Ruby provider takes about 4.7 seconds and returns 130 packages, 518 acceptance checks, and 25 blockers.
  Evidence: `scripts/jig status --json` in `/Users/aa/Documents/hocr2` reported `duration_ms: 4700`, `work_packages: 130`, `acceptance_checks: 518`, and `blockers: 25`. Refresh must therefore run off the terminal event loop and must not use an aggressive default interval.

- Observation: the workspace declares Rust 1.85, while the current Ratatui 0.30 line declares Rust 1.88. Ratatui 0.29 hard-pins `unicode-width` 0.2.0, so adopting its large-buffer fix requires aligning Jig's direct width dependency and Rustyline on that release.
  Evidence: the root `Cargo.toml` contains `rust-version = "1.85"`; Ratatui 0.29.0 declares Rust 1.74 and calculates `Rect` areas as `u32`; Ratatui 0.30.2 declares `rust-version = "1.88.0"`. The workspace therefore pins Ratatui 0.29.0, `unicode-width` 0.2.0, and Rustyline 17.0.2 until the Rust floor can move.

- Observation: Ratatui's compatible `instability = "0.3"` range can resolve to instability releases that require a newer Rust version than Ratatui itself.
  Evidence: `cargo metadata` reported `instability 0.3.12 rust-version 1.88` and `darling 0.23.0 rust-version 1.88.0`. `cargo info instability@0.3.10` reports Rust 1.64, while 0.3.11 reports Rust 1.88.

- Observation: Jig's existing provider supervisor already accepts a cancellation callback and owns full process-tree cleanup, but `status::snapshot` currently hard-codes a callback that never cancels.
  Evidence: `crates/jig/src/doctor.rs::run_owned_process_tree_with_output_limits` accepts `cancelled: impl FnMut() -> bool`; `crates/jig/src/status.rs::run_provider_inner_with_limits` passes `|| false`.

- Observation: `jig-ui` already demonstrates the desired dependency direction: presentation is a CLI-owned internal crate that consumes snapshots through a narrow provider trait rather than importing `RepoContext` or state storage.
  Evidence: `crates/jig-ui/src/lib.rs` defines `SnapshotProvider`, while `crates/jig/src/ui/snapshot.rs` adapts Jig runtime state to that interface.

- Observation: provider-controlled report strings are valid JSON data but can contain terminal control characters, and the provider contract's nonblank validation is not a terminal-escaping boundary.
  Evidence: a synthetic diagnostic containing ESC would otherwise reach a Ratatui text span unchanged. The TUI now recursively replaces Unicode control characters in aggregate string values and separately sanitizes snapshot-source error strings before rendering; a regression test proves ESC becomes U+FFFD.

- Observation: drawing every 100 ms while idle caused Crossterm to emit repeated cursor-control traffic even when no visible state changed.
  Evidence: the first 80x24 PTY dogfood trace contained repeated hide-cursor sequences between the completed refresh and input. The runtime now draws only on the initial frame, a refresh-state/result change, or an actionable key.

## Decision Log

- Decision: expose the TUI as an opt-in `jig status --tui` mode and retain the existing human summary as the default.
  Rationale: scripts and current operators keep stable behavior, while the interactive surface is explicit and testable. The global `--json` flag conflicts with `--tui`.
  Date/Author: 2026-07-27 / Codex

- Decision: use Ratatui 0.29.0 with Crossterm 0.28.1, both exactly pinned in workspace dependencies.
  Rationale: Ratatui 0.29 is the newest release below the workspace's Rust 1.85 floor and its `u32` buffer areas render terminals containing more than 65,535 cells. Its exact `unicode-width` 0.2.0 requirement is accepted as part of this decision; Jig aligns its direct width dependency and Rustyline accordingly. Exact pins prevent compatible-range drift.
  Date/Author: 2026-08-13 / Codex, superseding the 2026-07-27 Ratatui 0.28.1 decision after the large-terminal crash was reproduced

- Decision: add a direct exact `instability = 0.3.10` dependency to `jig-status-tui`.
  Rationale: this constrains Ratatui's transitive range to the latest release whose declared Rust floor is below Jig's 1.85 floor. A lockfile-only downgrade could drift again during an allowed dependency update.
  Date/Author: 2026-07-27 / Codex

- Decision: consume the existing aggregate JSON through a typed, additive-field-tolerant view model rather than call project-specific provider code or build a second discovery path.
  Rationale: the status aggregate remains the one inspection boundary for the text, JSON, and terminal consumers. Private providers stay private, and any conforming provider remains usable.
  Date/Author: 2026-07-27 / Codex

- Decision: render three views—Overview, Packages, and Blockers—with provider switching and bounded list navigation.
  Rationale: Overview makes repo/input freshness and normalized progress immediately visible; Packages exposes every package and acceptance state; Blockers gives direct access to the operator's primary queue without forcing a scan through all packages.
  Date/Author: 2026-07-27 / Codex

- Decision: collect snapshots in one background thread at a time and carry an atomic cancellation flag into the existing process supervisor.
  Rationale: hocr2 collection takes multiple seconds, so synchronous collection would make quit and navigation appear hung. A single worker prevents overlapping expensive provider runs. On quit, the event loop cancels and joins the worker so provider descendants are cleaned up before terminal restoration and process exit.
  Date/Author: 2026-07-27 / Codex

- Decision: keep the `jig status --tui` command adapter in `crates/jig`, but move all terminal presentation and event-loop code into a new `crates/jig-status-tui` workspace crate.
  Rationale: the user challenged whether another UI belongs inside Jig. The generic status consumer belongs in the Jig distribution because it works with the public aggregate, but it should follow the existing `jig-ui` dependency boundary rather than enlarge the runtime crate. The new crate accepts snapshots through a narrow trait, remains independent of `RepoContext` and provider execution, and can be extracted or reused later. Existing `jig-ui` remains the HTTP flight-recorder surface because it has a different snapshot model and transport/security concerns.
  Date/Author: 2026-07-27 / Codex

- Decision: refresh immediately on launch, every 30 seconds after a completed collection by default, and on `r`; allow `--refresh-seconds` to override the interval.
  Rationale: 30 seconds is useful operationally without continuously spending roughly five seconds inside hocr2's provider. Scheduling from completion prevents back-to-back runs when a provider is slower than the configured interval. One manual refresh may be queued while collection is already active.
  Date/Author: 2026-07-27 / Codex

- Decision: sanitize every aggregate string and source error before it reaches terminal rendering, and redraw only when application state changes.
  Rationale: a generic consumer must treat provider output as display data rather than terminal instructions. State-driven drawing also keeps the event loop responsive without producing unnecessary terminal output.
  Date/Author: 2026-07-27 / Codex

## Outcomes & Retrospective

The implementation, hocr2 dogfood, repository-wide verification, and structured plan closure are complete. The crate split answered the architectural concern cleanly: Jig owns provider execution and the generic CLI entrypoint, while `jig-status-tui` owns only a typed view model, terminal runtime, and rendering over aggregate JSON. The existing browser `jig-ui` remains unchanged.

The real hocr2 run showed that the dashboard is useful at the minimum practical 80x24 size and that a multi-second closed-source Ruby provider does not make quit unresponsive. It also surfaced two issues that fixture-only development would have missed: idle redraw traffic and provider strings as a terminal-injection boundary. Both are covered in the implementation and focused tests. The full recorded Jig work check passed both the contract gate and all workspace tests, and the required gates reported fresh receipts over every changed non-state path. The Codex launcher remains intentionally unimplemented because status interoperability does not imply launch authority.

The shared terminal stack now pins Ratatui 0.29.0 so the status dashboard and later Jig TUIs can render buffers larger than 65,535 cells. The workspace deliberately aligns `unicode-width` at 0.2.0 and Rustyline at 17.0.2 as compatibility consequences of retaining Rust 1.85.

## Context and Orientation

The repository root is `/Users/aa/Documents/jig-sh`. `crates/jig/src/status.rs` owns the version-1 status aggregate. It launches each configured provider, validates the public contract supplied by `jig-contract`, and returns a `serde_json::Value` containing `repository`, `work`, `loops`, `providers`, and `errors`. `crates/jig/src/status/git.rs` compares provider-declared Git inputs to local checkout revisions and cleanliness. `crates/jig/src/cli/output/status.rs` renders the existing non-interactive summary.

The root CLI is defined in `crates/jig/src/cli.rs`; `CommandKind::Status` carries `StatusOpts`. `crates/jig/src/cli/run.rs` loads `RepoContext`, calls `status::snapshot` for text/JSON, and delegates interactive mode through `status::tui`. Status-specific options live in `crates/jig/src/cli/status_opts.rs` so the already large CLI root remains small.

The owned process-tree supervisor is in `crates/jig/src/doctor.rs`. Despite its historical module location, `run_owned_process_tree_with_output_limits` is the shared primitive used by status providers. It polls a caller-supplied cancellation callback, terminates the owned process tree on timeout, cancellation, or normal leader exit, drains bounded output, and reaps the process. The TUI must reuse that path and must join its refresh thread before exiting.

Create `crates/jig-status-tui` as a CLI-owned internal presentation crate. Keep its responsibilities in `src/model.rs`, `src/render.rs`, and `src/runtime.rs`. `crates/jig/src/status/tui.rs` is only an adapter from cloned `RepoContext` plus `status::snapshot_with_cancellation` to the TUI crate's snapshot-source trait. A “view model” means typed display-oriented data derived only from aggregate JSON. It ignores unknown additive JSON fields and rejects an aggregate schema version other than 1. It must not become a second public protocol.

The TUI has three tabs. Overview displays repository identity and cleanliness, local tracking-ref ahead/behind state, each provider's outcome and duration, normalized specification/implementation/verification/acceptance counts, Git input freshness with expected and observed revisions, open Jig plans, loop attention, provider failures, diagnostics, and aggregate collection errors. Packages displays a selectable table containing id, title, native facet states, completed acceptance checks versus total checks, and blocker count; `b` toggles a blocked-only filter. The lower package detail includes dependencies, blockers, evidence, and source references. Blockers flattens all package blockers into a selectable queue and displays the full message, source, related package, and owning package state.

The keyboard contract is: `q`, Escape, or Ctrl-C quits; `r` refreshes; Tab and Shift-Tab change tabs; `1`, `2`, and `3` select a tab directly; Up/Down and `j`/`k` move selection; PageUp/PageDown and Home/End move by larger bounds; `[` and `]` switch providers; and `b` toggles the package filter. The footer always advertises the relevant keys. Selection is preserved by stable provider id, package id, and blocker identity when a refresh returns.

## Plan of Work

First, add exact workspace dependencies in `Cargo.toml`, add the new `jig-status-tui` member and matching exact internal dependency, and consume only `jig-status-tui` from `crates/jig/Cargo.toml`. Create `StatusOpts` in `crates/jig/src/cli/status_opts.rs`, change the command enum to `Status(StatusOpts)`, update help examples, and route `--tui` through the small Jig adapter. Reject non-terminal stdin or stdout with an actionable error before changing terminal mode. Preserve the output and exit behavior of the two existing modes.

Second, revise `crates/jig/src/status.rs` so the public-in-crate `snapshot` wrapper still uses a non-cancelling callback, while a new `snapshot_with_cancellation` passes a borrowed cancellation callback through every provider run and into `run_owned_process_tree_with_output_limits`. A cancelled collection is only an internal shutdown result when the TUI is quitting; ordinary snapshot behavior and JSON schema version remain unchanged.

Third, implement the typed view model and application state in `crates/jig-status-tui`. Define a public `SnapshotSource: Send + Sync` trait that returns aggregate JSON or a display-safe error string and accepts a cancellation callback; do not expose Jig runtime types. Deserialize only fields needed to render while ignoring additions. Convert the already validated provider report into package, blocker, diagnostic, evidence, and source views. Compute acceptance complete/total counts from checks and build a flattened blocker queue. Implement stable-id selection preservation, tab/provider movement, bounded row movement, and blocked-only filtering as pure functions.

Fourth, implement rendering with Ratatui widgets. Use color as a secondary signal and retain explicit status text. Render a concise loading or error screen before the first successful snapshot, a minimum-size notice below 72 columns or 20 rows, and valid empty states for no providers, no packages, no blockers, no inputs, or no diagnostics. Use Ratatui's `TestBackend` to make output deterministic without a real terminal.

Fifth, implement the runtime. Enter raw mode and the alternate screen through an RAII guard whose destructor restores cursor, screen, and raw mode on every ordinary error or unwind. Spawn one named refresh thread with a cloned `RepoContext`, an `Arc<AtomicBool>` cancellation flag, and a standard-library channel. Poll Crossterm events on the main thread. When quitting or encountering a terminal error, set cancellation, join the worker, and only then return. Do not spawn overlapping workers; queue at most one requested refresh and schedule automatic refresh from the prior completion time.

Finally, add tests and documentation. Extend status runner tests with cancellation proof. Add CLI parse and help tests for TUI options and conflicts. Add model/navigation/render tests for current, stale, dirty, partial, empty, malformed-version, small-terminal, and blocker-rich fixtures. Update `docs/status-provider.md` and the root README or CLI reference that introduces `jig status`. Build the development binary and dogfood it in hocr2 using `JIG_DEV_BIN=/Users/aa/Documents/jig-sh/target/debug/jig scripts/jig status --tui --refresh-seconds 3600`.

## Concrete Steps

Work from `/Users/aa/Documents/jig-sh`.

After editing dependencies and the cancellation boundary, run:

    cargo test -p jig-sh status
    cargo test -p jig-sh cli::tests

Expect all existing status and CLI tests plus the new cancellation and option tests to pass.

After implementing the model and renderer, run:

    cargo test -p jig-status-tui

Expect tests to prove schema parsing, selection preservation, package filtering, blocker flattening, minimum-size behavior, and visible freshness/progress/blocker labels.

Build and run the development binary:

    cargo build -p jig-sh
    cd /Users/aa/Documents/hocr2
    JIG_DEV_BIN=/Users/aa/Documents/jig-sh/target/debug/jig scripts/jig status --tui --refresh-seconds 3600

Wait for the first refresh. Expect the header to identify `hocr2`, Overview to show both `legacy` and `target` as `current`, progress to show 130 packages and 25 blockers, Packages to contain `WP-ACCT-001`, and Blockers to list package-scoped blocker messages. Press `r` and verify a refresh begins without losing keyboard control; press `q` and verify the original shell screen and input mode return.

Run repository verification:

    cargo fmt --all --check
    cargo clippy -p jig-sh --all-targets -- -D warnings
    scripts/jig check test
    scripts/jig work evidence --plan-id plan_01KYJ4SB78458NSJV1K6JEVK5E
    scripts/jig work gates --plan-id plan_01KYJ4SB78458NSJV1K6JEVK5E

Before closing, update this plan with observed outputs, commit the implementation, and run:

    scripts/jig work finish --plan-id plan_01KYJ4SB78458NSJV1K6JEVK5E --outcome success --resolution "Added and dogfooded the interactive status TUI."

Then commit the append-only closure records separately if the repository workflow produces them after the implementation commit.

## Validation and Acceptance

Acceptance requires all of the following observable behavior:

`scripts/jig status` still prints the existing concise human summary. `scripts/jig status --json` still emits aggregate `schema_version: 1`. `scripts/jig status --tui` opens an alternate-screen dashboard only when both input and output are terminals. Combining `--tui` and `--json`, supplying `--refresh-seconds` without `--tui`, or redirecting TUI input/output fails with a clear message and leaves terminal mode unchanged.

The initial provider collection does not freeze navigation or quit handling. Quitting during a long-running provider sets the supervisor cancellation callback, terminates and reaps the provider tree, joins the refresh worker, restores the terminal, and returns promptly. Automatic refreshes never overlap.

On the hocr2 fixture, Overview exposes target and legacy freshness, package and blocker totals, native/normalized progress, provider duration/outcome, and Jig plan/loop state. Packages and Blockers are navigable with documented keys and preserve a stable selection after refresh. Partial providers, failed providers, collection errors, and empty states remain inspectable rather than crashing.

All new rendering uses explicit labels in addition to color, handles at least an 80-by-24 terminal, and gives a deterministic minimum-size message below the supported dimensions. The full Jig test gate and plan gates pass.

## Idempotence and Recovery

All status modes remain read-only with respect to the inspected repository. Re-running tests or the TUI does not write a status cache or receipt and does not fetch remotes. The structured work commands are the only steps that append state records and are safe to inspect with `scripts/jig work status`.

If a render or event-loop error occurs after entering alternate-screen or raw mode, return through the terminal guard so its destructor restores the screen and input mode. If a refresh is active, cancel and join it before returning. If dependency resolution selects a crate that does not compile on Rust 1.85, retain the exact 0.28.1/0.28.1 pins and inspect `cargo tree -i <crate>` rather than raising the workspace Rust floor.

If hocr2 dogfooding fails before the alternate screen opens, use `JIG_DEV_BIN=/Users/aa/Documents/jig-sh/target/debug/jig scripts/jig status --json` to distinguish provider/aggregate failure from terminal setup failure. Do not modify hocr2's private provider as part of this plan.

## Artifacts and Notes

The baseline hocr2 aggregate observed before implementation was:

    repository: hocr2 main@9367b1bc94c, clean
    provider: factorish.hocr2.migration-readiness, complete, 4700 ms
    packages: 130
    blockers: 25 across 25 packages
    acceptance checks: 518
    input legacy: current at 389b1fa12473963e2a9c291dd9455f51b154ef42
    input target: current at 9367b1bc94c4fdbbc081900eeb187c769a3bce22

Record final test receipts and a concise dogfood transcript here during implementation.

Focused verification completed before the full repository gate:

    cargo test -p jig-status-tui
    # 5 passed
    cargo test -p jig-sh status -- --nocapture
    # 45 passed; 1 filtered integration test selected by "status" also passed
    cargo clippy -p jig-status-tui --all-targets -- -D warnings
    cargo clippy -p jig-sh --all-targets -- -D warnings
    cargo +1.85.0 check -p jig-sh --no-default-features
    cargo package -p jig-status-tui --allow-dirty
    # packaged and verified 11 files
    scripts/jig work check --plan-id plan_01KYJ4SB78458NSJV1K6JEVK5E
    # passed: receipt_01KYJ7FMPTNRWXBJJ7R5745DB6
    # contract: receipt_01KYJ6XV43QMGS8GKFSDMVY9E
    # workspace tests: receipt_01KYJ7FMKRRE56NB8XGR6VARJT
    scripts/jig work gates --plan-id plan_01KYJ4SB78458NSJV1K6JEVK5E
    # passed: both required receipts fresh and covering all changed paths

The hocr2 PTY dogfood at 80x24 displayed:

    hocr2 main@9367b1bc94c4 clean
    provider complete
    130 packages
    25 blockers across 25 packages
    specification: 1 complete, 104 ready, 25 blocked
    implementation and verification: 1 complete, 129 pending
    acceptance: 12 complete, 506 pending
    legacy: current at 389b1fa12473
    target: current at 9367b1bc94c4

The Blockers view selected `WP-ACCT-011` and showed its full message, facet states, and source. A second run received `q` while the provider was active; the shell returned in 0.0359 seconds, the alternate screen and cursor were restored, and `pgrep` found no matching `verify_migration_readiness.rb ... status-provider-v1` descendant.

## Interfaces and Dependencies

In `crates/jig/src/cli/status_opts.rs`, define a Clap `StatusOpts` with `tui: bool` and `refresh_seconds: Option<u64>`. The effective interval is 30 seconds when TUI mode is selected without an override, and accepted overrides are 1 through 3,600 seconds.

In `crates/jig/src/status.rs`, retain:

    pub(crate) fn snapshot(ctx: &RepoContext) -> anyhow::Result<serde_json::Value>

and add:

    pub(crate) fn snapshot_with_cancellation(
        ctx: &RepoContext,
        cancelled: &dyn Fn() -> bool,
    ) -> anyhow::Result<serde_json::Value>

Every provider invocation must call the callback before spawn and while awaiting the owned process tree.

In `crates/jig-status-tui/src/lib.rs`, expose:

    pub trait SnapshotSource: Send + Sync {
        fn snapshot(
            &self,
            cancelled: &dyn Fn() -> bool,
        ) -> Result<serde_json::Value, String>;
    }

    pub fn run(
        source: impl SnapshotSource + 'static,
        refresh_interval: std::time::Duration,
    ) -> anyhow::Result<()>

`crates/jig/src/status/tui.rs` implements `SnapshotSource` for a private adapter that owns `RepoContext`, then delegates to this `run` function. The TUI crate's model, renderer, runtime worker, and terminal guard remain implementation details. The view model accepts aggregate schema version 1, ignores unknown JSON fields, and does not change the public status-provider or aggregate JSON contracts.

Pin these workspace dependencies:

    ratatui = "=0.29.0"
    crossterm = "=0.28.1"

Use the Crossterm backend supplied by that Ratatui line. Do not add an async runtime: one standard thread, channel, and atomic cancellation flag are sufficient and keep the feature independent of Tokio.

Plan revision note (2026-07-27 16:00Z): replaced the initial one-paragraph plan body with a self-contained implementation plan after auditing the existing aggregate, hocr2's real payload and runtime, cancellation facilities, and Rust-compatible terminal dependencies.

Plan revision note (2026-07-27 16:08Z): after the user challenged whether a terminal UI should live inside Jig, moved terminal presentation from the main `jig` crate to a separate CLI-owned `jig-status-tui` workspace crate while retaining the generic `jig status --tui` entrypoint and explicitly preserving the proprietary launcher boundary.

Plan revision note (2026-07-27 16:12Z, superseded 2026-08-13): initially changed the Ratatui pin from 0.29.0 to 0.28.1 to avoid aligning the workspace on `unicode-width` 0.2.0. A reproduced crash on a 608x113 terminal later proved that Ratatui 0.29's large-buffer support is required.

Plan revision note (2026-07-27 16:32Z): pinned Ratatui's `instability` transitive dependency at 0.3.10 after dependency metadata showed that the unconstrained current 0.3.12 release had silently raised the effective Rust floor to 1.88.

Plan revision note (2026-07-27 16:33Z): recorded the completed implementation, targeted verification, hocr2 PTY evidence, state-driven redraw change, and terminal-string sanitization boundary. Full repository gates and plan closure remain.

Plan revision note (2026-07-27 16:43Z): recorded the passing full-work-check receipt and fresh required gates. Only structured closure and the final repository commit remain.

Plan revision note (2026-07-27 16:44Z): recorded successful structured closure after all required evidence remained fresh.

Plan revision note (2026-08-13): superseded the Ratatui 0.28.1 dependency decision with Ratatui 0.29.0 after reproducing the 65,535-cell buffer failure. The workspace now deliberately accepts Ratatui 0.29's `unicode-width` 0.2.0 requirement and Rustyline 17 compatibility dependency while retaining Rust 1.85.
