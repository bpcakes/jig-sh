# Developer UX

Jig is designed to make a repository feel immediately operable to a developer, an agent, or a CI job without requiring any of them to rediscover the same local conventions. The core UX promise is simple: after a repo is initialized or adopted, `scripts/jig` becomes the stable front door for setup, checks, local development, work evidence, agent readiness, and selected machine-local secrets.

This workflow is supported on Linux and macOS hosts. See [Platform Support](platform-support.md) for the CI guarantee, unsupported-host policy, and feature-specific limits.

That front door is intentionally repo-local. Developers do not need to remember whether a project uses a root Cargo workspace, SQLx metadata, a Vite frontend, a custom schema dump, or a particular MCP command. The repo records those decisions in `.jig.toml` and `.agent/jig-contract.json`, and the generated launcher accepts a runtime only after it validates the repository contract and requested build profile.

## First Contact

Jig splits the first-run experience into two cases:

- `jig init` creates a new repository with the harness already present, optionally with generated starter application code from a preset.
- `jig adopt` adds the harness to an existing repository while preserving project-owned files and guidance.

Both flows generate the same core assets: `.jig.toml`, `scripts/jig`, `.mcp.json`, root agent guidance, `agent-map.md`, `.agent/PLANS.md`, `.agent/jig-contract.json`, scripts, and CI workflows. Existing root `AGENTS.md` content is preserved; Jig only manages the marked block between the Jig comments. Existing root `Makefile` content also remains project-owned, because generated commands are routed through `scripts/jig`. For loop-only onboarding on an existing repo, `jig adopt . --minimal` renders `.jig.toml` plus `.agent/` scaffolding (contract, plans, state, cache ignore rules, and block-managed gitignore/gitattributes) without scripts, workflows, or agent context files.

The entry commands are intentionally separate. Start a new repo with `jig init`; add Jig to a repo that already exists with `jig adopt .`, which previews by default and applies only when re-run with `--write`. A bare terminal `jig init /path/to/new-repo` guides the project shape using the same five descriptions as `jig presets`; only the Rust React and Go React application choices continue to database and frontend questions. `--defaults` skips only the project-shape wizard and fills omitted shape choices with Rust React, no database, and `web`; initial vault setup can still request a passphrase unless `JIG_VAULT_PASSPHRASE` or `--no-vault` is used. `--no-input` skips the wizard but requires a complete explicit shape and never prompts for a vault passphrase. Non-terminal init follows the strict behavior unless `--defaults` is supplied. `harness-only`, `rust-library`, and `rust-cli` are complete when named explicitly and reject application-shape flags. Use `--preset harness-only --no-input --no-vault` for an unattended full harness without starter project code. An answers file with `harness_footprint = "minimal"` is itself a complete harness-only shape in every interaction mode and rejects Rust/database/frontend scaffold choices. Global `--json` only selects output format and never changes these interaction rules.

The practical result is that a new contributor can start with a small command set:

```sh
scripts/jig setup
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
```

