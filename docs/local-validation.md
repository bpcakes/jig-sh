# Local validation in the Jig source repository

## Ordinary tasks

Choose checks for the affected behavior. For example, run a focused Rust regression:

```sh
cargo test -p jig-context inputs_policy
```

Use `scripts/jig info targets` to discover configured checks, then run the relevant
`scripts/jig check COMPONENT:ACTION` targets directly. Broaden validation when
shared behavior, failures, or unresolved risks warrant it.

To run every Rust test a change can affect, and only those, use the
dependency-scoped selection:

```sh
scripts/jig check repo:source-affected-test
# or directly, with a different comparison base or a dry run:
scripts/test-rust-affected.py --base HEAD --print
```

A file counts as changed when a commit since the merge base of `origin/master`
touched it, even if the working tree has since restored its original contents,
or when it is staged, unstaged, or untracked. Each changed workspace package
selects its own tests and those of every package that depends on it, through
nextest's `rdeps()`. Packages whose tests read another package's files without
depending on it are listed in the script's `EXTRA_CONSUMERS` table; a guard test
in `scripts/tests/test_jig_rust_affected.py` fails when a crate names another
crate's directory without such an edge.

Files outside every crate select the tests that read them, as listed in the
script's `OUTSIDE_PACKAGE_READERS` table. For example, `templates/` selects
jig-bootstrap, which embeds the templates, and its dependents, while `docs/`,
`README.md`, `CHANGELOG.md`, and `agent-map.md` select jig-sh's
`ui_architecture` tests. Every change to a tracked file also selects jig-sh's
`supported_host_surface` tests, which scan every tracked file. Paths that no
test reads, such as `.agent/state/`, `.beads/`, other workflows, and Python
tests, select nothing else. Build and test-harness inputs run the whole suite:
`Cargo.toml`, `Cargo.lock`, the toolchain, `.config/`, `.gitattributes`,
`.gitignore`, `.jig.toml`, `.agent/jig-contract.json`, the `scripts/jig` and
`scripts/jig-dev` launchers, the Rust Tests workflow, and its test scripts. So
does any path the table does not list. When you add a test that reads a file
outside its crate, make the file's entry select that test. A guard test fails
when a crate `include_str!`s a file whose entry does not select the crate; files
that tests read at run time are pinned by the table's unit tests instead. Python
`__pycache__` directories are ignored.
`jig-sh` depends on every crate, so its tests run for any Rust change.

## Before handing off a broad change

Run the inexpensive preflight profile before the configured full suite:

```sh
# Formatting, Clippy, contract, file budget and source-runtime validation.
scripts/jig check --profile preflight

# The complete verification profile, including `api:test`.
scripts/jig check --profile verify
```

A preflight failure is cheaper to fix before starting workspace tests. The
preflight profile is an execution convenience, not a dependency of tests or an
additional delivery requirement; direct `scripts/jig check test` remains
available. Each run executes its targets; Jig does not reuse earlier results.

Preflight runs the same formatting and policy checks as CI's Repo Policy job,
including two that plain Cargo commands miss: `scripts/check-rust-format.sh`
also rejects Rust sources spliced in with `include!` or named by position
(`part_NN.rs`, `tail.rs`, `*_parts/`), which `cargo fmt` would not reach, and
`repo:file-budget` compares with the merge base, so it fails when a file already
over its budget grows. Run it before pushing a change that moves or rewrites code.

When testing an edited implementation, build it first and select the resulting
binary with `JIG_DEV_BIN`, or use `scripts/jig-dev` for the individual checks.
An override must refer to the current build. Routine work can use the released
runtime selected by `.jig/source-runtime-version`.

## Source repository CI

Linux jobs, including the policy and release jobs, use GitHub-hosted
`ubuntu-latest` runners. macOS jobs use GitHub-hosted `macos-latest` runners.
The repository's `ci_github_runner` setting in `.jig.toml` records the Linux
runner selection.

On pull requests the Rust Tests workflow runs three jobs: Linux tests, macOS
tests, and rendered fixtures. Generated-Rust Clippy validation and rendered
fixtures share a job and development binary, with both generated-project
toolchains installed before cache restoration. `validate-fixtures.sh` accepts an
already-built `JIG_DEV_BIN`; as with the launcher, the caller is responsible for
its freshness. Nightly, merge queue, and manual test runs build and validate the
source runtime through `scripts/jig-dev check contract`, then run `scripts/jig
check test-locked`, which avoids a final development rebuild after test-only
dependency features have been enabled. Pull requests skip that build, which the
tests cannot reuse, and run the same `bash scripts/ci/test-rust.sh workspace`
command directly, the way the launcher does: from the checkout root, with
`JIG_REPO_ROOT` set to it and no stdin, failing if the tests change the
checkout. `scripts/tests/test_jig_ci_tests.py` fails when that copy, or the
macOS step's time limit, no longer matches `.jig.toml`. Repo Policy runs
`scripts/jig-dev check contract` on Linux after an identical build, so it
compiles nothing more, and the macOS job runs `scripts/jig check contract` with
the test build's binary, so the launcher and installer still run on macOS.

