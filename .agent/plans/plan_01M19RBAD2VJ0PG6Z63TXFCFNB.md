# Ship the native file-budget engine, CLI, and CI behavior

This ExecPlan implements Bead `jig-sh-generic-monorepo-zac.8.4`. The observable result is that a prepared `jig.file_budget` repository action evaluates repository bytes in-process and that `jig file-budget check|audit|explain|validate` exposes the same bounded diagnostic implementation without creating a run or receipt. Comparison authority remains explicit: normal checks use a default-branch merge base, unborn worktrees use the repository hash-format empty tree, staged checks use the index, explicit trees never become merge bases, strict inventory is opt-in, and push CI supplies its exact before identity.

The plan baseline is Git commit `97db14b7677ad277724a4ee14e4378eeb908fd82`. The worktree also contains the intentional, uncommitted Task C implementation from Bead `.8.3`; preserve that work because this task consumes its contract-v7 prepared-input and native-result seams.

## Progress

- [x] (2026-08-30 16:33Z) Claimed Bead `.8.4`, read the repository and crate guides, and inspected the universal file-budget design plus the Task A-C implementation boundaries.
- [x] (2026-08-30 16:34Z) Built the current `jig` binary and opened structured work `plan_01M19RBAD2VJ0PG6Z63TXFCFNB` against the exact Git baseline.
- [x] (2026-08-30 18:03Z) Implemented bounded scope materialization, policy reconstruction, byte measurement, waiver-target observation, evaluation, normalized findings/evidence, evaluation identity, and validity.
- [x] (2026-08-30 18:03Z) Added the direct `jig file-budget` command family with exact selector grammar, independent built-in configuration, human/JSON output, and stable exit codes.
- [x] (2026-08-30 18:03Z) Wired provider-neutral exact push-before comparison authority through MCP, direct diagnostics, and prefixed repository-check flags; bounded the one exact-object fetch to 60 seconds; and declared the built-in action's repository-wide input boundary.
- [x] (2026-08-30 18:03Z) Added focused runtime, CLI, integration, and adversarial tests for arbitrary bytes, waiver facts, worktree/index/inventory authority, exact push-before, resource ceilings, bounded previews, and no-receipt direct execution.
- [x] (2026-08-30 17:48Z) Built the development binary, passed focused crate/runtime/CLI checks, passed all eight applicable Jig gates, passed the required final `scripts/jig check test`, inspected fresh evidence and historical failed receipts, and completed the acceptance audit.

## Surprises & Discoveries

- The prior Task C work is intentionally uncommitted in this worktree and already makes contract v7/run-plan schema 3 durable. It leaves a deliberate `file_budget.engine_pending` result in `crates/jig/src/runtime/tool_execution.rs`; replacing that result is the principal execution seam for this task.
- Task B already staged private bounded primitives in `crates/jig/src/git_receipts/`: `capture_scope_v1_with_cancellation` preserves rename ancestry and current content sources, while `observe_exact_paths_v1_with_cancellation` independently validates waiver targets in worktree, index, and inventory views. The engine should consume these rather than issue parallel Git queries.
- The pure `jig-file-budget` crate already owns arbitrary-byte streaming measurement and deterministic evaluation. Runtime code must adapt repository content into its fact types without moving Git or filesystem authority into the pure crate.

## Decision Log

- Decision: keep one runtime evaluator shared by repository actions and direct diagnostics.
  Rationale: the direct command is a leaf UX, not a second policy engine; using one evaluator prevents semantic and digest drift.
  Date/Author: 2026-08-30 / Codex.
- Decision: keep durable prepared inputs bounded and materialize bytes only inside the authenticated source epoch.
  Rationale: Task C deliberately persists authority rather than unbounded content; Task D must not weaken that replay boundary.
  Date/Author: 2026-08-30 / Codex.
- Decision: represent operational incompleteness as `Blocked`, never as policy `Failure` or success.
  Rationale: resource ceilings, scope issues, cancellation, timeouts, source mutation, and unavailable comparison cannot prove either compliance or a policy violation.
  Date/Author: 2026-08-30 / Codex.
