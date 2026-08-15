# Replace the Codex numbered prompt with a responsive native TUI

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while implementation proceeds. Maintain this file according to `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

The existing `jig codex launch` interactive path is a static numbered prompt that appears only after every discovered Codex home has finished account and usage inspection. It does not replace the experience of `/Users/aa/Documents/codex-launcher.sh`, whose `fzf` interface opens immediately, supports keyboard navigation and search, highlights one home, and shows detailed usage in a preview pane.

After this change, running `scripts/jig codex launch` in a terminal opens a full-screen native terminal user interface immediately after inexpensive directory discovery. Every home starts in a visible loading state while the existing bounded four-worker app-server inspection runs in the background. Account and usage results update rows as they arrive without blocking navigation. The operator can move with arrow keys or `j`/`k`, search with `/`, inspect the selected home in a detail pane, press Enter to launch it even while usage is still loading, or cancel with Escape, `q`, or Ctrl-C. Jig continues to obtain account data through `codex app-server`; it must not read `auth.json`.

## Progress

- [x] (2026-08-02 12:49Z) Compared the old Bash/Python/fzf launcher with the current numbered Jig picker and identified the missing interaction contract.
- [x] (2026-08-02 12:49Z) Inspected the existing `jig-status-tui` Ratatui/Crossterm runtime, terminal restoration, cancellation worker, and test-backend patterns.
- [x] (2026-08-02 14:02Z) Extracted reusable terminal and cooperative-worker primitives into `jig-tui`, migrated `jig-status-tui` to them, and added repository guidance.
- [x] (2026-08-02 14:02Z) Added the dedicated `jig-codex-tui` presentation crate on the shared foundation.
- [x] (2026-08-02 14:02Z) Exposed inexpensive discovered homes and stable indexed streaming inspection updates from `crates/jig/src/codex.rs` without losing exact paths.
- [x] (2026-08-02 14:02Z) Implemented the responsive picker model, wide and compact rendering, fuzzy filtering, navigation, focusable/scrollable detail pane, loading animation, and cancellation.
- [x] (2026-08-02 14:02Z) Replaced the numbered prompt in `crates/jig/src/cli/codex_run.rs` and removed obsolete picker formatting and selection code.
- [x] (2026-08-02 14:02Z) Added model/render/runtime coverage and a pseudo-terminal end-to-end test proving the interface appears before inspection completes and launches the searched exact home.
- [x] (2026-08-02 14:02Z) Updated CLI help, configuration documentation, crate maps, and this plan with the implemented behavior.
- [x] (2026-08-02 14:25Z) Built the development binary; focused TUI/Codex and PTY tests, strict Clippy, formatting, contract, and diff checks passed. The repository test gate recorded 1,294 passed and 2 ignored with only the four Windows dependency-checker failures explicitly excluded by the user.
- [x] (2026-08-02 14:38Z) Resolved the comprehensive-review findings: added signal-supervised picker cancellation, ordered final updates before completion, bounded detail scrolling, restored panic diagnostics, unified single-window weekly labeling, and made PTY EOF/restoration coverage portable and deterministic.
- [x] (2026-08-02 15:38Z) Resolved the follow-up UX review: derived detail scrolling from wrapped viewport rows, distinguished stopped inspection from successful completion, ranked home-name search ahead of shared paths, sanitized Unicode format controls, required explicit PTY-test exemptions, and routed account-semantics tests through production report assembly.
- [x] (2026-08-02 16:39Z) Resolved the final review findings: synchronized inspection updates and rendering before input, surfaced nonfatal discovery warnings, retained long-list viewport context, preserved defensive worker diagnostics, kept legitimate Unicode joiners, and corrected the review chronology.
- [x] (2026-08-13) Upgraded the shared terminal stack to Ratatui 0.29.0 and added a 608x113 PTY regression after reproducing Ratatui 0.28.1's 65,535-cell buffer failure.

## Surprises & Discoveries

- Observation: The old launcher does not block its interface on usage. It gives `fzf` inexpensive rows first, then runs a selected-home `preview-limits` process in the preview pane.
  Evidence: `/Users/aa/Documents/codex-launcher.sh` invokes `fzf --preview ... preview-limits {8}` while the initial rows contain `...` usage placeholders.

- Observation: Jig pins compatible Ratatui and Crossterm versions and has a robust alternate-screen runtime in a separate internal presentation crate.
  Evidence: the workspace pins `ratatui = 0.29.0` for large-terminal buffer support and `crossterm = 0.28.1`; `crates/jig-status-tui/src/runtime.rs` restores raw mode, cursor visibility, and alternate-screen state through `Drop`.

- Observation: The existing inspection callback does not expose the completed row index, which a live TUI needs because completion order differs from display order and non-UTF-8 display names can collide.
  Evidence: `homes_report_with_progress` currently reports only `(completed, total, home_json)` even though `inspect_homes_parallel` internally receives the exact `index`.

- Observation: `jig-status-tui` contains sound terminal lifecycle and cancellation-worker code, but those primitives are private inside a status-specific crate and cannot be reused directly by a Codex picker.
  Evidence: `TerminalSession`, `RefreshWorker`, `require_terminal`, and `is_actionable_key` are private in `crates/jig-status-tui/src/runtime.rs`; the crate's only public runtime entrypoint accepts a status `SnapshotSource`.

- Observation: A pseudo-terminal test must continuously drain terminal output while driving a live TUI; otherwise frequent spinner redraws can fill the PTY buffer and block the event loop before it consumes Enter.
  Evidence: the initial end-to-end test observed `/work` rendering but stalled until its wait loop was changed to drain the master side continuously.

- Observation: A final launched-process stub that exits immediately can close a macOS PTY before unread terminal-restoration bytes are observed, making the restoration assertion timing-dependent even though the picker restored before `exec`.
  Evidence: the launch and exact-home assertions passed while the post-exit `LeaveAlternateScreen` read intermittently missed the escape; keeping the launched stub alive while asserting restoration made the full PTY suite pass twice consecutively.

## Decision Log

- Decision: Build a native Ratatui interface instead of shelling out to `fzf`.
  Rationale: The repository already owns a Crossterm/Ratatui stack, a native interface avoids a new executable prerequisite, and exact `PathBuf` identities can remain in memory rather than crossing tab-delimited text.
  Date/Author: 2026-08-02 / Codex

- Decision: Extract `crates/jig-tui` for reusable terminal lifecycle and cooperative worker ownership, then add `crates/jig-codex-tui` rather than putting Ratatui code into `crates/jig` or coupling Codex behavior to `jig-status-tui`.
  Rationale: `jig-status-tui` is explicitly status-dashboard-specific, but copying its private `TerminalSession` and worker lifecycle would create two subtly different foundations. `jig-tui` will own the reusable raw-mode/alternate-screen restoration, terminal requirement, key-event filtering, cancellation token, and join-on-drop worker. Status and Codex remain separate presentation crates on that shared base.
  Date/Author: 2026-08-02 / Codex

- Decision: Discover homes before entering the TUI, then inspect every discovered home in the existing rolling four-worker pool while the TUI remains interactive.
  Rationale: Directory discovery is fast and supplies the complete stable row list. Inspecting all rows in the background provides richer list status than the old selected-only preview while retaining the essential property that usage never blocks the interface.
  Date/Author: 2026-08-02 / Codex

- Decision: Enter selects and launches the highlighted exact path regardless of whether its inspection has completed.
  Rationale: Account and usage are advisory selection context, not prerequisites for setting `CODEX_HOME`. This matches the old launcher's non-blocking usage preview and prevents a slow or malformed app server from trapping the user in the picker.
  Date/Author: 2026-08-02 / Codex

- Decision: Keep the current non-interactive `jig codex homes` report unchanged apart from shared indexed progress plumbing.
  Rationale: The complaint concerns the interactive replacement. JSON and human list output remain useful automation surfaces and should not acquire terminal behavior.
  Date/Author: 2026-08-02 / Codex

- Decision: Do not create intermediate commits in the current dirty worktree.
  Rationale: The entire launcher feature and its structured-work records are already uncommitted, and the user did not authorize commits. Verification and the ExecPlan provide restartable checkpoints without risking an accidental mixed commit.
  Date/Author: 2026-08-02 / Codex

- Decision: Keep one-window Codex usage labeling count-based and use duration-aware roles only when multiple windows are returned.
  Rationale: The operator's explicit contract is that the sole current window is weekly; if the five-hour limit returns alongside it, server-reported durations distinguish the two without imposing roles on unexpected additional durations.
  Date/Author: 2026-08-02 / Codex

- Decision: Pin the shared terminal stack to Ratatui 0.29.0 while retaining Crossterm 0.28.1 and the workspace Rust 1.85 floor.
  Rationale: Ratatui 0.29 replaces the 16-bit buffer-area calculation that crashes the picker on a 608x113 terminal. Its exact `unicode-width` 0.2.0 requirement and the compatible Rustyline 17 dependency are accepted consequences of supporting the full terminal without a viewport cap.
  Date/Author: 2026-08-13 / Codex

## Outcomes & Retrospective

The implementation now opens a visibly responsive full-screen picker before account inspection completes, preserves exact `PathBuf` selection, supports ranked fuzzy search and viewport-correct focusable usage details, cancels active app-server checks before launch, and removes the old numbered prompt. Inspection updates are rendered before input can act on them, long lists retain surrounding context, partial home discovery remains visible as a nonfatal warning, and legitimate Unicode script joiners survive terminal hardening. External Unix termination signals cancel and join active app-server inspections, restore the terminal, and only then re-deliver the signal. The terminal lifecycle and cooperative worker implementation are shared with the existing status dashboard instead of copied. Final validation passes 19 Codex TUI tests, 76 Codex-focused Jig tests, 12 status TUI tests, 5 shared TUI tests, and all 3 Unix launcher PTY/signal tests; PTY unavailability is a failure unless the environment explicitly opts out with `JIG_ALLOW_PTY_TEST_SKIP=1`. Strict Clippy, formatting, contract, guide/map, and diff checks pass. The full workspace gate recorded 1,295 passing and 2 ignored tests; its only four failures were the Windows dependency-checker baseline explicitly excluded by the user. Because the structured gate cannot encode that platform exclusion, the plan remains mechanically blocked in `jig work gates` even though the scoped implementation and accepted repository evidence are complete.

The later Ratatui 0.29.0 cutover removes the picker's 65,535-cell ceiling. A PTY regression now covers the reported 608x113 terminal while preserving the full terminal area rather than imposing a viewport workaround.

## Context and Orientation

The repository root is `/Users/aa/Documents/jig-sh`. `crates/jig/src/cli/codex_run.rs` currently owns interactive selection. Its `select_home_interactively` function checks for a terminal, calls `crate::codex::homes_report_with_progress(true, ...)`, prints progress to stderr, prints a numbered summary, and reads a line. This entire numbered path should be retired.

`crates/jig/src/codex.rs` owns Codex-home discovery, exact path identities, account normalization, parallel inspection, and launching. `inspect_homes_parallel` already uses a rolling pool of at most four scoped worker threads and delivers `(index, result)` internally. `crates/jig/src/codex/app_server.rs` owns the bounded app-server protocol and observes a cooperative cancellation callback. The new TUI must reuse those paths rather than implementing a second protocol client.

`crates/jig-status-tui` is an internal library that demonstrates the repository's preferred terminal architecture: the CLI supplies data through a narrow trait, while a presentation crate owns the model, renderer, event loop, background worker, and terminal cleanup. The new `crates/jig-codex-tui` crate should follow that structure but must not depend on `jig-sh`, `RepoContext`, state files, or app-server commands. A “TUI” here means a full-screen terminal application rendered through Ratatui in Crossterm's alternate screen and raw input mode.

`crates/jig/tests/codex_launcher.rs` contains Unix pseudo-terminal integration tests. Extend it to prove actual interactive behavior. The test stub must distinguish `codex app-server` inspection invocations from the final Codex launch, hold inspection behind a marker file, and record the launched `CODEX_HOME` so the test can verify selection.

## Plan of Work

First add `crates/jig-tui` to the workspace. Move the reusable behavior from `crates/jig-status-tui/src/runtime.rs` into public same-release primitives: `TerminalSession` for raw mode, alternate screen, cursor, drawing, and restoration; `require_terminal` for consistent redirect diagnostics; `is_actionable_key`; `CancellationToken`; and a generic `CooperativeWorker<T>` that delivers one final result, can be polled without blocking, and always cancels and joins on drop. Migrate `jig-status-tui` to these primitives and keep its existing tests green.

Then add `crates/jig-codex-tui` to the workspace and to `crates/jig` as an internal dependency. Give both new crates an `AGENTS.md` with the standard purpose, entrypoint, edit map, invariants, and commands. Define a small public Codex TUI boundary in `src/lib.rs`: an exact-path `Home` row, a `HomeUpdate` carrying the stable row index plus normalized JSON details, an `InspectionSource` trait whose implementation observes cooperative cancellation and emits updates, and a `select` function returning `Option<PathBuf>`. Keep Codex terminal-specific state private while relying on `jig-tui` for lifecycle.

Implement a model with one row per discovered home, exact `PathBuf` identity, loading/ready/error inspection state, stable selection, a case-insensitive filter string, and a visible subset computed from profile name, email, plan, status, and path. Updates identify rows by index rather than display text. Navigation wraps or clamps predictably; selection remains valid when filtering changes. Enter returns the selected path even while loading. Search mode accepts printable characters and Backspace, Enter launches the current result, and Escape leaves search before a second Escape cancels.

Implement Ratatui rendering with a header showing completed and total inspections plus an animated loading indicator, a highlighted table/list of homes, a selected-home detail pane, and a footer listing keys. The list must expose current-home state, name, account, plan, weekly usage, and inspection state with explicit text in addition to color. The detail pane must show the exact display path, account type/email/plan, all normalized rate-limit buckets and windows, resets, and inspection or usage errors. On narrow terminals, stack list and details vertically or prioritize the list while keeping controls visible. Sanitize control characters before rendering untrusted app-server text.

Implement the Crossterm runtime by adapting the proven patterns in `crates/jig-status-tui/src/runtime.rs`: require terminal stdin and stdout, enter raw mode and the alternate screen, hide the cursor outside search mode, poll events at a short interval, drain background updates, redraw only when state or spinner changes, and restore the terminal through `Drop` on every ordinary error or unwind. A single inspection worker calls the supplied `InspectionSource`; quitting sets an atomic cancellation flag and joins the worker before terminal restoration. Ctrl-C must be treated as a TUI cancel key in raw mode.

Refactor `crates/jig/src/codex.rs` to expose a crate-private discovered inspection set containing exact paths, current-home identity, discovery errors, and the configured Codex executable. Add candidate accessors for initial TUI rows and an inspection method that reuses `homes_report_from_discovered`. Thread the internal completed row index through progress callbacks. Preserve the existing `homes_report` output and signal-supervised non-interactive behavior.

In `crates/jig/src/cli/codex_run.rs`, construct a Jig-owned implementation of `jig_codex_tui::InspectionSource` around the discovered inspection set, map indexed JSON results into `HomeUpdate`, run the TUI, then revalidate and launch the returned exact path. Delete `resolve_picker_selection`, line-oriented prompting, and obsolete picker/progress formatters. Preserve the early `--json` rejection, explicit-home behavior, dry runs, and exact Codex arguments.

Finally update CLI help and `docs/configuration.md` to describe immediate loading, search/navigation keys, background usage, Enter behavior, and cancellation. Update `agent-map.md` for the new crate guide. Keep `docs/public-contract.md` focused on non-interactive JSON because the TUI is not a stable machine-readable contract.

## Concrete Steps

Work from `/Users/aa/Documents/jig-sh`.

1. Add `jig-tui` and `jig-codex-tui` workspace/dependency wiring in `Cargo.toml`, `crates/jig/Cargo.toml`, and the new crate manifests. Create both guides. Add `jig-tui/src/lib.rs`, migrate `jig-status-tui/src/runtime.rs`, and run:

       cargo test -p jig-tui
       cargo test -p jig-status-tui

   Then create `jig-codex-tui/src/{lib,model,render,runtime}.rs`.

2. Refactor indexed discovery and inspection in `crates/jig/src/codex.rs`, keeping existing unit tests green after each change:

       cargo test -p jig-sh codex --lib

3. Implement model and TestBackend rendering tests in `jig-codex-tui`:

       cargo test -p jig-codex-tui

   Expected behavior includes a loading row before any update, immediate search/navigation, detailed weekly usage after an update, exact-path selection, safe control-character rendering, and an empty-filter/no-match state.

4. Replace `select_home_interactively` and update the pseudo-terminal integration test:

       cargo test -p jig-sh --test codex_launcher -- --nocapture

   The test must observe the TUI title before releasing a blocked app-server, then search for a non-default home, press Enter, and confirm the final Codex process received that home's exact `CODEX_HOME`.

5. Run formatting and strict focused linting:

       cargo fmt --all
       cargo clippy -p jig-codex-tui --all-targets -- -D warnings
       cargo clippy -p jig-sh --all-targets -- -D warnings
       git diff --check

6. Build the current runtime and exercise repository-owned checks rather than the cached launcher:

       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
       JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract
       JIG_DEV_BIN=target/debug/jig scripts/jig check test

The existing four `bootstrap::tests::windows_dependency_checker` failures are an explicitly excluded baseline on this macOS host. Any other failure must be rerun in isolation and diagnosed before completion.

## Validation and Acceptance

The implementation is accepted when `scripts/jig codex launch` opens a full-screen interface before a deliberately blocked app-server returns. The discovered home names are immediately visible with loading text. Arrow keys and `j`/`k` move the highlight; `/` filters; Backspace edits; Escape first leaves search and then cancels; `q` and Ctrl-C cancel; Enter exits the TUI and launches the exact highlighted `CODEX_HOME`. Results arriving during interaction update account, plan, weekly usage, reset information, and errors without moving the operator's selection unexpectedly.

Cancellation must terminate every active app-server process and restore raw mode, cursor visibility, and the original terminal screen before returning. Selecting while loading must cancel remaining inspections cleanly, restore the terminal, revalidate the selected directory, and launch Codex. The pseudo-terminal integration test provides automated evidence for early visibility and exact selection; the TUI crate's TestBackend snapshots provide deterministic evidence for layout and detail content.

Non-interactive behavior must remain stable: `jig codex homes`, `jig codex homes --usage --json`, explicit `jig codex launch HOME`, dry-run output, forwarded arguments, and child exit status continue to work. Focused Codex tests, both launcher integration tests, the new TUI tests, strict Clippy, contract checks, and the repository test gate must produce the expected results described above.

## Idempotence and Recovery

All source edits and tests are repeatable. Cargo may safely rebuild the new crate. The TUI owns no durable state and writes no account data. If terminal initialization fails after raw mode or alternate-screen entry, the terminal-session constructor must restore every state it changed before returning the error. If rendering, input polling, inspection, or selection fails, dropping the terminal session and inspection worker must restore the terminal and clean up app-server descendants.

If implementation is interrupted, resume from the first unchecked `Progress` item and inspect `git diff` before editing; do not discard the existing uncommitted launcher work. Do not use destructive Git commands. If a test leaves a stub process after a panic, terminate only the PID recorded in that test's temporary directory.

## Artifacts and Notes

The pre-change interactive path has this observable sequence:

    Inspecting Codex accounts and usage (0/N)...
    [1/N] ...
    Codex homes: N found
    Select a Codex home [1-N], or q to cancel:

The required post-change sequence is a single alternate-screen interface visible immediately with rows marked `loading`, followed by in-place account and usage updates. No scrolling progress transcript or numbered `read_line` prompt should remain.

## Interfaces and Dependencies

In `crates/jig-codex-tui/src/lib.rs`, provide these semantic interfaces, with exact field names adjusted only when Rust ownership requires it:

    pub struct Home {
        pub path: PathBuf,
        pub name: String,
        pub current: bool,
    }

    pub struct HomeUpdate {
        pub index: usize,
        pub details: serde_json::Value,
    }

    pub trait InspectionSource: Send + Sync {
        fn inspect(
            &self,
            emit: &mut dyn FnMut(HomeUpdate) -> Result<(), String>,
            cancelled: &(dyn Fn() -> bool + Sync),
        ) -> Result<(), String>;
    }

    pub fn select(
        homes: Vec<Home>,
        source: impl InspectionSource + 'static,
    ) -> anyhow::Result<Option<PathBuf>>;

In `crates/jig-tui/src/lib.rs`, provide `TerminalSession`, `require_terminal`, `is_actionable_key`, `CancellationToken`, and `CooperativeWorker<T>`. `TerminalSession::enter` must unwind partial setup and its `Drop` must restore cursor visibility, leave the alternate screen, and disable raw mode. `CooperativeWorker<T>` must spawn one named thread, expose a nonblocking `try_finish`, and cancel and join before drop.

`jig-tui` depends on `anyhow`, `crossterm`, and `ratatui`. `jig-status-tui` and `jig-codex-tui` depend on it while retaining their own direct rendering/event dependencies. `jig-codex-tui` additionally depends on `serde_json` and must not depend on `jig-sh`. `crates/jig` depends on `jig-codex-tui` and implements `InspectionSource` using its existing Codex discovery and app-server code. The normalized JSON passed in `HomeUpdate.details` remains an internal same-release boundary and must be decoded additively, with missing or malformed fields rendered as unknown rather than panicking.

Revision note (2026-08-02): Created this plan after confirming that the initial Jig implementation replaced only launcher mechanics, not the old launcher's immediate searchable interface and live preview experience.

Revision note (2026-08-02): Replaced the initial copy-the-pattern approach with a shared `jig-tui` foundation after the user explicitly challenged whether existing TUI foundations had been considered. This keeps terminal cleanup and cooperative worker ownership single-sourced.

Revision note (2026-08-02): Recorded the completed implementation and focused evidence, including compact layouts, fuzzy search, a focusable detail pane, and the PTY-draining requirement discovered during end-to-end testing.

Revision note (2026-08-02): Finalized validation results and documented why the structured gate remains mechanically blocked on the user-excluded Windows baseline.

Revision note (2026-08-13): Recorded Ratatui 0.29.0 as the decided shared terminal dependency and the exact 608x113 large-terminal regression that superseded the earlier 0.28.1 assumption.
