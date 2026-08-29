# Universal File Budgets for Jig

Status: architecture reviewed to structural steady state; delivery graph ready

Planning branch: `feat/template-owned-loc-action`

Continuation pull request: `#18`

Continuation baseline: `8d6c4e5180c6131c062ab326c033ecce8cce35a2`

Parent initiative: `feat-codex-resume-generic-monorepo-zac`

Document kind: project-level architecture and delivery plan, not a task-local ExecPlan

## 1. Executive outcome

Jig will provide one dependency-free, language-neutral file-budget capability.

The capability will protect the size and reviewability of source files without shipping one checker per language.

Repositories will own the policy.

Jig will own deterministic measurement, Git scope, evaluation, findings, supervision, and evidence.

The initial metrics will be physical lines and exact bytes.

The initial compatibility strategy will be a debt ratchet.

New files must fit their budgets.

Compliant files must remain compliant.

Oversized legacy files may stay level or shrink.

Oversized legacy files may not accumulate more line debt or byte debt.

Renames will preserve comparison-side identity.

Copies and untracked files will be treated as new files.

Waivers will be explicit, bounded, reasoned, and expiring policy entries.

Source comments will not disable enforcement.

The implementation will run inside the Jig binary already required by an adopted repository.

It will not depend on Bash portability, a language toolchain, a young third-party LOC utility, or an AST grammar collection.

Rust, TypeScript, JavaScript, Python, Go, Java, Kotlin, C, C++, C#, Ruby, PHP, Swift, and other text-based languages will use the same engine.

Language-specific linters will remain responsible for semantic complexity.

The universal engine will be responsible only for the bounded size of a review unit.

The work will continue on pull request `#18` rather than discard the completed generic-action cutover.

The temporary Bash implementation on that branch will be removed before the pull request is merge-ready.

## 2. Why this project exists

### 2.1 The original problem

Jig has an opinionated source-file size policy for Rust repositories.

Historically, Jig core implemented that policy as native Rust-specific behavior.

The current pull request correctly moved orchestration to the generic repository-action path.

That cutover established an important product boundary.

Actions, profiles, planning, supervision, findings, receipts, and gates are stack-neutral.

Rust-specific quality policy does not belong in the generic planner or dispatcher.

### 2.2 What the Bash cutover revealed

The self-contained Bash checker proved the ownership boundary but exposed the wrong implementation boundary.

The branch currently carries three large checker copies.

One copy is the Jig repository dogfood script.

One copy is the project template source.

One copy is the embedded template snapshot.

The branch also carries a large end-to-end test suite dedicated to Bash and Git edge cases.

Together those files account for roughly 1,737 changed lines.

Most of that complexity is not line counting.

Most of it is Git baseline selection, rename handling, path safety, staging, untracked state, shell portability, diagnostics, and lifecycle integration.

Jig already owns stronger implementations of many of those mechanics for affected planning and evidence.

Keeping the Bash direction would leave two Git scope implementations inside one product.

Those implementations would inevitably drift.

Every additional language template would face pressure to copy or recreate the same machinery.

That is not stack neutrality.

It is language-specific duplication behind a generic action façade.

### 2.3 Why a third-party replacement is not the answer

Several external tools count or enforce lines.

Some are mature counters without enforcement semantics.

Some are language-specific linters.

Some promising universal enforcers are new and lightly exercised.

Replacing the Bash checker with a young mandatory binary would trade visible code duplication for supply-chain, installation, release, and compatibility risk.

It would also leave Jig responsible for Git scope integration and evidence alignment around that binary.

The durable design is smaller and more direct.

Counting physical lines and bytes is simple.

Jig already needs safe repository observation.

The repository already needs Jig to run its actions.

Therefore the correct abstraction is a generic Jig capability with repository-authored policy.

### 2.4 The architectural reversal is intentional

Closed feature `.7` explicitly rejected a generic LOC engine.

That feature must remain closed and historically truthful.

It delivered the outcome it promised.

Native Rust LOC policy was removed from the generic runtime surface.

The repository-action boundary became real and replaceable.

This project is a new feature, not a rewrite of history.

The new evidence is the size and complexity of the self-contained checker boundary.

That evidence justifies a generic engine that was not justified when `.7` was written.

The parent stack-neutral epic will gain a new child feature documenting the superseding decision.

## 3. Product thesis

The product is not a LOC counter.

The product is a reviewability debt ratchet.

Every governed file has a budget vector.

Version 1 contains two coordinates.

The first coordinate is physical-line debt.

The second coordinate is byte debt.

For metric `m`, debt is defined as:

`debt_m = max(current_m - limit_m, 0)`

A change is acceptable when each enabled debt coordinate is non-increasing relative to the resolved comparison-side tree.

This produces a compact rule with broad behavior.

A new file has baseline debt zero.

Therefore a new file cannot be introduced above a configured limit.

A compliant existing file has baseline debt zero.

Therefore it cannot cross above the limit.

An oversized existing file has positive debt.

It may retain or reduce that debt.

It may not increase that debt.

A rename retains the old file as its baseline.

A copy receives no inherited debt allowance.

When an oversized file becomes compliant, its debt reaches zero.

Later changes cannot recreate the retired debt.

The same rule applies independently to lines and bytes.

Minifying a file cannot escape a byte limit.

Expanding compact code into readable lines cannot silently escape a line limit.

The policy is intentionally a proxy for review-unit size.

It is not a claim that LOC equals complexity.

## 4. Product principles

### 4.1 Stack neutrality

The runtime engine will not contain a language registry.

The runtime engine will not branch on Rust, TypeScript, Python, Go, or another language.

File selection will come from checked-in repository policy.

Templates may generate language-appropriate patterns as authored data.

Adding a new language will require policy patterns, not runtime code.

### 4.2 Repository ownership

The repository will own which files are governed.

The repository will own thresholds.

The repository will own exclusions.

The repository will own waivers.

The repository may remove the action.

The repository may replace the action runner.

Update and recopy will preserve authored authority.

### 4.3 One source of Git truth

Jig will resolve comparison scope once per operation.

The file-budget engine will consume a typed scope reconstructed by Jig from persisted comparison authority.

It will not independently guess a default branch after planning.

It will not parse line-delimited Git filenames.

It will not carry a hard-coded SHA-1 empty-tree object.

It will not allow ambient Git rename configuration to change policy identity silently.

### 4.4 Determinism over cleverness

Policy parsing will reject unknown fields.

Rule identifiers will be unique.

Ambiguous rule matches will fail.

Waivers will match exact paths.

Findings will be sorted deterministically.

Every symbolic ref will resolve to an object ID before it becomes execution authority.

### 4.5 Boring metrics

Physical lines will have one documented byte-level definition.

Bytes will mean exact content bytes.

Version 1 will not count semantic statements.

Version 1 will not strip comments.

Version 1 will not estimate model-specific tokens.

Version 1 will not parse ASTs.

Version 1 will not load grammar plugins.

### 4.6 Explicit exceptions

Source comments will not bypass policy.

Generated markers will not bypass policy.

Wildcard waivers will not be accepted.

Every waiver will include a reason.

Every waiver will include an expiry date.

Every waiver will include bounded maxima.

An expired waiver will fail visibly.

### 4.7 Evidence alignment

The scope used for evaluation will be the scope named in evidence.

The policy digest will be recorded.

The prepared comparison authority and evaluation digest will be recorded.

Every resolved comparison object ID will be recorded.

Time-bounded authorization will produce time-bounded receipt validity.

Receipts will never contain file contents.

Historical append-only state will not be rewritten.

### 4.8 Replaceability

The generated Jig implementation will be a default.

It will not be the only legal implementation.

A repository may bind the action to another runner.

A repository may omit file budgets entirely.

Minimal harness repositories will not be forced to adopt source policy.

## 5. Non-goals

This project will not create a universal complexity score.

This project will not replace Clippy.

This project will not replace ESLint.

This project will not replace Pylint.

This project will not replace Checkstyle.

This project will not infer architectural boundaries from syntax.

This project will not enforce function length.

This project will not enforce cyclomatic complexity.

This project will not count comments differently by language.

This project will not download external binaries.

This project will not embed third-party language grammars.

This project will not publish a general external scope-manifest protocol in version 1.

This project will not add a second run graph.

This project will not add a second evidence store.

This project will not add remote execution.

This project will not add action caching.

This project will not auto-fix or split files.

This project will not silently rewrite an authored policy during update.

This project will not silently expand language coverage after a Jig upgrade.

This project will not make GitHub-specific annotations part of the evaluator.

## 6. Terminology

### 6.1 Policy

A versioned checked-in document defining governed paths, metrics, thresholds, exclusions, and waivers.

### 6.2 Rule

A named set of path patterns with one line budget, one byte budget, or both.

### 6.3 Limit

The ordinary maximum for a metric.

Crossing a limit creates debt.

### 6.4 Warning threshold

A lower non-failing threshold used to report approaching debt.

### 6.5 Debt

The positive amount by which a measurement exceeds its ordinary limit.

### 6.6 Ceiling

An optional bounded maximum authorized only by an exact waiver.

The base rule itself does not provide a permanent second unlimited tier.

### 6.7 Resolved comparison

A typed, persisted description of how current content is compared with Git authority.

It distinguishes merge-base comparisons, exact-tree comparisons, index-against-HEAD comparisons, and strict inventory.

The word `baseline` is used only for the concrete tree or blob on one side of a resolved comparison.

### 6.8 Current view

The worktree, index, or tracked-tree content being measured.

### 6.9 Scope snapshot

A deterministic execution-time mapping between comparison-side and current paths, materialized from persisted object IDs under bounded Git execution.

### 6.10 Prepared native input

A bounded, durable run-plan value that records comparison strategy, resolved object IDs, policy source, current view, and work-plan authority without embedding file contents or an unbounded entry list.

### 6.11 Waiver

An exact-path, exact-rule, expiring authorization for bounded debt.

### 6.12 Exclusion

A reasoned path pattern declaring content outside the reviewability policy.

### 6.13 Audit

A whole-repository measurement report that shows current debt without granting baseline grandfathering.

### 6.14 Explain

A path-specific report of matching policy, measurements, comparison identity, debt, waiver, and disposition.

## 7. High-level architecture

```text
symbolic request -> comparison resolver -> PreparedNativeInputV1 in RunPlan
                                                |
                                                v
checked-in policy -> execution-time ScopeSnapshotV1 -> streaming measurements
                                                |                  |
                                                +-------> pure evaluator
                                                                  |
                                                                  v
                                                       NativeActionResult
                                                                  |
                                      +---------------------------+-------------+
                                      |                           |             |
                                  human CLI                 target result    receipt
```

The architecture has three ownership layers.

The first layer is repository observation.

That layer belongs to `crates/jig`.

The second layer is pure policy and evaluation.

That layer belongs to a focused `crates/jig-file-budget` crate.

The third layer is durable execution context, orchestration, and evidence.

That layer remains in the existing repository action runtime.

### 7.1 Why a separate library crate

The evaluator should be testable without a Git repository.

The evaluator should be testable without a process.

The evaluator should be testable without Jig state.

The evaluator should accept supplied file facts and content streams.

The evaluator should return typed decisions and findings.

Keeping those boundaries in a dedicated crate prevents runtime concerns from entering policy semantics.

The crate will be an internal workspace crate.

It will not be published as a separate mandatory executable in version 1.

### 7.2 Why Git remains in `crates/jig`

The existing runtime already owns bounded Git process execution.

The existing runtime already scrubs unsafe Git environment.

The existing runtime already resolves work-plan baselines.

The existing runtime already discovers untracked files.

The existing runtime already computes gate scope fingerprints.

The existing runtime already enforces cancellation and output limits.

Extracting a shared internal change-set service reuses those guarantees.

Moving Git into the pure evaluator would duplicate process and repository policy.

### 7.3 Why the first-party action is native

A native first-party action can consume durable prepared comparison authority from the immutable run plan.

It reconstructs the exact execution-time scope from resolved object IDs rather than rediscovering symbolic refs.

It does not need Bash interpolation.

It does not need an additional installed executable.

It can return typed findings, bounded evidence, and time-validity metadata directly.

The action remains replaceable because the repository contract owns its runner.

A repository-authored command runner may replace the native default.

The generated native operation will therefore be a convenience capability, not policy authority.

### 7.4 Contract epoch consequence

A new default-profile native operation cannot be silently consumed by an older runtime in the same strict generated epoch.

The implementation plan therefore reserves a new repository contract epoch.

The exact number will be chosen against the then-current main branch at implementation start.

The plan will not hard-code `7` if another already-planned epoch lands first.

The new epoch will preserve the component/action/profile model introduced by contract 6.

It will change only the capabilities that older readers cannot safely ignore.

The epoch owns a backward-readable run-plan schema bump, durable prepared native inputs, typed native-action results, bounded finding metadata, and receipt validity.

Historical run plans and receipts remain readable.

Epoch allocation must be serialized with the open literal-argv runner work.

If both capabilities land atomically they may share one new epoch.

Otherwise the later capability uses the next epoch; two independent features never claim the same number.

