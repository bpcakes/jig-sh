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

The Rust Tests workflow runs formatting and launcher checks together without
building Jig. Its longer test and fixture jobs run independently. The Linux
no-default-features test job first runs the explicit no-default-features build
check, sharing checkout, toolchain setup, and cache restoration.

The Repo Policy workflow builds Jig once on each of Linux and macOS, then reuses
that binary for Clippy and file-budget checks. The Linux job also validates the
agent map and Beads export. Its path filters include Rust, policy, and agent-guide
inputs; guide-only changes therefore run the policy jobs without starting the
Rust test suite. The generated project workflows retain their own layout in
`templates/project/.github/workflows/`.

Manual policy runs compare the selected ref with its merge base against the
remote default branch, so they also work without a local `master` branch.

The locked test suite and local vault partition keep workspace selection for
all phases. The final phase filters to the two vault PTY tests and runs them
serially, reusing the workspace binaries without changing dependency features.
