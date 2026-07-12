# Developer UX

Jig is designed to make a repository feel immediately operable to a developer, an agent, or a CI job without requiring any of them to rediscover the same local conventions. The core UX promise is simple: after a repo is initialized or adopted, `scripts/jig` becomes the stable front door for setup, checks, local development, work evidence, agent readiness, and selected machine-local secrets.

That front door is intentionally repo-local. Developers do not need to remember whether a project uses a root Cargo workspace, SQLx metadata, a Vite frontend, a custom schema dump, or a particular MCP command. The repo records those decisions in `.jig.toml` and `.agent/jig-contract.json`, and the generated launcher pins the runtime version that knows how to execute them.

## First Contact

Jig splits the first-run experience into two cases:

- `jig init` creates a new repository with the harness already present, optionally with generated starter application code from a preset.
- `jig adopt` adds the harness to an existing repository while preserving project-owned files and guidance.

Both flows generate the same core assets: `.jig.toml`, `scripts/jig`, `.mcp.json`, root agent guidance, `agent-map.md`, `.agent/PLANS.md`, `.agent/jig-contract.json`, scripts, and CI workflows. Existing root `AGENTS.md` content is preserved; Jig only manages the marked block between the Jig comments. Existing root `Makefile` content also remains project-owned, because generated commands are routed through `scripts/jig`. For loop-only onboarding on an existing repo, `jig adopt . --minimal` renders `.jig.toml` plus `.agent/` scaffolding (contract, plans, state, cache ignore rules, and block-managed gitignore/gitattributes) without scripts, workflows, or agent context files.

The entry commands are intentionally separate. Start a new repo with `jig init`; add Jig to a repo that already exists with `jig adopt .`, which previews by default and applies only when re-run with `--write`. Use `jig presets` before greenfield app scaffolding as the reference for available starter shapes, defaults, frontend shorthands, examples, and ownership rules.

The practical result is that a new contributor can start with a small command set:

```sh
scripts/jig bootstrap
scripts/jig doctor
scripts/jig check contract
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
```

`doctor` is the short onboarding readiness pass, including harness and contract status without recording gate evidence. `scripts/jig check contract` records the contract gate evidence expected by reviews and CI.

Those commands are boring on purpose. They are meant to be copyable by humans, agents, onboarding docs, and CI without each caller having to infer project layout.

## Adopting Existing Repos

Adoption is optimized for low surprise:

- Repo-specific guidance remains outside the managed block in `AGENTS.md`.
- Application code, crate ownership, schema dump implementation, and app-specific orchestration stay project-owned.
- `jig adopt` previews by default; `--write` applies the reviewed render after confirmation unless `--defaults` or `--no-input` is supplied, and records an undo-oriented cache receipt with backups for overwritten managed files.
- Template-managed files are not overwritten during `jig update` unless the caller passes `--force`.
- `.jig.toml` rejects unknown keys so stale answers and typos fail early.
- Local template dogfooding can use embedded templates from an unreleased binary by default, an explicit committed template source for checkout metadata, or an explicit VCS ref for remote template code. Template edits must refresh the checked-in embedded snapshot with `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`.

This makes the adoption path friendly to established repositories. Jig adds an operating harness around the repo instead of trying to reorganize the application.

## Initializing New Repos

For greenfield repositories, `jig init` gives developers an immediate typed contract before the application has much code. Tooling-only repos can start without SQLx:

```sh
jig init /path/to/new-repo --repo-name new-repo --sqlx-enabled false
```

When the repo should start with an app, use a preset. The Rust + React preset creates the Jig harness, Rust workspace, API binary, core crate, main backend crate, HTTP boundary crate for Axum handlers and middleware, test-support crate, optional SQLx DB crate, crate-level ownership guides, and requested frontend apps in one pass:

```sh
jig presets
jig init /path/to/new-repo \
  --preset rust-react \
  --db postgres \
  --frontends web,landing,admin
```

