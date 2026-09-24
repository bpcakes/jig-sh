# Validation cadence

Use focused checks while editing and repairing review findings. Run the configured
final gates when the implementation and review repairs have converged. Keep the
delivery requirements intact; changing when they run removes repeated work
without weakening completion evidence.

## Editing, review repairs, and delivery

For each edit, choose the smallest check that exercises the changed behavior:
a package test, a frontend typecheck, or a specific browser scenario. Use an
iteration profile when its selected targets fit the change. A frontend-only
profile provides no backend validation. Expensive database, container, browser,
infrastructure, and security checks still belong in final verification when the
project requires them. Run one earlier when it directly tests the change being made.

Review controllers commonly repeat every configured validation command after
each repair. Give them focused commands, and run final verification once repairs
settle. Direct underlying tool commands are useful when a review loop must avoid
appending tracked Jig state; they provide feedback but do not create plan-bound
gate evidence. Never describe those commands as satisfying the final gates.

```sh
scripts/jig work check --plan-id PLAN_ID --phase iteration --explain
scripts/jig work check --plan-id PLAN_ID --phase iteration
# After implementation and review repairs converge:
scripts/jig work check --plan-id PLAN_ID --phase final
scripts/jig work gates --plan-id PLAN_ID --projection agent-v1
scripts/jig work evidence --plan-id PLAN_ID --projection agent-v1
scripts/jig work finish --plan-id PLAN_ID --resolution 'Implementation validated'
```

The preview runs no checks and writes no receipts. Explicit `--phase` and
`--explain` require the standard projection; do not combine either with
`--projection agent-v1`. Explicit phases also cannot combine with `--gate` or
`--tool`. The unphased
`work check --plan-id PLAN_ID --projection agent-v1` is an alternative final
invocation. Neither check phase runs review gates or closes the plan. Finish only
after the required checks and reviews pass. After further source edits, refresh
the affected final evidence. Unknown freshness calls for read-only inspection,
not an automatic full rerun.

## Generated profiles and existing repositories

New repositories receive an `iteration` profile containing generated backend
formatting, linting, and tests, plus frontend build, lint, tests, and typecheck
when configured. Database preparation and repository policy checks remain in
`verify`. Projects with no application check targets receive no iteration profile.
These are application checks, not a guaranteed fast subset: customize expensive
test runners or use focused commands during repairs. Existing repositories keep
their selected profile or leave it unconfigured until explicitly adopted.

Iteration profiles require contract v6 or later. Inspect `scripts/jig info targets`
and choose existing read-only checks and their real prerequisites. For example,
if ExampleProject already declares these frontend actions, merge the following
into `.jig.toml` (reuse an existing `[work]` table):

```toml
[work]
iteration_profile = "iteration"

[[repository.profiles]]
id = "iteration"
description = "Fast frontend feedback during edits."
targets = [
    { component = "web", action = "typecheck" },
    { component = "web", action = "test" },
]
```

