# Harness evaluation fixtures

`python3 scripts/evaluate-harness.py` runs local, synthetic guidance and tool-surface
experiments. Python 3.10+, Git, Cargo, rustc, and Node are required for all five graders.
There are no Python, Cargo registry, or npm dependencies. macOS and Linux are the
supported hosts. The commands are noninteractive and emit plain JSON without ANSI.

## Offline verification

From the repository root:

```sh
python3 scripts/evaluate-harness.py smoke --output tmp/harness-eval-smoke
python3 -B -m unittest discover -s scripts/tests
```

`smoke` requires a new output directory. It tests a correct solution, an incorrect
implementation accompanied by a false successful self-report, and a protected-file
violation for every family. `summary.json` contains fifteen pass/fail observations;
per-family JSON retains grader failures. It makes zero model calls and needs no
credentials. Rust and Node probes execute in fresh temporary directories outside
the candidate checkout. Submitted tests and claimed test results are never graders.

| Family | Required behavior | Independent evidence |
| --- | --- | --- |
| Small fix | Clamp signed counts to 0..100 | Compile and call the public Rust function at boundaries, including integer extremes |
| Cross-crate feature | Archived status in core and API | Compile both crates separately; exercise core status parsing and API labels, preserving unknown/open behavior |
| Migration | Add an INTEGER non-null enabled column with default 1 | Check INTEGER affinity (including INT/BIGINT aliases); apply new SQL to an existing SQLite row; exercise old INSERT writer and NULL rejection; preserve historical migration |
| Frontend | Empty state and escaped ordered HTML | Execute exported JavaScript with Node assertions for empty, multiple, hostile, and already-escaped strings |
| Interrupted resume | Finish summary without replay | Preserve export records, receipt, migration, and uncommitted parser edit; call summary repeatedly and with empty/alternate input |

The resume fixture commits the original source, then applies a fixed uncommitted
parser edit before starting either arm. Its existing mutation and receipt are
byte-protected. Protection starts before importing submitted code and is checked
after import and every call. Losing that edit or appending a duplicate export or
operation receipt during import or execution fails grading.
This is continuation from a persisted worktree checkpoint, not provider conversation
resume or simulation of interrupting an actual paid session.

## Frozen conditions and protocol

`baseline/AGENTS.md.snapshot` and `baseline/.agent/PLANS.md.snapshot` are exact bytes from audited
commit `03e9a9e4e5122b5bc12c66b1f635ae1faac05e15`. `baseline.json` pins their SHA-256
checksums. The suffix prevents archived instructions from becoming active repository
guidance; preparation restores their original names. Never regenerate them from current guidance. The integration test also
compares them with their original Git objects, so CI fetches complete history.
Treatment guidance comes only from an explicitly selected Git commit.

The initial tool experiment is a synthetic three-function surface: `read_file`,
`write_file`, and `run_command`. Original and compact JSON descriptors have identical
names, argument schemas, and execution semantics; their descriptions differ.
These are not snapshots of Jig's production MCP tools/list. Production surface
capture/experiments belong to tasks 10–12; this driver makes no claim about their
descriptor size or performance. Use a versioned fixture change and a new experiment
directory when changing tools, prompts, or grading criteria.

| Condition | Guidance | Tool descriptors |
| --- | --- | --- |
| baseline | Audited revision | Original synthetic surface |
| guidance-only | Selected revision | Original synthetic surface |
| tool-schema-only | Audited revision | Compact synthetic surface |
| combined | Selected revision | Compact synthetic surface |

Freeze inputs before measuring:

```sh
python3 scripts/evaluate-harness.py prepare \
  --guidance-revision HEAD --repetitions 3 --seed 20260911 \
  --output tmp/example-experiment
```

Preparation resolves HEAD to an immutable commit. The implementation itself can be
uncommitted: its exact driver, adapter, fixtures, and grader source bytes are also
retained and hashed. The default design is five families × three treatment/control
comparisons × three paired repetitions: 45 pairs, 90 isolated Git checkouts.
Each pair shares its source commit, fixed prompt, and interrupted state. Treatment
files are applied after the source commit. A local seeded RNG orders families,
comparisons, and arms. Replications with identical inputs reproduce trial metadata.

Toolchain versions, including both Cargo and rustc, are collected from a detached
temporary directory using the same environment allowlist as grader commands.
Repository rustup overrides therefore do not misidentify the compiler used by
trials. Execution checks the frozen environment before each client or resumed
finalization, and records before/after observations for each trial. Observed drift
during a trial excludes its result; drift before the next trial stops new execution.
These are version observations, not an attestation of every process instruction.