`web` and `admin` generate Vite React apps, with `admin` normalized to the `admin-panel` directory; `landing` generates an Astro marketing app. The app crate parses typed `AppConfig` once at startup and passes it into `AppState`; the API binary loads `.env` with `dotenvy`, then initializes tracing and panic logging; the HTTP crate owns request IDs, request tracing, handlers, and `/health/live` plus `/health/ready`. The scaffold includes a root `.env.example` for local settings and ignores local `.env` files. When `--db` is selected, startup requires `DATABASE_URL`, connects the DB crate, runs migrations, and only reports readiness after DB initialization succeeds. The generated `.jig.toml` wires those apps into `scripts/jig dev` and the TypeScript/web check gates, defaults Rust roots to `apps` and `crates`, uses `bun` unless a package manager is supplied, and leaves schema dumps disabled until the project provides a command. Use `--frontends` as the canonical multi-app form; repeat `--frontend name[:kind]` for one-off additions.

Preset application code is a starter shape, not a managed framework. After init, the backend crates and frontend apps are project-owned; `jig update` updates the harness and will not migrate or overwrite scaffolded application source.

Rust backend repos can opt into migration and SQLx checks from the start:

```sh
jig init /path/to/new-repo \
  --repo-name new-repo \
  --rust-migration-dir migrations
```

The default Rust check commands skip cleanly when no root `Cargo.toml` exists yet. Once real application structure appears, the repo can replace the generated defaults in `.jig.toml` with project-owned commands.

## Day-To-Day Loop

The daily developer loop is built around a few stable verbs:

- `scripts/jig bootstrap` prepares local dependencies.
- `scripts/jig doctor` checks runtime, config, contract, required tools, agent skills, proxy status, vault status, and the next setup command.
- `scripts/jig check ...` runs configured repo checks and records receipts by default.
- `scripts/jig work ...` opens work, runs configured check and review gates, can refine actionable review findings, reports receipt status, and refuses to finish work without fresh required evidence.
- `scripts/jig ui` serves the flight recorder: a local read-only dashboard over the same state.
- `scripts/jig mcp` exposes the same command contract to MCP clients.
- `scripts/jig agent doctor` remains the focused local agent tooling check.

This is where Jig is most agent-friendly: checks and review skills become named gates with structured results and append-only evidence under `.agent/state/`. A reviewer can inspect what was run, which skill produced findings, against which worktree fingerprint, and whether the required gates are still fresh.

## Flight Recorder UI

`scripts/jig ui` turns the append-only record under `.agent/state/` into a browsable page instead of raw JSONL:

```sh
scripts/jig ui               # prints a one-time loopback sign-in URL
scripts/jig ui --port 0      # pick any free port
```

One page answers the daily questions:

- **Open plans and gates.** Each open plan shows its gate table — status, freshness, last run, diff summary — plus the exact command to produce missing evidence.
- **Recent failures.** The latest failed receipts with expandable stderr, so "why is this red" never requires `jq`.
- **Recently finished work.** Closed plans with resolutions and how long they took.
- **Check health.** Per-tool aggregates over recent receipts: runs, failures, last status, average duration.
- **Loops.** Configured loop workflows, live leases, and exhausted attempt budgets that need a human, with the matching `loop clear-attempt` command.
- **Timeline.** Sessions, plan events, receipts, and decisions merged newest-first, filterable with `?show=receipts|failures|plans|sessions|decisions` and `?limit=N`. Failed receipts include a stderr preview inline.

Every plan id links to a detail page under the server's per-run namespace, covering open and closed plans alike: the plan body, gate evidence as recorded, linked decisions, and the plan's receipts with expandable stdout/stderr and changed paths.

The server is read-only, binds `127.0.0.1` only, records no receipts, and re-reads state on every request, so the page is always current (it also auto-refreshes). It accepts only the exact bound `Host` and same-origin `Origin`; the printed one-time URL establishes an `HttpOnly`, `SameSite=Strict` session cookie scoped to an unguessable per-run path before redirecting to the clean dashboard URL. Snapshot and plan JSON routes live below that same namespace after the session is established, preventing the browser from sending the cookie to ordinary paths on unrelated loopback ports. Proxy aliases are intentionally unsupported because arbitrary accepted hostnames would weaken the DNS-rebinding defense.

## Dev Proxy

The dev proxy improves local development by separating the public developer URL from whichever port an app happens to use today. Repos declare supervised apps in `[dev]` and `[[dev.apps]]`, then developers can run:

```sh
scripts/jig dev
scripts/jig proxy list
```

