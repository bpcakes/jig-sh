# Extract canonical Git comparison, scope, and target matching primitives

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while implementation proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

Jig currently has strong bounded Git observation inside `crates/jig/src/git_receipts.rs`, but generic affected selection separately interprets a symbolic base and flattens Git output immediately. After this work, internal Jig consumers can resolve a comparison once, reconstruct a typed and rename-preserving scope from that exact authority, flatten it for legacy affected selection, match paths against one target's declared inputs, and observe exact policy-named paths in a selected current view even when those paths were not changed.

This task does not activate the future native file-budget action or persist the potentially large scope. Existing contract epochs and affected-selection results remain compatible. The behavior is observable through focused Git fixtures: a rename carries its baseline path, a copy destination is added, worktree scope contains staged, unstaged, and non-ignored untracked files, index scope ignores unstaged bytes, incomplete Git states fail closed, and exact-path observation returns regular, missing, or unsupported independently of the changed set.

The structured-work Git baseline is commit `ecd6f939d2000ee9df5ed961f4d4a31fcd6b4789`. The worktree also contains the intentionally uncommitted, completed prerequisite `.8.1` crate `crates/jig-file-budget`; preserve that layer and all append-only Jig and Beads records.

## Progress

- [x] (2026-08-30 12:07Z) Claimed Bead `jig-sh-generic-monorepo-zac.8.2` and synchronized Beads state.
- [x] (2026-08-30 12:11Z) Read repository guidance, the Task B specification, existing `git_receipts` process/scope/worktree internals, generic affected selection, and planner integration.
- [x] (2026-08-30 12:11Z) Opened structured work plan `plan_01M199AJW848XRSHRNXM3V142B` with a freshly built development Jig binary.
- [x] (2026-08-30 12:35Z) Added the internal versioned comparison resolver and typed scope model while reusing the existing scrubbed, bounded, cancellable Git runner.
- [x] (2026-08-30 12:35Z) Built worktree, index, and strict-inventory scope capture with rename ancestry, copy-as-new semantics, untracked inclusion, and typed completeness issues.
- [x] (2026-08-30 12:35Z) Added bounded exact-current-view observation and a target-local non-empty-input matcher, then adapted generic affected selection to the canonical comparison service without changing legacy epoch selection semantics.
- [x] (2026-08-30 12:35Z) Added focused normal and fail-closed fixtures for the acceptance boundaries, ran formatting and strict all-target/all-feature Clippy, and kept every new or changed Rust file below the 800-line hard limit.
- [x] (2026-08-30 12:49Z) Built the development Jig binary, ran structured gates and `scripts/jig check test`, inspected fresh evidence, completed this retrospective, closed the plan and Bead, and synchronized Beads state.

## Surprises & Discoveries

- Observation: `git_receipts` already centralizes cancellation, Git-environment scrubbing, output ceilings, entry ceilings, source fingerprints, untracked-content handling, and baseline-to-worktree gate scope.
  Evidence: `crates/jig/src/git_receipts/process.rs` routes bounded proof commands through `jig-owned-process`, and `crates/jig/src/git_receipts/scope.rs` already discovers baseline, index, worktree, and untracked paths.

- Observation: generic affected selection currently calls `repo_changed_paths_since`, which resolves `<base>...HEAD` with `--no-renames` and unions worktree status paths. It therefore loses ancestry before selection even though gate scope separately uses `--find-renames`.
  Evidence: `crates/jig/src/repository/planner.rs::plan_run_with_policy` calls `git_receipts::repo_changed_paths_since`; `crates/jig/src/git_receipts.rs::repo_changed_paths_since` returns only strings.

- Observation: the completed `.8.1` work is not committed, so this plan's HEAD baseline predates its prerequisite crate.
  Evidence: `git status --short` lists `crates/jig-file-budget/` as untracked and workspace manifests as modified while `git rev-parse HEAD` remains `ecd6f939...`.

- Observation: successful `git diff --raw` commands can report rename-limit degradation only on stderr, so returning stdout alone is insufficient even when exit status is zero.
  Evidence: the focused `successful_git_rename_limit_degradation_marks_the_scope_incomplete` fixture pins `diff.renameLimit=1`; Git succeeds, the bounded runner preserves stderr, and the snapshot records `ScopeIssueKindV1::RenameLimit` with `complete = false`.

