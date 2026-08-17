# Harden the generated Rust–React developer experience

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current while the work proceeds.

## Purpose / Big Picture

After this change, a newly initialized Rust–React workspace can bootstrap its frontend without a running PostgreSQL server, receives a deterministic workspace dependency lockfile, and tells a developer exactly how to recover when dependencies are absent. Its generated root README gives a complete first-run path, PostgreSQL users get a one-command disposable integration database, and the separately deployed admin server denies every protected route until the application explicitly supplies an authorizer. The frontend dependencies, including Astro, are updated to current compatible exact versions and the result is proven in a freshly generated all-components application.

## Progress

- [x] (2026-08-16 11:41Z) Reviewed repository guidance, the Rust bootstrap architecture, generated dependency lifecycle, and the QA report from an all-components fixture.
- [x] (2026-08-16 11:41Z) Opened structured Jig work as `plan_01M055SHFKGN72C4S8TJY6W07R`.
- [x] (2026-08-16 12:01Z) Updated all stale compatible frontend pins, Astro to 7.2.2, npm to 12.0.2, shadcn provenance, npm's reviewed esbuild install-script policy, and Vite's native-config-compatible path usage.
- [x] (2026-08-16 12:01Z) Reordered bootstrap so frontend dependency setup completes before PostgreSQL validation or migration.
- [x] (2026-08-16 12:01Z) Added exact recovery diagnostics and source tests proving root workspace lock creation plus frozen reuse.
- [x] (2026-08-16 12:01Z) Generated a database- and package-manager-aware root quickstart.
- [x] (2026-08-16 12:01Z) Generated an opt-in disposable PostgreSQL integration-test command with cleanup and test-database safeguards.
- [x] (2026-08-16 12:01Z) Added an async generic admin authorization boundary whose generated binary explicitly passes the deny-all implementation, plus deny/allow/fallback tests.
- [x] (2026-08-16 12:57Z) Refreshed embedded template snapshots, formatted generated Rust, and updated the changelog.
- [x] (2026-08-16 12:57Z) Passed focused source tests and repository gates, then initialized and fully validated a fresh all-components application, including a real disposable PostgreSQL 18 run.
- [x] (2026-08-16 12:57Z) Recorded fresh plan-linked gate evidence in batch receipt `receipt_01M05AAA9CHQJQ6SVVHA58H5YN`; the structured Jig work item is ready to close successfully.

## Surprises & Discoveries

- Observation: `scripts/jig bootstrap` currently checks PostgreSQL configuration and readiness before invoking `scripts/check-webapps.sh bootstrap`, so an unavailable database prevents creation of the frontend lockfile and `node_modules` even though those operations do not need a database.
  Evidence: `scaffold_bootstrap_command` in `crates/jig/src/bootstrap/scaffold/frontend.rs` emits the database guard and database bootstrap before the web bootstrap command.
- Observation: dependency gates fail without a useful message when no selected-manager lockfile exists because the frozen-install worker returns status 1 after `dependency_lockfile` fails silently.
  Evidence: `run_dependency_install` and `run_dependency_install_worker` in `templates/project/scripts/check-webapps.sh.jinja`.
- Observation: the workspace install is already serialized and uses exact direct pins, so deterministic lock creation can remain an explicit bootstrap step instead of making `jig init` depend on the network.
  Evidence: the generated checker uses a shared dependency lock and selected package-manager frozen/install modes.
- Observation: the generated admin HTTP crate is a separate deployment artifact, but its router currently accepts only application state and exposes no mandatory authorization decision point for future routes.
  Evidence: `templates/scaffolds/rust-react/workspace/crates/admin-http/src/lib.rs.jinja` and `apps/admin-api/src/main.rs.jinja`.
- Observation: registry inspection found stale exact pins across all frontend variants; the latest `@types/node` major and TypeScript major need compatibility validation rather than an untested blind bump.
- Observation: TypeScript 7.0.2 is newer, but `typescript-eslint` 8.67.0 declares `typescript >=4.8.4 <6.1.0`; TypeScript 6.0.3 is therefore the newest compatible stable line for this stack. `@types/node` 22.20.1 is current on the generated Node 22 line, while the registry-wide latest targets Node 26.
  Evidence: npm registry peer metadata queried on 2026-08-16 and fresh-fixture `npm outdated` output.
