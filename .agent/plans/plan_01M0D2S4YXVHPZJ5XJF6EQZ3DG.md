# Add a production-oriented Go/React scaffold to `jig init`

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current as implementation proceeds. Maintain this document in accordance with `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

After this change, a user can run `jig init ./my-app --preset go-react --go-module github.com/acme/my-app --db postgres --frontends web` and receive a working Go 1.26 and React repository. The generated API uses chi for routing, Huma for typed operations and OpenAPI 3.1, pgxpool for long-lived PostgreSQL access, Goose for migrations, and sqlc for checked-in query code. The same in-memory Huma API constructor is used by the server and by an offline OpenAPI exporter, so TypeScript clients can be regenerated with Hey API without starting a database or HTTP listener.

Existing `rust-react` and `harness-only` initialization must retain their current defaults, output, and validation semantics unless a Go-specific branch is explicitly selected. The first Go increment supports no database or PostgreSQL, plus the existing public `web` application and optional `landing` site. It rejects SQLite and the privileged `admin` frontend with actionable errors rather than silently generating an incomplete security boundary.

The result is observable by rendering both no-database and PostgreSQL examples, bootstrapping dependencies, exporting OpenAPI, verifying the generated TypeScript client is clean, and running Go formatting, vet, tests, module verification, sqlc drift checks, and the repository's Rust gates.

## Progress

- [x] (2026-08-19 13:18Z) Fast-forwarded `feat/go-backend-support` from `bf701fe` to latest `origin/master` at `307c43a`; the branch had no unique commits and the worktree was clean.
- [x] (2026-08-19 13:18Z) Read the root and crate guidance, `.agent/PLANS.md`, and the `jig-exec-plans:write-exec-plan` skill; opened structured work plan `plan_01M0D2S4YXVHPZJ5XJF6EQZ3DG`.
- [x] (2026-08-19 14:05Z) Added a persisted backend-language model, the `go-react` preset, `--go-module`, interactive/default resolution, and invalid-combination tests while preserving Rust defaults.
- [x] (2026-08-19 14:52Z) Added backend-aware project templates, contract tools, generic `lint` and `sqlc` checks, a small `jig-go` feature registry crate, and Go-aware doctor/policy/workflow output.
- [x] (2026-08-19 15:28Z) Added the Go workspace scaffold, including shared API construction, Huma export, optional pgxpool, embedded Goose migrations through a short-lived adapter, sqlc configuration, checked-in generated query code, and disposable PostgreSQL integration testing.
- [x] (2026-08-19 15:47Z) Parameterized the existing frontend workspace so OpenAPI export, Playwright startup, E2E CI, public-boundary checks, error normalization, and request-ID expectations work with either Cargo or Go without duplicating the React architecture.
- [x] (2026-08-19 16:11Z) Refreshed embedded template snapshots and added focused Rust regression tests covering CLI parsing, wizard defaults, rendering, command registries, Go doctor behavior, and unsupported Go combinations.
- [x] (2026-08-19 16:12Z) Rendered and bootstrapped representative `none+web` and `postgres+web` Go repositories; validated Go, sqlc, exact OpenAPI/client drift, browser E2E, and a live Docker-backed PostgreSQL migration/query path.
- [x] (2026-08-19 16:58Z) Built the development `jig` binary, ran structured work checks/gates/evidence/receipts, completed contract/format/clippy/agent-guide gates and the full split nextest suite, reviewed the final diff, and prepared the work session for successful finish.

## Surprises & Discoveries

- Observation: The latest `origin/master` introduced 70 commits of contract-v4 runtime and launcher hardening after this branch was created.
  Evidence: `git fetch origin` reported `bf701fe..307c43a`, and `git merge --ff-only origin/master` advanced the branch without conflicts.

- Observation: The current scaffold's frontend assets are physically stored below `templates/scaffolds/rust-react/frontend`, but most are conceptually backend-neutral; only contract export, Playwright startup, and E2E workflow setup encode Cargo/SQLx assumptions.
  Evidence: the template inventory contains 122 frontend files and 38 Rust workspace files, while searches for Cargo/SQLx in frontend templates concentrate in `frontend/workspace/contracts.mjs.jinja`, `frontend/vite-react/playwright.config.ts.jinja`, and `frontend/workspace/e2e.yml.jinja`.

- Observation: Jig's `[commands]` parser and runtime already accept arbitrary command-backed tool keys, but public check subcommands and feature registries name the supported surface explicitly.
  Evidence: `RepoContext::command_for_key` resolves dynamic configured commands, while `crates/jig/src/cli/check.rs`, `crates/jig-contract`, and `crates/jig-features` contain fixed tool/feature names.

- Observation: Huma's chi integration uses the root `huma/v2` package for API types and the separate `huma/v2/adapters/humachi` package for the router adapter; Huma also includes its schema marker in exported OpenAPI.
  Evidence: the rendered project compiles against Huma v2.39.1, and the exporter-generated document exactly matches committed `openapi/public.json` including the Huma error models and `$schema` fields.

- Observation: Go's toolchain selection downloaded and ran Go 1.26.0 from a host whose default `go version` was 1.22.2.
  Evidence: generated `scripts/jig doctor --json` reported required and actual versions as `1.26.0`, and all generated Go commands compiled without lowering the requested language floor.

- Observation: The shared React error test encoded the Rust error response and the browser test required `X-Request-ID`; both are observable frontend contracts rather than cosmetic test details.
  Evidence: initial generated coverage rejected a Huma error body until Go-specific `detail`/`title` normalization was added, and the final Playwright run passed only after the Go router propagated chi's request ID to the response header.

- Observation: The existing shared workspace exposed `test:postgres` for every PostgreSQL scaffold, so the initial Go render needed a matching script and integration test rather than merely omitting the broken command.
  Evidence: the final Docker-backed run applied `00001_app_metadata.sql` with Goose and exercised `dbsqlc.GetMetadata` through pgxpool successfully.

## Decision Log

- Decision: Keep `rust-react` as the default preset and treat absent backend identity in old `.jig.toml` files as Rust.
  Rationale: Existing generated repositories are durable inputs to `jig update`; an additive Go discriminator must not reinterpret or rewrite them.
  Date/Author: 2026-08-19 / Codex

- Decision: Persist `backend_language = "go"` only for Go repositories and omit it from newly rendered Rust repositories.
  Rationale: Go update/doctor/template behavior needs a durable identity, while conditional emission preserves current Rust scaffold bytes and snapshots.
  Date/Author: 2026-08-19 / Codex

- Decision: Require `--go-module` in strict/noninteractive Go initialization, prompt for it interactively, and derive `example.com/<normalized-repo-name>` only under `--defaults`.
  Rationale: Import paths are source-level API and should normally be intentional, while the defaults mode explicitly opts into deterministic placeholder choices.
  Date/Author: 2026-08-19 / Codex

- Decision: Reject `go-react` with SQLite or `admin` during option validation.
  Rationale: sqlc/pgxpool are PostgreSQL-specific in this increment, and the existing admin scaffold represents a second privileged HTTP/OpenAPI/client boundary that should not be faked as public API reuse.
  Date/Author: 2026-08-19 / Codex

- Decision: Pin Go tools with `tool` directives in `go.mod` and invoke them as `go tool sqlc` and `go tool goose`.
  Rationale: Go 1.26 supports tool dependencies natively, keeping sqlc and Goose versions reproducible without globally installed binaries.
  Date/Author: 2026-08-19 / Codex

- Decision: Use pgxpool for application traffic and open a separate short-lived `database/sql` connection through pgx/v5's stdlib adapter only while applying Goose migrations.
  Rationale: This follows each library's natural interface and prevents the migration adapter from becoming the long-lived application pool.
  Date/Author: 2026-08-19 / Codex

- Decision: Embed Goose migrations from `internal/database/migrations` into the database package.
  Rationale: Startup and package integration tests must not depend on the caller's current working directory; the same directory remains sqlc's schema authority and a single source of truth.
  Date/Author: 2026-08-19 / Codex

- Decision: Extend existing frontend rendering with a small backend context instead of copying the React template tree under `go-react`.
  Rationale: The applications, generated client format, and package-manager policy are shared; only backend commands, paths, and toolchain setup differ.
  Date/Author: 2026-08-19 / Codex

## Outcomes & Retrospective

Implementation is complete. Rendered no-database and PostgreSQL repositories passed their backend, contract, TypeScript, browser, and live database checks. The initial full Jig suite reported 2,122 passes and three expectation/registry failures caused by the intentionally expanded CLI command surface; all three were corrected, passed individually, and the exact final tree passed the full split nextest gate. Structured `work check` batch `receipt_01M0D8WZTH7AE7QE5Y3K1HM9FS` records fresh passing contract and test evidence, while clippy, format, agent-map, agent-guide, and diff checks also passed. The intentionally unsupported surface remains SQLite and the privileged Go admin API/client boundary, both rejected before publication with actionable messages.

The delivered `go-react` preset persists its backend identity for future Jig updates, generates Go-specific contract/check/workflow/doctor policy, and keeps old configurations Rust-by-default. Its PostgreSQL startup applies embedded Goose migrations through a short-lived `database/sql` adapter before opening pgxpool; its checked-in sqlc and Hey API output is reproducible from pinned Go tool dependencies and the shared offline Huma schema constructor. No validation was deferred: Docker and Playwright were available, so the live database and browser paths both ran successfully.

## Context and Orientation

`jig init` is defined in the `jig-sh` crate under `crates/jig`. Clap-facing scaffold options and enums live in `crates/jig/src/bootstrap_parts/part_01.rs`; interactive and defaults resolution lives in `crates/jig/src/cli/init_wizard.rs`; initialization orchestration lives in `crates/jig/src/bootstrap/init.rs`. `crates/jig/src/bootstrap/scaffold.rs` converts resolved options into `InitScaffoldPlan`, applies command/config defaults, renders backend and frontend files, and performs collision preflight. Rust backend rendering is isolated in `crates/jig/src/bootstrap/scaffold/rust_workspace.rs`. Frontend rendering is split across `crates/jig/src/bootstrap/scaffold/frontend.rs` and `frontend_parts/`.

Project-level harness templates live under `templates/project`. These render `.jig.toml`, `.agent/jig-contract.json`, workflows, policy files, and agent guides. Scaffold source templates live below `templates/scaffolds/rust-react`. The Rust workspace subtree is backend-specific; the frontend subtree is shared source material whose context currently assumes Cargo. The build embeds source templates in snapshots under `crates/jig/src/bootstrap/embedded_template_snapshots`, so edits to project templates require refreshing those snapshots with the repository-supported environment switch.

Jig calls a configured command through a contract tool. A contract tool is a stable name such as `jig.test`; `.agent/jig-contract.json` maps it to a key such as `go_test_command`, and `.jig.toml` maps that key to a shell command such as `go test ./...`. `crates/jig-contract` owns shared tool-name constants, `crates/jig-features` validates registered feature bundles, and small crates such as `crates/jig-rust` and `crates/jig-sqlx` register related tools. The Go implementation adds `jig.lint` and `jig.sqlc_check` without changing existing Rust tool names.

The generated Go server must build its Huma API in a side-effect-free constructor. Here, side-effect-free means constructing routes and OpenAPI metadata without reading configuration, opening a database, binding a socket, or spawning work. `cmd/api/main.go` owns process lifecycle and supplies optional application dependencies. `cmd/openapi/main.go` calls the same constructor and serializes Huma's in-memory OpenAPI document to `openapi/public.json`. PostgreSQL repositories create a pgxpool once for request-time use; before serving, they briefly open and close a `database/sql` handle for Goose migrations.

sqlc reads SQL query files and database migrations, type-checks queries, and writes Go source into `internal/database/sqlc`. Goose migration files contain both `-- +goose Up` and `-- +goose Down` sections in one timestamped file. The generated repository commits sqlc output and OpenAPI/client output, so drift checks regenerate or diff those boundaries and fail if source inputs changed without refreshed artifacts.

## Plan of Work

First, add a `BackendLanguage` value with Rust as the deserialization default. Thread it through resolved render answers and repository config. Add `GoReact` to `ScaffoldPreset`, `go_module: Option<String>` to scaffold options, parser help, wizard prompts/defaults, and validation. `InitScaffoldPlan::from_options` will branch on the preset, enforce Go's database/frontend limits, validate a conventional Go module path, and retain the current Rust fields and behavior. Tests must prove old answer files and Rust CLI invocations behave unchanged, while strict Go mode reports the missing module and unsupported SQLite/admin combinations before destination mutation.

Second, make project harness rendering backend-aware. For Go answers, `.jig.toml` will emit `backend_language = "go"`, Go command keys, Go source roots, and no Rust/SQLx authorities. `.agent/jig-contract.json` will require the Go commands and expose `jig.fmt_check`, `jig.lint`, `jig.test`, `jig.test_locked`, and, for PostgreSQL, `jig.sqlc_check`. Add `jig check lint` and `jig check sqlc`, shared contract constants, and `crates/jig-go` registration following the structure of the existing feature crates. Add a Go test workflow and conditionally omit the Rust workflow; branch repository policy and agent guidance so generated Go repos do not claim Cargo/SQLx rules. Doctor should interpret the persisted backend and check the Go 1.26 floor plus configured Go tooling instead of Rust/SQLx.

Third, create `crates/jig/src/bootstrap/scaffold/go_workspace.rs` and templates under `templates/scaffolds/go-react/workspace`. Render `.go-version`, `go.mod`, `.env.example`, API and OpenAPI commands, internal configuration/app/HTTP packages, and database files only for PostgreSQL. The `go.mod` will declare `go 1.26.0`, pin chi v5, Huma v2, and conditionally pgx v5, plus sqlc and Goose tool directives. Huma must disable runtime schema/docs routes while retaining the in-memory schema used by the exporter. The PostgreSQL branch will include `sqlc.yaml`, a query, a Goose migration, and sqlc-generated code checked into `internal/database/sqlc`. Unit tests will instantiate and call the router without a database, while database integration tests will be opt-in through `DATABASE_URL`.

Fourth, pass backend language, backend development command, exporter command, artifact boundary paths, migration directory, and generated-code directory into the existing frontend renderer. Update the shared contracts script to run either the Rust exporter or `go run ./cmd/openapi`, update Playwright to start the Go API where selected, and update E2E CI to install Go and run Goose/sqlc commands for PostgreSQL Go projects. Keep Hey API at the existing repository-supported version and retain public client output paths. Landing-only Go repositories should not gain unnecessary API-client applications, and Go admin requests remain rejected.

Fifth, extend initialization bootstrap so Go repositories run `go mod tidy` before locked checks and then use the existing package-manager bootstrap for frontend dependencies. Add rendered-output tests for no-database and PostgreSQL Go variants, CLI/wizard tests, collision/update tests, and checks that Rust output remains stable. Refresh embedded project-template snapshots only after source templates and tests agree.

Finally, build `target/debug/jig`, export `JIG_DEV_BIN=target/debug/jig`, and dogfood the generated command surface. Render temporary example repositories outside the source tree, bootstrap them, and run `gofmt` cleanliness, `go vet ./...`, `go test ./...`, `go mod verify`, read-only tests, sqlc vet/diff, OpenAPI export, and Hey API drift checks. Use a disposable PostgreSQL service if available for Goose and integration tests. Run the source repository's focused test suite, contract/format/clippy gates, and `scripts/jig check test`; record evidence and finish structured work.

## Concrete Steps

All commands run from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume` unless a rendered repository is explicitly named.