`setup` begins with the read-only doctor pass, runs the configured project bootstrap, registers configured agent tooling when needed, records minimum contract evidence, and runs doctor again. `doctor` remains the standalone readiness diagnostic when no setup mutation is wanted. `scripts/jig bootstrap` and `scripts/jig check contract` remain available as the individual project-dependency and contract-evidence primitives.

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
jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
```

For the guided path, run `jig init /path/to/new-repo` from a terminal. Choose `rust-react`, `go-react`, `harness-only`, `rust-library`, or `rust-cli`. The application path then asks for its supported database and frontend selection: Go also asks for a module import path and supports `none`/`postgres` plus `web`/`landing`; Rust React additionally supports SQLite and `admin`. The harness-only and Rust-only paths ask no database or frontend questions. Jig resolves the answers file first, prompts only for missing choices, treats a stored minimal footprint as harness-only, validates incompatible shapes, and completes project-shape validation before asking for the initial vault passphrase or creating the destination.

For a new Rust repository without an application stack, use one of the explicit Rust-only presets:

| Preset | Initial virtual-workspace member | Starter artifact |
| --- | --- | --- |
| `rust-library` | `crates/<repo>` | `src/lib.rs` library |
| `rust-cli` | `crates/<repo>` | `src/main.rs` binary with an explicit `[[bin]]` target |

```sh
jig init /path/to/example-library --preset rust-library --no-input --no-vault
jig init /path/to/example-cli --preset rust-cli --no-input --no-vault
```

Both presets use Rust 2024 and the top-level Jig Rust 1.88 baseline. Their one seed package starts non-publishable and without license metadata. They add no database, SQLx, application contract, frontend, API, dev app, release workflow, or extra crate layer. The CLI starter uses only std and prints its package name and version; its argument parsing and logging choices remain project decisions. There is no public `rust-workspace` preset—choose the artifact the initial crate actually provides. If the Rust repository already exists, run `jig adopt .` instead so Jig previews and preserves its project-owned structure.

The generated Rust-only README gives the complete command list. The minimum post-init workflow is:

```sh
scripts/jig setup
scripts/jig check test
```

Setup creates `Cargo.lock`; commit it for either preset so locked checks and CI share the resolution. The root/member Cargo manifests, seed source, crate guide, and scaffold README are generated once and become project-owned. `jig update` maintains the harness without rewriting them. Root guidance talks about the Rust workspace and crate ownership, and neither preset configures or recommends `scripts/jig dev`.

When the repo should start with an app, use a preset. The Rust + React preset creates the Jig harness, Rust workspace, API binary, core crate, main backend crate, HTTP boundary crate for Axum handlers and middleware, test-support crate, optional SQLx DB crate, crate-level ownership guides, and requested frontend apps in one pass:

```sh
jig presets
jig init /path/to/new-repo \
  --preset rust-react \
  --db postgres \
  --frontends web,landing,admin
```

The Go + React preset creates a Go 1.26 module with chi/Huma HTTP boundaries, an offline Huma OpenAPI exporter, and the same generated React/client workspace. PostgreSQL adds pgxpool, embedded Goose migrations, sqlc queries, and checked-in generated code:

```sh
jig init /path/to/new-repo \
  --preset go-react \
  --go-module github.com/acme/new-repo \
  --db postgres \
  --frontends web