Stopping rule: finish the fixed schedule; never stop early for favorable speed or
usage. `--limit` may pause execution without changing the schedule. Keep every
failure, timeout, interruption, and exclusion with reasons. Exclude infrastructure
failures or identity mismatches from cost/timing comparisons, retain them in the
denominator and report counts by condition. Never exclude an incorrect implementation
merely because it worsens the comparison. Compare paired correctness and invariant
results before time or usage. Investigate any correctness regression before rollout.
Three pairs per family are a diagnostic sample, not a statistical quality claim.

## Explicit model execution

The bundled adapter uses the Responses API with an already configured
`OPENAI_API_KEY`. It creates no accounts, discovers no credentials, adds no billing
integration, and performs no fallback or automatic retry. Its function-call loop
follows the [official function-calling contract](https://developers.openai.com/api/docs/guides/function-calling).
Use a model identifier actually available to your existing account. There is no
default model, and a provider-returned identity mismatch is recorded and excluded.

Create a local JSON config (keep credentials out of it):

```json
{
  "argv": ["python3", "@openai-adapter"],
  "model": "EXPLICIT_AVAILABLE_MODEL_ID",
  "reasoning": "high",
  "client": "jig-harness-responses",
  "client_version": "1",
  "timeout_seconds": 600,
  "max_output_tokens": 25000
}
```

Replace the model placeholder before preparation. `@openai-adapter` selects the
frozen adapter snapshot. The adapter has a fixed maximum of 30 provider responses
and an optional `max_output_tokens` cap per response (default 4096). Preparation
freezes the effective cap in execution.json. It includes visible and reasoning
tokens; choose it explicitly for the model and task. The driver also enforces the
configured wall timeout (at most 3600 seconds). These are upper bounds, not spend
estimates. Provider `incomplete` outcomes retain their details and usage, get a
distinct result status, and are excluded from completed-run comparisons while
remaining in the denominator. An incorrect completed implementation still fails
its grade and is not automatically excluded.

```sh
python3 scripts/evaluate-harness.py prepare \
  --guidance-revision HEAD --config tmp/example-client.json \
  --output tmp/example-model-experiment
python3 scripts/evaluate-harness.py run \
  --output tmp/example-model-experiment --execute --limit 2
```

Only `run --execute` invokes the configured client. Preparation never does.
No paid calls are part of smoke tests or CI. `run` continues pending trials and
never replays finished ones. An operating-system lock rejects concurrent execution
of the same experiment and releases automatically when its process exits.
SIGINT/SIGTERM request client cancellation, allow bounded cleanup of its active
command group, then retire the outer process group and retain an interrupted result. After a hard crash, a `running` result requires
inspection of its recorded process handles before exclusion; age is not proof that
execution stopped. Once client cleanup completes, a durable `finalizing` result
retains the execution outcome and edit observations. Repeating `run` resumes only
local checkout retention, grading, and observations for that trial, never the client.
A terminal result is published only after those steps. A durable `checkout_retained`
checkpoint allows grading to resume from the recorded checkout even if the temporary
workspace was removed. If it disappeared before retention completed, exclude the
trial with a reason to continue the schedule; do not guess that a partial copy is
complete. Graders have their own 30-second subprocess limits, and SQLite grading
has a 30-second VM deadline that also preserves operator cancellation. Model-issued
commands have a separate 30-second deadline; a cleaned-up timeout is returned as
a structured tool error so the model can recover. Commands use owned process
groups with bounded cleanup and output previews; background descendants are
retired when a command ends. A cleanup failure aborts the client rather than
continuing with an unconfirmed command group.

Clients execute a copy in a detached temporary directory outside the repository
and experiment tree. Its ancestors contain neither frozen reference solutions nor
the source repository harness. Final working files are copied back to the trial
checkout before grading; the detached copy remains available if finalization is
interrupted and is removed after the terminal result is published. Keep the
`execution_workspace` recorded in a running or finalizing result until recovery is
complete. `TMPDIR` must be outside the repository and experiment directories.

This prevents accidental reference discovery through ancestor traversal. Local
code execution is **not a security sandbox**. Use a disposable evaluation host for model execution. The
bundled file tools enforce checkout paths; arbitrary command execution and submitted
programs have the host user's filesystem access. Do not use these synthetic tasks
to evaluate adversarial attempts to tamper with the host grader. Commands do not
inherit the API key; the same environment allowlist applies to grader subprocesses. Raw client logs and request paths remain local under ignored
`tmp/`; inspect them for credentials and machine-local/private details before sharing.

## Client adapter and artifacts

Other already-configured clients may implement the same adapter contract. `argv`
is an argument vector, never a shell string. The driver appends one absolute
`request.json` path and launches from the trial checkout with stdin closed. Use
absolute paths for custom adapter script arguments. Preparation snapshots file
arguments and pins their hashes; execution rejects changed files or executables.
Pin the client's version and dependencies in your environment as well.

The request includes `workspace`, the full fixed `prompt`, the selected `tools`,
`tools_sha256`, the requested model/reasoning/client/version, and `response_path`.
The client must load the checkout's AGENTS.md, expose exactly those tool descriptors
with their documented semantics, and write its observations to `response_path`.
It must report provider/client observations, not ask the model to self-report them.

```json
{
  "identity": {
    "model": "EXPLICIT_AVAILABLE_MODEL_ID",
    "reasoning": "high",
    "client": "example-client",
    "client_version": "1",
    "tools_sha256": "HASH_FROM_REQUEST"
  },
  "tool_calls": [],
  "tool_trace_complete": true,
  "usage_tokens": null,
  "client_context_tokens": null
}
```

Tool events require `name`; check events additionally use `kind: "check"`,
`check_key`, and `source_sha256` to count identical checks on unchanged source.
Missing traces or usage must be omitted or `null`, never fabricated as zero.
An observed empty trace may legitimately mean zero calls. Partial traces retain
their completeness flag. The bundled adapter classifies only `cargo test`,
`cargo check`, and `cargo clippy` argv as checks; other commands remain unclassified.
Its raw trace retains all commands for independent review.

Artifacts are versioned JSON (`schema_version: 1` on experiment/trial-result/summary
envelopes). Fields may be added; consumers should ignore unknown fields.

| Artifact | Meaning |
| --- | --- |
| `experiment.json`, `experiment.sha256` | Resolved revisions, seed, stopping rule, complete schedule, input and trial hashes |
| `inputs/` | Exact guidance, tools, task definitions, execution config, driver/grader/adapter sources |
| `trials/NNN/checkout/` | Independent source commit plus condition files and any interrupted edit |
| `trial.json`, `prompt.txt`, `tools.json` | Pair/order, starting hashes and Git status, fixed task prompt, exact descriptors |
| `request.json`, `response.json`, `stdout.log`, `stderr.log` | Client request, observations, and retained process output |
| `result.json` | Execution/finalization status, temporary workspace authority, exclusions, independent grade/invariants, wall time, identity and metrics |
| `edit-observations.json` | 50ms observations of task implementation hashes |
| `exclusion.json`, `annotations.json` | Independent exclusion or unnecessary-question annotation with reason |

`descriptor_bytes` counts the exact UTF-8 tools.json file bytes. The bundled
adapter additionally records `wire_descriptor_bytes` after function-schema
translation. Neither is model context or charged tokens. `elapsed_seconds` is
client process wall time, excluding preparation and grading. First useful edit is
the earliest observed implementation change retained in a passing final solution;
its sampling limit is 50ms and unobserved/intermediate edits are not inferred.
Missing values carry `value: null` and a reason. Usage contains actual reported
counts summed over responses; unsupported subfields remain null. Client context
tokens remain unavailable unless separately reported. Costs require actual usage
and current pricing and are not calculated by this driver.

Questions are retained in client messages. A human reviewer classifies unnecessary
questions independently, citing message indices and the information already supplied:

```sh
python3 scripts/evaluate-harness.py annotate --output tmp/example-model-experiment \
  --order 1 --unnecessary-questions 0 --reviewer example-reviewer \
  --reason "Reviewed all messages; none requested unnecessary clarification."
python3 scripts/evaluate-harness.py exclude --output tmp/example-model-experiment \
  --order 2 --reason "Example client was unavailable before execution."
```

Annotations are never overwritten. `run`, `exclude`, and `annotate` share the same
experiment lock. Exclusions preserve the original result, including failure status
and grade; their durable sidecar is authoritative during finalization and summary
reads, even if result publication was interrupted. Excluding a pending finalization
moves it to `excluded`, so it cannot block later trials. A detached workspace is
removed only when its checkout was already retained; otherwise keep it for
inspection. `run` includes existing annotations in its JSON summary.
For reconstruction, retain the whole experiment directory, verify its hashes, and
use its frozen implementation (or the exact original checkout). Restore referenced
custom adapter files at the configured paths with their recorded hashes. Reproduce
the fixture source commit and condition files in a **new** experiment; never reset
a measured checkout. This reconstructs inputs, not a deterministic model response.

Exit statuses: 0 for successful preparation/annotation or passing smoke/grade/run;
1 for failed grading or a run containing failed/excluded trials; 2 for invalid
arguments, changed inputs, or setup errors; 130 for interruption. Preparation and
setup failures print a diagnostic to stderr with empty stdout. A paused successful
run can return 0; its `finished` and `scheduled` counts show whether work remains.
Help is credential-free. No performance improvement follows merely from fewer bytes.