- Observation: Astro 7.2.2's `astro check` mutates the ordinary top-level `node_modules/.astro` runtime cache. Excluding only Vite/cache/temp directories caused the next workspace app to perform a complete frozen reinstall.
  Evidence: the first fresh all-components lint and typecheck each reinstalled 877 packages immediately after the Astro check.
- Observation: npm 12 blocks unreviewed dependency install scripts and identified only `esbuild@0.28.2`; Vite/Astro builds still worked via the platform package, but every install emitted a remediation warning.
  Evidence: fresh-fixture `npm install-scripts ls --json` and npm's official install-script approval documentation.
- Observation: Vite 8.2.1 warns that `__dirname` prevents its future native config loader; generated Node 22 supports `import.meta.dirname` directly.
  Evidence: fresh-fixture production builds for both Vite applications.
- Observation: an eager missing-lockfile preflight broke the existing dependency-free standalone Bun path, while rejecting a missing receipt before `run-script` broke its intentional non-installing behavior.
  Evidence: the first plan-linked full test run found exactly those two regressions; recovery checks now run only after the underlying frozen install or script fails, preserving the primary package-manager error and existing successful paths.

## Decision Log

- Decision: Keep `jig init` offline-capable and create the selected package manager's root workspace lockfile through `scripts/check-webapps.sh bootstrap`.
  Rationale: registry resolution is inherently external; making initialization perform it would weaken deterministic/offline scaffolding. A single serialized bootstrap install with exact direct pins gives a reproducible committed artifact and a clear refresh boundary.
  Date/Author: 2026-08-16 / Codex
- Decision: Run frontend bootstrap before any database guard or migration in the generated setup command.
  Rationale: frontend dependencies have no PostgreSQL dependency and should remain recoverable even when the database is intentionally unavailable.
  Date/Author: 2026-08-16 / Codex
- Decision: Protect admin routes with a generic authorizer supplied to router construction, and make the generated admin binary pass a `DenyAllAdminAuthorizer` explicitly.
  Rationale: this creates a fail-closed compile-time extension point without inventing a production authentication mechanism or secret format for the generated application.
  Date/Author: 2026-08-16 / Codex
- Decision: Run PostgreSQL integration tests in an ephemeral Docker container bound to loopback and a random host port, require a `test_db_` database name before migrations, and clean up via a shell trap.
  Rationale: the command must not reuse or mutate a developer database, collide on a fixed port, or leave a long-running service behind.
  Date/Author: 2026-08-16 / Codex
- Decision: Keep TypeScript at 6.0.3 and `@types/node` on 22.20.1 while updating every other stale exact frontend pin.
  Rationale: these are compatibility/runtime-major constraints, not forgotten pins. Moving either would deliberately put the generated stack outside `typescript-eslint`'s peer range or outside its Node 22 runtime contract.
  Date/Author: 2026-08-16 / Codex
- Decision: Approve only the currently resolved `esbuild@0.28.2` install script in npm-generated workspaces.
  Rationale: esbuild is a reviewed build dependency; version-specific approval removes noisy/partially initialized installs without granting future transitive versions blanket script execution.
  Date/Author: 2026-08-16 / Codex
- Decision: Treat only a top-level `node_modules/.astro` directory as ignorable runtime cache, with the same type/symlink/toplevel constraints as existing Vite caches.
  Rationale: it is generated by normal Astro checks and must not invalidate dependency state, while similarly named nested paths or non-directory replacements remain attested as tampering.
  Date/Author: 2026-08-16 / Codex
- Decision: Append the exact bootstrap recovery hint after dependency-backed operations fail instead of requiring a lockfile or receipt before every operation.
  Rationale: this gives actionable failures without changing valid dependency-free Bun installs or turning `run-script` into an installing command.
  Date/Author: 2026-08-16 / Codex

## Outcomes & Retrospective

The generated stack now bootstraps frontend dependencies before touching PostgreSQL, creates and deterministically reuses the root manager lockfile, emits an exact recovery command on missing dependency state, and documents the complete first-use flow. PostgreSQL scaffolds include an isolated one-command integration test, while the admin server is a separate binary with an explicit async authorizer and a deny-all default.

