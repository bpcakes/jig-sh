# Reduce CI latency while preserving coverage

Reduce avoidable pull-request CI time without removing a platform, feature mode,
check, or test. This branch is independent of the agent-guidance PR and starts
at `689a512e`. Jig plan: `plan_01M39PB06X7RW9JGZHHPZQ44VH`.

## Progress

- [x] T-01: Inspect workflows and a successful hosted baseline.
- [x] T-02: Optimize test scheduling, formatter startup, and policy caches.
- [x] T-03a: Validate syntax and formatting; open PR #50.
- [x] T-03b: Verify every changed hosted job and report timings.

Checkpoint: every changed hosted job passed at `ae1624fc` in PR #50.
Implementation and validation are complete; local structured-work closure
remains unavailable without a full local test receipt. No further workflow
edits are needed. All hosted checks passed on the implementation commit.

## Surprises & Discoveries

[Successful Rust run 35994903614](https://github.com/bpcakes/jig-sh/actions/runs/35994903614)
on 2026-09-24 took about 32 minutes. Measured step durations:

| Step | Linux | macOS |
| --- | ---: | ---: |
| No-default-feature Cargo tests | 17m56s | 30m48s |
| Full workspace Nextest tests | 9m14s | 14m38s |

The macOS no-default library tests alone took 25m54s. Many tests hold the
process-global environment mutex in `crates/jig/src/test_env.rs`. Nextest runs
individual tests in separate processes; both jobs already install it.
The existing Nextest configuration limits process/frontend/crypto contention.
These are different suites, so the timing comparison is a hypothesis for the
optimization, not proof of its eventual speedup.

Formatting spent 108s building Jig and 7s checking formatting. Agent-map and
both file-budget jobs also compile Jig without any artifact cache.
Rendered fixtures took 16m37s, with several real source installation builds in
a temporary target directory that is outside the existing cache.

## Decision Log

- 2026-09-24: Use Nextest for no-default unit/integration tests, preserving the
  existing non-vault, vault, and exclusive PTY phases from `test-locked`.
  Keep `cargo test --doc` because Nextest does not execute doctests.
  Retain check names, job IDs, feature flags, platforms, and macOS Bash checks.
- 2026-09-24: Run the exact configured format script directly. Launcher and
  current-source runtime coverage remain in the existing jobs.
- 2026-09-24: Add the existing pinned Rust cache action to agent-map and
  file-budget jobs, with separate keys by OS and job. Do not conflate different
  feature/build modes or turn a cache hit into permission to skip compilation.
- 2026-09-24: Defer fixture caching: real source installs have separate target
  and binary-path assumptions. Preserve benchmark qualification and dedicated
  dev-proxy Cargo tests, which exercise shared-process signal behavior.

## Outcomes & Retrospective

Actionlint, direct formatting, and whitespace checks passed. Independent review
found no coverage gaps: the three test selections are exhaustive and disjoint
for this package, and Cargo doctests remain.

The broad local `work check` ran every profile target even for workflow-only
changes. Its test command failed in proxy fixtures because the ambient umask
created group-writable directories. Committing during that run also invalidated
the parallel layer's receipts; this was an execution mistake, not a code defect.
A stable rerun passed fmt, Clippy, contract, file-budget, and source-runtime checks;
read-only gate inspection confirmed those five passes fresh. The 12 proxy
management tests passed separately under `umask 077`. No full local test pass
is claimed, and the structured plan must remain open until that required receipt
is available. Hosted validation directly exercises the changed test scheduling.

[Hosted run 35999815450](https://github.com/bpcakes/jig-sh/actions/runs/35999815450)
passed every changed job at implementation commit `ae1624fc`:

| Measurement | Baseline | After | Observed reduction |
| --- | ---: | ---: | ---: |
| Linux no-default test steps, including compilation and doctests | 17m56s | 9m42s | 46% |
| macOS no-default test steps, including compilation and doctests | 30m48s | 19m58s | 35% |
| Complete formatting job | 2m13s | 21s | 84% |

Linux passed 3,393 tests; macOS passed 3,375. Both doctest steps passed (zero
current doctests). Both file-budget jobs and agent-map passed; their logs
confirmed no existing cache, so warm-cache savings are not yet measured.
The full Linux suite, rendered fixtures, Clippy, MSRV, launcher, generated
scaffold, and dev-proxy checks passed. The full macOS workspace suite also passed.
These runs have different source revisions and runner load; comparisons are
observational and exclude queue time, not a controlled performance benchmark.

## Work and acceptance

T-01 has no dependencies; output is the baseline above. T-02 depends on T-01
and changes only `.github/workflows/{rust-tests,repo-policy,agent-map-check}.yml`.
T-03 depends on T-02: run actionlint on those workflows, check the formatter,
and use `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id
plan_01M39PB06X7RW9JGZHHPZQ44VH` for applicable repository gates. Inspect evidence
before repeating gates. No Rust source changes require a second broad local
backend suite; hosted runs directly validate the scheduling change.

Acceptance requires preserved test selection and doctests, passing changed
jobs on Linux/macOS, and a separate reviewable PR reporting observed timings
and limitations. Compare hosted job steps rather than treating queue time as
execution. Cache misses must still build and test normally. Revert the three
workflow edits to recover; no persisted application state is changed.

Sources: [Nextest execution and doctests](https://nexte.st/docs/running/) and
[Rust cache semantics](https://github.com/Swatinem/rust-cache).