Inspect and edit incrementally:

    rg -n "ScaffoldPreset|ScaffoldOpts|InitScaffoldPlan|RenderAnswers|RepoConfig" crates/jig/src
    cargo fmt --all -- --check
    cargo test -p jig-sh bootstrap:: cli::

Refresh project-template snapshots after intentional template edits:

    JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
    git diff --check

Build the changed runtime and force dogfooding through it:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig work check --plan-id plan_01M0D2S4YXVHPZJ5XJF6EQZ3DG

Render representative repositories into disposable directories created with `mktemp -d`; never remove a broad or unresolved path. Example invocations are:

    target/debug/jig init "$tmp_dir/no-db" --preset go-react --go-module example.com/no-db --db none --frontends web --defaults
    target/debug/jig init "$tmp_dir/postgres" --preset go-react --go-module example.com/postgres --db postgres --frontends web --defaults

Inside each rendered repository, bootstrap and validate with the generated launcher. Go automatically selects the `go.mod` toolchain when the host command supports toolchain downloads:

    scripts/jig bootstrap
    scripts/jig check fmt
    scripts/jig check lint
    scripts/jig check test
    scripts/jig check test-locked
    scripts/jig check sqlc          # PostgreSQL only
    bun run contract:check          # when the frontend workspace is present

