# jig-dev-proxy crate guide

## Purpose

`crates/jig-dev-proxy` contains the local development reverse proxy used by the Jig CLI. It owns route storage, hostnames, HTTP/HTTPS proxying, TLS certificates, process supervision, service file generation, LAN mode helpers, and workspace package discovery.

## Key entrypoints

- `src/lib.rs`: public API consumed by `crates/jig`.
- `src/types.rs`: request, route, app, proxy, certificate, and service types.
- `src/state.rs`: machine-local mutable proxy state under `~/.jig/proxy` or `JIG_PROXY_STATE_DIR`.
- `src/dev_sessions.rs`: repo-scoped foreground dev-session registration, lifecycle management, and replacement claims.
- `src/session_control.rs`: authenticated loopback control channel used by `dev stop` and `dev --replace`.
- `src/host.rs`: repo/app hostname normalization and validation.
- `src/ports.rs`: local port probing and LAN address detection.
- `src/server.rs`: HTTP and HTTPS proxy listeners.
- `src/processes.rs`: app command spawning, port env injection, and route cleanup.
- `src/certs.rs`: local CA and leaf certificate generation and trust helpers.
- `src/service.rs`: user service install/uninstall/status helpers.
- `src/workspace.rs`: JavaScript workspace package discovery.

## Edit here for X

- Change hostname or route behavior: `src/host.rs`, `src/state.rs`, and `src/server.rs`.
- Change HTTP, HTTPS, HTTP/2, or WebSocket forwarding: `src/server.rs`.
- Change app command launching or framework flag injection: `src/processes.rs`.
- Change dev status, stop, replacement, or registry behavior: `src/dev_sessions.rs`, `src/dev_sessions/`, `src/session_control.rs`, and `src/state/dev_sessions.rs`.
- Change local certificate behavior: `src/certs.rs`.
- Change launchd or systemd service files: `src/service.rs`.
- Change workspace discovery: `src/workspace.rs`.

## Invariants

