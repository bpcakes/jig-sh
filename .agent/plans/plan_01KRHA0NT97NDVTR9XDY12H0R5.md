# Make the Jig Dev Proxy Production Ready

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds. This plan follows `.agent/PLANS.md`.

## Purpose / Big Picture

Jig now has a Portless-style local development proxy, but the fresh review found several issues that would hurt real daily use: generated frontend configs can duplicate apps, workspace-discovered Vite apps may still collide on default ports, HTTPS certificates do not adapt well after the proxy starts, HTTP forwarding buffers streaming responses, local CA keys are too permissive, LAN/privileged-port behavior is underspecified, and the riskiest server paths lack tests. After this work, `scripts/jig dev` and `scripts/jig proxy` should be reliable enough for multi-project local development with Axum-style Rust backends, Vite frontends, HTTP, HTTPS/HTTP2, aliases, LAN mode, and service installation.

The observable outcome is that tests and smokes prove: generated configs do not duplicate frontend apps, Vite workspace discovery receives deterministic ports, HTTP forwarding supports streaming responses without full buffering, HTTPS can serve newly added route names without stale certificate surprises, certificate private keys are mode `0600` on Unix, LAN mode reports usable host information, privileged port errors point to concrete service/capability handling, and the comprehensive review loop has no actionable production-readiness findings left.

## Progress

- [x] (2026-05-13T18:36:40Z) Opened structured work `plan_01KRHA0NT97NDVTR9XDY12H0R5` and captured the production-readiness objective.
- [x] (2026-05-13T18:36:40Z) Reviewed `.agent/PLANS.md`, the ExecPlan skill, and the current diff/review findings.
- [ ] Phase 0: complete this comprehensive implementation plan and update it as decisions are made.
- [x] (2026-05-13T18:52:00Z) Phase 1 implementation batch completed: template duplicate emission removed, `dev.apps` made authoritative with `frontend_apps` fallback, Vite workspace detection and package-manager flag insertion added, shell Vite commands fixed, installer dev-bin branch clarified, certificate key permissions hardened, leaf cert host coverage extended to active/new routes, HTTPS listener reloads the leaf cert per connection, HTTP proxying streams bodies, WebSocket detection requires `Connection: upgrade`, LAN/privileged-port output improved, and focused tests added.
- [x] (2026-05-13T19:08:00Z) First comprehensive review loop produced actionable findings; fixed stop diagnostics, route caching, TLS config caching with file-signature reload, WebSocket forwarded Host/status gating, dedicated health endpoint, atomic certificate writes, clearer HTTPS-restart guidance, and Ctrl-C route cleanup across state directories.
- [ ] Phase 2: run formatting, clippy, workspace tests, harness checks/gates, smoke tests, and the comprehensive Claude plus Codex review loop.
- [ ] Loop: fix any actionable review or test findings and re-run the relevant validation.
- [ ] Finish the structured work plan only after no actionable issues remain.

## Surprises & Discoveries

- Observation: The thread-level goal API cannot create a second goal after the earlier completed goal.
  Evidence: `create_goal` returned `cannot create a new goal because this thread already has a goal; use update_goal only when the existing goal is complete`.

- Observation: The streaming server and merge tests compile and pass with the rest of the workspace.
  Evidence: `cargo test --workspace` passed with 21 `jig-dev-proxy` tests and 121 `jig-sh` tests after the first implementation batch.

- Observation: The HTTP health endpoint must be tested with a freshly rebuilt Jig binary.
  Evidence: An initial smoke against the old `target/debug/jig` returned 404 for `/__jig_proxy_health`; after `cargo build -p jig-sh --bin jig`, the same smoke returned `health={"ok":true} http=jig-smoke-ok https=jig-smoke-ok`.

## Decision Log

- Decision: Treat this repository work plan as the active production-readiness goal because the thread-level goal object is already complete and cannot be replaced.
  Rationale: The user explicitly asked for a goal loop, and the repo harness provides durable plan, receipt, gate, and finish semantics for exactly this case.
  Date/Author: 2026-05-13 / Codex.

- Decision: Keep the clean separate-crate architecture from the previous implementation.
  Rationale: `crates/jig-dev-proxy` owns proxy mechanics and `crates/jig/src/dev_proxy.rs` remains the CLI/config adapter, which matches the crate guides and keeps transport logic thin.
  Date/Author: 2026-05-13 / Codex.

## Outcomes & Retrospective

No implementation outcome yet. This section will be updated after the phase 1 fixes, then after the phase 2 review loop.

Phase 1 outcome: the initial production-readiness findings have corresponding code changes and focused tests. `cargo clippy --workspace --all-targets --locked -- -D warnings` passes after formatting.

Review-loop outcome: Claude Code identified high-priority issues in stop diagnostics, hot-path route reads, TLS reload cost, and WebSocket forwarding. The implementation now addresses those with diagnostics, caches, a health route, atomic certificate swaps, and corrected WebSocket behavior. Full validation and a second review pass remain.