On pull requests, the Linux and macOS `test-locked` jobs still compile every
target but run only the tests the pull request can affect: they set
`JIG_TEST_AFFECTED_BASE` to the merge commit's first parent, and
`scripts/ci/test-rust.sh` intersects each phase with
`scripts/test-rust-affected.py --filterset`. Nightly runs, merge queues, and
manual runs leave it unset and run the whole suite. The selection is printed in
the job log.

The macOS job on pull requests also sets `JIG_TEST_EXCLUDE` to a Nextest
filterset that `scripts/ci/test-rust.sh` removes from every workspace phase:
jig-bootstrap's platform-neutral tests, which take about half of macOS test
time. They render templates, adopt repositories, and run generated toolchain
checks, including the package-manager adoption scenarios. Pull requests keep
jig-bootstrap's tests of paths where macOS behaves differently: git and
`/private/var` path aliases, no-replace publication, flock guards, open-file
accounting through `/dev/fd`, installer snippets and generated scripts under
Bash 3.2, and `ps`-based install lock identity. A guard test in
`scripts/tests/test_jig_ci_tests.py` fails when a kept pattern stops matching a
test. The Linux job still runs every affected jig-bootstrap test on pull
requests, and the nightly macOS run keeps them all, including their Node 24
coverage. The macOS job also runs the launcher parity check with Apple's stock
Bash 3.2 and the source-runtime unit tests.

Merges to `master` do not start CI. The Rust Tests and Repo Policy workflows run
on `master` each night instead (04:23 UTC), covering every job, platform, and
feature mode at once. Each run's first job, `scripts/ci/nightly-scope.py`,
compares `master` with the commit that workflow's last completed nightly run
tested, and the run skips its other jobs when nothing has landed since.
Cancelled nightly runs do not count as tested, and lookup failures run the
suite. Nightly runs on `master` also refresh the dependency caches that pull
requests restore. GitHub sends scheduled-run failure notifications to whoever
last changed the workflow's `cron` line, so a failing `master` is reported the
morning after the merge that broke it rather than right away.

The Repo Policy workflow builds Jig once on each of Linux and macOS, then reuses
that binary for Clippy and file-budget checks. Pull requests run only the Linux
job; nightly, merge queue, and manual runs add macOS. Each platform job also
runs no-default-features Clippy and the serial dev-proxy test harness, which
keeps process-global signal coverage alongside the workspace's process-per-test
Nextest coverage; workspace Clippy covers jig-dev-proxy itself. On pull requests
the serial harness runs only when `scripts/test-rust-affected.py --selects
jig-dev-proxy` reports that a change reaches jig-dev-proxy or a workspace
package it depends on, or when the workflow itself changes. The Linux job also
validates the contract through `scripts/jig-dev` and the launcher, the agent
map, and the tracker triage adapter. It runs the checks that need no job of
their own, too: rustfmt with the module-split check, launcher and installer
parity, the source-runtime unit tests, the MSRV 1.88 `cargo check`, and the
no-default-features build, tests, and doctests. Its path filters include every
Rust Tests input plus policy and agent-guide inputs, so each pull request that
starts Rust Tests starts this job too, and it finishes well before the slowest
Rust Tests job. Each platform's checks share one cache containing their distinct
Cargo configurations. rust-cache never saves again over a cache that matched its
key exactly, so the key changes whenever the job adds a configuration, and every
Linux run installs the MSRV toolchain so that all of them share a key. Nightly
and manual runs save the cache even when a check fails, because only they save
the cache that pull requests restore. When every changed path is
`.beadroll.json`, an `AGENTS.md`, or `agent-map.md`, only Linux repository
policy checks run, without formatting, Clippy, compatibility, or proxy tests.
Unknown paths, unavailable comparison trees, manual runs, and merge queues
retain full policy coverage. The generated project workflows retain their own
layout in `templates/project/.github/workflows/`.

Manual and nightly policy runs compare the selected ref with its merge base
against the remote default branch, so they also work without a local `master`
branch. On `master` itself, the comparison base is the same commit.

The locked test suite uses `scripts/ci/test-rust.sh workspace`. The Linux
minimal-feature tests in Repo Policy use `minimal-focused` on pull requests and
`minimal` on nightly, merge queue, and full manual runs; the macOS
minimal-feature job in Rust Tests runs only `minimal`, and not on pull requests.
Both modes compile every minimal-feature test target; focused mode executes
CLI/JSON and launcher-repair integration tests, proxy-disabled dispatch and
discovery, runtime compatibility, configuration and TLD validation, and
template/runtime repair tests. Full default-feature tests still run on both
platforms for every PR, and all-target minimal Clippy on Linux. This
intentionally reduces repeated minimal-feature and macOS coverage on PRs;
nightly runs and merge queues retain the exhaustive matrix.

