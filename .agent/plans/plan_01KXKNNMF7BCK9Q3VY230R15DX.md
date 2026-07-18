# Generate a production-shaped shadcn admin application

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while work proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

After this change, `jig init ./demo --preset rust-react --frontends web,landing,admin` creates a coherent application instead of three unrelated hello-world frontends. The admin application is a real, responsive shadcn dashboard shell derived from a tested shadcn CLI release, can immediately accept future `shadcn add` commands, and visibly reports the generated Rust API's version and readiness. The ordinary web application also exercises that API boundary. Frontend installation happens once at the repository workspace root during `scripts/jig bootstrap`; starting a dev server never performs an implicit network mutation. Generated configuration records whether a frontend is a product SPA, admin application, or Astro site without guessing from its name.

A human can see the result by rendering a temporary repository with a freshly built Jig binary, running its bootstrap and checks, starting `scripts/jig dev`, and opening the generated web and admin routes. The admin route must show the application name, API version, readiness, navigation sidebar, and theme control; stopping the API or returning an invalid response must show an actionable retry state.

## Progress

- [x] (2026-07-15 20:01Z) Inspected the current scaffold, reports, generated configuration, frontend checks, package lifecycle, and Rust HTTP endpoints.
- [x] (2026-07-15 20:01Z) Generated a clean upstream reference project using `shadcn@4.13.0`, Vite, the `nova` preset, Radix primitives, and the required admin components.
- [x] (2026-07-15 20:22Z) Added explicit, backward-compatible frontend roles to answers, runtime config/info, generated config, scaffold reports, and adoption defaults.
- [x] (2026-07-15 20:22Z) Split SPA, Astro, and admin inventories and added deterministic shadcn 4.13.0 provenance.
- [x] (2026-07-15 20:22Z) Added the pinned root JavaScript workspace, one bootstrap install path, immutable CI installs, and mutation-free app `dev` scripts.
- [x] (2026-07-15 20:22Z) Implemented and tested the SPA/admin API slices, responsive admin shell, source-owned UI components, routing, and themes.
- [x] (2026-07-15 20:35Z) Refreshed both embedded snapshots through the supported build flag and updated public documentation, generated guidance, public diagnostics, and preset discovery text.
- [x] (2026-07-15 20:35Z) Rendered full Bun/Postgres and npm/admin repositories from fresh binaries, created one root lockfile each through bootstrap, and passed generated Rust, lint, typecheck, build, coverage, contract, shadcn-info, and live-dev checks.
- [x] (2026-07-15 20:40Z) Ran structured work checks, required gates, and evidence against the current worktree; audited every requirement and finding with no unresolved gate or implementation issue.

## Surprises & Discoveries

- Observation: the current `Admin` scaffold is not distinct from the SPA scaffold; both return `VITE_REACT_TEMPLATES`, and only the subtitle changes.
  Evidence: `crates/jig/src/bootstrap/scaffold/frontend.rs` maps `ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin` to one file list.
- Observation: generated frontend `dev` scripts currently run `<package-manager> install` even though the generated bootstrap command also installs every frontend.
  Evidence: `templates/scaffolds/rust-react/frontend/vite-react/package.json.jinja` embeds the install command in `dev`, while `scaffold_bootstrap_command` loops over frontend directories.
- Observation: the generated Vite proxy already forwards `/api` and `/health`, and the Rust HTTP crate already exposes `/api/version`, `/health/live`, and `/health/ready`; the missing piece is the frontend client and user-visible state.
  Evidence: `templates/scaffolds/rust-react/frontend/vite-react/vite.config.ts.jinja` and `templates/scaffolds/rust-react/workspace/crates/http/src/lib.rs.jinja`.
- Observation: current shadcn's project initializer succeeds when run directly with `-t vite` in a terminal, while applying it to the newest standalone `create-vite` output failed framework detection. The successful reference emitted Tailwind 4, `components.json`, theme support, source-owned components, and a `shadcn` runtime CSS dependency.
  Evidence: `npx --yes shadcn@4.13.0 init -t vite -b radix -p nova --no-monorepo -y -n shadcn-admin` completed and `shadcn add` created the selected components.
