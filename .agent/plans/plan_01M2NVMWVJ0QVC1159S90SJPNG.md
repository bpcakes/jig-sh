# Re-scope Beads integration around JSONL

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current as implementation proceeds.

This plan follows `.agent/PLANS.md`. Its exact baseline is commit `904ac64e7a933909fe7c00464dfcb94397281cbc` on branch `feature/jig-sh-x8ow-beads-evidence`.

## Purpose / Big Picture

PR #38 currently treats the `br` executable and its SQLite-family store as Jig's task authority. That architecture solves process pinning, live-store snapshots, mutation ambiguity, and crash recovery before Jig exposes any task-linking workflow. It also couples Jig to one `br` release while the intended direction is for Beads-compatible JSONL to become Jig's task-data contract and for `br` to remain an optional interoperable client.

After this change, configuring `[work.tracker] kind = "beads"` means that Jig reads the repository's current Beads JSONL export as a bounded, read-only task snapshot. Doctor validates that snapshot without invoking `br`, opening SQLite, or depending on platform-specific executable capture. The portable plan-to-task link journal, immutable task snapshots, and generic durable JSONL append primitive remain staged for the next delivery milestone. Tracker mutations, their external-operation journal, and retention/recovery machinery are removed until a concrete native JSONL write operation needs them.

The observable result in this PR is a smaller and platform-neutral authority boundary: a configured Beads workspace can be validated from `.beads/issues.jsonl` on Linux or macOS with no `br` executable installed. This PR does not yet add a public issue-linking flag or mutate task data; those behaviors belong to the following milestone, built on the retained work-link model and the new reader.

## Progress

- [x] (2026-09-16) Inspected PR #38's three commits and separated portable link/snapshot code from live-provider and mutation-recovery code.
- [x] (2026-09-16) Inspected a separate extracted Beads JSONL integration at its `origin/master` revision for reusable parsing patterns, without introducing a dependency on that project.
- [x] (2026-09-16) Verified `br 0.5.7` handoff behavior in an isolated fixture: ordinary commands auto-import a changed JSONL export, while divergent database and JSONL changes require explicit merge rather than unconditional reconciliation.
- [x] (2026-09-16) Implemented a bounded, pure Beads JSONL reader for exact issue lookup and whole-export validation.
- [x] (2026-09-16) Replaced the live-provider Doctor check with JSONL snapshot validation and removed all executable/SQLite/process dependencies.
- [x] (2026-09-16) Removed the staged external tracker-operation journal and the archive/restore/retention logic whose purpose was mutation recovery.
- [x] (2026-09-16) Retained and revalidated the portable work-link journal, structured task snapshot model, durable append primitive, configuration preservation, and deep diagnostics relevant to those retained streams.
- [x] (2026-09-16) Updated public contract, configuration, repository intent, delivery plan, and Beads issue descriptions so they describe JSONL-first ownership and defer native writes.
- [x] (2026-09-16) Built the final development binary, passed all five required Jig targets on the final source tree (4,120 tests), inspected the final diff, and passed the 31-test tracker suite on macOS arm64.
- [x] (2026-09-16) Committed and pushed the implementation as `6a8c0acb42808a03d567b54efc0f65b11e2d3af8`; verified that open PR #38 points at that commit.

## Surprises & Discoveries

- Observation: `br 0.5.7` can use JSONL as a real interoperability boundary, but its ordinary automatic import does not promise a three-way merge when both the database and JSONL changed.
  Evidence: `beads_rust/src/sync/mod.rs` skips auto-import for divergent state and directs the user to explicit sync/merge; an isolated local fixture confirmed changed JSONL is imported when the database is otherwise clean.

- Observation: this repository deliberately sets `sync.auto_flush: false` and exports through `scripts/beads-sync.py`, which also strips machine-local path data.
  Consequence: the supported transition model must preserve that privacy-aware handoff instead of enabling upstream automatic export or treating SQLite and JSONL as automatically equivalent authorities.

- Observation: the reference integration is intentionally pure and contains useful defensive decoding, bounds, duplicate-key rejection, timestamp normalization, and fixtures, but its public model is application-specific and omits fields Jig's historical task snapshot needs.
  Consequence: adapt the small parser/validation patterns into Jig rather than importing the entire graph/scope/digest abstraction.

- Observation: the current PR adds approximately 16,667 lines, including about 3,805 lines for executable discovery, immutable binary capture, SQLite-family copying, process supervision, and security tests, before exposing task linking.
  Consequence: deletion is the principal architectural fix; reproducing those guarantees around a read-only JSONL snapshot would compound the misalignment.

