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
- `src/ui.rs`: `jig ui` CLI adapter for the separately owned `jig-ui` server and presentation crate.
- `src/ui/snapshot.rs`: joins Jig-owned state, work-gate, and loop data for the UI provider boundary.
- `src/bootstrap.rs`: init/adopt/update command surface.
- `src/bootstrap/`: bootstrap support for native template rendering, git, staged renders, and template-source handling.

## Edit here for X

- Change CLI flags or subcommands: `src/cli.rs`.
- Change make-tool behavior or receipt recording around command execution: `src/runtime.rs`.
- Change MCP descriptors, schemas, or protocol handling: `src/mcp.rs`.
- Change session, plan, receipt, or decision persistence: `src/state.rs`.
- Change the data exposed by `jig ui`: `src/ui/snapshot.rs`.
- Change `jig ui` routes, query parsing, server behavior, or rendering: `crates/jig-ui/`.
- Change init/adopt/update behavior: `src/bootstrap.rs` and `src/bootstrap/`.
- Change git metadata captured in receipts: `src/git_receipts.rs`.

## Invariants

- Keep transport layers thin; shared behavior should live in runtime, state, or bootstrap helpers.
- Preserve generated-repo compatibility for `.jig.toml`, `.agent/jig-contract.json`, and `.agent/state/*.jsonl`.
- Treat `.agent/state/*.jsonl` as append-only unless a migration path is explicit.
- Keep execution tools aligned with the generated contract manifest and template outputs.
- Do not make template update flows switch source identity implicitly.
- When editing the runtime, build `target/debug/jig` and dogfood through `JIG_DEV_BIN=target/debug/jig scripts/jig ...` so the cached repo-local binary cannot mask current code.

## Common commands

- `cargo test -p jig-sh`
- `cargo test --workspace`
- `cargo build -p jig-sh --bin jig`
- `JIG_DEV_BIN=target/debug/jig scripts/jig work status`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check contract`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map`