```

Generated Go checks are `scripts/jig check fmt`, `lint`, `test`, `test-locked`, and PostgreSQL-only `sqlc`. The public TypeScript client is regenerated transactionally from Huma OpenAPI with Hey API. SQLite and the separate privileged admin boundary remain intentionally unsupported by `go-react` and fail before files are published.

Generated React Node-side typings stay in the same major and at or below the minor version of the generated minimum Node runtime.

`web` generates a shadcn Vite React product app; `landing` generates an Astro marketing app; and `admin` generates `admin-panel`, a responsive shadcn operational application with light/dark/system themes, navigation, and routes. Both React apps use Tailwind 4, source-owned components, and tested API version/readiness states. They record the tested shadcn CLI, preset, Radix base, and style, so init is deterministic and does not invoke `shadcn@latest`; each app's `components.json` and pinned CLI dependency support later component additions. Authentication and authorization are deliberately project-owned rather than represented by a fake starter login.

The canonical shorthands are `web`, `landing`, and `admin`; the compatible aliases `marketing` / `astro` and `admin-panel` resolve to the same scaffold families without a custom-name confirmation. Any other bare value remains supported as a custom frontend name, but interactive init displays the resolved app kind and directory before asking for confirmation. Non-interactive init includes the same notice in its summary. Supplying `name:spa`, `name:admin`, or `name:astro` makes custom intent explicit and needs no confirmation. The Rust + React backend owns the case-insensitive dev identity `api` and its `JIG_DEV_API_*` environment contract, so frontend names must use another identity such as `api-client`.

The generated root is a private JavaScript workspace with `.node-version`, pinned package-manager metadata, and one root lockfile. Applicable Node-version authorities must be bounded real regular files reached through stable in-repository directories and contain exactly one token; malformed, empty, multiline, special, or symlinked authorities fail instead of falling back. CI synthesizes the pinned version only when every applicable authority is truly absent and writes that fallback under runner-owned temporary storage, never through `.node-version`. Fresh Yarn scaffolds select the `node-modules` linker. Before the first database-backed bootstrap, export a nonempty `DATABASE_URL` or copy `.env.example` to `.env` and configure its assignment; an empty or unrelated `.env` does not pass the guard. Bootstrap creates or concurrently reuses the configured Postgres or SQLite database through SQLx, applies migrations idempotently, performs one dependency install in the package-manager-authoritative workspace scope, and records a versioned fingerprint of every authoritative manifest/config/patch plus structural proof of node-modules, complete Yarn PnP companions, or a genuinely dependency-free package. Every in-memory SQLite pool retains one non-retiring connection so its database survives for the pool lifetime; private-cache URLs additionally restrict the pool to one checkout, while shared-cache URLs retain concurrent connections. Yarn Classic fingerprints the runtime's effective configuration, environment overrides, cache, version, platform, and path, then stamps the exclusive artifact Yarn actually produced; its PnP proof attests referenced external-cache packages while local workspace source edits remain independent of dependency readiness. Berry readiness uses Yarn's effective linker, cache, install-state, unplugged, data, and ESM-loader paths; only archives and unplugged packages referenced by the PnP runtime state are attested, so unrelated additions to a shared global cache do not invalidate the repo. npm installs neutralize inherited settings that would skip writes, locks, workspace members, platform packages, or executable links while retaining the repository's install-script approval policy. The checker remains compatible with stock macOS Bash 3.2, treats Yarn authority-enumeration errors as hard failures, and follows Bash-owned job identity after an interrupted worker wait. Every generated web/E2E package script re-enters its configured app through the checker, so CI cannot bypass those scope and authority checks. `scripts/jig dev` requires that exact state before launching any app; replacing an install directory, loader, PnP data/cache, or authoritative workspace input invalidates readiness. A verified live install worker remains authoritative even if its calling wrapper dies, while stale owners are recovered and unverifiable owners fail closed after a bounded wait. App `dev` scripts perform no install or other network mutation. Commit the selected lockfile. Every generated Rust/React workspace requires Rust 1.94 or newer. The app crate parses typed `AppConfig` once at startup and passes it into `AppState`; the API binary optionally loads `.env` with `dotenvy`, then initializes tracing and panic logging. Its default filter enables both the library target and the generated `<module>_api` binary target, so startup and failure diagnostics remain visible without `RUST_LOG`; the HTTP crate owns request IDs, request tracing, handlers, and `/health/live` plus `/health/ready`. When `--db` is selected, startup requires `DATABASE_URL`, connects the DB crate, runs migrations, and only reports readiness after DB initialization succeeds. Database Rust/React scaffolds pin SQLx 0.9 and use `.sqlx`; Doctor requires the SQLx CLI to match the dependency's major/minor line. Adopted repositories with another metadata layout must provide a compatible project-owned check command. A generated SPA's Playwright suite starts that real API on isolated ports and sets `HOST`, `PORT`, and `BIND_ADDR` together, so inherited local bind variables cannot redirect the managed backend. PostgreSQL E2E uses an Ubuntu runner because its GitHub Actions service container requires Linux; other generated jobs retain the configured repository runner. Its production-source coverage include is broad enough that new untested feature and API modules affect the gate; test/setup/entrypoint/generated-UI exclusions remain explicit. The generated `.jig.toml` wires those apps into `scripts/jig dev` and the TypeScript/web check gates, records each app's execution `kind` and semantic `role`, defaults Rust roots to `apps` and `crates`, uses `bun` unless a package manager is supplied, and leaves schema dumps disabled until the project provides a command. Generated workflow jobs select Bash explicitly for repository-owned shell commands. Use `--frontends` as the canonical multi-app form; repeat `--frontend name[:kind]` for one-off additions.

Dependency readiness covers every authoritative root/member node-modules tree and launcher content, not only the selected app's scope root. Missing, empty, and exact ignored-only real install roots normalize to one absent proof, so TypeScript, Vite, Vitest, or Playwright creating only real top-level `.cache`, `.vite`, `.vite-temp`, `.tmp`, and `.DS_Store` output does not stale a root-hoisted receipt. Unknown entries, nested or type-replaced cache lookalikes, package metadata, dependency metadata, symlinks, and launchers remain authoritative. Every in-memory SQLite pool retains one connection without idle or lifetime retirement and disables the cancellable pre-acquire health check; private-cache URLs additionally serialize checkouts so migrations and requests keep the same schema.

Generated Rust source is rustfmt-clean before the first project command across supported repository names, no-database/SQLite/PostgreSQL branches, and valid custom migration paths. Rust/React init validates the normalized Cargo package stem before writing: 216 ASCII bytes is the maximum that leaves room for Cargo's generated test-support metadata filename, while a 217-byte stem fails with guidance to choose a shorter repository name. Longer supported names use a deterministic DNS-safe fallback API label; ordinary short repository names retain their readable `api.<repo>.localhost` origin.

Preset application code is a starter shape, not a managed framework. After init, the backend crates and frontend apps are project-owned; `jig update` updates the harness and will not migrate or overwrite scaffolded application source.

Rust backend repos can opt into migration and SQLx checks from the start:

```sh
jig init /path/to/new-repo \
  --preset harness-only \
  --repo-name new-repo \
  --rust-migration-dir migrations \
  --no-input \
  --no-vault
