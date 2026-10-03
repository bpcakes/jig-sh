# Local validation in the Jig source repository

## Ordinary tasks

Choose checks for the affected behavior. For example, run a focused Rust regression:

```sh
cargo test -p jig-sh --lib repository::freshness::tests
```

Use `scripts/jig info targets` to discover configured checks, then run the relevant
`scripts/jig check COMPONENT:ACTION` targets directly. Broaden validation when
shared behavior, failures, or unresolved risks warrant it.

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

When testing an edited implementation, build it first and select the resulting
binary with `JIG_DEV_BIN`, or use `scripts/jig-dev` for the individual checks.
An override must refer to the current build. Routine work can use the released
runtime selected by `.jig/source-runtime-version`.

## Source repository CI

All Linux jobs in this repository use `ubicloud-standard-4-ubuntu-2404`
(4 vCPU, 16 GB RAM, Ubuntu 24.04), including policy and release workflows.
macOS jobs use GitHub-hosted `macos-latest` runners. The repository's
`ci_github_runner` setting in `.jig.toml` records the Linux runner selection.

The Rust Tests workflow runs formatting and launcher checks together without
building Jig. Generated-Rust Clippy validation and rendered fixtures share a job
and development binary, with both generated-project toolchains installed before
cache restoration. `validate-fixtures.sh` accepts an already-built `JIG_DEV_BIN`;
as with the launcher, the caller is responsible for its freshness. Full tests build and validate the source runtime through
`scripts/jig-dev check contract` before testing, avoiding a final development
rebuild after test-only dependency features have been enabled. The Linux
no-default-features test job first runs the explicit no-default-features build
check, sharing checkout, toolchain setup, and cache restoration.

The Repo Policy workflow builds Jig once on each of Linux and macOS, then reuses
that binary for Clippy and file-budget checks. Each platform job also runs
no-default-features Clippy and the serial dev-proxy test harness; Linux retains
the standalone dev-proxy Clippy command. These checks share one cache containing
their distinct Cargo configurations. Both the serial proxy harness and the full
workspace Nextest coverage are retained. The Linux job also validates the
agent map and Beads export. Its path filters include Rust, policy, and agent-guide
inputs. When every changed path is Beads metadata, an `AGENTS.md`, or
`agent-map.md`, only Linux repository policy checks run; the macOS job stops
after checkout and classification, and both platforms skip Clippy and proxy
tests. Unknown paths, unavailable comparison trees, manual runs, and merge
queues retain full policy coverage. The generated project workflows retain their own layout in
`templates/project/.github/workflows/`.

Manual policy runs compare the selected ref with its merge base against the
remote default branch, so they also work without a local `master` branch.

The locked test suite uses `scripts/ci/test-rust.sh workspace`; minimal-feature
jobs use its `minimal` mode. Each invocation builds the complete selected set
of test binaries once, then passes Cargo and binary metadata to Nextest for
the non-vault, vault, and serial vault-PTY phases. Test-induced Git index
refreshes cannot cause an intervening rebuild. Each phase retains the default
test-group limits, and a failed phase still fails the job after the remaining
phases finish. Separate JUnit reports are saved under
`.agent/.cache/test-reports/` and uploaded for seven days. Set
`JIG_TEST_REPORT_DIR` to override the local report destination.

Generated frontend tests use Node 24 in CI. Their package-manager scenario
matrices are separate tests so Nextest can schedule and report each manager
independently, while retaining every scenario and the two-test frontend limit.
The local vault partition continues to use identical workspace features for
both of its phases.

The Rust Tests workflow accepts `benchmark_only=true` with `runner_size=2` or
`4` on manual runs. This runs just formatting/launcher and MSRV checks with the
same commands and cache keys as ordinary CI, for comparing complete job time
and billed minutes on the two runner sizes.

The Linux full-test and release jobs start a systemd user manager and export its
bus address before testing proxy shutdown. These tests exercise the real service
manager; an unreachable manager still blocks proxy shutdown.

Rendered-fixture and generated-Rust compilation artifacts are cached separately
from the source workspace. CI sets absolute `JIG_FIXTURE_TARGET_DIR` and
`JIG_GENERATED_RUST_TARGET_DIR` paths under `.agent/.cache/`; fixture repositories and
installation roots remain temporary. The fresh-Cargo-home Git installation test
also keeps a separate temporary target so its different registry paths cannot
invalidate the shared dependency artifacts. The pull-request/push fixture job sets
`CARGO_PROFILE_RELEASE_OPT_LEVEL=0` for ordinary installation fixtures, which test
paths, profiles, and compatibility. The isolated Git installation clears that
override and exercises the normal optimized release profile. Release validation
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
