# Implement Jig Local Dev Proxy v2

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds. This document follows `.agent/PLANS.md` and is intended to be sufficient for a contributor with only the current working tree.

## Purpose / Big Picture

Jig users often work on several local repositories at once. Fixed development ports like 3000, 5173, 8000, and 8080 collide, and every collision interrupts the work loop. This change adds a runtime-owned local development proxy to Jig so a user can run local apps through stable hostnames such as `web.my-repo.localhost` or `api.my-repo.localhost` while Jig assigns free backend ports automatically.

After this change, a user can run `scripts/jig dev` to start configured Rust backends and Vite-style frontends without choosing ports manually. They can also run `scripts/jig proxy run web -- npm run dev`, inspect routes with `scripts/jig proxy list`, use HTTPS locally after generating and trusting a local certificate authority, expose apps on the LAN when requested, and install a user service for the proxy. The proxy must support HTTP/1.1, HTTP/2 over TLS, WebSocket upgrades for HTTP/1.1 clients, `.jig.toml` multi-app config, package workspace discovery, and clear handling for privileged ports 80 and 443.

## Progress

- [x] (2026-05-13T17:34Z) Created structured work plan `plan_01KRH6C77BFYG4KEFG8TE5SZS0`.
- [x] (2026-05-13T17:34Z) Inspected current repository guidance, `.jig.toml` runtime shape, templates, CLI dispatch, and existing unrelated working-tree changes.
- [x] (2026-05-13T17:34Z) Wrote this comprehensive implementation plan.
- [x] (2026-05-13T17:38Z) Decided to implement proxy functionality as a separate workspace crate rather than a large internal `crates/jig` module.
- [x] Added dependencies and a `crates/jig-dev-proxy/` library crate with its own `AGENTS.md`.
- [x] Added `.jig.toml` dev proxy config parsing and workspace discovery.
- [x] Added CLI commands for `jig dev`, `jig proxy`, certificate trust, and service install.
- [x] Implemented HTTP, HTTPS, HTTP/2, WebSocket, route store, app process supervision, LAN mode, and privileged port checks.
- [x] Updated templates and docs.
- [x] Ran smoke tests, `make test`, and comprehensive review.
- [x] Fixed review findings and repeated validation.

## Surprises & Discoveries

- Observation: The current working tree has already moved repo config from `.jig.yml` to `.jig.toml`, and `crates/jig/src/context.rs` parses it with the `toml` crate.
  Evidence: `.jig.toml` exists at the repo root and `RepoContext::load` reads `root.join(".jig.toml")`.

- Observation: The current working tree has unrelated changes to CLI, runtime, and state files, including `agent` and `work goal` commands.
  Evidence: `git status --short` showed modified `crates/jig/src/cli.rs`, `crates/jig/src/runtime.rs`, `crates/jig/src/runtime/work.rs`, and related tests before proxy work began.

- Observation: HTTP/2 client requests do not always carry an HTTP/1.1 `Host` header and cannot be forwarded to HTTP/1 backends without normalizing the request version.
  Evidence: The first HTTPS/HTTP2 smoke returned `Bad gateway: Missing Host header`, then `client error (UserUnsupportedVersion)` until route lookup accepted `:authority` and forwarded backend requests as HTTP/1.1.

- Observation: TLS verification does not accept `*.localhost` for top-level aliases such as `smoke.localhost`, even when the SAN is present.
  Evidence: Curl rejected `https://smoke.localhost` with `no alternative certificate subject name matches`; switching CLI aliases to repo-scoped `smoke.jig-sh.localhost` uses the repo wildcard certificate and passed the HTTPS alias smoke.

- Observation: `scripts/install-jig.sh` can race when a long-running dev proxy and a second command both try to refresh the cached binary from `JIG_DEV_BIN`.
  Evidence: Earlier smoke tests exposed concurrent writes to `.git/jig-tools/.../bin/jig`; the dev-bin branch now validates and returns the dev binary path directly.

- Observation: A comprehensive review identified several correctness issues after initial smoke tests, including stale PID handling, non-Jig port false positives, duplicate app ports/hostnames, workspace discovery escaping the repo root, and background proxy process lifetime.
  Evidence: Claude Code review plus the native Codex pass produced actionable findings that were fixed before the final validation loop.

## Decision Log

- Decision: Build the proxy as runtime-owned Jig functionality rather than a Makefile-backed contract tool.
  Rationale: Proxy routes, certificates, services, and app processes are local runtime concerns. They should not be declared in `.agent/jig-contract.json`, which is reserved for make-backed repository checks and actions.
  Date/Author: 2026-05-13 / Codex

- Decision: Store mutable proxy state outside `.agent/state`.
  Rationale: `.agent/state/*.jsonl` is append-only repository memory. Proxy routes, PID files, certificates, and service files are mutable machine-local runtime state and must not pollute append-only work receipts.
  Date/Author: 2026-05-13 / Codex

