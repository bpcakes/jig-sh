# Rust-only `jig init` presets

Status: delivery plan, converted to Beads and reviewed to evidenced steady state;
implementation has not begun.

This document defines the product behavior, architecture, compatibility rules,
validation strategy, and delivery graph for adding first-class greenfield Rust
library and Rust CLI presets to `jig init`.

The plan is intentionally self-contained. An implementation agent should be able
to take any delivery bead in section 25, read that bead and this document, and
work without access to the planning conversation that produced it.

## 1. Executive decision

Jig will add two public scaffold presets:

- `rust-library`, which creates a virtual Cargo workspace containing one
  library crate at `crates/<package>` that can be prepared for publication when
  the project supplies its real package metadata;
- `rust-cli`, which creates the same expandable virtual Cargo workspace shape
  containing one runnable binary crate at `crates/<package>`.

Both public presets will use one shared internal Rust-workspace scaffold model.
The shared model owns package naming, workspace metadata, Cargo policy, Jig
answer defaults, output-path planning, and common templates. The public preset
kind owns only the artifact-specific source files, manifest target shape,
descriptor text, and acceptance checks.

There will not be a public `rust-workspace` preset or alias in this release.
`rust-workspace` is an internal architectural term, not a third user workflow.
A public preset should communicate the first useful artifact the user receives.
An empty or behaviorally identical `rust-workspace` preset would force users to
choose between names that do not produce meaningfully different results and
would create a compatibility name that Jig must support indefinitely. The
`rust-library` output is already a virtual workspace and can grow by adding
members beneath `crates/`.

The new presets are greenfield scaffold choices. They do not replace `jig
adopt`, which remains the correct path for existing repositories of any Rust
layout. They do not add Rust-specific concepts to the runtime repository
contract. They author ordinary Cargo files and existing Jig Rust answers through
the same init transaction used by `rust-react` and `go-react`.

The release will preserve these existing defaults and compatibility points:

- bare interactive `jig init <path>` still defaults to choice 1,
  `rust-react`;
- `jig init <path> --defaults` still resolves omitted shape choices to
  `rust-react`, database `none`, and frontend `web`;
- existing numeric interactive choices 1, 2, and 3 retain their meanings;
- `rust-react`, `go-react`, and `harness-only` output remains byte-for-byte
  unchanged unless a behavior-preserving internal refactor requires a snapshot
  update with no rendered difference;
- `jig update` never rewrites the application files produced by either new
  preset;
- no new `.jig.toml` key, resolved-contract field, action kind, adapter kind,
  runtime language branch, or contract epoch is introduced.

## 2. Why this expansion exists

Jig currently supports three greenfield choices:

- a Rust HTTP API plus React/Astro applications;
- a Go HTTP API plus React/Astro applications;
- a harness-only repository with no application source.

That leaves a real gap between full-stack generation and no source generation.
Many Rust projects begin as reusable libraries, multi-crate library workspaces,
or command-line programs. Today those users must initialize a harness-only repo
and then manually reproduce choices that Jig already knows how to make for Rust
repositories: package-name normalization, Cargo workspace policy, crate-root
answers, generated Rust checks, CI path authority, agent guides, transactional
publication, and setup guidance.

The gap is specifically in greenfield generation. Existing pure Rust projects
can already use `jig adopt`, and adoption should remain flexible enough to infer
custom workspace shapes. Adding these presets must not narrow adoption or imply
that existing repositories should be rearranged to match generated layouts.

The product value is therefore:

1. a one-command path from an empty destination to a coherent Rust library or
   CLI repository;
2. the same deterministic Jig harness and Cargo check surface as other generated
   Rust projects;
3. an intentionally small starter that does not invent application layers the
   project has not earned;
4. an expandable workspace layout that avoids a future source move when a
   second crate is added;
5. clear preset discovery so users do not mistake `rust-react` for the only
   supported Rust shape.

## 3. Product outcomes

After this work, each command below is a complete noninteractive project-shape
decision:

```sh
jig init ./example-library \
  --preset rust-library \
  --no-input \
  --no-vault

jig init ./example-cli \
  --preset rust-cli \
  --no-input \
  --no-vault
```

The library command creates a repository that can immediately run:

```sh
scripts/jig setup
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
scripts/jig check test-locked
scripts/jig check contract
```

The CLI command supports the same checks and additionally creates a runnable
binary:

```sh
cargo run -p example-cli
```

The generated repository has:

- a root virtual Cargo workspace;
- one seed crate with a normalized package name;
- Rust 2024 edition and the same supported Rust floor as current generated Rust
  scaffolds;
- no default software-license grant and `publish = false` on the seed package
  until the repository owner chooses publication metadata deliberately;
- a full Jig harness unless the product later adds an explicitly compatible
  minimal-scaffold mode;
- `sqlx_enabled = false`;
- no database files;
- no JavaScript workspace;
- no frontend applications;
- no dev proxy application;
- root agent guidance that describes a Rust workspace rather than an application
  backend and does not recommend `scripts/jig dev`;
- Rust format, Clippy, test, locked-test, policy, contract, and agent-map checks;
- a root README that explains the generated shape and project-owned boundary;
- a crate-level `AGENTS.md` and refreshed root `agent-map.md`;
- a human and JSON init report that identifies the selected preset and every
  scaffold file;
- transactional all-or-nothing destination publication.

## 4. Non-goals

This release does not:

- add a public `rust-workspace` alias;
- create an empty workspace with no buildable package;
- add multiple user-selected seed crates;
- accept repeated `--crate`, `--member`, or package-role flags;
- infer domain layers such as `core`, `storage`, `http`, `runtime`, `tui`, or
  `test-support` for a new library;
- scaffold SQLx, PostgreSQL, SQLite, migrations, or database test harnesses for
  the new presets;
- scaffold a web UI, TUI framework, daemon, service, HTTP API, plugin system, or
  release automation;
- choose a third-party CLI argument parser;
- generate domain-placeholder APIs that users must delete;
- add generalized Cargo metadata authoring flags for license, repository,
  description, authors, categories, keywords, or publish policy;
- choose a legal identity, copyright holder, or license on the caller's behalf;
- run `cargo add`, `cargo new`, `cargo init`, or another mutable external
  generator during init;
- run a package registry operation before destination publication;
- generate or commit `Cargo.lock` inside the init transaction;
- change the behavior of `jig adopt`;
- migrate existing repositories into the new layout;
- teach `jig update` to modify scaffolded Cargo or Rust source;
- add a runtime model of libraries, CLIs, binaries, or Cargo targets;
- block on the separate contract-v6 or generic repository-action roadmap;
- treat the new presets as long-term managed application frameworks.

Database-backed reusable crates remain a valid adoption workflow and a possible
future preset expansion. They are excluded here because choosing a storage crate
name, migration ownership, public feature boundary, test database policy, and
SQLx metadata layout is materially more opinionated than creating a seed Rust
artifact.

## 5. Current implementation map

The implementation is centered in the `jig-sh` package at `crates/jig`.

### 5.1 CLI types and interaction

- `crates/jig/src/bootstrap_parts/part_01.rs` defines `ScaffoldOpts`,
  `ScaffoldPreset`, `ScaffoldDb`, and frontend scaffold arguments.
- `crates/jig/src/bootstrap_parts/part_02.rs` validates init flag combinations,
  applies preset-derived answer defaults, and maps preset values to existing
  backend answers.
- `crates/jig/src/cli/init_wizard.rs` implements interactive, `--defaults`,
  strict `--no-input`, and implicit non-terminal resolution.
- `crates/jig/src/bootstrap/presets.rs` owns preset names, descriptions,
  reserved names and roots, layout metadata, examples, and the `jig presets`
  report.
- `crates/jig/src/cli/bootstrap_run.rs` renders human `jig presets` and init
  summaries.

The current wizard treats only `rust-react` and `go-react` as application
presets. Those shapes require a database choice and at least one frontend. The
new Rust-only presets need to be complete without either prompt, so the code
must move from a binary “application preset” test to explicit preset
capabilities.

### 5.2 Scaffold planning and rendering

- `crates/jig/src/bootstrap/scaffold.rs` builds `InitScaffoldPlan`, applies
  scaffold-specific answers, renders files, and reports output.
- The current plan always contains `ScaffoldBackendPlan`, which has only Rust
  API and Go API branches.
- `crates/jig/src/bootstrap/scaffold/rust_workspace.rs` renders the current
  multi-layer `rust-react` backend workspace.
- `crates/jig/src/bootstrap/scaffold/go_workspace.rs` renders the Go backend.
- `crates/jig/src/bootstrap/scaffold/frontend/` renders frontend workspaces and
  applications for backend-bearing presets.
- `crates/jig/src/bootstrap/scaffold/write.rs` preflights, writes, and reports
  scaffold files.

The new presets should reuse the plan, path validation, name normalization,
template rendering, collision detection, transaction, and report machinery.
They should not enter frontend rendering with fake backend context.

### 5.3 Init transaction and harness rendering

- `crates/jig/src/bootstrap/init.rs` resolves answers, builds the scaffold plan,
  reserves scaffold paths against template ownership, renders the Jig harness,
  renders project source, refreshes `agent-map.md`, initializes Git, and commits
  the init transaction.
- `crates/jig/src/bootstrap/init_transaction/` provides staged publication,
  destination identity checks, rollback, and resource budgeting.
- `crates/jig/src/bootstrap/scaffold/write.rs` uses the same guarded atomic file
  publication primitives as other scaffolds.
- `crates/jig/src/bootstrap/managed_paths.rs` ensures scaffolded application
  code is not adopted as template-managed source.

No second publication path is needed. The Rust-only render must produce a
bounded `Vec<ScaffoldFile>` and flow through these existing stages.

### 5.4 Answers and generated contract

- `crates/jig/src/bootstrap/opts.rs` holds existing Rust answers such as
  `rust_crate_roots`, `sqlx_enabled`, and Rust check commands.
- `crates/jig/src/bootstrap/answers/raw_answers.rs` resolves defaults and renders
  the existing project contract.
- `templates/project/.jig.toml.jinja` renders repository configuration.
- `templates/project/.agent/jig-contract.json.jinja` renders the resolved
  contract.
- `templates/project/.github/workflows/rust-tests.yml.jinja` renders Rust CI.
- `templates/project/.github/workflows/repo-policy.yml.jinja` renders Rust
  policy checks.

The existing answer model is sufficient. Rust-only presets must set existing
answers correctly rather than add preset identity to the long-lived runtime
contract.

### 5.5 Scaffold template sources

- live scaffold sources are under `templates/scaffolds/`;
- crate-local snapshots are under
  `crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/`;
- `crates/jig/build.rs` generates the embedded template manifest and refreshes
  snapshots when `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1` is set;
- `crates/jig/src/bootstrap/scaffold/embedded_templates.rs` verifies live and
  snapshotted template parity in tests.

New template sources must be added to the live tree and refreshed into the
crate-local snapshot. Editing only a generated manifest or snapshot is not an
acceptable implementation.

## 6. Terminology

**Preset** means a named, one-time greenfield application-source shape selected
by `jig init --preset`. A preset is not stored as runtime orchestration
authority after init.

**Harness** means the Jig-owned repository files rendered from
`templates/project`, including `.jig.toml`, `.agent/`, scripts, workflows,
agent guidance, and MCP configuration.

**Scaffold** means project-owned Cargo, Rust, and README files created once by a
preset. `jig update` does not manage them.

**Rust-only preset** means `rust-library` or `rust-cli`. “Only” means the preset
does not generate another language or application stack; the resulting project
may later add any components it owns.

**Virtual workspace** means a root `Cargo.toml` with `[workspace]` and no root
`[package]`. The seed crate is a workspace member beneath `crates/`.

**Seed crate** means the one immediately useful crate produced by a Rust-only
preset. It establishes a buildable repository without claiming future package
decomposition.

**Artifact kind** means library or CLI. It is scaffold-time authority only and
does not become a Jig runtime contract discriminant.

**Package name** means the normalized kebab-case Cargo package name.

