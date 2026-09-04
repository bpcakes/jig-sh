# Halve the Dogfood Test Suite Again

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` current as work proceeds.

Plan ID: `plan_01M141YZV6BYKT7QF8BDCS9B7Y`

## Purpose / Big Picture

The first dogfood-speed effort reduced the complete local Rust gate from a
recent 916.212-second receipt median to 374.14 seconds while preserving all
3,109 tests. This follow-up must reduce the merged 374.14-second result by at
least another 50 percent, to no more than 187.07 seconds, without removing,
ignoring, weakening, or replacing any test.

The merged local gate invokes four exact Nextest partitions sequentially. Its
dominant frontend partition is restricted to two workers even on large
development machines. This work replaces fixed/sequential local scheduling
with one comprehensive Nextest run whose worker count follows available logical
CPUs. Tests with actual shared-resource constraints remain isolated. The
default profile and locked CI stay conservative and comprehensive.

Validation must cover both development hosts: the MacBook Pro M5 Max exposes
16 logical CPUs; the 32-core Threadripper Linux workstation with 128 GiB RAM
exposes 64. The active Linux checkout contains unrelated work, so remote
validation must use an isolated temporary clone and leave it untouched.

## Progress

- [x] (2026-08-28) Confirmed the prior PR was merged at `73075bee`; opened
  `perf/dogfood-suite-speed-stage-2`, built `target/debug/jig`, opened this
  structured plan, and claimed Beads feature
  `feat-codex-resume-generic-monorepo-iwn`.
- [x] (2026-08-28) Re-profiled all 107 frontend tests at 252.89 seconds and
  identified the Yarn/pnpm generated-script long tail.
- [x] (2026-08-28) Proved a local-only four-worker frontend group passes in
  147.56 seconds and a unified four-worker 3,109-test run passes in 229.58
  seconds.
- [x] (2026-08-28) Confirmed pinned Nextest 0.9.130 supports
  `test-threads = "num-cpus"`; the Mac reports 16 logical CPUs and Linux 64.
- [x] (2026-08-28) Proved all 107 frontend tests pass with CPU-count scheduling
  on the Mac in 83.16 seconds.
- [x] (2026-08-28) Fixed the first high-load PID publication race and passed
  its complete regression 50/50 times.
- [x] (2026-08-28) Passed three complete Mac runs in 135.70, 138.26, and
  139.60 seconds; the 138.26-second median is 63.0 percent below the
  374.14-second baseline and comfortably below the 187.07-second target.
- [x] (2026-08-28) Validated CPU-aware scheduling in an isolated Linux clone.
  After identifying excess external-process contention at 64 frontend workers,
  capped that resource class at 16 and passed all 3,089 platform-applicable
  tests in 122.99 seconds without disturbing the active Linux worktree.
- [x] (2026-08-28) Recounted the exact focused partitions: 2,365 core, 107
  frontend, 207 process, and 430 vault tests sum to the complete 3,109-test Mac
  set, with zero pairwise overlap.
- [x] (2026-08-28) Passed all eight configured structured gates, including all
  four focused test partitions, in batch receipt
  `receipt_01M145Z36Y619NKG6QKGWAPA0J`.
- [x] (2026-08-28) Inspected fresh evidence/receipts, closed and synced Beads,
  removed the isolated Linux benchmark clone, and prepared the follow-up pull
  request branch.

## Surprises & Discoveries

- Frontend remains the dominant cost: 252.89 seconds versus prior evidence of
  roughly 75 seconds for core, 27 for process/signal, and 16 for vault.
- The frontend long tail is real generated-script behavior: examples include a
  47.484-second pnpm workspace matrix, a 43.308-second dependency-authority
  matrix, 36-43-second Yarn state cases, and a 24.840-second Yarn authority
  case.
- Two to four frontend workers reduced wall time from 252.89 to 147.56 seconds
  with similar total CPU time. The suite had unused independent work.
- A unified four-worker run passed all 3,109 tests in 229.58 seconds. Sequential
  partition scheduling therefore costs substantial wall time, but four workers
  cannot reach the target.
- Sixteen Mac workers made one Yarn test exceed its 60-second slow threshold
  under contention, yet all frontend tests passed in 83.16 seconds. Individual
  latency rose while total throughput improved by 67.1 percent.
- The first unified 16-worker run found an invalid readiness assumption in
  `generated_web_checks_recover_interrupted_and_contended_stale_install_locks`:
  the shell creates `claim-ready` before writing its PID, so path existence can
  expose empty content. Waiting for a positive parsed PID fixes the actual race.
- The user's active Linux checkout is dirty on an unrelated vault branch.
  Cross-host evidence must be collected elsewhere.
- Letting all 64 Linux workers enter process-heavy frontend fixtures caused a
  timing-sensitive MCP test to miss its channel deadline. That exact MCP test
  passed 20/20 in isolation, identifying host oversubscription rather than a
  logic defect. A 16-member frontend ceiling retained global 64-worker
  scheduling, passed the full Linux suite, and reduced observed process RSS
  from roughly 1.8 GiB in the failed run to roughly 403 MiB.
- One Mac run reported a single leaky test classification, but a third full run
  with leak-level status enabled passed all tests and reported no leak. The
  classification was transient and not reproducible; the diagnostic run showed
  only five slow frontend fixtures.
- The structured core gate later reported one unnamed leaky classification.
  Two subsequent leak-level core reruns passed all 2,365 tests without a leak,
  including a clean 69.12-second non-interactive diagnostic. With no stable
  owner or failure and multiple non-reproductions, this is not an actionable
  race; it is Nextest's transient inherited-file-descriptor classification.

## Decision Log

- Decision: Keep `profile.default` at four threads and `frontend-node` at two.
  Rationale: Locked CI is stable with those limits. Local feedback optimization
  does not justify changing merge reliability.
  Date: 2026-08-28.

- Decision: Set `profile.local.test-threads = "num-cpus"` rather than naming
  hosts or hard-coding 16/64.
  Rationale: This is a native pinned-Nextest facility, respects the CPU set
  visible to containers, and scales to other developer machines.
  Date: 2026-08-28.

- Decision: Give isolated frontend fixtures a high but bounded local group
  ceiling of 16; retain the two-worker default group.
  Rationale: Sixteen passes quickly on the 16-thread Mac and the 64-thread
  Linux host. Sixty-four simultaneous nested shell/Node process trees caused
  deadline contention without increasing useful CPU utilization.
  Date: 2026-08-28.

- Decision: Give local vault TUI tests a separate group with
  `threads-required = "num-test-threads"`.
  Rationale: Their old four-thread reservation was globally exclusive only
  because the default had four threads. It must scale to preserve that invariant.
  Date: 2026-08-28.

- Decision: Change only the complete unlocked local command to one unified
  Nextest invocation. Keep all four focused partition commands and gates.
  Rationale: One scheduler can overlap independent subsystems while groups
  still serialize process/signals and constrain crypto/PTY work.
  Date: 2026-08-28.

- Decision: Fix load-exposed readiness races rather than hiding them with lower
  concurrency or arbitrary sleeps.
  Rationale: Content/identity readiness is correct independently of performance.
  Date: 2026-08-28.

## Outcomes & Retrospective

The implementation exceeds its performance target without weakening coverage.
Three green complete Mac runs took 135.70, 138.26, and 139.60 seconds (median
138.26 seconds), a 63.0 percent reduction from the 374.14-second baseline. The
isolated Linux clone passed all 3,089 platform-applicable tests in 122.99
seconds. Global local scheduling follows the host CPU count (16 on Mac and 64
on Linux), while external-process frontend work is capped at 16 and genuine
shared-resource groups retain their prior isolation.

The main win came from replacing four sequential complete-suite invocations
with one scheduler that overlaps independent subsystems. Increased frontend
parallelism removed the remaining idle interval. High concurrency also exposed
a real PID-publication race; waiting for a positive parsed PID fixed it and the
regression passed 50/50 stress iterations. The complete 3,109-test Mac set is
unchanged, and the focused filters remain exact and disjoint. All eight
configured structured gates and their evidence are green; the branch is ready
for pull-request delivery.

## Context and Orientation

Nextest configuration is `.config/nextest.toml`. `.jig.toml` owns the complete
`rust_test_command` and the locked CI command. `scripts/test-rust-partition.sh`
owns exact `core`, `frontend`, `vault`, and `process` modes used by path-scoped
dogfood gates. GitHub coverage lives in `.github/workflows/rust-tests.yml`.

Frontend tests live under
`crates/jig/src/bootstrap/tests/frontend_adoption`. They use generated
`scripts/check-webapps.sh` and `scripts/web-node.cjs` with fake package managers,
while preserving real Bash process, filesystem, fingerprint, lock, and
environment behavior.

Safety groups are `process-signals` (one), `vault-crypto` (four), vault TUI
(globally exclusive under the active profile), and frontend (two under default,
CPU-scaled under local). Local overrides must not change default assignments.

## Plan of Work

### Milestone 1: Scheduler evidence

Record per-test frontend timings. Benchmark the frontend partition at two, four,
and CPU-count workers. Benchmark unified scheduling. Each run must execute the
same 107 frontend or complete 3,109-test set; no fail-fast result counts as a
performance success.

### Milestone 2: CPU-aware local profile

Configure `profile.local` to use `num-cpus`. Add local frontend and vault-TUI
groups so frontend may scale while PTY deadlines stay globally exclusive.
Replace the complete unlocked sequential partition chain with one unified
Nextest command. Preserve focused commands, default profile, locked command,
and workflows.

### Milestone 3: Concurrency hardening

For every high-worker failure, determine whether it is a product race, fixture
race, or genuine resource constraint. Replace path-only readiness with bounded
content/identity readiness and stress the exact regression. Never delete
assertions, add arbitrary sleeps, mock the generated-script boundary, or
serialize unrelated work merely to obtain green output.

### Milestone 4: Mac and Linux acceptance

Run the unified suite repeatedly on the M5 Max. At least one complete run must
be at or below 187.07 seconds and every run used as evidence must pass all tests.
Report the distribution. Validate profile selection and the full suite in an
isolated temporary Linux clone, then remove that clone. Never reset, clean, or
switch the active remote checkout.

### Milestone 5: Gates and delivery

Format and lint Rust, rebuild the dev Jig binary, force harness checks through
`JIG_DEV_BIN`, run structured gates, inspect evidence and receipts, sync/close
Beads only after acceptance, and create a follow-up PR.

## Concrete Steps

Run from the repository root unless explicitly in the isolated clone:

    /usr/bin/time -lp scripts/test-rust-partition.sh frontend
    /usr/bin/time -lp cargo nextest run --workspace -P local \
      --status-level fail --final-status-level fail
    cargo nextest show-config test-groups --profile local
    cargo nextest show-config test-groups --profile default
    cargo fmt --all -- --check
    scripts/jig check clippy
    scripts/jig check test
    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig check contract
    scripts/jig work check --plan-id plan_01M141YZV6BYKT7QF8BDCS9B7Y
    scripts/jig work gates --plan-id plan_01M141YZV6BYKT7QF8BDCS9B7Y
    scripts/jig work evidence --plan-id plan_01M141YZV6BYKT7QF8BDCS9B7Y
    scripts/jig work receipts --plan-id plan_01M141YZV6BYKT7QF8BDCS9B7Y

## Validation and Acceptance

- Complete local command passes all existing 3,109 tests plus regressions.
- Complete Mac wall time is at most 187.07 seconds.
- Frontend manager/workspace/lock/PnP/receipt cases remain present and run.
- Process/signals stay serialized; vault crypto stays capped; vault TUI remains
  globally exclusive.
- Worker selection follows available CPUs on both named hosts.
- Default/locked CI configuration and workflows stay comprehensive/conservative.
- Every observed concurrency race has exact repeat evidence.
- Contract, format, Clippy, configured work gates, evidence, and receipts pass.

## Idempotence and Recovery

Benchmarks create temporary directories and can be rerun. Structured state is
append-only and must be changed only through `scripts/jig`. Remote validation
must use `mktemp -d` or a separate worktree. If a high-worker run fails, retain
the evidence, fix the smallest correctness issue, stress it, and rerun fully.

## Interfaces and Dependencies

No new Rust dependency is planned. New configuration uses pinned Nextest values
`test-threads = "num-cpus"` and `threads-required = "num-test-threads"`.
Focused partition modes/gate IDs remain stable. No application API, persisted
state, generated runtime contract, or production vault behavior changes.
