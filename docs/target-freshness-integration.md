# Target freshness receipt integration

Epoch 8 introduces scoped target receipts, gate evaluation, and explicit
working-file receipt reuse across staging and commits. Normal source, rendering,
loading and launcher capabilities use epoch 8. The
[public policy](public-contract.md#target-freshness-policy-v1-design) defines
the compatibility rules, and the [measurements](target-freshness-benchmark.md)
record hosted CI and constrained qualification.

Use an epoch-8-compatible runtime to update a repository, then rerun `work check`
to record new evidence. Earlier receipts remain readable but cannot satisfy the
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
policy. See the [source-state contract](public-contract.md#contract-epoch-8-working-file-receipt-reuse)
for declaration and compatibility details.

## Adopt scoped freshness

For when to run focused checks, final gates, and freshness adoption together,
see [validation cadence](validation-cadence.md).

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

To claim narrower reuse for an audited custom check, select its target and make
the ownership assertions explicitly. For example, after verifying a formatter's
complete read set, append any missing patterns:

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

After applying and reviewing the paired files, record new evidence:

```sh
scripts/jig work check --plan-id PLAN_ID
```

An agent-v1-capable runtime can add `--projection agent-v1` to that check for
the completion summary described below. Use gates/evidence inspection when the
summary reports a blocker or more detail is needed.

The configuration change invalidates earlier evidence once; receipts are never
rewritten. Subsequent staging and commits of unchanged checked files preserve
worktree evidence. Unrelated source edits preserve it only with exhaustive input
ownership; whole-repository worktree checks still observe those edits. Included
content, additions, removals, renames, configuration, runners and dependencies
still invalidate affected proof. Global configuration authority and the existing
before/after execution mutation guard remain in force. Required ignored or
unobservable inputs remain unknown, rather than becoming reusable passes.

Ordinary update/recopy preserves saved Git policies, including old inferred
values. Missing generated fields may receive current defaults. Explicit policy
assertions retain the owning action and runner through footprint/capability
refresh; use this preview to migrate existing saved actions deliberately.

## Recording and inspection

For routine structured work, the compact workflow uses three commands:

```sh
plan_id="$(scripts/jig work start --title 'Example change' --body 'Implement and validate the change.' --print-plan-id)"
scripts/jig work check --plan-id "$plan_id" --projection agent-v1
# Only after the summary reports finish_ready and required reviews are satisfied:
scripts/jig work finish --plan-id "$plan_id" --resolution 'Example change validated'
```

`work check`, `work gates`, and `work evidence` accept this explicit projection.
Omission or `--projection standard` preserves existing responses. The compact
result separates check execution/reuse activity from current gate status and
freshness. A successful check may still report `finish_ready: false` because
review or other required evidence is unresolved. Failed checks retain a nonzero
CLI exit and one JSON result; inspection success does not imply gate success.

The summary renders one gate observation, includes bounded previews and full
counts, and supplies safely quoted recovery plus detailed-evidence/receipt
commands. Unknown observation recommends read-only inspection, never a check
execution as its first recovery step. A larger timeout cannot cure resource
ceilings or unobservable inputs; follow the observation diagnostics. No summary,
timestamp, or retained result authorizes closure: `work finish` independently
revalidates current source/configuration/evidence while holding its execution
lease.

### Closing a plan with tracked Jig evidence

A commit containing only `.agent/state/` changes leaves the repository source
fingerprint unchanged, but moves HEAD. Targets with `source_state = "git"`
intentionally become stale, including native targets that use prepared Git
comparison authority. `git_identity_changed` means the observed inputs are
unchanged and only HEAD or symbolic branch identity changed;
`direct_input_changed` means the observed input digest changed. Older receipts
without the separate diagnostic digest report `source_changed` when the cause
cannot be distinguished. None of these reasons grants reuse of stale evidence.

After committing product source, run final checks against that commit. If the
required gates are ready and only append-only Jig state remains to commit, close
the plan before committing that state:

```sh
scripts/jig work check --phase final --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id" --projection agent-v1
scripts/jig work finish --plan-id "$plan_id"
git add .agent/state
git commit -m 'Record Jig work evidence'
```

`work finish` rechecks required gates and source authority while the metadata
is still uncommitted, then records the close. A later Jig-only commit may make
the closed plan's Git-sensitive receipts appear stale in a new inspection; it
does not undo the completed close or require rerunning gates for that closed
plan. Finish only after the checked product source is final, and rerun any
genuinely HEAD-sensitive check if HEAD moved before finish. Audited commands
whose result depends solely on working files may instead opt in to
`source_state = "worktree"`; native checks keep their Git authority and use the
finish-before-commit sequence.

The MCP `agent-v1` surface advertises the same strict compact result for
`jig.work_check`, `jig.work_gates`, and `jig.work_evidence`. These tools' input
schemas remain unchanged. Full legacy evidence is available through the emitted
standard CLI commands or a standard-surface MCP server. No schema negotiation,
automatic plan creation, review execution, or closure is introduced.

Catalog inspection exposes the policy that would govern later freshness
evaluation without reading the receipt journal. Use the opt-in agent projection
to see the effective input and source-state policies, whether each value was
defaulted, and its authored-model provenance:

```sh
scripts/jig --json info target api:test --projection agent-v1
```

The same projection is available to MCP clients when the server is started with
`scripts/jig mcp --surface agent-v1`. Epoch-8 targets report
the policy through a separately advertised schema. In the epoch-8 inspection
fixture, serialized `jig.inspect` descriptors measure 29,584 bytes for standard
and 31,431 bytes for agent-v1 (+1,847 bytes). Other catalog/execution descriptors
are unchanged; the three work tools separately advertise their compact schema.
The regression test measures these sizes without treating a fixed
byte count as an API guarantee. Epoch-8 targets report
`mode: "target_freshness_v1"`; older contracts report `mode: "legacy_global"`
with conservative effective defaults and no invented or stale policy
provenance. Non-default values are never described as defaulted, even if their
stored provenance is inconsistent. The standard projection remains unchanged
for compatibility. A `freshness_policy` record is not receipt evidence and
never says that a target is currently fresh; use `work gates`, `work evidence`,
or status inspection for that evaluation.

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
receipt, target, run, work plan, identity, successful conclusion, and effective
time boundary. The child must finish before its dependent starts and remain
valid at that time. Reuse preserves those IDs; it does not fabricate another
successful target receipt.

For epoch 8 and later, a gate selects the latest plan-bound receipt for each explicitly
required plan-independent target across the repository, including receipts from closed
or other work plans. Receipts produced without a work-plan ID remain direct check
history; they neither satisfy nor block a structured work-plan gate. Native runners and
targets that depend on them keep plan-local selection.
An implicit dependency is verified from the original referenced receipt, so a later
independent dependency failure does not erase a successful dependent execution. If
that dependency is itself required by the gate, its latest outcome is checked
separately. Failed, legacy, or unusable latest required receipts continue to block
older passing receipts.

Opening a follow-up plan does not itself require executing unchanged checks. `work
check --plan-id NEW_PLAN` validates the originals against the new plan's current
requirements, reuses compatible passes, and records its ordinary validation batch
under `NEW_PLAN`. Each target reports `original_plan_id` beside its unchanged
`receipt_id` and `run_id`; the batch is a record of validation, not another target
execution. `work gates` and `work evidence` discover the same evidence without writing
receipts. `work finish` independently checks it again.

This is repository-local reuse, not a cross-worktree cache. Configuration, arguments,
runner authority, source policy, original dependency proof and expiry still have to
match. Native runners and their transitive dependents remain plan-local because
prepared inputs can carry work-plan and comparison authority. A new plan runs those
checks without displacing another plan's valid native evidence; ordinary test receipts
can still be reused. Pre-8 contracts keep plan-local target selection; legacy check
and review gates retain their existing rules. Older Jig runtimes can read the
unchanged original receipts but may rerun checks because they do not implement
cross-plan target reuse.

Inspection builds one bounded location index of the active receipt journal,
validates original dependencies iteratively, and then observes current source.
The index retains conflicts as ambiguous receipt IDs. Selected and dependency originals
with an ambiguous ID are unusable; conflicts in unrelated historical receipts
do not poison another target's proof. All historical records remain unchanged.
The collector's final source and configuration checks follow receipt lookup;
whole-repository Git-policy dependencies also recheck the existing global source token, then
the journal identity is checked again before returning results. Current default
invocations and native work-plan baselines determine expected authority. Recorded
explicit arguments cannot redefine the invocation a gate requires.

## Deadlines and diagnostics

The CLI accepts the request-only `--freshness-timeout-ms` option on `status`,
`work gates`, and `work evidence`, including status TUI mode. MCP gate and evidence
requests use `freshness_timeout_ms`. Both accept integers from 1 to 30,000;
omission means 2,000. Explicit nulls, strings, fractions, zero, and out-of-range
MCP values are request errors. Work-check recording, its post-check evaluation,
and work finish use 30,000 ms. Earlier cancellation still stops collection.

```sh
scripts/jig --json work gates --plan-id PLAN_ID --freshness-timeout-ms 30000
scripts/jig work evidence --plan-id PLAN_ID --freshness-timeout-ms 30000
scripts/jig status --freshness-timeout-ms 30000
```

An inspection that exhausts its deadline or resource budget reports unknown,
with `collection_limit` and its effective budget. The optional typed
`freshness_collection.limit` distinguishes `deadline` from `resource`. Deadline
exhaustion below 30 seconds suggests another read-only inspection at 30,000 ms;
it does not suggest rerunning checks. A larger timeout does not increase resource
ceilings. When inspection remains unavailable, reaches the maximum deadline or
exhausts a resource limit, execution previews and execution remedies are withheld
because current evidence has not been established. Typed CLI, MCP, and dashboard results share bounded
`freshness_reasons`, their total and truncation indicator, and separate outcome
and freshness precedence. Failed execution takes precedence in gate outcome;
unsupported authority remains visible in aggregate freshness.

The new phase includes original proof resolution, source collection, and
comparison. Existing global source checks and legacy journal selection retain
their own limits. The 16 MiB original-record cap is an additional bound;
collection never hashes a truncated record or partial file set.

## Recovery without unrelated reruns

`work gates` and `work evidence` show unresolved target IDs, freshness reasons,
available changed-input paths and any preview truncation. When inspection is
complete, they preview native targets that would execute and current required
passes that would be reused. Execution previews include actual prerequisites;
checks that merely need to pass together belong in a profile, not in one
another's `depends_on` lists. Generated test, formatting, contract and file-budget
checks already use independent profile membership. Updates preserve authored
dependencies, so repositories must review their custom prerequisite edges.

The normal recovery command preserves the work plan's comparison authority and
runs the required checks that need fresh evidence:

```sh
scripts/jig work check --plan-id PLAN_ID
```

For a deliberately forced native target, use the exact target command emitted
by inspection, for example:

```sh
scripts/jig check repo:file-budget --plan-id PLAN_ID
```

Do not add an explicit comparison override to this repair: the gate expects the
work plan's comparison authority. `work check --tool jig.file_budget` records
legacy tool evidence, which cannot satisfy a native target gate. When native
gates coexist, that command reports a `native_evidence_note` explaining the
mismatch instead of presenting its receipt as native repair evidence.

The runtime-owned CLI and MCP gate/evidence JSON adds `recovery`, or null when
there are no required native evidence targets. Its fields are:

| Field | Meaning |
| --- | --- |
| `scope` | `required_native_targets`; the preview does not claim to cover legacy or external gates. |
| `inspection` | `complete`, `deadline_exhausted`, `resource_exhausted` or `unavailable`. |
| `preview_available` | Whether current evidence supports an execution/reuse preview. |
| `execute`, `reuse` | Structured target IDs for scheduled execution, including prerequisites, and current required passes. |
| `targets` | Target disposition, reason and optional `refresh` command. Reasons include `current_pass`, `dependency_execution` and `evidence_not_current_and_passing`. |
| `next_step` | An optional command object with literal `argv` and `read_only`; timeout recovery uses read-only inspection. |
| `message`, `legacy_tool_note` | Human explanation and legacy/native receipt guidance. |

Each target `refresh` uses the same `argv`/`read_only` shape, with `read_only`
false for receipt-producing execution. This is an inspection result, not a new
`--dry-run` mode or a reservation of future execution state. Source changes after
inspection can alter the checks required by the eventual command. Existing
freshness statuses and reason codes retain their meanings.

## Time validity and retention

Original `valid_until_ms` fields keep their meaning. Additive
`effective_valid_until_ms` and `effective_requires_time_validity` describe
inherited validity. A parent can have no deadline of its own while inheriting a
dependency's deadline. A missing required boundary cannot be repaired by another
child's finite boundary, and equality with a deadline is already expired.

Target results, direct status, latest evidence, reusable evidence, work-check
gate and batch summaries, dashboard gates, and finish all carry this effective
validity. Finish checks expiry again after its final global source/configuration
verification and before closing the work plan.

Archive retains each configured plan-independent target's repository-wide newest
outcome, including blockers and receipts from closed plans, even between work plans.
It also retains existing open-plan protections and the original dependency closure of
protected, time-current proofs. It does not recompute source identities. If required
originals are missing or have unsupported metadata, archive stops before backup or
journal mutation. A single location index resolves required originals without
rescanning the journal for each dependency level. Archive has no inspection collection
quotas, so it can still shrink journals that inspection refuses to collect.

File-budget adoption and update still require the original full-repository,
input/configuration, policy, and native prepared-authority checks. Epoch 8 also
require complete original freshness proof and effective validity. A scoped
gate pass alone cannot authorize either operation.

Generated actions retain `whole_repository` input coverage and `git` source
state, including Cargo formatting checks. Recopy preserves owner policies and explicit exhaustive declarations, while
retracting the superseded inferred formatter policy described above. Enabling epoch 8 alone does not establish input completeness;
review the entire dependency closure before opting in. Original execution
safety remains unchanged even for worktree receipts.

Archive maintenance streams the required dependency frontier under its existing
writer lock. It does not spend source inspection quotas, so reaching an
inspection history ceiling does not prevent maintenance from shrinking the
journal. Missing or unsupported required originals still block deletion.
Known incomplete metadata preserves its recorded time constraints; scoped
proof failures remain independently visible. Missing required deadlines stay
missing through profile and transitive summaries.

The inspection timer begins after the existing plan-change scan. Aggregate
status and dashboard requests still share one new-phase budget across plans.
Recording carries both resource counters and cumulative observation time across
targets, excluding target execution time.

A later plan in an aggregate can exhaust that shared allowance even when its
individual `work gates --plan-id ...` inspection passes; aggregate counters include
earlier plans' work. Journal indexing also spends the shared entry/byte allowance.
Archive eligible old receipts if history exceeds those ceilings. The longer
timeout helps elapsed deadlines and leaves all resource ceilings unchanged.
Reason totals count diagnostic occurrences; previews remove duplicates and carry
an explicit truncation flag when a distinct reason cannot fit.

A completed native failure can have fresh identity, just like a completed command
failure; it still fails a success gate. Blocked, cancelled, timed-out, and skipped
executions cannot obtain complete execution proof. Native push-before inventory
fallbacks retain the intact diagnostic from their original bounded fetch attempt
while checking local object availability again; inspection performs no fetch.

Dependency execution proof requires a nonempty, shared work-plan identity. A
dependent run outside a work plan records incomplete gate proof. Archive frontier
resolution scans once per dependency depth; deep-chain performance has not been
separately benchmarked.

Aggregate inspection retains at most one request-local proof/source observation
for an identical plan-independent target graph and selected receipts. Each reuse
revalidates journal, source and configuration guards and current time validity.
Native actions and their dependents remain per-plan; no inspection result is a
completion token. The [bounded inspection measurements](benchmarks/work-inspection.md)
document physical scan counts, cold/warm timings, shared-budget limits and the
absence of demonstrated duplicate work in single-plan compact inspection.
