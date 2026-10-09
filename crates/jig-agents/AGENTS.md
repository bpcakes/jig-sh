# jig-agents crate guide

## Purpose

`crates/jig-agents` owns the agent providers Jig launches: Claude and Codex home discovery, account and usage inspection, credential lookup, and launch preparation behind the shared `AgentProvider` boundary. The CLI in `crates/jig` owns orchestration, pickers, process-wide signal supervision, and output.

## Key entrypoints

- `src/agent_provider.rs`: the `AgentProvider`, `SessionProvider`, and `HomeInspection` contract.
- `src/codex.rs` and `src/codex/`: Codex homes, app-server inspection, session resume lookup, and `codex/provider.rs`.
- `src/claude.rs` and `src/claude/`: Claude homes, `claude/provider.rs`, and read-only subscription usage under `claude/usage/`.
- `src/home_paths.rs`: path primitives shared by both providers.

## Edit here for X

- Add an agent provider: implement `AgentProvider` (and optionally `SessionProvider`) beside the existing providers, then wire its CLI family in `crates/jig/src/cli/`. See [agent providers](../../docs/agent-providers.md).
- Change shared Claude/Codex path primitives: `src/home_paths.rs`; keep discovery and default-home policy in the provider modules.
- Change Claude credential lookup and subscription usage: `src/claude/usage/`; the account email recorded in a home's `.claude.json` is read in `src/claude/usage/account.rs`.
- Change Codex app-server inspection or session lookup: `src/codex/app_server.rs` or `src/codex/resume.rs`.

## Invariants

- Keep home identity and credential policy provider-owned. Never reconstruct a launch identity from JSON, sanitized text, or a display path.
- Keep secrets, HTTP, and platform credential storage out of the TUI and output renderers.
- Inspection and session lookup take a cancellation callback, poll it, and retire every owned child process before returning. Callers own process-wide signal supervision; this crate never starts it.
- Keep this crate independent from repository context, state, and CLI orchestration.

## Common commands

- `cargo test -p jig-agents`
- `cargo clippy -p jig-agents --all-targets -- -D warnings`
- `cargo test -p jig-sh --test codex_launcher --test claude_launcher`
