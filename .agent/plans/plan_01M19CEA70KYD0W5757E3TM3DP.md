# Correct comparison scope consumer boundaries

This ExecPlan is a living document maintained under `.agent/PLANS.md`. The work follows Bead `jig-sh-pvp` and repairs review findings in the completed universal file-budget Task B layer without activating later contract epochs.

## Purpose / Big Picture

Task B introduced one canonical Git comparison resolver and a strict typed scope for future file-budget measurement, then reused that scope as the compatibility adapter for existing `jig run --affected` planning. Those consumers need different products: budget evaluation needs only measurable current regular files and must fail closed on ambiguous content authority, while affected planning needs every changed path (including deletions and unsupported current file types) and historically failed only when path enumeration itself was unavailable. Conflating them dropped deletions, made unrelated worktree types fatal, and added full-index probes to ordinary affected planning.

After this work, shared bounded Git commands feed two explicitly named outputs. `capture_scope_v1` remains the strict measurement-oriented snapshot. A separate affected-path capture returns the deterministic UTF-8 union of raw diff and worktree-status paths without inspecting file content or index-wide measurement metadata. Comparison ancestry accepts only regular-file blobs; reverse type changes cannot inherit symlink or gitlink objects. Unborn HEAD detection distinguishes an absent symbolic target ref from a present but unresolved ref. Exact paths reject Windows drive prefixes in every component on every host.

## Progress

- [x] (2026-08-30 13:06Z) Created and claimed Bead `jig-sh-pvp`, opened structured work, read repository/crate guidance, and established green focused baselines.
- [x] (2026-08-30 13:06Z) Researched review open questions against `docs/plans/universal-file-budget.md`, Tasks A-C, policy/evaluator tests, and CI configuration.
- [x] (2026-08-30 13:13Z) Added affected-path capture using shared bounded raw-diff and status primitives; deletion-only and strict-scope-only issue fixtures preserve legacy affected semantics without index-wide probes.
- [x] (2026-08-30 13:13Z) Coupled comparison path/blob authority in `BaselineFileV1` and made reverse unsupported-to-regular type changes new debt authority.
- [x] (2026-08-30 13:13Z) Corrected symbolic unborn-HEAD detection, Git diagnostic typing, untracked disappearance handling, portable path validation, and narrowed staged dead-code acknowledgements.
- [x] (2026-08-30 13:33Z) Passed focused format/tests/strict Clippy, all eight structured gates (2,415 core, 107 frontend, 442 vault, and 209 process tests), and the full 3,173-test `jig.test` suite.
- [x] (2026-08-30 14:15Z) Completed comprehensive review round one with Claude and Codex over verified fingerprint `14a9f9186fdf55adc77c0d1dec66606f46ed42cb04078b54fce69dd601679609`.
- [x] (2026-08-30 14:31Z) Repaired round-one findings: ignored replacements of staged deletions block scope capture; multiple best merge bases block as ambiguous; empty baselines are hashed without object writes; all unborn-HEAD consumers share exact ref-existence detection; auxiliary Git diagnostics affect completeness; invalid supplied paths use scope diagnostics; and patterns reject empty components.
- [x] (2026-08-30 14:38Z) Passed 51 file-budget tests, 70 Git-receipt tests, 18 affected-planning tests, format, and strict Clippy after round-one repairs.
- [x] (2026-08-30 14:55Z) Recovered the structured core gate from build-cache exhaustion by removing 19.2 GiB of package-scoped Cargo artifacts and disabling incremental compilation; the fresh run passed 2,420 tests with receipt `receipt_01M19FBSP478Y72SMJKJYAJECA`.
- [x] (2026-08-30 15:18Z) Completed comprehensive review round two with Claude and Codex over verified fingerprint `9effc15861432858dc748017dc2cc864a2293d331410698ee62ae6dd919b7c5d`.
- [x] (2026-08-30 15:34Z) Repaired round-two findings: NUL-framed Git parsers reject truncation, exact directory queries cannot expand subtrees, oversized waiver facts bypass indexing, candidate paths use a separate 4 KiB bound, and scope failures retain bounded Git diagnostic excerpts. A proposed Windows separator hardening was researched and removed because the supported-host contract is Linux/macOS and backslash is a valid POSIX filename byte.
- [x] (2026-08-30 15:40Z) Passed 52 file-budget tests, 71 Git-receipt tests, format, strict Clippy, and diff hygiene after round-two repairs.
- [x] (2026-08-30 15:58Z) Completed the third and final comprehensive review with Claude and Codex over verified fingerprint `0555f86e7322904d16f7cee97a21c692178ea19c9a3bcccb5cef1efd67161a84`.
- [x] (2026-08-30 16:09Z) Repaired the final actionable findings: exact queries partition ancestor/descendant requests, malformed protected-root facts fail structural validation, ambiguous merge-base errors list bounded candidate IDs and remediation, non-UTF-8 rename ancestry cannot form an entry, and affected-path union cardinality is bounded.
- [x] (2026-08-30 16:15Z) Passed 52 file-budget tests, 71 Git-receipt tests, 18 affected-planning tests, format, strict Clippy, and diff hygiene after final review fixes.
- [x] (2026-08-30 16:39Z) Passed the final eight-gate structured sweep with fresh evidence (2,422 core, 107 frontend, 442 vault, and 209 process tests) and the full 3,180-test `jig.test` suite; Bead `jig-sh-pvp` is closed.
- [x] Run the comprehensive-review loop and resolve actionable findings through the requested three-round maximum.
- [x] Close the structured plan and Bead only after final review and fresh evidence are green.