- Observation: exact dependency resolution from the successful reference produced React 19.2.7, Vite 8.1.4, TypeScript 6.0.3, Tailwind 4.3.2, and shadcn 4.13.0. The checked-in SPA still requests Vite 6 and TypeScript 5.7.
  Evidence: `npm list --depth=0 --json` in the isolated reference project and the current generated `package.json.jinja`.
- Observation: React's current hooks lint rejects a synchronous loading-state update inside an effect, and the source-owned shadcn component exports trip the fast-refresh component-only rule even though they intentionally export variants/helpers.
  Evidence: linting the first rendered Bun repository failed on those exact rules; moving loading transitions to event/retry boundaries and scoping the fast-refresh exception to `src/components/ui` made lint pass without weakening application-source checks.
- Observation: the initial success/error tests did not exercise enough API-client branches to satisfy the generated 80% branch gate.
  Evidence: rendered Vitest coverage identified HTTP-failure and connected-but-not-ready branches; adding behavior tests for both raised SPA and admin branch coverage above the configured threshold.
- Observation: local TypeScript checks still performed an unconditional install even after scaffold bootstrap, and concurrently starting npm checks caused the installs to race over the workspace symlink and `node_modules`.
  Evidence: the first clean npm integration render failed parallel lint/typecheck/build/coverage with npm `EEXIST`; the check runner now reuses present dependencies and serializes only the adopted-repository fallback install with an interruption-safe lock. A second and final clean npm render passed all four checks concurrently.
- Observation: the first complete repository test run found one stale preset presentation assertion after every implementation test passed.
  Evidence: `presets_summary_explains_defaults_and_ownership` expected the former generic Vite admin description; the assertion now requires the shadcn admin and provenance text, and the complete workspace test gate passes.
- Observation: final source inspection found the initial sidebar navigation nested a button inside a React Router link and the theme provider registered an undocumented global `D` shortcut.
  Evidence: the generated component source showed both behaviors. The sidebar now uses shadcn's `asChild` contract with the link as the interactive element, the shortcut was removed, and a fresh admin render passed lint, typecheck, build, and coverage.

## Decision Log

- Decision: bake tested shadcn source files into Jig's scaffold instead of invoking `shadcn@latest` during `jig init`.
  Rationale: init must be reproducible, reviewable, and usable without making its output depend on a mutable upstream CLI. A Jig release refreshes the checked-in source against a named CLI version, while generated `components.json` and the pinned `shadcn` dependency preserve the normal extension path.
  Date/Author: 2026-07-15 / Codex
- Decision: use shadcn CLI 4.13.0, the `nova` preset, Radix primitives, Tailwind 4, Lucide icons, and Geist as the initial admin design-system provenance.
  Rationale: this is the successfully generated current upstream shape observed during implementation. Pinning the provenance makes later refreshes explicit and testable.
  Date/Author: 2026-07-15 / Codex
- Decision: add `role = "spa" | "admin" | "astro"` to `[[frontend_apps]]`, default missing roles to `spa`, and never infer admin role from an app name.
  Rationale: runtime execution kind (`vite` or `env-port`) and product role are different facts. A default preserves old configurations; explicit scaffold inputs preserve semantics; adoption may identify Astro from its execution profile but must not treat a name such as `admin` as proof of an admin UI.
  Date/Author: 2026-07-15 / Codex
- Decision: create a root JavaScript workspace even for one generated frontend and keep frontend directories at their existing top-level locations.
  Rationale: one root install and one root lockfile remove duplicated dependency resolution without mixing JavaScript packages into the Rust `apps` crate root or forcing a shared UI package before the project needs one.
  Date/Author: 2026-07-15 / Codex
