# Bind dashboard parity rows to executable test oracles

This ExecPlan repairs reopened Task A (`jig-sh-l2x.1`) after Task I (`jig-sh-l2x.10`) found that the public parity registry's `behavioral_test` values were unchecked free-form labels. The repair makes the row-by-row acceptance registry resolve to real source-backed Rust tests and fixes the two product defects those stronger oracles exposed: incomplete loop counts and conflated local/provider observation times.

Implementation baseline: `871b501c6443f5383c002f4eaf7f1d173934abf4` on branch `jig-sh-l2x`.

## Progress

- [x] Reproduce the weak oracle in an isolated worktree: replace one registry test name with `this_test_does_not_exist` and observe the registry acceptance test still pass.
- [x] Confirm only 1 of 57 current `behavioral_test` strings names an actual Rust test function; preserve evaluator hashes and record zero Task I evaluator mutations.
- [x] Replace semantic free-form labels with real test function names and record the owning test source for each parity row.
- [x] Strengthen the registry contract test to resolve every row to its source-backed `#[test]` function while allowing one strong test to prove multiple related rows.
- [x] Run focused registry and dashboard tests plus adversarial nonexistent-name, comment/string, ignored, and cfg-gated mutation checks.
- [x] Run exactly two comprehensive Claude+Codex review/fix rounds over the predecessor repair and address every finding.
- [x] Pass focused tests, strict Clippy, formatting, mutation challenges, and the repository file-budget gate.
- [x] Close Task A again, flush Beads, finish this work plan, and commit the isolated repair before stopping as requested.

## Surprises & Discoveries

- `PARITY_REGISTRY` contains all 57 section 5.6 capabilities in exact order, but its acceptance test only checks capability equality and uniqueness/non-emptiness of keys, capabilities, and test labels.
- The source comment promises that named tests are implemented by owning tasks. Direct search contradicts that promise for 56 entries; only `production_tree_contains_no_http_surface` currently resolves.
- The test-audit mutation is discriminating evidence for the evaluator defect: baseline and candidate both accept an impossible oracle name, so the current result is green-on-green and not sufficient for Task I release acceptance.
- Round one confirmed that source existence alone still allowed semantically unrelated bindings. Direct field- and interaction-level oracles were added for every cited gap before the second review.
- Round two exposed two genuine implementation omissions: `LoopView` discarded workflow/waiting counts, and recorder-only refresh replaced the shared age while retaining stale provider data. These were symptoms of an under-modeled partition boundary, not presentation-only mistakes.
- The file-budget gate exposed test-module growth across the epic. Splitting status, local parity, and resolver tests restored the 800-line hard ceiling without weakening coverage.
- Full-suite validation exposed a second test-design issue: the MCP refinement test measured review startup and receipt collection as part of its five-second final-check deadline. It now synchronizes at the completed-review phase before introducing lease contention, so it measures the intended non-blocking boundary.
- Running 3,171 tests on every available CPU repeatedly overloaded process-tree supervision, producing unrelated Git cleanup failures, missed signal-fixture boundaries, and false one-second loop-lease expiries. A 16-worker cap and category isolation still allowed arbitrary Git cleanup failures; matching the proven four-worker CI ceiling is the smaller systemic fix. The missing signal-policy binary was also assigned to the existing process-signals group.

## Decision Log

