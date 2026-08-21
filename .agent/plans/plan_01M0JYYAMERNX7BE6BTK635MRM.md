# Run configured Codex tasks on schedule

This ExecPlan is a living document. The sections `Progress`, `Surprises &
Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept
current as implementation proceeds. Maintain this document in accordance with
`.agent/PLANS.md`.

## Purpose / Big Picture

After this work, a repository owner can define a durable prompt file and a cron
schedule in `.jig.toml`, manually test the task with `jig loop tick`, and have a
headless scheduler invoke `jig loop dispatch` periodically. Each dispatch will
find every due workflow, claim each scheduled occurrence once, run the Codex
task with explicit unattended permissions, and record machine-local schedule
state plus durable Jig receipts. `jig loop status`, the aggregate `jig status`,
and `jig ui` will show the next due time, latest run, worker receipt, and any
stale or exhausted work that needs attention.

This extends Jig's existing level-triggered loop design. It does not add a
resident daemon: cron, systemd, launchd, or GitHub Actions should call
`jig loop dispatch` at a regular cadence. A level-triggered dispatcher can be
called more often than a task is scheduled because durable occurrence state
prevents the same scheduled instant from running twice.

## Progress

- [x] (2026-08-21T20:07Z) Reviewed the existing loop facade, engine, schedule,
  workflow, state, worker runner, CLI, aggregate status, and UI boundaries.
- [x] (2026-08-21T20:07Z) Confirmed current OpenAI scheduled-task guidance:
  durable prompts, narrow unattended sandboxes, `approval_policy = never` where
  allowed, and isolated worktrees for local mutation.
- [x] (2026-08-21T20:07Z) Opened structured work as
  `plan_01M0JYYAMERNX7BE6BTK635MRM` and wrote this implementation plan.
- [x] (2026-08-21T21:05Z) Add renewable ownership for workflow and branch leases, including active
  renewal, owner-checked release, and crash expiry tests.
- [x] (2026-08-21T21:05Z) Add strict typed configuration for optional workflow schedules and the
  compiled `codex_task` workflow kind.
- [x] (2026-08-21T21:05Z) Add deterministic five-field cron evaluation with explicit IANA timezone
  handling, coalesced missed runs, and clock-controlled tests including DST.
- [x] (2026-08-21T21:05Z) Add durable occurrence claim, completion, known-failure, and stale-claim
  reconciliation state under `.agent/.cache/loop`.
- [x] (2026-08-21T21:05Z) Implement the safe Codex task workflow, repo-contained prompt loading,
  worktree isolation, worker receipts, and typed sandbox/model controls.
- [x] (2026-08-21T21:05Z) Add `jig loop dispatch`, human and JSON output, and compatibility-safe
  command conversion.
- [x] (2026-08-21T21:05Z) Extend loop status, aggregate status, dashboard snapshot types, and HTML
  rendering with schedule and recent-run facts.
- [x] (2026-08-21T21:05Z) Update strict configuration documentation, public-contract migration
  notes, CLI help, README, and changelog.
- [x] (2026-08-21T22:05Z) Run focused tests, formatter, strict Clippy, full Jig tests, development
  binary dogfood, contract checks, plan-scoped gates, and a requirement-by-
  requirement completion audit.

## Surprises & Discoveries

- Observation: the existing loop lease defaults to fifteen minutes, but the
  shared Codex worker timeout defaults to thirty minutes. `LeaseStore` has no
  renewal operation, so a second scheduler can acquire the workflow or branch
  while the first worker is still running.
  Evidence: `crates/jig/src/context/loop_config.rs` sets 900 seconds and
  `crates/jig/src/runtime/worker_runner.rs` sets 30 minutes.

- Observation: the split `schedule.rs` contains only immediate repeat-until-
  idle behavior. No current serialized state or public JSON field needs a
  schedule migration.
  Evidence: `crates/jig/src/runtime/loops/schedule.rs` only defines `run_until`.

- Observation: `croner` 3.0.1 supports a parser configuration that disallows
  seconds and years and evaluates a `chrono::DateTime` in an IANA timezone.
  `chrono-tz` 0.10.4 declares Rust 1.65 compatibility, below this workspace's
  Rust 1.85 floor. The selected versions will be pinned through workspace
  dependencies and validated with the repository's MSRV-compatible checks.

- Observation: official OpenAI documentation states that Codex CLI does not
  expose the Scheduled management interface. Jig's headless dispatch command
  therefore remains a separate repository-owned capability rather than a thin
  wrapper over a CLI schedule API.

- Observation: `croner` can return a timestamp one millisecond before a
  daylight-saving transition as its first previous-occurrence candidate for a
  nonexistent local wall time. The schedule boundary now verifies candidates
  with `is_time_matching` and continues searching before accepting them.
  Evidence: the fixed `Europe/Prague` spring-forward test initially received an
  invalid candidate and now resolves March 28 as the previous valid 02:30 run.

- Observation: the timezone-aware evaluator treats a repeated fall-back wall
  time as one scheduled occurrence, while a nonexistent spring-forward wall
  time is skipped. Both policies now have fixed Europe/Prague tests and are
  documented for operators.

- Observation: a scheduled claim can race with a manual tick that already
  holds the workflow lease. Consuming the occurrence in that case would lose a
  run even though no scheduled work started. Dispatch now abandons its
  owner-checked claim only when the engine proves it never acquired the
  workflow lease, then retries the same due occurrence later.

## Decision Log

- Decision: add `codex_task` as a compiled workflow kind rather than accepting
  arbitrary command graphs.
  Rationale: this preserves the original Jig invariant that `.jig.toml`
  parameterizes audited workflow implementations but cannot inject a new
  orchestration language.
  Date/Author: 2026-08-21 / Codex

- Decision: keep manual `loop tick --workflow ID` schedule-independent and add
  `loop dispatch` for due-only execution.
  Rationale: a task prompt must be manually testable before enabling recurring
  unattended runs, while the external scheduler needs one command that safely
  multiplexes all configured schedules. Existing tick semantics remain
  compatible.
  Date/Author: 2026-08-21 / Codex

- Decision: support one five-field cron expression and an explicit IANA
  timezone, defaulting to `UTC`; disallow optional seconds, years, and ambient
  local timezone.
  Rationale: minute granularity matches an external dispatcher and explicit
  zones make daylight-saving behavior deterministic across hosts.
  Date/Author: 2026-08-21 / Codex

- Decision: coalesce missed occurrences into the most recent due instant.
  Rationale: restarting a laptop after a week must not launch an unbounded
  backlog of autonomous tasks. The status record retains the occurrence time,
  making the coalescing observable.
  Date/Author: 2026-08-21 / Codex

- Decision: claim an occurrence before worker launch and classify an expired
  running claim as `needs_attention` rather than automatically executing it
  again.
  Rationale: Codex may have modified files or external systems before an
  unobservable crash. Preventing ambiguous duplicate side effects is safer than
  pretending exactly-once execution is possible.
  Date/Author: 2026-08-21 / Codex

- Decision: default `codex_task` to `sandbox = "read-only"` and
  `checkout = "worktree"`; allow explicit `workspace-write` but not full-access
  configuration.
  Rationale: unattended work should receive the narrowest capability. A
  worktree separates repository mutations from the user's checkout and is
  useful even when the prompt is later broadened.
  Date/Author: 2026-08-21 / Codex

- Decision: retain worktrees after a task that changed files or failed, and
  remove clean successful worktrees. Report retained paths in occurrence state
  and receipts.
  Rationale: changes need a reviewable handoff; clean worktrees would otherwise
  accumulate without value.
  Date/Author: 2026-08-21 / Codex

## Outcomes & Retrospective

Items 1–7 are implemented. Workflow and branch leases renew during blocking
work. Strict typed `codex_task` configuration feeds deterministic cron and IANA
timezone evaluation, versioned occurrence coordination, an externally driven
due dispatcher, narrow unattended Codex execution, and status/UI rendering.
Existing manual tick and run commands remain compatible; all new JSON and UI
fields are additive and no MCP tool was added.

Verification completed with strict formatting and Clippy, 49 focused config
tests, 16 schedule/state tests, 27 loop runtime tests, 6 Jig UI tests, the full
Jig suite (1,433 passed and 2 intentional stress/network tests ignored), CLI
dogfood, contract receipt `receipt_01M0K2PM0MFPN6PGYWKT8EWQ1P`, and final
passing plan gate receipt `receipt_01M0K3MKS39BPKPBGQ481VDR2B`. No part of
items 1–7 remains; installing a platform scheduler is deliberately
operator-owned.

## Context and Orientation

`crates/jig/src/context/loop_config.rs` strictly deserializes `[loop]` and
`[[loop.workflows]]`. Unknown keys fail fast. Workflow configuration currently
contains an id, a compiled kind, enabled state, lease and retry tuning, and an
optional Codex home accepted only by `pr_manager`.

`crates/jig/src/runtime/loops.rs` is the stable private facade. It routes manual
ticks and status into `engine.rs`, immediate repeat behavior into `schedule.rs`,
machine-local JSON caches into `state.rs`, and workflow configuration resolution
into `workflow.rs`. `github.rs`, `noop.rs`, and `pr_manager.rs` implement the
three compiled kinds. `crates/jig/src/runtime/worker_runner.rs` already runs
`codex exec` with timeouts, process cleanup, structured output support, and a
durable `jig.worker_run` receipt.

A lease is a machine-local record granting one random owner token exclusive
access to a workflow or branch until a timestamp. Renewal means extending that
timestamp only when the caller still presents the same owner token. A scheduled
occurrence is one cron instant for one workflow, identified by workflow id and
the scheduled UTC millisecond. Claiming it means writing a running record under
the loop cache before spawning Codex. The occurrence record is distinct from
the append-only receipt: it is mutable scheduler coordination state, whereas
receipts are durable history.

`crates/jig/src/cli/loops.rs`, `crates/jig/src/command/loops.rs`, CLI command
conversion, and `crates/jig/src/cli/output/loops.rs` own the command surface.
`crates/jig/src/status.rs` embeds `loop status` in the aggregate status result.
`crates/jig/src/ui/snapshot.rs` decodes the same result into public types from
`crates/jig-ui/src/model.rs`, and `crates/jig-ui/src/html/dashboard.rs` renders
the loop section.

Configuration changes are compatibility-sensitive. Existing workflow entries
must continue to decode and preserve their current JSON. New optional keys do
not require a generated contract version bump under `docs/public-contract.md`,
but they do require strict config validation, init/update round-trip tests,
documentation, and a changelog migration note. Adding a new CLI subcommand may
change help and command-conversion tests but does not expose a new MCP tool
unless tool definitions are intentionally changed.

## Plan of Work

First, extend `state.rs` with owner-checked renewal and introduce a lease guard
that runs a small renewal thread while blocking workflow work executes. The
guard must stop and join its thread before releasing the owner token. A renewal
failure must become visible and prevent the caller from claiming success. The
guard is used for the top-level workflow lease and PR-manager branch lease.

Second, extend loop configuration with optional `schedule` and `timezone` on
all compiled workflows and add fields specific to `codex_task`: `prompt_file`,
`model`, `sandbox`, and `checkout`. The validator accepts `codex_task`, requires
a schedule and prompt file for that kind, rejects task-only fields on other
kinds, accepts `codex_home` for PR manager or Codex tasks, limits sandbox to
`read-only` or `workspace-write`, and limits checkout to `repo` or `worktree`.
Prompt paths must be relative, normal, repository-contained paths with no NUL,
parent traversal, root, or symlink escape when loaded.

Third, add `croner`, `chrono`, and `chrono-tz` workspace dependencies and turn
`schedule.rs` into a pure schedule boundary plus the existing run wrapper. A
`ScheduleSpec` parses exactly five cron fields and one IANA timezone. Given a
UTC millisecond and the last claimed occurrence, it returns the most recent
unclaimed due occurrence and the next future occurrence. Tests use fixed
timestamps and cover invalid syntax, invalid zones, ordinary cadence, disabled
workflows, coalescing, and spring/fall daylight-saving transitions.

Fourth, extend `state.rs` or a focused sibling `occurrence.rs` when file size
requires it. Store a versioned `schedule.json` under `.agent/.cache/loop` using
the existing locked JSON protocol. Records carry workflow id, occurrence id,
scheduled time, claim owner, claim expiry, started/finished times, status,
worker receipt id, retained worktree path, and bounded error text. The only
legal persistent transitions are absent to running, running to succeeded or
failed by the same owner, and expired running to needs-attention during
reconciliation. Before work starts, a dispatcher that cannot acquire the
workflow lease may owner-check and remove its claim so the occurrence remains
due. A
succeeded, failed, or needs-attention occurrence is never automatically claimed
again. A later due cron instant creates a distinct record. Retain bounded recent
history per workflow so cache growth is controlled.

Fifth, create `crates/jig/src/runtime/loops/codex_task.rs`. It reads the prompt,
resolves the Codex home, chooses the repository root or an isolated worktree,
and calls the shared worker runner with `approval_policy = never`, the typed
sandbox, optional model, ephemeral execution, and receipt purpose
`scheduled_codex_task`. It returns a normal `WorkflowTick` action containing
the worker receipt, checkout path, Codex home, output summary, and cleanup
result. Manual tick runs it immediately with a manual item key. Scheduled
dispatch supplies the occurrence id so attempts and receipts correlate with
that run. Known worker failure records the failed occurrence; an ambiguous
expired claim is not retried.

Sixth, add `LoopCommand::Dispatch`, the `jig loop dispatch` CLI subcommand, and
a request with no arbitrary tuning overrides. `schedule::dispatch_due` resolves
all enabled configured workflows with schedules, reconciles stale claims,
calculates due occurrences, atomically claims each, and invokes the engine with
the occurrence context. It must continue evaluating other workflows after a
known per-workflow failure and return a structured summary whose overall status
reports acted, idle, partial failure, or needs attention. The command records a
batch receipt linking individual loop and worker receipts. Manual tick and
existing run behavior remain byte-compatible apart from additive workflow
fields.

Seventh, extend loop status with additive schedule facts. Each workflow value
will include configured cron and timezone, computed next due time, the latest
occurrence, whether it is due or overdue, and any retained worktree. The
aggregate status automatically preserves the richer loop JSON. Update Jig UI
model types and dashboard HTML to render next run, recent status, receipt id,
and attention state without reading runtime state directly. Update human CLI
formatters for both status and dispatch.

Finally, update documentation and run the full repository workflow. Build the
development binary before every Jig dogfood command. Keep `.agent/state/*.jsonl`
append-only, update this plan after each milestone, and close structured work
only after required gates prove the current fingerprint.

## Concrete Steps

All commands run from
`/home/aa/.herdr/worktrees/jig-sh/feat-vault-tui`.

Establish and repeat the focused baseline while editing:

    cargo fmt --all -- --check
    cargo test -p jig-sh --locked runtime::tests::loops
    cargo test -p jig-sh --locked context::tests
    cargo test -p jig-ui --locked

After configuration and schedule parsing, run their exact new tests by name and
expect all cases to pass with no ignored failures. After command wiring, build
the current runtime and exercise a fixture with a fake `JIG_CODEX_BIN`:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig loop status --json

The repository itself need not configure a real scheduled task. Integration
tests create temporary repositories, fixed schedules, prompt files, and fake
Codex executables. One test manually ticks a task, one dispatches a due task,
one dispatches twice and observes one worker run, one coalesces missed runs, and
one expires a running claim and observes `needs_attention` without a rerun.

Before completion run:

    cargo fmt --all -- --check
    cargo clippy -p jig-sh --all-targets --locked -- -D warnings
    cargo test -p jig-sh --locked
    cargo test -p jig-ui --locked
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig check test
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0JYYAMERNX7BE6BTK635MRM
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0JYYAMERNX7BE6BTK635MRM
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0JYYAMERNX7BE6BTK635MRM

Then inspect `git diff --check`, every untracked file, CLI help, JSON examples,
and the requirement audit before using `scripts/jig work finish`.

## Validation and Acceptance

Configuration acceptance is proven when an existing repository without new
keys still loads unchanged, a valid scheduled task round-trips through Jig
update, and each invalid kind-specific field produces a targeted error.

Lease acceptance is proven when a lease remains held beyond its original TTL
while its guard is active, a competing owner cannot acquire it, release is
owner-checked, and a simulated crashed owner becomes acquirable after expiry.

Scheduling acceptance is proven with a fixed clock: a due workflow runs once,
immediate redispatch is idle, downtime coalesces to the newest missed instant,
the next occurrence is stable in the configured timezone, and an expired
running claim becomes needs-attention without invoking the worker.

Codex task acceptance is proven with a fake executable that records its argv,
cwd, environment, and stdin. The test must observe `exec`, `--ephemeral`,
`--ask-for-approval never`, the configured narrow sandbox and model, the exact
prompt bytes, the resolved `CODEX_HOME`, and the expected repository or
worktree cwd. A clean successful worktree is removed; a dirty or failed one is
retained and reported.

Observability acceptance is proven when `loop status --json`, aggregate status,
human output, and the HTML dashboard all expose the same scheduled workflow's
next due time, latest occurrence status, receipt link, and attention condition.

Compatibility acceptance requires all prior loop tests to pass without
assertion weakening, existing command output fields and cache file formats to
remain readable, and every required Jig gate to be fresh and passing.

## Idempotence and Recovery

Configuration parsing and schedule evaluation are read-only and repeatable.
`loop dispatch` is intentionally safe to repeat because occurrence claim and
terminal state updates happen under the schedule-state file lock. A second
process either sees the running claim or a terminal occurrence and skips it.

If Jig exits while Codex is running, the workflow lease and occurrence claim
expire. The next dispatch reconciles the occurrence to needs-attention and does
not rerun it. The operator can inspect the retained worktree and worker receipt.
If a future explicit recovery command is added, it must create an auditable new
attempt rather than rewriting the terminal occurrence; such a command is not
part of this plan.

Worktree creation uses a repository-local cache path and a unique detached
checkout. On setup failure, remove only the path created by that
attempt after verifying their exact names. Never remove a dirty retained
worktree automatically. Re-running tests uses fresh temporary repositories.

Do not hand-edit or truncate `.agent/state/*.jsonl`. Jig commands append their
own records. Source edits use targeted patches and unrelated user changes are
preserved.

## Artifacts and Notes

The intended user configuration is:

    [loop]
    lease_ttl_seconds = 900
    max_attempts = 3
    backoff_seconds = 300

    [[loop.workflows]]
    id = "nightly-maintenance"
    kind = "codex_task"
    schedule = "0 2 * * *"
    timezone = "Europe/Prague"
    prompt_file = ".agent/tasks/nightly-maintenance.md"
    codex_home = "work"
    checkout = "worktree"
    sandbox = "workspace-write"

An external scheduler can safely invoke:

    scripts/jig loop dispatch

more frequently than the configured cadence. Expected human output identifies
the number of due, executed, skipped, failed, and attention-requiring workflows.

## Interfaces and Dependencies

Add workspace dependencies for `chrono`, `chrono-tz`, and exact `croner` 3.0.1.
The Jig crate consumes them only from the loop schedule boundary.

In `crates/jig/src/runtime/loops/schedule.rs`, define a schedule value that can
parse config and calculate the most recent due and next future UTC
milliseconds. Its public-within-subsystem interface must accept an explicit
`now_ms`; production passes `crate::state::now_ms`, while tests pass constants.

In `crates/jig/src/runtime/loops/state.rs`, provide owner-checked lease renewal
and schedule-store transitions. The claimant owner token must be created once,
persisted in the occurrence, carried into engine execution, and checked by
completion/failure. No alternate mutation path may bypass the owner check.

In `crates/jig/src/runtime/loops/workflow.rs`, represent resolved schedule and
Codex task settings as typed values rather than repeatedly interpreting raw
strings. Existing workflows keep `None` for task settings.

In `crates/jig/src/runtime/loops/engine.rs`, add an internal tick context that
distinguishes manual from scheduled execution and carries the occurrence id.
The external manual request continues to call the same stable facade.

In `crates/jig/src/runtime/loops/codex_task.rs`, expose one task tick function
that receives the resolved workflow and execution context and returns
`WorkflowTick`. It must use `crate::runtime::worker_runner::run_codex_exec`; it
must not spawn Codex directly.

In `crates/jig/src/command/loops.rs` and `crates/jig/src/cli/loops.rs`, add the
dispatch request and subcommand. Human output belongs in
`crates/jig/src/cli/output/loops.rs`.

In `crates/jig-ui/src/model.rs`, extend deserializable loop view types only with
additive optional/defaulted fields so older snapshots remain decodable. Render
them in `crates/jig-ui/src/html/dashboard.rs` through escaped text.

Plan revision note (2026-08-21T20:07Z): replaced the one-line bootstrap body
with the full end-to-end implementation and recovery plan because items 1–7
span durable state, concurrency, CLI compatibility, external worker execution,
and two presentation boundaries.