Finish source-repository validation and structured evidence:

    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
    JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
    JIG_DEV_BIN=target/debug/jig scripts/jig check test
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0D2S4YXVHPZJ5XJF6EQZ3DG
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0D2S4YXVHPZJ5XJF6EQZ3DG
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M0D2S4YXVHPZJ5XJF6EQZ3DG

## Validation and Acceptance

CLI acceptance requires `jig init --help` to list `go-react` and `--go-module`; a strict Go invocation without a module must fail with an error naming `--go-module`; `--defaults` must render a deterministic `example.com/<repo>` module; SQLite and `admin` must fail before any destination files are published. Existing Rust init tests and byte assertions must continue passing.

No-database scaffold acceptance requires `go test ./...`, `go vet ./...`, and `go test -mod=readonly ./...` to succeed under Go 1.26. Running `go run ./cmd/openapi` must update `openapi/public.json` without a database. Running the generated contracts check must leave both OpenAPI and `packages/public-api-client` clean. Starting `go run ./cmd/api` and requesting the generated health/status route must return HTTP 200 and the documented JSON body.

PostgreSQL scaffold acceptance additionally requires `go tool sqlc vet` and `go tool sqlc diff` to succeed. With `DATABASE_URL` pointing at an empty disposable PostgreSQL database, starting the API must apply the embedded Goose migration through a short-lived `database/sql` handle, close that handle, create the long-lived pgxpool, pass readiness, and serve the same public API. A second start must be idempotent. The disposable `test:postgres` path must exercise a generated sqlc query. If Docker or PostgreSQL is unavailable, unit/render/drift checks remain mandatory and the missing live integration proof must be recorded explicitly rather than reported as passed.