**Module name** means the Rust identifier form of the package name, with hyphens
replaced by underscores where source needs it.

**Project-owned boundary** means that after successful init, Cargo manifests,
Rust source, and generated README content belong to the repository. Jig updates
the harness but does not rewrite that content.

## 7. User workflows

### 7.1 Discover available presets

`jig presets` lists the existing presets first in their current order, then
`rust-library`, then `rust-cli`. Appending preserves the current JSON array
prefix and the established interactive numeric meanings.

Human output for each new preset includes:

- the preset name;
- a one-sentence artifact summary;
- the virtual workspace layout;
- the default Cargo and Jig policies;
- an explicit example;
- the project-owned boundary;
- non-goals stating that no database, frontend, or additional crate layers are
  generated.

JSON output uses the existing descriptor shape. No new descriptor field is
required:

```json
{
  "name": "rust-library",
  "summary": "Expandable Rust workspace with one library crate.",
  "defaults": [],
  "layout": [
    "Cargo.toml virtual workspace",
    "crates/<repo> library crate"
  ],
  "frontend_shorthands": [],
  "examples": [
    "jig init ./example-library --preset rust-library --no-input --no-vault"
  ],
  "ownership": "...",
  "non_goals": []
}
```

The exact prose is implementation-owned but must carry the semantics above and
remain covered by human and JSON summary tests.

### 7.2 Interactive init

The interactive project-shape menu retains:

1. `rust-react`;
2. `harness-only`;
3. `go-react`.

It appends:

4. `rust-library`;
5. `rust-cli`.

Text aliases accept the exact preset names plus concise unambiguous forms:

- `library` and `rust-library` select `rust-library`;
- `cli` and `rust-cli` select `rust-cli`.

Generic `rust` is not accepted because it does not distinguish the existing
Rust React shape from the two new shapes. Generic `workspace` is not accepted
because there is no public workspace preset.

After choosing either Rust-only preset, the wizard proceeds directly to the
existing vault interaction. It does not ask for database or frontend choices.
If the caller already supplied an incompatible flag, validation fails before
vault credential capture or destination mutation.

### 7.3 Strict noninteractive init

Each Rust-only preset is a complete shape by itself:

```sh
jig init ./ExampleLibrary \
  --preset rust-library \
  --no-input \
  --no-vault
```

and:

```sh
jig init ./ExampleCli \
  --preset rust-cli \
  --no-input \
  --no-vault
```

Strict mode does not require `--db`, `--frontend`, or an answers-file frontend.
The same holds for implicit strict mode when stdin or stderr is not a terminal.

An omitted preset remains an error in strict mode. The error should name all
valid complete alternatives without calling the Rust-only presets “application
presets that require database and frontend choices.”

### 7.4 `--defaults`

`--defaults` remains backward compatible:

- omitted preset becomes `rust-react`;
- an explicit `rust-library` or `rust-cli` remains unchanged;
- an explicit Rust-only preset does not receive database or frontend defaults;
- an explicit `go-react` retains Go defaults;
- an explicit `harness-only` retains harness-only defaults.

This means:

```sh
jig init ./ExampleLibrary \
  --preset rust-library \
  --defaults \
  --no-vault
```

produces the same project shape as the strict example. `--defaults` may still
fill common non-shape answers such as repository name and default branch through
the existing answer resolver.

### 7.5 Setup and first checks

After init, human next steps are:

```sh
cd <destination>
scripts/jig setup
scripts/jig check test
```

There is no `scripts/jig dev` next step because neither preset configures a
long-running dev application. There is no database setup step and no frontend
dependency-install guidance.

`scripts/jig setup` runs the existing Cargo bootstrap command. It may generate
`Cargo.lock`. The README and init notes tell users to commit the resulting lock
file because generated CI runs the locked test command.

### 7.6 Run the generated CLI

The CLI preset README includes:

```sh
cargo run -p example-cli
```

The generated binary exits successfully and prints a stable line containing
its Cargo package name and version. It takes no positional or option contract.
The source deliberately avoids choosing Clap, lexopt, argh, bpaf, or another
parser before the project has requirements.

The starter output is only a smoke behavior. The README calls it project-owned
and replaceable rather than presenting it as a stable user-facing CLI contract.

### 7.7 Extend the workspace

The library README explains that another crate can be added beneath `crates/`
and included in root workspace members. The initial member list is explicit,
not a wildcard. Explicit membership prevents an incidental nested manifest from
silently becoming production build authority.

The generated `rust_crate_roots = ["crates"]` answer already makes direct child
crate guides and Rust source policy discoverable. A new direct-child crate
should receive its own `AGENTS.md` when it has meaningful ownership or
invariants, consistent with the generated root guidance.

## 8. Preset naming and taxonomy

### 8.1 Public names

The exact public names are:

- `rust-library`;
- `rust-cli`.

They follow the existing language-purpose convention used by `rust-react` and
`go-react`, remain unambiguous in CLI help, and communicate generated artifact
kind.

### 8.2 Why not `rust-workspace`

Both new presets generate workspaces. Workspace is an implementation layout,
not the first artifact. A public `rust-workspace` could mean at least four
different products:

- an empty virtual workspace;
- a workspace with one generic library;
- a workspace with several opinionated layers;
- a harness-only repository prepared for later Cargo initialization.

Those meanings have different acceptance criteria. This plan chooses the useful
artifact and avoids reserving an ambiguous contract name.

### 8.3 Why the library is a workspace

A root package is simpler by one directory, but adding a second crate later
would require moving source or mixing a root package with nested members. The
target users for this expansion include reusable multi-crate repositories. A
virtual workspace with `crates/<package>` costs little, aligns with existing Jig
crate-root guidance, and grows without relocating the public crate.

### 8.4 Why the CLI uses the same layout

Using the same layout makes the Rust-only renderer, crate-root answer, policy
workflows, documentation, and future expansion consistent. A CLI can later add
supporting libraries under `crates/` without moving the binary package.

The CLI crate stays at `crates/<package>` rather than `apps/<package>` because
the two public presets share one scaffold contract and the generated repository
has only one artifact. `apps/` remains useful in the larger `rust-react` preset,
where deployable binaries and reusable crates are already distinct groups.

## 9. Exact generated layouts

### 9.1 `rust-library`

The scaffold-owned output is:

```text
README.md
Cargo.toml
crates/
└── example-library/
    ├── AGENTS.md
    ├── Cargo.toml
    └── src/
        └── lib.rs
```

The Jig harness separately owns and generates files such as:

```text
.agent/
.github/workflows/
.gitattributes
.gitignore
.jig.toml
.mcp.json
AGENTS.md
agent-map.md
scripts/
```

The two ownership sets must remain disjoint. Init preflight treats any collision
as a preset/template ownership bug even when `--force` is present.

### 9.2 `rust-cli`

The scaffold-owned output is:

```text
README.md
Cargo.toml
crates/
└── example-cli/
    ├── AGENTS.md
    ├── Cargo.toml
    └── src/
        └── main.rs
```

No `lib.rs` is generated for the CLI preset. A library target should be added
only when the project has reusable logic or test seams that justify it.

### 9.3 Files deliberately absent

Both presets omit:

- `LICENSE` or `LICENSE.*` because the generator has no authority to choose a
  license grant or copyright holder;
- `.env.example`;
- `migrations/`;
- `.sqlx/`;
- database crates;
- `apps/`;
- `openapi/`;
- `package.json`;
- JavaScript lockfiles;
- frontend directories;
- frontend contract scripts;
- generated dev-app entries;
- release workflow files owned by application policy;
- domain-specific examples or fixtures.

The base Jig template may still render generic scripts that are part of the
full harness. Conditional template logic must ensure frontend-only or
database-only workflows are absent or inactive exactly as it does for other
tooling-only Rust repositories.

## 10. Cargo workspace contract

### 10.1 Root manifest

Both presets render a root virtual manifest with this semantic shape:

```toml
[workspace]
resolver = "3"
members = [
  "crates/example-package",
]

[workspace.package]
edition = "2024"
version = "0.1.0"
rust-version = "1.94"
```

The exact Rust floor must come from one scaffold-owned constant or shared
template authority used by generated Rust presets. If implementation discovers
that the current supported floor changed before this bead starts, it must use
the then-current generated Rust floor everywhere and update documentation and
tests together. It must not leave `rust-react` and Rust-only presets claiming
different floors accidentally.

The root manifest has no `[workspace.dependencies]` table initially because the
library seed has no dependency and the CLI uses only `std`. Empty dependency
policy is clearer than placeholder dependencies.

The member list is explicit. A future feature may add a safe member-authoring
command, but wildcard membership is not introduced here.

### 10.2 Library package manifest

The library crate manifest has this semantic shape:

```toml
[package]
name = "example-library"
edition.workspace = true
version.workspace = true
rust-version.workspace = true
publish = false
```

The package is deliberately marked `publish = false`. A generic generator does
not know whether the caller intends a private crate, which legal entity owns the
work, or which license grant is appropriate. The preset therefore makes no
software-license claim and prevents an accidental registry publication. The
README tells users to choose a license or `license-file`, add description,
repository, documentation, categories, and keywords, and remove or change
`publish = false` only as part of an intentional release decision.

The manifest has no empty `[dependencies]` table unless Cargo formatting or an
established repository convention requires it.

### 10.3 CLI package manifest

The CLI crate manifest uses the same license-neutral, non-publishable package
metadata and adds:

```toml
[[bin]]
name = "example-cli"
path = "src/main.rs"
```

The explicit binary target makes the generated artifact contract obvious and
keeps tests independent of Cargo's implicit target inference. The package has
no parser or logging dependency.

### 10.4 Cargo lock policy

Init itself does not invoke Cargo and does not generate `Cargo.lock`. This
preserves deterministic, offline-capable destination publication and avoids a
network operation inside the transaction.

`scripts/jig setup` uses the generated Cargo bootstrap command and creates the
lock file when needed. Users commit `Cargo.lock` for both presets because:

- generated `test-locked` uses `--locked`;
- generated CI uses the locked test target;
- a workspace-wide lock records exact dependency resolution as the repository
  grows;
- committing it makes the repository's supported build graph reproducible.

The initial library has no external dependency, so this policy does not impose
a registry dependency merely to create the starter.

## 11. Generated Rust source contract

### 11.1 Library source

`src/lib.rs` contains crate-level documentation and no invented public domain
API. A compliant minimal source is semantically equivalent to:

```rust
//! Library entry point for `example-library`.
//!
//! Replace this module documentation with the crate's public contract as the
//! project takes shape.
```

It must:

- be rustfmt-stable;
- compile on the declared Rust floor;
- pass the generated strict Clippy command;
- build documentation with warnings denied in a focused acceptance test;
- contain no `todo!`, `unimplemented!`, panic placeholder, unsafe block, network
  operation, or environment lookup;
- avoid a fake function, type, or constant whose only purpose is making a test
  pass.

No unit test is needed for an empty documented library. The meaningful
acceptance oracle is that Cargo can build, test, lint, and document the package.

### 11.2 CLI source

`src/main.rs` provides one bounded smoke behavior:

```rust
fn main() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
}
```

The exact presentation may differ, but tests require:

- exit status zero with no arguments;
- one newline-terminated UTF-8 stdout line;
- the normalized package name;
- the Cargo package version;
- empty stderr;
- no argument parser or undocumented option behavior;
- rustfmt and strict Clippy success.

An integration test in the Jig source tree executes the generated binary. The
generated CLI project does not need to own a test that only reasserts its
replaceable starter output.

### 11.3 Unsafe and lint policy

The new manifests do not add a workspace-wide `unsafe_code = "forbid"` or a new
Clippy lint group. Some legitimate Rust libraries need carefully reviewed unsafe
code, and preset generation should not silently create a policy broader than
current generated Rust repositories.

The existing Jig check command remains authoritative:

```sh
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Projects can strengthen their workspace lint tables after init.

## 12. README and agent guidance

### 12.1 Root README ownership

Each Rust-only preset renders a root `README.md` because the base harness does
not create one and there is no frontend workspace README to fill the role.

The README includes:

- the normalized repository/package name;
- whether the starter is a library or CLI;
- the exact generated layout;
- prerequisites, including the supported Rust floor;
- setup and check commands through `scripts/jig`;
- `Cargo.lock` commit guidance;
- the CLI run command when applicable;
- a short expansion note for adding workspace crates;
- the project-owned scaffold boundary;
- a publishing metadata reminder for the library;
- a license/publication reminder explaining that the seed package starts with
  no `license`/`license-file` field and with `publish = false`;
- no claims about domain behavior, production readiness, or release support.

The README uses generic generated names and contains no Jig source-repository
paths or private fixture identifiers.

### 12.2 Root `AGENTS.md` neutrality

The full Jig harness still renders the managed root `AGENTS.md`, but Rust-only
repositories must not inherit guidance that calls every Rust crate a backend or
recommends a dev-service command that has no configured app. For the neutral
authored `workspace` component, the generated managed block therefore:

- says repository and crate guidance rather than backend-level guidance;
- uses `## Rust Defaults` rather than `## Backend Defaults`;
- retains the crate-root and crate-guide rules;
- omits the transport-layer rule because a library or CLI need not have a
  transport boundary;
- omits `scripts/jig dev` from preferred commands;
- says “For Rust changes” rather than “For backend changes” in done criteria;
- uses `## Crate Guide Conventions` and refers to Rust crates, not backend
  packages.

This is selected from the ordinary authored repository model, not from stored
preset history. Initial rendering already contains the neutral `workspace`
component described in section 16.4. Update and recopy can derive the same
guidance mode from that checked-in component and its Rust adapter, so no new
answer key or contract field is needed.

The existing managed-block bytes remain unchanged for Rust React, Go React,
harness-only, and compatibility answer shapes. A broad global terminology
rewrite would violate the release's compatibility boundary; the template uses
an exact semantic branch for a neutral authored Rust workspace instead.

### 12.3 Crate-level `AGENTS.md`

The seed crate guide follows the repository convention:

- `## Purpose`;
- `## Key entrypoints`;
- `## Edit here for X`;
- `## Invariants`;
- `## Common commands`.

The library guide names `src/lib.rs`. The CLI guide names `src/main.rs`. Both
state that the crate is project-owned and that its purpose text should be
updated when real behavior exists.

The guide must not claim invented domain ownership. It can state only artifact
kind, entrypoint, and repository policy.

### 12.4 Agent map refresh

The existing init flow refreshes `agent-map.md` after scaffold publication.
Acceptance tests require the new crate-level guide to appear exactly once and
the map check to pass.

No preset-specific agent-map writer is introduced.

## 13. CLI flag and answer compatibility matrix

The following table is normative.

| Input | `rust-library` | `rust-cli` | Rationale |
|---|---:|---:|---|
| `--repo-name` | accept | accept | Existing naming authority. |
| `--default-branch` | accept | accept | Existing Git/CI authority. |
| `--ci-github-runner` | accept | accept | Existing CI authority. |
| `--template` | accept | accept | Existing template-source authority. |
| `--template-mode` | accept | accept | Existing template-source authority. |
| `--vcs-ref` | accept | accept | Existing template-source authority. |
| `--force` | accept | accept | Existing transactional collision policy. |
| `--defaults` | accept | accept | Does not change explicit preset. |
| `--no-input` | accept | accept | Preset is a complete shape. |
| `--no-vault` | accept | accept | Existing vault lifecycle choice. |
| `--db` | reject | reject | No database artifact is defined. |
| `--frontend` | reject | reject | No frontend artifact is defined. |
| `--frontends` | reject | reject | No frontend artifact is defined. |
| `--go-module` | reject | reject | Existing Go-only authority. |
| `--frontend-app` | reject for scaffold shape | reject for scaffold shape | Existing-app wiring belongs to adopt/harness-only configuration, not a new pure Rust scaffold. |
| `backend_language = "go"` | reject | reject | Conflicts with generated Rust authority. |
| `sqlx_enabled = true` | reject | reject | Database support is out of scope. |
| `rust_migration_dir` | reject when nonempty | reject when nonempty | No migration owner exists. |
| `rust_sqlx_metadata_dir` | reject when nonempty | reject when nonempty | No SQLx metadata owner exists. |
| `schema_dump_enabled = true` | reject | reject | Implies SQLx behavior absent here. |
| explicit Rust check commands | accept | accept | Existing project-owned command overrides. |
| `rust_crate_roots` other than `crates` | reject | reject | The scaffold owns its initial layout; use adopt for custom existing layouts. |
| `harness_footprint = "minimal"` | reject | reject | Application source plus a partial harness is not an established init contract. |

Empty optional migration strings normalized away by the existing answer parser do
not create a conflict. Effective nonempty values do.

Errors must name the selected preset and the incompatible input. They must occur
before template resolution where the current preflight ordering permits, and
always before vault passphrase capture or destination publication.

## 14. Derived Jig answers

Both presets derive:

```toml
backend_language = "rust" # compatibility/render authority; not necessarily persisted in v6
sqlx_enabled = false
rust_crate_roots = ["crates"]
schema_dump_enabled = false
frontend_apps = []
```

They also derive these semantic values through existing answer machinery:

- `application_contracts_enabled = false`;
- Cargo bootstrap command appropriate for a root manifest;
- existing Rust fmt, Clippy, test, and locked-test commands;
- no frontend workspace roots;
- no generated frontend dev apps;
- no backend dev app;
- ordinary vault, status, execution, and agent-tooling defaults;
- the requested/default repository name and branch.

For the current authored repository contract, both presets also select a
bootstrap-only repository projection with:

- the existing repository policy component at `.`;
- one component named `workspace` rooted at `.`;
- the existing Rust adapter on that component;
- a neutral description such as “Primary Rust workspace”;
- ordinary Rust format, Clippy, test, locked-test, and file-policy actions;
- the existing default verification profile and compatibility aliases.

The component root remains `.` rather than only `crates/<package>` because the
workspace actions consume root `Cargo.toml`, `Cargo.lock`, and workspace-wide
Cargo configuration as well as crate source. This conservative authority is
truthful and avoids falsely narrow affected or freshness claims.

The component must not be named `api`, tagged `backend`, or described as a
primary application backend. That current compatibility projection is accurate
for the full-stack presets but false for a library or CLI workspace.

The scaffold plan must set `sqlx_enabled = false` explicitly. Relying on the
general answer default of true would make a migration directory mandatory and
would misrepresent the project.

The plan must set `rust_crate_roots = ["crates"]` when the caller did not supply
it. If the caller supplies the exact same effective root, acceptance is allowed;
if another root is supplied, validation fails because output layout and contract
authority would diverge.

No dev app is generated for `rust-cli`. Jig dev apps represent supervised,
long-running services with readiness and proxy behavior. Treating a one-shot CLI
as a dev service would transfer the wrong lifecycle semantics.

## 15. Init and presets reports

### 15.1 Scaffold JSON

The existing scaffold report remains additive and contract-compatible. For a
library it contains at least:

```json
{
  "preset": "rust-library",
  "repo_name": "example-library",
  "repo_name_sanitized_from": null,
  "db": "none",
  "frontends": [],
  "frontend_notices": [],
  "files_created": [],
  "files_modified": [],
  "files_unchanged": []
}
```

The CLI report differs only in preset and file paths. Keeping `db` and
`frontends` preserves the current report shape for automation. A new
`artifact_kind` field is not required because `preset` is exact and the report
is runtime diagnostic output rather than persistent contract authority.

### 15.2 Human init summary

The human summary identifies:

- `scaffold: rust-library` or `scaffold: rust-cli`;
- `db: none` only if the existing formatter prints database state for all
  scaffolds;
- no misleading backend/API wording;
- no frontend count;
- sanitized repository-name note when applicable;
- project-owned scaffold note;
- setup and test next steps;
- no database, frontend, or dev next step.

### 15.3 Preset ordering

`ScaffoldPreset::value_variants()` currently drives report order. New enum
variants are appended after existing variants so the first three JSON entries
remain stable. Tests assert names and order explicitly.

## 16. Internal architecture

### 16.1 Replace backend-only plan identity

`InitScaffoldPlan` currently assumes every scaffold has an HTTP backend. The
implementation will introduce a preset-shape enum with enough vocabulary for
all current presets without creating a runtime language model. A suitable
semantic shape is:

```rust
enum ScaffoldProjectPlan {
    RustReact(RustReactScaffoldPlan),
    GoReact(GoReactScaffoldPlan),
    RustOnly(RustOnlyScaffoldPlan),
}

struct RustOnlyScaffoldPlan {
    artifact: RustOnlyArtifact,
}

enum RustOnlyArtifact {
    Library,
    Cli,
}
```

Exact names may vary, but the following properties are required:

- `preset()` returns the exact public preset;
- `backend_language()` returns Rust for both new presets;
- `database()` returns `none` without pretending a DB plan exists;
- frontend rendering is callable only for React-bearing variants;
- Rust-only rendering cannot construct `FrontendBackendContext`;
- answer defaults are selected by typed variant rather than strings;
- report generation is exhaustive over every public preset;
- existing Rust React and Go behavior remains unchanged.

This is a bootstrap-only type. It must not escape into `jig-core`, repository
contracts, action planning, receipts, or generated application APIs.

### 16.2 Preset capabilities

Repeated match expressions in the wizard currently encode whether a preset
needs database/frontends/Go module. Add capability methods on
`ScaffoldPreset`, or an equivalent typed descriptor, for:

- `requires_database_choice()`;
- `requires_frontend_choice()`;
- `requires_go_module()`;
- `supports_database()`;
- `supports_frontends()`;
- `is_complete_without_shape_options()`;
- generated backend language;
- reserved backend dev names;
- reserved scaffold roots.

Names may vary. The intent is to centralize facts that the wizard, strict-mode
validator, flag validator, package-manager preflight, descriptor, and scaffold
planner currently repeat.

Do not add a generic map of arbitrary capabilities or deserialize preset
definitions from configuration. The set is compile-time product behavior and
small enough for an exhaustive Rust enum.

### 16.3 Common name context

Both new presets reuse existing repository/package normalization. The common
plan context retains:

- requested repository name;
- normalized repository name;
- normalized Cargo package name;
- Rust module name;
- default branch;
- CI runner.

It does not need a DNS label or web package manager for Rust-only variants.
Refactoring common fields into an enum-specific context is preferred over
filling unused strings with fake defaults.

Existing Rust React name validation includes the Cargo 216-byte artifact-path
boundary and normalized identifier checks. The Rust-only presets must reuse the
same relevant package-name and path-budget validation. DNS-only validation may
remain specific to the web/dev proxy shape.

### 16.4 Bootstrap-only repository projection

The current generated repository model uses an `api` component and a “Primary
application backend” description whenever compatibility answers select Rust.
The Rust-only presets need the same Rust adapter actions without that false
identity.

Add an internal, skipped render hint carried from `InitScaffoldPlan` through
`AnswerOpts` and `RenderAnswers`, or an equivalently scoped typed input, that
selects a neutral Rust workspace projection. It is not serialized as a new
`.jig.toml` key. Instead, `RepositoryRenderModel` uses it during initial render
to author ordinary existing `[repository]` components, actions, profiles, and
commands.

B02 owns this behavior together with the first internal Rust-only renderer that
consumes it. B01 may prepare type boundaries but must not land a dormant render
mode or change repository output. Keeping production behavior and its first
consumer in one task makes B01 genuinely behavior-preserving and makes a B02
rollback remove the entire internal feature slice.

The generated complete repository model then becomes checked-in authority.
`jig update` and recopy preserve it through the existing authored-model path and
do not need to rediscover the historical preset.

The same authored model also drives managed root guidance. Add a render-only
predicate such as `rust_workspace_guidance_enabled()` that is true when the
authored repository has the neutral root `workspace` component with a Rust
adapter and has no API/backend component identity. The exact method name may
vary, but it must derive from existing component and adapter records rather than
from a new persisted preset or artifact-kind key. This lets initial render,
update, and recopy select the same root `AGENTS.md` wording. It also keeps an
authored neutral Rust workspace truthful if it was produced by adoption rather
than by a historical preset.