### 7.5 Durable native execution contract

Generic planning entry points accept an optional typed `ComparisonRequestV1`, including optional work-plan identity.

Work checks pass their plan identity before repository-action planning rather than attaching it only at execution.

MCP planning exposes the same typed request; a later execute request may repeat the work-plan ID only for equality checking and receipt linkage.

Selection happens before file-budget comparison resolution.

Planning requires and resolves comparison authority only when the selected target still uses the built-in `jig.file_budget` native runner.

Custom command replacements, authored removals, missing actions, and unrelated profiles carry no file-budget prepared input and do not need a resolvable default branch.

A selected built-in target with a missing or invalid policy still carries authenticated policy and comparison preparation states so execution can report normalized policy findings.

Planning writes a bounded `PreparedNativeInputV1` only on each selected built-in file-budget target.

The value contains current view, original comparison request, policy source, optional work-plan identity, one `PolicyPreparationV1`, and one independent `ComparisonPreparationV1`.

It also contains the fully defaulted `NativeFileBudgetConfigV1` projected from checked-in action configuration.

It does not contain file bytes, measurements, or an unbounded path list.

The target worker can therefore execute a persisted or queued plan without relying on planner memory.

Submitted plans remain untrusted.

Before durable run acceptance, Jig independently replays policy preparation and comparison preparation and requires exact equality with both submitted states.

A moved ref, changed merge base, modified OID, altered policy identity, changed provenance, or mismatched work-plan ID makes the submitted plan stale.

This acceptance-time authentication is the only intentional re-resolution.

After the run is accepted durably, background workers use only persisted OIDs and never rediscover symbolic refs.

Immediately before execution, the worker materializes `ScopeSnapshotV1` from the persisted anchors inside the existing source epoch.

Native dispatch receives a `NativeActionContext` containing both prepared states, cancellation callback, deadline, run ID, target ID, and optional work-plan ID.

An execute-time work-plan ID cannot alter comparison semantics.

Any mismatch between the prepared authority and current repository authority blocks the target.

Ready policy plus ready comparison executes measurement and evaluation.

Invalid policy maps its replay-authenticated bounded diagnostics to `Failure` findings even if comparison preparation is also unavailable.

Ready policy plus failed requested comparison and configured `strict_inventory` fallback executes strict inventory through an authenticated ready comparison state.

Ready policy plus unavailable comparison under `block` maps its authenticated reason to a normalized `Blocked` result.

This precedence is deterministic: local policy invalidity is reported first because it is actionable without comparison authority; comparison unavailability is reported only after policy is valid.

Both states are still persisted and authenticated, so ref movement and policy repair independently make an untrusted submitted plan stale.

### 7.6 Applicability and freshness boundary

`ActionSpec.inputs` remains the one declared path boundary used by generic affected selection and input freshness.

The generated `repo:file-budget` action declares `inputs = ["**"]`.

In the new contract epoch, non-empty action inputs select their own target directly rather than broadening the input matcher for every action in the containing component.

Actions without explicit inputs preserve the previous component-root behavior.

Target dependency propagation runs after direct matching and remains unchanged.

Older contract epochs retain their existing component-aggregate projection.

The file-budget action is therefore selected for every repository source change, including a policy change or a newly governed hidden path, without selecting unrelated repository-level actions merely because they share the `repo` component.

The engine performs the cheap policy filter after selection.

Policy globs are never copied into action inputs and the generic planner never parses file-budget policy.

Contract tests prove that `"**"` covers `.jig/file-budget.toml` and the repository's supported hidden paths.

Mixed-repository tests pin selection of file budget, contract checks, ignored documentation, and target dependencies independently.

The deliberate tradeoff is conservative selection rather than an under-selection bypass.

### 7.7 Checked-in native configuration

The new contract epoch extends native runners with an optional tagged `NativeActionConfigurationV1`.

The `jig.file_budget` operation requires the `file_budget` configuration variant when explicit values are present and rejects a mismatched operation/configuration pair.

`NativeFileBudgetConfigV1` contains checked `max_candidates`, `max_total_bytes`, and `missing_comparison` fields.

Omitted fields receive versioned backward-compatible defaults during catalog projection.

`missing_comparison` is `block` by default and may be set to `strict_inventory` as checked-in stricter fallback authority.

Repository-adjustable ceilings are distinct from immutable runtime hard caps.

Catalog loading rejects configured values above the runtime caps; repository data cannot disable resource protection.

The fully defaulted DTO, not invocation-time `ActionArguments`, is authenticated into the run plan, target input identity, native context, evaluation digest, and receipt evidence.

Repository-action invocation arguments select comparison or presentation but cannot raise checked-in ceilings.

## 8. Repository policy file

The canonical path will be `.jig/file-budget.toml`.

The `.jig` directory is checked-in repository policy, not runtime state.

The generated action's conservative `inputs = ["**"]` declaration includes the path without duplicating policy patterns.

The path will participate in source identity and receipts.

The policy will have its own schema version independent of repository contract version.

### 8.1 Initial complete example

```toml
version = 1

[[rules]]
id = "application-source"
category = "source"
include = [
  "**/*.rs",
  "**/*.go",
  "**/*.py",
  "**/*.rb",
  "**/*.js",
  "**/*.jsx",
  "**/*.mjs",
  "**/*.cjs",
  "**/*.ts",
  "**/*.tsx",
  "**/*.mts",
  "**/*.cts",
  "**/*.java",
  "**/*.kt",
  "**/*.kts",
  "**/*.scala",
  "**/*.c",
  "**/*.h",
  "**/*.cc",
  "**/*.hh",
  "**/*.cpp",
  "**/*.hpp",
  "**/*.cxx",
  "**/*.hxx",
  "**/*.cs",
  "**/*.swift",
  "**/*.php",
  "**/*.dart",
  "**/*.ex",
  "**/*.exs",
  "**/*.erl",
  "**/*.hrl",
  "**/*.sh",
  "**/*.bash",
  "**/*.vue",
  "**/*.svelte",
]
exclude = []
notice_lines = 400
warn_lines = 500
max_lines = 800

[[exclusions]]
pattern = "clients/generated/**"
kind = "generated"
reason = "Regenerated from the checked-in public API schema"

[[exclusions]]
pattern = "vendor/example-library/**"
kind = "vendored"
reason = "Upstream source retained byte-for-byte"

[[waivers]]
id = "legacy-parser-split"
rule = "application-source"
path = "src/legacy/parser.rs"
ceiling_lines = 940
reason = "Parser extraction is tracked as a separate delivery task"
expires = 2026-11-30
```

### 8.2 Top-level schema

`version` is required.

Version 1 accepts only `rules`, `exclusions`, and `waivers` in addition to `version`.

Unknown top-level fields fail validation.

An unsupported version fails with an actionable diagnostic.

An empty policy is valid only when it contains an explicit empty rule list.

Templates will not emit an empty policy for a source-bearing repository.

### 8.3 Rule schema

Every rule has a stable `id`.

Every rule may have a bounded display-only `category`.

Categories group output; they never select parsers or runtime language behavior.

Rule IDs are unique.

Rule IDs use the same conservative identifier grammar as other Jig-authored IDs unless evidence requires a different grammar.

Every rule has at least one include pattern.

Every rule has at least one enabled maximum.

Enabled notice and warning thresholds are optional.

Thresholds must be monotonically ordered: `notice <= warning <= maximum` for every enabled metric.

Enforcement uses exact `measurement > threshold` comparisons.

Line values are positive integers.

Byte values are positive integers.

All numeric fields parse as checked `u64` values.

Legacy no-growth behavior is version 1 semantics and has no redundant configuration field.

The engine supports byte thresholds, but generated and migrated policies do not receive byte defaults until repository evidence calibrates them.

The initial Rust seed preserves current behavior with line thresholds of 400 notice, 500 warning, and 800 maximum.

### 8.4 Pattern semantics

Patterns are repository-relative.

Patterns use forward slashes on every supported host.

Patterns use the existing `globset` dependency.

Path separators are literal.

Backslash escaping is disabled.

Patterns may not be absolute.

Patterns may not contain parent traversal.

Patterns may not explicitly target `.git` or any `.agent/**` path.

Version 1's candidate universe excludes every `.agent/**` path unconditionally, including guidance and runtime state.

A broad glob is evaluated only within that candidate universe and therefore does not govern `.agent/**` implicitly.

`.agent/jig-contract.json` remains separately observed contract/configuration authority; it is not a file-budget candidate.

Governing `.agent` guidance would require a future source-identity and affected-selection design change.

A current file must match exactly one effective rule.

Matching no rule means the file is outside file-budget policy.

Matching more than one rule is a configuration error.

Order does not select a winner.

There is no implicit specificity algorithm.

There is no last-match-wins behavior.

Authors must make overlapping policy explicit by changing include and exclude sets.

### 8.5 Exclusion schema

An exclusion is a glob pattern.

An exclusion has a closed `kind` value.

Version 1 kinds are `generated`, `vendored`, and `policy`.

An exclusion includes a non-empty reason.

Top-level exclusions apply before rule ambiguity is evaluated.

Rule-local `exclude` patterns remove paths only from that rule before ambiguity is evaluated.

A rule-local exclusion never excludes the path from another matching rule.

An excluded file may match multiple rules because it is not governed.

Exclusion patterns may not target the policy file itself.

Exclusion patterns may not target repository contract authority.

Version 1 accepts at most 256 rules, 4,096 compiled include/exclude patterns in total, 4,096 waivers, and 1,024 UTF-8 bytes per pattern.

The policy file itself is limited to 1 MiB.

The implementation does not guess whether a user-authored exclusion is "too broad."

Instead, validation rejects any exclusion that matches `.jig/file-budget.toml` or `.jig.toml`; `.agent/**` never enters the candidate universe, and contract tests pin both boundaries.

### 8.6 Waiver schema

A waiver has one stable unique `id` using the conservative Jig identifier grammar.

A waiver names one existing rule ID.

A waiver names one exact repository-relative path.

Waiver paths do not accept glob syntax.

A waiver specifies at least one bounded `ceiling_lines` or `ceiling_bytes` value.

A waiver ceiling may exceed the rule maximum.

A waiver ceiling may not be unbounded.

A waiver includes a non-empty reason.

A waiver includes an ISO calendar expiry date.

Expiry is evaluated in UTC.

The waiver remains active through the named date.

The waiver expires at the next UTC date boundary.

Duplicate waiver IDs and duplicate waivers for the same rule and path are rejected.

Waivers for unmatched paths are errors in every mode.

Waivers do not suppress warning findings.

An active waiver produces a visible finding when exercised.

An expired waiver produces an error finding.

Removing a baseline waiver while its file remains above the ordinary maximum produces `file_budget.waiver_removed_with_debt`.

Renewal, ceiling growth, and path transfer are visible authorization changes.

### 8.7 Policy identity and authorization history

The engine records the raw policy-byte digest as source authority.

It also computes a canonical semantic digest from the parsed version 1 model.

A raw-byte change invalidates ordinary input evidence.

A semantic change forces a full governed inventory; formatting-only changes do not needlessly expand evaluation after freshness has already been invalidated.

The current policy defines current matching and ordinary limits.

When a comparison-side policy blob exists, it is parsed only to detect authorization continuity for stable waiver IDs.

It never causes an old include, exclusion, or ordinary threshold to override current policy.

This prevents waiver laundering: content grown under a waiver cannot become grandfathered ordinary debt merely by deleting that waiver.

### 8.8 Why policy is not stored in `.jig.toml`

The file-budget policy has an independent lifecycle.

It may contain many path patterns and waivers.

It should remain replaceable with the action.

It should not force every Jig runtime configuration parser to understand policy evolution.

A separate file lets policy version independently from the repository contract.

The action and contract still attest the policy path as authority.

### 8.9 Why policy is not stored in `.gitattributes`

Git attributes combine repository, worktree, index, info, global, and system sources.

That layered behavior is useful for Git presentation and checkout behavior.

It is undesirable for a deterministic CI policy boundary.

Custom attributes would also hide limits and waivers across distributed files.

One explicit policy document is easier to review and protect.

## 9. Measurement semantics

### 9.1 Physical lines

An empty file contains zero physical lines.

Each LF byte terminates one physical line.

CRLF contains one LF and therefore terminates one physical line.

A non-empty final segment without a trailing LF counts as one physical line.

A file containing one LF contains one physical line.

A file containing `a` contains one physical line.

A file containing `a` followed by LF contains one physical line.

A file containing `a`, LF, `b` contains two physical lines.

A file containing `a`, CR, LF, `b`, CR, LF contains two physical lines.

Line counting does not require UTF-8.

Line counting does not normalize content.

Line counting does not strip a byte-order mark.

Line counting does not skip blank lines.

Line counting does not skip comments.

### 9.2 Bytes

Byte measurement is the exact content length presented by the current view.

Worktree measurement uses the safely opened worktree file.

Index measurement uses the index blob.

Baseline measurement uses the baseline blob.

No checkout line-ending conversion is applied to blob measurements.

The selected view is part of the evidence.