- Observation: the first full gate run exposed only one Clippy defect: the normalized export result enum stored its large live issue directly.
  Consequence: boxing the live variant preserved the public semantics and made the representation proportional without weakening validation. The forced Clippy rerun passed.

- Observation: the repository's real privacy-cleaned export contains 205 issues and validates under the new profile without `br` execution or SQLite access.
  Evidence: a temporary generic `[work.tracker]` configuration made the development binary's Doctor report `beads-rust-jsonl-v1`, `.beads/issues.jsonl`, `issue_count = 205`, `read_issue_snapshot`, and `write_authority = false`; the temporary configuration was then removed.

## Decision Log

- Decision: Beads-compatible JSONL is Jig's task-data contract; `br` is an optional interoperable client, not Jig's runtime provider.
  Rationale: this matches the intended long-term ownership direction and avoids pinning task semantics to a replaceable executable and private database layout.
  Date/Author: 2026-09-16 / user and Codex.

- Decision: PR #38 remains a foundation milestone and does not expose task mutations or a public issue-linking flag.
  Rationale: the existing branch is the T1/T2 foundation. Re-scoping it before building further lets the next milestone add the first user workflow against the right boundary without mixing an architectural cutover with new CLI behavior.
  Date/Author: 2026-09-16 / Codex.

- Decision: the initial reader validates the entire configured export, rejects malformed or ambiguous authority, and supports exact issue IDs only.
  Rationale: exact IDs make historical links stable; whole-file validation detects duplicate IDs and malformed records that would make the snapshot ambiguous. Unknown JSON object fields remain tolerated because read-only compatibility is narrower than reproducing a pinned upstream schema.
  Date/Author: 2026-09-16 / Codex.

- Decision: the configured export path is selected from supported Beads filenames under the fixed `.beads` root, preferring the path reported by current repository convention (`issues.jsonl`) and accepting the legacy `beads.jsonl` only when unambiguous.
  Rationale: the repository guide documents both names, while hard-coding only one makes adopted legacy workspaces needlessly unusable. If both exist, silent selection could consume a stale snapshot, so Doctor must fail with a corrective message.
  Date/Author: 2026-09-16 / Codex.

- Decision: retain the immutable work-link journal and task snapshot model, but remove `tracker_operations.jsonl` and every archive/restore root that exists only to recover external mutations.
  Rationale: read-only JSONL access has no external side effect to reconcile. The work-link journal is durable Jig-owned evidence and remains part of the planned linking workflow.
  Date/Author: 2026-09-16 / Codex.

- Decision: version-1 historical work-link snapshots retain description and acceptance criteria as separate fields rather than one presentation string.
  Rationale: these are distinct fields in the shared JSONL task contract. Preserving the distinction prevents an irreversible information loss at the durable evidence boundary and lets later drift checks compare task semantics without parsing prose.
  Date/Author: 2026-09-16 / Codex.

- Decision: native JSONL writes will initially use an explicit serialized handoff with `br`; unrestricted simultaneous writing and behavioral equivalence with all `br` commands are out of scope.
  Rationale: this is the smallest honest compatibility promise. Its future release gate is an interoperability matrix covering export, Jig update, ordinary `br` import, re-export, divergent edits, disabled auto-import, conflicts, and preservation of unknown fields.
  Date/Author: 2026-09-16 / user and Codex.

## Context and Orientation

The CLI crate is `crates/jig`. `crates/jig/src/context/work_config/tracker.rs` owns strict `[work.tracker]` configuration. `crates/jig/src/doctor/tracker.rs` currently discovers and executes `br`; it will instead open the JSONL snapshot through a new pure reader module. `crates/jig/src/tracker.rs` and its `tracker/` subtree are the live-provider implementation and should be replaced rather than incrementally patched.

Jig's append-only repository memory lives under `.agent/state`. `crates/jig/src/state/work_links.rs` and `state/work_links/projection.rs` define immutable plan-to-task links and their historical task snapshots. `state/jsonl/durable_append.rs` provides the crash-safe append primitive used by work links. These remain. `state/tracker_operations.rs` and its tests define intents, attempts, observations, acknowledgements, and reconciliation for external tracker writes. Because this milestone performs no tracker writes, that stream and the receipt/run/archive/restore retention logic rooted in pending operations should be removed.