- Decision: prefix comparison selectors on the generic repository-check surface as `--comparison-*`, while retaining the shorter selector names on `jig file-budget`.
  Rationale: checker-owned arguments may legitimately include names such as `--staged`; a prefixed orchestration namespace preserves existing external checker CLIs while still carrying typed provider-neutral comparison authority.
  Date/Author: 2026-08-30 / Codex.

## Outcomes & Retrospective

The native file-budget path is complete. Ready repository actions now authenticate prepared policy/comparison authority, materialize the bounded selected view, observe waiver targets independently of changed candidates, stream arbitrary regular bytes without following symlinks, evaluate through `jig-file-budget`, and return normalized success/failure/blocked conclusions with complete finding/evaluation digests and bounded previews/evidence. The direct `jig file-budget check|audit|explain|validate` family uses that same evaluator, has explicit selector grammar and stable 0/1/2/3 exits, and creates no run or receipt. Generic repository checks can carry provider-neutral exact push-before authority through `--comparison-*`; explicit exact trees remain exact trees and the single exact-object fetch is bounded to 60 seconds.

Compatibility is additive at contract v7/run-plan schema 3. Empty authored native file-budget inputs normalize to the conservative repository-wide `"**"` boundary; nonempty custom inputs are rejected. The prefixed generic comparison flags avoid colliding with existing checker-owned arguments. Contract epochs below 7 reject the new explicit comparison request instead of silently losing authority.

Validation passed with development binary `target/debug/jig`. Focused evidence included 52 `jig-file-budget` tests, 12 runtime-engine tests, direct CLI and repository-comparison integrations, strict Clippy for both owning crates, and LOC/format checks. Structured work batch `receipt_01M19WDYA724GTPWQJYGHS4WG9` passed all eight configured gates: 2,467 core tests, 107 frontend tests, 442 vault tests, and 209 process tests. The required direct `scripts/jig check test` then passed 3,225 tests with 2 skipped under receipt `receipt_01M19WHR387Y7BEE2DY6AT24XK`. `git diff --check` is clean, the current gate evidence is fresh, and retained earlier failed receipts document the fix-forward iterations.

Task E remains intentionally deferred: seed policy generation, authored-action lifecycle migration, update/recopy preservation, and Bash retirement are not part of this task.

## Context and orientation

`crates/jig-file-budget` is a pure crate. `policy.rs` parses strict policy v1, `measurement.rs` streams LF and byte counts under per-file and aggregate budgets, and `evaluation.rs` consumes explicit current/comparison measurements plus exact waiver-target facts. It must remain free of Git, filesystem, runtime, or contract dependencies.

`crates/jig/src/repository/native_input.rs` reads policy authority from the selected current view and resolves the comparison request into `PreparedNativeInputV1`. `crates/jig/src/git_receipts/change_scope.rs` then reconstructs a bounded `ScopeSnapshotV1` from those exact object IDs. A scope entry names its current worktree path or index blob and, for modified or renamed files, the comparison blob/path. `exact_path.rs` observes policy-named waiver paths separately so an unrelated changed-only scope cannot hide a missing, mismatched, or unsupported waiver target.

`crates/jig/src/runtime/tool_execution.rs` is the typed native-action dispatch boundary. Task C already maps invalid-policy preparation to `Failure`, missing comparison preparation to `Blocked`, and transports `NativeActionResult` through target results and receipts. This task replaces the ready-state placeholder with actual evaluation while retaining the earlier failure precedence and result transport.

`crates/jig/src/cli.rs` defines root Clap commands and dispatches them near the bottom of the file. The new direct command belongs in a focused `crates/jig/src/cli/file_budget.rs` options module and a runtime-facing implementation module. It must construct policy and comparison preparation in memory, invoke the shared evaluator directly, print either a human report or versioned JSON, return exits 0/1/2/3 as specified, and never create `.agent/state` run or receipt records.

