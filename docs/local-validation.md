# Local validation in the Jig source repository

## Ordinary tasks

Choose checks for the affected behavior. For example, run a focused Rust regression:

```sh
cargo test -p jig-sh --lib repository::freshness::proof::tests
```

Use `scripts/jig info targets` to discover configured checks, then run the relevant
`scripts/jig check COMPONENT:ACTION` targets directly. Broaden validation when
shared behavior, failures, or unresolved risks warrant it. These commands do not
require a Jig work plan.

## When using structured work

For a selected work plan, use focused tests while changing code, then run the
inexpensive preflight before the configured final suite:

```sh
# While editing, choose the owning package and relevant regression filter.
cargo test -p jig-sh --lib repository::freshness::proof::tests

plan_id="$(scripts/jig work start --title 'Example change' --body 'Validate the change.' --print-plan-id)"

# Preflight alone; it does not satisfy the full verification gate.
scripts/jig work check --plan-id "$plan_id" --phase iteration

# After committing the completed implementation:
scripts/check-local --plan-id "$plan_id"
```

`check-local` first runs the configured iteration phase. In this source repository
that is the `preflight` profile: formatting, Clippy, contract, file budget and
source-runtime validation. A failure stops the command before workspace tests.
On success, it runs the final phase. Jig reuses fresh preflight receipts and
checks the complete, unchanged `verify` profile, including `api:test`. Repeating
the command with unchanged source and authority reuses passing evidence; changed
inputs or failed evidence require execution again. The wrapper never creates
or closes work, commits files, changes requirements, or forces receipt reuse.

The helper requires Python 3 and an open `--plan-id`. It resolves the repository
from its own location, inherits the normal launcher's runtime selection and
streams both phases' human output. Use native Jig commands for structured JSON
or phase previews. Help exits 0 without launching Jig; invalid arguments exit 2;
child failures propagate. Each phase runs in its own process group. SIGHUP/SIGINT/SIGTERM
reach the launcher and its startup descendants as well as the Jig runtime. The
wrapper waits for that group to stop before returning 129/130/143; after five seconds
it kills remaining group members and allows five more seconds to confirm cleanup.
Cleanup failures are reported as errors. An interrupted preflight cannot proceed
to final validation.

When testing an edited implementation, build it first and select the resulting
binary with `JIG_DEV_BIN`, or use `scripts/jig-dev` for the individual phases.
An override must refer to the current build. Routine work can use the released
runtime selected by `.jig/source-runtime-version`.
The source repository now pins 0.5.0, which supports iteration/final phases;
the previous 0.4.0 runtime cannot parse `work.iteration_profile`.

## Finish a selected plan without another full suite

Run final validation after the last implementation commit, then finish the plan
before committing the resulting evidence:

```sh
scripts/check-local --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id" --projection agent-v1 --json
scripts/jig work finish --plan-id "$plan_id" --resolution 'Implemented and verified'
git add .agent/state .agent/plans
git commit -m 'Record validation evidence'
```

`work finish` independently verifies readiness. If inspection reports unknown
freshness, follow its read-only recovery instruction instead of rerunning the
suite. A passing final `api:test` satisfies that plan's configured test gate;
there is no separate full-suite invocation for its closure. New source
changes or unresolved failures still require appropriate validation.

Do not make `api:test` or `api:clippy` worktree-only just to avoid a rerun:
`crates/jig/build.rs` reads HEAD, tags and tree cleanliness to determine the built
version and official-template policy. They retain Git-sensitive freshness.
Native file-budget and contract checks also retain their Git authority. Existing
content-based checks keep their declared policies. See
[tracked-evidence closure](target-freshness-integration.md#closing-a-plan-with-tracked-jig-evidence)
for the distinction between input changes and Git-only identity changes.

The preflight profile is an execution phase, not a dependency of tests or an
additional delivery gate. Direct `scripts/jig check test` and the existing final
verification commands remain available. Reviews can run with focused regression
evidence while a repair is in progress; reserve broad final validation for the
completed source state, subject to the user's explicitly requested review policy.