The implementation must:

- keep the current `api` projection unchanged for `rust-react` and existing
  compatibility inputs;
- build a `workspace` component at `.` for both Rust-only presets;
- reuse the registered Rust adapter and existing command implementations;
- retain repository-wide policy actions and the Rust file-LOC action;
- preserve compatibility aliases for fmt, Clippy, test, and locked test;
- avoid tags or fields that make library/CLI artifact kind runtime semantics;
- produce deterministic component/action/profile ordering;
- render neutral root agent guidance and omit the dev-service recommendation
  for the neutral workspace projection;
- add no schema field, adapter kind, runner kind, or contract epoch.

This authoring distinction is compatible with the separate stack-neutral
monorepo roadmap: it uses the generic component/action model already present and
does not add another discovery or execution model.

### 16.5 Render dispatch

`render_files()` dispatches as follows:

- Rust React: current Rust backend files, then frontend workspace/apps;
- Go React: current Go backend files, then frontend workspace/apps;
- Rust library: shared Rust-only workspace files plus library source;
- Rust CLI: shared Rust-only workspace files plus CLI source.

The pure Rust branches return before frontend context construction. This makes
impossible states unrepresentable and prevents later frontend changes from
accidentally affecting pure Rust output.

### 16.6 Output-path planning

`output_paths()` must return exactly the scaffold-owned files before harness
rendering. This allows the existing reserved-output collision check and init
transaction budget to account for every generated leaf.

Tests compare `output_paths()` with rendered relative paths for both new
presets. Missing or extra entries are failures.

## 17. Template architecture

### 17.1 Live template tree

Use a shared internal source tree for common workspace files and artifact trees
for source-specific files. One acceptable layout is:

```text
templates/scaffolds/
├── rust-only/
│   └── workspace/
│       ├── Cargo.toml.jinja
│       ├── README.md.jinja
│       └── crate/
│           ├── AGENTS.md.jinja
│           └── Cargo.toml.jinja
├── rust-library/
│   └── crate/
│       └── src/
│           └── lib.rs.jinja
└── rust-cli/
    └── crate/
        └── src/
            └── main.rs.jinja
```

If README or crate manifest differences become awkward conditionals, separate
small templates are acceptable. The implementation should share policy, not
force unrelated prose into unreadable template branches.

### 17.2 Renderer module

Add a focused module such as:

```text
crates/jig/src/bootstrap/scaffold/rust_only_workspace.rs
```

It owns:

- common and artifact-specific template lists;
- output-path substitution;
- strict template context construction;
- rendered-file assembly;
- relative-path reporting.

It does not own CLI parsing, answer resolution, transaction publication, or
frontend behavior.

### 17.3 Template context

The strict MiniJinja context includes only values templates consume:

- `repo_name`;
- `package_name`;
- `module_name` if source uses it;
- `artifact_kind` only when a common template requires a branch;
- `rust_version` if centralized as a value rather than literal template policy.

Undefined values remain hard errors through the existing strict environment.

The project-template context additionally exposes the derived neutral-workspace
guidance predicate. It is computed from the authored repository model already
available to `RenderAnswers`; it is not accepted from CLI input, answers files,
or persisted configuration. `templates/project/AGENTS.md.jinja` uses that
predicate only to select the neutral terminology and command list specified in
section 12.2. All existing branches retain their current rendered bytes.

### 17.4 Embedded snapshots

After adding live templates, refresh the snapshot with the repository-supported
command:

```sh
JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
```

Review:

- the generated snapshot manifest;
- copied template snapshots;
- ordering;
- trailing newlines;
- absence of local absolute paths;
- absence of downstream or private identifiers.

Tests must pass both from a live checkout and with
`JIG_EMBEDDED_TEMPLATE_SNAPSHOT=1` so packaged builds do not omit the new
presets.

## 18. Transactional and filesystem safety

The new presets inherit the current init safety contract.

### 18.1 Destination preflight

Before answers, template resolution, vault interaction, or writes:

- missing destination is allowed;
- empty real directory is allowed;
- nonempty directory requires `--force`;
- non-directory and symlink destinations fail;
- platform support for atomic no-replace publication is verified.

No Rust-only code path may bypass this ordering.

### 18.2 Scaffold path validation

Every output path is repository-relative, normalized, bounded, and checked for
portable collisions. Generated package names must not produce paths that exceed
the established Cargo artifact budget.

Tests cover:

- hyphenated names;
- names requiring normalization;
- reserved or invalid Cargo names;
- case-folded output collisions;
- overlong package/path combinations;
- a symlink at `Cargo.toml`, README, crate manifest, source, or guide;
- a directory where a regular file is expected;
- a file where a directory ancestor is expected.

### 18.3 Template/scaffold ownership collision

The base template and scaffold output sets must be disjoint. A collision is a
Jig implementation bug and fails even with `--force`.

README is scaffold-owned for Rust-only presets. If the base template later adds
a README, its owners must resolve the collision explicitly rather than letting
publication order choose a winner.

### 18.4 Transaction budget

The init transaction accounts for every scaffold file, every template file,
agent-map refresh, Git initialization, and retained generation. Adding outputs
requires boundary tests near the configured cap so resource budgeting remains
conservative.

### 18.5 Rollback

Injected failures at these points restore or remove the destination according
to the existing transaction contract:

- after harness staging;
- while rendering a Rust-only template;
- before the first scaffold file;
- after `Cargo.toml`;
- after crate manifest;
- after Rust source;
- during agent-map refresh;
- during Git initialization;
- during final publication.

Existing destination content admitted by `--force` must be restored byte- and
mode-exactly on failure.

## 19. Compatibility boundaries

### 19.1 Existing preset output

Behavior-preserving refactoring is proven by existing tests plus explicit
before/after fixture comparisons for representative:

- Rust React, no database, web;
- Rust React, PostgreSQL, web/admin;
- Go React, no database, web;
- Go React, PostgreSQL, web/landing;
- harness-only strict init.

No current scaffold file, report field, next step, answer, or prompt choice may
change unintentionally.

The neutral root-guidance branch is covered in both directions: Rust-only
fixtures assert its workspace terminology and missing dev recommendation, while
representative Rust React, Go React, and harness-only fixtures assert their
existing managed `AGENTS.md` bytes. The new predicate must not become a global
template wording change.

### 19.2 CLI compatibility

Clap exposes new enum values additively. Existing flags retain meanings.
Interactive numbers 1–3 remain stable. Bare and `--defaults` behavior remains
stable.

Errors that currently say “application preset” should be revised only where the
new taxonomy makes them false. Tests should assert actionable semantics rather
than fragile whole paragraphs where practical.

### 19.3 Generated repository compatibility

The new scaffold renders through the current template contract epoch. Older Jig
binaries do not need to understand the preset after generation because the
preset is not stored as runtime authority. They only need to support the
generated contract, exactly like other repositories at that epoch.

### 19.4 Update and recopy

`jig update` and `jig update --recopy` update template-managed harness files.
They do not recreate, migrate, or overwrite:

- root Cargo manifest;
- root README;
- seed crate manifest;
- seed crate source;
- seed crate guide.

Managed-path manifest tests prove those paths are absent from harness ownership.

### 19.5 Adoption

Adoption inference remains layout-driven. It may infer a generated Rust-only
repository as `rust_crate_roots = ["crates"]`, but it does not need to infer the
historical init preset. No adoption report field is added.

## 20. Validation strategy

### 20.1 Unit tests

Add focused tests for:

- Clap parsing of both preset names;
- `as_str`, backend language, reserved names, roots, and capabilities;
- descriptor contents and order;
- human presets summary;
- interactive numeric and text aliases;
- strict-mode completeness;
- `--defaults` preservation;
- every invalid flag/answer combination in section 13;
- package normalization and module names;
- scaffold plan summary;
- answer defaults;
- exact output-path list;
- template-list presence;
- report preset identity.

### 20.2 Generation tests

For each preset, initialize a temporary generic fixture and assert:

- exact scaffold-owned file set;
- expected harness-owned files;
- absence list from section 9.3;
- root manifest parses as TOML;
- member manifest parses as TOML;
- workspace member path resolves;
- the workspace declares no `license` or `license-file`, the package declares
  `publish = false`, and no license file is generated;
- `.jig.toml` disables SQLx, records `rust_crate_roots = ["crates"]`, and
  contains a neutral authored `workspace` component rather than an API/backend
  component;
- resolved contract includes Rust checks and excludes Go, SQLx, frontend, and
  generated dev authority;
- Rust workflow path filters conservatively include every root Cargo and crate
  input; a `workspace` component rooted at `.` may intentionally collapse this
  to `**` rather than claim a narrower path set;
- no web workflow is rendered;
- no database workflow or gate is rendered;
- agent map links to the seed crate guide;
- root `AGENTS.md` uses neutral Rust-workspace/crate terminology, contains no
  backend-only transport rule, and does not recommend `scripts/jig dev`;
- scaffold report file classifications are exact.

### 20.3 Generated runtime tests

Run in generated fixtures:

```sh
CARGO_NET_OFFLINE=true cargo generate-lockfile --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
scripts/jig check contract
scripts/jig check agent-map
scripts/jig check agent-guides
```

For the CLI fixture also run:

```sh
cargo run --locked -p example-cli
```

Capture stdout/stderr and verify the contract in section 11.2.

Tests that call Cargo should use local toolchains and no network dependencies.
Because generated manifests have no registry dependencies, the focused fixture
can remain hermetic after Cargo itself is available. At least one end-to-end
fixture runs `scripts/jig setup` with `CARGO_NET_OFFLINE=true` before the locked
checks, proving the documented first-run path without silently reaching a
registry.

### 20.4 Init safety tests

Run representative collision, force, symlink, rollback, and output-budget tests
for at least one Rust-only preset. Add artifact-specific output-set assertions
for the other. Shared transaction behavior does not need a combinatorial copy
of every injection point for both artifacts.

### 20.5 Snapshot tests

Verify:

- live template manifest contains all new templates;
- checked-in snapshot contains the same relative paths and bytes;
- snapshot-only build can render both presets;
- all rendered files end in a newline where text conventions require it;
- MiniJinja strict undefined behavior catches missing context.

### 20.6 CLI JSON tests

Extend `crates/jig/tests/cli_json.rs` to prove:

- `jig presets --json` returns both descriptors;
- init JSON reports exact preset, empty frontend list, `db: none`, and scaffold
  file classifications;
- usage errors remain one JSON error object;
- no progress text contaminates JSON stdout.

### 20.7 Full repository gates

Implementation finishes with a freshly built dev binary:

```sh
cargo build -p jig-sh --bin jig
export JIG_DEV_BIN=target/debug/jig
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
scripts/jig check contract
scripts/jig check agent-map
scripts/jig check agent-guides
```

If configured path policy marks an unrelated gate not applicable, the work
receipt should state that explicitly rather than fabricating evidence.

## 21. Documentation changes

Update all user-facing places that currently imply only three preset choices:

- `README.md` quick start and preset examples;
- `docs/developer-ux.md` init/adopt distinction, guided flow, generated layouts,
  and ownership boundary;
- `docs/configuration.md` answer behavior, preset compatibility, setup, Cargo
  lock policy, and scaffold snapshot notes where relevant;
- generated root `AGENTS.md` terminology and preferred-command behavior for a
  neutral authored Rust workspace;
- CLI long help in `crates/jig/src/bootstrap_parts/part_01.rs`;
- `jig presets` descriptors;
- wizard menu and strict-mode diagnostics;
- any doctor recovery text that lists complete init shapes;
- tests that snapshot or assert those messages.

Documentation must say:

- use `init` for a new destination;
- use `adopt` for an existing Rust repository;
- `rust-library` creates an expandable virtual workspace with one library;
- `rust-cli` creates an expandable virtual workspace with one binary;
- neither adds database or frontend source;
- `rust-workspace` is not a public preset;
- app source is generated once and project-owned;
- commit `Cargo.lock` after setup.

Do not include names, paths, or operational details from downstream/private
projects. Use `ExampleLibrary`, `ExampleCli`, `example-library`, and
`example-cli` fixtures.

## 22. Performance, supply-chain, and security considerations

### 22.1 Init performance

The new presets render only a handful of static templates and should be cheaper
than current full-stack scaffolds. No performance target is asserted without a
measurement. Existing init transaction tests are sufficient unless new output
planning introduces a measurable regression.

### 22.2 Network and supply chain

Init executes no Cargo command and selects no new dependency. The CLI uses
`std`, so this feature adds no parser dependency or registry resolution to the
generated source.

Setup may invoke Cargo through the generated bootstrap command, consistent with
existing harness behavior. The source and lock policy is visible to the user.

### 22.3 Path and identifier safety

All names pass current Cargo/package/path normalization before any write.
Templates never interpolate values into executable shell commands. Generated
README and Rust string content use renderer-controlled normalized names.

### 22.4 Secrets

The presets add no secret field, environment input, or credential file. Vault
setup remains in the existing post-shape interaction. Validation of incompatible
shape inputs must finish before vault credential capture.

### 22.5 Open-source fixture hygiene

All tests, docs, plan content, receipts, and Beads descriptions use generic
identities. Receipt-producing commands run only after fixture names have been
reviewed.

## 23. Rollout and release behavior

### 23.1 One coordinated release

The new CLI values, renderers, templates, snapshots, docs, and tests ship in one
coordinated Jig release. There is no persisted state migration and no staged
deployment need.

### 23.2 Failure behavior

If a release binary somehow lacks embedded templates for a new preset, plan
construction fails before destination mutation with an exact missing-template
diagnostic. Snapshot-only tests prevent shipping that state.

### 23.3 Telemetry

No telemetry is added. Human and JSON init reports already expose selected
preset and generated files to the caller.

### 23.4 Future extensions

Evidence that could justify later work includes:

- repeated demand for an empty workspace distinct from a library;
- repeated multi-seed-crate generation patterns;
- a stable storage-library layout shared by several new projects;
- a parser preference strong enough to justify a CLI framework option;
- a release/publish workflow that can be generic without claiming project
  policy.

Those changes require their own plans and must not be anticipated through
dormant flags or generic schema in this release.

## 24. Delivery graph

The delivery graph uses one epic, three feature groupings, and seven concrete
tasks. Planning/review/conversion are not beads.

```text
Epic: First-class Rust-only init presets
│
├── F1: Generalize scaffold planning for Rust-only artifacts
│   ├── B01: Refactor preset capabilities and backend-only plan assumptions
│   └── B02: Add shared Rust-only workspace renderer and templates
│       depends on B01
│
├── F2: Deliver public Rust library and CLI workflows
│   ├── B03: Ship explicit rust-library init
│   │   depends on B02
│   ├── B04: Ship explicit rust-cli init
│   │   depends on B03
│   └── B05: Integrate guided discovery and strict/default interaction
│       depends on B04
│
└── F3: Harden and document the complete preset family
    ├── B06: Prove transaction, snapshot, report, and generated-repo quality
    │   depends on B05
    └── B07: Complete docs, dogfood gates, and release acceptance
        depends on B06
```

B04 depends on B03 rather than running in parallel because both add exhaustive
matches to the same preset enum, plan dispatcher, report code, and central tests.
The sequence lets the CLI reuse the public library pattern and avoids resolving
the same central conflicts twice. B06 follows B05 because adversarial
end-to-end acceptance must run against the settled explicit and guided CLI
surface. B03 and B04 own finalized descriptor/report behavior, B05 owns only
interaction/help/diagnostics, and B06 composes those positive oracles rather
than redefining them. The explicit edge is cheaper and more truthful than
making two agents negotiate the same central integration tests.

The features are organizational parents, not blocking implementation gates.
Blocking edges are attached directly between concrete tasks.

### 24.1 Execution contract for every delivery task

Every B01–B07 task is substantial work and must use the repository's structured
workflow. Before editing production files, the implementing agent must:

1. verify the task is ready with `br show <task-id> --json` and
   `br ready --epic jig-sh-rust-only-init-presets-zc7 --type task --json`;
2. claim only the concrete task record, never the epic or feature containers;
3. create and maintain a task-local ExecPlan under `.agent/` following
   `.agent/PLANS.md`, capturing the exact Git baseline, decisions, discoveries,
   progress, validation, and recovery guidance;
4. start structured work with `scripts/jig work start` and record the returned
   plan ID in the ExecPlan and a Beads comment;
5. use the freshly built development binary through `JIG_DEV_BIN` for runtime
   changes; and
6. before closure, run the applicable focused checks plus `scripts/jig work
   check`, `scripts/jig work gates`, `scripts/jig work evidence`, and
   `scripts/jig work finish` with an exact resolution and `--outcome success`
   for that plan ID.

A task cannot close solely because its local unit tests pass. Its ExecPlan,
receipts, applicable gate state, acceptance evidence, and Beads description must
agree. If implementation reveals that a task is too broad, split concrete
delivery work and update the DAG rather than creating a planning-only bead.

## 25. Delivery bead specifications

### Epic — First-class Rust-only `jig init` presets

#### Outcome

Developers can create a coherent, expandable Rust library or Rust CLI repository
with one explicit `jig init` preset, while existing preset output, adoption,
runtime contracts, and transactional safety remain compatible.

#### Scope

Add public `rust-library` and `rust-cli` presets over one shared internal
Rust-workspace renderer. Generate one project-owned seed crate, existing Jig Rust
answers and checks, discovery/help/report integration, embedded template
snapshots, documentation, and hermetic acceptance fixtures.

#### Product boundaries

Do not add a public `rust-workspace` alias, database/frontend scaffold, multiple
seed crate API, parser framework, runtime artifact model, new contract field, or
adoption migration. Do not choose a software license or make the seed package
publishable before the repository owner supplies deliberate release metadata.

#### Acceptance criteria

- Both explicit noninteractive commands create buildable repositories.
- Interactive discovery lists them without changing existing defaults or
  numeric meanings.
- Both layouts have a root virtual Cargo workspace and one package beneath
  `crates/<package>` with its manifest and crate guide; the library owns only
  `src/lib.rs`, the CLI owns only `src/main.rs`, and both own a root README.
- Generated contracts contain ordinary Rust checks and no SQLx/frontend/dev
  authority.
- Existing presets remain behaviorally compatible.
- Init stays transactional and snapshot-backed.
- Seed packages are license-neutral and non-publishable by default.
- Every delivery task has a completed task-local ExecPlan and structured work
  receipts.
- All delivery tasks are closed with required gates green.

### Feature F1 — Generalize scaffold planning for Rust-only artifacts

#### Outcome

Bootstrap planning can represent full-stack backend scaffolds and small
Rust-only artifacts without fake backend/frontend state or runtime schema
expansion.

#### Scope

Refactor typed preset capabilities and plan dispatch, then add the shared
Rust-only workspace renderer and template foundation. Keep public existing
preset behavior unchanged until the delivery tasks expose new values.

#### Acceptance criteria

- Backend-only assumptions are removed from common scaffold planning.
- Frontend rendering is unreachable for Rust-only plans.
- Existing preset output and reports remain compatible.
- Shared workspace policy is represented once.
- No runtime or repository-contract type is added.

### B01 — Refactor preset capabilities and backend-only plan assumptions

#### Outcome

The init wizard, strict validator, scaffold planner, and report code consume one
typed source of preset capabilities, and `InitScaffoldPlan` can represent a
non-backend artifact without unused frontend/database state.

#### Context

Current code repeats `RustReact | GoReact` matches to mean “requires database and
frontend,” and `InitScaffoldPlan` always contains `ScaffoldBackendPlan`. Adding
small Rust presets by inserting more special cases would spread contradictory
taxonomy across CLI, preflight, planning, and reporting.

#### Scope

- Add explicit compile-time capability methods or a typed descriptor to
  `ScaffoldPreset`.
- Refactor wizard/default/strict/package-manager decisions to those methods
  without exposing new preset values yet.
- Replace or generalize backend-only scaffold plan dispatch so a Rust-only plan
  variant can be added cleanly.
- Separate common naming/branch/CI context from web-only package-manager and DNS
  context where doing so removes fake values.
- Make report and answer methods exhaustive through typed plan methods.
- Preserve existing frontend backend contexts and dev-app behavior.
- Add behavior-preserving unit and fixture tests.

#### Required tests

- Existing wizard choices, aliases, and defaults remain exact.
- Existing strict-mode errors remain semantically actionable.
- Package-manager preflight still runs only for frontend-bearing presets.
- Representative Rust React and Go React rendered files/reports are unchanged.
- Harness-only remains scaffold-free.
- A compile-time test plan can represent a non-backend shape without rendering
  files, constructing frontend context, or changing repository output.

#### Acceptance criteria

- Preset requirements are not encoded through repeated ad hoc enum unions.
- Common plan code does not require every shape to have an HTTP backend.
- Existing preset outputs, reports, answers, and next steps remain compatible.
- No public new preset is partially exposed.
- No render hint, neutral repository projection, contract-schema field, adapter
  kind, runner kind, persisted answer key, template, or generated file is added
  in this refactor task.

#### Execution workflow

Before production edits, create and maintain a task-local ExecPlan under
`.agent/` according to `.agent/PLANS.md`. Start structured work and record the
returned ID in both the ExecPlan and a Beads comment:

```sh
plan_id="$(scripts/jig work start --title "<task ID and outcome>" \
  --body "Execute the owning Beads acceptance criteria." --print-plan-id)"
```

Before closure, run the task's focused checks and complete:

```sh
scripts/jig work check --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id"
scripts/jig work evidence --plan-id "$plan_id"
scripts/jig work finish --plan-id "$plan_id" \
  --resolution "<task acceptance complete>" --outcome success
```

#### Dependencies and unblocks

No blocking dependency.

Unblocks B02.

### B02 — Add the shared Rust-only workspace renderer and templates

#### Outcome

Bootstrap has one internal renderer that can produce the common virtual
workspace, manifest, README, crate guide, and artifact-specific Rust source for
library or CLI plans.

#### Context

Both public presets need identical naming, workspace policy, crate roots, Rust
floor, output planning, and project-owned boundaries. Copying complete renderer
branches would invite drift; forcing them through `rust-react` would create fake
API, database, frontend, DNS, and dev state.

#### Scope

- Add `RustOnlyScaffoldPlan` and artifact kind internally.
- Add `rust_only_workspace.rs` or an equivalently focused renderer module.
- Add live templates for common workspace files, library source, and CLI source.
- Add strict template context and exact output paths.
- Add the crate-private, non-serialized render hint for a neutral Rust workspace
  component and teach `RepositoryRenderModel` to author it through the existing
  component/action/profile schema as part of this first consuming feature.
- Derive neutral root-guidance mode from that authored `workspace` component and
  Rust adapter, and condition the managed root `AGENTS.md` without persisting
  preset or artifact identity.
- Reuse relevant package normalization and Cargo path-budget checks.
- Render generic README and crate guides with artifact-specific content.
- Refresh embedded scaffold snapshots.
- Add internal generation tests for both artifact kinds without public CLI
  exposure.

#### Required tests

- Exact output set for internal library and CLI plans.
- TOML parse and workspace member resolution.
- License-neutral manifests with `publish = false` and no generated license
  file.
- Rustfmt-stable source.
- Missing template/context fails precisely.
- Live/snapshot template parity.
- Normalized and maximum-boundary package names.
- Output paths equal rendered relative paths.
- No DB/frontend/dev files or context.
- A Rust-only projection produces `workspace` Rust actions and no `api`/backend
  description, while the existing Rust React projection remains unchanged.
- Root guidance uses Rust workspace/crate terminology, omits the transport rule
  and `scripts/jig dev`, and re-renders identically during update/recopy; existing
  preset managed-block bytes remain unchanged.