Source acceptance requires the focused `jig-sh` tests, generated-template snapshot checks, `git diff --check`, and configured Jig contract/format/clippy/test gates to pass. `git status` may contain only intentional source, template, plan, and append-only structured-work changes.

## Idempotence and Recovery

The branch update used `git merge --ff-only`, which is repeatable and cannot create a merge commit. Template refresh is deterministic and can be rerun after editing source templates. Rendered validation repositories live in exact `mktemp -d` paths and can be abandoned safely on failure; do not delete a path unless its exact value has been printed and checked.

Initialization preserves its existing staged publication and collision preflight. New Go validation must occur before destination mutation, so a missing module or unsupported combination leaves no partial project. Dependency bootstrap commands are repeatable: `go mod tidy`, `go tool sqlc generate`, and Hey API generation converge to checked-in artifacts. Goose itself records applied migration versions and safely skips them on a second start.

If a generated dependency version or API differs from the templates, first reproduce it in a disposable rendered repository, then adjust the source template and rerun generation; never patch only an embedded snapshot. If the Go toolchain cannot download Go 1.26, retain compile-independent Rust render tests and record the environmental limitation in this plan. If frontend bootstrap cannot reach registries, preserve generated source validation and report the skipped network-dependent proof.

## Artifacts and Notes

