# Align the Rust application generator with Batter f4f90c9

This living ExecPlan follows `.agent/PLANS.md`. The `rust-react` preset generates
Rust services against unpublished Batter Git pins. Batter advanced from the pinned
revision `abbe6f5c27887db9c5cbc7b7bd1fb6dd9967b00f` to
`f4f90c9166ff255a91298c75cc020136f632d758`. This plan advances the generated pins,
decides what the generator adopts from that range, and verifies every generated
database variant against the new pin.

Scope is the generator only: `crates/jig/src/bootstrap/scaffold/rust_workspace.rs`,
`templates/scaffolds/rust-react/**`, their embedded snapshots, scaffold tests, and
the Rust application documentation. Generated application source stays project-owned;
`jig update` does not rewrite it.

Acceptance: generated no-database, SQLite, and PostgreSQL workspaces compile, pass
strict Clippy and their own tests against the new pin; a live service still becomes
ready and exits 0 on SIGTERM; scaffold tests assert the new revision; documentation
and the changelog state the new pin and the deliberate adoption boundary; all
required Jig gates pass.

## Progress

- [x] Inspected the Batter range `abbe6f5..f4f90c9` (`de7bd2d`, `521f0e3`, `f4f90c9`).
- [x] Confirmed the range is additive for generated code: no `batter` core change,
      `batter-axum` only gains exports, and `operational_http` keeps its signature.
- [x] Created structured work `plan_01M2V3NJ67JY0030RGNW9TBRYD` at baseline `6c3a9b33`.
- [x] Advanced the generated pin in `rust_workspace.rs` and both scaffold test assertions.
- [x] Recorded the adoption boundary in `docs/rust-applications.md` and `CHANGELOG.md`.
- [x] Added the single-HTTP-observer invariant to the generated public HTTP crate guide
      and refreshed the embedded template snapshot.
- [x] Verified generated no-database, SQLite, and PostgreSQL variants against the new pin.
- [x] Ran the required Jig gates and inspected gate status, evidence, and receipts.
- [x] Finished structured work with recorded outcomes.

Checkpoint: complete. All required gates passed and structured work is closed with
outcome success. Source changes remain uncommitted in the worktree. Disposable generated
fixtures remain under the session scratchpad; their PostgreSQL containers were removed by
their own cleanup. Reproducing any step needs a prebuilt dev binary:
`cargo build -p jig-sh --bin jig` and `export JIG_DEV_BIN=target/debug/jig`.

## Surprises & Discoveries

The range is larger than its generator impact. `521f0e3` adds a whole new
`batter-runlimit` crate (native quota execution plus authenticated HTTP assembly)
and `f4f90c9` refines its denial integration, but neither changes an API the
generated workspace uses. `de7bd2d` touches only Batter's reference-service example
and documentation; it contains no `crates/` change.

The only `batter-axum` behavior change reaching existing consumers is internal: a
custom failure renderer now receives request parts with Batter's private quota
observation extension removed, and `request_admission` installs an opaque
`RequestInterruptionResponder` instead of cloning parts for its own interruption
rendering. The generated renderer in `crates/<name>-http-common/src/requests.rs`
reads only `CorrelationId`, so neither change alters generated behavior.

An unmodified generated no-database workspace compiled against the new pin before
any template edit, which is the evidence that this alignment is a pin advance rather
than a migration.

## Decision Log

2026-09-18: Advance `batter`, `batter-axum`, and `batter-sqlx` together to
`f4f90c9166ff255a91298c75cc020136f632d758`. Keep one revision across all generated
Batter packages, as the existing generator and documentation require.

2026-09-18: Do not generate `batter-runlimit`. Its `Quota::run` needs
application-owned policies and a Runlimit memory or PostgreSQL backend, and its
`HttpQuota` assembly additionally needs an authentication closure returning a
principal plus a subject selector returning native opaque keys. The starter has no
account, session, or credential model: the public API is unauthenticated and the
admin boundary is deliberately fail-closed with `DenyAllAdminAuthorizer`. Generating
a placeholder authentication closure would make invented security policy look like
starter policy. This follows the precedent set for Batter's browser credential
transport, which the previous alignment also left application-owned. Batter's own
integration contract additionally records that its reference service has not adopted
this adapter and that fresh-agent and live PostgreSQL acceptance remain unexecuted
for it.

2026-09-18: Keep the ordinary `operational_http` observer. `operational_http_with_quota`
only adds value with a quota writer; without an adapter it would allocate a record
that nothing ever takes and add `quota_outcome`/`quota_consumption` completion fields
that always read `not_checked`. Adopt it together with a quota adapter, not before.

2026-09-18: Record one generator-owned guardrail for the capability that is not
adopted. `crates/<name>-http/AGENTS.md` now states that the boundary keeps exactly one
HTTP observer and that a quota adapter replaces `operational_http` rather than nesting
a second observer inside it. Batter documents nesting as unsupported, and a project
owner adopting quota later edits generated source without this plan in hand.

2026-09-18: Do not consume `RequestInterruptionResponder` in generated code. It exists
for an adapter nested inside request admission; the generated boundary nests none and
already renders cancellation and deadline failures through its own request policy
renderer.

## Outcomes & Retrospective

Delivered: the generator pins all applicable Batter packages to `f4f90c9`, generated
services keep their existing composition because the Batter range is additive, and the
adoption boundary for native quota is recorded in the generated HTTP crate guide, the
Rust application documentation, and the changelog. Acceptance is met.