#### Acceptance criteria

- One shared renderer owns common Rust-only workspace policy.
- Artifact branches differ only where the generated artifact differs.
- Templates are live-source-owned and snapshot-backed.
- Generated content uses only generic fixture identities.
- The neutral projection uses only ordinary existing component/action records;
  no contract field, adapter kind, runner kind, persisted answer, or epoch is
  added.
- Neutral root guidance is recoverable from authored repository semantics and
  does not depend on historical preset identity.
- No public preset is exposed until its end-to-end answer/report path exists.

#### Execution workflow

Before production edits, create and maintain a task-local ExecPlan under
`.agent/` according to `.agent/PLANS.md`. Start structured work and record the
returned ID in both the ExecPlan and a Beads comment:

```sh
plan_id="$(scripts/jig work start --title "<task ID and outcome>" \
  --body "Execute the owning Beads acceptance criteria." --print-plan-id)"
```

Before closure, run the task's focused checks and complete:

```sh
scripts/jig work check --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id"
scripts/jig work evidence --plan-id "$plan_id"
scripts/jig work finish --plan-id "$plan_id" \
  --resolution "<task acceptance complete>" --outcome success
```

#### Dependencies and unblocks

Depends on B01.

Unblocks B03.

### Feature F2 — Deliver public Rust library and CLI workflows

#### Outcome

Explicit and guided `jig init` workflows expose the two new presets with
complete validation, answer derivation, reports, and user guidance.

#### Scope

Ship the public enum values sequentially, then integrate the interactive menu,
strict/default behavior, preset report, help, and diagnostics across the full
preset family.

#### Acceptance criteria

- Each public value works as a complete explicit noninteractive shape.
- Incompatible database/frontend/Go/SQLx answers fail before mutation.
- `jig presets` describes exact output and boundaries.
- Interactive and default compatibility rules hold.

### B03 — Ship explicit `rust-library` init

#### Outcome

`jig init <path> --preset rust-library --no-input --no-vault` creates the library
layout and full Jig harness with correct Rust-only answers, reports, next steps,
and project-owned boundaries. A caller that wants initial vault setup supplies
the established passphrase environment authority instead of `--no-vault`.

#### Scope

- Add the public `RustLibrary` enum value and exact CLI spelling.
- Connect it to the internal library renderer.
- Add descriptor metadata at the end of current preset order.
- Derive Rust backend compatibility, SQLx false, crate root `crates`, no
  frontend/application contracts, no dev apps, and ordinary Rust commands.
- Validate the complete task-local input contract below.
- Report `preset = rust-library`, `db = none`, empty frontends, and exact files.
- Add explicit noninteractive init and generated contract tests.
- Update only the minimum help text needed for an explicit user to discover the
  value; B05 owns the full guided UX pass.

#### Complete task-local contract

The scaffold-owned file set is exactly root `README.md`, root `Cargo.toml`,
`crates/<package>/AGENTS.md`, `crates/<package>/Cargo.toml`, and
`crates/<package>/src/lib.rs`. It contains no license file, environment example,
migration or SQLx metadata tree, database crate, `apps/`, OpenAPI tree,
JavaScript manifest or lockfile, frontend contract script, dev-app entry, or
release workflow.

Accept existing common init authority: `--repo-name`, `--default-branch`,
`--ci-github-runner`, `--template`, `--template-mode`, `--vcs-ref`, `--force`,
`--defaults`, `--no-input`, and `--no-vault`. Accept explicit Rust check-command
overrides and an effective `rust_crate_roots = ["crates"]`.

Reject `--db`, `--frontend`, `--frontends`, `--frontend-app`, `--go-module`,
`backend_language = "go"`, `sqlx_enabled = true`, nonempty
`rust_migration_dir`, nonempty `rust_sqlx_metadata_dir`,
`schema_dump_enabled = true`, any effective Rust crate root other than
`crates`, and `harness_footprint = "minimal"`. Empty optional migration values
that the existing parser normalizes away are not conflicts. Every rejection
names `rust-library` and the incompatible input and occurs before vault capture
or destination publication.

#### Required tests

- Clap parse and invalid spelling.
- Strict/no-terminal complete shape.
- `--defaults` preserves explicit library.
- Every incompatible flag family fails prepublication.
- Exact library files and absence list.
- Generated `.jig.toml` and contract semantics.
- Neutral `workspace` component identity and Rust action aliases.
- Neutral root `AGENTS.md` guidance with no backend-only transport rule or
  `scripts/jig dev` recommendation.
- Cargo fmt, Clippy, test, locked test, and docs.
- Update/recopy does not own scaffold files.
- JSON and human init summary.

#### Acceptance criteria

- The explicit command creates a buildable documented library workspace.
- No database, frontend, API, dev app, or parser dependency appears.
- No license grant is implied and accidental publication is disabled.
- Setup generates a lock file and locked checks pass after it is present.
- Report and next-step output are truthful.
- Existing presets remain compatible.

#### Execution workflow

Before production edits, create and maintain a task-local ExecPlan under
`.agent/` according to `.agent/PLANS.md`. Start structured work and record the
returned ID in both the ExecPlan and a Beads comment:

```sh
plan_id="$(scripts/jig work start --title "<task ID and outcome>" \
  --body "Execute the owning Beads acceptance criteria." --print-plan-id)"
```

Before closure, run the task's focused checks and complete:

```sh
scripts/jig work check --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id"
scripts/jig work evidence --plan-id "$plan_id"
scripts/jig work finish --plan-id "$plan_id" \
  --resolution "<task acceptance complete>" --outcome success
```

#### Dependencies and unblocks

Depends on B02.

Unblocks B04.

### B04 — Ship explicit `rust-cli` init

#### Outcome

`jig init <path> --preset rust-cli --no-input --no-vault` creates the CLI layout
and full Jig harness, and the generated binary runs with a bounded
dependency-free smoke behavior. Initial vault setup remains available through
the existing passphrase environment authority.

#### Scope

- Add the public `RustCli` enum value after `RustLibrary`.
- Connect it to the shared Rust-only renderer's CLI branch.
- Add exact descriptor, validation, answer, report, and summary behavior.
- Generate the explicit binary target and task-local `src/main.rs` contract
  below.
- Add run guidance and hermetic binary execution acceptance.
- Preserve the absence of dev-app authority.

#### Complete task-local contract

The scaffold-owned file set is exactly root `README.md`, root `Cargo.toml`,
`crates/<package>/AGENTS.md`, `crates/<package>/Cargo.toml`, and
`crates/<package>/src/main.rs`. It contains no `lib.rs`, license file,
environment example, migration or SQLx metadata tree, database crate, `apps/`,
OpenAPI tree, JavaScript manifest or lockfile, frontend contract script,
dev-app entry, or release workflow.

The package manifest has `publish = false`, no `license` or `license-file`, and
an explicit `[[bin]]` whose name is the normalized package and whose path is
`src/main.rs`. The binary uses only `std`; with no arguments it exits zero,
writes exactly one newline-terminated UTF-8 stdout line containing
`env!("CARGO_PKG_NAME")` and `env!("CARGO_PKG_VERSION")`, and writes empty
stderr. It defines no argument or option behavior and adds no generated test
that merely reasserts this replaceable smoke output.

Accept existing common init authority: `--repo-name`, `--default-branch`,
`--ci-github-runner`, `--template`, `--template-mode`, `--vcs-ref`, `--force`,
`--defaults`, `--no-input`, and `--no-vault`. Accept explicit Rust check-command
overrides and an effective `rust_crate_roots = ["crates"]`.

Reject `--db`, `--frontend`, `--frontends`, `--frontend-app`, `--go-module`,
`backend_language = "go"`, `sqlx_enabled = true`, nonempty
`rust_migration_dir`, nonempty `rust_sqlx_metadata_dir`,
`schema_dump_enabled = true`, any effective Rust crate root other than
`crates`, and `harness_footprint = "minimal"`. Empty optional migration values
that the existing parser normalizes away are not conflicts. Every rejection
names `rust-cli` and the incompatible input and occurs before vault capture or
destination publication.

#### Required tests

- Clap parse and preset order.
- Strict/no-terminal complete shape.
- Full invalid input matrix.
- Exact CLI files and absence list.
- Binary exits zero, prints package/version, and writes no stderr.
- Cargo fmt, Clippy, test, locked test, and docs.
- Generated contract excludes dev, SQLx, Go, and frontend actions.
- Generated contract contains the neutral `workspace` component and no API or
  backend identity.
- Neutral root `AGENTS.md` guidance contains no backend-only transport rule or
  `scripts/jig dev` recommendation.
- JSON and human summaries.
- Update/recopy ownership boundary.

#### Acceptance criteria

- The explicit command creates a buildable runnable CLI workspace.
- The CLI chooses no third-party parser or logging framework.
- No license grant is implied and accidental publication is disabled.
- The binary is not modeled as a Jig dev service.
- Shared workspace policy does not drift from the library preset.
- Existing and library preset behavior remains compatible.

#### Execution workflow

Before production edits, create and maintain a task-local ExecPlan under
`.agent/` according to `.agent/PLANS.md`. Start structured work and record the
returned ID in both the ExecPlan and a Beads comment:

```sh
plan_id="$(scripts/jig work start --title "<task ID and outcome>" \
  --body "Execute the owning Beads acceptance criteria." --print-plan-id)"
```

Before closure, run the task's focused checks and complete:

```sh
scripts/jig work check --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id"
scripts/jig work evidence --plan-id "$plan_id"
scripts/jig work finish --plan-id "$plan_id" \
  --resolution "<task acceptance complete>" --outcome success
```

#### Dependencies and unblocks

Depends on B03.

Unblocks B05.

### B05 — Integrate guided discovery and strict/default interaction

#### Outcome

Humans and automation can discover and select the complete preset family without
breaking existing default or numeric interaction behavior.

#### Context

Explicit enum values and their finalized descriptors alone are insufficient.
The wizard, `--defaults`, strict errors, package-manager preflight, and long help
currently describe a three-shape world and sometimes equate application presets
with database/frontend requirements.

#### Scope

- Append numeric choices 4 and 5 and exact text aliases.
- Preserve choices 1–3 and default 1.
- Make database/frontend prompts capability-driven.
- Make strict mode accept each Rust-only preset without more shape flags.
- Preserve bare `--defaults` as Rust React/web/no DB.
- Update usage diagnostics and CLI help comprehensively.
- Ensure package-manager availability is not checked for Rust-only presets.
- Add interaction matrix tests.
- Treat B03/B04 descriptor, init-report, and human-summary output as finalized;
  regression-check it but do not redefine it in this task.

#### Required tests

- Every numeric choice and text alias.
- Empty interactive answer still selects Rust React.
- Library/CLI choices cause no DB/frontend prompt.
- Existing Rust React and Go flows still prompt correctly.
- Existing harness-only flow remains exact.
- Strict and implicit non-terminal error/success matrix.
- `--defaults` explicit/implicit matrix.
- Missing package manager does not block Rust-only init.
- B03/B04 preset human/JSON order, descriptors, and init summaries remain exact
  while interaction code changes around them.

#### Acceptance criteria

- New presets are first-class discoverable choices.
- Existing numeric choices and default remain stable.
- No diagnostic falsely says every non-harness preset needs DB/frontends.
- Automation needs only the explicit preset plus normal vault choice.
- JSON stdout remains uncontaminated.

#### Execution workflow

Before production edits, create and maintain a task-local ExecPlan under
`.agent/` according to `.agent/PLANS.md`. Start structured work and record the
returned ID in both the ExecPlan and a Beads comment:

```sh
plan_id="$(scripts/jig work start --title "<task ID and outcome>" \
  --body "Execute the owning Beads acceptance criteria." --print-plan-id)"
```

Before closure, run the task's focused checks and complete:

```sh
scripts/jig work check --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id"
scripts/jig work evidence --plan-id "$plan_id"
scripts/jig work finish --plan-id "$plan_id" \
  --resolution "<task acceptance complete>" --outcome success
```