- Observation: ordinary `M` and `T` raw-diff records carry one path rather than an explicit old/new pair, but the current path is still the comparison-side ancestry path.
  Evidence: `append_raw_entry` now assigns the current path as `baseline_path` for modified and type-changed entries; focused assertions pin this for staged and unstaged files.

## Decision Log

- Decision: Keep the new comparison and scope service inside `git_receipts` and expose only crate-internal APIs.
  Rationale: Task B explicitly extracts from this owner, must reuse its Git runner and limits, and must not publish an external scope protocol. Sibling files such as `git_receipts/comparison.rs`, `git_receipts/change_scope.rs`, and `git_receipts/exact_path.rs` keep the boundary auditable without duplicating process policy.
  Date/Author: 2026-08-30 / Codex

- Decision: Represent incomplete observation as a typed `ScopeSnapshotV1` with `complete = false` and sorted `ScopeIssueV1` values; adapters that require flattened paths turn incompleteness into an error.
  Rationale: future native consumers need structured issues, while current affected selection must fail closed and cannot silently select from an incomplete path set.
  Date/Author: 2026-08-30 / Codex

- Decision: Pin scope diffs to rename detection only, never copy detection, and treat any copy record defensively as an addition with no baseline.
  Rationale: renames preserve debt ancestry, but a copy destination must never inherit debt. Explicit command arguments must override ambient `diff.renames` configuration.
  Date/Author: 2026-08-30 / Codex

- Decision: Build a target-local matcher primitive now but derive the existing component-level matching from it until the later contract epoch activates target-local selection.
  Rationale: this proves and reuses the primitive while preserving the issue's requirement that legacy behavior not change in Task B.
  Date/Author: 2026-08-30 / Codex

- Decision: Keep the future-facing comparison, index/inventory scope, and exact-path APIs crate-internal and explicitly annotate their temporary Task B dead-code staging.
  Rationale: Task C and Task D are the first production consumers for several variants, while Task B must land and test the complete primitive without activating a new serialized contract epoch.
  Date/Author: 2026-08-30 / Codex

## Outcomes & Retrospective

Task B is complete. `git_receipts::resolve_comparison_v1` now owns merge-base, exact commit/tree, index-against-HEAD or hash-format-correct empty tree, and explicit strict-inventory resolution while preserving requested, peeled-commit, tree, HEAD, and merge-base identities. Every new probe calls the existing supervised Git runner, inherits environment scrubbing and cancellation, disables replacement objects, and uses bounded stdout and stderr. Exact push-before resolution has a direct test hook proving it does not enter merge-base resolution.

`capture_scope_v1` reconstructs worktree, index, or tracked inventory views. Worktree fixtures prove staged, unstaged, non-ignored untracked, ignored exclusion, deletion omission, rename ancestry, and copy-as-new behavior under hostile ambient rename, external-diff, and textconv configuration. Index fixtures prove index-blob authority and isolation from unstaged bytes. Typed incomplete snapshots cover successful rename-limit degradation, unmerged stages, intent-to-add, sparse entries, gitlinks, symlinks and type changes, special files, embedded directories, missing tracked worktree entries, and non-UTF-8 paths. The legacy affected adapter consumes `flattened_changed_paths`, including both rename paths, and refuses incomplete scope.

`observe_exact_paths_v1` returns one deterministic regular, missing, or typed unsupported fact for a bounded exact input set. Tests cover unchanged paths absent from the changed set, literal metacharacters, worktree/index/inventory distinctions, untracked and missing facts, and validation of count, bytes, duplicates, absolute paths, traversal, NUL, and protected authority before repository observation.

`TargetInputMatcherV1` retains each non-empty input glob under its exact `TargetId`. Its focused test proves disjoint ownership, while all 18 existing affected-selection tests prove Task B still aggregates matches to legacy component behavior and dependency propagation. No run-plan or receipt schema changed, no external protocol was added, and the in-memory scope remains outside durable state; Task C deliberately owns serialized prepared comparison authority and the epoch-gated target-local semantic switch.

Validation completed with 11 focused comparison/scope tests, 18 affected-selection tests, strict all-target/all-feature Clippy, a structured gate run of 2,412 core tests, and the required full `jig.test` run of 3,170 tests. After closing the Bead, the structured-work refresh executed all eight required gates from the final `.beads` state with no reuse and no failures.

