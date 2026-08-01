# Repo Intent For Agents

This document captures what the current codebase makes clear about `jig.sh`, plus the intent that can be reasonably inferred from its structure. It is written for agents and maintainers who need to understand the idea behind the repo before changing it.

## High-Level Intent

`jig.sh` is a reusable harness for making Rust application repositories operable by coding agents.

The core idea is to turn a repository into an operating environment for coding agents through a small, stable, repo-local contract:

- repo-wide and crate-level agent guidance
- a typed `scripts/jig` CLI for repo-local commands
- an MCP server exposing the same tools to MCP clients
- append-only session, plan, receipt, and decision state under `.agent/`
- required work gates evaluated from plan-linked receipts
- native agent tooling checks for client-side Jig skills
- policy scripts and CI workflows that keep the generated contract honest

In short: `jig.sh` is a harness for making agentic software work repeatable, inspectable, and reviewable.

## What Is Certain

The README says this repo is a "Reusable harness for making Rust application repos operable by coding agents, including SQLx/Postgres backends and tooling-only Rust repos, with optional web apps."

The harness was extracted from the durable parts of an existing application workflow. The extracted pieces are generic agent guidance, a stable command contract, repo policy scripts, GitHub Actions workflows, a template sync flow, and the Rust `jig` runtime.

Generated or adopted repos receive assets such as `.jig.toml`, `.mcp.json`, `AGENTS.md`, `agent-map.md`, `.agent/PLANS.md`, `.agent/jig-contract.json`, scripts, and workflows.

Generated repos use `scripts/jig` as the execution backend. `scripts/jig mcp` exposes the same declared command contract to MCP clients.

The runtime is implemented in `crates/jig`. Its main responsibilities are:

- bootstrap flows: `jig init`, `jig adopt`, and `jig update`
- command-backed tool execution
- MCP protocol handling over stdio
- append-only runtime state for sessions, plans, receipts, and decisions
- agent tooling doctor/bootstrap commands for Codex-side Jig skills
- receipt metadata collection, including git changed paths and diff stats

The stable generated contract is `.agent/jig-contract.json`. Current renders use `contract_version: 3`, with command-backed tools such as `jig.bootstrap`, `jig.fmt_check`, `jig.clippy`, `jig.test`, `jig.test_locked`, `jig.contract_check`, and optional SQLx/schema/migration tools. Legacy `contract_version: 2` command-backed manifests can still be loaded by the runtime.

The separate `jig.status-provider/v1` protocol is an open, language-neutral observation boundary for software-rewrite tooling. `crates/jig-contract` owns its Rust DTOs and packages its JSON Schema and conformance example under `contracts/status-provider/`. A provider may remain private; the report is interoperable. The `jig` runtime configures and safely executes providers and exposes a versioned aggregate through `jig status`; `crates/jig-status-tui` presents that aggregate without importing runtime internals. Provider caching, web flight-recorder integration, launchability policy, and an implementation launcher remain later milestones.

Runtime memory tools are intentionally not part of `.agent/jig-contract.json`. They are runtime-owned conveniences exposed by the CLI and MCP server.

The root `AGENTS.md` is block-managed during adoption and update. Existing repo-specific content outside the Jig managed block is preserved.

Crate-level `AGENTS.md` files are project-owned. `jig.sh` validates required sections for crate guides that exist, but it does not require or generate placeholder crate guides.

The template deliberately avoids generating application code, crate guides, or schema dump implementations. Those remain owned by the consumer repository.

The repo dogfoods itself. This checkout is both the `jig` source tree and an adopted `jig` harness repo, and the root `AGENTS.md` instructs agents to validate runtime changes by building a dev binary and running through `scripts/jig` with `JIG_DEV_BIN`.

## The Product Idea

The repo appears to be designing for a future where agents work repeatedly inside long-lived repositories and need more than a README. They need:

- a map of where repo guidance lives
- a clear command contract
- machine-readable tool definitions
- a way to run checks without guessing project conventions
- required gates that stop work from finishing without evidence
- durable traces of plans, decisions, and command results
- compatibility rules so generated tooling can evolve without surprising downstream repos

The strongest product thesis visible in the code is:

> Agents should not infer how to operate a repo from scattered conventions. Repos should expose an explicit, typed, versioned harness that agents can use safely.

A shorter product phrasing is:

> Jig turns a repo into an operating environment for coding agents.

## Architecture In One Pass

`templates/project/` is the source for generated repository assets. It renders `.jig.toml`, `AGENTS.md`, `.agent/jig-contract.json`, scripts, workflows, and agent support files.

`.jig.toml` is both public configuration and the renderer answer file. It records repo settings such as `repo_name`, `default_branch`, `jig_version`, crate roots, SQLx settings, web app settings, status-provider argv, and template source metadata.

The template source metadata is a trust boundary. In generated or adopted repos, `scripts/install-jig.sh` may install from the exact `_commit` recorded in `.jig.toml` when that value is a hex git revision, so changing `_src_path` or `_commit` is equivalent to changing the source used to install the repo-local Jig runtime.

`crates/jig/src/bootstrap.rs` and its submodules implement template application:

- `init` renders the harness into a new destination and initializes git.
- `adopt` renders the harness into an existing repo while preserving repo-owned root `AGENTS.md` content.
- `update` re-renders managed paths from stored template metadata and refuses to overwrite changed managed files unless forced.

`crates/jig/src/runtime.rs` dispatches CLI and MCP tool calls. For command-backed tools, it resolves the command key from `.agent/jig-contract.json`, executes the configured `.jig.toml` command from the repo root, records a receipt, and returns structured JSON.

`crates/jig-contract` owns dependency-downward DTOs and identifiers shared across Jig crates. It also owns the Rust source of truth for the open status-provider protocol but does not execute providers, load repositories, or aggregate their observations.

`crates/jig/src/status.rs` owns the separate runtime boundary: configured provider execution, v1 validation, local Git input freshness, and aggregation with structured work, gate, lease, and attempt state. It preserves accepted provider JSON as emitted and keeps execution/configuration dependencies out of `jig-contract`.

`crates/jig-status-tui` implements the interactive terminal consumer used by `scripts/jig status --tui`. Like the web UI crate, it depends on a read-only snapshot-provider trait rather than `RepoContext`, provider commands, state storage, or runtime policy. The small adapter in `crates/jig/src/status/tui.rs` supplies cancellable aggregate snapshots. The TUI does not cache status or launch agents.

`crates/jig-dev-proxy` implements the Jig local development proxy used by `scripts/jig dev` and `scripts/jig proxy ...`. It is split from `crates/jig` so route storage, HTTP/HTTPS forwarding, certificates, service files, LAN mode, workspace discovery, and process supervision remain testable without depending on the broader CLI, MCP, receipt, or template runtime.

`crates/jig-ui` implements the loopback HTTP server, routes, timeline query model, and server-rendered dashboard used by `scripts/jig ui`. It depends only on a read-only snapshot-provider contract. `crates/jig/src/ui/snapshot.rs` remains responsible for joining repository state, work-gate evaluation, and loop status, so UI presentation does not depend on Jig runtime internals or read `.agent/state` directly.

`crates/jig` enables the `dev-proxy` Cargo feature by default so normal installs include the local proxy. Minimal consumers that only need the contract, MCP, and work-receipt runtime can build `jig-sh` with `--no-default-features` to omit the proxy dependency tree.

`crates/jig/src/mcp.rs` is a minimal MCP stdio server. It lists execution tools from the manifest and runtime memory tools from code, then dispatches `tools/call` through the same runtime path as the CLI.

`crates/jig/src/state/` stores append-only JSONL records:

- `sessions.jsonl`: session start/end events and write-time summaries; recent-session references inside new summaries are shallow so history cannot recurse
- `plans.jsonl`: plan open/append/close events
- `receipts.jsonl`: tool execution evidence with bounded output and changed-path previews
- `decisions.jsonl`: structured decision records

