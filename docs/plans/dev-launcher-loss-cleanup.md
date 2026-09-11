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
- [x] Implement the private CLI/worker lifetime channel and signal/exit forwarding.
- [x] Add launcher-loss, descendant, startup, and replacement regression coverage.
- [x] Improve orphan diagnostics and document the protection's limits.
- [x] Run focused checks, configured work gates, and the final backend test check.

Restart checkpoint: implementation and verification are complete. The final
backend check passed all 4,068 workspace tests (3 skipped) on Linux. Structured work
is `plan_01M28HMC5RGK08FTZTZ4X8GSM4`, baseline
`dc68b74497c4f5ae27049425bb63e6d0ec4ff014`, Beads issue `jig-sh-sl1b`.
The worktree was clean at inspection. The Beads database was safely reconciled
with its newer JSONL export; the privacy helper now exports successfully.
An independent design review supports an owning worker; an external observer of
registry PIDs would violate the process-ownership invariant.

## Surprises & Discoveries

The installed CLI handles SIGINT, SIGHUP, and SIGTERM correctly in an isolated
fixture, but SIGKILL leaves its direct app alive. Once that app exits, the existing
orphan retirement path works. Separate app sessions are intentional for process
group cleanup. A parent-death signal on only the app wrapper would not cover its
forked descendants.

Independent review found that two forwarded OS signals could coalesce while the
worker is paused. The channel now sends ordered signal bytes and the worker uses
the existing generation-aware signal-intent handler directly. A paused-worker
test holds the route lock and proves that both first-signal status and forced
cleanup survive. Early messages wait for lifecycle/resource arming so they cannot
bypass validation error reporting.

The replacement regression launches a real app, observes its durable registration,
then makes initialization fail before listening. It verifies the captured app
error, retained successful recovery notice, and empty session registry. The test
also kills the actual worker deliberately and checks the explicit lost-supervisor
error rather than claiming that worker SIGKILL can clean up.

## Decision Log

2026-09-11: Preserve the existing rule that only the live owning supervisor
terminates registered app trees. Move CLI execution into a detached owning worker
and keep the existing registry schema and authenticated control endpoint. The
frontend forwards termination requests and returns the worker's exit status.

2026-09-11: Move the CLI launch adapter, orphan warning formatting, and focused
orphan test into small modules. Relocate the existing public preflight adapter
into `dev_api.rs` while retaining its root reexport. This keeps touched legacy
files from growing beyond the repository's file-budget limits.

2026-09-11: Keep preflight in the worker because it may own processes and cleanup
obligations too. Its closure-based library API remains usable directly; the private
worker split belongs to CLI launch, not serialized runtime requests or MCP.

2026-09-11: Register the new test binary in the source process partition and the
existing serialized process-test group, with matching declared inputs in the
repository configuration and contract. Keep all existing tests selected.

## Outcomes & Retrospective

Implementation and repository verification are complete. The launcher-loss suite (four behavior regressions
and two opt-in helper entrypoints), two lifecycle tests, nine existing signal
tests, and 479 dev-proxy unit tests passed during implementation. The final
`JIG_DEV_BIN=target/debug/jig scripts/jig check test repo:file-budget --plan-id
plan_01M28HMC5RGK08FTZTZ4X8GSM4` run passed all 4,068 workspace tests, with 3
skipped; its test receipt is `receipt_01M28MFSNXZ3WKNRX75CQ2YZ7Q`. Formatting,
Clippy, contract, and file-budget checks also passed. Earlier full-suite evidence
retains a failure of the unchanged one-second receipt-writer timing test; that
test passed both in isolation and in the final complete run without assertion or
timeout changes.

The originating failure's exact
termination source is unknown; the regression uses deliberate launcher termination
and does not claim to establish that history. The dev binary is built locally;
no global installation, commit, or downstream-project mutation is part of this work.

## Context and interfaces

`crates/jig/src/cli/run.rs` dispatches dev commands, and
`crates/jig/src/cli/run/dev_launch.rs` reexecs launch with a hidden
project identity in argv. `crates/jig/src/cli/proxy.rs` defines launch-only options.
The new private worker option will transport a Unix socket descriptor, never a
credential or persisted PID. The worker must validate the descriptor, take
ownership, and restore close-on-exec before any preflight or app can be spawned.

`crates/jig-dev-proxy/src/processes/cleanup.rs` owns one-shot signal registration,
first-signal selection, repeated-signal escalation, and bounded cleanup. A launcher
adapter at `crates/jig-dev-proxy/src/processes/cleanup/launcher.rs` reuses this
signal lifecycle to forward ordered requests to its direct child. Worker socket loss requests the same ordinary
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