The generated/adopted configuration path runs through `crates/jig/src/bootstrap/runtime_config.rs`, `bootstrap/renderer.rs`, and their tests. Existing config preservation should remain, but prose must no longer promise an executable adapter. `docs/configuration.md`, `docs/public-contract.md`, `docs/repo-intent.md`, and `docs/plans/beads-work-integration.md` describe the public boundary and delivery sequence.

The separately inspected reference implementation demonstrates bounded decoding, duplicate-key/depth rejection, typed field extraction, and supported timestamp normalization. Its application-specific graph selection, ownership scope, digest format, and privacy projection are not Jig contracts and were not copied.

## Plan of Work

First, replace the live adapter with a narrow module that resolves the fixed repository-local `.beads` boundary, selects exactly one supported JSONL export, enforces file/line/record/string/depth limits, rejects duplicate JSON object keys and duplicate issue IDs, extracts the fields needed by Jig's task snapshot, and offers exact-ID lookup. The module must perform no process execution, SQLite reads, imports, exports, or writes. Preserve unknown fields during parsing by accepting them; this reader is not yet a writer.

Second, rewrite the tracker Doctor check around that reader. An unconfigured repository remains a passing, non-required check. A configured repository passes only when its selected JSONL export is safe and valid. Diagnostics should report the root, selected relative export path, input profile, issue count, and read-only supported operation. Missing, ambiguous, oversized, malformed, or duplicate authority should fail with repository-local repair/export guidance. Doctor process availability and cancellation are irrelevant to this check and must not gate it.

Third, delete the live-provider subtree and its tests. Remove executable-version pinning, immutable executable snapshots, subprocess limits, environment hardening specific to `br`, SQLite/WAL/SHM family copying, sync-status checks, and mutation methods. Remove now-unused dependencies and imports if the crate no longer needs them elsewhere.

Fourth, remove the external tracker-operation journal. Unthread pending tracker roots from receipt and run archive, restore protection, and maintenance locks. Preserve generic archive correctness and the work-link stream. Review changes relative to `origin/master` rather than blindly reverting whole files, because several files contain both generic durable JSONL/work-link changes and mutation-specific changes.

Fifth, update tests and documentation. Adapt the useful reference parser cases with generic open-source fixtures. Cover current and legacy export filenames, ambiguous coexistence, symlinks and non-regular files, input/line/depth/string/issue limits, duplicate JSON keys, duplicate issue IDs, malformed supported fields, unknown-field tolerance, tombstones, exact-ID lookup, and macOS-neutral behavior. Rewrite the long-term plan so claims, backlinks, closure, and their recovery are explicitly future native-writer milestones with serialized `br` handoff acceptance tests.

Finally, build `target/debug/jig`, set `JIG_DEV_BIN=target/debug/jig`, run the focused crate tests, run configured Jig gates, inspect generated receipts and the final diff for stale live-provider references, update this plan's outcomes, commit, push, and verify PR #38's head.

## Concrete Steps

All commands run from the repository root.

1. Inspect retained and removed paths with `rg` and `git diff origin/master...HEAD`. Implement edits with `apply_patch`.
2. Format and run focused validation:

       cargo fmt --all -- --check
       cargo test -p jig-sh tracker
       cargo test -p jig-sh doctor
       cargo test -p jig-sh state::work_links

3. Build the current runtime and dogfood it:

       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig doctor
       JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M2NVMWVJ0QVC1159S90SJPNG
       JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M2NVMWVJ0QVC1159S90SJPNG
       JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M2NVMWVJ0QVC1159S90SJPNG

4. If the configured work check does not cover the required backend completion gate, run:

       JIG_DEV_BIN=target/debug/jig scripts/jig check test
       JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
       JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract

5. Inspect `git diff --check`, `git status --short`, and focused searches for `br 0.5.7`, `tracker_operations`, `BeadsAdapter`, SQLite snapshotting, claim, comment publication, and closure promises.
6. Finish structured work, commit all intended code/docs/state changes, push the branch, and verify the PR head commit through `gh pr view 38`.

## Validation and Acceptance

Acceptance is behavioral:

- With no `[work.tracker]`, Doctor reports the tracker as not configured and passes.
- With a valid `.beads/issues.jsonl`, Doctor succeeds without `br` on `PATH`, without reading `.beads/*.db`, and without spawning a process.
- A legacy workspace containing only `.beads/beads.jsonl` is readable; a workspace containing both supported filenames fails as ambiguous instead of choosing a potentially stale export.
- Unsafe path types, malformed JSONL, blank lines if unsupported by the profile, duplicate JSON keys, duplicate issue IDs, invalid required fields, excessive bounds, and tombstone-only exact lookups fail deterministically without leaking task body content into errors.
- Additional upstream fields do not break read-only parsing.
- Work-link journal projection, hashing, append durability, diagnostics, configuration round trips, bootstrap/adoption preservation, and archive/restore behavior all pass after tracker-operation retention is removed.
- No production source can invoke `br`, copy its SQLite store, mutate task JSONL, or append an external tracker-operation record.
- Documentation states that JSONL is a read-only snapshot in this milestone and that future writes require a tested serialized handoff.
- All configured required gates pass against the development binary.

