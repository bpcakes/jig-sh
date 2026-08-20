# Harden generated Go lifecycle invariants

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while implementation proceeds. Maintain this file in accordance with `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

After this work, a generated Go API will mutate database schema only during its explicit bootstrap command, will wait for in-flight HTTP requests to finish when it receives shutdown, and will let an interactive user correct an invalid Go module without restarting `jig init`. These changes reduce future bugs by giving each lifecycle action one owner: bootstrap owns migrations, the server runner owns both listening and draining, and the existing Go module validator remains the sole grammar authority.

The behavior is observable in focused Rust rendering and wizard tests, in generated Go tests that exercise shutdown, and in the repository's complete configured test, format, Clippy, and contract gates. The migration, shutdown, and prompt changes must be separate commits so each invariant can be reviewed or reverted independently.

## Progress

- [x] (2026-08-20 17:57Z) Classified the comprehensive-review findings by root cause and opened structured work plan `plan_01M0G52N232B6GBV0ANBBH5C7J`.
- [x] (2026-08-20 18:00Z) Moved migration execution out of ordinary pool opening and into explicit bootstrap, refreshed snapshots, and passed the focused rendering regression.
- [ ] Make HTTP serving and graceful shutdown one joined lifecycle, add a generated Go regression test, and commit the slice.
- [ ] Reuse canonical Go module validation inside an interactive retry loop, add focused wizard tests, and commit the slice.
- [ ] Rebuild the development binary and run the complete configured test, format, Clippy, and contract gates.
- [ ] Record evidence, review the final commit series, update this plan's outcome, and close structured work.

## Surprises & Discoveries

- Observation: The pinned Goose v3.27.3 `UpContext` path does not acquire a migration lock by default; locking is available only through separately configured provider/session-lock APIs.
  Evidence: The local module source at `/home/aa/go/pkg/mod/github.com/pressly/goose/v3@v3.27.3/up.go` reads migration state and applies pending migrations directly, while lock support is exposed separately under `lock/`.

- Observation: The generated browser runner already executes `--bootstrap-database` before starting a PostgreSQL API.
  Evidence: `templates/scaffolds/rust-react/frontend/vite-react/playwright.config.ts.jinja` constructs the Go backend command as bootstrap followed by serve, so removing implicit migration from `Open` preserves generated E2E setup.

## Decision Log

- Decision: Make explicit bootstrap the sole migration owner instead of adding a lock around migrations on every server start.
  Rationale: The scaffold already exposes and documents a bootstrap lifecycle. Keeping schema mutation out of ordinary connection establishment makes `Open` safe for every caller, avoids deployment-time replica races by construction, and prevents every future connection path from needing to remember a lock policy.
  Date/Author: 2026-08-20 / Codex

- Decision: Introduce a server-running helper that owns the listener, shutdown request, and terminal result as one joined operation.
  Rationale: Waiting on an incidental shutdown goroutine would repair the immediate symptom but retain split ownership. A helper whose contract is "serve until failure or cancellation, then finish draining before returning" is directly testable and keeps pool cleanup after HTTP cleanup.
  Date/Author: 2026-08-20 / Codex

- Decision: Keep `validate_go_module` as the only Go module grammar implementation and call it from a dedicated prompt loop.
  Rationale: Copying validation into the wizard would create drift. Final invariant validation remains defense in depth for noninteractive and library callers, while the prompt loop adds recoverable interaction.
  Date/Author: 2026-08-20 / Codex

## Outcomes & Retrospective

Implementation is in progress. At completion this section will compare the observed migration, shutdown, and prompt behavior with the purpose above and record final gate receipts.

## Context and Orientation

`crates/jig/src/bootstrap/scaffold/go_workspace.rs` inventories Go scaffold templates. The application source itself lives under `templates/scaffolds/go-react/workspace`, with an embedded byte-for-byte mirror under `crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace`. Source template changes therefore require the normal embedded snapshot refresh mechanism rather than hand-editing both copies.

`templates/scaffolds/go-react/workspace/internal/database/database.go.jinja` currently defines `Bootstrap`, `Open`, and the internal `migrate` helper. `Bootstrap` creates a missing PostgreSQL database and then calls `Open`; `Open` currently runs migrations before opening a pgx pool. That coupling means normal API startup mutates schema and two replicas can race. The corrected boundary will call `migrate` explicitly from `Bootstrap`, while `Open` will only construct and verify the long-lived pool.

`templates/scaffolds/go-react/workspace/cmd/api/main.go.jinja` constructs the HTTP server. Its current detached goroutine calls `Shutdown`, but `ListenAndServe` returns as soon as the listener closes, letting `run` and the process return before request draining completes. The corrected implementation will create a listener explicitly and call a helper that runs `server.Serve`, observes cancellation, invokes `Shutdown` synchronously, and joins the serving result before returning. `cmd/api/main_test.go.jinja` will prove an in-flight request prevents the helper from returning until the handler is released.

`crates/jig/src/cli/init_wizard.rs` owns interactive project-shape prompts. `bootstrap::validate_go_module` is the canonical module grammar and is already called by final scaffold invariant validation. A new prompt helper will loop over `prompt_line`, print the canonical validation error, and retry until the module is valid. Final validation remains unchanged for noninteractive and direct callers.

## Plan of Work

First, edit the Go database template so `Bootstrap` applies migrations after ensuring the target database exists and before it opens a pool for the final connectivity proof. Remove migration execution from `Open`. Extend the rendered-scaffold regression to assert that migration ownership appears in the right function, refresh the embedded template snapshot, run the focused scaffold tests, and commit this slice as `fix(go): isolate migration bootstrap lifecycle`.

Second, replace `ListenAndServe` plus the detached shutdown goroutine with explicit `net.Listen` and a `serve` helper. The helper starts `server.Serve` in one goroutine, selects between its terminal error and context cancellation, performs bounded shutdown on cancellation, force-closes after a shutdown error, then consumes the server result before returning. Extend the generated main test with an actual loopback request whose handler blocks during cancellation; the test must observe that `serve` remains blocked until the handler completes. Refresh the snapshot, run focused rendering tests, and, if the installed Go toolchain can select the pinned toolchain, run the generated Go package test before committing `fix(go): join graceful API shutdown`.

Third, add a `prompt_go_module` helper beside the other wizard prompt helpers. It will call the existing `bootstrap::validate_go_module`, print the error with the same indentation as other prompt feedback, and continue reading. Replace the one-shot prompt in `guide_project_shape` and add a test that enters one invalid module followed by a valid one. Run the focused wizard tests and commit `fix(init): reprompt invalid Go modules`.

Finally, rebuild `target/debug/jig`, force `scripts/jig` to use it through `JIG_DEV_BIN=target/debug/jig`, run structured work checks and the repository's full gates, inspect receipts and the complete diff, update this plan, and close the work.

## Concrete Steps

Run all commands from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`.