## Surprises & Discoveries

- Observation: `ScopeSnapshotV1::flattened_changed_paths` treats every measurement issue as a path-discovery failure, even though the pre-Task-B affected adapter only required bounded UTF-8 path enumeration.
  Evidence: `crates/jig/src/git_receipts/change_scope.rs` rejects incomplete snapshots wholesale, while the removed adapter in the staged diff unioned `git diff --name-only` with porcelain status.

- Observation: scope entries store `baseline_path` and `baseline_blob_oid` as independent options, allowing invalid combinations and permitting a symlink or gitlink object to masquerade as regular-file ancestry.
  Evidence: `append_raw_entry` validates only `new_mode` and independently derives the two baseline fields.

- Observation: parsing-time and evaluation-time waiver expiry checks are both intentional rather than dead duplication.
  Evidence: parsing rejects already-expired policy, while evaluation receives an explicit later date and protects prepared policy that expires before execution; Task C owns receipt validity propagation.

- Observation: unchanged content moved under a stricter rule is intentionally legacy debt because the current policy owns both current and comparison debt coordinates.
  Evidence: the product plan defines debt relative to the current limit and says historical policy never overrides ordinary current thresholds.

- Observation: `jig-file-budget` is intentionally an internal workspace crate staged before runtime consumption.
  Evidence: Tasks C and D own durable/native consumption, while workspace CI and `scripts/jig check test` already compile and test the crate.

- Observation: `git merge-base` without `--all` deliberately leaves the selected base unspecified in criss-cross history.
  Evidence: the installed Git 2.43 manual says multiple best common ancestors are possible and the result without `--all` is unspecified.

- Observation: empty-tree identity does not require object-database mutation.
  Evidence: `git hash-object` reports an object ID and writes only when `-w` is present; Jig's bounded Git runner supplies null stdin, which hashes the empty tree without installing it.

- Observation: the first post-review structured core gate failed before test execution because Cargo exhausted disk while writing incremental state; the first retry then encountered the damaged incremental dependency graph.
  Evidence: package-scoped `cargo clean` removed 19.2 GiB, and the fresh `CARGO_INCREMENTAL=0` core gate passed all 2,420 tests.

- Observation: Git literal pathspecs still interpret a directory-valued path as a subtree prefix.
  Evidence: the Git glossary defines the pathspec up to the last slash as a directory prefix; pairing each exact include with a literal `path/` exclusion returns the tracked file itself without descendants.

- Observation: version 1 documents a 1,024-byte pattern bound but no equivalent candidate-path bound.
  Evidence: the product plan's cardinality section names the pattern limit only; candidate paths now use an independent 4,096-byte practical filesystem bound.