Jig assigns or verifies app ports, starts trusted repo-configured commands, waits for readiness, and publishes stable local routes. Vite apps get structured `--port`, `--host`, and `--strictPort` injection when configured with `argv`, which avoids many fragile package-script edits.

Manual services can still join the same local routing model:

```sh
scripts/jig proxy alias api --port 8080
```

The proxy is friendly because it removes repeated port hunting, browser bookmark churn, and ad hoc hosts-file notes. It is also deliberately explicit around trust boundaries:

- HTTPS certificate generation and trust require explicit commands.
- Trust-store mutation requires `--accept-trust-scope`.
- LAN mode must be enabled deliberately.
- Alias routes remain loopback-client-only even when LAN mode is enabled.
- App commands inherit the developer environment, but the long-running background proxy process starts with a constrained environment.

Those constraints keep the normal path smooth while making machine-wide or network-visible changes visible in the command line.

## Vault

The vault handles a narrow but common developer problem: a local command needs a secret, but the repo should not store it and command receipts should not capture it.

The basic flow is:

```sh
scripts/jig vault init
scripts/jig vault secret set api_token --value-prompt
scripts/jig vault run --env TOKEN=api_token -- sh -c 'printf "%s\n" "$TOKEN"'
scripts/jig vault run --file TOKEN_FILE=api_token -- sh -c 'cat "$TOKEN_FILE"'
scripts/jig vault audit verify
```

Vault state is machine-local, encrypted, and stored outside `.agent/state`. Secret listing returns names and metadata, never values. `vault run` resolves requested secrets into a cleaned child-process environment or private temporary files, captures stdout and stderr, redacts known secret forms, returns JSON, and mirrors the child exit status.

The friendliness here is in the workflow shape: developers get an auditable secret handoff without adding new project-specific secret scripts. The important limits are also clear:

- Vault reduces accidental exposure; it is not a sandbox.
- Once a child process receives a secret, that child can use or disclose it.
- Output is buffered so redaction can happen before display.
- Non-interactive unlocks use `JIG_VAULT_PASSPHRASE`; command-line passphrases are intentionally unsupported.
- Audit metadata, including secret names and run IDs, is plaintext local operational metadata.

## Agent And MCP Friendliness

Jig treats agents as first-class repo operators. The generated root `AGENTS.md`, `agent-map.md`, optional crate-level guide conventions, MCP server, and work receipts all serve the same goal: reduce guessing.

An agent can discover:

- where repo-level and crate-level instructions live
- which checks exist for this repo profile
- which tools are stable contract tools
- which commands are runtime-owned local conveniences
- whether required work gates have fresh receipts
- whether local Codex-side Jig skills are available

The MCP surface is especially useful because it exposes the same declared tools as the CLI. Agents can call named tools instead of scraping README instructions or hard-coding one repo's check commands.

## Update And Maintenance UX

Jig's template update model favors predictable maintenance:

```sh
jig update
jig update --recopy
```

Plain `jig update` advances to the resolved template source. `jig update --recopy` re-renders from the stored commit and answers in `.jig.toml`. Changed managed files are protected unless `--force` is used.

This lets maintainers separate two tasks that are often conflated:

- "Re-render the current harness answers."
- "Move this repo to a newer Jig template."

The distinction matters for downstream repos because the harness is shared infrastructure, but the application remains project-owned.

## What Makes Jig Developer-Friendly

Jig's developer friendliness comes from a few consistent product choices:

- It gives every repo a small, stable command vocabulary.
- It records repo conventions in committed configuration instead of tribal memory.
- It preserves existing repo ownership during adoption.
- It makes local checks and work evidence inspectable.
- It makes MCP and CLI use converge on the same runtime contract.
- It keeps machine-local proxy and vault state out of repo history.
- It makes broad trust changes explicit at the command line.
- It supports dogfooding through `JIG_DEV_BIN` so Jig changes can be validated through the same launcher generated repos use.

The intentional friction is part of the UX. Trusting a local CA, exposing a proxy on the LAN, installing Codex marketplace support, overwriting managed files, or injecting secrets into a child process all require explicit commands. Ordinary repo work stays quick; higher-blast-radius actions are visible and auditable.

## Related References

- [Adoption Guide](./adoption.md)
- [Configuration Reference](./configuration.md)
- [Public Contract](./public-contract.md)
- [Repo Intent For Agents](./repo-intent.md)
