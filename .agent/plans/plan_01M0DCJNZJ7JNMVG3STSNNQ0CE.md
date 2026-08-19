# Harden the generated Go backend lifecycle

This ExecPlan is a living document. Maintain it according to `.agent/PLANS.md`, especially the `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` sections.

## Purpose / Big Picture

The first `go-react` scaffold renders and compiles, but several generated-repository behaviors still rely on assumptions inherited from the Rust scaffold. After this work, a generated Go repository will reject stale Huma OpenAPI and Hey API output, follow its documented `.env` and database bootstrap path, run on CI runners with explicitly installed prerequisites, protect Goose migrations from modification, and reject invalid module paths before publishing files. A user can observe the result by generating both database-free and PostgreSQL projects, running their Jig checks, and running PostgreSQL browser and database tests without manual database creation.

The root cause is structural. Backend identity is currently carried through `backend_language`, `go_database`, `sqlx_enabled`, and frontend conditionals, but lifecycle capabilities are not named or tested as one contract. Template snapshots prove bytes and paths but cannot prove that source changes trigger the correct workflow, that documented setup loads configuration, or that the database named by CI exists. The repair therefore aligns each lifecycle boundary and adds scenario tests, rather than only changing the failing strings.

## Progress

- [x] (2026-08-19T16:10:11Z) Reviewed the merged Claude/Codex findings and grouped them by lifecycle boundary.
- [x] (2026-08-19T16:10:11Z) Started structured work `plan_01M0DCJNZJ7JNMVG3STSNNQ0CE` and wrote this ExecPlan.
- [x] (2026-08-19T16:16:14Z) Commit slice 1: enforce Huma OpenAPI and Hey API client drift checks from Go-owned source changes.
- [x] (2026-08-19T16:25:02Z) Commit slice 2: make optional `.env` loading, database creation, Goose migration, local setup, integration tests, and browser E2E share one Go bootstrap lifecycle.
- [x] (2026-08-19T16:27:11Z) Commit slice 3: install the Jig runtime prerequisite explicitly in Go CI, use a cache key available immediately after init, and trigger sqlc for query changes.
- [x] (2026-08-19T16:31:44Z) Commit slice 4: make migration immutability consume a backend-neutral configured migration directory and enable it for PostgreSQL Go repositories.
- [ ] Commit slice 5: reject invalid Go module segments, preserve wizard numeric compatibility, make format failure propagation reliable, and keep reserved output paths backend-aware.
- [ ] Run the complete repository test suite through the development Jig binary, record evidence, close structured work, and commit the final plan/state update.

## Surprises & Discoveries

- Observation: `scripts/jig check contract` checks `.agent/jig-contract.json`; application OpenAPI drift is currently checked indirectly by the frontend `contract:check` script.
  Evidence: `templates/scaffolds/rust-react/frontend/vite-react/README.md.jinja` incorrectly attributes Go OpenAPI checking to the Jig contract command, while `templates/project/.github/workflows/webapp-checks.yml.jinja` does not trigger on Go source.
- Observation: the shared PostgreSQL E2E template creates only the `postgres` database but gives the Go API a distinct `jig_e2e_*` database URL.
  Evidence: `templates/scaffolds/rust-react/frontend/workspace/e2e.yml.jinja` sets `POSTGRES_DB: postgres`; the Go Playwright command directly starts `cmd/api`, whose Goose adapter can migrate only after connecting to the named database.
- Observation: generated `scripts/jig` installs its Rust implementation with `cargo install` when no compatible cache exists.
  Evidence: `templates/project/scripts/install-jig.sh.jinja` invokes `cargo install`, while the new Go workflows install only Go.
- Observation: a foreground `scripts/jig work check` continued its full test gate after the shell wrapper returned at 30 seconds, and correctly rejected its receipt because template refresh changed the worktree during the check.
  Evidence: receipt `receipt_01M0DCVEWRB132A9SZ1V641A0K` records exit status 100 and the before/after worktree fingerprints. The final work check must run only after all edits settle.
- Observation: the first OpenAPI regression test used one too many parent components when locating the generated repository root.
  Evidence: generated `go test ./...` failed while looking above the temporary app; changing the path from three parents to two made the generated test pass.
- Observation: placing a PostgreSQL-only command parser behind an inline template conditional made the database-free and PostgreSQL variants disagree about required trailing whitespace.
  Evidence: `gofmt -l` rejected one variant for an absent final newline and the other for a missing separator. Moving the parser to a PostgreSQL-only Go file made both variants format without post-generation rewriting.

## Decision Log

- Decision: Treat API schema drift, database readiness, CI prerequisites, and migration immutability as lifecycle capabilities, not language-specific incidental steps.
  Rationale: These invariants cross source templates, generated commands, workflows, and documentation. Fixing only one representation leaves another path able to regress.
  Date/Author: 2026-08-19 / Codex