```

The default Rust check commands skip cleanly when no root `Cargo.toml` exists yet. Once real application structure appears, the repo can replace the generated defaults in `.jig.toml` with project-owned commands.

Managed npm checks, browser E2E, and generated dev pin the exact app and required script despite inherited workspace, global, missing-script, or omit selectors. Explicit application `NODE_ENV`, registry/authentication, dependency layout, peer resolution, and lifecycle policy remain project-owned; custom dev commands continue inheriting all caller settings.

## Day-To-Day Loop

The daily developer loop is built around a few stable verbs:

- `scripts/jig setup` runs doctor, prepares local dependencies, registers configured agent tooling when needed, verifies the minimum contract, and runs doctor again.
- `scripts/jig bootstrap` remains the project-dependency-only primitive used by setup.
- `scripts/jig doctor` checks runtime, config, contract, required tools, agent skills, proxy status, vault status, and the next setup command. The launcher keeps `doctor` and `check contract` reachable through a capability-only final runtime probe against its rendered contract epoch, so a missing or malformed repository manifest can be reported instead of blocking its own diagnostic. Ordinary commands still require strict repository validation. Every external check—including SQLx capability probes, configured Codex marketplace support, and launcher-backed proxy/service diagnostics in either feature mode—runs inside a bounded owned process tree under one serialized signal owner. Clean handler retirement permits a later doctor call in the same host process; unsafe retirement permanently poisons reuse. Linux and macOS retain the exact child process-group identity until descendants are proven gone, cancellation prevents later check families from starting, and unsupported supervision fails the check closed before a child starts.
- `scripts/jig info --commands` lists every root command's primary-workflow availability, stable machine-readable reason code, and next setup step; the installed `jig info --commands` form also works before adoption.
- `scripts/jig check ...` runs configured repo checks and records receipts by default.
- `scripts/jig work ...` opens work, runs configured target/profile evidence, legacy check, and review gates, can refine actionable review findings, reports receipt status, and refuses to finish work without fresh required evidence.
- `scripts/jig status` joins configured software-rewrite providers with local repository, work/gate, lease, and attempt state; `--tui` makes that aggregate navigable in the terminal.
- `scripts/jig ui` serves the flight recorder: a local read-only dashboard over the same state.
- `scripts/jig mcp` exposes bounded repository discovery and execution tools to contract v6 clients, while older contracts retain direct command tools.
- `scripts/jig agent doctor` remains the focused local agent tooling check.
- `scripts/jig codex homes` shows the authenticated account in each local Codex home; bare `scripts/jig codex launch` opens an immediate searchable picker whose account, quota remaining, and at-current-pace projection fill in without blocking navigation. The picker marks the inspected home with the best projected outcome—most headroom or least overrun—without reordering results. `scripts/jig codex launch HOME` selects one account/state root directly. `scripts/jig codex resume SESSION_ID` reports lookup progress while finding the state root that owns a session, then launches Codex. Launch and resume forward Codex arguments after `--`.

This is where Jig is most agent-friendly: repository targets, verification profiles, legacy checks, and review skills become named gates with structured results and append-only evidence under `.agent/state/`. A reviewer can inspect the exact target and run, which skill produced findings, the contract and input digests, and whether the required evidence is still fresh.

### Repository targets and check plans

Jig also presents the repository as components with actions. Their executable
address is a target such as `api:test`. Contracts through version 5 are adapted
without changing their files: Jig exposes one synthetic `repo` component and
maps each declared tool to a `repo:*` target while retaining the original
`jig.*` name as an alias. Component-native contract records can therefore use
the same inspection and planning core as existing repositories.

Contract 6 generates component-native records directly. A typical application
has an `api` component plus one component per configured frontend, so `test`
selects `api:test`, `web:test`, and any sibling test targets without conflating
their commands. Stack integrations appear as composable adapters on components;
the runtime no longer uses one persisted backend-language switch to decide what
the workspace can do.

Use the static info views to discover the model without running commands:

```sh
scripts/jig info workspace
scripts/jig info components
scripts/jig info component repo
scripts/jig info targets
scripts/jig info target repo:test
scripts/jig info profiles
```

Bare `scripts/jig check` resolves the default verification profile. For a
legacy contract that profile comes from configured work checks, falling back to
its read-only check tools while omitting the duplicate locked-test action. An
unqualified action selects that action across components, an exact
`component:action` selects one target, and `*` can replace either side:

```sh
scripts/jig check
scripts/jig check test
scripts/jig check repo:test
scripts/jig check 'repo:*'
scripts/jig check test --affected origin/main --explain
scripts/jig check --profile verify --explain
```

On contract-v6 repositories, `--affected BASE` narrows those normal candidates
using committed changes from the Git merge base through `HEAD` plus staged,
unstaged, untracked, and ignored `.env`/`.env.*` paths beneath directories that
are not themselves ignored. Wholly ignored directories are pruned so generated
dependency and build trees do not become source inputs; unignore a containing
path when a repository intentionally stores an input there. Because observed
ignored dotenv files have no committed baseline, generated repositories exclude
their mere presence through reviewed `repository.affected_ignore` policy while
retaining their contents in evidence fingerprints. An explicit action input
overrides the affected-ignore policy when a check must be selected for that
dotenv. The plan explains each direct path and configured
component-dependent propagation; runtime-owned `.agent/state/` and
`.agent/.cache/` data are ignored while checked-in contract inputs remain
eligible. A valid empty
selection is a no-op. Action dependencies are added only after this filtering,
and versions 2 through 5 retain their legacy check behavior without affected
selection.

Contract-v6 work gates name the same target/profile vocabulary. A default
`scripts/jig work check --plan-id ...` resolves all configured evidence gates
and executes their target union once, allowing a profile gate to prove that all
members succeeded in one compatible run. An exact target receipt cannot satisfy
a different target, and separate partial runs cannot be combined into profile
evidence. Legacy tool gates remain available for older contracts and explicit
`work check --tool ...` compatibility.

`--explain` is read-only: it prints the immutable plan, bounded target-reason
previews (with total-count metadata when truncated), dependency layers, effects,
configuration digest, source identity, and input
digests, and executes no command or receipt write. Selectors are normalized and
targets are sorted before the plan id is derived, so equivalent requests
against the same repository state have the same plan id. The existing named
check forms and their receipt controls remain compatible.

Executing one of these plans creates an append-only durable run even when the
CLI waits for it to finish. Each target reaches its own conclusion and normally
writes one receipt carrying the run id, structured target, configuration and
input digests, and normalized findings. A reviewed plan is rejected before a
run is created if the contract or worktree changed. Query the accepted plan and
folded target results later with:

```sh
scripts/jig status run RUN_ID
```

Checks own their configured process trees, apply target timeouts, and preserve
every target result on cancellation or explicit fail-fast skips. Receipt flags
may appear before or after target selectors, for example
`scripts/jig check api:test --no-receipt`.

## State Health And Retention

Jig provides an offline repair path for its own repository state. `scripts/jig state diagnose` reports stream sizes and integrity without mutating state; add `--deep` to analyze legacy recursive session summaries and receipt payload growth. `state compact sessions --dry-run` validates and previews the repair. Apply mode creates an exact compressed backup under ignored `.agent/.cache/` before replacing the session stream, and `state restore --backup <path>` verifies that backup before restoring it.

Receipt retention is also local. `state archive --before <date>` compresses eligible old records into ignored `.agent/.cache/state-archives/`, writes an exact manifested pre-archive backup under `.agent/.cache/state-backups/`, and shrinks the active stream. `state restore --backup <path>` can restore that exact receipt preimage. `state export receipts --before <date> --output <file.jsonl.gz>` makes a non-mutating copy at a caller-selected destination. Cache artifacts are ignored local recovery aids rather than durable backups, and neither operation rewrites Git history, so durable retention and committed historical blobs require separate, coordinated handling.

## Rewrite Status

Repositories with a project-specific software-rewrite inspector can configure its exact argv in `.jig.toml` and use one read-only command for the operational picture:

```sh
scripts/jig status
scripts/jig status --json
scripts/jig status --tui
```

The human view calls out repo cleanliness and local tracking state, open plans, loop leases/attempts, provider failures, package and blocker totals, and whether each reported Git input is current, dirty, or stale. The versioned JSON retains each validated `jig.status-provider/v1` document and adds Jig-owned work, gate, and runtime facts beside it.

A failed provider does not erase the rest of the picture: it appears as a failed provider result and makes aggregate `outcome` partial. The command is read-only, records no receipt, stores no cache, and performs no remote fetch. See [Status-provider protocol](status-provider.md#jig-runner-and-aggregate) for configuration and exact semantics.

The TUI consumes that aggregate through three views: Overview for progress, repo and legacy/target freshness, diagnostics, and Jig state; Packages for selectable native facet and acceptance details; and Blockers for a flattened operator queue. Press Enter on a selected package to open its scrollable detail view, including bounded standard package fields and generic rendering of namespaced provider extensions; oversized fields and collections are marked when the terminal view truncates or omits them. Escape or Enter returns to the list. Press `r` to refresh, Tab or `1`/`2`/`3` to change views, `j`/`k` or arrows to move or scroll, `[`/`]` to switch providers, `b` to filter packages, and `q` to quit. Collection happens in one cancellable background worker, so a slow provider does not block navigation or quit. `--refresh-seconds` changes the 30-second default.

This status TUI and the browser flight recorder below are separate consumers with separate snapshot models. Provider caching, launchability policy, and a Codex implementation launcher remain outside this slice.

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
scripts/jig dev status
scripts/jig dev --replace
scripts/jig dev stop
scripts/jig proxy list
```

