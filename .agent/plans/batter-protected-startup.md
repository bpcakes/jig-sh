# Adopt Batter protected startup in generated Rust applications

This plan follows `.agent/PLANS.md`. New `rust-react` applications should use the
upstream startup boundary that retains supervision and cleanup authority, including
when initialization fails or is cancelled. PostgreSQL pool cleanup must be owned
before native construction can start work. Existing application source remains
project-owned; this is not an automatic migration of deployed applications.

## Progress

- [x] Investigate upstream changes from the generated pin and review migration scope.
- [x] Implement protected service startup, PostgreSQL pool ownership, and matching docs/tests,
  including the review corrections for bounded waits, hermetic PostgreSQL failure setup,
  normalized inferred/explicit `Batter_Sqlx` collision coverage, and retained production
  serve coverage.
- [x] Validate generated variants and production-entrypoint checks, including the
  subprocess-isolated no-DB bind-failure regression.
- [ ] Complete the full Jig `api:test` gate; it remains blocked by unrelated
  work-evidence recovery behavior documented below.

Structured Jig work: `plan_01M2BC2AFCQ00TT99Y3V4Z49XD` (baseline `7c65ef11`).

Checkpoint: implementation complete on baseline `7c65ef11` on
`feat/batter-rust-scaffold`; focused and generated validation is green. The worker
recorded structured plan `plan_01M2BC2AFCQ00TT99Y3V4Z49XD`. The no-DB production
bind-failure test now launches the generated test executable as a child with
command-scoped environment overrides; it makes no unsafe process-global environment
mutation. The one justified full `api:test` rerun reached 3,085/4,071 tests, then
was cancelled after two unrelated work-evidence recovery tests failed because Jig
could not safely clean the process tree while running `git diff --cached --binary` in
a temporary worktree. Do not commit or push without a new request.

The user requires frugal delegation: Astra owns planning, milestone reviews, and
blockers; reuse the Luna Max worker `batter_upgrade` for commands, implementation,
testing, and polling. Workers report completed milestones or blockers, not a stream
of logs. Preserve this division in future handoffs.

## Surprises & Discoveries

Upstream tip `f5824cf836c9d1146d67d7b0dc99d011921bd02f` contains the protected-startup
change `b87708db`; existing generated applications still compile against it. A pin
bump alone therefore does not adopt its protections. The new PostgreSQL `pool_in`
helper registers cleanup synchronously after lazy construction, before returning a
pool, and deliberately does not prove connectivity. There is no equivalent SQLite
helper in this upstream API.

## Decision Log

Use `Startup::scoped` with library-owned Unix signals and `register_http_in` for all
service shapes. Keep finite database setup under `Command`, whose API has not changed.
Keep request admission behavior and all current budgets unchanged. These decisions
follow the upstream source comparison on 2026-09-12.

Use the upstream PostgreSQL pool helper in both service startup and finite setup,
with explicit connectivity validation before readiness. Preserve SQLite's native
configuration and manual cleanup path. Add conditional package collision validation
for `batter-sqlx`, since adding that dependency must not regress rejection before
destination writes. Do not reserve that name for shapes that do not depend on it.

## Outcomes & Retrospective

Investigation and design review are complete. Implementation includes the protected
startup cutover, conditional PostgreSQL `batter-sqlx` dependency and preflight collision
guard, pool ownership before connectivity checks, preserved SQLite behavior, docs, and
refreshed embedded snapshots. Focused evidence is green:

- `cargo test -p jig-sh --lib scaffold_generation -- --nocapture` (40 passed, 1 ignored)
- `cargo test -p jig-sh --lib scaffold_runtime -- --nocapture` (32 passed)
- `cargo fmt --all -- --check`
- Fresh generic fixtures under `/home/aa/jig-batter-review.zxfjaS`: no-DB workspace tests
  and strict Clippy; SQLite and PostgreSQL admin workspace check, strict Clippy, and tests;
  the PostgreSQL pool ownership test uses a retained loopback listener and passes.
- Fresh no-DB fixture `/home/aa/jig-batter-final2.yUXQWV`: exact production
  `tests::production_serve_reports_bind_failure` passed with child-process environment
  isolation; fixture formatting and strict Clippy passed. The exact serial
  recovery-test reproduction is logged at
  `/home/aa/jig-batter-recovery-tests-20260912.log`.
- Fresh binaries exercised healthy SIGINT/SIGTERM for no-DB, SQLite API, PostgreSQL API,
  and PostgreSQL admin API. SQLite and PostgreSQL bootstrap success/failure plus startup
  cancellation retained `database.close` cleanup success where a local database was
  available. PostgreSQL used the disposable local service on port 55431.

