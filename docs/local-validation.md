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