Normal writes append to these streams. Explicit maintenance uses streaming,
validated whole-file rewrites: session compaction creates an exact recovery
backup under ignored `.agent/.cache/state-backups/`, and receipt archiving writes
compressed cold data under ignored `.agent/.cache/state-archives/`. Explicit
receipt exports go only to the caller-selected path. None of these operations
rewrite Git history.

The current session pointer is cache state, not part of the durable JSONL record model.

## Design Principles Visible In The Code

**Agent-first discoverability.** `agent-map.md`, root `AGENTS.md`, crate `AGENTS.md`, MCP tool descriptors, and `.agent/jig-contract.json` all reduce the need for an agent to guess where to start.

**The Jig binary is the portable backend.** `scripts/jig` is the stable human-, CI-, agent-, and MCP-friendly execution layer.

**Typed surfaces over shell conventions.** `scripts/jig` returns JSON, validates tool names against a manifest, records receipts, and exposes MCP schemas. Agents get structured results instead of scraping terminal output.

**Compatibility is explicit.** Public execution tools are governed by `contract_version`. Breaking changes require a contract version bump, and downstream clients are expected to discover available tools instead of assuming optional SQLx/schema support.

**Runtime memory is append-only by default and locally repairable.** State files are written as JSONL, readers tolerate missing files, and application code should not edit records in place. Runtime-owned diagnostics, compaction, restore, archive, and export commands provide the bounded maintenance path.

**Repo-specific ownership stays local.** The harness provides workflow and policy defaults, while application code, business rules, crate ownership, and schema dump details stay with the downstream repo.

**Template updates should be conservative.** Update flows preserve root `AGENTS.md` custom content, avoid implicit template source switching, and refuse to overwrite changed managed files without `--force`.

**Dogfooding matters.** Runtime changes are expected to be validated through the same `scripts/jig` launcher, MCP contract, and receipt paths generated repos use.

**Rust-first, but not only backend code.** The current scope is Cargo workspaces, Rust checks, optional SQLx/Postgres behavior, and optional Bun-based web apps.

## What This Repo Is Not Trying To Be

It is not an application framework. It does not generate app code or domain models.

It is not a project task runner monopoly. Existing project scripts remain project-owned; Jig standardizes the agent-facing command surface.

It is not a global agent memory system. The state is repo-local and runtime-owned.

It is not trying to centralize business ownership. Crate-level guidance and application-specific commands stay project-owned.

It is not currently a general polyglot generated harness. The generated defaults assume Cargo workspaces, Rust formatting/clippy/tests, optional SQLx, and Bun for configured web apps. The status-provider observation boundary is deliberately language-neutral, but it does not by itself make the generated execution harness polyglot.

## How Agents Should Approach This Repo

Start with root `AGENTS.md`, then `agent-map.md`, then the nearest crate guide.

For runtime changes, read `crates/jig/AGENTS.md` and use its entrypoint map:

- CLI shape: `crates/jig/src/cli.rs`
- command, legacy make, and MCP dispatch: `crates/jig/src/runtime.rs`
- MCP protocol: `crates/jig/src/mcp.rs`
- sessions/plans/receipts/decisions: `crates/jig/src/state.rs` and `crates/jig/src/state/`
- bootstrap and template rendering: `crates/jig/src/bootstrap.rs` and `crates/jig/src/bootstrap/`
- generated outputs: `templates/project/`

When changing the public contract, update the manifest template, runtime dispatch, MCP exposure, generated scripts/docs, and tests together.

When changing generated repo behavior, validate with fixture rendering. The broad fixture check is:

```sh
./scripts/validate-fixtures.sh
```

When changing runtime behavior, build a dev binary and dogfood through the launcher:

```sh
cargo build -p jig-sh --bin jig
JIG_DEV_BIN=target/debug/jig scripts/jig work status
```
