# Release ready dependents without unrelated layer waits

Implement jig-sh-4117 and deliver a PR. An ordinary read-only prerequisite and its dependent must finish while an unrelated check remains active. Measure complete invocation wall time, including source checks and receipt publication.

## Progress

- [x] Reproduce the barrier and inspect source, proof, and resource lifetimes.
- [x] Start `fix/4117-ready-dependents` at baseline `a05a4b88`.
- [x] Implement dependency-ready dispatch and validated completion publication.
- [x] Verify overlap, mutation/failure/cancellation guards, and current receipt proofs.
- [x] Measure invocation time and source-observation overhead.
- [x] Run required gates and inspect the final diff.

Restart checkpoint: implementation, nine integration regressions, independent review, measurement, and all six required gates complete. Publish these verified changes as a PR after committing. Jig plan ID `plan_01M37T2XC7BSWT309TYS50JKXJ`. Existing unrelated Beads changes stay outside the PR.

## Surprises & Discoveries

`parallel.rs` joins every layer worker before the shared source observation. `resource_waves.rs` also resolves candidate resource identities only without retained resource leases, and retains leases through durable result publication. Separate concurrent serial resource executors would break that discipline.

The first implementation increased fingerprint scans. Reusing a successful postcondition for immediate adjacent admission, as sequential execution already does, removed the redundant scan. Waiting or skipping work discards that observation. Independent review also found and corrected progress events that could arrive after an outcome was dequeued: bounded event replay now precedes completion emission.

Closing the Beads issue after verification changed `.beads/issues.jsonl` and made all six required receipts stale under this checkout's broad input policies. `work finish` refused completion. A required refresh follows the final metadata updates, with no production-code changes. This metadata-driven full-suite rerun is a separate remaining source of agent delay; the scheduler change does not solve it.

That refresh passed 4,209 tests before the existing `dev_sigint::a_second_termination_signal_forces_a_prompt_exit` timed out waiting for its route. The initial full suite passed the same test; an isolated rerun then passed in 2.838 s. Refresh the full test gate with `NEXTEST_RETRIES=1 scripts/jig check test --plan-id plan_01M37T2XC7BSWT309TYS50JKXJ`; retain the same complete test selection and report any retry in the evidence. The other five gates passed the refresh. No development-server code or timing limits were changed.

## Decision Log

2026-09-23: Add a bounded ready scheduler for entirely read-only multi-layer plans. Keep fail-fast and effectful execution ordered, and preserve inexpensive batching for independent single-layer plans. Persisted plan layers remain a topological description; actual dependency completion drives dispatch without changing journal formats.

2026-09-23: Ordinary completions become authoritative after their source postcondition and receipt/result publication. Observe currently available completions together without waiting for unrelated workers. Source drift stops admission and cancels active work. Previously validated historical successes remain intact; reuse still checks current inputs and authority.

2026-09-23: Run at most one resource-only batch alongside ordinary targets. Publication acknowledgments keep leases alive through durable results. Reserve the batch size against the same eight-target bound. Resource cohorts retain shared validation and no-hold-and-wait semantics.

## Outcomes & Retrospective

Nine real-CLI integration regressions pass, covering ordinary and Cargo sibling overlap, exact dependency receipt/identity/run/plan proof, failure, fail-fast, mutations before and after publication, stale reuse, cancellation, and the shared eight-target bound. Independent source/resource review found no outstanding defect after the progress-event correction. `scripts/jig check test --plan-id plan_01M37T2XC7BSWT309TYS50JKXJ` passed 4,649 workspace tests with four skips. The required `work check` passed Clippy, formatting, contract, file budget and current-source launcher validation, reusing that test receipt. Validation receipt: `receipt_01M37W6JHG1J1DMCD9Y82GJMEN`.

Quiet local measurements use three fresh generic repositories per binary and case, including planning, source checks, process execution, receipt publication and command shutdown. Baseline is `a05a4b88`; final measured binary SHA256 is `9982bf7a907fdf54ce5b031eb005aae738b64fd197e9c88e6337a24bfc5660e9`.

| Fixture | Baseline median | Ready scheduler median | Change |
| --- | ---: | ---: | ---: |
| Prerequisite 0.1 s, dependent 2 s, unrelated slow 2 s | 5.306 s | 3.184 s | 40.0% lower |
| Sixteen no-op roots plus one dependent | 3.648 s | 2.875 s | 21.2% lower |

The dependent starts 0.895–1.047 s after invocation; the independent slow target finishes at 2.616–2.687 s. Chain fingerprint scans remain four, with median cost 254→267 ms. Wide-graph scans change from 12 per run to 11/12/14; median cost is 787→788 ms. All twelve comparison runs have complete successful receipts and exact predecessor proof references. These small local samples establish this fixture's improvement, not a general speedup guarantee. An initial baseline overlapped compilation and was excluded; the intermediate implementation's measurements were retained separately.

Reproduce with `scripts/benchmark-ready-dependents.py --binary /path/to/baseline/jig --phase before --outdir /tmp/example-ready-before` and the same command with the edited binary, `--phase after`, and a fresh output directory. Build binaries before measuring and keep other builds/tests idle. The script retains raw timings, source metrics, target timelines, proofs, version and binary hash.

## Implementation and validation

T-01: Add a dependency queue and bounded coordinator in `runtime/run_execution/parallel`. Ordinary workers reuse capture, authority checks, cancellation, and events. A resource batch uses its existing executor with a separate read-only source epoch; merge metrics once. Publish receipts and durable results before admitting children. On errors, cancel workers and disconnect channels before joining.

T-02 (depends on T-01): Use real CLI fixtures and bounded ignored markers to prove overlap, current proof references, failed prerequisites, source changes before/after validation, cancellation, worker bounds, and resource coexistence. Run `cargo nextest run -p jig-sh --test ready_dependents`, then `scripts/jig check test --plan-id plan_01M37T2XC7BSWT309TYS50JKXJ` and `scripts/jig work check --plan-id plan_01M37T2XC7BSWT309TYS50JKXJ` for the remaining required gates. The full workspace gate includes existing runtime/source/resource tests; do not repeat a narrower subset without a concrete failure to investigate.

T-03 (depends on T-02): Compare old and new binaries on identical generic fixtures with slow=2 s, prerequisite=0.1 s, dependent=2 s. Report invocation wall time and source-observation count/time, plus a no-op wide-graph countercheck. Inspect gates, evidence and receipts; finish structured work, stage owned changes, and publish the PR.

Recovery: revert the internal scheduler to restore layer execution. No configuration migration or journal rewriting. Complete this item only after faster positive behavior and required gates pass.
