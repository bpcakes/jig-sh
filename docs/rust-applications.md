# Rust applications on Batter

The current `rust-react` preset uses [Batter](https://github.com/bpcakes/batter)
for every new Rust application, including database-free and admin variants:

```sh
jig init ./example-app --defaults
jig init ./example-app --preset rust-react --db postgres --frontends web,admin \
  --metrics otlp --jobs runledger
```

There is no separate Batter preset or non-Batter application mode. Standalone Rust
library and CLI presets are not services and do not acquire a Batter runtime. This
does not mean Jig's own runtime uses Batter throughout.

## Requirements and dependencies

Generated applications require Rust 1.94 on Linux or macOS. Database variants use
SQLx 0.9 and PostgreSQL 18, the minimum server for Batter's SQLx adapter. Batter is
unpublished: the generated workspace pins the `batter` facade to one Git revision
and enables only the features the shape uses: `axum` always, `sqlx` for PostgreSQL,
`otlp` with `--metrics otlp`, and `runledger` with `--jobs runledger`. The facade
keeps the established `batter::...` foundation paths and exposes adapter namespaces
such as `batter::axum`, `batter::sqlx`, `batter::otlp`, and `batter::runledger`.
PostgreSQL variants also pin `postgres-test-harness` to the exact revision that
Batter's `sqlx-test-support` feature selects, because Batter's fixture suite takes
the harness by value. The first uncached build needs network access. Commit the
generated `Cargo.lock` after dependency bootstrap.

Application names whose generated packages would share a name with a Batter-owned
package in the selected graph are rejected before destination files are written:
`batter`, `batter-core`, and `batter-axum` always; `batter-sqlx`,
`batter-test-support`, and `postgres-test-harness` with PostgreSQL; `batter-otlp`
with metrics; and `batter-runledger`, `runledger-core`, `runledger-postgres`, and
`runledger-runtime` with jobs, including through generated suffixes such as
`runledger-core` for a repository named `runledger`. Shapes that do not select a
package do not reserve its name. Rust library and CLI presets remain independent.
Choose another `--repo-name` when the selected shape includes a conflicting package.

## Service options

`--metrics` and `--jobs` apply only to `rust-react` and default to `none`. The
interactive wizard offers them while it guides the project shape; explicit flag
invocations are not prompted. `--jobs runledger` requires `--db postgres`.

- `--metrics otlp` compiles Batter's bounded OTLP/HTTP exporter. Setting
  `METRICS_OTLP_ENDPOINT` to an HTTP(S) URL ending in `/v1/metrics` exports the
  foundation metrics catalog (operations, admission decisions, task exits,
  cleanup, and shutdown) every ten seconds, followed by one bounded final export
  after the service result and cleanup are retained. Without the setting nothing
  is installed and no collector is contacted. Ambient `OTEL_*` variables are
  rejected while export is enabled because the native builders would merge them.
  Export diagnostics are logged and never change the exit code.
- `--jobs runledger` adds `crates/<repo>-jobs` with an example handler, the job
  catalog, worker settings, and an atomic enqueue helper. The public API process
  registers inert native preparation through `batter::runledger::register_in`, so
  startup waits for native initialization, drain stops claiming work, and the pool
  closes only after the worker settles. The admin API does not run a worker. The
  pool becomes a profiled `RunledgerDatabase`. Runledger's migration history is
  applied before the application's in the shared `_sqlx_migrations` table, and the
  application migrator ignores Runledger's versions. Set `JOBS_WORKER_ID` for a
  stable worker identity; otherwise each process derives a unique one.

## Ownership in the generated workspace

Paths below use `example-app` as the normalized package name.

| Location | Responsibility |
| --- | --- |
| `apps/example-app-api` | Load environment, initialize diagnostics, parse configuration, enter the runtime |
| `crates/example-app-runtime` | Protected startup, listeners, signals, readiness inputs, native resource cleanup, finite database setup, optional metrics export |
| `crates/example-app` | Typed `AppConfig` from `batter::settings`, application state, and use cases; no Batter supervision in state |
| `crates/example-app-http` | Public routes and OpenAPI document assembled through `HttpBoundary` |
| `crates/example-app-http-common` | Error envelope, request policy, rendered probes, and OpenAPI route inventories |
| `crates/example-app-db` (optional) | SQLx pool construction, migrations, and the database health monitor |
| `crates/example-app-jobs` (optional) | Runledger handlers, catalog, worker settings, and enqueue helpers |
| `apps/example-app-admin-api` and `crates/example-app-admin-http` (optional) | Separate privileged listener, browser policy, and authorization boundary |

Production entrypoints call `assemble` with the operation admission and readiness
policy captured from the runtime supervisor; root shutdown control stays in the
runtime. Each surface's `in_process` client dispatches through the same sealed
boundary with an independent, approved lifecycle. It proves response construction
only: async `TestApp` tests do not prove serving, signal handling, or native
resource cleanup.

## Lifecycle and HTTP policy

Service startup uses Batter's protected `Startup::scoped` boundary, with
library-owned Unix signal listeners, inside `batter::service::start`, which owns
startup, shutdown, and optional diagnostics until completion. Completion is checked:
a startup failure, an unsuccessful shutdown report, or a coordinator failure is
an error. Startup has one 30-second budget covering resource initialization,
database connection and migrations when enabled, worker registration, HTTP
assembly, and listener binding. Cleanup capacity is reserved before connecting a
database, and pool closure is owned before later fallible startup work.
Initialization failure or cancellation therefore retains the same cleanup owner;
it does not make native resource acquisition asynchronous RAII.

Each HTTP surface assembles through Batter's sealed `HttpBoundary`: server
correlation and one HTTP observer are outermost, probes sit outside admission, and
lifecycle admission plus a validated 10-second response-construction budget wrap
every route and the fallback. The order is library-owned; generated code uses no
`batter::axum::low_level` helper. OpenAPI routers join through
`GuardedRouter::from_router` with a `RouteInventory` derived from the same OpenAPI
document, so the boundary serves exactly the documented operations, and a test
checks that documented paths equal served routes plus probes. The deadline ends at
response construction; it does not bound streaming bodies or WebSocket sessions.
Handlers read the request's operation context and server-generated correlation ID
from the `AdmittedRequest` extractor. Batter replaces any inbound `x-request-id`,
and admission, deadline, and fallback failures keep the API error envelope with
`code`, `message`, and `request_id`. Client headers are never the identity witness
for error bodies or telemetry.

Both public and admin listeners expose `/health/live` and `/health/ready` outside
request admission, rendered by the boundary; the public surface also answers
`/health`. Renderers keep the documented contract (`ok`, an empty ready response,
or the error envelope) while the boundary owns status and completion severity.
Readiness combines Batter's lifecycle state, a supervised PostgreSQL health
monitor when a database is configured, and the application's `application-state`
readiness condition. The monitor probes the pool every five seconds independently
of traffic, so a database outage turns readiness unready within about ten seconds
and recovery restores it without a restart; liveness is unaffected. Keep probes
reachable during draining without admitting new business work.

Admin business routes under `/admin-api` remain fail-closed with
`DenyAllAdminAuthorizer`. Supply the real authorizer through the admin binary's
`assemble` call and keep the admin listener private at the network layer. The
admin route group and its probes carry `Cache-Control: no-store`,
`X-Content-Type-Options: nosniff`, and `Referrer-Policy: no-referrer` on every
response, including admission, deadline, method, and mutation rejections. Mutating
admin requests must carry `Sec-Fetch-Site: same-origin` and the `x-admin-request`
marker, which the generated admin panel sends, before authorization runs; this is
the CSRF defense if the authorizer accepts browser cookies. Probe reachability does
not grant access to privileged operations, and Batter's browser primitives do not
supply an account, session, authorization, or CORS model.

Batter's `batter-runlimit` quota adapter and `batter-at-rest` encryption are not
generated dependencies. Quotas require application-owned policies, an
authentication closure, and native opaque subject keys, and replace the boundary
with Runlimit's own protected assembly; at-rest encryption requires application
key management. Both stay project-owned work rather than starter policy.

Shutdown allows 10 seconds for drain, 2 seconds for cancellation, and 1 second for
abort/reap. These phases share the first-stop clock; scheduling delays consume
the budget. Cleanup has its own bounded allowance. These are application-owned
source policies in the runtime and HTTP-common crates, not `.jig.toml` switches
or generated environment settings.

The `jig dev` proxy gives a service process group only two seconds after SIGTERM
before force-killing it, so development shutdown can preempt the longer service
drain and cleanup. Test full graceful shutdown with the built binary directly.
Production process-manager grace must cover all shutdown and cleanup phases.
Incomplete shutdown or cleanup is reported as failure when the runtime can finish;
SIGKILL cannot provide that guarantee.

## Configuration

`AppConfig` captures the environment once, after `.env` loading, into
`batter::settings::SettingsSource` and parses it into typed values.
`DATABASE_URL` is a `SecretString`, so `Debug` output cannot leak it, and settings
errors name the variable without echoing its value. Tests construct configuration
from explicit sources instead of mutating the process environment.

## Database setup and tests

`scripts/jig bootstrap` installs dependencies; it does not create application
databases or require database credentials. For database-enabled projects, explicit
`bash scripts/setup-database.sh` invokes the API's `--bootstrap-database` path.
The PostgreSQL role must already exist; setup does not grant privileges.

Database bootstrap uses Batter's finite `Command` owner with a 30-second work
budget and independently bounded cleanup, rather than an empty service supervisor.
PostgreSQL setup uses the same cleanup-slot-aware pool owner and bounded
connectivity probe before migrations. Migrations run on one `PgLease`, which is
retired on failure or cancellation instead of returning a connection that may hold
a migration lock. SIGINT or SIGTERM cancels the command and waits for cleanup.
Normal server startup also applies pending migrations but does not create the
database. Cancellation does not roll back already committed migrations, database
creation, remote transactions, or other remote effects; local pool closure is not
a remote rollback.

`scripts/test-postgres.sh` starts a disposable PostgreSQL 18 container, or uses
`POSTGRES_TEST_ADMIN_URL` when it is set, and runs the ignored PostgreSQL tests.
Batter's fixture suite migrates one template database per migration fingerprint,
including Runledger's history with jobs, gives each test its own clone, and drops
it after the test even when the test fails. The generated tests cover readiness
through the public boundary, the health monitor under supervision, and, with jobs,
an example job executed by the supervised worker. They never use `DATABASE_URL`.

## Upgrades and existing applications

Upgrade the Batter Git revision and enabled facade features together in the
generated `Cargo.toml`, together with the `postgres-test-harness` revision that
Batter's SQLx fixture support selects, regenerate and commit the lockfile, and
validate startup, request admission, shutdown, and database setup for the
project's enabled shape. For Jig maintainers, new scaffold pins live in
`crates/jig-bootstrap/src/scaffold/rust_workspace.rs`.

Generated application source is project-owned. Neither `jig update` nor adoption
automatically converts an existing application. Generate a disposable reference
application with matching database, frontend, metrics, and jobs options, then port
runtime ownership and HTTP lifecycle boundaries to the existing architecture. Do
not assume the old and new file trees correspond one-to-one or overwrite the
existing app with `init --force`. Preserve deployed API contracts and persisted
data boundaries while changing internal ownership.

See [Developer UX](developer-ux.md) for preset options and bootstrap commands, and
[Configuration](configuration.md) for harness settings.
