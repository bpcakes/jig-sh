# Action input declarations

Contract epoch 8 lets each action declare which repository source can affect its
result: `inputs_policy` covers path coverage and `source_state` covers whether Git
placement is an input. The [public contract](public-contract.md#action-input-declarations)
defines the values and validation rules.

Jig no longer records target freshness. Earlier runtimes derived a
`target_freshness` identity from these declarations and wrote it into every check
receipt; checks now record no receipts, and every check run executes its targets.
The declarations remain validated contract fields that inspection reports, but
they do not change what a check runs or records. Run records that carry
`target_freshness` remain readable, and current readers ignore it.

Omitted policies mean `inputs_policy = "whole_repository"` and
`source_state = "git"`. Generated checks keep these conservative defaults,
including Cargo formatters. Declare `exhaustive` only after auditing the entire
input and dependency closure. Declare `worktree` only for commands whose results
depend on working files without depending on HEAD, branch, history, index state
or a Git comparison. Both assertions remain the repository owner's
responsibility. Native checks keep Git and prepared comparison authority and
reject `worktree`.

## Preview and apply declarations

Inspect current and proposed policies without running checks or writing repository files:

```sh
scripts/jig info freshness
scripts/jig info freshness --target api:fmt --json
```

Selectors are exact `component:action` addresses from `info targets`; repeat
`--target` to select several actions. The report separates current/proposed
source state and input policy, lists inputs, and gives a reason for each decision.
Command spelling does not prove Git independence: Cargo aliases can redirect even
`cargo fmt --all -- --check` to an implementation that reads the index or HEAD.
Cargo formatters, custom scripts, `just`/`make` wrappers, tests and lints all
require owner assessment. Native checks retain their Git comparison authority.
Owner-declared policies are preserved. Updates retract the superseded unreleased
Cargo formatter inference: a saved `worktree` policy with `inferred` provenance
returns to `git` when its runner still matches the old formatter shape. This
also keeps otherwise generated repository models eligible for ordinary updates.
If the runner was subsequently customized, audit its policy explicitly.

After auditing the formatter's effective command and Cargo configuration, assert
Git independence explicitly to produce a patch:

```sh
scripts/jig info freshness --target api:fmt --assert-worktree \
  --patch > /tmp/jig-freshness.patch
```

This updates `.jig.toml` and `.agent/jig-contract.json` together in the patch.
Nothing is applied by Jig. Human-mode `--patch` emits only the unified diff;
`--json --patch` includes it in the report's `patch` field. An empty patch means
there is nothing to apply. For a nonempty patch, review it and apply from the
repository root:

```sh
git apply --check /tmp/jig-freshness.patch
git apply /tmp/jig-freshness.patch
```

Normal `git apply` fails without applying either file when a hunk conflicts.
Regenerate the preview after concurrent authority edits; do not use partial
application to split the pair. Repeating the same adoption is a no-op.

To declare a narrower input scope for an audited custom check, select its target
and make the ownership assertions explicitly. For example, after verifying a
formatter's complete read set, append any missing patterns:

```sh
scripts/jig info freshness --target api:fmt \
  --assert-worktree --assert-exhaustive \
  --input 'scripts/check-format.sh' --input 'fixtures/**/*.source' \
  --patch > /tmp/jig-freshness.patch
```

These example patterns are additions, not a universal Rust input list.
`--assert-worktree` declares independence from Git placement;
`--assert-exhaustive` declares that the resulting input patterns cover every
repository file read by the action. Each assertion requires explicit targets
and deliberately replaces the selected policy with declared provenance.
`--input` requires `--assert-exhaustive` and appends deduplicated patterns to
existing inputs; it never removes them. Either assertion can be used separately.
Added `inputs` also change affected selection, because action inputs identify
the targets a changed path selects.

Audit Cargo aliases, manifests, lockfiles, toolchain and formatter configuration, custom
runner scripts, fixtures, generated inputs and transitive dependencies. Cargo
target paths and Rust `#[path]` modules can use extensions other than `.rs`, so
`**/*.rs` and existing affected-selection hints do not prove completeness. Keep
the declaration current when command resolution, Git dependencies or the read set changes. Installed tools,
ambient environment and live services are not attested by these policies.

Ordinary update/recopy preserves saved Git policies, including old inferred
values. Missing generated fields may receive current defaults. Explicit policy
assertions retain the owning action and runner through footprint/capability
refresh; use this preview to migrate existing saved actions deliberately.

## Inspection

Catalog inspection exposes the declared policies. Use the opt-in agent
projection to see the effective input and source-state policies, whether each
value was defaulted, and its authored-model provenance:

```sh
scripts/jig --json info target api:test --projection agent-v1
```

The same projection is available to MCP clients when the server is started with
`scripts/jig mcp --surface agent-v1`; epoch-8 targets report the policy through a
separately advertised schema, and other catalog and execution descriptors are
unchanged. Epoch-8 targets report `mode: "target_freshness_v1"`; older contracts
report `mode: "legacy_global"` with conservative effective defaults and no
invented or stale policy provenance. Non-default values are never described as
defaulted, even if their stored provenance is inconsistent. The standard
projection remains unchanged for compatibility. A `freshness_policy` record
describes configuration only and never says that a target is currently fresh.

`jig status --freshness-timeout-ms`, which bounded the gate and evidence
inspections removed with `jig work`, was removed as well.