- Decision: Add a `[dev]` table in `.jig.toml` for proxy settings and `[[dev.apps]]` entries for multi-app dev, while preserving the existing top-level `dev_command`.
  Rationale: Existing repositories already have `dev_command` behind `make dev`. The new table gives Jig-native process supervision without breaking `make dev` or existing generated repos.
  Date/Author: 2026-05-13 / Codex

- Decision: Put proxy functionality in a separate workspace crate named `jig-dev-proxy`, with `crates/jig` acting as a thin CLI and configuration adapter.
  Rationale: The v2 scope includes route storage, HTTP proxying, HTTPS and HTTP/2, certificate generation and trust, service install, LAN mode, workspace discovery, and process supervision. Those are cohesive local-dev proxy concerns and should be testable without the broader Jig CLI, MCP, receipt, and template runtime.
  Date/Author: 2026-05-13 / Codex

- Decision: Implement certificate generation and trust commands, but make trust an explicit user command.
  Rationale: Trusting a local certificate authority mutates user or system trust stores. It is valid v2 functionality, but it must not happen implicitly during tests or ordinary `jig dev`.
  Date/Author: 2026-05-13 / Codex

## Outcomes & Retrospective

Implemented the v2 feature as a separate `jig-dev-proxy` crate with a thin `crates/jig` adapter. The crate owns route state, HTTP/HTTPS forwarding, HTTP/2 over TLS, WebSocket upgrade forwarding for HTTP/1.1, certificate generation and explicit trust/untrust commands, service file generation, workspace package discovery, process supervision, Ctrl-C route cleanup, LAN bind mode, and privileged-port checks.

Validation covered unit tests, workspace tests, clippy, dogfooded `scripts/jig` HTTP/HTTPS/WebSocket smokes, certificate and service status commands, privileged-port refusal, and structured work gates. Review findings were folded back into implementation: rustls provider selection is explicit, aliases are repo-scoped for TLS coverage, the dev-bin launcher path does not copy into the cache or swallow version mismatches, background proxy processes detach on Unix, stale PID stop is guarded by a proxy handshake, workspace discovery is bounded and supports recursive `**`, duplicate apps and ports are rejected, and non-proxied apps still run directly.

## Context and Orientation

The Jig CLI lives in `crates/jig`. The binary entrypoint is `crates/jig/src/main.rs`, which calls `jig::run()` from `crates/jig/src/lib.rs`. CLI command definitions are in `crates/jig/src/cli.rs`. Runtime command dispatch is in `crates/jig/src/runtime.rs`, with submodules under `crates/jig/src/runtime/`. Repository configuration is loaded by `crates/jig/src/context.rs` from `.jig.toml`, and templates for generated repositories are under `templates/project/`.

The term "reverse proxy" in this plan means a local HTTP server that accepts browser requests for hostnames like `web.jig-sh.localhost`, looks up which backend port owns that hostname, then forwards the request to `127.0.0.1:<backend_port>`. "Route" means one hostname-to-port mapping plus metadata such as the owning process ID. "Local certificate authority" means a generated certificate and private key stored in Jig's local state directory and used to sign local HTTPS certificates for development hostnames. "LAN mode" means the proxy binds to `0.0.0.0` and can advertise URLs using the machine's local network IP rather than only loopback.

The local state directory should be `~/.jig/proxy` by default, or `$JIG_PROXY_STATE_DIR` when set. It should contain `routes.json`, `routes.lock`, `proxy.pid`, `proxy-http.port`, `proxy-https.port`, `ca.pem`, `ca-key.pem`, `leaf.pem`, `leaf-key.pem`, and service files as needed. Tests should set `JIG_PROXY_STATE_DIR` to a temporary directory.

## Plan of Work

First add dependencies in `Cargo.toml` and create a new workspace library crate at `crates/jig-dev-proxy`. This crate owns async networking and TLS dependencies: `tokio`, `hyper`, `hyper-util`, and `http-body-util` for HTTP and HTTP/2, `tokio-rustls`, `rustls`, `rustls-pemfile`, and `rcgen` for TLS, `dirs` for user directories, structured argv handling plus conservative shell detection for dev commands, and `ctrlc` for cleaning up child processes on Ctrl-C. `crates/jig` should depend on `jig-dev-proxy`, but should not own those implementation details.