Baseline evidence:

    $ git fetch origin
    From github.com:bpcakes/jig-sh
       bf701fe..307c43a  master -> origin/master

    $ git merge --ff-only origin/master
    Updating bf701fe..307c43a
    Fast-forward

    $ git status --short --branch
    ## feat/go-backend-support...origin/master

The host initially reports Go 1.22.2, Node 22.23.2, and Bun 1.3.14. Generated Go validation therefore uses Go's `GOTOOLCHAIN` download mechanism to run the pinned 1.26 line rather than weakening the generated version floor.

## Interfaces and Dependencies

`ScaffoldPreset` gains `GoReact`. The flattened answer options carry `go_module: Option<String>` exposed as `--go-module`, allowing both CLI and answer-file resolution before scaffold planning. A strongly typed backend-language value serializes as `rust` and `go`, with Rust used when old config omits the field. `InitScaffoldPlan` exposes enough backend context to render Rust or Go files without making common frontend code infer behavior from filesystem paths.

The Go module pins Go 1.26 and these dependency lines unless validation discovers an incompatibility that is recorded in the Decision Log: `github.com/go-chi/chi/v5` v5.3.0, `github.com/danielgtaylor/huma/v2` v2.39.1, `github.com/jackc/pgx/v5` v5.10.0, `github.com/sqlc-dev/sqlc` v1.31.1, and `github.com/pressly/goose/v3` v3.27.3. Huma uses its chi adapter package. pgxpool is the request-time pool; pgx/v5/stdlib supplies Goose's `*sql.DB` only during migration.