## Context and Orientation

`crates/jig/src/git_receipts.rs` is the current repository-observation owner. Its child `process.rs` configures a known-repository read-only Git environment and uses `jig-owned-process` for bounded cancellable output. Its child `scope.rs` builds plan-change snapshots and gate fingerprints from a resolved baseline. Its child `worktree.rs` parses porcelain status and fingerprints worktree and untracked state. New code must call these facilities instead of spawning Git independently.

A comparison is the immutable answer to "which prior Git authority is current content compared with?" A merge-base comparison stores the requested ref, that ref's resolved commit, current HEAD, and their merge base. An exact-tree comparison stores the caller's requested object identity, an optional peeled commit, the exact tree, and provenance such as work-plan, push-before, unborn-worktree, or explicit internal use. Index-against-HEAD stores HEAD or the repository-hash-format empty tree. Strict inventory has no baseline inheritance.

A scope snapshot is an execution-local list of typed entries. Each entry has a change kind, current path, optional baseline path, current source, and optional baseline blob object ID. Worktree paths identify content on disk; index sources identify a blob object. Rename entries keep the old baseline path. Copy entries and untracked entries have no baseline. Deletions are omitted because they have no current content. The snapshot also carries sorted typed issues and a `complete` flag. Large entry vectors are never serialized into run plans or receipts by this task.

`crates/jig/src/repository/affected.rs` currently aggregates every action input by component. Task B must add a matcher that retains `TargetId` ownership for non-empty action input lists. Existing component fallback and dependency propagation stay unchanged until Task C; the current implementation can aggregate the new primitive back to components.

Exact-path observation accepts an explicitly bounded set of repository-relative UTF-8 paths and a current view. Worktree observation reports filesystem authority without following a symlink leaf or a symlink ancestor. Index observation reads stage-zero index metadata. Inventory observation is tracked-worktree authority and does not promote an untracked path. Every path returns regular, missing, or unsupported, even when no scope entry mentions it.

## Plan of Work

First, extend the reusable Git runner just enough to return bounded stderr alongside stdout for commands whose success can still carry rename-limit degradation warnings. Every comparison and scope invocation starts with `--no-replace-objects`, disables external diff and text conversion, uses NUL-delimited machine output, pins file-mode and submodule behavior, pins rename enablement, similarity, and rename limit, and remains cancellable through `GitReceiptCollection`.

Second, add `git_receipts/comparison.rs`. Define crate-internal `ComparisonRequestV1`, `ResolvedComparisonV1`, `ExactTreeProvenanceV1`, and `StrictInventoryReasonV1`. Implement blocking and cancellable resolution. Merge-base resolution validates the symbolic request, resolves both tips once, and records their exact merge base. Exact-tree resolution preserves the requested identity separately, handles a hash-format-correct all-zero or unborn empty-tree authority only under explicit provenance, peels commit or tree objects without computing a merge base, and rejects other object kinds. Index resolution chooses HEAD or a verified empty tree only for an actually unborn repository.

Third, add `git_receipts/change_scope.rs`. Define `CurrentViewV1`, `ScopeSnapshotV1`, `ScopeEntryV1`, `FileChangeKindV1`, `CurrentSourceV1`, `ScopeIssueV1`, and issue kinds. Parse bounded raw `-z` diffs so path bytes are never line-delimited. Capture worktree scope from the resolved baseline plus non-ignored untracked status, index scope from HEAD/empty-tree to the index, and inventory scope from tracked stage-zero entries. Detect malformed output, non-UTF-8 paths, unmerged stages, intent-to-add entries, sparse authority, symlinks, gitlinks, special or embedded entries, and rename-limit warnings as typed incomplete issues. Deduplicate and sort entries deterministically. Expose a `flattened_changed_paths` method that includes both sides of a rename and refuses incomplete snapshots.

Fourth, add `git_receipts/exact_path.rs`. Validate count, byte, uniqueness, and repository-relative bounds before invoking Git. Worktree and inventory checks validate every existing ancestor and inspect the leaf with `symlink_metadata`; index checks use literal pathspecs and stage metadata. Return one sorted fact per requested path, including unchanged paths absent from the change set. Unsupported facts retain a typed reason and never become missing merely because they are outside the changed candidates.

