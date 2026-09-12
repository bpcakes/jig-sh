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
SQLx 0.9. Batter is unpublished: the generated workspace pins `batter` and
`batter-axum` to the same Git revision, so the first uncached build needs network
access. Commit the generated `Cargo.lock` after dependency bootstrap.

Application names that normalize to `batter` or `batter-axum` collide with runtime
packages and are rejected before destination files are written. Choose another
`--repo-name`; this restriction does not apply to standalone library or CLI presets.

## Ownership in the generated workspace

Paths below use `example-app` as the normalized package name.

| Location | Responsibility |
| --- | --- |
| `apps/example-app-api` | Load environment, initialize diagnostics, parse configuration, enter the runtime |
| `crates/example-app-runtime` | Startup, listeners, signals, owned tasks, native resource cleanup, finite database setup |
| `crates/example-app` | Typed `AppConfig`, application state, and use cases; no Batter supervision in state |
| `crates/example-app-http` | Public routes, probes, and OpenAPI assembly |
| `crates/example-app-http-common` | Request IDs, tracing, deadlines, admission, and common errors |
| `crates/example-app-db` (optional) | SQLx pool and migrations |
| `apps/example-app-admin-api` and `crates/example-app-admin-http` (optional) | Separate privileged listener and authorization boundary |

Production entrypoints use `router_with_shutdown` with the runtime's handle.
The convenience `router` creates an independent ready handle for in-process tests;
it does not connect a server to the production shutdown lifecycle. In-memory
`TestApp` tests do not prove signal handling or native resource cleanup.

## Lifecycle and HTTP policy

Service startup has one 30-second budget covering resource initialization,
database connection and migrations when enabled, and listener binding. Cleanup
capacity is reserved before connecting a database, and pool closure is registered
before later fallible startup work.

Business requests pass through Batter admission and a 10-second request deadline.
The deadline ends at response construction; it does not bound streaming bodies or
WebSocket sessions. Middleware failures retain the API error envelope with
`code`, `message`, and `request_id`.

Both public and admin listeners expose `/health/live` and `/health/ready` outside
request admission. Admin probes are also outside authorization. Readiness requires
both Batter's ready lifecycle state and initialized application state; a configured
pool is not a continuous database connectivity check. Keep probes reachable during
draining without admitting new business work.

Admin business routes under `/admin-api` remain fail-closed with
`DenyAllAdminAuthorizer`. Supply the real authorizer through the production
`router_with_shutdown` wiring and keep the admin listener private at the network
layer. Probe reachability does not grant access to privileged operations.

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
SIGINT or SIGTERM cancels the command and waits for cleanup. Normal server startup
also applies pending migrations but does not create the database. Cancellation
does not roll back already committed migrations, database creation, or remote effects.

## Upgrades and existing applications

Upgrade both Batter Git pins together in the generated `Cargo.toml`, regenerate
and commit the lockfile, and validate startup, request admission, shutdown, and
database setup for the project's enabled shape. For Jig maintainers, new scaffold
pins live in `crates/jig/src/bootstrap/scaffold/rust_workspace.rs`.

Generated application source is project-owned. Neither `jig update` nor adoption
automatically converts an existing non-Batter application. Generate a disposable
reference application with matching database/frontend options, then port runtime
ownership and HTTP lifecycle boundaries to the existing architecture. Do not assume
the old and new file trees correspond one-to-one or overwrite the existing app with
`init --force`. Preserve deployed API contracts and persisted data boundaries while
changing internal ownership.

See [Developer UX](developer-ux.md) for preset options and bootstrap commands, and
[Configuration](configuration.md) for harness settings.
