# Upgrade generated Rust applications to the latest Batter pin

New `rust-react` applications pin Batter `bd836a29c9d484b96ee1ce0af0af84d58d3df1ee`
(2026-09-20). Upstream master is `18cdf97ac544c665e0189efd28388d1e10456232`
(2026-10-08), 367 commits later. The pin bump alone breaks generated code: the
sealed HTTP assembly hard cut moved `operational_http`, `request_admission`,
`liveness`, `readiness` and `register_http_in` to `batter::axum::low_level`, and
`OperationContext::new` became `OperationOwner::new(..)?.into_context()`.

The operator asked for the generated project to use every applicable Batter
capability, with OTLP metrics and a Runledger job worker as generation-time
options and the PostgreSQL fixture harness always on for PostgreSQL. Runlimit
quotas and at-rest encryption stay out: both need application-owned
authentication or key policy that a starter cannot supply.

## Progress

- [x] Jig plumbing: `--metrics none|otlp`, `--jobs none|runledger` (rust-react
  only; jobs requires `--db postgres`), wizard prompts, validation, scaffold
  report, preset descriptor, template context and conditional file lists.
- [x] Canonical template migration for every shape.
- [x] PostgreSQL fixture harness, health monitor and lease-owned migrations.
- [x] OTLP metrics option.
- [x] Runledger job worker option.
- [x] Package-collision guard for every Batter-owned package the shape selects.
- [x] Jig tests, embedded snapshots, docs and changelog.
- [x] Generated-variant validation, including live PostgreSQL 18 and signals.

## Decision log

- Pin `18cdf97ac544c665e0189efd28388d1e10456232`; `postgres-test-harness` pins
  `3d525e6fc5745ce2e2437c7997de5cccdecff4ac`, the exact revision Batter's
  `sqlx-test-support` feature selects, so both resolve to one package.
- HTTP surfaces assemble through `HttpBoundary`. OpenAPI routers join through
  `GuardedRouter::from_router` with a `RouteInventory` derived from the same
  OpenAPI document, so documentation and served routes cannot drift.
- Probes use the boundary's rendered liveness/readiness and keep the existing
  OpenAPI contract (`ok` text, empty 200 or the API error envelope on 503), so
  committed TypeScript clients do not change.
- Readiness combines lifecycle, a supervised PostgreSQL `HealthMonitor` when a
  database is configured, and an `application-state` readiness condition.
- The admin surface selects `PrivateResponsePolicy::NoReferrer` for its route
  group and probes and enables Batter's custom-marker mutation checks, so a
  future browser-session authorizer is CSRF-protected by default.
- Service roots use `batter::service::start`; diagnostics are `NoDiagnostics`
  or the OTLP exporter. Completion is checked; diagnostics never change exit.
- Configuration is captured once into `batter::settings::SettingsSource`;
  `DATABASE_URL` is a `SecretString`, so `AppConfig` Debug cannot leak it.
- The Runledger worker runs in the public API process only. Runledger history
  is applied first; the application migration plan then recognizes both bundled
  histories, preserving SQLx's missing-version and checksum checks.

## Validation

Render no-DB, PostgreSQL, PostgreSQL+admin, and PostgreSQL+metrics+jobs+admin
fixtures with generic names. For each: `cargo fmt --check`, strict Clippy with
all features, and `cargo test --workspace`. Exercise built binaries directly for
SIGINT/SIGTERM and probe responses, and run the PostgreSQL fixture tests against
a disposable PostgreSQL 18 container.

## Outcomes

Generated variants (`example-app`, `example-pg` with admin, `example-metrics`,
`example-full` with PostgreSQL, admin, metrics, and jobs) pass `cargo fmt --check`,
strict Clippy with all features, and `cargo test --workspace` (21, 30, 31, and 35
tests). `scripts/test-postgres.sh` passes against PostgreSQL 18 containers: readiness
through the public boundary, the supervised health monitor, and an example job run
by the supervised Runledger worker. Built binaries were driven directly:
probes keep their contract, readiness turns 503 about seven seconds after the
database is paused and recovers after it resumes, admin responses carry private
headers and reject cross-site or unmarked mutations before authorization, SIGINT
and SIGTERM exit 0, an unreachable database fails startup with pool cleanup
observed, OTLP export reached a fake collector with an acknowledged final export,
and ambient `OTEL_*` settings are rejected. Admin panel lint, typecheck, and tests
and the generated contract and public-artifact checks pass. Projects named `app`,
`db`, and `http` compile.

Jig's `jig-bootstrap` and `jig-sh` suites, `api:fmt`, `api:clippy`, `repo:contract`,
and `repo:file-budget` pass. The generated embedded-template manifests are excluded
from file budgets as generated code. A pre-existing race between the late-init
git-failure test and the `batter-sqlx` collision test is closed by holding the
environment lock in the collision test.