Generated fixtures used disposable, generic destinations under the session scratchpad:
`example-app` (`--defaults`, no database), `example-sqlite` (`--db sqlite --frontends web`),
and `example-postgres` (`--db postgres --frontends web,admin`). All three pin `batter`
and `batter-axum`, and the PostgreSQL variant additionally pins `batter-sqlx`, to
`f4f90c9166ff255a91298c75cc020136f632d758`.

Observed: an unmodified generated no-database workspace compiled against the new pin
before any generator edit, confirming the range is additive for generated code. Each
of the three variants then passed `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
`cargo test --workspace --all-features` (17, 28, and 22 tests respectively, no failures).

Observed live: the no-database service bound its listener, answered `/health/live`,
`/health/ready`, `/api/version`, and `/api/status` with 200, rendered the application
404 envelope with the server-generated correlation ID, and exited 0 after SIGTERM. The
PostgreSQL service against a disposable PostgreSQL 18 container migrated, became ready,
served `/api/status`, exited 0 after SIGTERM, and logged `cleanup="database.close"
outcome=Succeeded`. The generated `scripts/test-postgres.sh` migration workflow passed
its disposable-container test. Test containers were removed by their own cleanup.

Observed telemetry: completion events from the live services carried `method`, `route`,
`status`, `http_outcome`, and `latency_ms` and no `quota_outcome` or `quota_consumption`
fields, which is the direct evidence for the documented claim that the ordinary
`operational_http` observer allocates no quota record.

Observed gates: `work check` passed all five required targets, `api:test` running 4,234
tests with 4,234 passed and 3 skipped, alongside `api:clippy`, `api:fmt`, `repo:contract`,
and `repo:file-budget`. Run `run_01M2V4PPTC9JDKBA0CHWY5N27T` and target validation receipt
`receipt_01M2V5QRQKW6J4C36T7MEGRE4Q` record that source. `work gates` and `work evidence`
initially reported `unknown` because freshness inspection exceeded its default 2,000 ms
budget; re-inspecting with `--freshness-timeout-ms 30000` reported the single required
`verify` gate passed and fresh, with all seven changed source paths covered and no
unresolved gates. That timeout is an inspection budget, not stale evidence.

Not run: no live evaluation of `batter-runlimit` was performed, because the generator
deliberately does not depend on it. Refreshing the embedded template snapshot produced
exactly one changed snapshot file, matching the single edited template.

Lesson: the cost of a Batter alignment is dominated by deciding what not to adopt. The
mechanical pin advance was one constant and two test assertions; the work was reading the
range closely enough to prove it additive and to place the boundary where a project owner
will actually read it.

## Context and orientation

`crates/jig/src/bootstrap/scaffold/rust_workspace.rs` supplies the single
`batter_revision` template value. `templates/scaffolds/rust-react/workspace/Cargo.toml.jinja`
interpolates it for `batter`, `batter-axum`, and, for PostgreSQL only, `batter-sqlx`.
`crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/` mirrors the template
tree and is refreshed with `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`;
it needs no refresh when only the context value changes, because the revision is not
literal template text. Two scaffold tests assert the exact revision:
`crates/jig/src/bootstrap/tests/basic/scaffold_generation_parts/part_02_backend_assertions.rs`
and `crates/jig/src/bootstrap/tests/basic/scaffold_runtime_parts/part_01.rs`.

The generated HTTP boundary lives in `crates/<name>-http-common/src/requests.rs`
(`guard`, the response-construction budget, and the failure renderer),
`crates/<name>-http/src/lib.rs` (`router_with_lifecycle` and the outer observer), and
`crates/<name>-runtime/src/lib.rs` (protected startup, cleanup reservation, signals).

## Concrete steps

From the repository root with `JIG_DEV_BIN=target/debug/jig` exported after
`cargo build -p jig-sh --bin jig`:

1. Generate disposable fixtures under the session scratchpad with generic names:
   `target/debug/jig init <scratch>/example-app --defaults --no-vault`, plus
   `--preset rust-react --db sqlite --frontends web` and
   `--preset rust-react --db postgres --frontends web,admin` variants with `--no-input`.
2. In each fixture run `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
   `cargo test --workspace --all-features`.
3. Start the no-database API binary, confirm `/health/ready` and an application route,
   send SIGTERM, and require exit status 0.
4. Run `scripts/jig work check --plan-id plan_01M2V3NJ67JY0030RGNW9TBRYD`, then
   `work gates`, `work evidence`, and `work receipts` for that plan.

Expected result for each command is success; observed results are recorded in
`Outcomes & Retrospective` above, not here.

## Validation and acceptance

The pin advance is accepted when every generated variant passes its own formatting,
Clippy, and test gates against the new pin, the live no-database service exits 0 on
SIGTERM, and all required Jig gates pass. PostgreSQL variants need the repository's
disposable database workflow; if that endpoint is unavailable, record the skipped
check rather than claiming it.

## Idempotence and recovery

Fixtures are disposable: delete and regenerate them after a failure. Repeat failed
checks after fixes; never edit historical receipts. The change affects only newly
generated application source, so no persisted state, deployed contract, or existing
project needs migration. Reverting is a single revision-string change plus the
documentation edits.

## Interfaces and dependencies

Generated manifests depend on unpublished Batter Git packages, so the first uncached
build of each fixture needs network access. Batter requires Rust 1.94 on Unix.
Advancing a pin does not migrate an existing generated project; owners upgrade their
own `Cargo.toml` and lockfile deliberately.