## Context and Orientation

The repository root is `<private-path>`. The Jig CLI crate is `crates/jig`; it parses CLI commands in `crates/jig/src/cli.rs`, loads `.jig.toml` in `crates/jig/src/context.rs`, dispatches commands in `crates/jig/src/runtime.rs`, and adapts CLI/config to the proxy crate in `crates/jig/src/dev_proxy.rs`.

The proxy implementation lives in `crates/jig-dev-proxy`. Its public API is in `src/lib.rs`, shared request/config types are in `src/types.rs`, local mutable state is in `src/state.rs`, HTTP/HTTPS forwarding is in `src/server.rs`, certificate generation and trust helpers are in `src/certs.rs`, process supervision and dev command launching are in `src/processes.rs`, service file generation is in `src/service.rs`, and JavaScript workspace discovery is in `src/workspace.rs`.

The generated project template is under `templates/project`. The template `.jig.toml` is `templates/project/.jig.toml.jinja`; the repo-local installer script is `scripts/install-jig.sh`, and generated repos receive `templates/project/scripts/install-jig.sh.jinja`.

Terms used here: a route is a hostname-to-local-port mapping stored in the proxy state directory. The proxy state directory defaults to `~/.jig/proxy` unless `JIG_PROXY_STATE_DIR` is set. LAN mode means the proxy binds beyond loopback so another device on the local network can reach it. A local CA is a development certificate authority generated on the user's machine; if trusted, its private key must be protected because it can sign certificates the browser will accept.

## Plan of Work

First, fix configuration and template behavior. In `crates/jig/src/dev_proxy.rs`, make `[[dev.apps]]` authoritative for dev proxy app definitions when it is non-empty. Only synthesize apps from legacy `[[frontend_apps]]` when `dev.apps` is empty, and add a test that loads a template-shaped `.jig.toml` with both sections and verifies only one app per frontend appears. In `templates/project/.jig.toml.jinja`, either stop emitting duplicate `[[dev.apps]]` or keep it with the authoritative rule; the implementation should be clear in docs.

Second, fix workspace and Vite handling. In `crates/jig-dev-proxy/src/workspace.rs`, inspect the package `scripts.dev` command to decide whether a discovered package is Vite-backed. In `crates/jig-dev-proxy/src/processes.rs`, ensure package-manager commands receive Vite flags in the correct place. For `bun run dev`, `npm run dev`, `pnpm run dev`, and `yarn run dev`, Vite flags should be added after a `--` separator when needed. For direct `vite`, flags can be appended directly. Shell commands marked `kind = "vite"` should either get safe inline flags or fail with a useful message requiring `argv`; prefer safe inline appending only when it is simple and deterministic.

Third, fix proxy forwarding and TLS lifecycle. In `crates/jig-dev-proxy/src/server.rs`, stop collecting full request/response bodies for ordinary HTTP. Use Hyper body types so request and response bodies stream through the proxy. Preserve header rewriting, `x-jig-proxy`, HTTP/1.1 upstream normalization, and hop-by-hop header filtering. For HTTPS, avoid stale certificate surprises when new route hostnames are added after proxy startup. A pragmatic production-ready approach is to generate leaf certificates with broad development SANs that cover route hostnames for the configured TLD, including `*.localhost`, `*.*.localhost`, configured repo wildcard names, `localhost`, and LAN host/IP names when available. If a requested route still is not covered, return a clear restart/regenerate error before adding it.

Fourth, harden certificates, service, LAN, and privileged-port UX. In `crates/jig-dev-proxy/src/certs.rs`, write CA and leaf private keys with mode `0600` on Unix. In `src/service.rs` and docs, make privileged port handling explicit: service install writes a user service by default and reports the exact Linux `setcap` or macOS root/port-forward option needed for ports 80/443 instead of implying the current process can do it. In `src/lib.rs`, `src/processes.rs`, and `src/server.rs`, include LAN bind host and display host information in JSON/output so users can reach the proxy from another device.

Fifth, close the test gaps. Add focused tests in `crates/jig-dev-proxy` for Vite workspace discovery and flag injection, certificate key permissions and SAN coverage, streaming HTTP forwarding, WebSocket request classification, HTTPS not-found port reporting, and app config duplicate avoidance. Add `crates/jig` tests for dev app merging and template-shaped config behavior. Prefer deterministic local tests using ephemeral ports and `JIG_PROXY_STATE_DIR`.