### 9.3 Streaming

Files are measured as streams.

The implementation does not load an entire file into memory.

The implementation updates line and byte counters in one pass.

The implementation may update a content digest in the same pass when required for scope verification.

The implementation checks cancellation between bounded reads.

The implementation applies a per-file byte-read limit.

The implementation applies a total byte-read limit.

The initial numeric limits will be chosen by benchmarks and existing repository-observation limits.

The plan does not invent performance numbers without evidence.

### 9.4 Binary and non-UTF-8 files

The engine is byte-oriented.

Non-UTF-8 content can be measured.

Policy patterns determine whether that content is governed.

A matched regular file containing NUL bytes is not silently skipped.

Version 1 measures arbitrary regular-file bytes and LF delimiters bytewise.

There is no binary heuristic and no content-based bypass.

An excluded generated or vendored binary is not opened for measurement.

### 9.5 Symlinks and special files

The engine does not follow symlinks.

A matched tracked symlink fails with a typed unsupported-file finding.

A matched untracked symlink fails with a typed unsupported-file finding.

Submodule gitlinks are not regular files.

A matched gitlink fails with a typed unsupported-file finding.

Sockets, devices, and named pipes are never opened as ordinary worktree files.

Special untracked entries fail closed when they match policy.

### 9.6 Changed-during-read protection

Every worktree path component is traversed descriptor-relatively beneath an already opened repository root.

No path component is followed through a symlink.

The final descriptor is opened nonblocking and checked with `fstat` before use.

Only a regular file is measured.

Identity, type, size, and modification metadata are captured from that descriptor before reading.

The same descriptor is checked again after reading.

A changed identity, type, size, or modification time invalidates the measurement.

The enclosing source epoch remains responsible for whole-target repository mutation detection.

The evaluator never treats a partial read as a valid measurement.

## 10. Prepared comparison, scope, and Git semantics

### 10.1 Internal versioned model

The first implementation introduces three internal versioned models.

`ResolvedComparisonV1` is bounded planning authority and is persisted inside the ready comparison state of `PreparedNativeInputV1`.

`ScopeSnapshotV1` is the execution-time path and ancestry inventory reconstructed from those anchors.

`MeasuredFileSetV1` records the identities and measurements actually evaluated.

Only the bounded comparison descriptor is required in the run plan.

The potentially large scope and measured-file set remain in memory and contribute digests and counts to ordinary target evidence.

None of these models is an external action protocol in version 1.

### 10.2 Proposed shape

```rust
enum ComparisonRequestV1 {
    MergeBaseRef { requested_ref: String },
    ExactTree {
        requested_oid: String,
        provenance: ExactTreeProvenance,
    },
    IndexAgainstHead,
    StrictInventory { reason: StrictInventoryReason },
}

enum ResolvedComparisonV1 {
    MergeBase {
        requested_ref: String,
        resolved_ref_oid: String,
        head_oid: String,
        merge_base_oid: String,
    },
    ExactTree {
        requested_oid: String,
        peeled_commit_oid: Option<String>,
        tree_oid: String,
        provenance: ExactTreeProvenance,
    },
    IndexAgainstHead {
        head_or_empty_oid: String,
    },
    StrictInventory {
        reason: StrictInventoryReason,
        fallback_from: Option<StrictInventoryFallbackV1>,
    },
}

struct StrictInventoryFallbackV1 {
    original_request: ComparisonRequestV1,
    failure: ComparisonPreparationFailure,
    attempted_object_ids: Vec<String>,
    failure_digest: String,
}

struct PreparedNativeInputV1 {
    schema_version: u32,
    view: CurrentView,
    request: ComparisonRequestV1,
    configuration: NativeFileBudgetConfigV1,
    policy_source: PolicySource,
    work_plan_id: Option<String>,
    policy: PolicyPreparationV1,
    comparison: ComparisonPreparationV1,
}

enum PolicyPreparationV1 {
    Ready {
        policy_raw_digest: String,
        policy_semantic_digest: String,
    },
    InvalidPolicy {
        policy_raw_digest: Option<String>,
        reason: PolicyPreparationFailure,
        diagnostics_count: u64,
        diagnostics_digest: String,
        diagnostics_preview: Vec<PreparedDiagnosticV1>,
    },
}

enum ComparisonPreparationV1 {
    Ready {
        comparison: ResolvedComparisonV1,
    },
    ComparisonUnavailable {
        reason: ComparisonPreparationFailure,
        attempted_object_ids: Vec<String>,
    },
}

struct ScopeSnapshotV1 {
    entries: Vec<ScopeEntry>,
    complete: bool,
    issues: Vec<ScopeIssue>,
}

enum CurrentView {
    Worktree,
    Index,
    Inventory,
}

struct ScopeEntry {
    kind: FileChangeKind,
    current_path: String,
    baseline_path: Option<String>,
    current_source: CurrentSource,
    baseline_blob_oid: Option<String>,
}

enum FileChangeKind {
    Added,
    Modified,
    TypeChanged,
    Renamed,
    Untracked,
    Unchanged,
}
```

Preparation diagnostics are deterministically sorted, bounded, counted, and digested.

Missing policy bytes have no raw digest; invalid readable bytes have a raw digest but no semantic digest.

Acceptance replay requires the same independent preparation variants, authority identities, total diagnostic count, and digest.

Explicit strict inventory prepares `Ready(StrictInventory)` with no fallback evidence.

When a requested comparison fails and checked-in `missing_comparison` is `strict_inventory`, Task C prepares `Ready(StrictInventory)` with the original request, typed failure, attempted object IDs, and failure digest.

When the same configuration is `block`, the failure remains `ComparisonUnavailable`.

Acceptance replay must reproduce either the fallback failure or the unavailable state exactly; a newly available comparison makes the submitted plan stale.

If policy becomes valid, comparison becomes available, or either formerly ready state fails, the submitted plan is stale and must be replanned.

For exact-tree authority, `requested_oid` preserves the caller or event identity, `peeled_commit_oid` preserves the canonical commit when one exists, and `tree_oid` identifies bytes read.

All-zero push and unborn-worktree provenance retain their explicit requested sentinel or empty-tree identity while resolving to the repository-hash-format empty tree.

Copy detection is deliberately absent from the authority model.

A copy destination appears as added.

That prevents inherited debt.

### 10.3 Extraction boundary

The implementation extracts change identity from `git_receipts` internals.

It does not replace the bounded Git runner.

It does not add another Git environment scrubber.

It does not add another output-limit policy.

It does not weaken existing cancellation.

Existing gate applicability may continue consuming flattened paths.

File budgets consume the richer entries.

Future internal consumers may use the same typed model.

External serialization requires a separate product decision and a second concrete consumer.

### 10.4 Worktree scope

Worktree scope compares the exact tree selected by `ResolvedComparisonV1` with the current worktree view.

It includes staged tracked changes.

It includes unstaged tracked changes.

It includes non-ignored untracked files.

It excludes deletions from current measurement.

It preserves rename ancestry when Git detects a rename under pinned settings.

It treats a heavily rewritten delete/add pair as a new file.

### 10.5 Index scope

Index scope compares `HEAD` or an empty tree with the index.

It reads current content from index blobs.

It does not read unstaged worktree content.

It reads the policy from the index.

An unstaged policy change does not alter a staged check.

An unborn repository uses its hash-format-correct empty tree.

### 10.6 All-tracked scope

All-tracked scope enumerates every tracked regular file.

It measures the selected current view.

It does not grant baseline debt inheritance.

It is used by `audit`.

An audit reports every current violation.

An audit does not fail solely because legacy debt exists unless strict mode is requested.

### 10.7 Comparison resolution

Symbolic refs resolve before scope capture.

The requested ref and every resolved object ID become prepared execution authority.

Default-branch resolution uses checked-in repository default-branch authority where available.

Remote refs are re-resolved once while authenticating an untrusted submitted plan.

They are not re-resolved after durable run acceptance.

Pull-request, default-branch, explicit-base, and generic `--affected` checks use `MergeBase`.

The merge base is computed once and persisted with both resolved tips.

Work checks use `ExactTree` with the captured work-plan commit or hash-format-correct empty tree.

A zero-selector local or authored-action check in an unborn repository uses the hash-format-correct empty tree with dedicated `UnbornWorktree` provenance.

This is valid resolved authority, not missing-history fallback, and all governed worktree files are additions.

Push checks use `ExactTree` with the event's exact `before` commit.

An all-zero push `before` identity means the hash-format-correct empty tree.

A missing nonzero push `before` triggers one bounded fetch attempt.

If that attempt fails, the default result is `Blocked` with `file_budget.baseline_unavailable`.

Strict inventory is used only when the invocation explicitly selected `StrictInventory` or checked-in native action configuration explicitly authorizes that stricter fallback.

Fallback conversion happens during Task C preparation, before the immutable plan is accepted.

Fallback use is recorded as ready strict-inventory authority together with the original request and failure digest.

No missing-history path silently substitutes `HEAD^` or an unrelated empty tree.

Staged checks use `IndexAgainstHead`, with an empty tree only for an unborn repository.

Direct CLI checks accept the closed comparison selector or resolve the configured default at command planning time.

CLI, MCP, CI adapters, work checks, and generic run planning share this selector rather than overloading one string field.

An interactive caller may request merge-base, exact-tree, index, or strict-inventory behavior because that caller already chooses the diagnostic scope.

Repository action planning obtains exact-tree provenance only from the work-plan baseline or an explicit CI/internal exact-tree request; it never infers push authority from ambient provider variables inside the engine.

Generic affected selection consumes flattened changed paths from this same comparison service rather than maintaining another ref interpretation.

Receipts record comparison strategy, requested ref when present, and all resolved object IDs.

Exact-tree receipts record the requested object or event sentinel, peeled commit when applicable, and resolved tree as distinct fields; equal trees do not collapse different commit authority.

### 10.8 Rename behavior

Rename detection is explicitly enabled.

Ambient `diff.renames=false` cannot disable it.

The similarity behavior is pinned by Jig invocation.

The selected behavior is documented.

The baseline path is carried in the typed entry.

The current path is used for current policy matching.

The baseline content is evaluated under the current matching rule.

### 10.9 Policy changes

A policy-file change makes the file-budget action applicable.

A policy-file change triggers a whole governed-set evaluation.

This is necessary because includes, exclusions, limits, and waivers may have changed.

Current policy is applied to current and comparison-side file facts for ordinary matching and debt.

The comparison-side policy is also inspected for stable waiver authorization continuity.

Policy tightening therefore creates grandfathered debt instead of breaking every pre-existing file immediately.

Policy relaxation is reported as a visible policy-change finding.

Version 1 does not attempt to prove that arbitrary glob changes are stronger or weaker.

It reports normalized structural differences for review.

The scope snapshot includes unchanged governed files when the semantic policy digest changed.

### 10.10 Untracked files

Non-ignored untracked regular files participate in worktree scope.

They are always new files.

They never inherit baseline debt.

Untracked directories are enumerated through Git's untracked-file authority.

Embedded repositories and unsupported entries fail closed under existing repository-observation rules.

### 10.11 Ignored files

Ignored untracked files do not participate.

Tracked files remain governed even if a later ignore rule matches them.

An ignored generated tree should be represented as an explicit policy exclusion when its tracked files exist.

### 10.12 Non-UTF-8 paths

The current affected-selection contract requires UTF-8 Git paths.

Version 1 file budgets retain that boundary.

An unsupported path encoding makes the scope incomplete.

An incomplete scope cannot produce a passing enforcement result.

### 10.13 Pinned Git behavior

Every scope command disables replacement objects, external diff drivers, and text conversion.

Machine-readable status and raw diff output are NUL-delimited.

Rename detection threshold and rename limit are pinned by the invocation.

Rename-limit degradation is an incomplete-scope error, not a silent change in ancestry semantics.

Git execution retains the existing scrubbed environment, output and entry bounds, deadline, and cancellation behavior.

Unmerged index entries, intent-to-add ambiguity, gitlinks, unsupported sparse entries, and malformed records become explicit typed scope issues.

The diagnostic identifies the omission without rendering unsafe path bytes.

## 11. Evaluation semantics

### 11.1 Per-metric facts

For each governed current file and enabled metric, the evaluator receives a current measurement.

When comparison-side ancestry exists, the evaluator receives a comparison-side measurement.

The current rule determines both limits.

The current waiver determines any bounded override, while the comparison-side policy proves authorization continuity.

### 11.2 New files

Added files have no baseline allowance.

Untracked files have no baseline allowance.

Copy destinations have no baseline allowance.

Each enabled current measurement must be at or below its ordinary maximum or active waiver ceiling.

Crossing a notice threshold emits a notice.

Crossing a warning threshold emits a warning.

Crossing a maximum emits an error.

### 11.3 Existing compliant files

If the comparison-side measurement is at or below the ordinary maximum, comparison-side debt is zero.

Current measurement at or below the maximum passes.

Current measurement above the maximum creates positive debt and fails.

An active waiver may authorize bounded current debt.

The waiver remains visible.

### 11.4 Existing oversized files

