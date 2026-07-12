# jig-ui crate guide

## Purpose

`crates/jig-ui` contains the read-only loopback HTTP server, routes, query model, and server-rendered HTML used by `jig ui`. The owning `jig-sh` CLI supplies dashboard and plan snapshots through the `SnapshotProvider` boundary.

## Key entrypoints

- `src/lib.rs`: snapshot-provider contract, query types, and public server API.
- `src/server.rs`: loopback listener, HTTP parsing, routing, and response security headers.
- `src/html.rs`: shared HTML shell, escaping, formatting, and gate rendering.
- `src/html/dashboard.rs`: flight-recorder dashboard rendering.
- `src/html/plan.rs`: plan detail rendering.

## Edit here for X

- Change HTTP routes, listener behavior, or response headers: `src/server.rs`.
- Change timeline query parsing: `src/lib.rs`.
- Change dashboard or plan presentation: `src/html/`.
- Change how repository state, gates, or loops become snapshots: `crates/jig/src/ui/snapshot.rs`, not this crate.

## Invariants

- Keep this crate independent from `jig-sh` repository context, state storage, runtime policy, MCP, and templates.
- Consume repository data only through `SnapshotProvider`; do not read `.agent/state` directly.
- Bind only to IPv4 loopback because snapshots can contain plan bodies and command output.
- Keep responses script-free under the strict content security policy unless the security model is explicitly redesigned.
- Escape all repository-controlled text before inserting it into HTML.
- Keep the server read-only and do not record receipts from requests.
- `jig-ui` is a CLI-owned internal crate; its public API is versioned with the matching `jig-sh` release rather than as a stable third-party integration surface.

## Common commands

- `cargo test -p jig-ui`
- `cargo test -p jig-sh ui`
- `cargo test --workspace`