The full design and acceptance contract is `docs/plans/universal-file-budget.md`, especially sections 12-14, 17-19, 21.5, 22, and 23. Task E owns generated policy/action lifecycle work; do not absorb its seed/update transaction. Task D nevertheless owns any runtime/CI request surface needed to supply exact push-before authority and the built-in action's `"**"` applicability semantics.

## Plan of work

First, add a runtime evaluator that accepts `RepoContext`, authenticated prepared input, deadline, and cancellation. Re-read and parse the selected current policy, verify its prepared raw and semantic digests, capture the exact scope from the persisted comparison, and independently observe every current and comparison-policy waiver target required by the pure evaluator. Reconstruct comparison-side policy from the exact baseline tree when one exists; strict inventory has no comparison policy. Any mismatch with prepared authority or incomplete scope blocks.

For each candidate, classify before reading content so excluded/outside paths do not consume byte budget. Enforce the configured candidate ceiling. Open worktree files with no symlink following and identity checks around streaming reads; read index and comparison blobs only through bounded, scrubbed Git helpers. Measure arbitrary bytes with `measure_stream_v1`, sharing one aggregate budget. Map current/baseline measurements and rename ancestry into `EvaluateFileV1`. Convert every operational read/scope/resource problem into a bounded `file_budget.*` finding and a blocked result.

Capture one UTC evaluation instant and use it for policy parsing/evaluation. Compute the earliest next UTC boundary following any active waiver expiry for `valid_until_ms`. Before returning a passing result, resample the clock; if the boundary crossed, evaluate again at the new date so an expired waiver cannot produce fresh success.

Normalize all pure diagnostics to `jig_contract::Finding` with source `jig.file_budget`, stable code, whole-file location, and deterministic severity/path/code ordering. Bound only previews/human/evidence output, never totals. Compute the complete findings digest over all normalized findings and a canonical evaluation digest covering policy identities, every comparison identity, view, effective ceilings, scope issues, per-candidate authority/content digests/measurements/disposition, evaluation instant/validity, counts, and complete findings digest. Emit a bounded file-budget evidence object through the existing native result path.

Second, add `jig file-budget check`, `audit`, `explain PATH`, and `validate`. Enforce that check/explain choose at most one of no selector, `--base`, `--exact-tree` with required provenance, `--staged`, or `--strict-inventory`. No selector uses default-branch merge base when history exists and exact empty-tree `UnbornWorktree` authority when it does not. Direct options use `NativeFileBudgetConfigV1` defaults plus hard-capped `--max-candidates` and `--max-total-bytes`; they do not read action configuration. `audit` inventories governed tracked plus nonignored untracked files, supports `--tracked-only`, and is informational unless `--strict`. `validate` parses worktree policy or staged index policy, compiles/matches current paths, and validates exact waiver targets. `explain` filters the shared detailed evaluation to one exact path while preserving scope/policy authority. Human output starts with scope/policy identity and prints errors before warnings before notices; JSON uses an explicit versioned report structure and contains complete counts and comparison OIDs. Map passing/informational to 0, violations to 1, invalid invocation/policy to 2, and blocked authority to 3.

Third, expose exact push-before selection through provider-neutral typed input rather than ambient CI variable inspection. Reuse `ComparisonRequestV1::ExactTree { provenance: PushBefore }`, including all-zero before handling and Task C's one-fetch/missing-comparison behavior. Ensure the built-in file-budget action is conservatively applicable to every repository change through `"**"`, without bypassing target-local contract-v7 selection. Keep provider annotation rendering outside the evaluator.

Finally, add tests at the narrowest owning layer and then integration coverage. Runtime tests must include unrelated changes with waiver facts, arbitrary NUL bytes, worktree/index/inventory views, rename/copy ancestry, missing/unsupported waiver targets, excluded files not charged, candidate and byte exhaustion, bounded finding preview with complete digest/counts, exact push-before, no-history default, policy digest replay, validity crossing, cancellation/timeout, and changed-during-read. CLI tests must cover every selector conflict/default, independent direct overrides and hard caps, output/exit contracts, audit modes, explain decisions, validate failures, and proof that no durable state is appended. CI/request tests must prove exact-before never becomes merge-base and missing nonzero history blocks unless authenticated fallback was prepared.

