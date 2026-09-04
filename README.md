# jig.sh

[![Tests](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml)
[![Crates.io](https://img.shields.io/crates/v/jig-sh)](https://crates.io/crates/jig-sh)

> **Keep coding agents on contract.**

Jig is a repo-local operating harness for coding agents. It gives supported Rust, Go, and TypeScript repositories a versioned command catalog, gated work plans, and append-only receipts. You can adopt an existing repository or scaffold one of Jig's supported project shapes.

Agents should not have to infer how to operate a repository from scattered scripts and prose. Jig makes the repository's commands, ownership boundaries, checks, and definition of done explicit to humans, CI, CLI clients, and MCP clients.

## Contents

- [What you get](#what-you-get)
- [Supported project shapes](#supported-project-shapes)
- [Install](#install)
- [Quick start](#quick-start)
- [How it works](#how-it-works)
- [Command contract](#command-contract)
- [Creating and adopting repositories](#creating-and-adopting-repositories)
- [Feature guide](#feature-guide)
- [Documentation](#documentation)

## What you get

- **Agent guidance** through `AGENTS.md` and `agent-map.md`.
- **A typed command catalog** in `.agent/jig-contract.json`, executed through the repo-local `scripts/jig` launcher.
- **Structured work and gates** that let `work finish` close a plan only when every required gate has current evidence.
- **Append-only receipts** under `.agent/state/` for checks, plans, decisions, and runs.
- **Affected checks and file budgets** so agents can select work from checked-in component policy and enforce repository-owned source limits.
- **A bounded MCP runtime** for repository inspection, immutable planning, execution, and cancellation.
- **Local runtime tools** for orchestration loops, status dashboards, reusable prompts, development hostnames, and encrypted local secrets.
- **Conservative template updates** that preserve project-owned application code and refuse to overwrite customized managed files without `--force`.

## Supported project shapes

Jig is opinionated about the stacks it generates. It is not a universal application framework.

| Path | Generated project shape | Toolchain requirements |
| --- | --- | --- |
| `harness-only` | Jig harness files without application code | Rust 1.88+, Bash, Python 3.8+ |
| `rust-library` | Rust 2024 workspace with one library crate | Rust 1.88+, Bash, Python 3.8+ |
| `rust-cli` | Rust 2024 workspace with one binary crate | Rust 1.88+, Bash, Python 3.8+ |
| `rust-react` | Rust API plus optional Vite React, Astro, and admin frontends | Rust 1.94+; Node.js 24.19.0+ and a supported package manager for frontends; selected database tools when enabled |
| `go-react` | Go API plus Vite React or Astro frontends | Go 1.26; Node.js 24.19.0+ and a supported package manager; PostgreSQL tools when enabled |
| `jig adopt` | Harness added to an existing repository after a read-only preview | Depends on the repository; Rust/SQLx and JavaScript/TypeScript inference are the most established adoption paths |

Linux and macOS are supported hosts. See [Platform Support](docs/platform-support.md) for CI guarantees and feature-specific limits. Run `jig presets` for the current generated layouts and rejected combinations.

## Project status

Jig is pre-1.0. The current source renders contract v7; contracts v2 through v6 remain readable through documented compatibility paths. Contract epochs protect repository compatibility independently of the installed Jig product version. Review the [Public Contract](docs/public-contract.md) before wiring long-lived automation to Jig.

## Install

Install the bootstrap CLI from crates.io:

```sh
cargo install jig-sh
```

The Jig workspace MSRV is Rust 1.88. Generated application requirements vary by preset; use the table above instead of treating every supported toolchain as a universal prerequisite. The checked-in `rust-toolchain.toml` pins contributor and default CI tooling to Rust 1.98.0.

You only need a global installation for the first `jig init` or `jig adopt`. Generated repositories install and select a contract-compatible runtime through `scripts/install-jig.sh`, then expose it through `scripts/jig`.

## Quick start

Create a harness-only repository without prompts, prepare it, and complete one structured work plan:

```sh
jig init ./ExampleProject --preset harness-only --no-input --no-vault
cd ./ExampleProject
scripts/jig setup

plan_id="$(scripts/jig work start \
  --title "First change" \
  --body "Validate the harness loop." \
  --print-plan-id)"
scripts/jig work check --plan-id "$plan_id"
scripts/jig work finish \
  --plan-id "$plan_id" \
  --resolution "Harness loop verified" \
  --outcome success
```

For the guided path, run `jig init ./ExampleProject` in a terminal. Inside an existing repository, use `jig adopt .` to preview changes and `jig adopt . --write` to apply them.

`setup` runs the read-only doctor, bootstraps project dependencies, registers configured agent tooling when needed, verifies the generated contract, and runs doctor again. Pass `--json` to Jig commands when automation needs structured output.

## What changes in the repository

A full harness contains this core structure:

```text
.
├── .jig.toml                   # public configuration and renderer answers
├── .mcp.json                   # MCP client wiring
├── AGENTS.md                   # repo-wide agent guidance
├── agent-map.md                # index of nested agent guides
├── .agent/
│   ├── PLANS.md                # ExecPlan guidance
│   ├── jig-contract.json       # versioned command catalog
│   └── state/                  # append-only plans, receipts, and decisions
├── scripts/
│   ├── jig                     # repo-local launcher
│   └── install-jig.sh          # compatible runtime installer
└── .github/workflows/          # generated policy and test workflows
```

Checks append receipt records. A simplified record looks like this:

```json
{
  "tool_name": "jig.test",
  "plan_id": "plan_...",
  "exit_status": 0,
  "changed_paths": ["README.md"],
  "diff_stat": { "files": 1, "insertions": 8, "deletions": 2 }
}
```

Inspect the current evidence with `scripts/jig work status`, `scripts/jig work evidence`, or `scripts/jig work receipts`.

## How it works

1. **Render or adopt the harness.** `jig init` creates a supported project shape; `jig adopt` previews and then adds the harness to an existing repository.
2. **Discover the repository contract.** Humans, CI, and agents use the same checked-in components, actions, profiles, and command runners through `scripts/jig`.
3. **Plan and check work.** A work plan captures an exact Git baseline. Required gates execute only when their checked-in path policy applies and record explicit not-applicable evidence otherwise.
4. **Review the receipts.** Checks and structured work append evidence under `.agent/state/`; `jig ui` and status commands present that state without changing it.
5. **Update conservatively.** `jig update` advances managed harness files while preserving project-owned code and customized managed files unless replacement is explicitly forced.

## Command contract

`.agent/jig-contract.json` is the stable repository authority. Current contract v7 describes components, actions, targets, profiles, adapter provenance, native file-budget policy, and target-local affected selection.

Contract v6 and later expose four bounded MCP repository operations: inspect, plan, execute, and cancel. Contracts v2 through v5 retain their declared command tools through the legacy projection. Runtime-owned commands manage local workflow state, processes, prompts, status providers, or secrets outside the generated command catalog.

| Surface | Stable contract? | Records receipts? | Machine-local? |
| --- | --- | --- | --- |
| `check` | yes | yes | no |
| `work` / `loop` | runtime-owned | yes | no |
| `state` / `prompt` | runtime-owned | no | partly |
| `status` / `ui` | runtime-owned | no | partly |
| `dev` / `proxy` | runtime-owned | no | yes |
| `vault` | runtime-owned | no | yes |

Run configured checks directly:

```sh
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
scripts/jig check test --affected origin/main --explain
```

In contract v7, `--affected BASE` combines Git changes with checked-in component, dependency, and action-input policy. The plan explains why each target was selected before execution. See [Public Contract](docs/public-contract.md), [Status-provider protocol](docs/status-provider.md), and [Developer UX](docs/developer-ux.md) for the full surface.

## Creating and adopting repositories

Run `jig presets` before automation to inspect the supported shapes and their boundaries.

```sh
# Harness without application code
jig init ./ExampleProject --preset harness-only --no-input --no-vault

# One Rust library or CLI crate
jig init ./ExampleProject --preset rust-library --no-input --no-vault
jig init ./ExampleProject --preset rust-cli --no-input --no-vault

# Rust API with product, marketing, and admin frontends
jig init ./ExampleProject \
  --preset rust-react \
  --db postgres \
  --frontends web,landing,admin

# Go API with PostgreSQL and a product frontend
jig init ./ExampleProject \
  --preset go-react \
  --go-module example.com/example/project \
  --db postgres \
  --frontends web
```

The Rust-only presets create a virtual Rust 2024 workspace with one non-publishable, license-neutral crate. They add no database, frontend, API, dev app, or release workflow. Commit the generated `Cargo.lock` after `scripts/jig setup`.

The Rust/React preset generates a Cargo workspace plus source-owned shadcn Vite React, Astro, or admin applications. The Go/React preset generates a chi/Huma API, optional pgxpool/sqlc/Goose PostgreSQL support, and a Huma OpenAPI to Hey API TypeScript client. Generated application code becomes project-owned immediately; `jig update` does not migrate or overwrite it.

For an existing repository, preview before writing:

```sh
cd /path/to/repository
jig adopt .
jig adopt . --write
```

Adoption preserves existing root files such as `AGENTS.md` and `Makefile`; it changes only Jig's marked or explicitly managed sections. Override inferred settings with flags or an answers file. See [Adoption](docs/adoption.md) and [Configuration](docs/configuration.md).

Update an adopted or generated repository with:

```sh
jig update             # advance the template, preserving local changes
jig update --recopy    # re-render from the stored .jig.toml answers
```

`jig update` refuses to overwrite changed managed files unless `--force` is passed.

## Feature guide

### Structured work, affected checks, and file budgets

`work start` captures an exact Git baseline. `work check` evaluates required gates against checked-in path policy, records executed or not-applicable evidence, and can reuse eligible exact-input evidence. `work finish` refuses to close a plan until every required gate has current evidence.

Contract v7 also provides the native `repo:file-budget` action backed by the repository-owned `.jig/file-budget.toml` policy. Run `scripts/jig file-budget` for diagnostics without opening a run, or let the configured work gate and CI policy enforce it. See [Day-to-day workflow](docs/developer-ux.md#day-to-day-loop) and [Public Contract](docs/public-contract.md#repository-catalog-and-check-plans).

### Orchestration, status, and flight recorder

`jig loop` runs configured, bounded orchestration workflows and records their leases, attempts, and outcomes. `jig status` joins local Git and work state with any configured `jig.status-provider/v1` inspectors. `jig ui` serves a read-only loopback dashboard over plans, gates, receipts, decisions, and loops.

```sh
scripts/jig loop status
scripts/jig status
scripts/jig status --tui
scripts/jig ui --port 0
```

Provider failures remain visible as partial status instead of hiding available local state. The UI binds to `127.0.0.1`, validates loopback host and origin, and uses a one-time sign-in URL. See [Status-provider protocol](docs/status-provider.md) and [Flight Recorder UI](docs/developer-ux.md#flight-recorder-ui).

### State maintenance

`jig ui` serves a read-only loopback dashboard over `.agent/state/`: open plans with gate status and the next command to unblock them, recent failures with stderr, finished work with resolutions, per-tool check health, loop workflows with scheduled Codex-task run state and attempt budgets, and a filterable timeline of sessions, plans, receipts, and decisions. Plan ids link to detail pages with the plan body, gate evidence, decisions, and per-receipt output. See [Loop configuration](docs/configuration.md#loop-shape) for running durable prompts through `jig loop dispatch` from an external scheduler.

```sh
scripts/jig ui               # prints a one-time loopback sign-in URL
scripts/jig ui --port 0      # pick any free port
```

The dashboard validates the exact loopback `Host` and `Origin` and requires a
session cookie established by the printed one-time URL. Proxy aliases are not
supported because accepting arbitrary hostnames would reopen DNS-rebinding
access to receipt and plan contents.

The printed unguessable namespace contains JSON snapshot and plan endpoints returning the same joined data. The server binds `127.0.0.1` only and records no receipts. See [Developer UX](docs/developer-ux.md#flight-recorder-ui).
Use `scripts/jig state diagnose` to inspect receipt and session growth. Compaction, archival, export, restore, locking, and recovery behavior are documented under [Runtime State](docs/public-contract.md#runtime-state). Recovery artifacts under `.agent/.cache/` are local and ignored; copy any artifact that needs durable retention outside the checkout.

### Vault

Jig Vault stores an encrypted environment bundle outside the repository and resolves selected values only for brokered child processes.

```sh
scripts/jig vault init
scripts/jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt
scripts/jig vault exec --env-file .env.jig -- command
scripts/jig vault audit verify
```

Vault metadata, child output, and plaintext do not enter command receipts or MCP results. Once a child receives a value, however, that process can disclose it; output redaction does not stop malicious transformations or side channels. Jig Vault reduces local development exposure and does not replace a production secret manager. See [Vault runtime](docs/configuration.md#vault-runtime) and [Security Policy](SECURITY.md).

### Prompt library

Prompts can be user-level, repo-level, or distributed through read-only prompt packs:

```sh
scripts/jig prompt add comprehensive-review-loop --file prompt.md --tag review
scripts/jig prompt get comprehensive-review-loop
scripts/jig prompt get repo:release-checklist --var base=main
scripts/jig prompt search review
```

`prompt get` prints only the rendered MiniJinja body unless global `--json` is passed. See [Developer UX](docs/developer-ux.md).

### Local development proxy

Configured development apps run behind stable, repo-scoped local hostnames:

```sh
scripts/jig dev
scripts/jig dev status
scripts/jig dev stop
scripts/jig proxy list
```

The proxy owns route and process state outside `.agent/state/`. HTTPS certificate generation and trust require an explicit local trust-store acknowledgement. See [Developer UX](docs/developer-ux.md) and [Platform Support](docs/platform-support.md).

## Stack-specific repository contracts

Jig does not require every generated repository to be a Cargo workspace. Rust presets use Cargo, `cargo fmt`, and `cargo clippy`; Go presets use their generated Go adapter commands; `harness-only` generates no application toolchain files.

Configured web apps must expose `lint`, `typecheck`, `build:bundle`, and `test:coverage` package scripts. `test:coverage` writes `coverage/coverage-summary.json` for the generated threshold check. Bun is the default package manager, with supported npm, pnpm, and Yarn configurations documented in [Configuration](docs/configuration.md).

## Templates and versioning

Release builds of `jig init` and `jig adopt` use the official `jig-sh` template pinned to the installed release tag. Unreleased or dirty local builds use templates embedded in the binary. Pass `--template` only for a local checkout, fork, or private template.

When editing this repository's files under `templates/project`, refresh the packaged snapshot before committing:

```sh
JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
```

## Documentation

- [Developer UX](docs/developer-ux.md): command surface and daily workflow
- [Configuration](docs/configuration.md): `.jig.toml`, presets, package managers, and runtime options
- [Adoption](docs/adoption.md): previewing and adding Jig to an existing repository
- [Public Contract](docs/public-contract.md): contract epochs, CLI, MCP, receipts, runs, and state
- [Status-provider protocol](docs/status-provider.md): open JSON observation contract
- [Platform Support](docs/platform-support.md): supported hosts and feature limits
- [`examples/`](examples/): visible `.jig.toml` answer files

## Repository layout

- `crates/jig/`: publishable CLI, bootstrapper, and MCP runtime
- `crates/jig-contract/`: shared DTOs and public status-provider contract
- `crates/jig-{rust,go,typescript,sqlx}/`: repository model adapters
- `crates/jig-file-budget/`: native file-budget policy and evaluation
- `crates/jig-dev-proxy/`: local HTTP/HTTPS proxy and process supervision
- `crates/jig-{status-tui,ui,codex-tui,vault,vault-tui}/`: status, dashboard, Codex, and vault surfaces
- `templates/project/`: files rendered into downstream repositories
- `examples/`: sample answer files
- `scripts/validate-fixtures.sh`: rendered-repository validation

Validate this source tree with:

```sh
./scripts/validate-fixtures.sh
```

## Security

Please report vulnerabilities privately as described in [SECURITY.md](SECURITY.md). Do not include secrets, private repository contents, or exploit details in a public issue.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for development checks, release steps, and changelog conventions.

## License

[MIT](LICENSE)
