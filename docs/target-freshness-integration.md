# Target freshness receipt integration

Epoch 9 activates scoped target receipts and gate evaluation. Normal source,
rendering, loading and launcher capabilities use the same epoch; the development
feature and special test-loader bypass have been removed. The
[public policy](public-contract.md#target-freshness-policy-v1-design) defines
the compatibility rules, and the [measurements](target-freshness-benchmark.md)
record hosted CI and constrained qualification.

Use an epoch-9-compatible runtime to update a repository, then rerun `work check`
to record new evidence. Earlier receipts remain readable but cannot satisfy
epoch 9 gates without their missing original proof. Actions default to
`whole_repository`; opt into `exhaustive` only after auditing the entire input
and dependency closure. Existing epoch 2–8 repositories keep their prior rules.

## Recording and inspection

Each epoch 9 target receipt adds `target_freshness`. A complete value contains
the current identity, original dependency receipt references, effective expiry,
and proof that the original execution did not change global source. Incomplete
values contain bounded reasons without a partial identity. Future metadata is
retained as unsupported data; an epoch 9 reader never substitutes legacy digests
for missing or unsupported metadata.

The live execution worker collects one shared source snapshot, checks runner,
working-directory, and native prepared authority around each execution, and
retains the existing global launch and mutation guards. Sequential and parallel
targets use the same recording rules. Dependency references name the original
receipt, target, run, work plan, identity, successful conclusion, and effective
time boundary. The child must finish before its dependent starts and remain
valid at that time. Reuse preserves those IDs; it does not fabricate another
successful target receipt.

A gate selects the latest receipt for each explicitly required target in its
plan. An implicit dependency is verified from the original referenced receipt,
so a later independent dependency failure does not erase a successful dependent
execution. If that dependency is itself required by the gate, its latest outcome
is checked separately. Failed, legacy, or unusable latest required receipts
continue to block older passing receipts.

Inspection builds one bounded location index of the active receipt journal,
validates original dependencies iteratively, and then observes current source.
The index retains conflicts as ambiguous receipt IDs. Selected and dependency originals
with an ambiguous ID are unusable; conflicts in unrelated historical receipts
do not poison another target's proof. All historical records remain unchanged.
The collector's final source and configuration checks follow receipt lookup;
whole-policy dependencies also recheck the existing global source token, then
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
with `collection_limit`, its effective budget, and the explicit longer retry
when the default was used. Typed CLI, MCP, and dashboard results share bounded
`freshness_reasons`, their total and truncation indicator, and separate outcome
and freshness precedence. Failed execution takes precedence in gate outcome;
unsupported authority remains visible in aggregate freshness.

The new phase includes original proof resolution, source collection, and
comparison. Existing global source checks and legacy journal selection retain
their own limits. The 16 MiB original-record cap is an additional bound;
collection never hashes a truncated record or partial file set.

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

Archive retains each selected target's newest outcome, including blockers, and
retains the original dependency closure of protected, time-current proofs. It
does not recompute source identities. If required originals are missing or have
unsupported metadata, archive stops before backup or journal mutation.

File-budget adoption and update still require the original full-repository,
input/configuration, policy, and native prepared-authority checks. Epoch 9 also
requires complete original freshness proof and effective validity. A scoped
gate pass alone cannot authorize either operation.

Generated and inherited actions default to `whole_repository` with ordinary
field provenance. Recopy preserves explicit exhaustive declarations. Enabling
epoch 9 alone does not establish that an action's inputs are exhaustive; its
entire dependency closure must be audited before unrelated edits can save reruns.

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
resolution scans once per dependency depth; deep-chain performance and aggregate
multi-plan exhaustion have not been separately benchmarked.