## Concrete steps

1. Inspect and, where necessary, minimally extend the Task B scope/content APIs so runtime can stream worktree, index, and comparison blobs through one cancellation/deadline-aware bound without exposing unsafe raw process behavior.
2. Add the shared engine module and replace `file_budget.engine_pending` in `runtime/tool_execution.rs` with its result.
3. Add the direct CLI option types, dispatch, report DTO, human renderer, and process exit mapping.
4. Add provider-neutral push-before request plumbing and applicability coverage at the existing repository/check/CI boundary.
5. Run `cargo fmt --all`; then `cargo test -p jig-file-budget`, focused `cargo test -p jig-sh` filters, `cargo clippy -p jig-file-budget --all-targets -- -D warnings`, and `cargo clippy -p jig-sh --all-targets -- -D warnings` while iterating.
6. Rebuild with `cargo build -p jig-sh --bin jig`, export `JIG_DEV_BIN=target/debug/jig`, run `scripts/jig work check --plan-id plan_01M19RBAD2VJ0PG6Z63TXFCFNB`, inspect `work gates`, `work evidence`, and `work receipts`, then run the required direct `scripts/jig check test`.
7. Audit every Bead criterion against direct source/test/command evidence, update this living plan, inspect the final diff for stale placeholder/Bash/comment-bypass semantics, close structured work, close the Bead, and flush Beads JSONL only when everything is proven.

## Validation and acceptance

Completion requires all of the following evidence, not merely compilation:

- A repository `jig.file_budget` action with ready Task C authority returns real `Success` or policy `Failure`, normalized findings, complete counts/digests, bounded previews/evidence, evaluation time, and waiver validity; it no longer returns `file_budget.engine_pending`.
- Zero-selector/history, zero-selector/unborn, base, exact-tree/provenance, staged, and strict-inventory comparisons are unambiguous and tested. Exact push-before uses its requested identity and never computes a merge base.
- Every current waiver path is independently observed in each selected current view, including when ordinary changed scope is unrelated. Missing, unmatched, and unsupported targets remain visible failures.
- Arbitrary regular bytes are streamed exactly with no binary heuristic and no symlink following. Candidate, per-file, aggregate, Git-output, result-preview, deadline, and cancellation bounds fail closed with truthful totals/digests.
- Direct configuration is independent of authored action replacement/removal; direct commands append no run/receipt; human and versioned JSON reports and exits 0/1/2/3 match the design.
- Push-before history unavailability blocks by default and uses strict inventory only when the checked-in prepared fallback says so.
- Relevant focused tests pass, every configured Jig gate is fresh and passing/not-applicable for a justified path policy, `scripts/jig check test` passes using the freshly built binary, and the completion audit finds no unverified Task D deliverable.

## Idempotence and recovery

All implementation edits and test commands are repeatable. `.agent/state/*.jsonl` is append-only; do not edit existing records. If a focused check fails, retain its receipt, fix forward, rebuild the development binary, and rerun. If execution discovers that a Task B primitive cannot provide required bounded content authority, extend that primitive in place with tests rather than bypassing it with unbounded `std::process::Command`. Do not delete or reset the inherited Task C worktree changes.

## Interfaces and dependencies

The engine consumes `PreparedNativeInputV1`, `ResolvedComparisonV1`, `NativeFileBudgetConfigV1`, `capture_scope_v1_with_cancellation`, `observe_exact_paths_v1_with_cancellation`, `parse_policy_v1`, `parse_comparison_policy_v1`, `measure_stream_v1`, and `evaluate_v1`. It returns the existing `NativeActionResult`; no new durable result channel or journal is allowed. Contract DTO changes are permitted only if direct JSON or bounded file-budget evidence cannot be represented without them, and any such change must preserve historical readers and run-plan authentication.

Task E remains responsible for seed policy generation, authored action lifecycle, update/recopy preservation, and Bash retirement transactions. This task may provide the runtime/CLI/CI surfaces that Task E will render, but must not silently seed or rewrite repository policy.