The generated Go packages must have a side-effect-free HTTP constructor callable by both commands. Conceptually the interface is:

    func New(deps Dependencies) (http.Handler, huma.API)

The exact return wrapper may differ if Huma's API makes a small struct clearer, but the exporter must obtain `huma.API.OpenAPI()` without configuration, network, or database access. `cmd/api` owns signal handling, server shutdown, migration application, and pool closure. `cmd/openapi` owns deterministic JSON serialization and replacement of `openapi/public.json`.

`sqlc.yaml` uses engine `postgresql`, reads embedded migrations from `internal/database/migrations`, reads `internal/database/queries`, emits package `dbsqlc` into `internal/database/sqlc`, and targets pgx/v5. The checked-in initial query and generated files are exactly what the pinned sqlc version produces. Goose files contain `-- +goose Up` and `-- +goose Down` annotations in one file.

The Go contract maps `jig.fmt_check` to `go_fmt_check_command`, `jig.lint` to `go_lint_command`, `jig.test` to `go_test_command`, `jig.test_locked` to `go_test_locked_command`, and PostgreSQL-only `jig.sqlc_check` to `sqlc_check_command`. The default command bodies are respectively a failing gofmt cleanliness check, `go vet ./...`, `go test ./...`, `go mod verify && go test -mod=readonly ./...`, and `go tool sqlc vet && go tool sqlc diff`. Existing Rust mappings remain unchanged.

Revision note (2026-08-19, Codex): Replaced the short work-start body with a self-contained implementation and validation plan after fast-forwarding to the contract-v4 master baseline. This records the compatibility strategy, supported Go matrix, dependency ownership, and observable acceptance criteria needed to resume from this file alone.
