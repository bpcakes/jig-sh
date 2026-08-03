# Extraction Matrix

This matrix captures what was extracted from the source application workflow and how it was treated in `jig.sh`.

| Source asset | Treatment | Notes |
|---|---|---|
| `AGENTS.md` | Templated | Converted to generic repo-wide guidance with configurable paths and commands. |
| `agent-map.md` | Templated + generated | Rendered as a starter file, then refreshed by native `scripts/jig agent-map generate`. |
| `.agent/PLANS.md` | Templated | Preserved as the generic ExecPlan contract. |
| `.agent/jig-contract.json` | Templated | Declares command-backed and native repo contract tools for CLI and MCP consumers, with SQLx tools gated by `sqlx_enabled`. |
| `.agent/state/*.jsonl` | Runtime-owned | Append-only repo memory populated by `jig`. |
| `scripts/jig doctor` / `scripts/jig doctor --json` | Runtime-owned | Unified read-only readiness check for the pinned runtime, config, contract, required command executables, Codex-side Jig skills, dev proxy status, vault status, and the next setup command. Human-readable by default; `--json` for automation. |
| `scripts/jig agent doctor` / `scripts/jig agent doctor --json` + `scripts/jig agent bootstrap` | Runtime-owned | Checks and explicitly installs expected Codex-side Jig skills without adding more rendered shell scripts. Human-readable output is the default; pass `--json` for structured automation output. |
| `scripts/jig codex homes` + `scripts/jig codex launch` | Runtime-owned | Discovers separate `CODEX_HOME` account/state roots through the Codex app-server API and launches Codex with the selected home. CLI-only; no rendered launcher or MCP surface. |
| `.mcp.json` | Templated | Repo-local MCP entrypoint that launches `scripts/jig mcp`. |
| `crates/jig` | Added | Publishable runtime that exposes the typed CLI/MCP surface over the generated command contract and runtime-owned state. |
| Agent map, guide, Rust LOC, `mod.rs`, migration immutability, and SQLx unchecked-query checks | Runtime-owned | Implemented natively in `crates/jig`; generated repos call `scripts/jig ...` instead of rendered helper scripts. |
| `scripts/jig migration-add` | Runtime-owned | Adds timestamped forward-only migration stubs when `sqlx_enabled` is `true`. |
| `scripts/jig check contract` | Runtime-owned | Validates runtime wiring and manifest drift. |
| `scripts/install-jig.sh` + `scripts/jig` | Templated | Exact-version runtime launcher and installer for generated repos. |
| `scripts/enforce-coverage.cjs` | Extracted | Kept generic and CommonJS so it runs inside ESM frontend packages. |
| `scripts/new-checkout.sh` | Extracted + generalized | Uses current repo basename instead of source-specific naming. |
| `.github/workflows/agent-map-check.yml` | Templated | Runner label is configurable. |
| `.github/workflows/repo-policy.yml` | Templated subset | Keeps core policy checks and only includes SQLx/migration jobs when `sqlx_enabled` is `true`. |
| `.github/workflows/rust-tests.yml` | Templated subset | Simplified to generic fmt, clippy, and locked workspace tests. |
| `.github/workflows/webapp-checks-reusable.yml` + app workflows | Consolidated | Replaced with one generated matrix-style workflow or a disabled placeholder. |
| `scripts/dev.sh` | Excluded | Too application-specific. Downstream repos implement dev orchestration through `[dev]`, `[[dev.apps]]`, or project-owned scripts. |
| `scripts/dev-bootstrap.sh` | Excluded | App-specific onboarding/bootstrap logic. |
| Source crate guides | Excluded | Remain project-owned because ownership and entrypoints are repo-specific. |