If the comparison-side measurement is above the ordinary maximum, comparison-side debt is positive.

Current debt below baseline debt passes with an improvement warning or notice.

Current debt equal to baseline debt passes with a legacy-debt warning.

Current debt above baseline debt fails.

Current measurement below the ordinary maximum retires the debt.

The current Git comparison side on a later run observes zero debt.

### 11.5 Independent debt coordinates

Line debt and byte debt are evaluated independently.

Improving line debt does not authorize increasing byte debt.

Improving byte debt does not authorize increasing line debt.

A file passes only when every enabled metric passes.

### 11.6 Warning semantics

Warnings do not fail the action.

Warning findings remain structured and durable.

An approaching-limit warning names the current measurement and limit.

A legacy-debt warning names baseline and current debt.

An active-waiver warning names the expiry date.

An improvement notice names the retired debt amount.

### 11.7 Error semantics

Errors fail the action even if an internal process exit code would otherwise be zero.

Errors use stable finding codes.

Errors include the governed path.

Errors include the rule ID.

Errors include current measurement.

Errors include the applicable maximum.

Debt-growth errors include comparison-side measurement and growth.

### 11.8 Waiver behavior

An active waiver replaces the ordinary maximum only for its stable ID, exact rule, path, and enabled metric.

An omitted waiver ceiling leaves the ordinary rule maximum active for that metric.

Current measurement above a waiver ceiling fails.

Comparison-side debt above a waiver ceiling does not silently expand the waiver.

An expired waiver fails before content can pass under it.

Removing a waiver is valid only after the file is at or below the ordinary maximum.

If a comparison-side waiver existed and current content still has ordinary debt, removal fails with `file_budget.waiver_removed_with_debt`.

Changing a waiver's path, ceiling, or expiry is recorded as a visible authorization change.

Under unchanged ordinary policy, debt cannot grow without an active bounded waiver.

### 11.9 Exclusion behavior

Excluded files are not measured.

Exclusion counts are reported.

Exclusion reasons are available through explain output.

Adding or broadening an exclusion produces a policy-change finding.

Exclusions do not become invisible enforcement bypasses.

## 12. User workflows

### 12.1 Fresh repository

`jig init` selects stack-specific template adapters.

Those adapters contribute explicit source patterns.

The renderer writes `.jig/file-budget.toml`.

The renderer writes an ordinary `repo:file-budget` action.

The action enters the default verification profile when source policy is enabled.

The first check measures new files against ordinary maxima.

No baseline file is generated.

### 12.2 Existing repository adoption

Adoption scans repository evidence through the existing bounded inference path.

The preview lists detected common source extensions.

The preview lists proposed rules.

The preview lists proposed generated or vendored exclusions only when evidence is explicit.

The preview reports how many current files exceed proposed limits.

Those files will enter no-growth debt rather than fail immediately.

Ambiguous source groups require review.

Adoption writes a policy only when no authored policy exists.

Adoption estimates candidate count and bytes under proposed policy.

If repository scale exceeds generated action ceilings, the preview proposes explicit higher `max_candidates` and `max_total_bytes` native configuration rather than silently omitting files.

### 12.3 Normal local check

```sh
scripts/jig check file-budget
```

The selector resolves the repository action.

Jig creates a plan.

The plan persists `PreparedNativeInputV1` with resolved comparison authority.

The native runner reconstructs `ScopeSnapshotV1` at execution inside the source epoch.

Findings stream through the existing execution observer.

The target result records normalized findings.

### 12.4 Affected check

```sh
scripts/jig check --affected origin/main file-budget
```

Jig resolves `origin/main` once.

The affected planner selects the action for every repository source change because its declared input is `"**"`.

The file-budget evaluator receives the same persisted merge-base comparison used to flatten affected paths.

The run plan names comparison authority; the receipt names that authority plus the evaluation digest of bytes actually measured.

### 12.5 Staged check

```sh
scripts/jig file-budget check --staged
```

The direct command is a leaf diagnostic surface.

It reads policy and content from the index.

It does not create a durable run unless invoked through an authored action.

It is suitable for a pre-commit hook.

### 12.6 Explicit base check

```sh
scripts/jig file-budget check --base origin/main
```

The ref resolves to an object ID before measurement.

The worktree, staged state, and non-ignored untracked files form the current view.

### 12.7 Audit

```sh
scripts/jig file-budget audit
```

Audit measures every governed tracked file and every governed non-ignored untracked regular file.

`--tracked-only` explicitly narrows that inventory for diagnostic use.

Audit reports the largest files.

Audit reports files nearest limits.

Audit reports all current debt.

Audit reports active and expired waivers.

Audit is informational by default.

`--strict` fails on any unwaived ordinary debt.

Strict inventory is the only permitted enforcement fallback when an expected comparison cannot be obtained.

### 12.8 Explain

```sh
scripts/jig file-budget explain src/large-module.ts
```

Explain names the matching rule.

Explain names every include and exclusion decision.

Explain names current and comparison-side measurements.

Explain shows line and byte debt.

Explain shows waiver state.

Explain shows the final disposition.

Explain creates no run and no receipt.

### 12.9 Validate

```sh
scripts/jig file-budget validate
```

Validate parses the policy strictly.

Validate compiles globs.

Validate checks rule ambiguity against current tracked paths.

Validate checks waiver targets.

Validate checks expiry.

Validate creates no run and no receipt.

### 12.10 Work gates

`scripts/jig work check` uses the captured plan baseline.

The evidence profile treats `repo:file-budget` as conservatively applicable for every repository source change.

When applicable, the evaluator consumes the plan's exact `ExactTree` comparison.

The gate receipt records policy and evaluation digests plus time validity.

There is no policy-derived gate path filter and no unrelated-path not-applicable shortcut for this action.

That extra execution cost preserves the same non-bypass boundary as `inputs = ["**"]`.

### 12.11 CI pull request

The workflow fetches enough history to resolve the event baseline.

The workflow invokes the exact `repo:file-budget` action or verification profile.

The engine requires no language toolchain.

The engine emits the same normalized findings as local execution.

Provider annotations are rendered outside the evaluator.

### 12.12 CI default-branch push

The workflow supplies the pushed range.

Its provider adapter constructs `ExactTree { requested_oid: before, provenance: PushBefore }` explicitly.

The engine does not inspect ambient provider variables to invent that authority.

The engine does not compare the new branch head with an identical remote ref.

An all-zero `before` uses the repository-hash-format empty tree.

A missing nonzero `before` receives one bounded fetch attempt and then blocks by default.

Strict inventory occurs only through an explicit selector or checked-in action argument and is recorded visibly.

It never silently substitutes `HEAD^` or empty-tree authority.

### 12.13 Policy waiver workflow

An author first runs explain.

If splitting is not currently feasible, the author adds one exact waiver.

The waiver names the governing rule.

The waiver sets bounded maxima.

The waiver includes a reason and expiry.

The policy change appears as a finding and ordinary Git diff.

Repository owners may protect the policy with `CODEOWNERS`.

### 12.14 Update and recopy

An authored or previously seeded policy is preserved byte-for-byte.

An authored action replacement is preserved.

An authored action removal is preserved.

An authored alias choice is preserved.

An authored profile-membership choice is preserved.

Managed action wiring updates only when the previous generated authority is still recognizable.

The policy is seed-once authored state and never enters managed replacement or retirement.

New default recommendations are never silently merged into authored policy.

## 13. CLI and output contract

### 13.1 Command family

The direct family is `jig file-budget`.

Version 1 subcommands are `check`, `audit`, `explain`, and `validate`.

The direct command is not a second orchestrator.

It is a leaf implementation and diagnostic surface.

Repository action execution remains owned by `jig check` and future `jig run`.

`jig file-budget` always invokes the built-in diagnostic implementation.

`jig check file-budget` resolves checked-in repository authority and may therefore run a custom replacement or report no such action after authored removal.

`check` accepts zero or one of `--base REF`, `--exact-tree OID --provenance KIND`, `--staged`, or `--strict-inventory`.

With history, zero selectors resolve the configured default branch into a merge-base comparison.

In an unborn repository, zero selectors resolve the hash-format-correct empty tree with `UnbornWorktree` exact-tree provenance.

`--base` always means merge-base semantics.

`--exact-tree` always means direct tree comparison and never silently computes a merge base.

Closed provenance values are `push_before`, `work_plan`, `unborn_worktree`, and `explicit`; public CLI callers normally use `push_before` or `explicit`, while work-plan and unborn-worktree provenance are populated by internal planning.

MCP and internal APIs carry the corresponding tagged `ComparisonRequestV1` object.

`audit` accepts `--strict` and `--tracked-only`.

`explain PATH` accepts the same comparison selectors as `check`.

`validate` reads the worktree policy by default and accepts `--staged` to validate the index policy.

Direct commands do not inherit checked-in action configuration, because the action may be replaced or removed while the built-in diagnostic remains available.

They use versioned built-in `NativeFileBudgetConfigV1` defaults.

`check`, `audit`, and `explain` accept explicit `--max-candidates` and `--max-total-bytes` diagnostic overrides within immutable runtime hard caps.

The overrides may raise or lower direct-command ceilings but cannot change policy limits, turn an incomplete evaluation into success, or affect repository action execution.

Direct missing-comparison behavior remains `block` unless `--strict-inventory` is explicitly selected.

The effective direct configuration appears in human scope output, versioned JSON, and the evaluation digest.

When adoption predicts that built-in defaults are too small, its preview proposes both checked-in action configuration and the equivalent one-shot direct diagnostic flags.

The direct path constructs the same two preparation states in memory but creates no durable run plan or receipt.

Direct commands use exit `0` for a completed passing or informational result, `1` for policy violations, `2` for invalid invocation or policy, and `3` for blocked comparison/scope authority.

Cancellation and deadline termination retain the existing Jig process-level conventions.

### 13.2 Human summary

Human output starts with scope and policy identity.

Human output lists errors before warnings and notices.

Within severity, findings sort by path and code.

The summary includes evaluated, excluded, waived, warning, and error counts.

The summary includes omitted counts when previews are bounded.

### 13.3 Finding codes

Version 1 reserves stable codes:

- `file_budget.max_lines`
- `file_budget.max_bytes`
- `file_budget.debt_growth_lines`
- `file_budget.debt_growth_bytes`
- `file_budget.legacy_debt`
- `file_budget.debt_improved`
- `file_budget.notice_lines`
- `file_budget.notice_bytes`
- `file_budget.warning_lines`
- `file_budget.warning_bytes`
- `file_budget.waiver_active`
- `file_budget.waiver_expired`
- `file_budget.waiver_invalid`
- `file_budget.waiver_removed_with_debt`
- `file_budget.policy_changed`
- `file_budget.policy_invalid`
- `file_budget.rule_ambiguous`
- `file_budget.scope_incomplete`
- `file_budget.baseline_unavailable`
- `file_budget.unsupported_file`
- `file_budget.changed_during_read`
- `file_budget.resource_limit`

Codes are additive within a policy schema epoch.

Code meaning does not depend on language.

### 13.4 Structured findings

Native execution returns a typed `NativeActionResult` directly.

It does not serialize findings into stdout and parse them back.

The existing `Finding` fields remain sufficient for each previewed diagnostic.

The result additionally carries conclusion, total finding count, truncation flag, digest over all findings, bounded human output, bounded receipt evidence, `evaluated_at_ms`, and optional `valid_until_ms`.

`severity` expresses notice, warning, or error.

`code` uses the stable code.

`source` is `jig.file_budget`.

`location.path` names the current repository path.

Line and column are omitted because the finding concerns a whole file.

### 13.5 Direct JSON

The direct command offers one versioned JSON report.

The report includes raw and semantic policy digests when available for its policy preparation state.

The report includes comparison authority and evaluation digest.

The report includes every resolved comparison object ID.

The report includes view.

The report includes complete counts.

The report includes normalized findings.

The report does not become a second durable evidence schema.

### 13.6 Bounded output

Every candidate is evaluated until a safety limit is reached.

Finding previews are bounded.

Raw counts are not bounded to the preview count.

Omitted finding counts are explicit.

An output limit does not convert errors into success.

## 14. Findings, receipts, and evidence

### 14.1 Existing result path

The action extends the generic native-action-to-`TargetRunResult` path rather than creating a file-budget journal.

The action uses existing run conclusions.

The action uses existing normalized findings.

The action uses existing target receipts.

The action uses existing work-gate evidence.

No file-budget-specific journal is created.

The pure evaluator returns `BudgetDiagnostic` values and has no dependency on `jig-contract`.

The runtime boundary maps those values to `Finding` and constructs `NativeActionResult`.

`NativeActionResult` supports `Success`, `Failure`, `Blocked`, `Cancelled`, and `TimedOut` conclusions without laundering them through process exit status.

### 14.2 Evidence payload

New receipts may include a bounded `file_budget` evidence object.

The object includes policy schema version.

The object includes raw and semantic policy digests when available for its policy preparation state.

The object includes prepared-input, comparison, scope, and report schema versions.

The object includes independent policy and comparison preparation states.

