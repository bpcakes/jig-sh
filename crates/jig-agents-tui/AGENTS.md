# Home Picker TUI Guide

## Purpose

This crate owns the shared interactive home picker used by Codex and Claude. Codex and Claude supply background account and usage inspections; Claude also supplies configuration-mode details. It does not discover homes, inspect accounts, launch agents, or read authentication files.

## Key entrypoints

- `src/lib.rs`: `select_provider_with_cancellation` is the only entrypoint; callers supply the title, an explicit primary subscription bucket, and an optional inspection source. This is a same-release boundary supplied by `jig-sh`; configuration selection returns the original entry index so modes sharing a path remain distinct.
- `src/model.rs`: home rows; `src/model/inspection.rs` decodes additive inspection data, `src/model/app.rs` owns filtering and selection, and `src/model/configuration.rs` prepares static or inspected configuration entries.
- `src/render.rs`: frame entrypoint, header, and shared pane and status styles. `src/render/layout.rs` owns every breakpoint and decides which panes are visible and which list style fits; `src/render/fleet.rs` draws the global quota and forecast panel; `src/render/list.rs` composes the list rows, `src/render/details.rs` builds the selected-home pane, `src/render/wrap.rs` wraps its lines, `src/render/meter.rs` draws usage meters, `src/render/theme.rs` owns the palette and its color-depth fallbacks, and `src/render/footer.rs` fits key hints to the width.
- `src/model/gauge.rs`: per-window meter data (used, elapsed, projected) derived from the same sample as the projection.
- `src/model/fleet.rs`: provider-neutral collective quota outcomes and coverage; `src/model/fleet/cohort.rs` assembles comparable account pools, and `src/model/fleet/simulation.rs` owns the pure reset-aware engine.
- `src/model/app/fleet_view.rs`: cached global assessment and independent explanation scrolling; `src/render/fleet/details.rs` renders the complete explanation, and `src/runtime/fleet.rs` routes its controls before home or search actions.
- `examples/picker_preview.rs`: the real picker with example homes and simulated inspection, for presentation work (`cargo run -p jig-agents-tui --example picker_preview`).
- `src/runtime.rs`: event loop and background inspection ownership; `src/runtime/mouse.rs` maps clicks and the wheel to actions using the areas the last frame recorded (`App::hit_areas`).
- `src/usage.rs`: normalized quota validation, remaining calculation, and duration labels shared with the matching CLI release.

## Edit here for X

- Picker interaction or keyboard behavior: `src/model/app.rs` and `src/runtime.rs`; mouse behavior in `src/runtime/mouse.rs`.
- List rows, badges, and columns: `src/render/list.rs`, including the static configuration list.
- Meter glyphs or the palette: `src/render/meter.rs` and `src/render/theme.rs`.
- Detail pane content, order, or wrapping: `src/render/details.rs`.
- Footer key hints and search prompt: `src/render/footer.rs`.
- Global fleet panel: `src/render/fleet.rs`. Header and loading presentation: `src/render.rs`; panel sizing and small-terminal layout: `src/render/layout.rs`.
- Breakpoints, pane arrangement, or pane sizes: `src/render/layout.rs`.
- CLI/runtime data boundary: `src/lib.rs`.
- Collective quota outcomes and assumptions: `src/model/fleet.rs`; eligibility and sample classification: `src/model/fleet/cohort.rs`.

## Invariants

- Keep exact `PathBuf` identities separate from lossy, sanitized display text.
- Enter may select a home while its account inspection is still loading.
- Inspection is cooperative: cancel and join the worker before restoring the terminal.
- Do not read authentication files or the Keychain; account and usage details arrive only through `InspectionSource`.
- Missing or additive JSON fields render as unknown instead of panicking.
- Enable the fleet forecast for every inspected picker whose provider names a primary subscription bucket, and pool only that bucket's windows; configuration views and pickers without a bucket keep the per-account view. Fleet text names no provider, and mentions banked resets only when an included account reports them. Forecast every discovered row, independent of search or selection, and invalidate its cache when inspection or refresh changes the cohort. Unknown capacity, identity, or window timing stays visibly unsupported or partial.
- A provider whose plans do not identify quota size states the subscription bucket's `capacity_tier`; when present it alone decides capacity, so a null tier is unknown capacity. Otherwise the account plan, then the bucket plan, decides. A window without a reset joins the forecast only when its provider marks it `"started": false` with zero usage; it then holds a full allowance whose period begins when work first charges it. Any other missing, null, or unreadable reset stays incomplete.
- Fleet quota meters use the same aligned, distinct account pools as the simulation. Their fill is pooled used quota; shade is aggregate estimated consumption over one full window, normalized by pooled capacity. Staggered resets have no shared elapsed-time tick, and warmup withholds the shaded estimate. Usable-account counts require quota in every window.
- Sample alignment advances an account's windows together under the same all-windows usability constraint as simulation. A warming pace spends no observed quota during alignment but still contributes to pooled demand.
- Current fleet blockage and recovery do not depend on measurable pace, including at zero elapsed window time. Keep complete forecast qualifications accessible with F1, `?` outside search, or a fleet-panel click at every supported size; its global view preserves home selection and search and never launches a home.
- Color only reinforces text: every status, stale sample, and outcome is also written out, and `NO_COLOR` keeps the picker usable. Use glyphs the common Nerd Fonts include (check before adding one; `★`, `✗`, and `⚠` are missing from Fira Code).
- Panes stack whenever the details keep a comfortable height below the list; side by side is only for terminals too short for that, and keeps the details within their readable width range. The arrangement depends on the terminal size and the number of homes, never on the selection or the search.

## Common commands

- `cargo test -p jig-agents-tui`
- `cargo clippy -p jig-agents-tui --all-targets -- -D warnings`
