# Codex Picker TUI Guide

## Purpose

This crate owns the interactive terminal presentation for choosing an exact Codex home. It does not discover homes, inspect accounts, launch Codex, or read authentication files.

## Key entrypoints

- `src/lib.rs`: same-release boundary supplied by `jig-sh`.
- `src/model.rs`: filtering, selection, and additive inspection decoding.
- `src/render.rs`: Ratatui layout and visual states.
- `src/runtime.rs`: event loop and background inspection ownership.

## Edit here for X

- Picker interaction or keyboard behavior: `src/model.rs` and `src/runtime.rs`.
- List, detail pane, loading, or small-terminal presentation: `src/render.rs`.
- CLI/runtime data boundary: `src/lib.rs`.

## Invariants

- Keep exact `PathBuf` identities separate from lossy, sanitized display text.
- Enter may select a home while its account inspection is still loading.
- Inspection is cooperative: cancel and join the worker before restoring the terminal.
- Do not read `auth.json`; account and usage details arrive only through `InspectionSource`.
- Missing or additive JSON fields render as unknown instead of panicking.

## Common commands

- `cargo test -p jig-codex-tui`
- `cargo clippy -p jig-codex-tui --all-targets -- -D warnings`