The object includes the evaluation digest.

Failed preparation has no evaluation digest and instead records its diagnostics or attempted-authority digest.

The object includes comparison strategy and every resolved object ID.

The object includes current view.

The object includes evaluated file count.

The object includes excluded file count.

The object includes active waiver count.

The object includes notice, warning, and error counts.

The object includes completeness status.

The object includes bounded issue summaries.

The object includes total finding count, preview count, truncation flag, and digest over all normalized findings.

The object includes `evaluated_at_ms` and optional `valid_until_ms`.

The object does not include file content.

### 14.3 Canonical evaluation digest

The evaluation digest uses domain `jig-file-budget-evaluation-v1\0` and length-prefixed fields.

It covers policy schema plus raw and semantic policy digests; comparison variant, request, provenance, and every resolved OID; current view; effective resource ceilings; and completeness issues.

For every candidate in UTF-8 bytewise path order it covers change kind, current path, comparison-side path, ancestry, current and comparison-side content digests, line and byte measurements, matched rule, exclusion or waiver ID, debt coordinates, and final disposition.

It also covers the captured evaluation instant, optional validity boundary, total finding counts, and the digest over all normalized findings.

Missing optional fields receive explicit tags so concatenation cannot collide.

File contents and raw paths outside the existing UTF-8 boundary never enter receipts.

Tests pin digest stability for identical facts and sensitivity to every authority, content, ancestry, view, policy, measurement, waiver, and disposition field.

### 14.4 Historical compatibility

Historical receipts without this evidence remain readable.

Historical receipts without time validity retain their existing semantics.

A new file-budget receipt that exercised an active waiver but lacks time validity is stale or unknown, never indefinitely fresh.

Historical generic-action receipts from the Bash phase remain truthful.

Historical closed Beads records remain unchanged.

The implementation does not redact or rewrite append-only state.

### 14.5 Source epoch

The existing pre-target source check remains active.

The existing post-target source check remains active.

File measurement adds per-file changed-during-read protection.

A scope mismatch blocks a successful conclusion.

### 14.6 Calendar validity

Each target captures one UTC evaluation instant and supplies it to the pure evaluator.

Because expiry validation is global, every active waiver in the authoritative policy is relevant.

`valid_until_ms` is the earliest next UTC boundary after the inclusive expiry date of any active waiver, even when its path was not a changed candidate.

At or after that instant, work-gate freshness rejects the receipt even if repository bytes are unchanged.

Immediately before committing a passing result, the runtime samples the clock again.

If the validity boundary was crossed during evaluation, the target reports the waiver as expired and cannot record a passing receipt.

Generic receipt freshness learns optional time validity so future time-bounded native policies can reuse it.

This addition is independent of the open input-scoped freshness feature and must remain compatible with its future conjunction of source and input evidence.

The earliest boundary propagates through `ReceiptRecord`, target receipt status, work-check batch evidence, scoped and reusable gate evidence, profile/latest-evidence output, and archive/reuse decisions.

Every freshness query compares the current clock with the boundary; no projection merely copies the field without enforcing it.

Historical records without the field preserve old behavior except for new file-budget evidence that proves active waivers but lacks validity, which remains stale or unknown.

### 14.7 Failure mapping

Policy violations produce `Failure`.

Invalid policy produces `Failure` with policy findings.

Incomplete scope produces `Blocked` when authority cannot be established.

Cancellation produces `Cancelled`.

Deadline exhaustion produces `TimedOut`.

Internal invariant failure produces `Blocked`.

Resource-ceiling exhaustion produces `Blocked`, because it proves neither policy compliance nor violation.

## 15. Template and lifecycle design

### 15.1 Template contributions

Template adapters contribute explicit rule patterns to one repository-model value.

Rust templates contribute Rust source patterns.

React templates contribute TypeScript, JavaScript, JSX, TSX, Vue, or Svelte patterns only when applicable.

Go templates contribute Go source patterns.

Other adapters contribute only their selected stack patterns.

The evaluator does not know which adapter contributed them.

Init renders one seed policy programmatically from that combined repository model.

The policy is written only when absent and is never registered as a managed replacement asset.

### 15.2 Mixed repositories

One repository policy may contain multiple disjoint rules.

Rules may use different thresholds.

Rules may cover different component roots.

Overlaps remain errors.

The generated repository action is a repository-level action because policy spans components.

The action findings retain exact file paths and rule IDs.

### 15.3 Generated files

Known generated paths are explicit exclusions.

Generated markers in file content are not authority.

Adoption may propose an exclusion only when repository evidence is strong.

The preview shows the evidence and reason.

The author confirms the result before write.

### 15.4 Vendored files

Vendored paths are explicit exclusions.

The exclusion reason names the upstream ownership boundary generically.

The engine does not infer vendor status from directory name alone during runtime.

### 15.5 Existing authored policy

Init refuses to overwrite an existing policy.

Adoption previews and preserves an existing policy.

Update preserves an existing policy.

Recopy preserves an existing policy.

After first write, the policy is permanently authored state and preserved byte-for-byte.

Managed action wiring recognizes exact generated authority separately from policy content.

A preserved policy without an action is legal.

A custom replacement runner is legal and preserved.

### 15.6 Removal

Removing the action is a supported authored decision.

Removing the action from the default profile is a supported authored decision.

Removing the canonical alias is a supported authored decision.

Update does not recreate removed authority.

Recopy does not recreate removed authority.

### 15.7 Default evolution

New Jig releases may change suggested defaults.

They do not mutate existing policy silently.

A future `file-budget diff-defaults` command may show recommendations.

That command is outside version 1 delivery unless implementation evidence makes it trivial.

### 15.8 Two-phase Bash retirement

Migration uses two update invocations rather than executing repository policy inside the update transaction.

Phase one seeds a policy when absent, installs the generated native action and profile wiring, and retains every recognized Bash checker asset.

Task E introduces a real `RepositoryUpdateTransaction` for full updates; current per-path publication is not treated as atomic.

The seed-once policy is staged as a separately classified authored publication within that transaction and is never added to the managed-path manifest.

Phase-one policy, action, contract, and manifest writes use one in-process atomic, crash-recoverable repository transaction.

If migration needs a human-authored waiver reason or expiry, phase one performs no lifecycle mutation and emits a bounded proposal until a valid authored policy exists.

After phase one, the repository runs `repo:file-budget` through ordinary action execution.

That run records a successful receipt bound to the generated native runner, action configuration, policy bytes, comparison authority, evaluated source, evaluation digest, and calendar validity.

Phase two is a later update invocation.

It retires only exact recognized generated Bash assets and only when the latest native-action receipt is successful and fresh for every bound identity at update time.

The receipt decision is preflighted, then revalidated under the update lock against exact receipt, runner, configuration, policy, source, comparison, evaluation, and validity identities immediately before mutation.

The transaction computes the staged post-update native authority and evaluated-source identity before deciding whether retirement operations remain in the commit set.

That staged identity must equal the receipt-bound identity after excluding only the exact recognized legacy deletions and deterministic managed-manifest bookkeeping proposed by phase two.

Any staged contract, runner, configuration, policy, runtime-source, or governed-source change outside those exclusions means the receipt proves the old state only.

In that case the transaction commits the otherwise valid update while retaining Bash, reports the identity difference, and requires a fresh native run before another phase-two attempt.

A missing, stale, expired, failed, truncated-inconclusively, or differently configured receipt retains the Bash assets and prints the exact rerun command.

Noninteractive update follows the same rule and never assumes consent or success.

An authored action replacement, action removal, or modified Bash asset disables automatic retirement and is preserved.

Phase-two retirements and manifest updates use the same transaction protocol.

The transaction records absence, bytes, type, permissions, and managed metadata for every destination before the first publication or removal.

Concurrent destination mutation detected before a path is first touched aborts without overwriting that path.

On later failure, rollback restores a preimage only while the destination still matches a state written by this transaction.

If a non-cooperating writer replaces a destination during commit, Jig preserves the foreign bytes, retains the protected preimage in the transaction recovery area, and reports incomplete recovery with exact paths and instructions.

The plan does not promise both byte-for-byte rollback and overwriting an arbitrary concurrent writer.

Failure injection before and after every publication and removal proves complete restoration when no foreign writer intervenes, and preservation plus explicit recovery evidence when one does.

In-process repository-file transaction guarantees end at the committed marker.

Post-commit runtime-cache refresh remains recoverable and warning-only where current product behavior requires it; the plan does not call cache publication part of the repository transaction.

### 15.9 Crash recovery contract

Multi-file replacement is not instantaneously atomic to an external observer across process death.

The promised contract is in-process failure atomicity plus deterministic crash recovery before a later update proceeds.

Before the first destination mutation, Jig acquires the update lock and creates a mode-0700 transaction directory under the worktree-specific Git metadata path returned by `git rev-parse --git-path`.

The transaction directory contains transaction kind, versioned manifest, operation order, destination-relative paths, preimage metadata and bytes or absence markers, staged-output digests, phase-two proof identity when applicable, and no absolute host paths.

The manifest, preimages, staged payloads, and containing directories are flushed durably before the state advances from `Preparing` to `Prepared`.

Each completed publication or removal appends and flushes a progress record.

After every destination matches the staged commit set, Jig writes and flushes a `Committed` marker before post-commit cache work.

Cleanup of a committed transaction is idempotent.

On startup of update, recopy, or launcher repair, Jig checks for an incomplete transaction while holding the same lock.

Recovery direction is deliberately one-way.

A durable `Committed` marker means repository mutation finished; recovery performs idempotent cleanup only.

Every `Preparing`, `Prepared`, or applying/progress state without `Committed` rolls back all transaction-owned destinations to their preimages.

Recovery never resumes or completes an uncommitted publication or retirement, even if every staged output was already written.

An interrupted phase-two retirement therefore restores or retains Bash and requires a later update to revalidate receipt, source, configuration, evaluation, and current time from scratch.

If a foreign writer changed a destination, recovery preserves the foreign bytes, retains the recovery bundle, and blocks further mutation with exact manual guidance.

Recovery never guesses from timestamps and never overwrites bytes whose digest is absent from the transaction journal.

The migration requires a Git worktree because the durable journal lives outside repository source; absence of usable Git metadata blocks mutation before phase one.

Subprocess crash tests terminate the updater before and after every operation and marker, then prove uncommitted rollback, committed cleanup, foreign-write preservation, and idempotence.

Phase-two crash tests let proof expire and mutate governed source while the updater is down; both recover by rollback and cannot resume retirement.

### 15.10 Durable legacy-asset recognition

Task E replaces dependence on live Bash template copies with a bounded legacy generation table.

Each recognized asset entry records generation ID, repository-relative path, SHA-256 byte digest, expected file type, and executable-bit expectation.

Recognition compares exact bytes and metadata; a digest match at the wrong path or with authored metadata is not retirement authority.

The table contains no executable checker source and remains after Task F deletes source, template, and embedded copies.

Generic synthetic migration fixtures test recognizer and transaction behavior without preserving another full checker copy.

Before Task F, an integration test also migrates an actual old generated repository.

After Task F, a newly built binary must still complete phase one, accept fresh proof, and complete phase two using the durable table plus generic fixture coverage.

## 16. Compatibility and migration

### 16.1 Current pull-request assets

The branch's generic action cutover is retained.

The branch's native Rust LOC deletion is retained.

The branch's authored replacement/removal preservation is retained.

The branch's generic receipt and work-evidence tests are retained where still applicable.

The Bash checker copies are removed.

The Bash portability tests are removed.

The Rust-root command rendering special case is removed.

### 16.2 Generated action identity

The canonical new target is `repo:file-budget`.

The canonical compatibility alias is `jig.file_budget`.

Fresh repositories do not advertise `jig.rust_file_loc` as canonical policy.

A narrow compatibility alias may remain during the supported migration window.

The friendly selector `rust-file-loc` may resolve only for migrated Rust contracts that explicitly retain the alias.

It is not a dedicated CLI command.

### 16.3 Existing Bash-generated repositories

Repositories remain source-revision pinned.

Their existing runtime and checker continue to work together.

The first update to the new contract epoch publishes repository contract/action files and the seed policy through the crash-recoverable transaction while retaining the Bash fallback; post-commit runtime-cache refresh is reported separately.

A later update may retire exact untouched generated Bash assets only after verifying the fresh successful receipt defined in section 15.8.

Authored replacements are not deleted.

Authored removals are not recreated.

### 16.4 Legacy exception markers

The new engine does not interpret `agentic-loc-exception:`.

The new engine does not interpret `@generated` as a bypass.

Migration preview identifies currently relied-upon markers.

Known generated scaffold paths become reasoned exclusions.

Other marked paths require waivers only when current comparison semantics would otherwise reject their debt or growth; legacy ordinary debt does not receive a needless waiver.

Required waivers become migration blockers with draft IDs, paths, and measured ceiling suggestions.

Jig never invents an authorization reason or expiry.

A human must supply a reviewed reason and expiry before adoption writes a valid waiver.

No permanent marker compatibility remains in the evaluator.

