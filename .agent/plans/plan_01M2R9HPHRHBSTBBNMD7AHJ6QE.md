# Upgrade generated Batter capability boundaries

Update the `rust-react` generator from Batter revision
`d52a733a797bea197dda506493433e6ff3c8c4be` to
`abbe6f5c27887db9c5cbc7b7bd1fb6dd9967b00f`. Generated services must compile
against Batter's purpose-qualified lifecycle capabilities and must use typed,
server-generated request correlation rather than trusting an inbound header.

Acceptance requires the no-database, SQLite, and PostgreSQL generated workspace
variants to compile and test, the scaffold assertions and embedded snapshots to
match the source templates, and all applicable Jig gates to pass.

## Progress

- [x] Inspected repository guidance, current generated templates, and the upstream
  Batter range `d52a733..abbe6f5`.
- [x] T-01: migrate generated lifecycle and request-metadata boundaries and advance
  every applicable Batter pin together.
- [x] T-02: update template snapshots, scaffold assertions, ownership guides, and
  user-facing documentation.
- [x] T-03: verify focused generator tests, generated variants, and configured Jig
  gates.

Restart checkpoint: structured plan
`plan_01M2R9HPHRHBSTBBNMD7AHJ6QE`, baseline
`4e3768f0db9447e16be0335e66c7c8c6e797eb18`. The worktree already contains the
completed-but-uncommitted prior Batter `d52a733` update; preserve and extend it.
All tasks are complete. Fresh generated no-database, SQLite, and PostgreSQL projects
passed workspace tests, rustfmt, and strict all-target Clippy. The repository passed
all 4,071 tests plus Clippy, fmt, contract, and file-budget gates. A duplicate plan
`plan_01M2R9HPGW22SD64MGKY47WNFP` was opened by overlapping launcher commands and
must be closed as superseded without discarding either plan's append-only state.

## Surprises & Discoveries

- Upstream moved `RequestPolicy` from root `ShutdownHandle` authority to
  `OperationAdmission`, and readiness probing from the root handle to
  `LifecycleStatus`. Readiness approval is now a one-shot capability.
- Batter's `operational_http` already replaces inbound `x-request-id` values and
  inserts a typed `CorrelationId`. The current generator instead preserves raw
  request headers with Tower's request-ID layers, so a pin-only update would both
  fail to compile and retain the now-disallowed trust boundary.
- SQLx verification-policy changes do not affect the empty starter schema: the
  generator does not synthesize a verification manifest and continues to own
  connectivity and migration checks.
- The first generated compile exposed one stale `shutdown.clone()` capture after
  the signature migration. Removing it and regenerating produced passing builds;
  the failed disposable project was not reused as evidence.

## Decision Log

### D-01 — Pass purpose-qualified lifecycle projections to HTTP routers

- Status: accepted
- Context: upstream `RequestPolicy::new` requires `OperationAdmission`, while
  `batter_axum::readiness` requires `LifecycleStatus`.
- Choice: capture both projections from `Supervisor` before protected startup and
  pass them to generated routers. Keep `ShutdownHandle` inside the runtime owner.
- Why: generated HTTP code receives only the authority it needs and follows the
  new compile-time boundary.
- Alternatives: adapting the root handle inside HTTP crates is no longer accepted
  by the adapter API and would defeat the capability split.

### D-02 — Use Batter's typed server correlation as the application error witness

- Status: accepted
- Context: raw request headers are caller-controlled. `operational_http` replaces
  them and installs `CorrelationId` before inner middleware and handlers run.
- Choice: replace Tower request-ID middleware with `operational_http`; render
  custom `ApiError` values from `CorrelationId` extensions, including admission
  failures and authorization failures.
- Why: preserves the existing response envelope while proving that inbound IDs
  are never trusted.
- Alternatives: keeping the raw-header API would preserve an unsafe ambiguity;
  switching to Batter's built-in infrastructure envelope would change generated
  application messages unnecessarily.

## Outcomes & Retrospective

