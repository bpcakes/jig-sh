# Clean up development apps after launcher loss

This ExecPlan follows `.agent/PLANS.md` and is maintained during implementation.

## Purpose and acceptance

Prevent an abruptly terminated `jig dev` CLI from leaving its development app
trees running and blocking the next launch. On Linux and macOS, a dedicated
worker will own the existing development lifecycle. A private Unix socket connects
that worker to the invoking CLI. Closure of the CLI endpoint requests the worker's
ordinary bounded cleanup. Apps and their descendants must not inherit this socket.

Acceptance requires killing the invoking CLI while a wrapper and server descendant
are running, observing both processes disappear and session state retire, and
successfully launching again. Normal SIGINT, SIGHUP, SIGTERM, authenticated stop,
replacement, startup errors, and JSON exit statuses must retain their behavior.
Replacement followed by startup failure must retire the new session and preserve
successful orphan-recovery notices. Fixtures use only generic project names and
isolated proxy state.

The protection covers loss of the invoking CLI, including its Unix process group.
SIGKILL of the actual owning worker defeats userspace cleanup and remains subject
to the existing conservative orphan policy. Existing orphan sessions cannot be
retrofitted. No downstream project state is part of this implementation.

## Progress

- [x] Investigated session ownership, signal cleanup, CLI reexec, and existing tests.
- [x] Built the current development binary for harness commands.
- [ ] Implement the private CLI/worker lifetime channel and signal/exit forwarding.
- [ ] Add launcher-loss, descendant, startup, and replacement regression coverage.
- [ ] Improve orphan diagnostics and document the protection's limits.
- [ ] Run focused checks, configured work gates, and the final backend test check.

Restart checkpoint: implementation has not started. The worktree was clean at
inspection. The baseline and structured plan ID will be recorded after work start.
An independent design review supports an owning worker; an external observer of
registry PIDs would violate the process-ownership invariant.

## Surprises & Discoveries

The installed CLI handles SIGINT, SIGHUP, and SIGTERM correctly in an isolated
fixture, but SIGKILL leaves its direct app alive. Once that app exits, the existing
orphan retirement path works. Separate app sessions are intentional for process
group cleanup. A parent-death signal on only the app wrapper would not cover its
forked descendants.

## Decision Log

2026-09-11: Preserve the existing rule that only the live owning supervisor
terminates registered app trees. Move CLI execution into a detached owning worker
and keep the existing registry schema and authenticated control endpoint. The
frontend forwards termination requests and returns the worker's exit status.

2026-09-11: Keep preflight in the worker because it may own processes and cleanup
obligations too. Its closure-based library API remains usable directly; the private
worker split belongs to CLI launch, not serialized runtime requests or MCP.

## Outcomes & Retrospective

Implementation and verification remain pending. The originating failure's exact
termination source is unknown; the regression uses deliberate launcher termination
and does not claim to establish that history.

## Context and interfaces

`crates/jig/src/cli/run.rs` dispatches dev commands and reexecs launch with a hidden
project identity in argv. `crates/jig/src/cli/proxy.rs` defines launch-only options.
The new private worker option will transport a Unix socket descriptor, never a
credential or persisted PID. The worker must validate the descriptor, take
ownership, and restore close-on-exec before any preflight or app can be spawned.

`crates/jig-dev-proxy/src/processes/cleanup.rs` owns one-shot signal registration,
first-signal selection, repeated-signal escalation, and bounded cleanup. A launcher
adapter in the same crate can reuse this signal lifecycle to forward requests to
its unreaped direct child. Worker socket loss will request the same ordinary
termination handling, including during preflight and app readiness.

`crates/jig-dev-proxy/src/dev_sessions.rs` and `dev_sessions/management.rs` own
session claims and stop/recovery. Records continue to identify the process that
actually owns apps: the worker. Old binaries can still use the authenticated stop
protocol. Existing state versions and conservative orphan retirement remain intact.

## Milestones and concrete steps

First implement the CLI worker branch and Unix lifetime transport. Both socket
ends are close-on-exec by default; only the worker endpoint is explicitly inherited
for the one worker exec. Keep frontend stdin/stdout/stderr semantics, and run the
worker in its own Unix session. Management subcommands never create a worker.

Next exercise real CLI process trees in `crates/jig/tests`: normal termination,
launcher SIGKILL after readiness and during startup, prompt cleanup of descendants,
and subsequent launch. Keep fixture cleanup independently armed on failures. Update
existing lifecycle assertions that previously equated CLI PID with supervisor PID
to verify the actual worker relationship. Extend orphan failure diagnostics with
inspection and manual recovery guidance without suggesting that an unsafe forget
flag can terminate a live app.

Finally update the crate guide and `docs/configuration.md` and
`docs/developer-ux.md`, review the complete diff, rebuild `cargo build -p jig-sh
--bin jig`, and use `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id
<plan-id>`. Inspect `work gates`, `work evidence`, and `work receipts`, then finish
backend verification with `JIG_DEV_BIN=target/debug/jig scripts/jig check test`.
Close structured work only after required gates pass.

## Validation and recovery

Run focused CLI lifecycle and signal suites and the dev-proxy unit suite before
the configured verify gate. Regressions must use observable process/listener and
session disappearance, not only a successful return code. Verify no proxy state
outside each fixture is touched, no control credential is emitted, and no new
worker is recursively launched. Required gates include formatting, tests, contract,
and file budget as configured by the repository.

The change is an internal coordinated binary cutover with unchanged persisted
state. Repeating `dev stop` remains safe. On failed worker startup, close the
frontend socket so any successfully started worker receives cancellation. On
unconfirmed worker cleanup, retain current diagnostic/state behavior. No existing
registry records are rewritten for migration and no live project is stopped.


Implementation is complete. Current living plan: docs/plans/dev-launcher-loss-cleanup.md. Focused verification: 5 launcher-loss tests, 2 session lifecycle tests, 9 signal tests, and all 479 jig-dev-proxy unit tests passed. The private socket preserves first/force order while a worker is paused; worker SIGKILL reports dev_supervisor_lost. Beads synchronization reconciled existing export data without deletion and passed privacy validation. Run configured gates next.

Final checkpoint: implementation and verification are complete; the maintained ExecPlan is docs/plans/dev-launcher-loss-cleanup.md. The final backend run passed all 4068 workspace tests (3 skipped), receipt receipt_01M28MFSNXZ3WKNRX75CQ2YZ7Q. Focused launcher-loss, lifecycle, signal, and 479 dev-proxy tests also passed. The unchanged one-second receipt-writer test failed in an earlier full attempt and passed in isolation and the final full run; no assertion or timeout was weakened. Required gate inspection reported every target passed and fresh. Protection covers invoking CLI/process-group loss, not SIGKILL of the owning worker. The binary is built locally; nothing was globally installed or committed, and no downstream project was changed.