#### Dependencies and unblocks

Depends on B04. B04 already carries the transitive B03 dependency.

Unblocks B06.

### Feature F3 — Harden and document the complete preset family

#### Outcome

The release has transaction, snapshot, generated-code, compatibility, docs, and
dogfood evidence proportional to init's filesystem and repository-policy risk.

#### Scope

Prove safety and packaged-template behavior after the public UX/report surface
settles, then close documentation and all repository gates over that exact
integrated shape.

#### Acceptance criteria

- Transaction and filesystem invariants hold.
- Packaged builds render every template.
- Generated repositories pass their own checks.
- Documentation has no stale three-preset claims.
- Final diff and Beads graph are consistent.

### B06 — Prove transaction, snapshot, report, and generated-repo quality

#### Outcome

Both Rust-only presets are covered by hermetic generated-repository acceptance,
packaged snapshot parity, exact report oracles, and representative init
transaction failure tests.

#### Scope

- Compose the finalized B03/B04 generated-file, Cargo/Jig, CLI-process, and
  report oracles into one cross-preset acceptance harness; reuse their helpers
  instead of adding competing positive-behavior assertions.
- Add template/scaffold collision and snapshot-only tests.
- Add representative symlink, force, rollback, and budget tests.
- Add before/after compatibility proofs for existing presets.
- Keep expensive tests partitioned consistently with the current dogfood suite.

#### Required tests

For each public preset, initialize a generic temporary repository and assert the
exact scaffold-owned files, expected harness files, deliberate absence list,
parseable Cargo manifests, resolvable workspace member, SQLx-disabled
configuration, neutral authored `workspace` component, Rust action/profile
aliases, conservative CI inputs, no web/database workflow, crate guide in the
agent map, and exact scaffold report classifications.

The deliberate absence oracle covers license files, `.env.example`, migrations,
`.sqlx`, database crates, `apps/`, OpenAPI output, JavaScript manifests and
lockfiles, frontend directories and contract scripts, generated dev-app entries,
and release workflows. The CLI additionally lacks `lib.rs`; the library lacks
`main.rs`. Root guidance uses neutral workspace/crate terminology, omits the
backend-only transport rule, and does not recommend `scripts/jig dev`.

With Cargo forced offline, run setup or generate the lock file and then run
rustfmt, strict all-target Clippy, locked workspace tests, rustdoc with warnings
denied, contract validation, agent-map validation, and agent-guide validation.
Execute the generated CLI and assert zero status, one package/version stdout
line, and empty stderr.

Render both presets from live templates and from the checked-in snapshot-only
source. Assert relative-path and byte parity. Inject at least one rollback after
a Rust-only scaffold file, reject representative symlink/type/case/length
collisions, prove `--force` restores admitted prior bytes and modes, and exercise
the transaction budget near its output limit. Compare representative existing
Rust React, Go React, and harness-only output/report behavior across the
foundation refactor.

Extend CLI JSON acceptance to assert both preset descriptors, exact init report
shape, and one-object usage errors with uncontaminated stdout.

#### Acceptance criteria

- Tests prove user-visible behavior rather than only internal helper calls.
- Every scaffold-owned file appears in output planning and reports.
- Failure leaves no partial new repo and restores admitted existing content.
- Live and packaged template sources are equivalent.
- Generated Cargo/Jig checks pass without registry dependencies.
- Existing preset fixture output does not drift.

#### Execution workflow

Before production edits, create and maintain a task-local ExecPlan under
`.agent/` according to `.agent/PLANS.md`. Start structured work and record the
returned ID in both the ExecPlan and a Beads comment:

```sh
plan_id="$(scripts/jig work start --title "<task ID and outcome>" \
  --body "Execute the owning Beads acceptance criteria." --print-plan-id)"
```

Before closure, run the task's focused checks and complete:

```sh
scripts/jig work check --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id"
scripts/jig work evidence --plan-id "$plan_id"
scripts/jig work finish --plan-id "$plan_id" \
  --resolution "<task acceptance complete>" --outcome success
```

#### Dependencies and unblocks

Depends on B05. B05 already carries the transitive B04 dependency and settles
the interaction/help surface around the finalized B03/B04 descriptors and
reports that these end-to-end assertions consume.

Unblocks B07.

### B07 — Complete docs, dogfood gates, and release acceptance

#### Outcome

User documentation, CLI guidance, generated examples, repository gates, and
Beads state consistently describe and validate the released Rust-only preset
family.

#### Scope

- Update `README.md` quick start and examples; `docs/developer-ux.md` init/adopt
  distinction, guided flow, layouts, and ownership boundary;
  `docs/configuration.md` answers, compatibility, setup, lock policy, and
  snapshot notes; CLI long help in
  `crates/jig/src/bootstrap_parts/part_01.rs`; wizard and strict diagnostics;
  doctor recovery text that enumerates complete shapes; and snapshots/assertions
  for those messages.
- Ensure those surfaces say that `init` is for a new destination, `adopt` is for
  an existing Rust repository, `rust-library` creates a one-library virtual
  workspace, `rust-cli` creates a one-binary virtual workspace, neither adds a
  database or frontend, `rust-workspace` is not public, scaffold source is
  project-owned, and `Cargo.lock` should be committed after setup.
- Verify generated root guidance uses neutral Rust-workspace/crate terminology
  and does not recommend `scripts/jig dev` for either Rust-only preset.
- Use only generic open-source fixture names.
- Build a fresh Jig binary and force the launcher to use it.
- Run focused tests and configured relevant gates.
- Inspect generated receipts, gate state, plan diff, and stale docs.
- Close delivery beads only after acceptance evidence exists.
- Sync Beads JSONL after every final mutation.

#### Required validation

```sh
cargo build -p jig-sh --bin jig
export JIG_DEV_BIN=target/debug/jig
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
scripts/jig check contract
scripts/jig check agent-map
scripts/jig check agent-guides
```

Also initialize one generic library and one generic CLI through the public dev
binary, run their documented setup/check commands, and inspect human and JSON
reports.

#### Acceptance criteria

- README, developer UX, configuration docs, help, and presets output agree.
- Existing init/adopt distinction is explicit.
- No downstream/private identifiers appear in source, tests, docs, plans, Beads,
  or receipts.
- All required relevant gates pass or record truthful not-applicable evidence.
- Final diff contains no stale generated snapshot or dependent test update.
- Beads dependencies and statuses match delivered work.

#### Execution workflow

Before production edits, create and maintain a task-local ExecPlan under
`.agent/` according to `.agent/PLANS.md`. Start structured work and record the
returned ID in both the ExecPlan and a Beads comment:

```sh
plan_id="$(scripts/jig work start --title "<task ID and outcome>" \
  --body "Execute the owning Beads acceptance criteria." --print-plan-id)"
```

Before closure, run the task's focused checks and complete:

```sh
scripts/jig work check --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id"
scripts/jig work evidence --plan-id "$plan_id"
scripts/jig work finish --plan-id "$plan_id" \
  --resolution "<task acceptance complete>" --outcome success
```

#### Dependencies and unblocks

Depends on B06. B06 already carries the transitive B05 dependency.

Closes the epic.

## 26. Implementation sequencing and recovery

### 26.1 Sequence

1. Complete B01 as a behavior-preserving refactor and run existing preset tests.
2. Complete B02 internally and prove live/snapshot parity before public values.
3. Expose and validate `rust-library` through B03.
4. Reuse that path for `rust-cli` through B04.
5. Complete B05 interaction, help, and diagnostic integration without reopening
   B03/B04 descriptor or report ownership.
6. Run B06 hardening against that settled public surface.
7. Finish docs and full dogfood evidence in B07.

### 26.2 Partial implementation recovery

If work stops after B01 or B02, no public preset should be visible. Internal
types/templates may remain behind tests while existing behavior stays green.

If work stops after B03, `rust-library` is a coherent released unit. Do not merge
a `rust-cli` enum value until its renderer, answers, reports, and tests are all
present.

If a template snapshot refresh produces unrelated churn, stop and verify the
live template source root and build environment. Do not hand-edit generated
snapshot manifests to hide drift.

If generated `cargo ... --locked` checks fail because `Cargo.lock` is absent,
run the documented setup/generate-lock step in the fixture. Do not weaken the
locked command or generate a lock during init merely to satisfy a test.

If the existing default output changes during B01, treat it as a regression
unless the plan is explicitly amended with rationale and compatibility review.

### 26.3 Idempotence

Template rendering is deterministic for the same normalized answers and source
revision. Re-rendering into the same staged destination produces unchanged
bytes. Ordinary `jig init` still rejects or requires force for a populated
destination; this plan does not add an idempotent re-init workflow.

Update/recopy idempotence applies only to harness-managed paths. Scaffold paths
remain project-owned.

## 27. Plan review record

This section records planning review rounds. Review activities are not delivery
beads.

### Review provenance and evidence policy

A counted review round records its date, reviewer/model, input baseline or
content digest, review focus, structural-versus-marginal result, and durable
resolution. Model diversity is useful but is not claimed where it did not
occur. Historical authoring passes without retained provenance remain useful
design history but do not count toward the four evidenced strong-model rounds.

Rounds 1–4 below are historical authoring passes from before commit `f282726`.
Their exact reviewer, model, intermediate drafts, and dates were not retained,
so they are explicitly excluded from the evidenced-round count rather than
being retroactively presented as auditable model reviews.

Rounds 5–8 were four topic-specific Codex GPT-5 review passes on 2026-08-29 in
the planning session that produced `f282726`. Their integrated durable evidence
is the complete plan in that commit, the synchronized Beads descriptions, and
epic comment 27, which summarizes the audit remediation. Per-pass intermediate
drafts were not retained, so their resolutions are evidence of the decisions
but not evidence of round-to-round steady state.

Rounds 9 onward use explicit baselines and content digests. Round 9 begins at
commit `f282726`; later rounds record the SHA-256 digest of the reviewed plan so
the exact input can be recovered from this worktree or its resulting commit.

### Round 1 — Product taxonomy and user contract

Review focus:

- whether two or three public presets are justified;
- whether layouts create immediately useful artifacts;
- whether existing defaults remain compatible;
- whether the plan overreaches into databases, frontends, release, or runtime
  schema.

Resolution:

- choose two public artifact presets;
- keep `rust-workspace` internal;
- use a one-member virtual workspace for both;
- reject unsupported shape flags;
- preserve existing numeric/default behavior.

### Round 2 — Architecture and compatibility

Review focus:

- whether current backend-only planning can safely represent Rust-only output;
- whether a generalized capability system would be overengineered;
- whether generated answers require a contract migration;
- whether frontend, dev, and package-manager paths can be excluded structurally.

Resolution:

- add a small exhaustive compile-time capability source;
- add a bootstrap-only Rust artifact variant;
- construct no frontend context for Rust-only plans;
- reuse existing Rust answers and current contract epoch;
- keep preset identity out of persisted runtime authority.

### Round 3 — Safety and validation

Review focus:

- whether file sets and ownership are exact;
- whether Cargo lock/setup behavior is honest;
- whether tests prove generated repositories instead of helper internals;
- whether snapshot-only packaged builds and transaction failures are covered.

Resolution:

- define exact present/absent sets;
- keep init network-free and generate lock during setup;
- require Cargo, contract, agent-map, and CLI process acceptance;
- add representative rollback/symlink/budget tests;
- require live/snapshot parity for both artifacts.

### Round 4 — Delivery graph and steady state

Review focus:

- whether every task is independently verifiable;
- whether dependencies form an acyclic graph;
- whether any bead exists only for planning or review;
- whether parallel tasks would duplicate central enum/dispatcher edits;
- whether the final plan contains unsupported assumptions or private fixtures.

Resolution:

- use seven concrete delivery tasks under three feature groupings;
- sequence CLI after library to reuse central public integration;
- sequence hardening after UX because both touch descriptor/report integration;
- create no meta-planning beads;
- use only generic fixture identities;
- consider the plan steady when final validation finds only wording-level edits.