### 16.5 Contract versioning

The new generated native operation enters a new contract epoch.

Older contract epochs remain readable through their existing catalog projection.

The current runtime does not pretend an older contract declared the new action.

Migration creates the new action explicitly.

The implementation coordinates epoch numbering with the argv-runner and freshness features already planned under the parent epic.

Each incompatible feature lands in dependency order.

No two features independently claim the same fixed epoch number.

### 16.6 Source repository dogfooding

The Jig source repository will dogfood the universal action before the parent epic's authored-contract cutover closes.

The existing dogfood issue will depend on the seed/adopt/update task and will block final Bash deletion.

Its wording will target the current authored component/action epoch rather than freeze an obsolete numeric version.

## 17. Security and trust boundaries

### 17.1 Policy authority

Only the checked-in policy in the selected view is authority.

Ambient user configuration is not authority.

Environment variables do not change thresholds or path patterns.

CLI flags may select scope but do not weaken policy.

The policy and action are repository review authority, not an authorization boundary that Jig can defend from repository writers.

An authorized same-change policy relaxation, exclusion, waiver, action removal, or runner replacement can change or remove enforcement.

Jig makes those changes visible and attestable; branch protection, review permissions, and optional `CODEOWNERS` decide who may approve them.

### 17.2 Git authority

Git commands use the existing scrubbed environment.

Git pathspecs are literal where paths are data.

Git output is NUL-delimited.

Replacement objects, external diff, and text conversion are disabled.

Rename threshold and limit are pinned.

Git output is bounded.

Git operations are cancellable.

Symbolic refs resolve before execution.

### 17.3 Filesystem authority

Worktree paths are joined beneath the repository root.

Parent traversal is rejected.

Symlink following is disabled.

File identity is checked around reads.

Special files are not consumed as regular source.

### 17.4 Policy bypass resistance

Source comments cannot waive policy.

Generated strings cannot waive policy.

Wildcard waivers are forbidden.

Expired waivers fail.

The action's `"**"` input boundary includes policy and every possible governed repository path.

Policy changes trigger full governed-set evaluation.

Policy structural changes produce review-visible findings.

Those findings are not described as preventing a repository author from changing repository-owned policy.

Receipt time validity prevents an expired waiver from retaining a fresh passing receipt.

Comparison-side waiver inspection prevents removal from laundering authorized growth into ordinary grandfathered debt.

### 17.5 Resource exhaustion

Policy file size is bounded.

Rule count is bounded.

Pattern count is bounded.

Waiver count is bounded.

Candidate count is bounded.

Git output is bounded.

Per-file reads are bounded.

Total reads are bounded.

Findings are bounded for presentation.

Evaluation counts remain truthful when previews truncate.

Candidate and byte ceilings are explicit checked native configuration included in action input identity and receipts.

Lowering a ceiling can only block evaluation; it cannot turn a violation into success.

Adoption proposes higher ceilings when a legitimate repository exceeds generated defaults.

Deadline, cancellation, operating-system errors, and configured ceilings still fail closed as `Blocked`.

### 17.6 Secrets

The engine reads only governed repository files.

Ignored secrets are not discovered through untracked enumeration.

File contents never enter findings.

File contents never enter receipts.

Diagnostics never print content excerpts.

Paths are escaped through existing safe display logic.

## 18. Performance model

### 18.1 Expected cost

Scope discovery cost is dominated by bounded Git operations.

Measurement cost is linear in bytes read.

Evaluation cost is linear in candidates times compiled matcher cost.

The evaluator streams each required current or comparison-side content once.

### 18.2 Avoided cost

No compiler is started.

No language parser is loaded.

No dependency installation is required.

No source tree is copied.

No baseline ledger is regenerated.

### 18.3 Benchmark plan

Benchmarks will use generated generic repositories.

One fixture will contain many small source files.

One fixture will contain a smaller number of large source files.

One fixture will contain rename-heavy changes.

One fixture will contain many untracked files.

One fixture will contain multiple disjoint language rules.

Measurements will record scope collection and content measurement separately.

Measurements will record bytes read.

Measurements will record finding count.

Measurements will compare sequential evaluation with any proposed concurrency.

Concurrency will not be added unless measurement demonstrates material benefit.

Numeric safety limits will be selected from benchmark evidence and existing Jig observation limits.

### 18.4 Deterministic parallelism boundary

Version 1 may remain sequential.

If bounded parallel measurement is added, output order remains sorted.

Cancellation remains prompt.

Total-byte accounting remains atomic and fail closed.

The number of workers is bounded.

## 19. Test strategy

### 19.1 Pure policy tests

Parse a minimal valid policy.

Reject unknown fields.

Reject unsupported versions.

Reject duplicate rule IDs.

Reject empty include sets.

Reject rules without maxima.

Reject invalid threshold ordering.

Reject absolute patterns.

Reject parent traversal.

Reject ambiguous rule matches.

Reject duplicate waivers.

Reject duplicate waiver IDs.

Reject wildcard waiver paths.

Reject missing waiver reasons.

Reject missing waiver expiry.

Reject expired waivers.

Reject unmatched waivers.

Reject numeric overflow and configured cardinality limits.

Protect contract and policy authority paths from exclusion.

### 19.2 Counting tests

Count an empty file as zero lines.

Count a one-byte unterminated file as one line.

Count one LF as one line.

Count LF-terminated content correctly.

Count CRLF content equivalently.

Count an unterminated final segment.

Count embedded NUL bytes without unsafe string conversion.

Measure exact bytes.

Stream across buffer boundaries.

Cancel between reads.

Enforce per-file limits.

Enforce aggregate limits.

### 19.3 Debt tests

Pass a new compliant file.

Fail a new oversized file.

Warn near a line limit.

Warn near a byte limit.

Pass an unchanged oversized legacy file.

Pass a shrinking oversized legacy file.

Fail line-debt growth.

Fail byte-debt growth.

Fail one metric while another improves.

Retire debt after compliance.

Prevent debt reintroduction.

Apply active waiver ceilings.

Fail above waiver ceilings.

Fail expired waivers.

Fail removal of a baseline waiver while ordinary debt remains.

Report renewal, ceiling growth, and path transfer.

Table-test missing comparison-side policy, rename, rule reassignment, waiver-ID replacement, path transfer, removal with debt, and same-change waiver addition.

Prove that an authorized same-change limit relaxation or exclusion changes the result while remaining visibly reported.

### 19.4 Git scope tests

Resolve a merge-base comparison and persist every object ID.

Resolve an exact work-plan tree comparison.

Resolve an exact push-before comparison.

Resolve index-against-HEAD and an empty tree for an unborn repository.

Resolve a zero-selector unborn worktree as exact empty-tree authority and treat governed files as additions.

Handle a missing nonzero push-before with one bounded fetch attempt.

Block by default when that fetch fails.

Use strict inventory only under an explicit selector or checked-in native action configuration.

Prepare configured fallback as ready strict-inventory authority carrying the original request, typed failure, attempted identities, and digest.

Keep the same failure unavailable under `block`, and make a newly resolvable request stale at acceptance.

Never substitute `HEAD^` or an unrelated empty tree.

Include staged changes in worktree scope.

Include unstaged changes in worktree scope.

Include non-ignored untracked files.

Exclude ignored untracked files.

Read index blobs in staged scope.

Ignore unstaged content in staged scope.

Preserve rename ancestry.

Treat copies as additions.

Ignore deletions for current measurement.

Handle type changes safely.

Reject unsupported non-UTF-8 paths.

Exclude every `.agent/**` path from the candidate universe while observing `.agent/jig-contract.json` through ordinary contract authority.

Reject matched symlinks.

Reject matched gitlinks.

Detect changed-during-read files.

Ignore ambient rename-disable configuration.

Detect rename-limit degradation.

Reject unmerged index and intent-to-add ambiguity.

Disable replacement objects, external diff, and text conversion.

Use hash-format-correct empty trees.

Prove exact push-before selection never computes a merge base.

### 19.5 Cross-language conformance

One table-driven evaluator test applies identical facts and expectations to representative Rust, TypeScript/JavaScript, Python, Go, JVM, C/C++, C#, Ruby, PHP, and Swift paths.

Property tests vary path, byte content, threshold, and comparison-side measurement to prove syntax neutrality and monotonic debt.

Each template adapter has one renderer assertion proving that its selected stack contributes explicit policy data.

No fixture requires the corresponding compiler or package manager.

### 19.6 CLI tests

Check produces human findings.

Check produces structured JSON.

Audit reports all debt without default failure.

Strict audit fails on debt.

Explain identifies the effective rule.

Explain identifies an exclusion.

Explain identifies an active waiver.

Validate reports policy errors.

Direct diagnostics create no durable run.

### 19.7 Action integration tests

The default profile selects `repo:file-budget`.

The canonical alias resolves.

The native runner returns normalized findings.

An error finding fails a target.

Warnings preserve target success.

Receipts record findings and evidence.

Persisted and background execution reconstruct exact prepared comparison authority.

Work checks supply work-plan identity before planning.

Untrusted-plan acceptance rejects modified object IDs, merge bases, policy source or digest, provenance, and work-plan identity.

Acceptance rejects a modified policy or comparison preparation state, diagnostic count/digest, attempted object identity, requested exact object, peeled commit, or resolved tree.

Missing and invalid policies round-trip through `InvalidPolicy` and produce normalized failure findings after replay authentication.

Unavailable comparison authority under `block` round-trips through `ComparisonUnavailable` and produces a normalized blocked result.

Either preparation state changing before acceptance makes the plan stale.

Simultaneous invalid policy and unavailable comparison persist both states and deterministically report policy failure first.

Different commits with the same tree remain different exact-tree authority.

Checked-in native configuration round-trips, applies omitted defaults, rejects operation/config mismatch and over-hard-cap values, and remains readable in old contracts.

A ref moved before acceptance makes the plan stale.

A ref moved after durable acceptance does not change worker comparison authority.

Unrelated targets, missing or removed file-budget actions, and custom command replacements do not resolve or persist file-budget comparison authority.

Target-local `"**"` selection picks file budget without selecting unrelated actions in the same repository component.

Legacy epochs retain their prior component-aggregate affected behavior.

Work gates use the plan's exact comparison.

Finding truncation preserves total count and digest.

An active waiver sets receipt validity at the correct UTC boundary.

Expired time validity makes unchanged work-gate evidence stale.

The same expiry invalidates direct target status, work-check batch evidence, scoped and reusable gate proof, profile/latest evidence, and archival reuse.

A waiver crossing expiry during evaluation cannot record a passing receipt.

Evaluation digests are stable for identical facts and sensitive to every canonical domain field.

Resource-ceiling exhaustion blocks rather than passes or reports a policy violation.

Cancellation remains truthful.

Timeout remains truthful.

Source mutation blocks success.

### 19.8 Lifecycle tests

Fresh init seeds policy once and renders the action.

An unborn fresh repository passes its first authored-action check only when every generated file satisfies new-file limits.

Mixed templates render disjoint rules.

Minimal init omits policy.

Adoption previews debt.

Adoption preserves an existing policy.

Update preserves every existing policy byte-for-byte.

Recopy preserves every existing policy byte-for-byte.

Policy is absent from managed replacement and retirement authority.

Recopy preserves action replacement.

Recopy preserves action removal.

Recopy preserves alias choice.

Recopy preserves profile membership.

Exact generated Bash assets retire safely.

Phase one retains generated Bash assets while installing the native action.

Phase two retires them only with a fresh successful receipt bound to runner, action, policy, source, comparison, evaluation, and time validity.

Missing, stale, failed, or expired receipts retain Bash and explain the rerun.

Noninteractive update, failed apply, custom action replacement, action removal, and modified Bash assets preserve recoverability and authored authority.

A phase-two update that also changes staged native authority or governed source commits those changes while retaining Bash until a receipt for the post-update state exists.

Failure injection before and after every phase-one and phase-two publication/removal restores absence, bytes, type, permissions, and manifest metadata byte-for-byte when destinations remain transaction-owned.

Concurrent destination mutation is detected and preserved; the foreign path is not overwritten, the corresponding preimage remains recoverable, and the command reports incomplete rollback.

Subprocess termination before and after every publication, removal, progress flush, and commit marker leaves a durable journal that the next invocation rolls back when uncommitted or cleans up when committed.

Committed-journal cleanup and repeated recovery are idempotent.

Seed policy publication shares the crash-recoverable transaction with managed action/contract wiring while remaining absent from managed replacement authority.

A post-Task-F binary recognizes actual legacy generation digests without embedding checker source and completes the generic two-phase migration fixture.

Authored Bash replacements remain untouched.

### 19.9 CI and rendered repository tests

Rendered backend fixture runs file budget.

Rendered unborn fixture exercises zero-selector empty-tree provenance through both direct and authored-action paths.

Rendered mixed-stack fixture runs file budget.

Rendered tooling-only fixture omits or explicitly configures file budget.

Pull-request baseline resolution is exercised.

Default-branch push range is exercised.

Missing push-before behavior is exercised.

