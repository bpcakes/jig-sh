# jig.sh

[![Tests](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml)
[![Crates.io](https://img.shields.io/crates/v/jig-sh)](https://crates.io/crates/jig-sh)

> **Keep coding agents on contract.**

Coding agents are good at writing code and bad at operating repositories. They
guess the test command, skip the lint step, run checks from the wrong directory,
and leave no record of what they verified. Every repository answers this with a
different mix of Makefiles, scripts, and prose, and every agent has to rediscover
it.

Jig replaces that with one repo-local contract. It turns your repository's
commands, ownership boundaries, and definition of done into a typed catalog that
humans, CI, and agents all run through the same entrypoint, `scripts/jig`, with
every run recorded. Around that contract it ships the local runtime a
repository needs day to day: a dev proxy with stable hostnames, an encrypted
vault for secrets, scheduled agent loops, and a terminal dashboard.

## See it in 30 seconds

Add Jig to a Rust, Go, or TypeScript repository you already have. Adoption
previews by default and writes nothing until you re-run it with `--write`:

```sh
jig adopt .            # preview: components found, files to create, warnings
jig adopt . --write    # apply the reviewed preview
scripts/jig setup      # bootstrap dependencies and verify the contract
```

The preview shows what Jig found and what it would change (trimmed):

```text
adopt summary
  mode: preview
  managed files: 14 created, 1 modified, 0 removed
  review:
    - component api at .: included (high confidence; root Cargo manifest; evidence: Cargo.toml)
    - stack: Rust crate
  warnings: 1
    - The generated Clippy command checks all Cargo features. ...
  next steps:
    - Review the adoption preview and managed-file diff.
    - Re-run jig adopt . --write after reviewing the summary.
```

From then on, every operator uses the same commands and gets the same answers:

```text
$ scripts/jig info targets
Jig targets: ExampleProject (contract v9)
  Targets: 7
  - api:clippy
  - api:fmt
  - api:test
  - api:test-locked
  - repo:bootstrap
  - repo:contract
  - repo:file-budget

$ scripts/jig check fmt
[ok] Repository target 'api:fmt' (289ms)
Jig check: passed
  Plan: run-plan_sha256:15d160c9...
  Targets: 1/1 executed
  - api:fmt: passed (exit 0)
```

Add `--json` to any command for structured output. Add `--affected BASE` to a
check to run only the targets whose declared inputs changed since `BASE`. Every
check appends a record to `.agent/state/runs.jsonl`, so "the tests passed"
becomes something you can inspect with `scripts/jig state summary` instead of
something you take on trust.

## Why Jig

- **Agents stop guessing.** The command catalog, the root `AGENTS.md`, and the
  per-crate `agent-map.md` are generated together, so an agent learns how to
  operate the repository from the repository itself.
- **One entrypoint for humans, CI, and agents.** The same `scripts/jig check test`
  runs on a laptop, in the generated GitHub Actions workflows, and inside an
  agent session. Nobody maintains three copies of the test command.
- **Checks leave evidence.** Each run records its target, exit code, and output
  tail in repo-local history. Failed runs can be reviewed after the fact, and
  scheduled agent loops record what each occurrence observed and did.
- **Affected selection from checked-in policy.** Actions declare their inputs,
  so `--affected` picks targets from what actually changed instead of running
  everything or trusting an agent to choose.
- **Updates never clobber your code.** `jig update` advances the managed harness
  files and refuses to overwrite files you customised unless you pass `--force`.
  Scaffolded application code is project-owned from the first render.
- **Local runtime, no service.** The dev proxy, the vault, agent loops, and the
  dashboard all run on your machine against local state. Nothing phones home.

A Makefile and a hand-written `AGENTS.md` get you part of the way. What they do
not give you is machine-readable targets with declared inputs and effects,
affected selection that CI and agents can trust, a record per check, and harness
updates that know which files you changed.

Jig is for teams running coding agents such as Claude Code or Codex against
real repositories, and for anyone who wants `scripts/jig check test` to mean the
same thing on every machine. It is not a build system or a CI provider: it does
not replace Cargo, Go tooling, package managers, Nx, Turborepo, Dagger, Taskfile,
or GitHub Actions. It gives those tools one stack-neutral front door and
connects their results to repository guidance and run history. Linux and macOS
are supported hosts; see [Platform Support](docs/platform-support.md).

## Install

Install the latest prebuilt CLI on Linux or macOS. The installer needs curl and
Python 3, not Rust:

```sh
curl -fsSL https://raw.githubusercontent.com/bpcakes/jig-sh/master/scripts/install.sh | bash
```

It selects your architecture, verifies the SHA-256 checksum and the executable's
version, and installs `jig` to `~/.local/bin`, printing the PATH command if
needed. Run it again to upgrade; an existing executable is replaced only after
verification succeeds. To choose an exact release or installation directory:

```sh
curl -fsSL https://raw.githubusercontent.com/bpcakes/jig-sh/master/scripts/install.sh | bash -s -- --version 0.7.2 --bin-dir "$HOME/.local/bin"
```

Linux binaries need glibc 2.35 or newer; macOS binaries need macOS 13 or newer.
Both x86-64 and ARM64 are available. You can also download and inspect the script
first, or unpack a verified archive from
[GitHub Releases](https://github.com/bpcakes/jig-sh/releases) yourself. Source
installation remains available for other hosts and needs Rust 1.88 or newer:

```sh
cargo install jig-sh --locked
```

You only need a global installation for the first `jig init` or `jig adopt`.
Generated repositories install a contract-compatible runtime through
`scripts/install-jig.sh` and expose it through `scripts/jig`. To pin an exact
published runtime, commit a `.jig/runtime-version` file; see
[runtime release pins](docs/configuration.md#runtime-release-pins).

## Features

### Start a new repository: `jig init`

`jig init` renders the harness and, if you choose a preset, a working
application alongside it in one pass. Run it bare for a guided wizard, or pass
the full shape for unattended use:

```sh
jig init ./ExampleProject                                           # guided wizard
jig init ./ExampleProject --preset rust-cli --no-input --no-vault   # one binary crate
jig init ./ExampleProject --preset rust-react --db postgres --frontends web,landing,admin
jig init ./ExampleProject --preset go-react --go-module example.com/example/project --db postgres --frontends web
cd ./ExampleProject && scripts/jig setup
```

| Preset | Generated project shape | Toolchain requirements |
| --- | --- | --- |
| `harness-only` | Jig harness files without application code | Rust 1.88+, Bash, Python 3.8+ |
| `rust-library` | Rust 2024 workspace with one library crate | Rust 1.88+, Bash, Python 3.8+ |
| `rust-cli` | Rust 2024 workspace with one binary crate | Rust 1.88+, Bash, Python 3.8+ |
| `rust-react` | Batter-based Rust API plus optional Vite React, Astro, and admin frontends | Unix, Rust 1.94+; Node.js 24.19.0+ and a supported package manager for frontends; database tools when enabled |
| `go-react` | Go API plus Vite React or Astro frontends | Go 1.26; Node.js 24.19.0+ and a supported package manager; PostgreSQL tools when enabled |

The application presets generate more than a skeleton. `rust-react` renders a
Cargo workspace with an API binary, core, HTTP, runtime, and test-support
crates, an optional SQLx database crate, crate-level agent guides, and shadcn
Vite React, Astro, or admin frontends, with the service lifecycle owned by
[Batter](https://github.com/bpcakes/batter). `go-react` renders a chi/Huma Go
API, optional pgxpool, sqlc, and Goose PostgreSQL support, and a Huma OpenAPI to
Hey API TypeScript client. The Rust-only presets give a virtual Rust 2024
workspace with one crate and a strict Clippy gate. Every preset also renders
the CI workflows, the dev proxy configuration where it applies, and the agent
guides for each crate.

Frontends live under `apps/<name>`, Rust libraries under `crates/`, and shared
TypeScript clients under `packages/`. `jig init ./ExampleProject --defaults`
selects `rust-react`. Run `jig presets` for the current layouts and rejected
combinations, and see [Initializing New Repos](docs/developer-ux.md#initializing-new-repos)
and [Rust applications on Batter](docs/rust-applications.md).

### Adopt a repository you already have: `jig adopt`

```sh
cd /path/to/repository
jig adopt .                 # preview only; nothing is written
jig adopt . --write         # apply after reviewing the preview
scripts/jig setup           # doctor, bootstrap, verify contract, doctor again
scripts/jig info targets    # what this repository can run
```

The preview lists each component candidate with its evidence, confidence, and
`included`, `excluded`, or `review_required` status. Use repeated
`--include-component ROOT` and `--exclude-component ROOT` flags to adjust the
selection, and repeat them with `--write`. Existing root files such as
`AGENTS.md` and `Makefile` are preserved; Jig changes only its marked or
explicitly managed sections. `jig adopt . --minimal` renders only `.jig.toml`
and the `.agent/` scaffolding for repositories that want loops without the full
harness. See [Adoption](docs/adoption.md) for workspace and command-inference
limits.

### Keep the harness current: `jig update`

```sh
jig update             # advance the template, preserving local changes
jig update --recopy    # re-render from the stored .jig.toml answers
```

`jig update` refuses to overwrite changed managed files unless `--force` is
passed, and never migrates or overwrites application source.

### Run checks through one contract: `check`, `run`, `info`

```sh
scripts/jig info targets                          # components, actions, profiles
scripts/jig check test                            # one target
scripts/jig check test --affected origin/main     # only targets whose inputs changed
scripts/jig run api:generate --explain            # preview a plan; creates no run
scripts/jig run api:generate --approve-effect worktree
scripts/jig run --profile verify --json           # a named profile, structured output
```

Targets are `COMPONENT:ACTION` pairs such as `api:test` or `web:lint`, grouped
into profiles like the default check profile and `verify`. Each target declares
its inputs, its effects, and a literal argv or explicit shell runner, which is
what lets `--affected` select by what changed and lets effectful targets demand
an explicit approval. Every executed plan appends a record to run history with
the target, exit code, and output tail. The generated GitHub Actions workflows
run the same targets through the same launcher.

The native `repo:file-budget` action enforces the repository-owned
`.jig/file-budget.toml` source-size policy; `scripts/jig file-budget audit`
gives the same diagnostics without creating a run. See
[Day-to-day workflow](docs/developer-ux.md#day-to-day-loop),
[action input declarations](docs/action-input-declarations.md), and the
[Public Contract](docs/public-contract.md).

### Local development with stable URLs: `dev` and `proxy`

Declare your apps once in `.jig.toml`. `scripts/jig dev` assigns ports, starts
them, waits for readiness, and publishes each one behind a stable hostname such
as `web.example-project.localhost`, so URLs, bookmarks, and API origins stop
depending on whichever port was free today.

```toml
[[dev.apps]]
name = "api"
kind = "env-port"
command = "cargo run --bin api"
port = 4000

[[dev.apps]]
name = "web"
dir = "apps/web"
kind = "vite"
argv = ["bun", "run", "dev"]
```

```sh
scripts/jig dev                                   # start every configured app behind the proxy
scripts/jig dev --app web                         # just one
scripts/jig dev status                            # sessions, supervisor, cleanup state
scripts/jig dev stop
scripts/jig proxy list                            # routes and runtime status
scripts/jig proxy alias api --port 8080           # give a hostname to something already running
scripts/jig proxy cert trust --accept-trust-scope # opt in to local HTTPS
```

Vite and Astro apps get host and port injection, so you stop editing package
scripts. A supervisor worker owns the app processes and route cleanup, so a
killed terminal does not leave orphans or stale routes behind. HTTPS, trust-store
changes, and LAN exposure are each explicit opt-ins rather than defaults. See
[Dev Proxy](docs/developer-ux.md#dev-proxy) and the
[`dev` configuration](docs/configuration.md#dev-shape).

### Secrets that stay out of the repository: `vault`

The repository, its run history, and your agent transcripts should hold
references, not values. Jig Vault keeps an encrypted bundle outside the
checkout. A dotenv file in the repo holds references such as
`jig://Production/RESTIC_PASSWORD`, and the values are resolved only for the
child process you run through the broker, with concealed values redacted from
its streamed output.

```sh
scripts/jig vault init
scripts/jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt
printf 'RESTIC_PASSWORD=jig://Production/RESTIC_PASSWORD\n' > .env.jig
scripts/jig vault exec --env-file .env.jig -- restic backup .
scripts/jig vault tui                             # keyboard-first manager
scripts/jig vault audit verify                    # tamper-evident audit log
scripts/jig vault backup create
scripts/jig vault import onepassword --env-file .env --item Production --out-env .env.jig --dry-run
```

Fields are concealed or text; both are encrypted at rest, and only concealed
fields are redacted, so modes and URLs stay readable. `vault exec` is the
analogue of `op run --env-file`, `vault read` and `vault inject` the analogue of
`op read`, and the one-time 1Password import turns `op://` references into
vault fields. Passphrase changes reseal the vault under a fresh key, a per-user
witness detects rolled-back vault files, and encrypted backups carry the vault
and its audit log between machines.

Vault reduces local exposure; it is not a sandbox or a production secret
manager, and a child that receives a value can disclose it. The generated agent
guidance keeps vault setup and the passphrase operator-owned, so an agent can run
a task through the broker but never sees or chooses the passphrase. See
[Vault runtime](docs/configuration.md#vault-runtime) and
[SECURITY.md](SECURITY.md).

### Operate coding agents: `claude`, `codex`, `agent`, `loop`

Jig treats agents as first-class operators of the repository, and ships the
tooling for the people running them:

```sh
scripts/jig claude homes             # list Claude Code configuration homes
scripts/jig claude launch work       # launch Claude Code with a selected home
scripts/jig codex homes              # list Codex homes and their accounts
scripts/jig codex resume <session>   # resume a Codex session from its owning home
scripts/jig agent doctor             # check that Jig's Codex skills are registered
scripts/jig loop status              # configured workflows, leases, and attempts
scripts/jig loop dispatch            # run due occurrences; call from cron or launchd
scripts/jig loop show <id>           # what one occurrence observed and did
```

The generated `AGENTS.md` tells agents to discover targets with
`scripts/jig info targets`, validate changes with focused checks, and treat
`.agent/state/` as append-only memory. `jig loop` runs bounded, compiled-in
workflows such as a `codex_task` prompt on a cron schedule and records the
evidence from every occurrence. See
[coding agents](docs/configuration.md#coding-agents) and
[Scheduled Codex Tasks](docs/codex-task-operations.md).

### See what happened: `ui`, `status`, `state`

```sh
scripts/jig ui              # terminal dashboard: Status, Timeline, and Health tabs
scripts/jig status --tui    # same dashboard, starting on Status
scripts/jig status --json   # one local status snapshot for scripts
scripts/jig state summary   # runs and target results recorded locally
scripts/jig state diagnose  # run-history growth and archive candidates
```

The dashboard is read-only over local repository and recorder state: recent
failed targets, per-target check health, loop workflows, and a filterable
timeline of finished targets. Collection failures show up as partial status
instead of hiding the state that is still usable. See
[Terminal Dashboard](docs/developer-ux.md#terminal-dashboard) and
[Runtime State](docs/public-contract.md#runtime-state).

## What lands in your repository

```text
.
├── .jig.toml                   # public configuration and renderer answers
├── AGENTS.md                   # repo-wide agent guidance
├── agent-map.md                # index of nested agent guides
├── .agent/
│   ├── jig-contract.json       # versioned command catalog
│   └── state/                  # append-only run history and runtime records
├── scripts/
│   ├── jig                     # repo-local launcher
│   └── install-jig.sh          # compatible runtime installer
└── .github/workflows/          # generated policy and test workflows
```

`.agent/jig-contract.json` is the stable authority: components, actions,
targets, profiles, declared inputs and effects, and literal argv runners. Run
history under `.agent/state/` is local execution evidence and is not committed.
Proxy and vault state live outside the checkout under `~/.jig`.

## Status

Jig is pre-1.0. Released runtimes render contract v9; contracts v2 through v8
remain readable through documented compatibility paths. Contract epochs are
versioned independently of the Jig release, so a repository pinned to an older
runtime keeps working. Review the [Public Contract](docs/public-contract.md)
before wiring long-lived automation to Jig, and see [CHANGELOG.md](CHANGELOG.md)
for release notes.

## Documentation

- [Developer UX](docs/developer-ux.md): command surface and daily workflow
- [Configuration](docs/configuration.md): `.jig.toml`, presets, package managers, dev proxy, and vault options
- [Adoption](docs/adoption.md): previewing and adding Jig to an existing repository
- [Public Contract](docs/public-contract.md): contract epochs, CLI, runs, and state
- [Rust applications on Batter](docs/rust-applications.md): runtime ownership and operational limits
- [Action input declarations](docs/action-input-declarations.md): input and source-state declarations
- [Scheduled Codex Tasks](docs/codex-task-operations.md): unattended `codex_task` workflows
- [Platform Support](docs/platform-support.md): supported hosts and feature limits
- [`examples/`](examples/): visible `.jig.toml` answer files

## Contributing

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers the
repository layout, local validation, template snapshots, and the release
process. This repository is itself a Jig harness repo, so `scripts/jig` and the
[local validation](docs/local-validation.md) guide are the way to check changes.

## Security

Please report vulnerabilities privately as described in
[SECURITY.md](SECURITY.md). Do not include secrets, private repository contents,
or exploit details in a public issue.

## License

[MIT](LICENSE)