Adjust the exact targets to the repository; a test action that includes browser
or service startup may be unsuitable here. Keep `repository.default_check_profile`
and the required final gates pointing to the complete delivery scope. Re-render
with `jig update --recopy`, review `.jig.toml` and `.agent/jig-contract.json`
together, and preview both phases before execution. Authored profiles remain
project-owned; updates preserve custom selections. See
[focused Rust checks](configuration.md#iteration-and-focused-rust-checks) for
typed package and test selection without changing the final suite.

## Remove artificial prerequisites

`depends_on` means an action needs another action's successful result before it
can run. It also brings that prerequisite into focused runs and receipt reuse.
It is not a way to list checks that all need to pass before delivery.

For example, a license check, vulnerability audit, and browser test can usually
be independent members of `verify`. Chaining the license check to the audit and
the audit to the browser test makes a license-only check launch all three.
Remove an edge only after confirming the runner does not consume the preceding
action's output or require its preparation. Preserve real generation/build
prerequisites. Inspect resolved plans after changing dependencies; profile
membership keeps independent checks required without imposing execution order.

## Adopt freshness policies after auditing inputs

`source_state = "worktree"` allows unchanged checked files to retain evidence
through staging and commits. `inputs_policy = "exhaustive"` also permits reuse
after unrelated file edits. Neither is a safe blanket default. Git history,
branch, index, diff, or HEAD-sensitive commands must retain Git authority; native
checks retain it as well. Audit scripts, command aliases, manifests, lockfiles,
configuration, fixtures, generated inputs, and transitive dependencies before
asserting a complete read set.

```sh
scripts/jig info freshness --target web:typecheck
# Only after auditing this action's Git independence and complete inputs:
scripts/jig info freshness --target web:typecheck \
  --assert-worktree --assert-exhaustive \
  --input 'packages/**' --input 'scripts/check-types.sh' \
  --patch > /tmp/jig-freshness.patch
git apply --check /tmp/jig-freshness.patch
git apply /tmp/jig-freshness.patch
```

The example inputs are additions to the action's existing inputs, not a complete
frontend input declaration. Review the patch before applying it: Jig updates
the config and resolved contract together. Include future workspace additions
with patterns rather than enumerating today's package names. Verify additions,
deletions, and edits inside the scope invalidate evidence, while a truly unrelated
edit preserves it. Configuration changes invalidate evidence once. Live services,
ambient environment, and installed tools are not attested by these policies;
retain appropriate expiry and final execution rules for changing external data.
See [freshness adoption](target-freshness-integration.md#adopt-scoped-freshness).

## Isolate dependency license checks

A project that combines package installation, license validation, and a live
vulnerability audit can split them into independent actions while retaining both
in its final profile. License policy and vulnerability policy remain project
decisions. The isolated license runner should:

1. Create a temporary directory and register cleanup for success, failure, and
   interruption before installing anything.
2. Read the root workspace definition and dynamically enumerate every workspace
   manifest. Copy manifests at their original relative paths, along with the
   frozen lockfile, package-manager configuration, patches, local dependency
   sources, and any other files required by that package manager. Include new
   workspace members automatically; fail if the dependency graph cannot be
   reproduced.
3. Install the intended dependency scope in that directory with the pinned
   package manager and frozen lockfile. Choose lifecycle-script and optional
   dependency handling explicitly; do not silently change which dependencies
   the license policy covers.
4. Run the project-owned license checker and allowlist against that installed
   graph. Propagate installation and checker failures, then clean up. Never
   replace or modify the developer's `node_modules` to run this check.
5. Keep the live vulnerability audit as a separate final action, with its own
   external-data freshness requirements and diagnostics.

For a project whose complete workspace layout is `apps/*` and `packages/*`,
an input audit might include the following patterns in addition to the root
manifest, actual lockfile, package-manager config, license policy, and runner:

```toml
inputs = [
    "package.json", "bun.lock", "bunfig.toml", ".npmrc",
    "apps/*/package.json", "packages/*/package.json", "patches/**",
    "scripts/check-licenses.sh", "config/license-policy.json",
]
```

This is a starting point, not an exhaustive assertion. Match the workspace
definition exactly, including nested workspaces and any `file:` dependencies;
include source files and install hooks if installation reads them. Dynamic
manifest copying paired with a fixed list of today's package paths would still
miss future packages during freshness evaluation. Keep the runner's discovery
and the declared globs aligned.

## Keep project-specific optimizations explicit

- **SQLx compilation:** when committed query metadata supports it, use
  `SQLX_OFFLINE=true` for compile-oriented checks. Keep migrations, query metadata
  freshness, and integration behavior covered by an actual database gate.
  Offline compilation cannot establish database correctness. Custom metadata
  directories need a checker that supports them; see
  [SQLx metadata](configuration.md#sqlx-metadata-directory).
- **Runtime selection:** generated launchers already bind runtime installation
  to the configured source and revision. Avoid repeated refreshes during edits.
  In Jig's source checkout, routine `scripts/jig` uses the released runtime pin;
  `scripts/jig-dev` deliberately builds and exercises current source. Do not copy
  that source-checkout override into application repositories.
- **Browser teardown:** asynchronous response observers may still be reading
  bodies when a test closes its context. Track their pending promises, capture
  failures, drain them while the context is alive, then assert their results and
  close the context. Stop or settle producers before the final drain. Preserve
  the response assertion and surface real failures; disabling it only hides the
  race. Adapt this ordering to the project's browser lifecycle.

These recipes introduce no additional required gates. Measure iteration time
and final time separately, including failures and retries, to confirm a change
reduces repeated work while final verification still completes reliably.