## Decision Log

- Decision: Apply Fowler's **Split Phase** and **Change Function Declaration** by exposing separate strict scope and affected-path capture operations over shared private Git helpers.
  Rationale: a policy flag on `flattened_changed_paths` would retain the misleading abstraction and invite consumer-specific issue filtering to spread through the snapshot type.
  Date/Author: 2026-08-30 / Codex

- Decision: Apply **Replace Primitive with Object** by coupling comparison path and blob OID in one optional baseline-authority value.
  Rationale: the current pair of independent `Option` fields represents impossible ancestry states and directly enabled unsupported-object inheritance.
  Date/Author: 2026-08-30 / Codex

- Decision: Treat a current regular file whose old mode is unsupported as new debt authority rather than an incomplete current scope.
  Rationale: no regular comparison content exists to inherit, and treating the current file as new is deterministic and fail-closed for debt without rejecting valid current bytes.
  Date/Author: 2026-08-30 / Codex

- Decision: Preserve unexpected successful Git stderr as a distinct strict-scope issue, but do not make it fatal to affected path enumeration.
  Rationale: budget measurement remains conservative while legacy affected selection consumes bounded successful machine output as before.
  Date/Author: 2026-08-30 / Codex

- Decision: Reject multiple best merge bases instead of synthesizing a virtual merge result.
  Rationale: deterministic persisted comparison authority is a product invariant; synthesis would add content-merge policy and new authority not specified by version 1.
  Date/Author: 2026-08-30 / Codex

- Decision: Reuse non-writing `git hash-object -t tree --stdin` for every empty baseline and share the exact symbolic-ref absence check across comparison and source-snapshot consumers.
  Rationale: this preserves object-format awareness, cancellation, bounded execution, and the observational Git boundary without adding hash dependencies or duplicated unborn logic.
  Date/Author: 2026-08-30 / Codex

- Decision: Do not memoize ancestor filesystem metadata during strict worktree observation.
  Rationale: cached directory verdicts can become stale while the worktree mutates and would introduce a TOCTOU gap in exchange for a low-severity performance optimization; measurement correctness requires rechecking the path chain.
  Date/Author: 2026-08-30 / Codex

- Decision: Require terminal NUL framing at the shared parser boundary for every machine-readable `-z` stream used by the new scope code.
  Rationale: successful process exit is not proof of a complete final record; one shared invariant prevents truncated filenames from becoming authority.
  Date/Author: 2026-08-30 / Codex

- Decision: Preserve legal backslashes on the supported Linux/macOS hosts and add no Windows-only path branch.
  Rationale: the repository's supported-host contract excludes Windows, while backslash is a legal POSIX filename byte; an unsupported-host branch both violated the host-policy gate and created an unverified portability claim.
  Date/Author: 2026-08-30 / Codex

- Decision: Keep matcher complexity and cross-crate unsupported-state mapping for their documented/later owning tasks.
  Rationale: the plan explicitly budgets evaluation as candidates times matcher cost, and Task C/D owns the adapter from incomplete scope to evaluator diagnostics; speculative V1 enum expansion would blur that boundary.
  Date/Author: 2026-08-30 / Codex

## Outcomes & Retrospective

The defect was structural rather than a collection of unrelated mistakes: one measurement-oriented scope value had become a compatibility API for path selection, while comparison authority and Git framing relied on loosely coupled primitives. Splitting consumer products, coupling baseline ancestry, and centralizing fail-closed framing removed the invalid states that produced the original regressions.

Three independent Claude/Codex review rounds found and drove repairs for deletion replacement ambiguity, nondeterministic merge bases, object-database mutation, incomplete NUL framing, pre-bound resource work, exact-directory pathspec expansion, and ancestor/descendant exclusion interference. The Windows native-separator proposal was rejected after checking the repository's Linux/macOS-only host contract. Lower-severity proposals that conflicted with mutable-worktree safety or later Task C/D ownership were recorded and deliberately not broadened into this slice.

Final structured evidence is green under batch receipt `receipt_01M19J3592R6NE26RHB2GQWBV8`: all eight required gates passed and remained fresh, followed by a successful 3,180-test repository-wide run under receipt `receipt_01M19J85KK3GKVQDDJ5QC4Q9AR`. Bead `jig-sh-pvp` is closed.