- Decision: do not generate authentication.
  Rationale: a decorative login flow would imply a security boundary that the Rust backend does not enforce. The scaffold will have a clear provider/router seam where a project can add its chosen authentication system.
  Date/Author: 2026-07-15 / Codex

## Outcomes & Retrospective

The completed `rust-react` scaffold now produces a coherent, tested application shape. Its admin is a real shadcn 4.13.0/Tailwind 4 application with source-owned components, responsive navigation, theme selection, routing, API status views, error handling, and retry behavior. The SPA exercises the same typed browser-to-Rust boundary. Generated config carries explicit semantic roles and keeps older role-less repositories compatible.

Frontend dependency ownership is now clear: a pinned root workspace and one root lockfile, one bootstrap install, mutation-free dev scripts, pinned CI setup, and safe reuse by checks. The npm integration run exposed and drove the fix for a cross-check install race; the final clean Bun and npm renders pass all executable checks, and the no-database render was also exercised through live Jig proxy routes.

Repository format, clippy, full workspace tests, contract, agent guides, agent map, embedded snapshot checks, and structured required gates pass. The all-files Rust LOC and no-`mod.rs` policy diagnostics were inspected separately and report only existing repository-wide legacy violations outside this feature's completion gates; this work did not treat those unrelated baselines as evidence for or against the scaffold change.

## Context and Orientation

`crates/jig/src/bootstrap.rs` defines the `jig init`, `adopt`, and `update` command data. A `FrontendApp` stores a name, directory, coverage threshold, dev execution kind, and semantic role. `crates/jig/src/bootstrap/scaffold.rs` converts `--preset rust-react` options into an `InitScaffoldPlan`. `crates/jig/src/bootstrap/scaffold/frontend.rs` selects and renders files from `templates/scaffolds/rust-react/frontend`. Scaffold files are project-owned after init, so `jig update` does not rewrite them.

The generated harness is separate from the application scaffold. `templates/project/.jig.toml.jinja` records frontend checks and dev applications. `templates/project/scripts/check-webapps.sh.jinja` reuses bootstrapped dependencies, retains a serialized fallback for adopted layouts, and runs each frontend's scripts; `.github/workflows/webapp-checks.yml.jinja` owns CI installation and the same checks. `crates/jig/src/context.rs` parses generated `.jig.toml` at runtime and rejects unknown fields, with `role` defaulted for backward compatibility.

The Rust backend templates live below `templates/scaffolds/rust-react/workspace`. The HTTP template exposes health and version endpoints. The Vite config proxies relative `/api` and `/health` requests to the Jig-supervised API process. A vertical slice here means a small feature that crosses that whole boundary: the browser fetches the generated endpoint, validates the response shape, and presents loading, success, failure, and retry states.

The embedded-template snapshots in `crates/jig/src/bootstrap/embedded_templates_snapshot.rs` and `crates/jig/src/bootstrap/scaffold/embedded_templates_snapshot.rs` are generated copies compiled into dirty or unreleased Jig binaries. They must be refreshed after changing live templates; otherwise snapshot drift tests intentionally fail.

## Plan of Work

First, extend `FrontendApp` and `FrontendAppConfig` with a semantic role. Add shared defaults and validation for `spa`, `admin`, and `astro`; serialize the role into generated `.jig.toml`; include it in `jig info` and scaffold JSON; and change `FrontendScaffold::from_frontend_app` to select from the role rather than the app name. Existing answers and repositories without the field must continue to parse as `spa`. Adoption should emit `astro` for an env-port/Astro profile and `spa` otherwise, never `admin` based only on a filename.

Second, split frontend rendering into ordinary Vite SPA, shadcn admin, and Astro inventories. Add a root workspace template with `private`, `workspaces`, `engines.node`, and a package-manager declaration, plus `pnpm-workspace.yaml` only for pnpm. Render `.node-version` at the root. Change scaffold bootstrap to run one root install after optional `cargo fetch`. Remove all install commands from individual `dev` scripts. Update generated CI path filters and install logic to recognize the root workspace, use immutable Yarn installs, and keep old adopted repositories with per-app lockfiles working.