Fetch success, fetch failure, default blocking, and explicit strict-inventory fallback are exercised.

Same-change policy relaxation, exclusion, waiver addition, action removal, and runner replacement have documented expected CI results.

Linux passes.

macOS passes.

No Bash 3.2-specific checker test remains.

## 20. Documentation plan

Configuration documents the policy schema.

Configuration documents debt semantics.

Configuration documents exact counting.

Configuration documents exclusions and waivers.

Configuration documents direct commands.

Public contract documents the new action epoch.

Public contract documents compatibility aliases.

Public contract documents normalized findings and evidence.

Adoption documents proposal and preservation behavior.

Platform support documents that the engine uses the existing Jig binary on Linux and macOS.

Repository intent describes file budget as generic reviewability policy.

Extraction matrix removes stale classification of Rust LOC behavior.

Migration notes explain removal of source-comment bypasses.

Examples use only generic fixture names.

## 21. Delivery architecture

### 21.1 Feature: universal repository file budgets

The new feature is a P1 child of the existing stack-neutral monorepo epic.

It explicitly supersedes the implementation non-goal from closed feature `.7`.

It does not reopen `.7`.

It depends on the architectural boundary delivered by `.7.2`.

### 21.2 Task A: pure policy and evaluator

Outcome: a focused library parses versioned policy and evaluates supplied facts deterministically.

This task owns policy DTOs.

This task owns strict validation.

This task owns glob matching.

This task owns line and byte measurement semantics.

This task owns debt and waiver decisions.

This task owns pure cross-language tests.

This task does not own Git.

This task does not own CLI.

This task does not own templates.

This task blocks the CLI engine and lifecycle tasks.

### 21.3 Task B: canonical Git comparison and scope

Outcome: Jig exposes one resolved-comparison service and rename-preserving execution-time scope to internal consumers.

This task extracts from existing `git_receipts` code.

This task owns merge-base, exact-tree, index, and strict-inventory comparison resolution.

This task owns worktree, index, and all-tracked views.

This task owns untracked inclusion.

This task owns rename ancestry.

This task owns copy-as-new behavior.

This task owns prepared comparison anchors, scope completeness, and flattened paths for generic affected selection.

This task supplies a target-local matching primitive for non-empty action inputs without changing contract projection by itself.

This task does not persist unbounded path inventories or publish an external protocol.

This task blocks the durable native contract.

### 21.4 Task C: durable native context, typed results, and contract epoch

Outcome: persisted and background native actions receive exact prepared comparison authority and return typed, bounded, time-valid results.

This task owns `PreparedNativeInputV1` in the immutable run plan.

This task owns independent authenticated policy and comparison preparation states plus bounded diagnostic digests and deterministic conclusion precedence.

This task owns conversion of configured missing-comparison fallback into authenticated ready strict-inventory authority with original failure evidence.

This task preserves requested object, peeled commit, and tree identities separately for exact-tree authority.

This task owns `ComparisonRequestV1` across CLI, MCP, work-check, and untrusted-plan acceptance boundaries.

This task owns `NativeActionContext` and `NativeActionResult`.

This task owns finding totals, truncation, and digest metadata.

This task owns optional receipt `valid_until_ms` and work-gate time freshness.

This task propagates validity through target, batch, scoped, reusable, latest, and archival evidence projections.

This task owns the typed checked-in `NativeFileBudgetConfigV1`, backward defaults, runtime hard-cap validation, plan authentication, and receipt projection.

This task owns the backward-readable run-plan and receipt schema changes.

This task owns one serialized contract epoch coordinated with literal-argv work.

This task owns the epoch-gated switch to target-local non-empty input matching, legacy-epoch component behavior, dependency propagation, and compatibility tests over the primitive from Task B.

This task depends on Tasks A and B.

### 21.5 Task D: file-budget engine, CLI, and CI

Outcome: Jig ships native action execution plus check, audit, explain, validate, and provider-neutral CI behavior.

This task owns mapping pure diagnostics into `Finding` values.

This task maps failed preparation states into normalized `Failure` or `Blocked` target results without pretending evaluation ran.

This task owns human and versioned direct JSON output.

This task owns comparison flags, exit codes, resource bounds, and strict-inventory fallback.

This task executes Task C's prepared strict-inventory fallback, default blocked outcome, and explicit exact-tree selection without changing comparison authority.

This task owns CI merge-base and exact push-before integration.

This task declares generated action inputs as `"**"` and proves hidden-path coverage.

This task depends on Task C.

### 21.6 Task E: seed-once policy and repository lifecycle

Outcome: fresh and adopted repositories receive a replaceable action and a policy that becomes authored state after first write.

This task owns repository-model pattern contributions and seed rendering.

This task owns adoption preview and human-completed waiver migration.

This task owns byte-for-byte policy preservation.

This task owns action replacement, removal, alias, and profile-membership preservation.

This task owns the two-phase migration protocol and fresh-receipt proof before managed Bash retirement.

This task owns the real full-update transaction, fsynced crash journal and next-run recovery, seed-once authored publication classification, failure rollback, and durable legacy generation/digest table.

This task owns rendered fixtures.

This task does not put policy under managed replacement authority.

This task depends on Task D.

### 21.7 Existing source dogfood task

Outcome: the existing `.1.2` task authors the Jig source repository on the allocated contract epoch and proves `repo:file-budget` through ordinary repository authority.

Its title and acceptance criteria stop freezing contract v6.

It owns source dogfood configuration, source policy, default profile membership, CI/release invocation, and the passing native action on this branch.

It depends on Task E.

### 21.8 Task F: final Bash deletion and compatibility cleanup

Outcome: pull request `#18` contains the proven universal implementation and no temporary Bash checker.

This task owns deletion of source, template, and embedded Bash assets from this source branch only.

Downstream recognition and retirement mechanics remain owned by Task E and must work in a newly built post-deletion binary.

This task owns replacement of Bash-specific tests.

This task owns compatibility alias migration notes and final stale-semantics searches.

This task depends on the existing source dogfood task and closed `.7.2`.

### 21.9 Dependency graph

```text
stack-neutral epic
└── universal file-budget feature
    ├── A: pure policy/evaluator
    ├── B: canonical comparison/scope
    ├── C: durable native contract       depends on A, B
    ├── D: engine, CLI, and CI           depends on C
    ├── E: seed/adopt/update             depends on D
    └── F: final Bash deletion           depends on source dogfood, closed .7.2

existing authored-contract dogfood task  depends on E
```

### 21.10 Why there are no planning beads

This document is planning work.

The four plan reviews are planning work.

Beads will represent only delivery outcomes A through F and their parent feature.

No bead will exist merely to review or polish this plan.

### 21.11 Why the graph does not depend on unrelated epic features

The engine can use the current generic action runner model.

It does not require the future foreground `jig run` command.

It does not require action string arguments.

It does not require the future argv runner, but Task C serializes contract epoch allocation with it.

It adds generic time validity without claiming the future input-scoped freshness design.

It does not change dev-app lifecycle.

Those features remain independent.

## 22. Acceptance criteria by layer

### 22.1 Product acceptance

One implementation governs all configured text-based languages.

No language compiler is required.

No external LOC binary is required.

No Bash checker remains in generated defaults.

Repositories can replace or remove the action and author the seeded policy.

Under unchanged ordinary policy, debt cannot increase without an explicit active waiver.

### 22.2 Policy acceptance

The policy is versioned and strict.

Rules are disjoint.

Lines and bytes are independently enforceable.

Exclusions are reasoned.

Waivers are exact and expiring.

Waiver IDs are stable and removal cannot launder authorized growth.

Policy changes are visible findings.

### 22.3 Scope acceptance

Comparison strategy and every object identity are exact and persisted.

Worktree scope includes staged, unstaged, and untracked content.

Index scope reads index content.

Renames inherit ancestry.

Copies receive no ancestry.

Unsupported or incomplete scope fails closed.

Affected selection consumes the same comparison service.

### 22.4 Runtime acceptance

Pure diagnostics map to existing findings through a typed native-action result.

Results use the existing run lifecycle.

Receipts use the existing storage.

Finding totals and digests remain truthful when previews truncate.

Calendar validity makes waiver-bearing receipts stale at expiry.

Cancellation and timeouts remain truthful.

Source mutation cannot yield success.

### 22.5 Lifecycle acceptance

Fresh templates seed appropriate explicit rules once.

Adoption previews existing debt.

Update preserves every existing policy byte-for-byte.

Recopy preserves policy and authored action authority.

Policy never enters managed replacement authority.

Migration removes temporary Bash assets safely.

### 22.6 Compatibility acceptance

Closed `.7` history remains unchanged.

Historical append-only state remains unchanged.

Supported older contracts remain readable.

New generated behavior uses a compatible new contract epoch.

The existing source dogfood task consumes the lifecycle-ready universal action before Bash deletion.

## 23. Validation gates

Each implementation slice runs focused crate tests.

Backend changes finish with `scripts/jig check test` through a freshly built development binary.

Contract changes run `scripts/jig check contract`.

Formatting runs `scripts/jig check fmt`.

Clippy runs `scripts/jig check clippy`.

The file-budget action runs against the final branch.

Rendered repository fixtures run independently.

Linux and macOS CI remain required.

The final diff is searched for stale `rust-file-loc` native semantics.

The final diff is searched for checker Bash copies.

The final diff is searched for source-comment policy bypasses.

The final Beads graph is cycle-free.

## 24. Rollout and recovery

### 24.1 Commit slicing

Task A lands as an independently testable pure crate.

Task B lands as an independently testable comparison/scope extraction.

Task C lands the durable native contract only after both foundations pass.

Task D lands the engine and UX over that contract.

Task E lands seed-once lifecycle behavior.

The existing source dogfood task proves the authored action.

Task F deletes the Bash implementation last.

### 24.2 Same-branch continuation

All slices continue on `feat/template-owned-loc-action` as requested.

The pull request remains the review surface.

Each slice is committed independently.

The branch is rebased or merged from current default-branch authority before final review according to repository policy.

### 24.3 Recovery before final cutover

Before Task F, the Bash checker remains functional on the branch.

Tasks A through E can be reverted according to their dependency order without losing the generic-action cutover.

Task F deletes Bash assets only after the universal action passes source dogfood and rendered fixtures.

### 24.4 Recovery after cutover

If the native engine regresses before merge, revert Task F and retain the reviewed infrastructure while source dogfood is repaired.

Do not restore native Rust-specific policy.

Do not partially restore one generated Bash copy.

The temporary implementation must remain atomic across source, template, and embedded assets.

## 25. Review-resolved design choices

The bounded prepared comparison descriptor is serialized in the run plan; large scope entries remain execution-local.

Default audit reports active ordinary debt without failing; `--strict` fails on all unwaived ordinary debt.

The engine supports byte limits, but templates emit none until calibrated by repository evidence.

Waiver expiry is inclusive through the named UTC date and receipt validity ends at the next UTC boundary.

Unmatched waiver paths are errors in every mode.

A semantic policy change forces a full governed-set scan, including staged mode against the index policy.

NUL-containing regular files remain byte-measurable.

Direct check without a comparison flag uses configured-default-branch merge-base semantics when history exists and exact empty-tree `UnbornWorktree` semantics when it does not.

The first-party action is native immediately; literal-argv work is independent except for serialized epoch allocation.

Durable native context/result and the contract epoch live in Task C.

The existing `.1.2` task owns source dogfood; final deletion is a later Task F.

## 26. Decisions already made

The engine is generic and language-neutral.

The engine is dependency-free for adopted repositories.

Policy is checked in and repository-owned.

Policy lives outside `.jig.toml`.

Physical lines and bytes are the version 1 metrics.

Debt is independently monotonic per metric.

Source comments do not waive policy.

Waivers are exact, bounded, reasoned, and expiring.

Git is not reimplemented in the evaluator crate.

The existing Finding and receipt journals are reused through a typed native result extension.

Prepared native input is durable; execution never depends on planner memory.

`ActionSpec.inputs = ["**"]` is the sole conservative applicability and path-freshness declaration.

Policy is seeded only when absent and becomes byte-preserved authored state immediately.

Comparison semantics are explicit: merge base, exact tree, index against HEAD, or strict inventory.

Waiver authorization continuity and calendar receipt validity prevent laundering and stale passes.

The Bash checker is temporary and will not merge.

Closed feature `.7` remains closed and unchanged.

The work continues under the existing stack-neutral epic.

## 27. Source grounding

The current branch demonstrates the generic action cutover and Bash implementation cost.

`crates/jig/src/git_receipts.rs` owns bounded repository observation and plan baselines.

`crates/jig/src/git_receipts/scope.rs` owns baseline-to-worktree changed-path discovery and gate scope fingerprints.

`crates/jig-contract/src/run.rs` owns normalized findings and target results.

`crates/jig-contract/src/repository.rs` owns action runners and action declarations.

`crates/jig/src/runtime/run_execution.rs` owns target execution and normalized capture.

`crates/jig/src/runtime/run_execution/source_epoch.rs` owns source mutation checks.

`crates/jig/src/bootstrap/repository_model.rs` owns generated action and profile composition.

