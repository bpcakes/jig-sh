# Target freshness collector measurements

Epoch 9 is active after receipt integration and the required hosted CI and
constrained qualification. The final qualification results below establish the
[Target Freshness Policy v1](public-contract.md#target-freshness-policy-v1-design)
rollout criteria. Earlier measurements, including failed experiments, are retained
to document the implementation baseline and the changes that made it qualify.

The fixture has 4,000 generated 512-byte source files, a 100-file narrow input
tree, broad inputs covering all 4,000 files, four targets sharing a transitive
dependency chain, and 10,000 unrelated ignored files. Fixture preparation is
outside measurement. Each sample launches a separate process and performs a
complete collection; no identity cache is populated or reused. The five cases
are clean, one narrow file edited, 3,900 broad files edited, one staged edit,
and one observable untracked addition. The context and filesystem page cache
are ordinary host conditions; the first sample is reported separately and is
not a claim of a cold filesystem cache.

Initial measurements used the Cargo debug test profile on Linux, with 20
separate process invocations per case. All 200 collections returned complete
identities. Times below are the shared identity phase in milliseconds, excluding
process startup and existing repository context loading.

| Case | Initial p95 | Batched p95 |
| --- | ---: | ---: |
| Clean | 3422.604 | 1241.490 |
| Narrow dirty | 3488.121 | 1256.319 |
| Wide dirty | 3299.281 | 1171.906 |
| Staged | 3423.696 | 1287.738 |
| Untracked | 3354.590 | 1305.726 |

[All initial sample counters](benchmarks/target-freshness-initial.jsonl) include
elapsed time, committed/index parsing, worktree collection, discovered entries,
content bytes, graph size, and the enforced deadline. Batched samples also
separate shared Git execution time (`git_us`) from parsing. Initial committed
and index timings include their individual Git subprocesses; compare total
elapsed time across implementations, not those phase counters. The combined
Git response has a 16 MiB cap; this deliberately bounds the complete batch,
including intent-to-add and ignore queries, rather than allocating that amount
for each constituent command.

The initial implementation launched each Git observation in a separately owned
process tree. Measurements showed repeated process-tree supervision dominated
the tiny-fixture cost. The batched implementation retains that supervision and
its cleanup proof, grouping fixed read-only queries in one sanitized Bash
process per observation. It captures Git projections before and after bounded
file reads and rejects incomplete, malformed, failed, or changing observations.
The first 20 clean samples were recorded before this optimization; the remaining
initial cases used the same copied, unchanged executable.

Git enumeration prunes independent top-level trees using the normalized input
prefixes. It deliberately retains the first path component and its full subtree
so committed/index symlink or submodule ancestors remain observable; it never
narrows a deep declaration past those ancestors. Pattern matching still chooses
only the declared inputs for hashing. Entry accounting charges every record
visited in each projection, including repeated before/after validation, rather
than treating the limit as a unique-path count. Shared patterns compile once.
The second Git observation must match the entire previously validated protocol
byte for byte; it charges the original visited-record count without rebuilding
identical maps. Files retain an open-file metadata check before and after
streaming, followed by a complete path/directory signature pass. One buffer is
shared across file reads, and the final pass retains at most one parent handle.
Nonrecursive globs stop descent at their maximum possible path depth.
Normalized declarations share one complete match set, using path indices rather
than duplicating strings across patterns. Each retained match membership and
projection visit counts against the entry budget. This bounds shared-set memory
as well as the existing time bound; near-ceiling overlap can return unknown.
Unsupported path bytes are retained only for relevance matching; affected
inputs become unknown and diagnostics expose only a printable ancestor.
Nested plain repositories are unknown without recursive authority.

Optional identity snapshots and collection errors do not enter execution-plan
hashing or equality. Submitted-plan validation re-resolves all existing global
source/configuration and executable authority without a second scoped scan,
and returns a plan with client-supplied freshness metadata discarded. Receipt
integration prepares that proof inside the live, cancellable execution
worker. The original plan validation and mutation-safety boundaries remain
whole-repository checks.

An exhaustive argv action covers every repository-local PATH candidate that
execution can reach, including declared absence, because an executable-looking
file can fail `execve` and allow search to continue. These candidate paths enter
runner identity. Shell command text is bound directly; scripts and helpers read
by either shell or argv commands remain part of the author's exhaustive input
assertion. Git failure messages retain an exit status, diagnostic category, and
local next step without persisting arbitrary stderr paths or values.

Release measurements of the first reviewed implementation completed 100/100
samples on the host (case p95 678–709 ms) and 100/100 in a one-CPU container
with a 20 MiB/s block-device read cap (warm p95 379–388 ms). Cold measurements
flushed regular fixture files before every sample with `fsync` and
`POSIX_FADV_DONTNEED`, including Git objects and metadata; directory metadata
remained warm. All 100 cold samples completed, but case p95 was 1,016–1,141 ms,
which **fails qualification**. Cgroup I/O counters showed approximately 16.8 MB
of physical reads per sample charged to the capped device. These were local
containers, not CI. The next measurements rechecked the reviewed collector after
its per-file allocation and parent-capability improvements.
The next local cold run also completed 100/100: clean 988.674 ms, narrow dirty
1,007.708 ms, wide dirty 996.845 ms, staged 1,062.855 ms, and untracked 996.236 ms
p95. It still fails the all-cases threshold. The next frozen revision, with raw-protocol reuse and redundant-parent-lookup
removal, completed 100/100: clean 1,012.732 ms, narrow dirty 962.319 ms, wide
dirty 959.500 ms, staged 977.811 ms, and untracked 973.856 ms p95. This still
fails qualification because the clean case exceeds the limit.
[All 700 release samples](benchmarks/target-freshness-release.jsonl) preserve
the observations, including the failed cold runs and enforced cgroup limits.

The batched debug result fit the two-second inspection deadline but exceeded
the one-second p95 qualification threshold. Release builds, actual CI,
one-CPU/20-MiB/s storage, cold/warm conditions, near-ceiling outcomes and
full-command comparisons were therefore required before activation. The final
qualification below records those checks.

To reproduce a measurement, build the library test executable and pass its
printed path to the driver:

```sh
cargo test -p jig-sh --lib --no-run
python3 scripts/benchmark-target-freshness.py \
  --test-binary target/debug/deps/jig-EXAMPLE_HASH \
  --samples 20 --timeout-ms 2000 --output /tmp/example-freshness.json
```

Use the actual executable printed by Cargo. Add `--require-qualified` to fail
when any sample is incomplete or a case's p95 is at least 1,000 ms. Use
`--fixture` only with the generic temporary checkout created and printed by a
previous driver run. The driver restores that fixture between cases. For release
measurements build with `--release` and pass the release test executable.

## Encoding vectors

Identity encoding starts with the UTF-8 domain and a NUL byte. Every following
field has an unsigned 64-bit big-endian byte length followed by its exact bytes.
Epoch and schema are unsigned 32-bit big-endian fields. Numeric counts are
unsigned 64-bit big-endian fields. A target contributes component and action as
separate UTF-8 fields; an optional value contributes a one-byte presence field
and, when present, the value field. Digests have a `sha256:` prefix.

Two vectors were calculated independently with Python's `struct.pack` and
`hashlib.sha256` and are asserted by the Rust tests:

| Domain | Epoch/schema | Remaining fields | Digest |
| --- | --- | --- | --- |
| `jig-target-source-v1` | 9 / 1 | text `exhaustive`, count 1, text `apps/web/**`, count 0, count 0 | `sha256:f69025da6d1624321cbe4635e1a2e247e03f7ded42e4bbbd88d19cb72e91f089` |
| `jig-target-identity-v1` | 9 / 1 | target `web:test`, text `source`, text `authority`, text `dependency` | `sha256:1015e16914c1f65117d6da50fe7101e86728939559164cda8021c6378985416c` |

The second vector uses illustrative component tokens to pin framing; it is not
a complete executable target proof. Source and dependency ordering, absent
matches, and defaulted invocation behavior have separate fixture tests.

## Final fingerprint review decisions

Claude and Codex completed three independent review rounds of `.4.2`, with
matching before/after working-tree scope checks and no exclusions. The final
round used `e18ab21980c178baa2a5bf7a7f576ffb04c9c5d70153e91f3a1dc952b202d6dc`.
Final fixes retain and revalidate ignored working-directory components, forward
MCP planning cancellation, share complete pattern matches, and remove the
unreleased timeout flag from the deadline message. These last fixes will be
covered by the later full-branch review; no fourth per-task review was run.

A successful Git command that emits warnings remains unknown. Configuration
access warnings can mean ignore/source authority was incomplete even when
stdout has valid framing. Safe diagnostic categories and a regression make
this intentional availability tradeoff explicit. Git diagnostics do not persist
untrusted stderr values. Shell helpers retain the explicit author assertion
agreed in the policy; parsing shell text cannot discover an exhaustive input
set. PATH observation must match actual execution, so it cannot substitute a
fixed fallback while execution uses ambient PATH.

A plan's empty preview with `source_preview_truncated=true` means its displayed
preview is incomplete relative to `source_entry_count`, whether omitted for
plan size or shortened by the collector. Optional proof is discarded on
submitted-plan acceptance; receipt integration collects its own full bounded
preview. Neither preview shape nor omission affects identity equality.

The final `.4.2` code, including shared match sets and cwd revalidation, was
also measured with 40 independent cold invocations per case while repository
test suites ran on the host. All 200 completed within the two-second deadline.
Case p95 was 1,196.984 ms clean, 1,286.206 ms narrow dirty, 947.084 ms wide
dirty, 1,175.618 ms staged, and 973.915 ms untracked. This contended run fails
the p95 requirement and is preserved in the release sample file; it is not a CI
qualification. Median times were 927–937 ms. Normal epoch 8 remained active at this stage.

## Receipt integration measurements

The first bounded file read-ahead implementation completed 100/100 collector
samples in the one-CPU/20-MiB/s cold profile. Case p95 was 928.269 ms clean,
920.257 ms narrow dirty, 921.378 ms wide dirty, 930.503 ms staged, and
930.327 ms untracked. This qualifies that collector experiment, not the full
receipt integration. Its pending-file window held at most 16 verified open
files, with a 64 KiB advisory read per file, bounded by remaining content budget.

Real CLI smoke measurements exposed the extra final source scan after original
receipt lookup. Moving lookup before source collection retains a final source
check after journal I/O and removes that duplicate scan. The next cold smoke
reported phase times of 1002.996 ms for status, 936.022 ms for gates, and
991.923 ms for evidence. These single samples are not qualification and the
status sample still exceeds the one-second requirement. The current candidate
uses a sliding window of at most 64 open source files to overlap sibling I/O;
all content still passes through the same streaming hash, byte accounting, and
metadata checks. The queue drains before descending or closing a directory.

[Integration experiments](benchmarks/target-freshness-integration-experiments.jsonl)
retain the complete collector run and the failed smoke runs, including a host
smoke performed alongside focused tests on a host with more than 3,400 processes.
That host smoke hit the two-second phase deadline and is not a substitute for
the documented qualification profiles.

The first full CLI matrix completed all 600 inspections and retained non-leaf
freshness, but four command/case p95s exceeded one second. A subsequent
240-invocation clean/narrow probe also completed, with phase p95s of
1,006–1,020 ms. Both failed results are retained. The probe overlapped host
compilation and focused tests. Counters separate matching, identity encoding,
original proof lookup, and path revalidation; the subsequent candidate removed
duplicate glob evaluations within each filesystem entry's observation.

The full-command driver creates two independent generic repositories with the
same source shape and graph, one on epoch 8 and one on epoch 9. Each case records
real original receipts before measurement. Each command uses at least 20 new
processes, with separate warm and cold runs. It measures `status`, `work gates`,
and `work evidence`, verifies their gate outcomes, and compares phase p95 and
full-command p95 against the matching epoch 8 baseline. All four fixture targets
opt in and retain exhaustive authority through both shared dependency levels.
It also checks non-leaf freshness after an unrelated edit and successful finish.

```sh
cargo build --release -p jig-sh --bin jig
python3 scripts/benchmark-target-freshness-commands.py \
  --binary target/release/jig --cache warm --profile local \
  --output /tmp/example-command-warm.json
bash scripts/qualify-target-freshness-constrained.sh \
  target/release/jig cold /tmp/example-command-cold.json
```

The constrained wrapper uses a separate container with one CPU and enforced
20 MiB/s reads on the fixture's backing device. `JIG_BENCH_DEVICE` can select
that device when automatic discovery is unavailable. Cold reports require
observed physical reads through the throttled queue, not just a declared cgroup
setting. Every sample evicts regular fixture file data with `fsync` and
`POSIX_FADV_DONTNEED`; directory metadata remains warm. Warm runs perform one
untimed command before each series.

The separate limits driver declares its expectations before execution: 480 MiB
of committed source must fit recording, explicit 30-second inspection, and
finish; 513 MiB and 27,000 ordinary files must produce bounded unknown. The
entry case counts repeated Git, matching, and final path observations, which
exceed 250,000 even though its unique file count is smaller. Default inspection
of the within-ceiling case may pass or report its elapsed two-second deadline.
Every over-ceiling case must block finish and produce no complete target proof.
Separate required ignored-file and symlink cases must report unobservable input
within the default phase deadline and block finish; unaffected targets may still
have complete proof.
The constrained cold wrapper includes these cases after the command matrix.

`.github/workflows/target-freshness-qualification.yml` runs real hosted CI and
constrained profiles with both cache conditions and retains failed artifacts.
The CI profile refuses to identify an ordinary local run as GitHub Actions.
Normal epoch 8 remained active until the complete qualification and task review passed.

The subsequent full cold CLI matrix with shared match checks completed every
inspection but retained eight phase-p95 failures. Instrumentation showed the
existing plan-change Git scan was included in the new-phase timer. The candidate
now prepares that existing scope before the shared freshness budget begins;
source, original proof, native authority, final revalidation, and full-command
elapsed measurement retain their previous work. Both failed matrices remain in
the experiment journal. The corrected candidate was qualified separately.
A constrained 480 MiB case has passed recording, 30-second inspection, and finish;
its default inspection reported the expected collection-limit deadline.

The corrected release binary completed all 600 cold full-command samples in the
one-CPU/20-MiB/s profile with no qualification failures. Phase p95 across all 15
command/case pairs was 299.462–873.346 ms. Full-command p95 was within the two-second
allowance over each matching epoch-8 baseline. Both shared dependency levels
remained fresh after an unrelated edit, and finish succeeded. This result uses
binary SHA-256 `a9186d9663c22a7470da0c173a2c4173e9145cd1aacc16d42cf7fa89ef00dd4b`.
The five constrained limit cases also passed, including 480 MiB recording and
explicit inspection, and refusal for bytes/entries over the ceiling and required
ignored/symlink inputs. [Raw local qualification reports](benchmarks/target-freshness-integration-qualified-local.jsonl)
include every sample and limit outcome. These local results are separate from the
actual hosted-CI warm/cold matrix reported below.

## Hosted CI qualification

[Qualification run 34461112394](https://github.com/bpcakes/jig-sh/actions/runs/34461112394)
passed on 2026-09-10 at commit `0de59e8826ec11a5142bb4bed5bba5faca020c17`.
The four jobs used the same release runtime, SHA-256
`74a53d88a0d34f09076e529fbc1ee7bb2bfbde4600e94df6eec3fcee1186c3fe`.
Each row covers 600 independent inspections: five source cases, three commands,
two epochs and 20 samples per combination.

| Profile | Cache | Phase p95 range (ms) | Maximum phase (ms) | Largest full-command p95 increase over epoch 8 (ms) |
| --- | --- | ---: | ---: | ---: |
| Hosted CI | Warm | 147.923–157.026 | 159.316 | 170.319 |
| Hosted CI | Cold | 161.697–342.367 | 343.864 | 568.630 |
| One CPU, 20 MiB/s | Warm | 174.326–188.590 | 198.770 | 191.602 |
| One CPU, 20 MiB/s | Cold | 186.793–822.039 | 822.832 | 845.743 |

Every inspection passed with its original receipts. All four exhaustive targets
retained scoped authority through their full dependency closure; non-leaf
freshness survived the unrelated edit and finish succeeded in every matrix.
Constrained cold samples also verified physical reads through the throttled
device. Hosted CI and constrained storage each passed all five limit cases,
including 480 MiB recording, explicit inspection and finish, and bounded refusal
for oversized source, entry overflow, ignored inputs and symlinks.
[All six complete CI reports](benchmarks/target-freshness-integration-qualified-ci.jsonl)
retain the samples, counters, outcomes and exact runtime identity. Downloaded
artifact checksums were verified before inspecting the reports, and the phase
and full-command p95 values were recalculated from the samples. These results
satisfy the performance prerequisite for epoch 9 activation.