- Decision: Keep commits aligned to independently verifiable lifecycle slices.
  Rationale: The user requested separate commits, and each boundary has a focused rollback and test story.
  Date/Author: 2026-08-19 / Codex
- Decision: Reuse the Rust scaffold's explicit `--bootstrap-database` process contract for Go instead of special-casing database creation inside CI.
  Rationale: The same command can serve local setup, browser E2E, and integration testing, reducing divergent database provisioning logic.
  Date/Author: 2026-08-19 / Codex
- Decision: Add an optional standard Go dotenv dependency rather than implementing a custom parser.
  Rationale: The generated docs already promise `.env` behavior; a maintained parser has a smaller correctness and security surface than project-local parsing.
  Date/Author: 2026-08-19 / Codex
- Decision: Preserve the existing Jig launcher architecture and explicitly install Rust in Jig-invoking Go workflows.
  Rationale: Replacing source-based Jig installation with release binaries is a separate distribution project. Explicit prerequisites are the smallest truthful fix here.
  Date/Author: 2026-08-19 / Codex
- Decision: Represent the PostgreSQL-only API command parser as its own generated file.
  Rationale: File-level capability selection avoids whitespace-sensitive source fragments and keeps database-free source independent of PostgreSQL command policy.
  Date/Author: 2026-08-19 / Codex

## Outcomes & Retrospective

Work is in progress. At completion this section will list observable generated-project results, validation receipts, commits, and any deferred design work.

## Context and Orientation

`crates/jig/src/bootstrap/scaffold.rs` converts init options into an `InitScaffoldPlan`. `crates/jig/src/bootstrap/scaffold/go_workspace.rs` chooses the Go-owned files and supplies template values. `templates/scaffolds/go-react/workspace/` contains application source. Shared frontend and browser templates live under `templates/scaffolds/rust-react/frontend/` because both backend presets use the same React workspace. Managed harness configuration and workflows live under `templates/project/`. Every edited template has an embedded copy under `crates/jig/src/bootstrap/**/embedded_template_snapshots/`; the repository's snapshot generation/check commands must keep those copies byte-identical.

An application API contract means `openapi/public.json` plus the generated TypeScript tree under `packages/public-api-client/src/generated`. It is distinct from the Jig command contract in `.agent/jig-contract.json`. Database bootstrap means creating the configured PostgreSQL database if absent and then applying embedded Goose migrations. Migration immutability means rejecting a change to any migration file already present in the comparison base branch.

The source tree is itself a Jig-managed repository. After modifying runtime Rust, build `target/debug/jig` and set `JIG_DEV_BIN=target/debug/jig` for all `scripts/jig` commands so validation exercises current code.

## Plan of Work

First, add a Go test that serializes `Service.API.OpenAPI()` and compares it byte-for-byte with `openapi/public.json`. Make the webapp workflow observe Go API, OpenAPI document, and generated public-client paths so its existing transactional `contract:check` catches both schema and client drift. Correct documentation to name the application contract command rather than `scripts/jig check contract`.

Second, add optional `.env` loading in `cmd/api`, using `github.com/joho/godotenv`. Add a `--bootstrap-database` mode for PostgreSQL builds. In `internal/database`, define a bootstrap function that parses the target URL, connects to the `postgres` maintenance database, creates the requested database with a safely quoted identifier when absent, tolerates a concurrent duplicate-database result, then opens the target and applies Goose migrations. Use this command from the generated bootstrap command and from Playwright. Change the disposable PostgreSQL script so the integration test proves database creation instead of asking Docker to create the target in advance.

Third, update Go workflows to install Rust before invoking `scripts/jig`, configure `actions/setup-go` to hash `go.mod` so caching works before `go.sum` exists, and include sqlc query directories in change filters. Add render assertions for these workflow invariants.

Fourth, introduce a backend-neutral migration policy directory in generated configuration while retaining the existing Rust key for compatibility. Make runtime policy resolution prefer the neutral value and fall back to the Rust value. Render the migration-immutability tool and gate for PostgreSQL Go repositories and enable the workflow job outside the Rust-only SQLx branch. Add context, contract, and policy tests for both old Rust configuration and new Go configuration.

Fifth, validate every Go module segment, rejecting empty, `.` and `..` components before destination mutation. Preserve numeric wizard input `2` as harness-only and assign Go a new numeric alias. Replace the inline format check with a command that propagates `gofmt` failures and inspects only tracked or unignored project Go files. Thread `ScaffoldPreset` through frontend output-path calculation so reservation and rendering use the same template set. Add focused regression tests.