Jig assigns or verifies app ports, starts trusted repo-configured commands, waits for readiness, and publishes stable local routes. Vite apps get structured `--port`, `--host`, and `--strictPort` injection when configured with `argv`, which avoids many fragile package-script edits. Generated Astro apps consume the same injected `HOST` and `PORT`, fail instead of moving to a different busy port, and stay in Jig's supervised foreground tree even when Astro detects an agent environment.

On Unix, the foreground supervisor also identifies itself in process listings as `jig dev --jig-project=<repo-name>@<repo-root>`. The name appears before the full path so simultaneous sessions remain distinguishable even when `ps` truncates a long command column.

Every foreground launch also registers a tied dev session in the proxy state directory. `scripts/jig dev status` is a read-only view of sessions owned by the current canonical repository, including supervisor/app observations, durable preflight-cleanup evidence, and whether an orphan is `recoverable`; no sessions is a successful stopped state. Aggregate `running` excludes stale and recoverable records, so a recoverable record appears in `sessions` while `running` is false and still requires explicit cleanup. `scripts/jig dev stop` requests shutdown of every registered session for that repository and is idempotent when nothing is running. It uses the session's authenticated loopback control endpoint so the live supervisor performs its ordinary handle-backed cleanup. If that supervisor is gone while preflight cleanup is unconfirmed, an app spawn is pending or unknown, or a registered identity remains live or uncertain, the command exits nonzero and retains the session record for inspection; it never converts persisted numeric PIDs into authority to signal a possibly recycled process. If cleanup evidence is complete and all exact registered identities are absent, `dev stop` and `dev --replace` recover by retiring the orphan and its exact-owned process routes under the state lock and print a recovery notice with app targets, last-known PIDs, and any explicitly forgotten ambiguities. Blocking warnings remain separate from successful recovery notices. When only unconfirmed preflight cleanup or pending or legacy-untracked spawn evidence blocks a dead-supervisor orphan, an operator who has independently checked for unrecorded processes can run `scripts/jig dev stop --forget-ambiguous-orphans`. This explicit repair remains fail-closed for every live or uncertain registered identity, signals no persisted PID, and records that an unrecorded process may still be running.

