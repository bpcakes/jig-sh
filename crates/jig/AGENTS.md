# jig crate guide

## Purpose

`crates/jig` contains the repo-local `jig` CLI and MCP runtime used by generated repositories. It executes the generated command contract, manages append-only `.agent/state` memory, and handles template init/adopt/update flows.

## Key entrypoints

- `src/main.rs`: binary entrypoint.
- `src/lib.rs`: library entrypoint and module wiring.
- `src/cli.rs`: clap command definitions and top-level command dispatch.
- `src/runtime.rs`: command-backed tool execution plus MCP tool call dispatch.
- `src/mcp.rs`: JSON-RPC/MCP stdio server.
- `src/state.rs`: sessions, plans, receipts, and decisions stored under `.agent/state`.
- `src/ui.rs`: `jig ui` and `jig status --tui` CLI adapter for the separately owned `jig-ui` terminal crate.
- `src/ui/source.rs`: typed recorder, plan, and status source with retained local epochs.
- `src/status.rs`: read-only local repository, work, and loop aggregate snapshots.
- `src/runtime/vault/tui.rs`: fixed-scope, process-local credential adapter for the separately owned `jig-vault-tui` crate.
- `src/bootstrap.rs`: init/adopt/update command surface.
- `src/bootstrap/`: bootstrap support for native template rendering, git, staged renders, and template-source handling.

## Edit here for X

- Change CLI flags or subcommands: `src/cli.rs`.
- Add an agent provider: `src/agent_provider.rs` defines the internal contract; `src/claude/provider.rs` and `src/codex/provider.rs` are implementations. Keep home identity and credential policy provider-owned; `src/cli/agent_run.rs` owns common homes/launch orchestration. See [agent providers](../../docs/agent-providers.md).
- Change shared Claude/Codex path primitives: `src/home_paths.rs`; keep discovery and default-home policy in the provider modules.
- Change Claude credential lookup and read-only subscription usage: `src/claude/usage/`; keep secrets, HTTP, and platform storage out of the TUI and output renderers.
- Change shared operation signal supervision: `src/signal_supervision.rs`; `src/cli/home_picker.rs` supplies picker diagnostics and provider adapters supply entries to `jig-codex-tui`.
- Change transparent agent execution: `src/agent_launch.rs`; providers prepare their own commands and environment overrides.
- Change command-preview sanitization and warnings: `src/cli/output/command_display.rs`; provider renderers own layout and JSON interpretation.
- Change make-tool behavior or receipt recording around command execution: `src/runtime.rs`.
- Change MCP descriptors, schemas, or protocol handling: `src/mcp.rs`.
- Change session, plan, receipt, or decision persistence: `src/state.rs`.
- Change the data exposed by the unified dashboard: `src/ui/source/`.
- Change dashboard navigation, scheduling, or rendering: `crates/jig-ui/`.
- Change status provider execution or aggregate facts: `src/status.rs` and `src/status/`.
- Change terminal status navigation, refresh runtime, or rendering: `crates/jig-ui/src/terminal/`.
- Change Vault TUI navigation, forms, or rendering: `crates/jig-vault-tui/`; keep scope, environment capture, external tools, and core calls in `src/runtime/vault/tui.rs`.
- Change bounded owned-process execution or process-tree cleanup: [jig-owned-process](../jig-owned-process/AGENTS.md).
- Change init/adopt/update behavior: `src/bootstrap.rs` and `src/bootstrap/`.
- Change git metadata captured in receipts: `src/git_receipts.rs`.

## Invariants

- Keep transport layers thin; shared behavior should live in runtime, state, or bootstrap helpers.
- Preserve generated-repo compatibility for `.jig.toml`, `.agent/jig-contract.json`, and `.agent/state/*.jsonl`.
- Treat `.agent/state/*.jsonl` as append-only unless a migration path is explicit.
- Keep execution tools aligned with the generated contract manifest and template outputs.
- Before changing process supervision or Bash probes, read the [process reference](../../docs/process-supervision.md) and the [owned-process guide](../jig-owned-process/AGENTS.md).
- Before changing vault entrypoints or dispatch, read the [vault runtime guide](src/runtime/vault/AGENTS.md).
- Before changing bootstrap entrypoints, scaffold toolchain checks, or project templates, read the [bootstrap guide](src/bootstrap/AGENTS.md).
- When editing the runtime, build `target/debug/jig` and dogfood through `JIG_DEV_BIN=target/debug/jig scripts/jig ...` so the cached repo-local binary cannot mask current code.

## Common commands

- `cargo test -p jig-sh`
- `cargo test -p jig-sh --test codex_launcher -- --nocapture` (requires a Unix PTY; set
  `JIG_ALLOW_PTY_TEST_SKIP=1` only when the environment is intentionally exempt)
- `cargo test --workspace`
- `cargo build -p jig-sh --bin jig`
- `JIG_DEV_BIN=target/debug/jig scripts/jig work status`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check contract`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map`
