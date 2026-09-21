# Request-local work inspection measurements

T-05 (`jig-sh-ndz2.5`) profiles the shared compact projection delivered by
`b871f4e8` and aggregate status before and after retaining an eligible observation
within one request. Raw results are [baseline](work-inspection-baseline.json) and
[retained observation](work-inspection-retained.json). Both contain 16 completed
samples, no failed samples, and passing gates throughout.

## Method and limits

`scripts/benchmark-work-inspection.py` owns generic temporary repositories. Each
has four required command targets with shared prerequisites, 128 generated
512-byte source files, two additional API input files, and the fixture's normal
script/configuration files. It creates 1 or 20 real plans and a real successful
check, then pads the receipt journal with non-target records to exactly 1,000 or
10,000 lines. Every sample starts a new debug/test process. Status inspects every
plan; compact inspection selects one plan on `agent-v1`.

Each matrix cell has one baseline and one comparison trial: **two of the maximum
three trials per condition have been used**. Cold samples fsync and request
regular-file data eviction with `POSIX_FADV_DONTNEED`; directory metadata stays
warm. Warm samples read fixture files first. Host contention was uncontrolled.
These observations are not production latency guarantees or timing thresholds.
Every inspection explicitly uses 30 seconds; the production default remains two
seconds and the maximum remains 30 seconds.

The ignored `repository::freshness::observations::measurement` test supplies
test-only counters around actual journal indexing, original receipt reads,
source capture, and identity collection. Identity timing includes source capture:
these phases **must not be summed**. API time includes repository loading and
command work; process time additionally includes startup. Per-gate collection
statistics are cumulative under the request's existing shared budget.

To reproduce with an unused output path (only within the remaining trial budget):

```sh
cargo test -p jig-sh --lib --no-run --message-format=json
python3 scripts/benchmark-work-inspection.py --binary target/debug/jig \
  --test-binary target/debug/deps/jig-TEST_ARTIFACT_HASH \
  --output /tmp/ExampleInspection-results.json --label comparison
```

Select the library test executable from Cargo's compiler-artifact output. Build
the fixture binary from the same implementation first. The driver refuses to
overwrite retained results, saves each completed sample, and cleans up only its
owned temporary repositories. A baseline requires the pre-optimization code plus
the same test-only instrumentation, not an uninstrumented release executable.

## Observations and decision

Process elapsed milliseconds, cold / warm (rounded):

| Plans | Receipts | Command | Baseline | Retained |
| --- | --- | --- | --- | --- |
| 1 | 1,000 | status | 2,399 / 2,354 | 2,375 / 2,352 |
| 1 | 1,000 | compact | 1,951 / 1,913 | 1,909 / 1,890 |
| 1 | 10,000 | status | 3,128 / 3,050 | 3,035 / 3,013 |
| 1 | 10,000 | compact | 2,437 / 2,453 | 2,411 / 2,415 |
| 20 | 1,000 | status | 12,256 / 12,315 | 6,791 / 6,720 |
| 20 | 1,000 | compact | 1,935 / 1,916 | 1,884 / 1,880 |
| 20 | 10,000 | status | 19,823 / 19,889 | 7,487 / 7,485 |
| 20 | 10,000 | compact | 2,427 / 2,404 | 2,424 / 2,414 |

For 20-plan status, original journal index scans, identity collections and source
captures each fall from 20 to one; original receipt reads fall from 80 to four.
At 10,000 receipts, total original-index time falls from 8.18–8.24 seconds to
0.409–0.411 seconds, and identity collection (including capture) from about 9.02
seconds to 0.461–0.472 seconds. The receipt reducer and dashboard receipt scan
already ran once. Compact inspection also already performed one index scan and
one capture. There is no demonstrated duplicate phase to remove from that path,
and no compact latency improvement is claimed.

The implementation retains at most one original-proof validator and source
identity observation per request. Sharing requires the exact required target
set, selected receipt records, typed invocations, configuration identity and
whole-source token. Only the existing plan-independent receipt policy is eligible:
native actions and their dependents, including mixed graphs, stay per-plan.
Consequently a repository's mixed native verification profile need not obtain
this speedup. No persistent cache or new completion token is introduced.

Each reuse revalidates the pinned journal and live source/configuration guards;
time validity is recomputed. Remaining elapsed work includes source revalidation
and existing context/dashboard collection, not just initial capture. The shared
deadline, entry/byte ceilings and cancellation checks remain authoritative.
Changed observations fail closed; unavailable compact inspection recommends only
read-only recovery. `work finish` collects independently and cannot rely on an
earlier successful inspection. Regression tests cover repeated requests, source,
journal and configuration mutation, cancellation, resource limits, deadline
exhaustion, native graph exclusion, and source changes before finish.
