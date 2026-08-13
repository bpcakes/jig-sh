# jig-tui crate guide

## Purpose

`crates/jig-tui` contains shared terminal lifecycle, safe display-text, and cooperative worker primitives used by Jig's terminal interfaces. It does not own any feature-specific model or rendering.

## Key entrypoints

- `src/lib.rs`: terminal requirement checks, alternate-screen session ownership, actionable key filtering, cancellation tokens, and join-on-drop workers.

## Edit here for X

- Change raw-mode, alternate-screen, cursor, or restoration behavior: `src/lib.rs` in `TerminalSession`.
- Change shared worker cancellation or joining behavior: `src/lib.rs` in `CooperativeWorker`.
- Change terminal-safe text handling: `src/lib.rs` in `sanitize_text`.
- Change status-specific interaction: `crates/jig-status-tui/`.
- Change Codex-picker interaction: `crates/jig-codex-tui/`.
- Change Vault-manager interaction: `crates/jig-vault-tui/`.

## Invariants

- Restore raw mode, alternate-screen state, and cursor visibility on every ordinary return and unwind.
- A `CooperativeWorker` must signal cancellation and join its owned thread before drop returns.
- Keep this crate free of repository, status-provider, Codex, state, process-launch, and MCP policy.
- Feature-specific crates own event mappings and rendering; this crate owns only reusable mechanics.

## Common commands

- `cargo test -p jig-tui`
- `cargo clippy -p jig-tui --all-targets -- -D warnings`
- `cargo test -p jig-status-tui`
- `cargo test -p jig-codex-tui`
- `cargo test -p jig-vault-tui`