After each template slice, refresh embedded scaffold snapshots with:

    JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh

Run focused tests using the exact names added or changed under `bootstrap::tests::basic::scaffold_generation` and `cli::init_wizard_tests`. Verify every slice with:

    cargo fmt --all -- --check
    git diff --check
    git status --short

At the end, build and use the current runtime:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig work check --plan-id plan_01M0G52N232B6GBV0ANBBH5C7J
    scripts/jig check fmt
    scripts/jig check clippy
    scripts/jig check contract
    scripts/jig check test
    scripts/jig work gates --plan-id plan_01M0G52N232B6GBV0ANBBH5C7J
    scripts/jig work evidence --plan-id plan_01M0G52N232B6GBV0ANBBH5C7J
    scripts/jig work receipts --plan-id plan_01M0G52N232B6GBV0ANBBH5C7J

## Validation and Acceptance

A rendered PostgreSQL Go scaffold must contain exactly one call site that applies migrations in the database package, and that call must be in `Bootstrap`, not `Open`. The generated database integration test must continue to prove that bootstrap creates the database, applies the migration, and supports the checked-in sqlc query.

The generated API shutdown test must start a real loopback server, enter a blocking handler, cancel the server context, and prove the server helper has not returned. After releasing the handler, the client request and server helper must both complete successfully within a bounded timeout. This demonstrates the active request-drain path and guards against the original early-return behavior.

During interactive initialization, entering `example.com/ExampleProject.` and then `example.com/ExampleProject` at the module prompt must leave the final answer set to the valid value and print the canonical invalid-module diagnostic once. Strict and defaults modes must retain their current behavior.

The final complete configured test gate, format check, warnings-denied Clippy check, and contract check must all exit zero. The work gate must report fresh passing contract and test receipts with no intervening changes.

## Idempotence and Recovery

Template refresh and focused tests are deterministic and safe to repeat. Each behavioral slice is committed only after its focused tests pass, so a later failure can be repaired in a new commit without rewriting earlier work. Temporary generated projects, if needed, must use `mktemp -d` and generic fixture names. `.agent/state/*.jsonl` is append-only; never truncate or rewrite earlier records.

If graceful-shutdown validation exposes a toolchain mismatch, keep the Rust rendering regression mandatory and record the missing external proof rather than weakening the helper. If a full gate fails, diagnose and repair the failing slice, rebuild the development binary, and rerun the affected gate before recording evidence.

## Artifacts and Notes

The review established two architectural failures and one interaction omission:

    Open = connection establishment + schema mutation      # unsafe ownership
    run returns when listener closes, not when drain ends   # unjoined lifecycle
    prompt accepts raw input; final validation aborts       # non-recoverable UX

The target boundaries are:

    Bootstrap -> create database -> migrate -> Open/ping
    Open      -> create pool -> ping
    serve     -> listen -> observe cancellation -> drain -> join -> return
    prompt    -> read -> canonical validate -> retry or return

## Interfaces and Dependencies

No new Rust or Go dependency is required. Generated Go code continues using `net/http`, adding only the standard-library `net` package to create a listener explicitly. The server helper will have the stable local signature:

    func serve(ctx context.Context, server *http.Server, listener net.Listener) error

`database.Bootstrap(ctx, databaseURL)` remains the sole public schema-initialization entrypoint, and `database.Open(ctx, databaseURL)` retains its existing signature while becoming side-effect-free with respect to schema. `bootstrap::validate_go_module(&str) -> anyhow::Result<()>` remains the validation authority; the wizard adds only an interactive adapter around it.

Plan revision note (2026-08-20 17:57Z): Expanded the structured-work summary into a self-contained implementation plan after root-cause analysis of the comprehensive-review findings.