- Keep this crate independent from Jig repository state, receipts, MCP, and templates.
- Store mutable proxy state outside `.agent/state`; that directory is append-only work memory.
- Route mutations must be lock-protected and safe to repeat.
- Dev-session mutations share the route-state lock so a replacement claim and route ownership decision are atomic. Keep `dev-sessions.json` versioned, owner-only, symlink-hardened, bounded, and fail-closed on malformed or unknown state.
- Only the live foreground supervisor may terminate its registered app trees. Management reaches it through the authenticated loopback control channel; persisted PID and start-token observations are never external signaling authority. Persist spawn-pending state before creating each app process and clear it only when the exact process identity is durable. When the supervisor is gone, retain `cleanup_required` records while spawn state is pending or unknown, or while any exact registered identity remains live or cannot be classified safely. Once spawn state is known and every registered identity is absent, explicit `dev stop` or `dev --replace` may retire the orphan and its exact-owned process routes under the shared state lock without signaling stored PIDs. The stop-only `--forget-ambiguous-orphans` repair may relax pending or legacy-untracked spawn evidence for a dead supervisor, but it must still refuse every live or uncertain registered identity, must never signal persisted PIDs, and must not be used by replacement.
- Certificate trust and service installation must be explicit commands; do not mutate user or system trust/service state from ordinary `dev` or `proxy run`.
- The local CA must stay name-constrained to configured Jig development DNS names plus loopback and detected IPv4 LAN addresses. The CA validity is intentionally longer than leaf certificates but bounded for local development, so treat the CA key as machine-local sensitive material, document the local trust scope, and keep `cert untrust` available for compromise, uninstall, or forced regeneration flows.
- Platform trust helpers are invoked from fixed system tool directories rather than the invoking environment's `PATH`; service and certificate commands are explicit user operations, so document this trust boundary when changing those paths.
- The state directory is part of the local proxy TCB. Route files, health tokens, PID files, and private keys must stay owner-only and symlink-hardened because the server treats route state as authoritative.
- Background proxy processes must keep a constrained environment and not inherit the caller's repo root or `PATH`; Unix background starts detach from the caller's cwd and use a private umask. App child commands are different: they are trusted repo-configured development commands and intentionally inherit the invoking environment so package managers and dev credentials keep working. Derived `JIG_DEV_<APP>_{HOST,PORT,ORIGIN,URL}` coordinates are the exception: remove inherited copies and overlay only the current selection so stale topology cannot impersonate a managed app. The exact generated npm dev argv is another narrow exception: remove npm package-routing, missing-script, and dependency-class selectors so hostile ambient config cannot redirect it or synthesize production mode, but preserve explicit `NODE_ENV` and project-owned registry/auth/layout/peer/lifecycle policy. Do not apply that exception to custom npm commands.
- Do not require privileged ports in tests.
- App port assignment is a local bind probe followed by child process startup. Do not hold a listener across arbitrary child commands; publish process routes only after the child target port is observed listening and the listener owner is verified as part of the spawned child process group. The bind probe is not a reservation, so a local port race should surface as an app startup/readiness error rather than a published bad route.
- Listener ownership verification is a best-effort local race defense: keep the start-token checks before and after listener inspection, and do not weaken the final owner-set recheck before route publication.
- App process cleanup should terminate the process group where the platform supports it; package-manager wrappers often leave grandchildren behind when only the wrapper PID is signaled. Keep the direct Unix child unreaped until group cleanup is confirmed. While that exact child pins the PGID generation, re-send group `SIGKILL` before each forced membership proof so a concurrently exposed member cannot outlive a one-shot signal; never retry an unpinned numeric PID or a group after reap/identity loss. Carry one absolute SIGTERM-grace deadline across a running-to-exited leader transition and one absolute SIGKILL-confirmation deadline across every retry, leader exit, and group confirmation; cap every poll sleep to the remaining phase budget. `ESRCH` is inconclusive, and on macOS `EPERM` is not absence: accept quiescence only when a fresh terminal leader observation and an atomic group snapshot prove that exact pinned leader is the sole member.
- Process cleanup outside Linux and macOS remains unsupported. Do not publish process-owned routes on platforms without high-confidence process start-token support; use aliases or no-proxy mode there until platform-specific start tokens are implemented.
- Workspace discovery intentionally supports common package workspace globs and a small pnpm-workspace subset. `**` recursion skips `node_modules`, dot-directories, symlinked package directories, and canonical paths outside the repo root, with bounded glob depth and match counts; avoid broad patterns in large monorepos. Add a real YAML parser before expanding pnpm syntax beyond simple `packages` lists.
- Foreground Ctrl-C handling terminates registered child processes without taking route locks in the signal handler; process-route liveness pruning must keep interrupted sessions from serving stale routes. Route and certificate files must continue to use lock-protected atomic writes so interrupted cleanup cannot leave partial JSON or PEM files behind. Replacement uses atomic rename, and readers must continue to reject partial state.
- The in-process lock-order guard is thread-local by design; cross-thread route/cert ordering is enforced by advisory file locks and the documented cert-lock-before-route-lock discipline. Do not introduce a path that holds the route lock while trying to acquire the cert lock.
- The proxy is local development tooling. Keep the explicit connection limit in place. Listener tasks acquire a permit before `accept()`, so overload waits in the OS backlog or times out; slow TLS handshakes must remain bounded by `TLS_HANDSHAKE_TIMEOUT`.
- Backend HTTP forwarding intentionally normalizes requests to HTTP/1.1 and disables backend idle pooling to keep connection-permit accounting simple for local development workloads. WebSocket proxying is limited to HTTP/1.1 Upgrade; do not enable RFC 8441 HTTP/2 WebSockets without adding explicit routing and tunnel tests.
- Public dev-proxy command entry points are synchronous and may run blocking process, listener-owner, and filesystem operations. Call them from CLI/blocking contexts; async callers must put the boundary behind `spawn_blocking` or equivalent.
- The health endpoint is local management surface and returns the proxy PID after token validation. Successful health responses must require loopback client and local addresses even when LAN mode binds the main listener broadly; do not expose the PID through LAN routing.
- LAN mode exposes Jig-owned process routes because Jig starts and supervises them on loopback IP literals. Alias routes remain loopback-client only, even when they target loopback, because Jig does not own or supervise those processes. Newly written proxy routes should use IP literal targets; hostname targets in legacy/manual state are not served as routable proxy targets.
- LAN IP discovery uses a UDP route probe without sending application data. The proxy captures the detected IPv4 LAN address at startup for address reporting and self-loop detection; a changed LAN IP may require leaf certificate regeneration or proxy restart before browsers trust or route through the LAN address again.
- The proxy executable path is canonicalized for process reuse. Upgrades that replace the binary require restarting the long-running proxy.
- `crates/jig-dev-proxy` is a CLI-owned internal crate even though it is split out for testability; `anyhow::Result<serde_json::Value>` public functions are acceptable at this boundary.

## Common commands

- `cargo test -p jig-dev-proxy`
- `cargo test -p jig-dev-proxy state`
- `cargo test -p jig-dev-proxy processes`
- `cargo test -p jig-sh` when CLI integration or feature-gated fallback behavior changes
