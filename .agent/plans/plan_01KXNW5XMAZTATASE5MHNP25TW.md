# Add real-backend Playwright coverage to generated web apps

After this change, a repository created with `jig init --preset rust-react` has a package-local Playwright smoke suite in every generated SPA package. Running the package's E2E script starts the selected SPA and the generated production API on isolated loopback ports, waits for both services, and verifies that the browser renders data returned by the live Rust HTTP server rather than a mocked fetch.

## Progress

- [x] Map the scaffold renderer, generated frontend/backend boundaries, existing test harness, and current CI/check contract.
- [x] Choose a package-local Playwright suite instead of expanding the public Jig check contract for adopted and non-SPA apps.
- [x] Add the Playwright templates, dependency/scripts, typecheck coverage, documentation, and generated artifact ignores.
- [x] Add focused scaffold-rendering regressions and refresh the embedded scaffold snapshot.
- [x] Render a clean SQLite scaffold and run its lint, unit tests, typecheck, and real-backend E2E smoke test.
- [x] Run the relevant Jig work checks/gates and inspect the final diff.

## Surprises & Discoveries

- The generated SPA already has a mocked Vitest browser-to-API unit slice, while the Rust workspace already has in-process Axum integration tests. The missing layer is specifically a real browser over a real socket.
- `scripts/jig dev` owns app selection and cleanup, but its proxy daemon and state directory intentionally outlive an app session. Using it here would couple product E2E to shared machine state and conflict with an already-running development session.
- A first-class `typescript-e2e` tool would change the public contract for adopted SPAs, admin apps, and Astro apps that do not necessarily have a generated Rust backend. Package-local E2E is a narrower initial contract.
- The working tree contains a substantial pre-existing scaffold series. All edits must remain additive and preserve staged and unstaged user changes.
- Starting structured work exposed a pre-existing/current-runtime JSONL recursion error in `scripts/jig work status`; preserve append-only state and diagnose after rebuilding the development binary rather than rewriting state.
- The first rendered TypeScript check correctly caught that including `playwright.config.ts` also requires Node ambient types; the SPA now typechecks both browser and E2E sources.
- Local ports 4173 and 4174 were already owned by unrelated Bun processes. The suite failed closed rather than reusing them, and the documented `E2E_WEB_PORT` / `E2E_API_PORT` overrides produced a clean passing run with both ports released afterward.
- Starting Vite through an npm wrapper made successful SIGTERM cleanup print a misleading lifecycle error. Starting the local `vite` binary directly preserves Playwright-owned teardown without that noise.
- Playwright runs `globalTeardown` before stopping configured `webServer` processes. SQLite cleanup there races the live API, so the suite resets its ignored database before startup instead.
- GitHub Actions parses unquoted YAML keywords such as `null` as scalars rather than strings. Scaffolded matrix names, directories, and the default branch are quoted and regression-tested.

## Decision Log

- Generate Playwright only for `ScaffoldFrontendKind::Spa`, matching the request for the product web package. Do not silently impose backend E2E semantics on adopted repositories or Astro packages.
- Start the production API binary and Vite directly as two Playwright `webServer` processes. Use dedicated, environment-overridable loopback ports, application-level readiness URLs, strict Vite port binding, and bounded SIGTERM cleanup.
- Keep the Vite proxy in the tested path by injecting the direct API origin. A separate Jig runtime smoke test should own the persistent `*.localhost` proxy topology.
- Never fall back to the developer database: use a clean ignored SQLite file or a dedicated `_e2e` Postgres database, both bootstrapped through the generated production API's existing database mode.
- Keep Chromium as the default browser for a fast scaffold smoke test. Capture trace and screenshot evidence on failure and provide UI/debug scripts for local authoring.
- Generate a scaffold-owned SPA-only E2E workflow rather than extending managed web checks for adopted, Astro, or admin packages. PostgreSQL CI jobs receive isolated service containers; SQLite jobs use package-specific ignored databases.
- Allow `E2E_BASE_URL` to target an already-running local or preview environment; otherwise Playwright owns and cleans up both direct servers.
- Do not add sleeps, snapshots, an all-browser matrix, or E2E coverage thresholds to the starter suite.

## Outcomes & Retrospective

Generated SPA packages now include a Playwright suite that drives Chromium through Vite's proxy into the production Axum API, observes the real `/api/version` response, and verifies readiness without mocks. Package-specific database defaults isolate multiple SPAs; SQLite resets before each run, and PostgreSQL CI uses a fresh service-backed database. The scaffold also emits an SPA-only matrix workflow with portable Bun, npm, pnpm, and Yarn commands plus retained Playwright artifacts.

Verification covered all 12 database/package-manager workflow renders, YAML keyword edge cases, template registry completeness, a freshly initialized npm/SQLite repository's lint/typecheck/Vitest suite, repeated live-backend Playwright runs with database reset and port release, all Rust tests, Clippy, rustfmt, contract checks, and required Jig work gates.

## Context and Orientation

`crates/jig/src/bootstrap/scaffold/frontend.rs` owns the explicit Vite/React scaffold file registry and render context. The generated SPA templates live in `templates/scaffolds/rust-react/frontend/vite-react/`. The generated API accepts `BIND_ADDR`, exposes `/health/ready`, and has a database-bootstrap mode for database-backed presets. Embedded scaffold templates are refreshed into `crates/jig/src/bootstrap/scaffold/embedded_templates_snapshot.rs` with `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`.

## Plan of Work

Add a database-aware Playwright config and semantic smoke spec to the SPA template registry. Extend the package template with a pinned Playwright dependency and E2E authoring/install scripts, keep Vitest collection separate, include the new sources in TypeScript validation, document browser/Postgres prerequisites, and ignore generated reports. Add assertions to the full scaffold test and collision-path test. Refresh snapshots, render a clean SQLite repository, bootstrap dependencies, install Chromium, and run the suite against its live backend and isolated E2E database.

## Validation and Acceptance

Acceptance requires all of the following observable outcomes:

- A rendered SPA contains `playwright.config.ts` and `e2e/app.spec.ts`.
- Its package exposes `test:e2e`, `test:e2e:ui`, and browser-install scripts and pins `@playwright/test`.
- `typecheck` and `lint` include the E2E sources.
- The smoke test observes a successful `/api/version` browser response from the generated API and renders `Ready`.
- Playwright starts and stops only the production API plus the selected Vite server, with no arbitrary timing sleeps or shared proxy state.
- `playwright-report/` and `test-results/` remain untracked.
- Focused Rust scaffold tests and configured Jig gates pass.

## Idempotence and Recovery

Template snapshot refresh and scaffold rendering are repeatable. Generated E2E reports are ignored. Playwright owns both server processes when `E2E_BASE_URL` is absent and requests graceful termination. Dedicated ports fail fast rather than reusing unrelated listeners, and both ports are overridable for a retry. Existing `.agent/state/*.jsonl` files remain append-only.

## Interfaces and Dependencies

The generated package adds `@playwright/test` and uses its `webServer`, `baseURL`, trace, screenshot, and Chromium project APIs. It starts the existing generated Cargo API package with `BIND_ADDR` and, when applicable, a dedicated `DATABASE_URL`; no Jig contract tool or persisted state schema changes are introduced.