Bare `scripts/jig dev` remains the launch command. Add `--replace` when a new launch should stop only its conflicting registered sessions from the same canonical repository before claiming their apps and routes. Replacement never takes over a cross-repository session, an ad-hoc `proxy run`, or a live route that cannot be attributed to a registered session. A process started by an older Jig without session registration therefore needs a one-time manual stop followed by `scripts/jig proxy prune`; subsequent registered sessions can be handled with `dev status`, `dev stop`, or `dev --replace`.

Launch and both management subcommands must use the same proxy state directory. They default to `JIG_PROXY_STATE_DIR` or `~/.jig/proxy`; pass `--state-dir <path>` after `status` or `stop` when the launch used an explicit directory. Launch-only flags such as `--app`, `--replace`, and proxy listener options are not accepted by the management subcommands. The stop-only `--forget-ambiguous-orphans` repair is never applied by replacement.

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
- Jig replaces inherited `JIG_DEV_<APP>_{HOST,PORT,ORIGIN,URL}` coordinates with the current app selection. Generated Vite apps prefer the current namespaced API origin, while `API_ORIGIN` remains the explicit override for direct or web-only starts.

Those constraints keep the normal path smooth while making machine-wide or network-visible changes visible in the command line.

## Vault

The vault handles a common developer problem: a project needs a complete local environment bundle, but the repository and command receipts should contain references rather than protected values.

