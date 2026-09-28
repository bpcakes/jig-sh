# Target freshness receipt integration

Epoch 8 introduces scoped target receipts and an explicit working-file source
state whose identity survives staging and commits. Normal source, rendering,
loading and launcher capabilities use epoch 8. The
[public policy](public-contract.md#target-freshness-policy-v1-design) defines
the compatibility rules, and the [measurements](target-freshness-benchmark.md)
record hosted CI and constrained qualification.

Work-gate evaluation and receipt reuse, which consumed this metadata through
`jig work check`, were removed with `jig work`. Receipts still record
`target_freshness`, but no check execution reuses a receipt: every check run
executes its targets. See
[Removed Work Commands](public-contract.md#removed-work-commands).

Use an epoch-8-compatible runtime to update a repository, then rerun
`jig check` to record new receipts. Earlier receipts remain readable without the
new epoch's identity. Existing epoch 2–7 repositories keep their prior rules;
upgrading does not rewrite receipts.

Omitted policies mean `inputs_policy = "whole_repository"` and
`source_state = "git"`. Generated checks keep these conservative defaults,
including Cargo formatters. The first declaration controls path coverage; the second
controls whether Git placement itself is an input. Opt into `exhaustive` only
after auditing the entire input and dependency closure. Opt into `worktree` only
for commands whose results depend on working files without depending on HEAD,
branch, history, index state or a Git comparison. Both assertions remain the
repository owner's responsibility.

Working-file source identity covers current paths, types, executable modes and
bytes. Staging and committing unchanged checked files preserve it. Edits,
additions, deletions and renames still change it. Whole-repository worktree
collection observes eligible source outside `.agent/`, `.git/` and ignored
outputs, honoring explicit `work.receipt_metadata` exclusions as well. Exhaustive
declarations retain their input scope; required ignored files remain unknown. Existing
observable ignored-dotenv handling remains. Git source authority conservatively
includes committed/index state plus HEAD commit and symbolic branch identity.
Native checks retain Git and prepared comparison authority and reject worktree
policy. See the [source-state contract](public-contract.md#contract-epoch-8-working-file-source-state)
for declaration and compatibility details.

## Adopt scoped freshness

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

To declare a narrower recorded identity for an audited custom check, select its
target and make the ownership assertions explicitly. For example, after verifying
a formatter's complete read set, append any missing patterns:

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

Audit Cargo aliases, manifests, lockfiles, toolchain and formatter configuration, custom
runner scripts, fixtures, generated inputs and transitive dependencies. Cargo
target paths and Rust `#[path]` modules can use extensions other than `.rs`, so
`**/*.rs` and existing affected-selection hints do not prove completeness. Keep
the declaration current when command resolution, Git dependencies or the read set changes. Installed tools,
ambient environment and live services are not attested by these policies.

After applying and reviewing the paired files, record new receipts:

```sh
scripts/jig check api:fmt
```

The configuration change gives later receipts a new identity; receipts are never
rewritten. Staging and commits of unchanged checked files leave a worktree
target's recorded source identity unchanged. Unrelated source edits leave it
unchanged only with exhaustive input ownership; whole-repository worktree checks
still observe those edits. Included content, additions, removals, renames,
configuration, runners and dependencies still change the recorded identity.
Global configuration authority and the existing before/after execution mutation
guard remain in force. Required ignored or unobservable inputs remain unknown.

Ordinary update/recopy preserves saved Git policies, including old inferred
values. Missing generated fields may receive current defaults. Explicit policy
assertions retain the owning action and runner through footprint/capability
refresh; use this preview to migrate existing saved actions deliberately.

## Recording and inspection

Catalog inspection exposes the policy recorded by later receipts without reading
the receipt journal. Use the opt-in agent projection to see the effective input
and source-state policies, whether each value was defaulted, and its
authored-model provenance:

```sh
scripts/jig --json info target api:test --projection agent-v1
```

The same projection is available to MCP clients when the server is started with
`scripts/jig mcp --surface agent-v1`. Epoch-8 targets report
the policy through a separately advertised schema. In the epoch-8 inspection
fixture, serialized `jig.inspect` descriptors measure 29,584 bytes for standard
and 31,431 bytes for agent-v1 (+1,847 bytes). Other catalog/execution descriptors
are unchanged. The regression test measures these sizes without treating a fixed
byte count as an API guarantee. Epoch-8 targets report
`mode: "target_freshness_v1"`; older contracts report `mode: "legacy_global"`
with conservative effective defaults and no invented or stale policy
provenance. Non-default values are never described as defaulted, even if their
stored provenance is inconsistent. The standard projection remains unchanged
for compatibility. A `freshness_policy` record is not receipt evidence and
never says that a target is currently fresh.

Epoch 8 target receipts contain `target_freshness`. A complete value contains
the current identity, original dependency receipt references, effective expiry,
and proof that the original execution did not change global source. Incomplete
values contain bounded reasons without a partial identity. Future metadata is
retained as unsupported data; a target-freshness reader never substitutes legacy digests
for missing or unsupported metadata.

The live execution worker collects one shared source snapshot, checks runner,
working-directory, and native prepared authority around each execution, and
retains the existing global launch and mutation guards. Sequential and parallel
targets use the same recording rules. Dependency references name the original
receipt, target, run, identity, successful conclusion, and effective time
boundary. The child must complete in the dependent's run, finish before the
dependent starts, and remain valid at that time. The reference's `plan_id` is
recorded empty and not compared.

## Deadlines and diagnostics

Recording uses a 30,000 ms collection ceiling. Exhausting that deadline or a
resource budget records unknown with `collection_limit` rather than a partial
identity; a longer timeout does not increase resource ceilings. Earlier
cancellation still stops collection. The 16 MiB original-record cap is an
additional bound; collection never hashes a truncated record or partial file set.

The read-only inspection deadline applied only to the removed gate and evidence
inspections. `jig status --freshness-timeout-ms` is still accepted for
compatibility but hidden and ignored, and the MCP `freshness_timeout_ms` request
field was removed with the `jig.work_*` tools.

## Time validity and retention

Original `valid_until_ms` fields keep their meaning. Additive
`effective_valid_until_ms` and `effective_requires_time_validity` describe
inherited validity. A parent can have no deadline of its own while inheriting a
dependency's deadline. A missing required boundary cannot be repaired by another
child's finite boundary, and equality with a deadline is already expired.

Target results and receipts carry this effective validity. File-budget
adoption and update enforce it.

Archive no longer retains receipts for work plans or their dependency closures;
every receipt older than the cutoff is archived.

File-budget adoption and update still require the original full-repository,
input/configuration, policy, and native prepared-authority checks. Epoch 8 also
require complete original freshness proof and effective validity. A scoped
target pass alone cannot authorize either operation.

Generated actions retain `whole_repository` input coverage and `git` source
state, including Cargo formatting checks. Recopy preserves owner policies and explicit exhaustive declarations, while
retracting the superseded inferred formatter policy described above. Enabling epoch 8 alone does not establish input completeness;
review the entire dependency closure before opting in. Original execution
safety remains unchanged even for worktree receipts.

Archive maintenance streams the required dependency frontier under its existing
writer lock. It has no collection quotas, so a large receipt history does not
prevent maintenance from shrinking the journal. Missing or unsupported required
originals still block deletion. Known incomplete metadata preserves its recorded
time constraints. Missing required deadlines stay missing through profile and
transitive summaries.

Recording carries both resource counters and cumulative observation time across
targets, excluding target execution time. Reason totals count diagnostic
occurrences; previews remove duplicates and carry an explicit truncation flag
when a distinct reason cannot fit.

A completed native failure can have a complete identity, just like a completed
command failure. Blocked, cancelled, timed-out, and skipped executions cannot
obtain complete execution proof.

Dependency execution proof no longer requires a work plan: every dependent run
records complete proof for dependencies that completed earlier in the same run.

The historical [bounded inspection measurements](benchmarks/work-inspection.md)
record the performance of the removed compact work inspection.
