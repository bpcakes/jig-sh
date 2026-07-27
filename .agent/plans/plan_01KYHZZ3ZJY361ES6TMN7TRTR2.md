# Run and aggregate status providers

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` current as implementation
continues.

## Purpose / Big Picture

Jig already publishes the language-neutral `jig.status-provider/v1` report
contract, but it cannot yet execute a project-owned provider. After this work, a
repository can configure one or more provider argv arrays in `.jig.toml` and run
`scripts/jig status` (or `scripts/jig status --json`) to inspect a fresh,
validated aggregate. The aggregate combines the unchanged provider reports with
Jig-owned repository freshness, structured work state, gate state, loop leases,
and attempts. A failed or partial provider remains visible as partial status
instead of making the whole inspection disappear. This snapshot is the stable
runtime boundary a later terminal UI can consume.

The user-visible proof is an adopted fixture whose configured provider is
executed from the repository root, whose report is semantically validated, and
whose input revision is compared with the current checkout. The same command
must also surface a provider timeout, nonzero exit, malformed JSON, excessive
output, or identity mismatch without trusting that invocation's stdout.

## Progress

- [x] (2026-07-27) Read the repository, crate, and ExecPlan instructions.
- [x] (2026-07-27) Inspected the CLI, strict repository configuration, renderer
  answer round-trip, public v1 contract, work/loop snapshots, Git helpers, and
  owned-process-tree supervisor.
- [x] (2026-07-27) Opened plan
  `plan_01KYHZZ3ZJY361ES6TMN7TRTR2` and its structured work session.
- [x] (2026-07-27) Added round-trippable status-provider configuration and
  validation.
- [x] (2026-07-27) Implemented provider execution, v1 decoding/semantic
  validation, input
  freshness, and aggregate snapshot construction.
- [x] (2026-07-27) Added the top-level `jig status` CLI and concise human
  rendering.
- [x] (2026-07-27) Added unit/integration tests and updated generated template
  snapshots.
- [x] (2026-07-27) Updated configuration, protocol, public-contract,
  developer-UX, README, and repository-intent documentation.
- [ ] Build the development binary, dogfood the new command, run the relevant
  checks and final Jig gates, then close this plan.

## Surprises & Discoveries

- Observation: Jig's `doctor` implementation already owns a cross-platform
  process-tree supervisor with bounded nonblocking stdout/stderr drains,
  timeouts, Unix process-group cleanup, and Windows Job Object cleanup.
  Evidence: `crates/jig/src/doctor.rs` exposes
  `run_owned_process_tree_with_output`; runtime agent and dev-proxy probes
  already reuse it.

- Observation: `jig loop status` is a read-only snapshot and already exposes
  configured workflows, active leases, attempts, waiting attempts, and
  exhausted attempts. The aggregate should embed this result instead of
  reproducing loop-store rules.

- Observation: `.jig.toml` is both strict runtime configuration and the renderer
  answer file. Adding only a `RepoConfig` field would make `jig update --recopy`
  silently omit provider configuration because `RawAnswers` and
  `RenderAnswers` currently do not carry it.

- Observation: provider reports for real rewrites can be much larger than the
  supervisor's 16 KiB diagnostic default. The reusable supervisor needs an
  explicit caller-supplied limit; the status runner caps provider stdout at
  8 MiB and stderr at 64 KiB.

- Observation: several long-lived Jig source and test modules predate the
  current Rust LOC ceiling. Growing them, even for small status additions,
  fails the repository's changed-file policy.
  Resolution: moved answer tests and status output into focused modules, kept
  status config tests beside the config type, and exposed the existing
  supervisor's internal capture-limit types without increasing `doctor.rs`.

## Decision Log

- Decision: implement the reusable status snapshot and CLI now, and leave the
  interactive terminal UI and Codex launcher for later milestones.
  Rationale: configuration, safe execution, validation, and aggregation are the
  dependency boundary both features need.
  Date/Author: 2026-07-27 / Codex

- Decision: configure providers as `[[status.providers]]` entries with a stable
  `id`, an exact `argv` array, and optional `timeout_seconds`.
  Rationale: argv avoids implicit shell parsing and makes the executed boundary
  auditable; a per-provider timeout supports inspectors with different costs.
  Date/Author: 2026-07-27 / Codex

- Decision: preserve a successfully decoded v1 report unchanged under each
  provider result, and place Jig-owned summaries and input freshness beside it.
  Rationale: the open provider contract remains producer-owned while consumers
  can use normalized aggregate facts without mutating or extending the report.
  Date/Author: 2026-07-27 / Codex

- Decision: top-level `ok: true` means the inspection command completed;
  `outcome: "partial"` records unavailable Jig sections, failed providers, or
  provider reports that are themselves partial. Domain blockers, failed gates,
  dirty inputs, and stale revisions are valid observed facts and do not by
  themselves make collection partial.
  Rationale: this matches existing `work gates` inspection semantics and lets a
  UI render failures rather than losing the snapshot to a nonzero command.
  Date/Author: 2026-07-27 / Codex

- Decision: compare Git input revisions only with local checkout state and local
  tracking refs. Do not fetch remotes from `jig status`.
  Rationale: status observation stays read-only, deterministic with respect to
  local state, and free of network latency/authentication side effects. The JSON
  must label tracking freshness as local.
  Date/Author: 2026-07-27 / Codex

## Outcomes & Retrospective

Not complete yet.

## Context and Orientation

The public wire DTOs and semantic validator live in
`crates/jig-contract/src/status_provider.rs` and
`crates/jig-contract/src/status_provider/v1.rs`. That crate must remain free of
configuration and process execution.

The CLI parser and dispatch live in `crates/jig/src/cli.rs` and
`crates/jig/src/cli/run.rs`; human/JSON rendering lives in
`crates/jig/src/cli/output.rs`. Top-level runtime-only commands such as `ui` do
not appear in the generated MCP command manifest. `status` follows that model.

Strict runtime `.jig.toml` deserialization lives in
`crates/jig/src/context.rs`. Renderer answer deserialization and serialization
live in `crates/jig/src/bootstrap/answers.rs`, and the generated file is
`templates/project/.jig.toml.jinja`. Provider configuration must traverse all
three paths so update/recopy preserves it.

Structured work summaries are produced by
`crate::state::state_summary`. Per-plan gates are evaluated read-only by
`crate::runtime::work_gates_snapshot`, and loop runtime state is exposed by
`crate::runtime::loop_status_snapshot`.

The owned child lifecycle is in `crates/jig/src/doctor.rs`. It deliberately
terminates the owned process tree even after the direct child exits, preventing
background descendants from keeping capture pipes or work alive. Status will
reuse its public-within-crate custom-limit types rather than copying that
implementation.

## Plan of Work

First, add serializable/deserializable `StatusConfig` and
`StatusProviderConfig` types near repository configuration. Runtime validation
will reject duplicate or unsafe ids, empty argv, NUL-bearing arguments, zero or
unreasonably large timeouts, and an excessive provider count. Add the config to
`RepoConfig`, expose it through `RepoContext`, carry it through
`RawAnswers`/`RenderAnswers`, and render an explicit empty provider list or the
configured array-of-tables.

Second, add a `status` module to the `jig` crate. For every configured provider,
construct `std::process::Command` directly from its argv, set the repository
root as CWD, close stdin, and capture bounded stdout/stderr through the existing
owned-process-tree supervisor. Require exit zero, complete untruncated UTF-8
stdout, exactly one JSON value, successful `v1::Report` deserialization and
semantic validation, and an exact configured/provider id match. Map every
failure to a stable error code and bounded diagnostic rather than merging its
stdout.

Third, collect a repository snapshot (HEAD, branch, dirty state, and
ahead/behind counts against an existing local upstream), existing structured
work state, per-open-plan gate snapshots, and loop status. After all provider
runs, compare each Git input's declared revision with the relevant root or
repository-relative Git checkout and classify it as current, dirty, stale,
unknown, or unavailable. Construct a versioned aggregate with stable ordering,
provider summaries, section errors, `ok`, and observation `outcome`.

Fourth, add `jig status` to the CLI constants/parser/dispatcher and a concise
human renderer that reports aggregate completeness, repository state, active
work/leases, and provider package/blocker/freshness totals. Global `--json`
prints the full aggregate. The command remains read-only and records no receipt.

Finally, add config, parser, decoder, process-failure, aggregate, and formatter
tests. Regenerate embedded template snapshots using the repository's existing
generator rather than hand-editing them. Update the docs that currently say no
runner exists, document the exact configuration/process/freshness semantics,
and verify both default and no-default-feature builds through the normal Jig
checks.

## Concrete Steps

From `/Users/aa/Documents/jig-sh`:

1. Edit configuration, answer round-trip, and template files with
   `apply_patch`; run focused context/bootstrap tests.
2. Edit the process-supervisor API and add `crates/jig/src/status.rs`; run
   focused status and doctor tests.
3. Wire CLI parsing/dispatch/output and run focused CLI tests.
4. Regenerate embedded templates with the repository-provided command located
   during implementation, then inspect the generated diff.
5. Build and dogfood:

       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig status --json

6. Run formatting, Clippy, and tests through the development binary:

       JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
       JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
       JIG_DEV_BIN=target/debug/jig scripts/jig check test

7. Record evidence and run the plan-scoped Jig gate commands before finishing
   the work plan.

## Validation and Acceptance

Acceptance is behavioral:

- An absent `[status]` section loads and `jig status --json` succeeds with an
  empty provider list.
- A configured conforming provider runs from the repo root and its complete v1
  report appears intact in the aggregate.
- Configured status entries survive `jig update --recopy` rendering.
- Duplicate ids, empty argv, and invalid timeouts fail while loading
  `.jig.toml` with actionable messages.
- Nonzero exit, timeout, malformed/non-UTF-8/multiple JSON output, report
  semantic errors, identity mismatch, and output truncation produce failed
  provider results and top-level partial collection; none preserve stdout as a
  trusted report.
- Provider-reported `outcome: partial` remains a valid report and makes the
  aggregate partial.
- A root Git input at current HEAD is current when clean and dirty when local
  changes exist; a different revision is stale. A nested legacy checkout is
  compared at its repository-relative path.
- Work state, per-open-plan gates, loop leases, and attempts are present without
  appending state records.
- Human output is concise, while `--json` retains complete provider reports and
  stable error codes.
- `cargo test -p jig-contract` still passes, proving execution changes did not
  alter the public v1 wire contract.

## Idempotence and Recovery

`jig status` is read-only and safe to rerun. It does not fetch, append receipts,
change provider files, or cache reports. Provider commands are contractually
read-only and are always bounded by configured timeout and owned-tree cleanup.

Template snapshot regeneration is deterministic and may be rerun. If a provider
test leaves a child alive, stop and fix process cleanup before continuing; do
not weaken or skip that assertion. Existing user changes outside this plan are
not to be reverted.

## Artifacts and Interfaces

Expected public configuration:

    [[status.providers]]
    id = "factorish.hocr2.migration-readiness"
    argv = ["ruby", "scripts/verify_migration_readiness.rb", "--status-provider-v1"]
    timeout_seconds = 30

Expected CLI:

    scripts/jig status
    scripts/jig status --json

Expected aggregate discriminator and main sections:

    {
      "ok": true,
      "command": "status",
      "schema_version": 1,
      "observed_at_ms": 0,
      "outcome": "complete",
      "repository": {},
      "work": {},
      "loops": {},
      "providers": [],
      "errors": []
    }

Provider reports under `providers[].report` remain
`jig.status-provider/v1`. Runner/config/aggregate types remain in `crates/jig`;
`crates/jig-contract` remains the dependency-downward wire-contract crate.