Third, update the ordinary Vite template to current tested React/Vite/TypeScript/Vitest dependencies and add a small API client plus a tested app status view. Create `templates/scaffolds/rust-react/frontend/admin-shadcn` from the successful upstream reference, retaining `components.json`, Tailwind 4 CSS, source-owned shadcn components, import aliases, and a provenance README. Build project-owned `app/providers.tsx`, `app/router.tsx`, `components/app-sidebar.tsx`, `components/mode-toggle.tsx`, `features/overview/overview-page.tsx`, and `lib/api.ts`. Tests must cover loading, successful version/readiness display, malformed or failed API responses, and retry behavior without depending on a running server.

Fourth, update scaffold tests from existence-only and string-only assertions to cover the new file inventory, explicit roles, shadcn provenance, root workspace, root install command, no install-on-dev behavior, and API UI. Add runtime configuration tests for valid, defaulted, and rejected roles. Refresh both embedded snapshots and update `README.md`, `docs/developer-ux.md`, `docs/configuration.md`, `docs/public-contract.md`, and preset discovery text.

Finally, build `target/debug/jig` and force every dogfood command through `JIG_DEV_BIN=target/debug/jig`. Render at least one full Bun repository with web, landing, admin, and Postgres, and one npm admin-only repository. Run bootstrap to generate the root lockfile, then run contract, Rust, and TypeScript checks. Inspect the generated applications directly and run `shadcn info` from the admin directory. Finish with repository format, clippy, test, contract, agent guide, and agent map checks, followed by structured work gates and evidence.

## Concrete Steps

All commands run from `/Users/aa/Documents/jig-sh` unless a command explicitly changes directory.

1. Keep the living plan and structured work open:

       export JIG_DEV_BIN=target/debug/jig
       scripts/jig work status

2. Make source and template edits with `apply_patch`. After template edits, refresh embedded snapshots:

       JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh

   Expect both checked-in snapshot files to change and the subsequent snapshot drift tests to pass without the refresh variable.

3. Build and run focused tests repeatedly:

       cargo fmt --all -- --check
       cargo test -p jig-sh bootstrap::tests
       cargo test -p jig-sh context::tests

4. Build the dev binary before any dogfood render:

       cargo build -p jig-sh --bin jig
       export JIG_DEV_BIN=target/debug/jig

5. Render representative repositories into temporary directories. Run `scripts/jig bootstrap`, then the generated `scripts/jig check typescript-*`, `scripts/jig check test`, and `scripts/jig check contract`. Preserve short command transcripts in this plan's `Artifacts and Notes` section.

6. Run the repository-wide gates:

       JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
       JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
       JIG_DEV_BIN=target/debug/jig scripts/jig check test
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract
       JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides
       JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map
       JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KXKNNMF7BCK9Q3VY230R15DX
       JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KXKNNMF7BCK9Q3VY230R15DX
       JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01KXKNNMF7BCK9Q3VY230R15DX

## Validation and Acceptance

Completion requires evidence for every statement below.

An old `.jig.toml` frontend entry without `role` loads successfully and reports role `spa`. New scaffolds emit explicit roles. Invalid role values fail with a message naming `spa`, `admin`, and `astro`. A Vite application called `admin` but configured with role `spa` renders the SPA template; an arbitrarily named frontend with role `admin` renders the shadcn admin template.

The full generated repository contains one root `package.json`, one package-manager lockfile after bootstrap, and no frontend-local lockfiles. Its bootstrap command installs JavaScript dependencies once from the root. Every frontend `dev` script starts only its dev server. Root metadata pins a supported Node baseline and the selected package manager. Bun, npm, pnpm, and Yarn command rendering remain covered by Rust tests even when only Bun and npm receive full network integration renders.