## Idempotence and Recovery

The implementation is read-only with respect to `.beads`, so repeated Doctor checks and reader calls are idempotent. No command in this plan should import, export, reconcile, or mutate tracker data. Tests must use private temporary fixtures and generic identifiers.

Repository source edits are recoverable through Git; append-only `.agent/state` records created by the Jig workflow are not rewritten. If validation exposes an architectural mistake, amend source and documentation with another patch and record the change in this plan's Decision Log. Do not delete or rewrite existing state records. If a work check fails, inspect the recorded receipt, fix the underlying issue, and rerun the same plan-scoped gate.

## Artifacts and Notes

Baseline live-provider footprint:

    crates/jig/src/tracker.rs
    crates/jig/src/tracker/path.rs
    crates/jig/src/tracker/process.rs
    crates/jig/src/tracker/process/{budget,executable,store}.rs
    crates/jig/src/tracker/process/store/authority.rs
    crates/jig/src/tracker/profile_0_5_7.rs

Baseline mutation-recovery footprint:

    crates/jig/src/state/tracker_operations.rs
    crates/jig/src/state/tracker_operations/
    crates/jig/src/state/maintenance/restore_protection.rs
    crates/jig/src/state/runs/tests/tracker_operations.rs
    crates/jig/src/state/tests/receipt_cases/tracker_operations.rs

The source checkout used for comparison is temporary and outside this repository. No dependency on that path may enter code, tests, manifests, or generated evidence.

## Interfaces and Dependencies

The resulting internal reader should expose a small interface equivalent to:

    pub(crate) const INPUT_PROFILE: &str = "beads-rust-jsonl-v1";

    pub(crate) struct BeadsExport { /* validated issues indexed by exact ID */ }
    pub(crate) struct BeadsIssueSnapshot { /* normalized task fields */ }
    pub(crate) enum BeadsJsonlError { /* path, bounds, syntax, schema, ambiguity */ }

    impl BeadsExport {
        pub(crate) fn open(repo_root: &Path) -> Result<Self, BeadsJsonlError>;
        pub(crate) fn issue(&self, exact_id: &str) -> Result<&BeadsIssueSnapshot, BeadsJsonlError>;
        pub(crate) fn len(&self) -> usize;
        pub(crate) fn relative_path(&self) -> &Path;
    }

Names may vary to fit the crate, but the boundary must stay pure and read-only. It may depend on existing `serde`, `serde_json`, time, hashing, and safe-path primitives already in the workspace. It must not depend on `jig-owned-process`, `br`, SQLite, a network service, or an external command.

## Outcomes & Retrospective

The architectural cutover is complete. The branch removes the live `br` executable/SQLite provider and the speculative tracker-operation recovery stream, replacing them with a 467-line pure JSONL boundary and focused cross-platform tests. The retained work-link snapshot now preserves title, description, and acceptance criteria as distinct fields. The implementation commit deletes 11,947 lines while adding 1,411 lines across code, tests, documentation, and durable work records.

On Linux, required targets `api:clippy`, `api:fmt`, `api:test`, `repo:contract`, and `repo:file-budget` passed; `api:test` ran 4,120 tests with 3 skipped. After commit, the commit-bound targets were refreshed successfully under validation receipt `receipt_01M2P1D9C4S9RFK3FHRB71V6GR`. On macOS arm64, a clean temporary checkout of exact commit `6a8c0acb42808a03d567b54efc0f65b11e2d3af8` passed all 31 tracker-filtered tests, including Unix file-identity defenses and Doctor behavior without a process provider; that checkout was removed afterward. Open PR #38 includes the tested implementation commit; the final evidence-only follow-up changes only `.agent` plan and state records. Jig then closed structured work plan `plan_01M2NVMWVJ0QVC1159S90SJPNG` successfully.

The deliberate remaining boundary is product scope rather than recovery debt: this PR exposes Doctor as the only public reader. T3 adds read-only linking against the normalized snapshot. Native JSONL writes, serialized `br` handoff, and their real interoperability matrix remain deferred until a concrete task mutation is required.
