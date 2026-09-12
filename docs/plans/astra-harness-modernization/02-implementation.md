# Reproducible harness evaluation fixtures

Implements `jig-sh-9wcn.2` against implementation baseline
`d89129a880042665751954f81e270e02073a67ee`. The immutable experimental control
remains `03e9a9e4e5122b5bc12c66b1f635ae1faac05e15`.

## Progress

- [x] Inspect fixture conventions, audited guidance, and existing tool definitions.
- [x] Claim the bead and open work `plan_01M28DTQG7Q8TFY7YYH9FPJYV7`.
- [x] Add fixed tasks and independent positive/negative graders for all five families.
- [x] Freeze historical guidance and create deterministic paired source checkouts.
- [x] Implement explicit client execution, missing observations, exclusions, and resume state.
- [x] Finish process tests, fixture smoke, documentation, and configured gates.
- [x] Record results and acceptance disposition for bead and structured-work closure.

Restart checkpoint: task 02 implementation and verification are complete. The
remaining handoff operation is to close work `plan_01M28DTQG7Q8TFY7YYH9FPJYV7`
and bead `jig-sh-9wcn.2` if their authoritative status is still open. Do not rerun
model trials or start another epic task as part of this handoff. Implementation is
in `scripts/evaluate-harness.py`, `scripts/harness_eval/`, and
`tests/fixtures/harness-eval/`. No paid model calls ran.

## Surprises & Discoveries

The existing repository-policy workflow already runs Python unittest discovery.
The repository has fixture answer files but no agent evaluation framework to reuse.
Objective Rust probes need only rustc; SQLite is in Python's standard library;
frontend checks use Node's built-in assertions. No package installation is needed.

The source root's historical instructions refer to full Jig workflows. The
synthetic tasks are deliberately small and carry a common orientation explaining
their available files and native verification commands. These results cannot
stand in for production MCP or adopted-repository evaluation.

## Decision Log

2026-09-11: Use a Python standard-library driver, with a provider-neutral adapter
contract and an optional Responses API adapter using existing credentials.
There is no model default, provider fallback, automatic retry, or account setup.

2026-09-11: Freeze the audited root AGENTS.md and .agent/PLANS.md verbatim.
Use a synthetic three-tool surface with identical names, parameters and execution
semantics in each arm. Only descriptions change. Production tools/list capture and
compact MCP policy remain work for later epic tasks; no production byte claim follows.

2026-09-11: Use three paired repetitions for each of three treatment comparisons
and five families: 45 pairs, 90 trials, with deterministic seeded ordering.
Keep failures and exclusions. Fixture-only tests are not comparative model results.

## Outcomes & Retrospective

The implementation records source commits, prompts,
configuration, driver/grader snapshots, SHA-256 manifests, trial order, elapsed
time, client identity, trace counts, retained-edit observations, and usage when
reported. Human review supplies unnecessary-question counts with reasons.

All six configured gates passed in `run_01M28FHZ8CTNFMJGCS14XK1Y1S`, with validation
receipt `receipt_01M28FYKSMFDZ2EQ5NR18GDQ6C`. This includes 18 Python tests,
4,083 Rust tests (3 skipped by the configured profile), Clippy, formatting,
contract validation, and file budgets. Agent-map validation also passed.

The first gate run completed the Rust tests successfully but its evidence was
invalidated by concurrent source/configuration edits. A later run exposed a test
mock intercepting Git preflight in addition to client launch. The mock was narrowed
and the full final gate passed. An earlier exclusion test also caught shared mutable
annotation state, which was fixed. These failed receipts remain in append-only state.

Final fixture-only smoke: `tmp/harness-eval-verified/summary.json` reports all five
families accepting their reference implementation, rejecting an incorrect outcome
despite a successful agent report, and rejecting protected-file changes (15 outcomes).
The retained `tmp/harness-eval-prepared/experiment.json` contains 90 isolated trial
checkouts at the fixed guidance revisions. Portable results and implementation
hashes are in [02-results.json](02-results.json). The implementation is an
uncommitted worktree change over the recorded Git baseline; no new commit is claimed.

| Acceptance criterion | Evidence and disposition |
| --- | --- |
| Reconstruct trials from immutable revision/configuration metadata | Frozen source commits, exact input/code snapshots, environment versions, manifests and checksums; deterministic reconstruction test passes |
| Same task and starting source across arms | All 45 pairs checked for equal source commits, prompts, task files and interrupted edits; separate directories |
| Reject incorrect implementations despite successful reports | Five negative smoke cases plus independent helper-module regression check |
| Continue existing work without replay | Resume grader preserves records, receipt, migration and uncommitted parser edit; explicit duplicate replay and lost-edit tests fail as required |
| Credential-free fixture-only execution | Final smoke and all process tests ran without model calls; adapter transport tested with a local fake |
| Distinguish bytes, tokens, time and missing observations | Typed JSON fields; missing usage tests; separate descriptor and wire bytes; elapsed clock and retained-edit sampling; human question annotations |
| Preserve failures, timeouts and exclusions | Process failure/timeout/interruption, original exclusion preservation, and no-replay tests pass; OS lock prevents concurrent execution |
| No improvement claimed from instruction counts | No performance result or paid comparative trial is claimed; fixed paired protocol and stopping rule documented |

The live provider API was not tested; its execution is opt-in. Supported hosts
are macOS and Linux. The synthetic tool surface is not Jig's production MCP catalog.

## Validation and recovery

From the repository root, run:

```sh
python3 scripts/evaluate-harness.py smoke --output tmp/harness-eval-smoke-final
python3 -B -m unittest discover -s scripts/tests
JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M28DTQG7Q8TFY7YYH9FPJYV7
JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M28DTQG7Q8TFY7YYH9FPJYV7 --freshness-timeout-ms 30000
```

Output directories must be new. Never reset or reuse trial source. `run` skips
finished trials; interrupted or excluded trials remain in the result set.
See the fixture README for execution selection, artifact contracts, limitations,
and the predeclared stopping rule. No persisted Jig format changes are involved.