The admin application contains valid `components.json` provenance for `radix-nova`, imports Tailwind 4 and `shadcn/tailwind.css`, has source-owned UI components, and passes `shadcn info`. Its page renders a responsive sidebar and theme switcher. With mocked successful `/api/version` and `/health/ready` responses, tests find the generated repository name, version, and a ready badge. With a failed or malformed response, tests find an error message and can click Retry to issue a new request.

The ordinary web app also fetches `/api/version`, validates the returned name and version strings, and exposes loading, failure, retry, and success states. Both frontend coverage reports meet the generated threshold. The Vite proxy continues to honor `API_ORIGIN`, `JIG_DEV_API_ORIGIN`, and the generated Jig hostname.

The Rust workspace tests pass for no database, Postgres, and SQLite scaffold branches already covered by the test suite. Template snapshot drift tests pass. Public docs describe the root workspace, explicit role field, shadcn provenance, one-time bootstrap install, lockfile commitment, and the fact that authentication remains project-owned.

No completion claim may rely only on source inspection. The final audit must include successful generated-repository installs and executable frontend checks, plus all repository gates listed above.

## Idempotence and Recovery

Template rendering remains preflighted before writes, so duplicate output paths or collisions must fail before partially changing a destination. Snapshot refresh is idempotent and may be rerun after any template adjustment. Temporary reference and rendered repositories live outside the worktree and can be discarded after their command transcripts are captured.

If a package-manager bootstrap fails, keep its generated lockfile only if the command completed successfully; otherwise discard that temporary render and rerender from the fresh dev binary. Do not hand-edit generated embedded snapshot Rust files. If a role addition breaks an older config fixture, add the missing serde default rather than rewriting old fixtures, because backward compatibility is an acceptance requirement.

## Artifacts and Notes

The upstream reference was generated outside the repository with:

    npx --yes shadcn@4.13.0 init -t vite -b radix -p nova --no-monorepo -y -n shadcn-admin
    npx --yes shadcn@4.13.0 add card badge breadcrumb dropdown-menu table skeleton empty alert sonner sidebar separator tooltip sheet input -y -c shadcn-admin

The commands completed with `Project initialization completed` and created `components.json`, theme support, Tailwind CSS, UI component sources, and `src/lib/utils.ts`.

## Interfaces and Dependencies

`FrontendApp` in `crates/jig/src/bootstrap.rs` must expose a `role: String` serialized in answers. A shared default returns `spa`; validation accepts exactly `spa`, `admin`, or `astro`. `FrontendAppConfig` in `crates/jig/src/context.rs` must deserialize the same field with the same default and expose it through `jig info`.

`FrontendScaffold` preserves its `ScaffoldFrontendKind` directly. `from_frontend_app` maps the explicit role to that enum. Template selection uses separate SPA and Astro inventories plus an admin inventory derived from the embedded `admin-shadcn` template prefix. Admin provenance constants have one authoritative definition used by report rendering and tests: CLI `4.13.0`, preset `nova`, base `radix`, style `radix-nova`, and Tailwind major `4`.

The frontend API client must export an `AppVersion` type, an `AppStatus` type, a runtime response validator, and an abort-aware `fetchAppStatus(signal?: AbortSignal): Promise<AppStatus>`. The generated UI must not trust `response.json()` without checking that `name` and `version` are strings. Readiness is true only for an HTTP-success response from `/health/ready`.

The root workspace renderer must receive repository name, selected package manager and pinned package-manager spec, and all frontend directories. It emits `package.json`, `.node-version`, and conditional pnpm workspace metadata. The bootstrap command performs optional `cargo fetch` followed by exactly one root package-manager install when frontends exist.

Plan revision note (2026-07-15 20:01Z): replaced the initial one-line work body with a self-contained implementation and verification plan after inspecting the current scaffold and producing a successful shadcn 4.13.0 reference project.

Plan revision note (2026-07-15 20:40Z): recorded completed implementation, integration evidence, discovered npm lifecycle and UI issues, their fixes, and the final structured gate outcome.