After every slice, run its narrow Rust tests plus generated-project checks, inspect `git diff --check`, and commit only that slice. After all slices, rebuild Jig and run `JIG_DEV_BIN=target/debug/jig scripts/jig check test`, along with contract, formatting, Clippy, agent map, and agent guide gates if they are not already included. Generate database-free and PostgreSQL Go repositories and exercise their locked Go checks, OpenAPI checks, and disposable PostgreSQL test. Record receipts and close structured work only after all required checks pass.

## Concrete Steps

All commands run from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`.

For template and unit-test iteration:

    cargo test -p jig-sh bootstrap::tests
    cargo test -p jig-sh doctor::tests
    cargo test -p jig-sh policy::tests
    cargo fmt --all -- --check
    git diff --check

After Rust runtime edits:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig check contract

For generated Go acceptance, create temporary repositories with the development binary and run:

    scripts/jig init <temp>/no-db --preset go-react --go-module example.com/no-db --db none --frontends web --no-input --no-vault
    scripts/jig init <temp>/postgres --preset go-react --go-module example.com/postgres --db postgres --frontends web --no-input --no-vault
    (cd <temp>/no-db && scripts/jig bootstrap && scripts/jig check fmt && scripts/jig check lint && scripts/jig check test-locked)
    (cd <temp>/postgres && scripts/jig bootstrap && scripts/jig check sqlc && bun run test:postgres)

The final repository command is:

    JIG_DEV_BIN=target/debug/jig scripts/jig check test

Success means every command exits zero, `git diff --check` prints nothing, and `git status --short` contains only the planned append-only state updates before the closing commit.

## Validation and Acceptance

A Go HTTP type change without regenerating OpenAPI must make `go test ./...` fail with a stale-document message. Regenerating the document but not the TypeScript client must make the web application contract check fail. Regenerating both must pass.

For PostgreSQL, copying `.env.example` to `.env` and running the generated setup/dev flow must make the API start without exporting `DATABASE_URL` separately. Against a PostgreSQL server containing only the `postgres` maintenance database, `go run ./cmd/api --bootstrap-database` must create the configured target, apply migration `00001_app_metadata.sql`, and exit successfully. Repeating the command must be safe. The disposable integration test and browser E2E must use this path.

Generated Go workflows must install both Go and the Rust prerequisite for `scripts/jig`, must be cacheable with only the initial scaffold files, and must run sqlc drift checks for changes under `internal/database/queries/`.

Changing a committed Goose migration relative to a base branch must fail `scripts/jig check migration-immutability --changed-against <base>`. Adding a new migration must pass. Existing Rust repositories that only configure `rust_migration_dir` must continue to pass.

Invalid module paths containing `.` or `..` segments must fail before files are published. A malformed Go file must make `scripts/jig check fmt` return nonzero. Numeric wizard input `2` must remain harness-only. Frontend reserved output paths must match the files selected for both Rust and Go presets.

## Idempotence and Recovery

All template regeneration and tests are repeatable. Database bootstrap uses existence checks and treats a concurrent duplicate-database result as success before applying idempotent Goose migrations. Temporary generated repositories and containers must live outside the source tree or under ignored `.agent/tmp` paths; disposable database scripts remove their named containers on exit.

If a slice fails validation, do not commit it. Keep the living plan updated, fix the slice, and rerun its narrow tests. Once a slice is committed, repair later discoveries in a new focused commit rather than rewriting earlier commits unless the user explicitly requests history rewriting. Append-only `.agent/state/*.jsonl` records must never be manually truncated or rewritten.

## Artifacts and Notes

The initial comprehensive review identified the highest-risk observable failures: stale Huma contracts can merge, PostgreSQL E2E addresses a database that does not exist, `.env` is documented but ignored, and custom Go CI runners lack the Rust prerequisite used by the Jig launcher. The implementation commits and final receipt identifiers will be recorded here as work proceeds.

## Interfaces and Dependencies

The generated Go module will add `github.com/joho/godotenv` for optional development environment loading. PostgreSQL scaffolds will expose `database.Bootstrap(ctx context.Context, databaseURL string) error`; `database.Open(ctx context.Context, databaseURL string) (*pgxpool.Pool, error)` remains the runtime entrypoint and continues applying Goose migrations before returning a live pool.

`cmd/api` will accept no arguments for normal serving and `--bootstrap-database` for create-and-migrate setup in PostgreSQL scaffolds. Database-free scaffolds will keep the no-argument server surface.

The Jig runtime will expose a backend-neutral migration-directory accessor used only by migration immutability policy. Existing `rust_migration_dir` configuration remains accepted as a fallback so generated repository updates are compatible.

Plan revision note (2026-08-19): Replaced the structured-work placeholder with a self-contained implementation plan after the comprehensive review. The revision records the structural root cause, five commit slices, compatibility decisions, and executable acceptance criteria.
