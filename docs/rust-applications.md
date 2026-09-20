# Rust applications on Batter

The current `rust-react` preset uses [Batter](https://github.com/bpcakes/batter)
for every new Rust application, including database-free and admin variants:

```sh
jig init ./example-app --defaults
```

There is no separate Batter preset or non-Batter application mode. This is an
unreleased change after Jig 0.3.0; use the current source to generate this layout.
Standalone Rust library and CLI presets are not services and do not acquire a
Batter runtime. This cutover does not mean Jig's own runtime uses Batter throughout.

## Requirements and dependencies

Generated applications require Rust 1.94 on Linux or macOS. Database variants use
SQLx 0.9. Batter is unpublished: the generated workspace pins the `batter` facade
to one Git revision and enables its `axum` feature; PostgreSQL variants also enable
the facade's `sqlx` feature. The facade keeps the established `batter::...`
foundation paths and exposes adapter namespaces such as `batter::axum` and
`batter::sqlx`. The first uncached build needs network access. Commit the generated
`Cargo.lock` after dependency bootstrap.

Application names that normalize to `batter`, `batter-core`, or `batter-axum`
collide with packages selected by the required facade and are rejected before
destination files are written. PostgreSQL Rust-react applications also reject
names normalizing to `batter-sqlx`; database-free application shapes do not reserve
that name. Rust library and CLI presets remain independent. Choose
another `--repo-name` when the selected shape includes a conflicting package.

## Ownership in the generated workspace

Paths below use `example-app` as the normalized package name.

| Location | Responsibility |
| --- | --- |
| `apps/example-app-api` | Load environment, initialize diagnostics, parse configuration, enter the runtime |
| `crates/example-app-runtime` | Startup, listeners, signals, owned tasks, native resource cleanup, finite database setup |
| `crates/example-app` | Typed `AppConfig`, application state, and use cases; no Batter supervision in state |
| `crates/example-app-http` | Public routes, probes, and OpenAPI assembly |
| `crates/example-app-http-common` | Typed request correlation, tracing, deadlines, admission, and common errors |
| `crates/example-app-db` (optional) | SQLx pool and migrations |
| `apps/example-app-admin-api` and `crates/example-app-admin-http` (optional) | Separate privileged listener and authorization boundary |

Production entrypoints use `router_with_lifecycle` with read-only lifecycle status
and readiness-gated operation admission captured from the runtime supervisor. Root
shutdown control stays in the runtime. The convenience `router` creates and consumes
an independent one-shot readiness approval for in-process tests; it does not connect
a server to the production shutdown lifecycle. In-memory `TestApp` tests do not prove
signal handling or native resource cleanup.

## Lifecycle and HTTP policy

Service startup uses Batter's protected `Startup::scoped` boundary, with library-owned
Unix signal listeners and constrained HTTP registration. It has one 30-second budget
covering resource initialization, database connection and migrations when enabled,
and listener binding. Cleanup capacity is reserved before connecting a database, and
pool closure is owned before later fallible startup work. Initialization failure or
cancellation therefore retains the same cleanup owner; it does not make native
resource acquisition asynchronous RAII.

Business requests pass through Batter admission and a validated 10-second
response-construction budget. The generated HTTP boundary retains that witness
before assembling the infallible request policy. The deadline ends at response
construction; it does not bound streaming bodies or WebSocket sessions.
Middleware failures retain the API error envelope with `code`, `message`, and
`request_id`. Batter's outer operational middleware replaces any inbound
`x-request-id`, installs a typed server-generated correlation ID for inner handlers
and failure renderers, and writes the same ID to the response. Client headers are
never the identity witness for error bodies or telemetry.

Generated services install the ordinary `operational_http` observer, which
allocates no quota record, so HTTP completion events carry no `quota_outcome` or
`quota_consumption` fields. Batter's admission boundary also installs an opaque
interruption responder for a nested adapter to reuse; the generated boundary
nests none and keeps rendering cancellation and deadline failures through its own
request policy renderer.

Both public and admin listeners expose `/health/live` and `/health/ready` outside
request admission. Admin probes are also outside authorization. Readiness requires
both Batter's ready lifecycle state and initialized application state; a configured
pool is not a continuous database connectivity check. PostgreSQL pool construction
is lazy but performs an explicit bounded connectivity check before readiness and
migrations. Keep probes reachable during draining without admitting new business work.

Admin business routes under `/admin-api` remain fail-closed with
`DenyAllAdminAuthorizer`. Supply the real authorizer through the production
`router_with_lifecycle` wiring and keep the admin listener private at the network
layer. Probe reachability does not grant access to privileged operations. Batter's
browser credential transport primitives do not supply an account, session,
authorization, CORS, or CSRF model; adopting them remains application-owned work.

Batter's optional `batter-runlimit` quota adapter is not a generated dependency.
Its protected quota execution and authenticated HTTP assembly require
application-owned policies, an authentication closure, native opaque subject keys,
and a Runlimit memory or PostgreSQL backend, so per-subject rate limiting stays
project-owned work rather than starter policy. Adopt the quota-aware observation
middleware together with that adapter; generated code installs neither.

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

## Database setup

`scripts/jig bootstrap` installs dependencies; it does not create application
databases or require database credentials. For database-enabled projects, explicit
`bash scripts/setup-database.sh` invokes the API's `--bootstrap-database` path.
The PostgreSQL role must already exist; setup does not grant privileges.

Database bootstrap uses Batter's finite `Command` owner with a 30-second work
budget and independently bounded cleanup, rather than an empty service supervisor.
PostgreSQL setup uses the same cleanup-slot-aware pool owner and bounded connectivity
probe before migrations. SIGINT or SIGTERM cancels the command and waits for cleanup. Normal server
startup also applies pending migrations but does not create the database. Cancellation
does not roll back already committed migrations, database creation, remote
transactions, or other remote effects; local pool closure is not a remote rollback
or continuous connectivity guarantee.

## Upgrades and existing applications

Upgrade the Batter Git revision and enabled facade features together in the
generated `Cargo.toml`, regenerate and commit the lockfile, and validate startup,
request admission, shutdown, and database setup for the project's enabled shape.
For Jig maintainers, new scaffold pins live in
`crates/jig/src/bootstrap/scaffold/rust_workspace.rs`.

Generated application source is project-owned. Neither `jig update` nor adoption
automatically converts an existing non-Batter application. Generate a disposable
reference application with matching database/frontend options, then port runtime
ownership and HTTP lifecycle boundaries to the existing architecture. Do not assume
the old and new file trees correspond one-to-one or overwrite the existing app with
`init --force`. Preserve deployed API contracts and persisted data boundaries while
changing internal ownership.

See [Developer UX](developer-ux.md) for preset options and bootstrap commands, and
[Configuration](configuration.md) for harness settings.