Create focused modules under `crates/jig-dev-proxy/src/`. `types.rs` defines route, proxy, app, certificate, service, and command data types. `state.rs` owns local state paths, route storage, file locking with `fs4`, PID and port files, and stale route pruning. `host.rs` validates and sanitizes hostnames. `ports.rs` finds free backend ports and validates privileged proxy ports. `certs.rs` generates the CA and leaf certificates and implements `trust`, `untrust`, and `status` for macOS and Linux where practical. `server.rs` runs the HTTP and HTTPS proxy listeners. `processes.rs` starts configured app commands, injects `PORT`, `HOST`, Vite-style flags when safe, and cleans routes when children exit. `workspace.rs` discovers package workspaces and turns packages with dev scripts into app configs. `service.rs` writes launchd or systemd user service files and reports status. `lib.rs` exposes a small API used by `crates/jig`.

Extend `crates/jig/src/context.rs` so `RepoConfig` includes existing top-level `dev_command`, `web_package_manager`, and `frontend_apps`, plus a new defaulted `dev: DevConfig`. `DevConfig` should include proxy ports, `https`, `http2`, `lan`, `tld`, `workspace_discovery`, and `apps`. Each `DevAppConfig` should include `name`, `dir`, `kind`, `command`, `argv`, `port`, `host`, and `proxy`. The parser must default missing dev config so existing `.jig.toml` files continue to load.

Extend `crates/jig/src/cli.rs` with a top-level `dev` command and `proxy` subcommands. `dev` starts configured apps, falls back to legacy `[[frontend_apps]]` when no dev apps are configured, or uses workspace discovery when requested. `proxy start`, `proxy stop`, `proxy list`, `proxy prune`, `proxy run`, `proxy alias`, `proxy cert`, and `proxy service` expose the lower-level operations. `proxy cert` has `generate`, `trust`, `untrust`, and `status`. `proxy service` has `install`, `uninstall`, and `status`.

Extend `crates/jig/src/runtime.rs` to dispatch the new commands to a narrow adapter that constructs `jig_dev_proxy` request structs from `RepoContext` and clap options. Do not add these tools to `.agent/jig-contract.json`. MCP exposure can be deferred unless a later user request asks for agent-callable proxy tools, because long-running local process management is better as an interactive CLI surface first.

Update `templates/project/.jig.toml.jinja` with default `[dev]` settings and optional rendered `[[dev.apps]]` examples derived from existing `frontend_apps` when possible. Update `templates/project/Makefile.jinja` so `make dev` remains the configured legacy command, but the help text can mention `scripts/jig dev` in docs. Update `docs/configuration.md`, `docs/public-contract.md`, and `docs/repo-intent.md` to describe the new runtime-owned proxy without claiming it is a make-backed contract tool.

## Concrete Steps

From `<private-path>`, edit files with `apply_patch` only for manual edits. After each major edit, run a focused command:

    cargo fmt --all
    cargo test -p jig-dev-proxy
    cargo test -p jig-sh dev_proxy
    cargo test -p jig-sh

After runtime changes compile, build and dogfood the dev binary:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    JIG_PROXY_STATE_DIR="$(mktemp -d)" scripts/jig proxy start --foreground --http-port 0

The foreground smoke command above should print a listening port and keep running until interrupted. For automated smoke tests, use short-lived local servers and random ports rather than privileged 80 or 443.

## Validation and Acceptance

The feature is accepted when these behaviors work:

1. `scripts/jig proxy start --http-port 0 --foreground` starts a proxy on an OS-assigned port, writes a port file into `$JIG_PROXY_STATE_DIR`, and responds with `X-Jig-Proxy: 1` on proxy-generated 404 pages.

2. A registered HTTP route works. Start a tiny local HTTP server on a random backend port, run `scripts/jig proxy alias smoke --port <backend_port> --http-port <proxy_port>`, then request `http://127.0.0.1:<proxy_port>/` with `Host: smoke.jig-sh.localhost`. The response body must be the backend response and include `X-Jig-Proxy: 1`.

3. A registered HTTPS route works. Generate local certs into a temporary state dir, start the HTTPS listener on a random port, then request with a client that trusts the generated CA file. The negotiated response must work over TLS, and a test should assert that HTTP/2 requests can be served by the TLS listener.

4. A WebSocket upgrade works over HTTP/1.1. A test backend should echo bytes after upgrade, and a client request through the proxy should complete the tunnel. If this is too heavy for the first automated test, at minimum the code path must be covered by a focused unit or integration test before marking done.

5. `scripts/jig proxy run vite-smoke -- <command>` assigns a free backend port, injects Vite host and port flags when the command vector is clearly Vite or package-manager-run-Vite, registers the route, cleans it when the child exits, and exits with the child status.

6. `scripts/jig dev` starts apps from `[[dev.apps]]`, or discovers workspace packages with dev scripts when configured, and prints stable URLs for each app.

7. `scripts/jig proxy cert generate` creates CA and leaf files. `scripts/jig proxy cert status` reports whether files exist and whether the CA appears trusted on the current platform. `trust` and `untrust` invoke platform commands only when explicitly requested.