The final fixture at `/tmp/jig-rust-react-final.jTOkZm/app` passed lint, typecheck, coverage, production builds, contract generation/checking, public-artifact isolation, Rust tests, formatting, clippy, npm audit with zero vulnerabilities, and a real disposable PostgreSQL 18 test whose container was removed afterward. Bootstrap without either database URL first installed 877 workspace packages and created the lockfile, then stopped at the expected database guard; a repeated bootstrap preserved lockfile SHA-256 `904dc08e7b5898fe2ab34cd9752b990a8465f3d95b6f3f9251e8e4f0efea7049`.

The only versions intentionally below registry-wide latest are TypeScript 6.0.3, constrained by `typescript-eslint`'s `<6.1.0` peer range, and `@types/node` 22.20.1, aligned with the generated Node 22 runtime. No functional acceptance item is deferred.

## Context and Orientation

The scaffold implementation lives under `crates/jig/src/bootstrap/scaffold/`; canonical user-facing templates live under `templates/scaffolds/rust-react/` and `templates/project/`. Embedded copies under `crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/` are generated snapshots and must be refreshed after canonical edits. `frontend.rs` constructs frontend template context and the generated setup shell command. `rust_workspace.rs` constructs Rust workspace and database context. Source-level scaffold acceptance tests live under `crates/jig/src/bootstrap/tests/`.

The generated repository uses a root JavaScript workspace. `scripts/check-webapps.sh` owns dependency installation, per-app checks, and dependency receipts. A lockfile is the selected manager's root lock (`package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, or `bun.lock`). `scripts/jig setup` invokes the scaffold-configured bootstrap command. PostgreSQL bootstrap is performed by the generated Rust `bootstrap-database` binary.

The public API binary depends on the public `http` crate. Admin HTTP routes and the `admin-api` binary are generated only when the admin frontend is selected. They form a separate deployment boundary and must remain absent from the public server dependency graph and image.

## Plan of Work

First, update exact versions in every frontend package template and the shadcn provenance constant. Use npm registry metadata for the current releases, retain a supported Node type major when the generated Node runtime remains on that major, and validate any major TypeScript or jsdom move against typecheck, tests, builds, and the generated clients.

Second, change the generated setup command order to fetch Rust dependencies, bootstrap the frontend workspace, and only then validate/bootstrap the database. Enhance `check-webapps.sh` with a shared diagnostic that names `scripts/check-webapps.sh bootstrap` when the selected workspace has no lockfile or its installed dependency state is absent. Preserve the silent readiness probe and serialized install behavior. Add tests proving a new repository can create its root lock while PostgreSQL is unavailable and that subsequent frozen checks use it.

Third, render a root `README.md` from frontend workspace context so it can state the selected package manager, enabled apps, database setup, contract commands, public/admin deployment boundary, and recovery path. For PostgreSQL scaffolds, add `scripts/test-postgres.sh` and a generated integration test. The script will create an isolated container, wait for readiness, export `TEST_DATABASE_URL`, run the test-support integration target, and always remove the container. The Rust test will validate the current database name before applying migrations.

Fourth, add `AdminAuthorizer`, `AdminAuthorizationError`, and `DenyAllAdminAuthorizer` to the admin HTTP template. Construct protected admin routes through authorization middleware and make the generated admin binary opt into the deny-all implementation until the application replaces it. Test deny, allow, request-ID error shape, and unmatched-route behavior.

Finally, refresh embedded templates, update assertions and changelog, build the development Jig binary, and run focused and full checks through `scripts/jig`. Generate a clean all-components fixture using that binary, prove frontend bootstrap without PostgreSQL, run contracts and all configured checks, run the disposable PostgreSQL test when Docker is available, inspect public deliverables/dependency trees for admin leakage, and record receipts.

## Concrete Steps

Run commands from `/home/aa/Documents/jig-sh` unless otherwise noted.