For exhaustive execution, each invocation builds the complete selected set
of test binaries once, then passes Cargo and binary metadata to Nextest for
the non-vault, vault, and serial vault-PTY phases. Test-induced Git index
refreshes cannot cause an intervening rebuild. Each phase retains the default
test-group limits, and a failed phase still fails the job after the remaining
phases finish. Separate JUnit reports are saved under
`.agent/.cache/test-reports/` and uploaded for seven days. Set
`JIG_TEST_REPORT_DIR` to override the local report destination.

Generated frontend tests pin Node 22 on Linux and Node 24 on macOS. Their
package-manager scenario matrices are separate tests so Nextest can schedule and report each manager
independently, while retaining every scenario and the two-test frontend limit.
The local vault partition continues to use identical workspace features for
both of its phases.

The separate [Ubicloud trial](https://github.com/bpcakes/jig-sh/pull/73) compared
runner sizes on 2026-10-03 at `b8d34b0c`, using two successful runs per size of
the formatting and MSRV jobs, which Repo Policy has since absorbed.
The following results describe that provider trial; they do not describe the
GitHub-hosted runners used by this configuration:

| Job | 2-core durations | 4-core durations | Total estimated 2-core / 4-core cost |
| --- | --- | --- | --- |
| Formatting and launcher | 33s, 30s | 36s, 42s | $0.004 / $0.008 |
| MSRV | 47s, 85s | 46s, 70s | $0.006 / $0.012 |

Runs: [2-core A](https://github.com/bpcakes/jig-sh/actions/runs/37154621162),
[4-core A](https://github.com/bpcakes/jig-sh/actions/runs/37154714188),
[2-core B](https://github.com/bpcakes/jig-sh/actions/runs/37154821090),
[4-core B](https://github.com/bpcakes/jig-sh/actions/runs/37154976390).
MSRV restored dependency caches. Estimates round each complete job up to a
minute at the published Premium rates of $0.002/minute for two cores and
$0.004/minute for four, before credits and storage. Both pairs crossed the
same billing boundaries, so two cores halved their sampled compute cost.
These are a small warm-cache sample, not an invoice or a cold-build forecast.

The Linux full-test and release jobs start a systemd user manager and export its
bus address before testing proxy shutdown. These tests exercise the real service
manager; an unreachable manager still blocks proxy shutdown.

Rendered-fixture and generated-Rust compilation artifacts are cached separately
from the source workspace. CI sets absolute `JIG_FIXTURE_TARGET_DIR` and
`JIG_GENERATED_RUST_TARGET_DIR` paths under `.agent/.cache/`; fixture repositories and
installation roots remain temporary. The fresh-Cargo-home Git installation test
also keeps a separate temporary target so its different registry paths cannot
invalidate the shared dependency artifacts. The CI fixture job sets
`CARGO_PROFILE_RELEASE_OPT_LEVEL=0` for ordinary installation fixtures, which test
paths, profiles, and compatibility. The isolated Git installation clears that
override and exercises the normal optimized release profile, except on pull
requests: there the job sets `JIG_FIXTURE_GIT_INSTALL_INHERIT_OPT_LEVEL=1`, and
the Git installation keeps the lowered opt-level, because building it optimized
from a fresh Cargo home took about four minutes. Release validation
does not set the override: all its installation fixtures retain production
optimization. Cache keys include the Cargo profile environment, so these
configurations cannot share incompatible artifacts. Without these overrides,
local checks retain their existing temporary build-directory behavior.

The release workflow commits prepared files locally, then validates that commit
once before any push or publish. `RELEASE_VALIDATION_RECEIPT` lets tag and publish
reuse that success only for the same version, commit, GitHub job, run and attempt,
with a clean working tree except for the existing explicit run-journal allowance.
A failed recheck removes prior evidence. Standalone release commands without this
override still perform full validation.

The Release binaries workflow builds native archives on Linux and macOS for both
x86-64 and ARM64. Changes to binary distribution tooling also build the x86-64
Linux archive on pull requests; releases and manual runs build all four.
Repository policy and each release target run
`scripts/check-vault-production-features.py` to reject `jig-vault/test-utils`
in the production Cargo dependency graph. The guard excludes dev-dependencies;
isolated test-support builds remain available. Run its real-Cargo regression
fixtures with `python3 -m unittest discover -s scripts/tests -p 'test_vault_production_features.py'`.
The `test_jig_binary_distribution.py` unittest module exercises
cold installs without Cargo, checksums, host selection, fallback, publication
retries, and cache preservation. `test_jig_standalone_install.py` covers first-time
installation, latest-release selection, host requirements, and safe replacement. On stable tags,
`scripts/smoke-release-binary.py` also installs the actual compiled executable
with local asset transport and no Cargo on PATH. Development builds run both
native profile probes; stable pins deliberately reject development versions.
When testing a binary built from another tag, pass `--release-source PATH` to
use that checkout's contract. `test_jig_release_smoke.py` covers older contracts,
failed probes, and the standalone-only historical `v0.1.0` exception (which
predates repository compatibility probes).
