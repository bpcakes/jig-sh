# jig-status-tui crate guide

## Purpose

`crates/jig-status-tui` contains the read-only terminal dashboard used by `jig status --tui`. The owning `jig-sh` CLI supplies versioned aggregate snapshots through the `SnapshotSource` boundary.

## Key entrypoints

- `src/lib.rs`: snapshot-source contract and public TUI entrypoint.
- `src/model.rs`: additive-field-tolerant aggregate view model and navigation state.
- `src/model/package_detail.rs`: package-detail DTOs, modal state, and scroll bounds.
- `src/render.rs`: Ratatui layout, widgets, colors, and compact terminal behavior.
- `src/render/package_detail.rs`: full package-detail presentation and generic extension rendering.
- `src/runtime.rs`: terminal lifecycle, keyboard events, refresh worker, and cancellation.

## Edit here for X

- Change status tabs, navigation, selection preservation, or filters: `src/model.rs`.
- Change terminal layout or presentation: `src/render.rs`.
- Change refresh timing, keyboard bindings, or terminal cleanup: `src/runtime.rs`.
- Change how providers run or how repository state becomes aggregate JSON: `crates/jig/src/status.rs`, not this crate.
- Change the public provider wire protocol: `crates/jig-contract/src/status_provider/`.

## Invariants

- Keep this crate independent from `RepoContext`, provider commands, state storage, runtime policy, MCP, and templates.
- Consume repository data only through `SnapshotSource`; do not read the repository or `.agent/state` directly.
- Treat aggregate schema version 1 as input and ignore unknown additive fields.
- Keep the dashboard read-only. It must not fetch remotes, cache reports, record receipts, or launch agents.
- Never overlap refresh workers. Cancellation must reach the snapshot source, and the worker must be joined before terminal restoration.
- Restore raw mode, alternate-screen state, and cursor visibility on every ordinary return or unwind.
- Use explicit text in addition to color for all statuses.
- `jig-status-tui` is a CLI-owned internal crate; its public API is versioned with the matching `jig-sh` release rather than as a stable third-party integration surface.

## Common commands

- `cargo test -p jig-status-tui`
- `cargo clippy -p jig-status-tui --all-targets -- -D warnings`
- `cargo test -p jig-sh status`
- `cargo test --workspace`