8. `scripts/jig proxy service install` writes a user service file on macOS launchd or Linux systemd and `service status` reports the path and installed state. Unsupported platforms must return a clear error rather than silently succeeding.

9. Privileged ports are handled clearly. If a non-root user asks to bind 80 or 443 directly, Jig should fail before launching and print a command or service-install hint. Non-privileged random-port tests should continue to work without sudo.

The final verification run must include:

    cargo fmt --all -- --check
    cargo test -p jig-sh
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KRH6C77BFYG4KEFG8TE5SZS0
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KRH6C77BFYG4KEFG8TE5SZS0
    comprehensive review of the working tree, followed by fixes and another validation run if findings are actionable

## Idempotence and Recovery

All route operations must be safe to repeat. Adding a route for an existing hostname replaces the route atomically. Removing a missing route succeeds. `prune` removes routes owned by dead processes. `proxy stop` should remove only PID and port files for a process that is actually the Jig proxy, or give a clear warning when the PID file is stale.

Certificate generation should not overwrite an existing CA unless `--force` is provided. Leaf certificates can be regenerated when hostnames or LAN settings change. Trust and untrust commands must be explicit because they mutate user or system trust stores.

Service install should overwrite only Jig-owned service files at deterministic paths, such as `~/Library/LaunchAgents/sh.jig.proxy.plist` on macOS or `~/.config/systemd/user/jig-proxy.service` on Linux. Uninstall should unload/disable the service when possible and remove only those files.

## Interfaces and Dependencies

In `crates/jig-dev-proxy/src/types.rs`, define these core types:

    pub(crate) struct Route { pub(crate) hostname: String, pub(crate) target_host: String, pub(crate) target_port: u16, pub(crate) owner_pid: Option<u32>, pub(crate) mode: RouteMode, pub(crate) created_at_ms: u64 }
    pub(crate) enum RouteMode { Process, Alias }
    pub(crate) struct ProxyOptions { pub(crate) http_port: u16, pub(crate) https_port: Option<u16>, pub(crate) https: bool, pub(crate) http2: bool, pub(crate) lan: bool, pub(crate) tld: String, pub(crate) foreground: bool }
    pub(crate) struct AppRunSpec { pub(crate) name: String, pub(crate) dir: PathBuf, pub(crate) command: CommandSpec, pub(crate) kind: AppKind, pub(crate) hostname: String, pub(crate) explicit_port: Option<u16>, pub(crate) proxy: bool }
    pub(crate) enum CommandSpec { Argv(Vec<String>), Shell(String) }
    pub(crate) enum AppKind { EnvPort, Vite }

In `crates/jig-dev-proxy/src/lib.rs`, expose functions shaped like:

    pub fn dev(request: DevRequest) -> Result<serde_json::Value>;
    pub fn proxy_start(request: ProxyStartRequest) -> Result<serde_json::Value>;
    pub fn proxy_stop(request: ProxyStopRequest) -> Result<serde_json::Value>;
    pub fn proxy_list(request: ProxyListRequest) -> Result<serde_json::Value>;
    pub fn proxy_prune(request: ProxyPruneRequest) -> Result<serde_json::Value>;
    pub fn proxy_run(request: ProxyRunRequest) -> Result<serde_json::Value>;
    pub fn proxy_alias(request: ProxyAliasRequest) -> Result<serde_json::Value>;
    pub fn proxy_cert(request: ProxyCertRequest) -> Result<serde_json::Value>;
    pub fn proxy_service(request: ProxyServiceRequest) -> Result<serde_json::Value>;

Use dependency APIs as follows. `tokio::net::TcpListener` accepts connections. `hyper::server::conn::http1::Builder` serves cleartext HTTP and supports upgrades. `hyper_util::server::conn::auto::Builder` serves TLS connections and negotiates HTTP/1.1 or HTTP/2. `tokio_rustls::TlsAcceptor` wraps TLS sockets. `rcgen` generates a CA and leaf certificate for `localhost`, repo-scoped wildcard names, configured `tld`, and extra app hostnames when needed. `fs4::fs_std::FileExt` locks route files. `ctrlc::set_handler` removes active process routes on Ctrl-C.

## Artifacts and Notes

The Portless projects informed the shape of this plan. The Rust version demonstrates a small CLI with route storage, proxy start/stop, app run, `PORT` and `HOST` injection, route cleanup, and WebSocket forwarding. The Vercel Labs version demonstrates the broader v2 scope: config, HTTPS, local CA trust, workspace discovery, LAN/Tailscale features, service install, and framework-aware command handling.

Revision note 2026-05-13 / Codex: Replaced the initial one-line structured work body with a complete phase 0 ExecPlan so phase 1 can proceed from a restartable document.

Revision note 2026-05-13 / Codex: Updated the plan to move proxy behavior into a separate `jig-dev-proxy` crate after reviewing the v2 scope through a clean-architecture lens.
