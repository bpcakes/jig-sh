# Home Picker TUI Guide

## Purpose

This crate owns the shared interactive home picker used by Codex and Claude. Codex and Claude supply background account and usage inspections; Claude also supplies configuration-mode details. It does not discover homes, inspect accounts, launch agents, or read authentication files.

## Key entrypoints

- `src/lib.rs`: provider selection accepts an explicit primary subscription bucket and optional inspection source; legacy Codex/configuration entrypoints remain compatibility wrappers. This is a same-release boundary supplied by `jig-sh`; configuration selection returns the original entry index so modes sharing a path remain distinct.
- `src/model.rs`: home rows and additive inspection decoding; `src/model/app.rs` owns filtering and selection, and `src/model/configuration.rs` prepares static or inspected configuration entries.
- `src/render.rs`: Ratatui widgets and visual states; `src/render/layout.rs` owns every breakpoint and decides which panes are visible and which list style fits.
- `src/runtime.rs`: event loop and background inspection ownership.
- `src/usage.rs`: normalized quota validation, remaining calculation, and duration labels shared with the matching CLI release.

## Edit here for X

- Picker interaction or keyboard behavior: `src/model/app.rs` and `src/runtime.rs`.
- List, detail pane, loading, or small-terminal presentation: `src/render.rs`; the configuration list is in `src/render/configuration.rs`.
- Breakpoints, pane arrangement, or pane sizes: `src/render/layout.rs`.
- CLI/runtime data boundary: `src/lib.rs`.

## Invariants

- Keep exact `PathBuf` identities separate from lossy, sanitized display text.
- Enter may select a home while its account inspection is still loading.
- Inspection is cooperative: cancel and join the worker before restoring the terminal.
- Do not read authentication files or the Keychain; account and usage details arrive only through `InspectionSource`.
- Missing or additive JSON fields render as unknown instead of panicking.
- Layout tiers are monotonic: widening the terminal never selects a poorer list style, and side-by-side details keep their readable width range.

## Common commands

- `cargo test -p jig-codex-tui`
- `cargo clippy -p jig-codex-tui --all-targets -- -D warnings`