Fifth, replace `repo_changed_paths_since` with a compatibility adapter that resolves `MergeBaseRef`, captures a worktree scope, and flattens the complete snapshot. Keep its public crate signature so repository planning and plan replay continue to behave identically. Add `repository/affected/target_matcher.rs`; compile each target's non-empty inputs once, independently test exact target results, and use it to derive the current component matcher so contract-version behavior remains unchanged.

Finally, add focused fixtures under the existing `git_receipts` and `repository::affected` test modules. Exercise identity preservation; no merge-base call for exact tree; staged, unstaged, and untracked worktree content; ignored exclusion; index isolation; empty-tree unborn repositories; unchanged exact paths; rename and copy semantics; deterministic ordering; cancellation; ambient rename, replacement, external-diff, and textconv hardening; and fail-closed unmerged, intent-to-add, sparse, symlink, gitlink, special, embedded, non-UTF-8, malformed, output-limit, and rename-limit conditions. Preserve generic fixture names and avoid downstream identifiers.

## Concrete Steps

Work from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`.

Inspect current state before each milestone:

    git status --short
    br show jig-sh-generic-monorepo-zac.8.2 --json
    JIG_DEV_BIN=target/debug/jig scripts/jig work status

After comparison and scope types compile, run focused library tests:

    cargo fmt --all -- --check
    cargo test -p jig-sh git_receipts --lib
    cargo test -p jig-sh repository::affected --lib
    cargo clippy -p jig-sh --all-targets --all-features -- -D warnings

Before repository gates, build the current runtime and force the launcher to use it:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig work check --plan-id plan_01M199AJW848XRSHRNXM3V142B
    scripts/jig work gates --plan-id plan_01M199AJW848XRSHRNXM3V142B
    scripts/jig work evidence --plan-id plan_01M199AJW848XRSHRNXM3V142B
    scripts/jig work receipts --plan-id plan_01M199AJW848XRSHRNXM3V142B
    scripts/jig check test

The focused tests must pass with no ignored failure. Structured work must report all required gates passed and fresh. The final `jig.test` receipt must cover the complete workspace suite.

## Validation and Acceptance

Comparison tests must assert every stored identity, not only the final tree. A merge-base fixture proves requested ref, resolved ref commit, HEAD, and merge-base fields. Exact commit and exact tree fixtures prove requested object, optional peeled commit, and tree. An exact-tree fixture uses a Git wrapper or another direct observation to prove no merge-base command ran. Unborn index and worktree fixtures compare against the repository's own `git mktree` empty-tree output.

Scope tests must inspect typed entries. A real rename has `Renamed`, current destination, old baseline path, and old blob. A copied file has `Added` and no baseline. Worktree fixtures independently add staged, unstaged, and non-ignored untracked paths, while ignored paths do not appear. Index fixtures prove unstaged-only paths and bytes do not affect entries or exact observation. Deletions have no current entry.

Fail-closed tests must assert both `complete == false` and the precise typed issue kind for ambiguous or unsupported authority. The compatibility adapter used by affected selection must return an error instead of flattened paths for such a snapshot. Ambient configuration tests install hostile replacement, diff, textconv, and rename settings and prove neither helpers execute nor scope semantics change. Rename-limit degradation must be detected even when Git exits successfully.

Exact-path tests must request a path unchanged from the comparison while another path changes and still receive `Regular`. Worktree, index, and inventory variants must also prove `Missing` and typed `Unsupported` for symlink, gitlink, special, unmerged, or sparse authority as applicable. Over-count, over-length, duplicate, absolute, traversal, and NUL inputs must fail before repository observation.

Target-matcher tests must use two actions in one component with disjoint non-empty inputs and prove each path maps only to its owning target. Existing affected-selection tests must remain unchanged and passing, demonstrating that Task B has not activated target-local epoch semantics.

Completion requires the 8.2 Bead acceptance criteria, not merely compilation: one canonical comparison path feeds generic affected selection; all listed current views and failure classes have direct evidence; every Git call reuses bounded cancellable execution; no external protocol or unbounded durable list is added; and legacy plan serialization remains byte-compatible.

## Idempotence and Recovery

All implementation commands are read-only except source edits, structured-work append operations, and Beads mutations. Re-running focused tests, comparison resolution, scope capture, and exact observation is safe. Preserve all existing dirty work and append-only `.agent/state/*.jsonl` records. Do not reset, checkout, or rewrite those records.

If a test fixture exits midway, its `tempfile` repository is removed by Rust test cleanup. If a structured gate fails, fix the source and rerun `scripts/jig work check` with the same plan ID so a new receipt records the corrected worktree. Always rebuild `target/debug/jig` before rerunning harness commands. No schema migration or external state rollback belongs to this task.

## Artifacts and Notes

Initial evidence:

    HEAD: ecd6f939d2000ee9df5ed961f4d4a31fcd6b4789
    Bead: jig-sh-generic-monorepo-zac.8.2 (in_progress)
    Structured plan: plan_01M199AJW848XRSHRNXM3V142B
    Existing affected adapter: git_receipts::repo_changed_paths_since
    Existing consumer: repository::planner::plan_run_with_policy

Focused implementation evidence:

    cargo test -p jig-sh git_receipts::tests::comparison_scope --lib
    result: 11 passed, 0 failed

    cargo test -p jig-sh repository::affected --lib
    result: 18 passed, 0 failed

    cargo clippy -p jig-sh --all-targets --all-features -- -D warnings
    result: passed

    largest new/changed Rust test file: 787 lines

    final structured work batch: receipt_01M19BD5XZW1PSD0HBF8NNE495
    result: 8 required gates executed, 8 passed, 0 reused, 0 unresolved

    scripts/jig check test: receipt_01M19B6B71CJYDFW6A5HJHEGHZ
    result: 3,170 passed, 0 failed, 2 skipped

    Bead jig-sh-generic-monorepo-zac.8.2: closed and synchronized

Update this section with concise focused-test totals, gate receipts, and the final full-test receipt.

## Interfaces and Dependencies

The exact names may be refined to satisfy canonical Rust organization, but the finished internal boundary must provide equivalents of:

    enum ComparisonRequestV1 {
        MergeBaseRef { requested_ref: String },
        ExactTree { requested_oid: String, provenance: ExactTreeProvenanceV1 },
        IndexAgainstHead,
        StrictInventory { reason: StrictInventoryReasonV1 },
    }

    enum ResolvedComparisonV1 {
        MergeBase { requested_ref: String, resolved_ref_oid: String, head_oid: String, merge_base_oid: String },
        ExactTree { requested_oid: String, peeled_commit_oid: Option<String>, tree_oid: String, provenance: ExactTreeProvenanceV1 },
        IndexAgainstHead { head_or_empty_oid: String },
        StrictInventory { reason: StrictInventoryReasonV1 },
    }

    fn resolve_comparison_v1(root: &Path, request: ComparisonRequestV1) -> Result<ResolvedComparisonV1>;

    fn capture_scope_v1(root: &Path, comparison: &ResolvedComparisonV1, view: CurrentViewV1) -> Result<ScopeSnapshotV1>;

    fn observe_exact_paths_v1(root: &Path, view: CurrentViewV1, paths: &[String]) -> Result<Vec<ExactCurrentPathFactV1>>;

    struct TargetInputMatcherV1 { /* compiled target-owned non-empty inputs */ }

The Git functions depend only on existing `anyhow`, `globset`, `tempfile`, `sha2`, standard-library filesystem/process types, and the current `jig-owned-process` path already used by `git_receipts`. Do not add a Git library, second process runner, second environment scrubber, new contract field, native action runner, durable comparison field, or new evidence store.

Revision note (2026-08-30 12:11Z): Replaced the initial one-line structured-work body with a self-contained ExecPlan after inspecting Task B, `git_receipts`, affected selection, and repository guidance. This records the extraction boundary, compatibility strategy, fail-closed proof matrix, and recovery workflow before implementation.

Revision note (2026-08-30 12:35Z): Recorded completion of the comparison, scope, exact-path, and target-matcher implementation milestones plus focused test and strict-Clippy evidence before starting repository-wide gates.

Revision note (2026-08-30 12:48Z): Completed the acceptance retrospective with final focused, full-test, structured-gate, and Bead evidence. Task C remains the explicit owner of durable comparison serialization and the contract-epoch semantic cutover.

Revision note (2026-08-30 12:49Z): Marked the final workflow step complete after `scripts/jig work finish` closed the structured plan successfully.