The configured full `api:test` gate remains blocked by the two recovery-test failures
above; 986 tests were not run after nextest cancelled on failure. The serial bounded
follow-up passed `native_target_recovery_preserves_plan_comparison_without_overrides`
in 36.9 seconds. The overall 120-second command bound included 55 seconds of
compilation, so it terminated
`read_only_recovery_predicts_independent_execution_and_reuse` after only 22.7 seconds
of execution. That isolated reproduction is incomplete, not proof of a hung test.
The full-gate failure details are retained in
`.agent/state/receipts.jsonl` under
`receipt_01M2BHBSAACS14CN1YQR5S3Y20`; no further full-suite retry was started because
the failure is outside this change and the parent requested bounded validation.
Direct generic Batter tests are not treated as production-entrypoint proof. The
task-created PostgreSQL container `batter-review-primary` was stopped after validation.

## Context and interfaces

The Git pin is rendered by
`crates/jig/src/bootstrap/scaffold/rust_workspace.rs`. Editable templates live under
`templates/scaffolds/rust-react/workspace`; embedded copies under
`crates/jig/src/bootstrap/scaffold/embedded_template_snapshots` are regenerated by
the build, not independently edited. At the baseline, generated runtime `serve` used
`Startup::new`, accessed the supervisor during initialization, installed signals
manually, and registered pool closure only after an awaited database connection; the
implementation now uses the protected startup boundary described below.

`batter::startup::ProtectedStartupScope` exposes `stage`, `reserve_cleanup`, and
registration-only authority, not the full supervisor. Capture the shutdown handle
from the supervisor before moving it into `Startup::scoped`; pass that same handle
to the HTTP router. `.with_unix_signals("signals")` installs library-owned SIGINT
and SIGTERM listeners when starting. `batter_axum::register_http_in(scope, name,
listener, router)` registers the server without handing out supervisor authority.

`batter_sqlx::pool_in(slot, PgPoolOptions, PgConnectOptions)` consumes a reserved
`CleanupSlot` and returns a native lazy PostgreSQL pool. Lazy construction can start
maintenance work. The caller must explicitly check connectivity and run migrations
before readiness. Cleanup awaits local pool closure; it cannot guarantee remote
rollback or stop detached server sessions. Keep database configuration and native
pool policy in the DB crate; runtime owns cleanup slots and operation sequencing.

## Milestones and concrete steps

The implementation milestone updates the shared Git revision, generated manifests,
runtime initialization, and PostgreSQL DB construction. Add a cleanup-slot-aware
PostgreSQL entrypoint without unnecessarily breaking native test helpers. Use it
from service and command paths. Preserve existing SQLite connection/migration
semantics and preserve the application state's lack of supervision responsibilities.
Update preflight name validation, including normalized spellings and existing
destinations, before writes. Update generated ownership guides and current user
documentation; historical plans remain historical.

In the repository root, build with
`JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo build -p jig-sh --bin jig`, then use
`JIG_DEV_BIN=target/debug/jig scripts/jig` for structured work and validation. Open
work with `work start --title "Adopt Batter protected startup" --body` containing
this plan's scope and validation, and record its returned ID here. Follow the
configured `work check`, `work gates`, `work evidence`, and `work finish` workflow.

The validation milestone renders fresh no-DB, SQLite, and PostgreSQL applications,
including the separate admin listener. Check formatting, strict Clippy, and tests
in generated projects; run direct binary signal tests outside the two-second Jig
dev grace period. Use fresh generic fixtures and shared build artifacts where safe.
Run `cargo test -p jig-sh --lib bootstrap::scaffold -- --test-threads=4` and relevant
generation regressions. Finish backend verification through `scripts/jig check test`
using the dev binary; configured gate execution may satisfy this without duplication.

## Validation and acceptance

Generated service initialization has no full-supervisor accessor or manual signal
handoff. Healthy public/admin services become ready, reject business work once
draining, and leave probes outside admission. Existing request IDs and error
envelopes remain intact. Normal SIGINT/SIGTERM shutdown joins work and cleanup;
failure or cancellation during initialization must retain registered finalizers.

PostgreSQL construction registers cleanup before a connection await, and failure
to connect cannot yield a ready application. Verify normal bootstrap/migrations and
failed or interrupted setup cleanup with a local database if available. Include
behavioral regression coverage for the new ownership path, not only string checks.
Document any unavailable external service or unrun check explicitly. Prove that
`batter-sqlx` collisions are rejected before both new and existing destination
mutation for PostgreSQL, while unrelated shapes keep accepting the name.

## Recovery and scope limits

No persisted data format or public API migration is intended. Never overwrite an
existing application to test regeneration. Reserve cleanup before acquisition;
do not interpret cancellation as rollback of completed migrations or database
creation. Use short temporary paths on a volume with sufficient capacity, preserve
unrelated work, and avoid cleanup commands targeting broad directories. Update this
plan at milestones with concise observed results and remaining gaps.

Initial plan: records reviewed upstream APIs and adds the newly introduced package
collision boundary to the worker's proposed migration scope.

Final review: generated-code changes and focused validation are accepted; the full
test gate remains unsuccessful. Corrected the reproduction timing from the actual
log so the command-wide deadline is not mistaken for a per-test hang, and recorded
the final subprocess-test fixture path. No unrelated recovery code was changed.