Finally, validate and review. Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace`, `make test`, `make contract-check`, `make check-agent-guides`, `make check-agent-map`, `JIG_DEV_BIN="$PWD/target/debug/jig" scripts/jig work check --plan-id plan_01KRHA0NT97NDVTR9XDY12H0R5`, and `JIG_DEV_BIN="$PWD/target/debug/jig" scripts/jig work gates --plan-id plan_01KRHA0NT97NDVTR9XDY12H0R5`. Run the comprehensive review skill, merge findings, fix any actionable issues, and repeat the validation necessary for those fixes.

## Concrete Steps

All commands run from `<private-path>`.

Build the current development binary before harness operations:

    cargo build -p jig-sh --bin jig

Use the current binary for harness commands:

    JIG_DEV_BIN="$PWD/target/debug/jig" scripts/jig work status

Edit files with `apply_patch`. Do not remove unrelated untracked files such as `landing.html`.

After implementation, run:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace
    make test
    make contract-check
    make check-agent-guides
    make check-agent-map
    JIG_DEV_BIN="$PWD/target/debug/jig" scripts/jig work check --plan-id plan_01KRHA0NT97NDVTR9XDY12H0R5
    JIG_DEV_BIN="$PWD/target/debug/jig" scripts/jig work gates --plan-id plan_01KRHA0NT97NDVTR9XDY12H0R5

## Validation and Acceptance

Acceptance requires the commands above to pass and a comprehensive review loop to report no actionable findings. Additional targeted acceptance:

Generated repos with `frontend_apps` must not fail `jig dev` because of duplicate app names. A test should fail before the merge fix and pass after it.

Workspace discovery must identify Vite dev scripts and launch them with deterministic port and host flags. Tests should show `bun run dev`, `npm run dev`, `pnpm run dev`, `yarn run dev`, and direct `vite` commands receive flags in the correct shape.

HTTP forwarding must stream. A test should proxy a response that sends multiple chunks over time and prove the client receives the first chunk before the backend finishes the full body.

HTTPS certificate behavior must cover repo-scoped route names such as `api.demo.localhost`, aliases added after startup where feasible, and LAN names/IPs that the code reports. Tests should inspect generated certificates or successfully complete TLS handshakes with the expected names.

Certificate private keys must have Unix mode `0600` when generated on Unix.

Privileged-port behavior must either work via documented service/capability setup or fail with precise actionable messaging.

## Idempotence and Recovery

Most edits are local source changes and can be repeated safely. Proxy smoke tests must use a temporary `JIG_PROXY_STATE_DIR` and must stop any proxy they start before exiting. If a smoke test leaves a process behind, find it with `ps -axo pid,command | rg 'jig proxy|proxy run|server.py'` and terminate only the process started by the test. Do not kill unrelated user processes.

If a harness command fails because the repo-local cached Jig binary is stale, rebuild with `cargo build -p jig-sh --bin jig` and rerun using `JIG_DEV_BIN="$PWD/target/debug/jig"`. If the worktree changes while this plan is active, preserve user changes and adapt around them rather than reverting.

## Artifacts and Notes

The fresh review immediately before this plan identified the following actionable issues to address: duplicate generated frontend apps, workspace Vite discovery missing Vite kind/flags, stale HTTPS certificates after new routes, full-body HTTP buffering, incomplete port 80/443 privilege handling, permissive certificate key files, LAN mode only binding without usable host information, Vite shell command flag injection, installer helper dead code, missing server tests, and missing dev app merge tests.

## Interfaces and Dependencies

Keep the public proxy crate API in `crates/jig-dev-proxy/src/lib.rs` stable for `crates/jig/src/dev_proxy.rs`. New helper types may be added inside `types.rs` if they make JSON outputs or tests clearer. Use the existing dependencies already in `Cargo.toml` unless a small, well-justified test dependency is required. Use Hyper 1 and `http-body-util` for streaming body composition rather than introducing another proxy framework.

The CLI command names remain `scripts/jig dev` and `scripts/jig proxy ...`. The mutable proxy state remains outside `.agent/state`, with `StateStore::resolve` honoring `JIG_PROXY_STATE_DIR`.

## Revision Notes

2026-05-13 / Codex: Replaced the initial one-paragraph harness body with a self-contained production-readiness ExecPlan so another agent can continue from this file alone.

2026-05-13 / Codex: Updated progress after the implementation batch and recorded test/clippy evidence. The next required phase is full validation plus comprehensive review.

2026-05-13 / Codex: Recorded the first comprehensive review loop and fixes. The next step is to rerun all validations and repeat the review to check for remaining actionable issues.

2026-05-13 / Codex: Completed repeated review loops and addressed the remaining concrete findings: proxy stop verification, route read locking, workspace discovery clarity, macOS CA untrust scope, bounded WebSocket fallback reads, route/TLS cache hardening, Vite/CLI parsing edge cases, and expanded regression tests including WebSocket happy-path and route-cache invalidation. Final local validations passed: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace`, `cargo check -p jig-sh --no-default-features`, live HTTP/HTTPS proxy smoke, `make test`, `make contract-check`, `make check-agent-guides`, `make check-agent-map`, and fresh structured work gates. `landing.html` remains an unrelated untracked file and was not modified.
