# jig.sh

[![Tests](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml)
[![Crates.io](https://img.shields.io/crates/v/jig-sh)](https://crates.io/crates/jig-sh)

> **Keep coding agents on contract.**

Jig turns any repository into an operating environment for coding agents. Without it, agents lose context across machines, have no stable execution contract, and leave no inspectable record of their work. Jig generates that scaffolding once — a typed command contract, MCP runtime, receipts, gates, a dev proxy, and a sealed local vault — and keeps it in sync as the harness evolves.

## What you get

- **Agent context files** (`AGENTS.md`, `agent-map.md`) so agents learn the repo layout and conventions without reading source.
- **A typed `jig` command contract** so every machine, CI run, and agent executes the same commands and leaves append-only receipts under `.agent/state/`.
- **An open status-provider protocol and aggregate** so public or private project inspectors can publish versioned software-rewrite observations and `jig status` can join them with repo and harness state.
- **Work gates backed by receipts** so a task cannot be marked done without a verifiable output artifact.
- **A local dev proxy** so app hostnames stay stable across port changes and machine restarts.
- **A local encrypted vault** so selected secrets resolve into brokered child processes without ever living in the repo.
- **A prompt library** so reusable prompts live outside the agent context window.
- **Repo policy scripts and CI workflows** so lint, tests, and coverage enforcement run consistently from day one.
- **Template sync via `jig update`** so the harness stays current without overwriting files you have customized.

## Install

**Supported hosts:** Linux and macOS. Native Windows and Git Bash are unsupported; see [Platform Support](docs/platform-support.md) for the WSL boundary and feature-specific limits.

**Prerequisites:** Rust 1.85+, Bash, Python 3.8+, Node.js 24.19.0+, the selected web package manager (Bun by default), and your database engine when SQLx is enabled.

```sh
cargo install jig-sh
```

You only need a global install to run `jig init` or `jig adopt` on a repo for the first time. Generated repos install their own runtime through `scripts/install-jig.sh`, then reuse it only while its recorded template source revision still matches and it supports the repository's contract epoch and requested build profile. A same-contract `jig update` therefore refreshes the cached runtime when `_commit` advances without pinning the repository to a product release.

Set `JIG_INSTALL_REFRESH=1` for one normal `scripts/jig` invocation, or call `scripts/install-jig.sh --refresh --profile runtime`, to deliberately replace a compatible cached runtime whose source follows an explicitly approved unpinned or embedded-source fallback.

## Quick start

Render the harness, check readiness, and run the work loop:

```sh
# 1. Run the guided init (or `jig adopt .` inside an existing repo)
jig init ./my-app

# 2. Prepare the generated repo and verify its minimum contract
cd ./my-app
scripts/jig setup

# 3. Do work behind gates
plan_id="$(scripts/jig work start --title "First change" --body "Validate the harness loop." --print-plan-id)"
scripts/jig check test
scripts/jig work finish --plan-id "$plan_id" --resolution "Harness loop verified" --outcome success
```

`setup` runs doctor first, bootstraps project dependencies, registers configured agent tooling when needed, verifies the generated contract, and runs doctor again. It records the bootstrap and contract receipts. `doctor` remains the read-only diagnostic command and exits nonzero until required setup is complete. Human-readable output is the default; pass `--json` for structured automation output.

## How it works

1. **Render the harness.** `jig init` (greenfield) or `jig adopt` (existing repo) renders the template into your project: `scripts/jig`, agent context files, CI workflows, and MCP config — pinned to a template version. `scripts/jig` is the generated command surface for everything below.
2. **Work behind gates.** Agents run the same typed commands on every machine. Each `check` and `work` step appends a receipt under `.agent/state/`, so a task can't be closed without verifiable evidence.
3. **Stay in sync.** `jig update` pulls template improvements without clobbering files you've changed — they are never overwritten without `--force`.

## The command contract

`.agent/jig-contract.json` records the stable command tools that MCP clients and CI can execute across machines. Runtime-owned commands manage local workflow state, processes, or secrets and are intentionally outside the generated contract.

| Surface         | Stable contract? | Records receipts? | Machine-local? |
| --------------- | ---------------- | ----------------- | -------------- |
| `check`         | yes              | yes               | no             |
| `work`          | runtime-owned    | yes               | no             |
| `state`         | runtime-owned    | no                | partly         |
| `prompt`        | runtime-owned    | no                | partly         |
| `dev` / `proxy` | runtime-owned    | no                | yes            |
| `vault`         | runtime-owned    | no                | yes            |

For local validation, call the contract commands directly:

```sh
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
```

These append receipts under `.agent/state/`. Pass `--no-receipt` to a one-off command when you don't want evidence recorded. Read existing state with `scripts/jig work status`, `scripts/jig work evidence`, and `scripts/jig work receipts --failed-only`.

See [Public Contract](docs/public-contract.md), [Status-provider protocol](docs/status-provider.md), and [Developer UX](docs/developer-ux.md) for the full surface.

## Creating and adopting repos

Run bare `jig init /path/to/target-repo` in a terminal for the guided path: choose a Rust + React starter or a harness-only repo, then choose the database and one or more `web`, `landing`, and `admin` frontends when scaffolding an app. `--defaults` skips the project-shape questions and fills omitted choices with `--preset rust-react --db none --frontend web`; initial vault setup can still prompt unless `JIG_VAULT_PASSPHRASE` is set or `--no-vault` is passed. `--no-input` is strict automation: pass a complete Rust React shape or the explicit `--preset harness-only`; redirected/non-terminal init is equally strict unless `--defaults` is present. Explicit CLI and answers-file values always win over defaults.

**Greenfield, harness only:**

```sh
jig init /path/to/target-repo --preset harness-only --no-input --no-vault
```

**Greenfield Rust backend + React frontends.** Run `jig presets` to see available presets and their generated layout, then:

```sh
jig init /path/to/target-repo --preset rust-react --db postgres --frontends web,landing,admin
```

This scaffolds a Cargo workspace (`apps/<repo>-api`, `crates/<repo>-core`, `crates/<repo>`, `crates/<repo>-http`, `crates/<repo>-test-support`, optional `crates/<repo>-db`) plus a shadcn Vite React product app, an Astro site, and a responsive shadcn admin application. Both React apps start with TanStack Router, TanStack Query, Tailwind 4, source-owned shadcn components, and a tested API version/readiness slice; the admin adds theme switching, navigation, and operational routes. Authentication and authorization remain project-owned. Jig records the tested shadcn CLI, preset, primitive library, and style for both React apps instead of running a mutable `shadcn@latest` during init.

Bare frontend names other than `web`, `landing`, `admin` and the compatible `marketing`, `astro`, and `admin-panel` aliases are custom names. Interactive init shows the resolved app kind and directory, then asks for confirmation so a typo such as `amdin` does not silently become a directory; non-interactive init calls the custom name out in the summary. Use an explicit kind such as `dashboard:spa`, `ops:admin`, or `campaign:astro` when a custom name is intentional. The Rust + React preset reserves `api` (case-insensitively) for its backend dev app, so use a name such as `api-client` for an API-facing frontend.

The frontends share a private root JavaScript workspace, pinned Node/package-manager metadata, and one root lockfile; fresh Yarn workspaces use the `node-modules` linker for compatibility with the generated Vite and Astro apps. For a database-backed scaffold, export `DATABASE_URL` or copy `.env.example` to `.env` and configure it before running `scripts/jig setup` (or the narrower `scripts/jig bootstrap`). Bootstrap creates or safely reuses the configured database, applies migrations, installs frontend dependencies once, and records both the selected dependency inputs and installed artifact. `scripts/jig dev` verifies that exact state without installing packages, and frontend `dev` scripts only start their servers. Commit the generated root lockfile. The app crate owns typed `AppConfig`/`AppState`; the API binary optionally loads `.env` with `dotenvy`; the HTTP crate owns the Axum router, handlers, middleware, and health endpoints. Local `.env` files remain ignored. Preset application code is generated once and then becomes **project-owned** — `jig update` keeps the harness current but never migrates or overwrites your application source.

**Adopt an existing repo.** `jig adopt` scans first and previews managed-file changes; re-run with `--write` after reviewing:

```sh
cd /path/to/target-repo
jig adopt .            # preview
jig adopt . --write    # apply
```

Adopt infers the repo name, default branch, Rust crate roots, SQLx/migrations, frontend apps, and CI `runs-on` values. Override anything with explicit flags (e.g. `--sqlx-enabled false`), or add `--json` for the full detection report. Existing root files like `AGENTS.md` and `Makefile` stay repo-owned — adopt only inserts or updates its marked block.

**Update an adopted repo:**

```sh
cd /path/to/target-repo
jig update             # advance to the current template, preserving your changes
jig update --recopy    # re-render from stored .jig.toml answers without advancing
```

`jig update` refuses to overwrite changed template-managed files unless `--force` is passed. `.jig.toml` is both the public repo config and the renderer answers file.

See [Adoption](docs/adoption.md) and [Configuration](docs/configuration.md) for the complete flag reference and update/versioning rules.

## Feature reference

### Structured work & receipts

`work start` opens a plan, `check` runs gates, and `work finish` closes a plan only after fresh evidence exists. Contract and gate commands append receipts under `.agent/state/`, giving every change a reviewable trail. See [Developer UX](docs/developer-ux.md#work-receipts-and-gate-evidence).

### Local state maintenance

State repair and retention work locally; no hosted service is required. Start with `scripts/jig state diagnose`, adding `--deep` to analyze recursive session summaries and receipt payload growth. Preview a legacy-session repair with `scripts/jig state compact sessions --dry-run`, then run it without `--dry-run` to write an exact compressed backup under ignored `.agent/.cache/` before replacing the working-tree state. Restore that backup with `scripts/jig state restore --backup <backup-directory>`.

`scripts/jig state archive --before <date>` compresses eligible old receipts into ignored local storage, creates an exact pre-rewrite recovery backup, and shrinks the active receipt stream. Restore that backup with the same `state restore` command. `scripts/jig state export receipts --before <date> --output <file.jsonl.gz>` creates a non-mutating export at a caller-selected path. Artifacts under `.agent/.cache/` are local recovery aids, so copy them elsewhere for durable retention. Working-tree compaction and archiving do not remove state blobs already reachable from Git history. See [Runtime State](docs/public-contract.md#runtime-state).

Before applying a compaction, archive rewrite, or restore, stop Jig processes launched with older runtimes; current runtimes share the repository state lock, but a legacy writer waiting on a pre-opened inode cannot follow an atomic replacement. After verifying a rewrite, keep or copy the recovery artifact you need and remove obsolete ignored cache artifacts. `state diagnose` reports that cache usage separately.

### Rewrite status

Configure a public or private `jig.status-provider/v1` inspector in `.jig.toml`, then join its blockers and implementation/verification progress with local Git freshness, plans, gates, loop leases, and attempts:

```sh
scripts/jig status
scripts/jig status --json
scripts/jig status --tui
```

The terminal dashboard has Overview, Packages, and Blockers views, background refresh, provider switching, and stable package/blocker selection. Press Enter on a package for a scrollable detail view covering facets, dependencies, acceptance checks, blockers, evidence, and provider-specific details. The command is read-only and keeps provider failures visible as partial status. It is separate from `jig ui`, the browser-based work/receipt flight recorder. See [Status-provider protocol](docs/status-provider.md#terminal-dashboard).

### Flight recorder UI

`jig ui` serves a read-only loopback dashboard over `.agent/state/`: open plans with gate status and the next command to unblock them, recent failures with stderr, finished work with resolutions, per-tool check health, loop workflows and attempt budgets, and a filterable timeline of sessions, plans, receipts, and decisions. Plan ids link to detail pages with the plan body, gate evidence, decisions, and per-receipt output.

```sh
scripts/jig ui               # prints a one-time loopback sign-in URL
scripts/jig ui --port 0      # pick any free port
```

The dashboard validates the exact loopback `Host` and `Origin` and requires a
session cookie established by the printed one-time URL. Proxy aliases are not
supported because accepting arbitrary hostnames would reopen DNS-rebinding
access to receipt and plan contents.

The printed unguessable namespace contains JSON snapshot and plan endpoints returning the same joined data. The server binds `127.0.0.1` only and records no receipts. See [Developer UX](docs/developer-ux.md#flight-recorder-ui).

### Vault

Jig Vault stores an encrypted environment bundle outside the repo. `jig init` and `jig adopt . --write` initialize a repo-scoped local vault by default; pass `--no-vault` to defer that setup. References are relative to the selected vault, so `jig://Production/RESTIC_PASSWORD` means item `Production`, field `RESTIC_PASSWORD`, in the current project scope. The project name is intentionally not repeated in the reference.

```sh
# Run this only when init/adopt used --no-vault.
scripts/jig vault init
scripts/jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt
printf '%s' 'local' | scripts/jig vault field set jig://Production/MODE --text --value-stdin
scripts/jig vault exec --env-file .env.jig -- command
scripts/jig vault audit verify
```

Both field kinds are encrypted: concealed fields contribute output-redaction patterns, while text fields are contextual values that remain visible when passed to a command. `vault exec` is a transparent, streaming developer wrapper; the compatible `vault secret` and constrained `vault run` commands remain available for the older cleaned-environment, closed-stdin, capped-output workflow. Controlled `read` and `inject`, one-time 1Password dotenv import, passphrase rotation, encrypted backup, and Linux-only absent-home restore complete the local workflow. Terminal use prompts for the passphrase; non-interactive callers export `JIG_VAULT_PASSPHRASE`. See [Configuration](docs/configuration.md#vault-runtime) for compatibility, recovery, scope, and audit limits.

### Prompts

Jig Prompt stores reusable prompts outside the agent context window. Prompts can be user-level, repo-level, or distributed through read-only prompt packs.

```sh
scripts/jig prompt add comprehensive-review-loop --file prompt.md --tag review
scripts/jig prompt get comprehensive-review-loop
scripts/jig prompt get repo:release-checklist --var base=main
scripts/jig prompt list
scripts/jig prompt search review
```

Without `--json`, `prompt get` is the exact-output primitive: it prints only the rendered prompt body, with no envelope or added newline. Pass global `--json` for the standard command envelope instead. Bodies render as MiniJinja templates (`--var KEY=VALUE`, or `--raw` to skip rendering). Names may be namespaced with `user:`, `repo:`, or `pack:<pack>/`; unqualified writes default to `user:` and `pack:` prompts are read-only. Common subcommands have shell-style aliases (`cat`, `cp`, `new`, `rm`, `ls`, `find`).

### Local dev proxy

Generated repos run supervised dev commands behind stable local hostnames, so app URLs survive port changes and restarts.

```sh
scripts/jig dev
scripts/jig dev status
scripts/jig dev --replace
scripts/jig dev stop
scripts/jig proxy alias api --port 8080
scripts/jig proxy list
```

Bare `scripts/jig dev` still launches the configured apps in the foreground. Each successful launch is registered as a repo-scoped dev session, so a terminal or agent that loses the foreground process can inspect it with `dev status` and request a safe, idempotent shutdown with `dev stop`. Use `dev --replace` to stop only conflicting registered sessions from the same canonical repository before launching; Jig refuses to replace another repo's session or an unregistered/ad-hoc process. If a supervisor crashed, spawn state is known, and every exact registered process identity is gone, either explicit command reports the session as recoverable and retires it with its exact-owned stale routes without signaling stored PIDs.

For HTTPS, generate and explicitly trust a local, name-constrained CA:

```sh
scripts/jig proxy cert generate
scripts/jig proxy cert trust --accept-trust-scope
```

`--accept-trust-scope` acknowledges platform trust-store mutation. Automatic certificate management and process-owned routes are supported on Linux and macOS. Other hosts are outside Jig's [platform support policy](docs/platform-support.md). See [Developer UX](docs/developer-ux.md).

## Required repo conventions

All generated repos are expected to use Cargo workspaces, `cargo fmt`, and `cargo clippy`. When `sqlx_enabled` is `true`, repos also use SQLx workspace metadata (e.g. `.sqlx/`) and repo-owned migrations.

Optional web apps must expose `lint`, `typecheck`, `build:bundle`, and `test:coverage` package scripts in each app directory. `test:coverage` must write `coverage/coverage-summary.json` so generated checks can enforce the threshold. The default workflow assumes Bun for package install and script execution.

## Templates and versioning

A *template* is the source repo whose files are rendered into your project. Release builds of `jig init`/`jig adopt` use the official `jig-sh` template, pinned to the release tag for the installed Jig version. Pass `--template` only to dogfood a local checkout, fork, or private template:

```sh
jig init ./my-app --template /path/to/jig-sh --template-mode committed
```

Unreleased or dirty local builds use the templates embedded in the binary. When editing files under `templates/project`, refresh the packaged snapshot before committing:

```sh
JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
```

## Documentation

- [Platform Support](docs/platform-support.md) — supported hosts, CI guarantees, and feature-specific limits
- [Developer UX](docs/developer-ux.md) — the `jig` command surface and daily workflow
- [Configuration](docs/configuration.md) — full `.jig.toml` reference and options
- [Adoption](docs/adoption.md) — bring Jig into an existing repository
- [Public Contract](docs/public-contract.md) — stable command contract for MCP clients and CI
- [Status-provider protocol](docs/status-provider.md) — open JSON contract for software-rewrite observations
- [`examples/`](examples/) — visible `.jig.toml` answer-file examples

## Repository layout

- `crates/jig/` — publishable `jig` runtime and MCP server
- `crates/jig-codex-tui/` — searchable Codex-home picker with live account and usage details
- `crates/jig-dev-proxy/` — local HTTP/HTTPS proxy with TLS certificate management
- `crates/jig-status-tui/` — read-only terminal dashboard over versioned status aggregates
- `crates/jig-tui/` — shared terminal lifecycle and cooperative-worker foundations
- `crates/jig-ui/` — read-only loopback dashboard server and presentation
- `crates/jig-vault/` — local encrypted vault, redaction, audit, and brokered-run primitives
- `crates/jig-contract/contracts/status-provider/` — public status-provider JSON Schema and conformance example
- `templates/project/` — files rendered into downstream repos
- `examples/` — sample `.jig.toml` answer files
- `scripts/validate-fixtures.sh` — renders sample repos and validates the generated harness

Validate this repo with:

```sh
./scripts/validate-fixtures.sh
```

## Release

Use the GitHub Actions `Release` workflow — leave `version` blank for the next patch, or set it explicitly. See [CONTRIBUTING.md](CONTRIBUTING.md) for local release steps, CHANGELOG conventions, and crates.io trusted-publishing setup.

## Contributing

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
