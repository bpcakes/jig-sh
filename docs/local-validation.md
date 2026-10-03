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
cache restoration. Full tests build and validate the source runtime through
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
inputs; guide-only changes therefore run the policy jobs without starting the
Rust test suite. The generated project workflows retain their own layout in
`templates/project/.github/workflows/`.

Manual policy runs compare the selected ref with its merge base against the
remote default branch, so they also work without a local `master` branch.

The locked test suite and local vault partition keep workspace selection for
all phases. The final phase filters to the two vault PTY tests and runs them
serially, reusing the workspace binaries without changing dependency features.

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
