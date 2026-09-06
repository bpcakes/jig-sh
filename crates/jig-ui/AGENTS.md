# jig-ui crate guide

## Purpose

`crates/jig-ui` contains the unified read-only terminal dashboard used by `jig ui` and `jig status --tui`. The owning `jig-sh` CLI supplies typed recorder, plan, and status snapshots through the `DashboardSource` boundary.

## Key entrypoints

- `src/lib.rs`: public dashboard and terminal module boundary.
- `src/dashboard.rs`: bounded snapshot contracts and the typed source interface.
- `src/terminal.rs`: dashboard options and public TUI entrypoint.
- `src/terminal/model.rs`: four-tab application state and typed view projection.
- `src/terminal/render.rs`: Ratatui layout, widgets, colors, and responsive presentation.
- `src/terminal/runtime.rs`: terminal lifecycle, scheduling, refresh workers, and cancellation.

## Edit here for X

- Change recorder/status/plan wire contracts or bounds: `src/dashboard/`.
- Change tabs, navigation, selection preservation, filters, or detail state: `src/terminal/model/`.
- Change terminal layout or presentation: `src/terminal/render/`.
- Change refresh timing, preemption, keyboard events, or terminal cleanup: `src/terminal/runtime/`.
- Change how repository state, gates, or loops become snapshots: `crates/jig/src/ui/source/`, not this crate.

## Invariants

- Keep this crate independent from `RepoContext`, state storage, runtime policy, MCP, and templates.
- Consume repository data only through `DashboardSource`; do not read `.agent/state` directly.
- Keep every rendered collection and text field within its declared bound, preserving explicit omission counts.
- Keep the dashboard read-only. It must not mutate state, record receipts, fetch remotes, or execute displayed remediation commands.
- Never overlap refresh workers. Cancellation must reach the typed source, and the worker must be joined before terminal restoration.
- Restore raw mode, alternate-screen state, and cursor visibility on every ordinary return or unwind.
- Use explicit text in addition to color for all statuses.
- `jig-ui` is a CLI-owned internal crate; its public API is versioned with the matching `jig-sh` release rather than as a stable third-party integration surface.

## Common commands

- `cargo test -p jig-ui`
- `cargo clippy -p jig-ui --all-targets -- -D warnings`
- `cargo test -p jig-sh --test ui_cutover`
- `cargo test --workspace`