## Context and Orientation

`crates/jig/src/git_receipts/comparison.rs` resolves comparison authority. `change_scope.rs` captures raw Git changes and current content authority. `exact_path.rs` validates and inspects policy-named paths. `git_receipts.rs::repo_changed_paths_since` adapts Git observation into the affected planner. Focused fixtures live in `git_receipts/tests_parts/comparison_scope.rs`; pure policy path validation lives in `crates/jig-file-budget/src/policy/validation.rs` with tests in `tests/policy.rs`.

The worktree includes the completed but uncommitted Tasks A and B plus append-only Jig and Beads records. Preserve all of them. No external protocol, serialized plan shape, or contract epoch changes in this repair.

## Plan of Work

First extract the existing worktree-status command from untracked measurement handling. Add an affected-path capture function that runs the same bounded raw diff and porcelain status, adds every raw current/baseline path and status current/original path, validates UTF-8, sorts, and deduplicates. Migrate `repo_changed_paths_since` to it and remove the measurement snapshot's flattening compatibility method.

Second replace independent baseline path/OID options with an optional cohesive baseline authority. Construct it only when both the change status carries ancestry and the old mode is a regular file. Keep copies, additions, untracked files, and reverse unsupported-to-regular type changes without inherited authority.

Third resolve symbolic HEAD's target name, enumerate that exact ref, and call the empty-tree path only when the ref is absent. Classify non-rename Git stderr separately. Omit an untracked entry that disappears after status. Reject drive-prefix-shaped components in both exact Git paths and pure policy candidate paths.

Finally add direct regressions for deletion-only affected paths, affected planning with strict-scope-only issues, reverse type changes, a broken symbolic HEAD ref, unexpected Git diagnostics, disappeared-untracked handling, and portable drive-prefix validation. Run focused and repository-defined verification, then run independent comprehensive reviews and repeat fixes/reviews up to three rounds.

## Concrete Steps

Work from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`.

    cargo fmt --all -- --check
    cargo test -p jig-file-budget
    cargo test -p jig-sh git_receipts --lib
    cargo test -p jig-sh repository::affected --lib
    cargo clippy -p jig-file-budget --all-targets -- -D warnings
    cargo clippy -p jig-sh --all-targets --all-features -- -D warnings

Build the runtime and force dogfood commands through it:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig work check --plan-id plan_01M19CEA70KYD0W5757E3TM3DP
    scripts/jig work gates --plan-id plan_01M19CEA70KYD0W5757E3TM3DP
    scripts/jig work evidence --plan-id plan_01M19CEA70KYD0W5757E3TM3DP
    scripts/jig check test

## Validation and Acceptance

- A deletion-only change appears in `repo_changed_paths_since` although strict measurable scope contains no current entry.
- Symlink, intent-to-add, sparse, missing, or other content-authority issues remain typed and incomplete for strict scope but do not erase enumerated affected paths.
- Non-UTF-8 or malformed path enumeration still fails affected selection.
- Reverse symlink/gitlink-to-regular changes never expose the old object as comparison blob authority.
- A genuine unborn symbolic HEAD uses the empty tree; a present broken ref returns an error.
- Unexpected successful Git stderr is not mislabeled `RenameLimit`.
- A vanished untracked path does not invalidate the whole strict snapshot.
- Every Windows drive-prefix-shaped component is rejected before path joining.
- No external schema, durable state format, or contract epoch changes.

## Idempotence and Recovery

Source edits and tests are safe to repeat. Do not reset or rewrite `.agent/state/*.jsonl`; structured work and Beads mutations are append-only. If a refactoring step fails, stop at the last compiling state and fix forward. Temporary Git fixtures are owned by `tempfile` and may be rerun.

## Interfaces and Dependencies

Keep all new APIs crate-internal. Reuse `GitReceiptCollection`, bounded Git output, raw diff parsing, and porcelain status parsing. Do not add dependencies or move Git into `jig-file-budget`. Preserve Rust 1.88 and edition 2024 compatibility.