The generator now pins all applicable Batter packages to `abbe6f5`, supplies only
`LifecycleStatus` and `OperationAdmission` to HTTP composition, consumes linear
readiness approval in test routers, and uses `operational_http` plus typed
`CorrelationId` for error responses. Inbound request IDs are replaced rather than
trusted. No-database, SQLite, and PostgreSQL generated projects passed workspace
tests, rustfmt, and strict all-target Clippy; PostgreSQL compiled `batter-sqlx` at
the same revision. Repository gates passed with 4,071 tests, and upstream `master`
still resolved to the pinned full revision during final review. No gaps remain.

## Context and orientation

`crates/jig/src/bootstrap/scaffold/rust_workspace.rs` supplies the pinned revision.
Source templates live under `templates/scaffolds/rust-react`; byte-matched embedded
copies live under
`crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/rust-react`.
Scaffold contract assertions are in
`crates/jig/src/bootstrap/tests/basic/scaffold_generation_parts/part_02_backend_assertions.rs`
and `scaffold_runtime_parts/part_01.rs`. `docs/rust-applications.md` and
`CHANGELOG.md` describe the generated runtime contract.

## Plan of work and milestones

### T-01 — Generated services use the new Batter authority model

- Outcome: generated services compile against `abbe6f5` and never pass root
  shutdown control into HTTP policy or trust an inbound request ID.
- Context: D-01 and D-02.
- Changes: runtime, public HTTP, admin HTTP, HTTP-common, entrypoint, manifests,
  and directly affected tests under `templates/scaffolds/rust-react/workspace`;
  the generator revision in `rust_workspace.rs`.
- Depends on: none
- Verify: render a representative scaffold and run `cargo test --locked` after
  refreshing its lockfile against the new pin; assert a supplied `x-request-id`
  differs from the returned typed server ID.
- Recovery: forward-fix templates; generated applications are new outputs and no
  persisted state is migrated.
- Done when: no generated use of removed `ShutdownHandle::new`, `mark_ready`, or
  `RequestPolicy::new(ShutdownHandle, ...)` remains.

### T-02 — Generator contract and documentation match the cutover

- Outcome: embedded templates, assertions, ownership guides, and public docs state
  the same capability and request-correlation contract.
- Changes: embedded snapshots, scaffold tests, generated `AGENTS.md` templates,
  `docs/rust-applications.md`, and `CHANGELOG.md`.
- Depends on: T-01
- Verify: focused scaffold generation tests and an exact source/snapshot diff.
- Recovery: not needed; documentation and snapshots are derived from T-01.
- Done when: assertions detect the new pin, capability types, operational wrapper,
  and inbound-ID replacement while rejecting obsolete wiring.

### T-03 — All generated variants and repository gates pass

- Outcome: no-database, SQLite, and PostgreSQL projects are valid outputs and the
  repository accepts the generator change.
- Changes: validation evidence and append-only Jig receipts only, except focused
  fixes required by failures.
- Depends on: T-02
- Verify: focused Jig crate tests; generated `cargo fmt --check`, strict Clippy,
  and tests for all three DB branches; `scripts/jig work check`, `work gates`, and
  required final backend test gate with `JIG_DEV_BIN=target/debug/jig`.
- Recovery: delete only disposable `/tmp` generated projects after evidence is
  captured; never alter user projects.
- Done when: all applicable checks pass and the structured plan can be closed.

Critical path: T-01 -> T-02 -> T-03. The tasks intentionally remain sequential
because source templates, their embedded copies, and scaffold assertions share one
generated contract.

## Idempotence and recovery

Template regeneration and project generation are repeatable. All validation
projects use unmistakably generic `ExampleProject` names under `/tmp`. No existing
application is rewritten by `jig update`, and no database state is migrated.

## Interfaces and dependencies

All applicable workspace dependencies must use exact Git revision
`abbe6f5c27887db9c5cbc7b7bd1fb6dd9967b00f`: `batter`, `batter-axum`, and
PostgreSQL-only `batter-sqlx`. HTTP composition accepts
`LifecycleStatus` plus `OperationAdmission`; application errors accept an optional
typed `batter_axum::CorrelationId`. The runtime retains shutdown control and driver
ownership through cleanup.