- Route the defect to Task A because Task A owns `ParityEntry`, `PARITY_REGISTRY`, and the registry completeness test. Task I remains open and explicitly depends on the reopened predecessor.
- Keep existing behavior tests and their assertions intact. Map parity rows to the strongest existing real test functions rather than duplicating 57 tests or inventing wrapper tests whose only assertion is registry membership.
- Add a source path to each registry entry. The owning `jig-ui` contract test parses that exact source with `syn`, requires an active Rust `#[test]` function with the declared name, rejects ignored/cfg-gated or textual fakes, and checks module wiring; this makes a stale rename or nonexistent label fail without recursively spawning Cargo from Cargo.
- Permit multiple rows to cite one behavior test when that test directly asserts a coherent group of fields. Uniqueness remains required for parity keys and capability descriptions, not for oracle functions.
- Bound one oracle's fan-out to five rows so future broad rebinding is an explicit contract failure.
- Keep parity metadata behind `cfg(test)`/`test-support`; repository-relative test paths are validation metadata, not a production or public runtime API.
- Pair source resolution with the existing full test-target commands in Task I. Static source binding establishes collection intent; the package/integration runs establish execution and pass status.
- Carry local and provider observation times explicitly in `StatusRefresh` and the dashboard model. Recorder-only refresh updates only the local timestamp; rendering labels both ages independently.
- Sort failure and mixed-timeline projections newest-first at the model boundary so a malformed producer ordering cannot silently violate the UI contract.
- Synchronize phase-sensitive transport tests on observable execution events instead of assuming all setup completes inside a wall-clock deadline.
- Match the local nextest worker ceiling to the proven four-worker default profile and isolate the signal-policy integration binary under the existing process-signals group. This avoids per-test exemptions and prevents host saturation from invalidating process-cleanup, lock-timing, signal, or one-second lease assertions.

## Outcomes & Retrospective

The registry now binds all 57 matrix rows to collected tests, rejects nonexistent/ignored/optionally gated/textual fakes, and remains usable from a packaged `jig-ui` crate. Both comprehensive review rounds completed. Focused dashboard and CLI tests, strict Clippy, formatting, file-budget validation, two deliberate mutation challenges, and the complete 3,171-test core suite pass with the mutations restored. Phase-sensitive transport validation now starts at the boundary it claims to measure, and the local runner uses a bounded concurrency policy for deterministic process supervision.

## Context and orientation

`crates/jig-ui/src/dashboard/parity.rs` is the public, ordered 57-row registry copied from section 5.6 of `docs/plans/unified-terminal-dashboard.md`. `crates/jig-ui/tests/dashboard_contract.rs` validates that registry. Actual behavior tests live in dashboard unit modules under `crates/jig-ui/src/terminal/`, dashboard contract tests, and CLI architecture/cutover integration tests under `crates/jig/tests/`. Task I runs all of those targets, but it cannot currently prove that the registry points at them.

## Plan of work

Extend `ParityEntry` with an owning source path and update the registry macro so every entry names an actual function from the strongest existing test module. Replace aspirational labels with exact function names. Add a small source parser in the dashboard contract integration test that recognizes `#[test]` functions and fails with the parity key, source, and missing function. Preserve the exact 57 capability rows and areas. Then rerun the direct test, the owning test targets, Clippy/formatting, and an isolated mutation that changes a declared function name to an impossible value and must fail.

## Validation and acceptance

`cargo test -p jig-ui --test dashboard_contract parity_registry_has_one_named_oracle_for_every_matrix_row -- --exact` must pass on the repair and fail after an isolated `behavioral_test` mutation. `cargo test -p jig-ui`, `cargo test -p jig-sh --test ui_architecture --test ui_cutover`, `cargo fmt --all -- --check`, strict Clippy for `jig-ui` and `jig-sh`, and all required structured work gates must pass. Every source path must be repository-relative, exist, and contain the declared collected test. Production changes are limited to the two dashboard-model correctness fixes exposed by the stronger oracles; test harness policy changes are limited to phase-accurate synchronization and bounded process concurrency. No schema, version, release, or generated contract file may change.

## Idempotence and recovery

The repair is one isolated commit. Reverting it restores the old weak oracle and the two exposed dashboard-model defects. Mutation challenges are restored before final validation.

## Interfaces and dependencies

`ParityEntry` is an internal CLI-owned crate API. Adding `test_source` is an additive metadata field. Consumers continue using `key`, `capability`, `area`, and `behavioral_test`; Task I additionally relies on the registry test to prove each referenced function exists in the declared source.
