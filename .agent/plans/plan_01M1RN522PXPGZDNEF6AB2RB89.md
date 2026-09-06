# Migrate dashboard documentation to terminal-only behavior

This ExecPlan implements Task H (`jig-sh-l2x.9`) from `docs/plans/unified-terminal-dashboard.md`. Tasks A through G have completed the runtime cutover and deleted the retired browser transport. This task makes every maintained user and contributor document describe the shipped terminal dashboard and its one-shot JSON recorders, without changing runtime behavior, generated launcher templates, or the repository contract epoch.

Implementation baseline: `0be7cfc4ff21717f3cf525fdcceb19b7e49eca15` on branch `jig-sh-l2x`.

## Progress

- [x] Read repository guidance, the complete delivery plan, Task H acceptance criteria, and current UI/CLI crate guides; claim the Bead and open structured work.
- [x] Inventory maintained docs, examples, help assertions, diagrams/screenshots, guides, and release notes for browser/server-era claims.
- [x] Update public and contributor documentation to the terminal-only workflow, exact recorder contracts, navigation, refresh behavior, sizing, errors, and migration timeline.
- [x] Regenerate or validate managed guides/maps and run focused help, link, example, and stale-claim checks.
- [x] Run exactly two comprehensive Claude+Codex review/fix rounds over the working tree.
- [x] Close the Bead, pass final exact-diff gates, record evidence, finish structured work, and prepare the isolated documentation commit.

## Surprises & Discoveries

- No maintained dashboard screenshots or diagrams exist, so there are no binary assets to replace or delete.
- The source-owned crate guides and `agent-map.md` already describe the final topology from Task G; Task H validates them instead of creating a second documentation-only edit.
- Browser-related text in `docs/configuration.md` and `docs/platform-support.md` belongs to the separate development proxy and generated browser-E2E workflow. The stale-claim audit must be dashboard-specific rather than banning generic HTTP, browser, port, or loopback vocabulary repository-wide.
- Exact behavior was derived from CLI help, integration tests, schema field constants, limit specifications, responsive-layout constants, and the current binary. No runtime discrepancy requires routing back to a predecessor task.
- `docs/plans/unified-terminal-dashboard.md` deliberately retains browser-era statements as the historical before-state and delivery rationale. Current-behavior stale-claim checks exclude plan artifacts while covering user docs, architecture intent, crate guides, and release notes.
- Review round one found documentation-only keymap drift: top-level `h`/`l` was not implemented, the Status tab retained its old Overview name, and tab-scoped actions were described too broadly. The fix now follows the runtime match arms and the regression test derives schema fields, limit identifiers/ceilings, and error scopes from exported model constants.
- Review round two found that provider laziness needed an explicit `R` exception, the delivery plan's 1 MiB legacy-record safety tightening and published-crate retirement were missing from release notes, and whole-document token tests did not bind exact schema relationships. The final fix documents status collection's fresh local epoch, exact JSON wire/error/limit contracts and partial exits, records both breaking changes, and pins ordered field sets, identifier/ceiling pairs, error registries, anchors, and relevant exit/key behavior in tests.

## Decision Log

- Keep this task documentation-only. If a maintained claim cannot be reconciled with implemented runtime behavior, stop and route the discrepancy to its owning predecessor rather than changing code in Task H.
- Do not edit generated launcher templates or `.agent/jig-contract.json`: `ui` and `status` retain their existing repository-scoped command classification, so the cutover changes presentation rather than launcher authority.
- Document product version 0.3.0 as the browser-to-terminal cutover and 0.4.0 as the earliest removal of the hidden `--port` diagnostic shim.
- Treat interactive dashboard output as human-only; direct automation to schema-1 `jig ui --json`, `jig ui --plan PLAN_ID --json`, or unchanged schema-1 `jig status --json` according to the required domain.

## Outcomes & Retrospective

Maintained documentation now describes the terminal dashboard as the sole interactive UI, with `jig status --tui` as its permanent status-first alias and schema-1 JSON commands as the automation boundary. The public contract records exact root fields, bounded wire shapes, limit pairs, error fields/codes/scopes, partial-success semantics, terminal requirements, refresh behavior, and the `--port` compatibility timeline. Regression coverage binds those claims to exported runtime constants and guards maintained guides against browser-dashboard drift.

Exactly two comprehensive Claude+Codex review rounds completed. Their findings exposed documentation drift and weak whole-file substring assertions rather than a missing runtime feature; the fixes narrowed key claims to their real tab/domain scope and strengthened tests to verify exact relationships. Focused validation passed 117 `jig-ui` tests and 13 UI architecture/cutover tests. Final structured verification passed formatting, Clippy with warnings denied, contract and file-budget checks, 3,150 core tests, and the 3,916-test aggregate suite. Three known timing-sensitive tests required configured retries across the two final suites; every retry completed successfully, and no unrelated runtime code was changed.

## Context and orientation

The shipped command and source behavior is owned by `crates/jig/src/cli.rs`, `crates/jig/src/ui.rs`, `crates/jig/src/ui/source/`, and `crates/jig/src/status.rs`. The terminal application and its exact model/keymap/layout behavior live under `crates/jig-ui/src/terminal/`; shared terminal mechanics remain in `crates/jig-tui`. Public documentation includes the root `README.md`, `docs/developer-ux.md`, `docs/public-contract.md`, `docs/status-provider.md`, `docs/repo-intent.md`, command references and examples under `docs/`, source-owned crate guides, and release notes/changelog files discovered by the audit.

## Plan of work

First derive current behavior from CLI help, tests, schema fixtures, terminal model/render constants, and the implemented adapter. Search every maintained documentation-like file for `jig ui`, browser, HTTP, localhost, port, cookie, capability, flight-recorder, and old status-TUI terminology. Update the smallest authoritative documents and any fixtures that assert their output. Then run generated guide/map checks, documentation link/example checks available in the repository, focused help tests, precise negative searches, and exactly two independent comprehensive review rounds before final plan-bound gates.

## Validation and acceptance

Current docs must explain the six tabs, keymap, status-first compatibility alias, independent/lazy refresh behavior, plan and receipt details, terminal sizing/requirements, one-shot recorder/plan JSON schemas and limits, unchanged provider schema, structured error/exit behavior, and the one-release hidden `--port` migration diagnostic. No maintained current-behavior text may recommend a browser URL, listener, cookie, capability token, HTTP endpoint, or `jig ui --port`, except clearly historical migration/release notes. `JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides`, `agent-map`, `contract`, relevant help/snapshot tests, documentation links/examples, focused stale-claim searches, and all applicable structured-work gates must pass.

## Idempotence and recovery

This task changes documentation and documentation-backed fixtures only. Re-running generators and checks is safe. Reverting its isolated commit restores prior prose without touching runtime, state, release binaries, or generated repository contracts.

## Interfaces and dependencies

Describe `jig ui` as the canonical unified terminal dashboard, `jig status --tui` as the permanent status-first alias into the same engine, `jig ui --json` as one-shot recorder schema 1, `jig ui --plan PLAN_ID --json` as one-shot plan schema 1, and `jig status --json` as the existing provider/status schema 1. Preserve the 0.3.0/0.4.0 compatibility timeline and the unchanged launcher/contract-epoch decision.