References are project-relative. `jig://Production/RESTIC_PASSWORD` selects the `Production` item in whichever vault the current repository, `--global`, or `--home` chooses. There is no repository-name segment and no cross-project reference syntax. A project moves its encrypted state with backup and restore, not by adding a project qualifier to references.

A field is either concealed or text. Both are encrypted at rest. Concealed is the default for credentials and participates in streamed output redaction; `--text` is for contextual values such as modes, URLs, and identifiers that should remain visible in ordinary output.

One everyday flow is:

```sh
scripts/jig vault init
scripts/jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt
printf '%s' 'local' | scripts/jig vault field set jig://Production/MODE --text --value-stdin
printf '%s\n' \
  'RESTIC_PASSWORD=jig://Production/RESTIC_PASSWORD' \
  'MODE=jig://Production/MODE' > .env.jig
scripts/jig vault exec --env-file .env.jig -- command
scripts/jig vault audit verify
```

`vault exec` invokes the command directly, inherits stdin and the ordinary environment, streams stdout and stderr without a Jig timeout or output cap, redacts concealed values, and preserves the child status. It is the developer-facing analogue of `op run --env-file`. The older `vault run` remains intentionally different: it uses an allowlisted environment, closes stdin, caps and buffers output, applies a timeout, and owns child-tree cleanup. That constrained behavior remains useful for agent-controlled execution, and `vault secret` remains the compatible concealed-field vocabulary.