`docs/public-contract.md` owns contract epoch compatibility policy.

Git documentation defines machine-readable diff and rename records.

Git documentation defines the staged, worktree, and tree comparison modes used by the adapter.

No load-bearing decision depends on an unverified external library capability.

## 28. Planning workflow revision log

### Initial architecture synthesis

The initial synthesis combined repository inspection with three isolated memos.

The runtime memo favored a pure evaluator and Jig-owned typed scope.

The policy memo introduced the two-dimensional debt vector and rejected source-marker bypasses.

The delivery memo preserved closed history and proposed a non-duplicative feature DAG.

The initial synthesis chose a private versioned scope model rather than an external protocol.

The initial synthesis chose a native first-party action and a new contract epoch.

These choices remain subject to at least four sequential plan reviews.

### Review round 1

Round 1 performed an architecture challenge against the full plan, run-plan persistence, native dispatch, Git scope, receipts, bootstrap lifecycle, public contract, and existing Beads graph.

It found six structural blockers: in-memory-only native scope, ambiguous comparison semantics, duplicated/under-selecting action inputs, process-shaped native results, expiry absent from freshness plus waiver laundering, and contradictory managed-versus-authored policy ownership.

The plan now persists a bounded prepared comparison descriptor and reconstructs scope during execution.

It defines merge-base, exact-tree, index, and strict-inventory comparisons.

It makes `inputs = ["**"]` the sole conservative planner boundary.

It adds typed native results with bounded finding metadata and receipt validity.

It compares stable waiver authorization across policies and rejects removal with debt.

It makes policy seed-once authored state.

It also split scope, measurement, and evaluation identities; pinned Git/filesystem safety; simplified thresholds and byte defaults; resolved CLI outcomes; and rebuilt the delivery graph around durable contract work and existing source dogfood.

The changes are structural rather than marginal, so steady state has not been reached.

Post-integration validation found no unresolved design-question section, no whitespace errors, and one acyclic delivery path with source dogfood owned exactly once.

Five sampled load-bearing decisions retain explicit rationale: the pure crate, Jig-owned Git service, native default, conservative `"**"` input boundary, and seed-once policy lifecycle.

The document remains self-contained at the project level; task-local command sequencing is intentionally deferred to future ExecPlans.

### Review round 2

Round 2 challenged product, security, CLI/CI, and migration semantics against current planning, MCP, work-check, affected-selection, update, and evidence code.

It found that work-plan identity currently arrives too late for the proposed immutable input, untrusted-plan validation must authenticate caller-supplied OIDs, push-before comparison lacked a request surface, missing-history fallback was undecided, and Bash retirement lacked an executable proof transaction.

The plan now defines one optional closed comparison request across planning entry points and resolves it lazily for selected built-in file-budget targets, including work-plan identity before planning.

Untrusted-plan acceptance re-derives authority and rejects stale or modified plans; accepted workers then use persisted OIDs only.

Exact-tree push authority is explicit, unavailable nonzero push history blocks by default, and strict inventory requires explicit authority.

Lifecycle migration is two-phase: install while retaining Bash, produce a fresh bound native receipt, then retire exact assets in a later transactional update.

The revision also states that repository policy is review authority rather than an engine-enforced authorization boundary, removes the work-gate not-applicable shortcut, defines the evaluation-digest domain, makes resource exhaustion blocked, and closes the waiver-expiring-during-evaluation race.

These changes are still structural, so steady state has not been reached.

Post-integration validation found balanced code fences, no fallback ambiguity, no work-gate not-applicable claim, no whitespace errors, and one explicit authority path from request through acceptance, execution, and receipt.

Five sampled decisions retain rationale and adversarial tests: acceptance-time re-resolution, post-acceptance OID stability, default blocking on missing push history, policy-as-review-authority, and two-phase retirement.

### Review round 3

Round 3 challenged delivery slicing, backward compatibility, update/recopy transactions, affected selection, freshness projections, resource configuration, and post-deletion migration support.

It found that current full update has no whole-operation rollback, resource ceilings had no checked-in DTO, `"**"` would broaden other actions through component aggregation, target-only validity would leave reusable batch evidence fresh, eager comparison resolution would burden custom replacements, and retirement recognition would disappear with the Bash sources.

The plan now assigns Task E a real full-update transaction with seed-once authored publication, under-lock receipt revalidation, concurrent mutation checks, failure injection, complete rollback for transaction-owned states, foreign-write preservation, and a precise repository-file versus runtime-cache boundary.

Task C now owns typed native configuration with versioned defaults and immutable hard caps, plus validity propagation across every evidence projection.

Round 3 introduced target-local non-empty input matching; Round 8 later split its primitive into Task B and the epoch-gated behavior into Task C.

Comparison preparation is lazy and absent for unrelated or authored replacement targets.

A durable generation/digest table retains downstream retirement authority after Task F deletes checker sources.

The changes remain structural, so a steady-state review is still required.

Post-integration validation found balanced code fences, no stale transaction claim, no eager-comparison claim, no selector grammar contradiction, no whitespace errors, and an acyclic graph with downstream retirement mechanics owned only by Task E and source deletion owned only by Task F.

Five sampled decisions retain rationale and tests: target-local affected selection, lazy prepared input, hard-cap-separated native configuration, all-waiver validity propagation, and durable digest recognition.

### Review round 4

Round 4 audited the full revised authority model, lifecycle transaction, evidence path, and delivery graph for steady state.

It found two remaining structural contradictions: zero-selector checks in unborn repositories lacked comparison authority, and phase-two retirement could rely on a receipt for pre-update authority while changing that authority in the same transaction.

The plan now uses a hash-format-correct empty tree with dedicated `UnbornWorktree` provenance for unborn local worktrees.

Phase two now compares the staged post-update authority and evaluated-source identity with the receipt, excluding only exact legacy retirement and deterministic manifest bookkeeping; other changes commit while retaining Bash for a fresh proof.

The review also closed the version 1 `.agent/**` candidate-universe boundary.

Because the first two changes are structural, Round 4 did not establish steady state.

Post-integration validation found balanced code fences, no whitespace errors, one explicit unborn-worktree authority path, and no pre-update-only retirement claim.

### Review round 5

Round 5 audited prepared-input failure paths and exact-tree authority preservation.

It found that the durable contract could represent only a valid policy plus resolved comparison even though the product promises normalized invalid-policy failures and unavailable-comparison blocks.

It also found that exact-tree resolution discarded the requested commit/event identity by retaining only the tree OID.

The plan now persists independently authenticated policy and comparison preparation states, including ready, invalid-policy, and comparison-unavailable variants.

Acceptance replays and authenticates both states; execution applies deterministic policy-first failure precedence and maps failed preparation to truthful target results without claiming evaluation ran.

Exact-tree authority now retains requested identity, peeled commit when present, and resolved tree separately.

These durable-schema changes are structural, so Round 5 did not establish steady state.

Post-integration validation found balanced code fences, no whitespace errors, outcome-conditional digest wording, and separate requested/commit/tree fields in schema, evidence, and tamper tests.

### Review round 6

Round 6 challenged simultaneous preparation failures, concurrent-write rollback, and direct-command resource authority.

It found that one mutually exclusive outcome could not preserve both invalid policy and unavailable comparison authority, and it left conclusion precedence unspecified.

It also found that exact rollback and preservation of an arbitrary concurrent writer are mutually incompatible, and that built-in direct diagnostics lacked a source for effective resource ceilings after action replacement or removal.

The plan now prepares policy and comparison independently, authenticates both, and reports invalid policy before unavailable comparison when both fail.

Rollback now restores only transaction-owned states; foreign concurrent bytes are preserved with retained preimages and explicit incomplete-recovery guidance.

Direct diagnostics use versioned built-in defaults plus explicit hard-capped resource flags, independent of checked-in action configuration, and record their effective configuration in output and digests.

These are implementation-changing contract revisions, so Round 6 did not establish steady state.

Post-integration validation found balanced code fences, no whitespace errors, no mutually exclusive preparation outcome, no unconditional rollback claim, and explicit direct-command configuration authority.

### Review round 7

Round 7 found no authority-model or delivery-graph regression.

It identified one missing `diagnostics_count` field required by invalid-policy authentication and one undefined crash boundary in the multi-file update transaction.

The schema now persists the total diagnostic count alongside bounded preview and digest.

The lifecycle now promises in-process failure atomicity plus a fsynced, worktree-specific Git-metadata journal, transaction-owned rollback, foreign-write preservation, and deterministic next-run recovery.

Because crash recovery changes Task E's delivery contract, Round 7 did not establish steady state.

Post-integration validation found balanced code fences, no whitespace errors, an authenticated diagnostic count, and one consistent in-process-plus-crash-recovery transaction claim.

### Review round 8

Round 8 found no new product-authority defect and confirmed the delivery graph remains acyclic.

It identified one crash-recovery proof gap and one task-ownership dependency gap.

Uncommitted recovery now always rolls back and never resumes phase-two retirement, so expiry or source mutation during downtime cannot authorize further deletion; only committed journals receive cleanup.

Task B now owns only the target-matcher primitive, while Task C owns the new-epoch semantic switch, legacy projection, dependency propagation, and compatibility tests.

Because both corrections change delivery acceptance, Round 8 did not establish steady state.

Post-integration validation found balanced code fences, no whitespace errors, one uncommitted-recovery direction, and no epoch ownership cycle between Tasks B and C.

### Review round 9

Round 9 confirmed the delivery DAG remains acyclic and found two comparison-preparation contradictions.

Configured strict-inventory fallback had no durable transition from requested-comparison failure, and zero-selector wording failed to distinguish historical from unborn repositories.

Task C now converts configured fallback into authenticated ready strict-inventory authority carrying original request and failure evidence; `block` preserves `ComparisonUnavailable`.

Zero selectors now mean configured-default-branch merge base only with history and exact empty-tree `UnbornWorktree` authority without history.

Because fallback preparation changes Task C acceptance, Round 9 did not establish steady state.

Post-integration validation found balanced code fences, no whitespace errors, one explicit fallback transition, and consistent history-aware zero-selector wording.

### Review round 10

Round 10 performed a strict structural audit after every prior correction.

It found no high-confidence contract contradiction, impossible intermediate state, unresolved implementation-changing choice, or DAG defect.

The frozen delivery path is `A + B -> C -> D -> E -> source dogfood -> F`.

No plan revision was warranted.

Structural steady state has been reached.

## 29. Beads conversion and graph polish

The reviewed delivery architecture is represented by feature
`feat-codex-resume-generic-monorepo-zac.8` and its six concrete delivery tasks.

The dependency graph preserves both parallel foundation work and the guarded
cutover sequence:

```text
.8.1 pure policy/evaluator -----+
                                +-> .8.3 durable contract
.8.2 Git/scope primitives ------+      -> .8.4 engine/CLI/CI
                                       -> .8.5 lifecycle migration
                                       -> .1.2 source dogfood
                                       -> .8.6 Bash deletion
```

The existing closed Bash tasks remain unchanged as historical evidence. The
current source-dogfood task now owns the authored epoch cutover exactly once,
and final source deletion remains a separate downstream proof boundary.

Six graph-polish passes reached steady state:

1. Cycle and critical-path inspection confirmed an acyclic graph and the
   intended `A + B -> C -> D -> E -> source dogfood -> F` path.
2. Parentage inspection confirmed one feature parent for every delivery task
   and no orphaned outcome.
3. Boundary inspection confirmed that comparison primitives, epoch semantics,
   lifecycle migration, source dogfood, and Bash deletion have distinct owners.
4. Acceptance inspection confirmed that each task is independently described,
   testable, and free of planning-only work.
5. Priority and readiness inspection confirmed that Tasks A and B are the only
   initially ready implementation tasks and that the P1 chain matches the
   active pull-request cutover.
6. Alert and suggestion inspection found no actionable alert. Keyword-only
   dependency suggestions were rejected where they duplicated ancestry,
   reversed the dogfood chain, introduced a cycle, or coupled this feature to
   an intentionally independent freshness feature.

`br --no-db sync --flush-only` confirms that the authoritative JSONL has no
unflushed issue mutations. `br --no-db dep cycles --json` reports zero active
cycles.

## 30. Final planning-workflow completion checklist

- [x] The document is self-contained for a fresh implementation agent.
- [x] Every load-bearing architecture choice has rationale.
- [x] Every delivery task has an independently verifiable outcome.
- [x] The dependency graph is acyclic.
- [x] There are no planning-only Beads.
- [x] Ten sequential strong-model reviews are integrated.
- [x] The most recent review is marginal rather than structural.
- [x] The plan contains no unresolved implementation-changing question.
- [x] Beads descriptions preserve the plan's delivery boundaries.
- [x] Beads dependencies preserve the plan's graph.
- [x] `bv --robot-insights` reports no cycles.
- [x] Beads JSONL is flushed after mutation.
- [x] Fixture names satisfy open-source hygiene.
- [x] The plan and Beads changes are committed to the continuation branch.
- [x] Pull request `#18` points at the committed planning continuation.
