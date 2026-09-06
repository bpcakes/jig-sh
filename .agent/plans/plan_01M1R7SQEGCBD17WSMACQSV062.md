# Cut the unified dashboard into the public CLI

This ExecPlan implements Task F (`jig-sh-l2x.7`) from `docs/plans/unified-terminal-dashboard.md`. It makes the already-proven typed source and serialized terminal runtime authoritative for `jig ui` and `jig status --tui`, adds one-shot recorder/plan JSON, installs the shared signal-session boundary, retains the rejected hidden port diagnostic, and performs the 0.3.0 public cutover while the old web/status implementations remain available for Task G rollback.

Implementation baseline: `574ca639` on branch `jig-sh-l2x`.

## Progress

- [x] Read repository/crate guides and Task F references; claim `jig-sh-l2x.7` and open structured work.
- [x] Replace `UiOpts` with bounded terminal/JSON options and the hidden retired-port shim; add parser, help, conflict, and structured-usage tests.
- [x] Turn `ui.rs` into the thin unified adapter for Work/Status startup, one-shot recorder/plan JSON, shared signal supervision, and exact error classification.
- [x] Route both public entrypoints to `RepoDashboardSource`, preserving status JSON and the still-present rollback implementations until Task G.
- [x] Add integration tests for no listener, exact JSON envelopes/goldens, missing plan, entrypoint equivalence, source phases, signal cleanup, and supported/oversized status compatibility.
- [x] Set every product/release surface to 0.3.0 without changing the generated repository contract epoch; refresh and audit the lockfile.
- [x] Run focused validation and exactly two comprehensive Claude+Codex review/fix rounds.
- [x] Close the Bead before final exact-diff gates, record evidence, finish structured work, and commit Task F.

## Surprises & Discoveries

- Task B2 already implemented `RepoDashboardSource`, provider-first/local-second phases, retained epochs, Fresh detail, and supported-size differential status tests behind dead-code allowances. Task F should expose that existing authority, not build a parallel adapter.
- Task E deliberately exposes `run_with_cancellation`; Task F owns the `DoctorSignalSession` around that call and must retire it only after the dashboard worker has joined and terminal restoration has completed.
- The old `UiServer` adapter and `jig-status-tui` adapter must remain compiled as a rollback boundary through this task. Task G, not F, deletes them and their dependencies.
- Timeline-limit changes are scheduler state, not just view state: an in-flight old-limit status result may update status cards but must not republish stale timeline rows. The runtime now normalizes requeues to the current limit and immediately reprojects safe local shrinks.
- UI JSON failures need the same one-document usage/runtime discipline as other structured commands while retaining `command: "ui"`; a typed command error preserves that identity without weakening the shared reporter.
- Round 1 found that post-output signal retirement could append a second JSON envelope, no-epoch timeline growth could request impossible reuse, and several CLI/test contracts were under-specified. JSON is now fully serialized before output begins, any later error is marked already-emitted, option conflicts are explicit, and live PTY coverage proves initial-plan and signal behavior.
- Round 2 exposed view-limit actions as unintended provider-preemption authority and a failed initial epoch as a stuck plan overlay. Shrink is now strictly model-local, growth reuses an active collection, stale projections recover at the retained epoch, and initial-plan failures become retryable detail errors.

## Decision Log

- Keep CLI routing and signal/error translation in `crates/jig/src/ui.rs`; keep repository traversal and retained-epoch behavior in `ui/source.rs`; keep rendering and scheduling in `jig-ui`.
- Use `DashboardOptions::with_refresh_intervals(Work, 10s, 30s)` for `jig ui`, and `(Status, 10s, status --refresh-seconds)` for the compatibility entrypoint.
- JSON mode directly invokes the typed source once. Recorder uses `RecorderMode::Refresh`; plan uses `PlanBasis::Fresh`. It never enters the terminal or touches the legacy server.
- Keep `--port` parseable but hidden, then reject it in `post_parse_usage_error` so human and JSON failures retain usage status 2.
- Preserve the generated contract version because command names and launcher routing do not change; 0.3.0 is a product/API cutover only.
- The delivery plan resolves the status cadence question explicitly: `status --tui --refresh-seconds` controls provider refresh while its local recorder timer remains the canonical ten seconds. The compatibility adapter now preserves that split.
- Documentation, changelog, and crate-description removal remain Task H scope; Task F keeps the old implementations compiled narrowly as its documented rollback boundary.

## Outcomes & Retrospective

`jig ui` now enters the unified Work-first terminal dashboard, while `jig status --tui` uses the same engine Status-first. Recorder and plan JSON modes emit exact schema-1 roots, the retired port is a hidden usage error, and the workspace reports product version 0.3.0 without changing contract epoch 7. Shared signal supervision restores the terminal and reaps provider processes before redelivering SIGINT/SIGTERM.

Two independent Claude+Codex review rounds were completed and every supported finding was fixed. The final plan-bound evidence is fresh and passing: contract, formatting, strict Clippy, file budget, core (3,177 tests), frontend (112), vault (445), process (209), and aggregate API (3,943) gates. One intermediate core run lost a Nextest child executable during double-spawn; the aggregate suite passed the same tests and the isolated core rerun passed all 3,177, replacing the failed receipt.

## Context and orientation

`crates/jig/src/cli.rs` owns `UiOpts`; `cli/run/argument_parsing.rs` owns semantic usage conflicts; `cli/run.rs` routes `Ui` and `Status`. `crates/jig/src/ui.rs` currently owns the HTTP adapter, while `ui/source.rs` already implements the typed dashboard source. `status/tui.rs` is the obsolete compatibility adapter retained until Task G. `jig-ui::terminal` owns the unified runtime. `DoctorSignalSession` is the existing Unix process-wide signal boundary used by status and other observational commands.

## Plan of work

First cut the CLI model and tests to the final command contract. Then replace dispatch with a small adapter that creates one typed source and chooses JSON or terminal mode. Add the signal-session wrapper once both paths are cancellable. Freeze entrypoint and output behavior with fixture-driven integration tests, update the version surfaces and lockfile, and finally run the complete compatibility and repository gates.

## Validation and acceptance

Parsing must accept bare `ui`, both refresh bounds, plan IDs, timeline limits, and global JSON placement while rejecting invalid/conflicting forms. Hidden `--port` must be absent from help and fail before dispatch with usage status 2. `ui --json` and `ui --plan ... --json` must emit one schema-1 document and exit without a listener/provider run. `ui` and `status --tui` must select Work and Status on the same runtime. Supported status JSON must remain semantically identical; the documented oversized-record partial delta remains exact. Cargo metadata must report 0.3.0 while the repository contract remains v7. All focused tests, strict Clippy, file budget, and plan-bound gates must pass after two review rounds.

## Idempotence and recovery

All source reads are read-only and JSON collection is one-shot. Before Task G, reverting this commit restores the old dispatch because `UiServer`, the legacy status adapter, and dependencies still exist. No persisted state schema or generated launcher contract changes.

## Interfaces and dependencies

No new dependency is expected. Reuse `RepoDashboardSource`, `DashboardSource`, `DashboardOptions`, `InitialTab`, `DoctorSignalSession`, existing JSON emit/error helpers, Clap bounded parsers, and workspace version inheritance.