`vault read` is the exact-byte analogue of `op read`; terminal stdout requires `--reveal`, while pipelines are accepted and private file output requires an explicit overwrite opt-in. `vault inject` replaces only `{{ jig://ITEM/FIELD }}` placeholders under the same output rules. Raw reveal commands reject `--json` so values cannot enter structured results.

For a one-time 1Password cutover, `vault import onepassword` parses the restricted dotenv grammar, resolves whole `op://...` values with direct `op read --no-newline` calls, stores them as concealed, stores literals as encrypted text, and writes a reference-only dotenv file. `--dry-run` invokes no `op` process and makes no mutation. A post-commit destination-install failure says that the vault import succeeded; rerun the emitted command with `--replace --overwrite` to converge. The importer does not provide ongoing synchronization.

For example, importing a source whose 1Password vault is named `ExampleVault` into item `Production` produces project-local assignments like these; the source vault name is context, not another Jig reference segment:

```dotenv
RESTIC_PASSWORD=jig://Production/RESTIC_PASSWORD
RESTIC_REPOSITORY=jig://Production/RESTIC_REPOSITORY
RESTIC_COMPRESSION=jig://Production/RESTIC_COMPRESSION
```

Jig reads only the requested dotenv source during this explicit cutover; it does not inspect or modify another project checkout automatically.

Passphrase rotation reseals a version 2 vault without changing its fields or identity. Encrypted backup captures the vault and audit log; from a checkout configured for repo scope, restore automatically selects that checkout's vault home, prepares missing private parents, and installs only when the vault home itself is entirely absent. `--global` makes legacy user-level selection deliberate, while `--home` remains an explicit recovery and testing override; omitted selection retains the legacy default during contract v4. This gives a project an explicit relocation and recovery path without weakening path-bound repo isolation.

The friendliness here is in the workflow shape: developers get an auditable secret handoff without adding new project-specific secret scripts. The important limits are also clear:

- Vault reduces accidental exposure; it is not a sandbox.
- Once a child process receives a secret, that child can use or disclose it.
- Redaction is a backup control; transformed values and side channels are outside its guarantee.
- Non-interactive unlocks use `JIG_VAULT_PASSPHRASE`; command-line passphrases are intentionally unsupported.
- Audit metadata, including field names and run IDs, is plaintext local operational metadata. The local HMAC chain detects edits and broken links, but deletion, rollback, or compromise by someone with the vault and passphrase requires an external checkpoint or backup to detect.

## Agent And MCP Friendliness

Jig treats agents as first-class repo operators. The generated root `AGENTS.md`, `agent-map.md`, optional crate-level guide conventions, MCP server, and work receipts all serve the same goal: reduce guessing.

An agent can discover:

- where repo-level and crate-level instructions live
- which checks exist for this repo profile
- which tools are stable contract tools
- which commands are runtime-owned local conveniences
- whether required work gates have fresh receipts
- whether local Codex-side Jig skills are available

The contract v6 MCP surface is deliberately independent of repository size. Agents inspect components, targets, profiles, and durable runs with `jig.inspect`; resolve an exact immutable plan with `jig.plan_run`; submit that plan with `jig.execute_run`; and poll or cancel by run id. Effectful targets require explicit selection, closed plan-bound arguments, and exact worktree/external effect approval at execution. Adding another component or action changes catalog data rather than adding another MCP tool. Contracts v2 through v5 keep their direct manifest tools for compatibility.

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
