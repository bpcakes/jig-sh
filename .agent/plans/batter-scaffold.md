# Make Batter the default Rust application scaffold

This living ExecPlan follows `.agent/PLANS.md`. New `jig init --defaults` projects should use Batter's operational foundation. `--preset rust-react` is the sole Rust application preset and always generates Batter services. The user explicitly cancelled gradual retirement and requested a direct cutover. Rust library and CLI presets remain available for those separate project shapes.

## Progress

- [x] Inspected init, preset discovery, render contexts, and upstream Batter source.
- [x] Created structured work `plan_01M28R3W6MYTGFXX3EFK98VBKR` at baseline `76e2f2f6`.
- [x] Keep one Rust application preset and remove all Batter/non-Batter branching; 65 wizard tests pass.
- [x] Integrate generated service startup, request policy, readiness, and bounded shutdown in a dedicated runtime crate.
- [x] Verify generated no-database, SQLite, PostgreSQL, and admin variants; generated tests, Clippy, live shutdown, and PostgreSQL migration checks pass.
- [x] Update documentation and embedded template snapshots.
- [x] Complete all configured gates: 4,068 tests passed (3 skipped), plus Clippy, formatting, contract, and file budget.
- [x] Inspect gate status, evidence, and receipts: all required targets are fresh and passing.
- [x] Required `scripts/jig check test` passed all 4,068 tests (3 skipped); structured work closed with outcome success.

Checkpoint: complete. All required gates and the final standalone test invocation passed. Structured work `plan_01M28R3W6MYTGFXX3EFK98VBKR` and its session are closed successfully. Source changes remain uncommitted in the worktree. Temporary generated fixtures remain under `/tmp/jig-batter-only-example`, `/tmp/jig-batter-verified-example`, and `/tmp/jig-batter-postgres-example`; disposable database containers were cleaned up.

## Surprises & Discoveries

Batter packages are unpublished (`publish = false`) and currently target Unix, Rust 1.94 and newer. Its source is split into `batter` and `batter-axum`; there is no HTTP feature on the base crate. SQLx remains an optional separate adapter. The existing Jig application scaffold already uses Axum 0.8, SQLx 0.9, and Rust 1.94, so those boundaries align.

## Decision Log

2026-09-11 (superseded): Initially proposed parallel presets and conditional templates. The user rejected the assumed one-to-one file layout, then explicitly requested Batter-only Rust applications. Keep the public rust-react name, remove the non-Batter implementation, and give supervision a dedicated runtime crate separate from application state and shared HTTP errors. Existing project-owned applications are still not rewritten by adopt/update.

2026-09-11: Pin Batter dependencies to public Git revision `f1cafe9abeb9c08960523288c2c2c25da5e18202`. Do not invent a registry release or require a sibling checkout. Generated project owners can deliberately upgrade the pin later.

## Outcomes & Retrospective

The single-path generated public/admin workspace compiles against pinned Batter. Application state does not depend on Batter; the runtime crate owns resource acquisition, cleanup registration before migrations, native HTTP serving, and signals. Public route lists are shared between runtime wiring and OpenAPI generation. Deadlines retain application error envelopes; drain rejects application work but leaves probes available. No Batter/non-Batter condition or alternate preset remains.

Observed verification: 65 wizard tests passed; the generated-format matrix across names, database choices, and migration paths passed. Generated SQLite and PostgreSQL workspaces passed tests and strict Clippy; generated OpenAPI consistency tests passed. The disposable PostgreSQL migration test passed. Live no-database public/admin services returned expected responses and exited 0 on SIGTERM. Final SQLite and PostgreSQL services also started, migrated, became ready, and exited 0 after SIGTERM. A stalled PostgreSQL handshake interrupted by SIGTERM exited 1 through the owned startup failure path immediately. A real PostgreSQL-backed service with an occupied HTTP listener failed at `http.bind` and exited 1 within five seconds. Temporary test containers were removed by their test cleanup.

Final `work check` passed all five required targets. Run `run_01M28Y0RG62GYY3AC2MNA0G98W` and validation receipt `receipt_01M28Z5RF5CAP1Y6BC5ATBPFY6` record the final source; `work gates` and `work evidence` with a 30-second freshness inspection confirmed all targets fresh, with no unresolved gates. The separately invoked `scripts/jig check test` also passed all 4,068 tests (3 skipped). `work finish` closed the plan and session with outcome success. No implementation or verification work remains.

## Context and implementation milestones

`crates/jig/src/bootstrap_parts/part_01.rs` declares CLI preset values. `bootstrap/presets.rs` defines their capabilities and public discovery. `cli/init_wizard.rs` resolves defaults and interactive answers. `bootstrap/scaffold/project.rs` records the selected identity; `scaffold/rust_workspace.rs` supplies the template context. Templates in `templates/scaffolds/rust-react/workspace` generate application-owned Rust code, which is not rewritten by harness updates. A dedicated generated runtime crate owns process supervision. There is no Batter context flag or alternate non-Batter branch.

First complete selection and discovery, then compose Batter's `Supervisor`, `ShutdownHandle`, `register_signals`, and `register_http` in generated services. A supervisor owns service tasks and reports shutdown completion; handle readiness gates incoming work and changes during drain. Apply `RequestPolicy` only to application routes, keeping health probes reachable. Keep the application's error response format and independent admin authorization boundary. Native database pools should be closed during owned cleanup. Do not substitute generic foundation types for application state or business errors.

Then test actual generated source against the pinned dependency. Verify successful requests, readiness, rejected work after drain, deadline exhaustion, signal shutdown, and process failure reporting. Check admin still denies unauthenticated callers. Keep generated OpenAPI and frontend clients consistent with existing endpoints. Document Unix support, the Git pin, budget semantics, and the direct cutover.

## Concrete validation and acceptance

From the repository root, rebuild with `cargo build -p jig-sh --bin jig`. Refresh embedded snapshots using `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh` after template edits. Run focused init and scaffold tests, then generate generic temporary fixtures with `target/debug/jig init <temporary-directory>/example-app --defaults --no-vault`. Use explicit database and admin selections for additional variants. Run generated `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features`. A live no-database service must answer existing health/API routes, then exit cleanly after SIGTERM. PostgreSQL live checks require the repository's local test database workflow.

Run `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M28R3W6MYTGFXX3EFK98VBKR`, inspect `work gates`, `work evidence`, and `work receipts` for that plan, and finish with `scripts/jig check test` under the same dev binary setting. Record actual results here and finish structured work only when applicable checks pass.

## Recovery

Use fresh generic temporary directories for generated fixtures; init's existing transactional safeguards remain authoritative for existing destinations. Rebuild and repeat failed checks after fixes. Never edit historical receipts. No existing downstream scaffold or persisted configuration needs migration: the scaffold cutover only affects newly generated application source.

Initial plan written after inspecting the existing scaffold and pinned upstream APIs.

Revision: replaced the abandoned parallel-preset design with the user-requested direct cutover, and recorded executed lifecycle checks rather than planned results.