1. Query authoritative npm registry metadata for remaining ambiguous compatible versions and patch canonical package templates plus version assertions.
2. Patch `templates/project/scripts/check-webapps.sh.jinja` and `crates/jig/src/bootstrap/scaffold/frontend.rs`; add or adjust bootstrap and dependency-lifecycle tests.
3. Add the root README and PostgreSQL test templates; register their template descriptors and context; add render/lifecycle tests.
4. Patch the admin HTTP and binary templates, supporting manifests/guidance, and focused generated Rust tests.
5. Refresh snapshots with `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`.
6. Run `cargo fmt --all`, focused `cargo test -p jig-sh bootstrap`, then build `cargo build -p jig-sh --bin jig`.
7. Export `JIG_DEV_BIN=target/debug/jig`, run configured Jig checks and evidence commands, and initialize a fresh temporary all-components application for end-to-end validation.

## Validation and Acceptance

Acceptance requires all of the following observable results:

- No obsolete exact frontend dependency pin remains in canonical package templates; Astro resolves to a release containing the upstream security fix, and a fresh install reports no actionable known vulnerability from the old pin.
- In a fresh PostgreSQL scaffold with PostgreSQL unavailable, `scripts/jig bootstrap` reaches and completes frontend dependency bootstrap before reporting the database problem; the root workspace lockfile and install state exist afterward.
- A dependency gate without a lockfile prints the exact recovery command `scripts/check-webapps.sh bootstrap`.
- Running dependency bootstrap twice is serialized/idempotent, and a following frozen install/check accepts the committed root lock.
- Generated `README.md` provides runnable first-use, contract, admin, and database-test commands.
- `bash scripts/test-postgres.sh` starts an isolated database, runs the generated test and migrations only after validating a `test_db_` database, and removes the container on success or failure.
- Admin matched routes return a standardized unauthorized response under the default authorizer, an explicit allow implementation can admit them, unmatched routes remain 404, and request IDs propagate.
- The public binary and public frontend artifacts contain neither the admin client nor admin operation names/specification.
- Repository tests and configured gates pass, followed by lint, typecheck, test/coverage, build, contract check, Rust test, fmt, and clippy in a new all-components generated repository.

## Idempotence and Recovery

Canonical templates are the source of truth; embedded snapshots are regenerated rather than hand-maintained. Snapshot refresh is repeatable. Dependency bootstrap uses the generated checker lock to avoid concurrent workspace installs and may be rerun after interruption. The disposable PostgreSQL script uses a unique container name and a trap, so rerunning does not reuse application data; if the host terminates the shell before the trap executes, the exact emitted container name can be removed with `docker rm -f NAME`. All fixture generation occurs in a temporary directory and does not overwrite repository files.

If a package major proves incompatible, record the concrete failing command and choose the newest version supported by the generated toolchain, documenting the constraint in the decision log rather than suppressing checks. If Docker is unavailable, retain source-level lifecycle tests and report the skipped real-container validation explicitly.

## Artifacts and Notes

- Structured work ID: `plan_01M055SHFKGN72C4S8TJY6W07R`.
- Final plan-linked gate receipt: `receipt_01M05AAA9CHQJQ6SVVHA58H5YN` (`jig.contract_check` and the full 1,941-test `jig.test` suite passed with fresh worktree coverage).
- Additional repository receipts: formatting `receipt_01M0597PRGZAR4V5VDDGHNQZD2`, clippy `receipt_01M05989E2JWG45R7ZDB46FKNZ`, and contract `receipt_01M0598FSYSAY0B5B0B08J8JTF`.
- Final all-components fixture: `/tmp/jig-rust-react-final.jTOkZm/app`.
- Prior all-components QA fixture: `/tmp/jig-rust-react-qa.HwXsTA/app` (diagnostic only; final proof uses a newly generated fixture).
- The worktree contains earlier related public/admin OpenAPI changes. They are part of the same user-requested hardening sequence and must be preserved.

## Interfaces and Dependencies

The generated admin HTTP crate exposes:

    #[async_trait]
    pub trait AdminAuthorizer: Clone + Send + Sync + 'static {
        async fn authorize(
            &self,
            request: &AdminRequestContext,
        ) -> Result<(), AdminAuthorizationError>;
    }

    pub struct DenyAllAdminAuthorizer;

    pub fn router<A: AdminAuthorizer>(state: AppState, authorizer: A) -> axum::Router;

The public API and admin API continue to use the shared `http-common` error envelope. The PostgreSQL integration command depends on a working Docker-compatible CLI and the pinned PostgreSQL container image documented in the generated README. The dependency lifecycle continues to support npm, pnpm, yarn, and Bun through the existing selected-manager branches.