### Round 5 — Audit remediation and task boundaries

Review focus:

- whether the first task is truly behavior-preserving;
- whether dormant production behavior lands before its first consumer;
- whether rollback boundaries remove complete feature slices.

Resolution:

- keep B01 limited to typed capabilities and plan-shape refactoring;
- move the neutral workspace render hint and projection into B02 beside the
  first internal Rust-only renderer that consumes it;
- require B01 tests to prove existing output equality and B02 tests to prove the
  new neutral projection.

### Round 6 — Product and legal neutrality

Review focus:

- whether a generic preset has authority to grant MIT terms;
- whether publication can happen accidentally with incomplete metadata;
- whether exact layouts and tests agree with Cargo metadata.

Resolution:

- generate no `license` or `license-file` field and no license text;
- set `publish = false` on both seed packages;
- document the deliberate owner action required to choose license and release
  metadata;
- add exact absence and manifest assertions.

### Round 7 — Workflow and graph executability

Review focus:

- whether the graph's claimed concurrency matches actual file ownership;
- whether organizational records can be mistaken for delivery tasks;
- whether every multi-hour task satisfies repository work-receipt policy.

Resolution:

- sequence B06 after B05 and make B07 depend only on B06 transitively;
- put the epic and feature containers in progress while concrete tasks remain
  claimable through `--type task`;
- require a task-local ExecPlan and structured work lifecycle for B01–B07;
- make normal Beads discovery resolve the healthy canonical database.

### Round 8 — Final steady-state audit

Review focus:

- whether the revised graph is acyclic and has exactly one ready task;
- whether plan text, Beads descriptions, dependencies, and statuses agree;
- whether any audit finding remains only acknowledged rather than resolved.

Resolution:

- verify B01 through B07 form one explicit blocking path;
- verify B01 is the only ready concrete task;
- verify every task carries the shared execution contract and that its command
  flags match the checked-in `WorkStartOpts`, check/gates/evidence options, and
  `WorkFinishOpts` contract;
- treat further revisions as marginal unless implementation changes a grounded
  source fact.

### Round 9 — Planning-workflow audit with source grounding

Provenance:

- date: 2026-08-30;
- reviewer/model: Codex, GPT-5;
- input baseline: commit `f282726`;
- evidence: complete-plan read, live Beads graph and descriptions, current
  bootstrap plan/answer code, root `AGENTS.md` template, and Git history.

Review focus:

- whether generated harness guidance is truthful for a library or CLI;
- whether delivery beads can execute without consulting numbered plan sections;
- whether descriptor/report/test ownership is singular;
- whether review history and feature statuses are auditable and honest.

Resolution:

- derive neutral root guidance from the authored `workspace` component so
  initial render, update, and recopy agree without stored preset identity;
- embed complete layouts, input contracts, source behavior, absence oracles,
  and documentation inventory in their owning beads;
- keep descriptors and positive reports in B03/B04, guided interaction in B05,
  and adversarial integrated acceptance in B06;
- add this provenance policy and correct organizational feature statuses to
  open while the epic remains in progress.

Result: structural revision required. The plan was not steady state at this
round and made no contrary claim.

### Round 10 — Standalone-task and ownership validation

Provenance:

- date: 2026-08-30;
- reviewer/model: Codex, GPT-5;
- input plan SHA-256:
  `45ab2ff45f47b54bd5d5ce21bbbfd7fbd602096d22c708430fc222878a4afd21`;
- evidence: standalone scan of every B01–B07 specification for numbered-section
  dependencies, exact contract content, ownership terms, and stale markers.

Review focus:

- whether an agent receiving only one task description has its normative inputs
  and observable acceptance oracles;
- whether B03–B06 still assign the same report or test artifact to multiple
  owners.

Resolution:

- no delivery task retains a numbered-section dependency;
- the scan found one stale sequence sentence assigning descriptor/report
  ownership to B05; it was corrected to interaction/help/diagnostics only;
- no task split, product behavior, or dependency edge changed.

Result: marginal wording correction only, providing the first explicit
post-remediation steady-state signal.

### Round 11 — Dependency and justification validation

Provenance:

- date: 2026-08-30;
- reviewer/model: Codex, GPT-5;
- input plan SHA-256:
  `1bb919efad447a840beb435d7ab3175e052377e7f44f9dee1ac005b301517419`;
- evidence: `br ready --epic jig-sh-rust-only-init-presets-zc7 --type task
  --json`, `bv --robot-insights --format json`, live feature rollups, and a
  source read of five load-bearing decision rationales.

Review focus:

- cycles, orphans, readiness, and the complete blocking path;
- truthful epic/feature/task statuses;
- rationale for public names, virtual workspace layout, shared CLI layout,
  license neutrality, and authored-model-derived root guidance.

Resolution:

- the graph has zero cycles, the seven-task path is intact, and B01 is the only
  ready concrete task;
- the epic is in progress while all three unclaimed feature containers are open;
- all five sampled decisions contain explicit tradeoff rationale grounded in
  current Cargo/Jig behavior;
- no plan or graph revision was required.

Result: no change, confirming steady state after round 10.

### Round 12 — Final plan/Beads steady-state audit

Provenance:

- date: 2026-08-30;
- reviewer/model: Codex, GPT-5;
- input plan SHA-256:
  `1f6cde68f570034db5d37f17018b68b89098ccf82d31633d88d486d439155a9a`;
- evidence: byte comparison of all 11 plan-node descriptions with live Beads,
  canonical-versus-branch epic-record digests, JSONL parsing, `git diff
  --check`, private-fixture scans, task-scoped readiness, graph insights, and
  canonical Beads sync status.

Review focus:

- exact plan/Beads equivalence after status and description mutations;
- residual external section references or stale ownership language;
- graph health, ready-task cardinality, open-source fixture hygiene, and branch
  baseline compatibility.

Resolution:

- all 11 descriptions match byte-for-byte after newline normalization;
- branch and canonical epic records have the same digest, JSONL is valid, and
  canonical Beads reports no dirty issue or sync drift;
- the graph has zero cycles and B01 remains the only ready concrete task;
- no private fixture name, stale B05 ownership statement, numbered-section task
  dependency, or formatting defect remains;
- no plan, product, task, or graph revision was required.

Result: no change for a second consecutive round. The plan is at evidenced
steady state and further revision is deferred until implementation discovers a
new grounded fact.

## 28. Final acceptance checklist

Product:

- [ ] `rust-library` is public, discoverable, complete, and buildable.
- [ ] `rust-cli` is public, discoverable, complete, buildable, and runnable.
- [ ] no public `rust-workspace` alias exists.
- [ ] existing defaults and numeric choices remain stable.
- [ ] existing repositories are still directed to `jig adopt`.

Generated source:

- [ ] both use one-member virtual workspaces under `crates/`.
- [ ] manifests share current Rust edition/floor policy, omit license claims,
      and set seed packages `publish = false`.
- [ ] library source has docs and no fake API.
- [ ] CLI source has bounded dependency-free smoke behavior.
- [ ] root README and crate guide are artifact-appropriate.
- [ ] Cargo lock policy is documented and locked checks pass after setup.

Jig answers and contract:

- [ ] Rust backend compatibility is selected.
- [ ] SQLx and schema dump are disabled.
- [ ] crate roots equal `crates`.
- [ ] frontends and application contracts are absent.
- [ ] no dev app is generated.
- [ ] no new persistent contract field or epoch exists.

Safety:

- [ ] output paths and rendered files are equal.
- [ ] scaffold/template ownership sets are disjoint.
- [ ] symlink/type/path/case/length checks fail closed.
- [ ] force and rollback restore prior content.
- [ ] init remains network-free and transactional.
- [ ] snapshot-only packaged rendering works.

Compatibility:

- [ ] Rust React output remains compatible.
- [ ] Go React output remains compatible.
- [ ] harness-only remains compatible.
- [ ] bare and `--defaults` init remain Rust React/web/no DB.
- [ ] update/recopy never owns scaffold files.
- [ ] adoption behavior does not change.

Validation:

- [ ] focused unit and generation tests pass.
- [ ] generated Cargo fmt/Clippy/test/test-locked/doc pass.
- [ ] generated Jig contract/agent-map/agent-guides pass.
- [ ] CLI process oracle passes.
- [ ] human and JSON reports are exact.
- [ ] full relevant repository gates pass.
- [ ] docs and help have no stale preset list.
- [ ] fixtures and receipts satisfy open-source hygiene.
- [ ] Beads graph is acyclic and dependency-complete.
- [ ] every task has a completed task-local ExecPlan and structured work
      receipts connected to its Beads record.

## 29. Grounded decisions summary

The plan relies on these verified current-code facts:

- only `RustReact`, `GoReact`, and `HarnessOnly` exist in `ScaffoldPreset`;
- preset descriptors already provide human and JSON discovery metadata;
- strict init currently requires DB/frontends only for the two React presets;
- package-manager preflight is already limited to React presets;
- `InitScaffoldPlan` is currently backend-centric and needs a bootstrap-only
  generalization;
- scaffold output already flows through reserved-path preflight, guarded atomic
  writes, init transaction rollback, and JSON file classification;
- existing Rust answers already express crate roots, SQLx disabled, Rust checks,
  frontend absence, and dev absence;
- the current generated repository model otherwise labels every Rust shape as
  an API/backend, so Rust-only initial rendering needs a neutral projection
  through the existing component/action schema;
- live scaffold sources and embedded package snapshots have an established
  refresh command;
- generated application source is already documented as project-owned;
- repository policy requires generic open-source fixtures and freshly built dev
  binaries for runtime changes.

If any of those facts changes before implementation begins, the owning bead must
update its task-local ExecPlan and this product plan where the change affects
public behavior or dependency structure.

## 30. Delivery Beads created from this plan

The planning pass created the following delivery graph. The epic is in progress
because its product outcome is active. Feature containers remain open until
their concrete work is actually active; status is not used as a queue-filtering
hack. Concrete tasks remain open until claimed. Agents use `--type task` for
implementation readiness. These records are implementation work, not planning
or review ceremony.

| Plan node | Beads ID | Status | Title |
| --- | --- | --- | --- |
| Epic | `jig-sh-rust-only-init-presets-zc7` | in progress | First-class Rust-only jig init presets |
| F1 | `jig-sh-rust-only-init-presets-zc7.1` | open/organizational | Generalize scaffold planning for Rust-only artifacts |
| B01 | `jig-sh-rust-only-init-presets-zc7.1.1` | open/ready | Refactor preset capabilities and backend-only scaffold planning |
| B02 | `jig-sh-rust-only-init-presets-zc7.1.2` | open/blocked | Add the shared Rust-only workspace renderer and templates |
| F2 | `jig-sh-rust-only-init-presets-zc7.2` | open/organizational | Deliver public Rust library and CLI init workflows |
| B03 | `jig-sh-rust-only-init-presets-zc7.2.1` | open/blocked | Ship explicit rust-library init |
| B04 | `jig-sh-rust-only-init-presets-zc7.2.2` | open/blocked | Ship explicit rust-cli init |
| B05 | `jig-sh-rust-only-init-presets-zc7.2.3` | open/blocked | Integrate guided discovery and strict/default preset interaction |
| F3 | `jig-sh-rust-only-init-presets-zc7.3` | open/organizational | Harden and document the Rust-only preset family |
| B06 | `jig-sh-rust-only-init-presets-zc7.3.1` | open/blocked | Prove Rust-only preset transaction, snapshot, report, and generated-repo quality |
| B07 | `jig-sh-rust-only-init-presets-zc7.3.2` | open/blocked | Complete Rust-only preset docs, dogfood gates, and release acceptance |

The blocking path is:

```text
B01 -> B02 -> B03 -> B04 -> B05 -> B06 -> B07
```

The graph-aware validation reported 11 scoped open records, seven delivery
tasks, zero dependency cycles, and B01 as the only ready delivery task. The epic
and feature records organize work; they are not substitutes for claiming and
completing the task records. Agents scope readiness with `--type task`.
