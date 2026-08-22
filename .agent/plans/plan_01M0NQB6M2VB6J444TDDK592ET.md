# Close Execution Review Findings

This ExecPlan is a living document. Maintain it in accordance with `.agent/PLANS.md` as implementation and validation proceed.

## Purpose / Big Picture

The unpublished branch introduced cooperative cancellation, bounded process capture, supported-host enforcement, and related workflow changes. A comprehensive Claude-plus-Codex review found two lifecycle gaps in those abstractions, one fail-open policy check, and three smaller cases of test or diagnostic coupling. After this work, cancellation must remain effective through receipt finalization, a worker's authoritative result file must be bounded while the process is alive, the supported-host inventory must distinguish an empty result from a failed inventory command, and the smaller misleading or coupled behaviors must be removed. Each independently reviewable slice is committed separately and the complete configured test suite is run at the end.

## Progress

- [x] (2026-08-22T23:55:02+02:00) Completed the comprehensive review, reproduced the supported-host fail-open behavior, and classified the findings by owning abstraction.
- [x] (2026-08-22T23:55:02+02:00) Opened structured work as `plan_01M0NQB6M2VB6J444TDDK592ET` and built the development `jig` binary.
- [x] (2026-08-22T23:58:59+02:00) Committed this plan and its append-only work records as the planning slice.
- [x] (2026-08-23T00:18:09+02:00) Made Git-backed receipt finalization cooperative without sacrificing durable cancellation receipts; the full `jig-sh` library and signal-policy integration tests passed before committing the slice.
- [x] (2026-08-23T00:22:00+02:00) Monitored the Codex authoritative result file during execution, terminated the owned process tree at the configured limit, preserved external cancellation semantics, and passed all worker-runner regressions before committing the slice.
- [x] (2026-08-23T00:23:29+02:00) Made the supported-host source inventory fail closed, narrowed Windows path matching, excluded append-only work plans as a class, and passed the direct script plus four integration regressions before committing the slice.
- [ ] Decouple the timeout test fixture, correct pull-request diagnostics, cover included Rust fragments with the format gate, format those fragments, and commit the cleanup slice.
- [ ] Run targeted checks, then the full configured test suite and remaining repository gates through the development binary.
- [ ] Record evidence, update this plan's outcome, finish structured work, and commit the final append-only records.

## Surprises & Discoveries

- Observation: the process supervisor cancels and reaps the owned child tree correctly, but callers then use blocking Git helpers to collect receipt metadata and fingerprints.
  Evidence: `runtime/tool_execution.rs`, `runtime/worker_runner.rs`, `runtime/work/checks.rs`, `runtime/work/review.rs`, and `runtime/loops.rs` call blocking receipt or fingerprint functions after supervised work returns.

- Observation: receipt durability and receipt enrichment are coupled even though only the append is mandatory after cancellation.
  Evidence: `state/receipts.rs::record_receipt` collects Git metadata before appending the JSONL record. Cancellation-aware fingerprinting exists, but there is no cancellation-aware full metadata path.

- Observation: Codex's `-o` output is a separate authoritative side channel, outside stdout/stderr capture and therefore outside the existing byte-limit monitor.
  Evidence: `runtime/worker_runner.rs` checks the result file's metadata only after `run_worker_command` returns.

- Observation: the supported-host script erases both `git grep` and `git ls-files` failures with `|| true`, so a missing or failed inventory looks identical to a clean inventory. Its `windows?` path expression also accepts `window`.
  Evidence: replacing `git` with a function that exits 128 still lets `scripts/check-supported-host-surface.sh` exit successfully.

- Observation: `cargo fmt --all -- --check` does not discover source fragments loaded with `include!`; direct `rustfmt --check` reports drift in multiple files under `doctor_parts` and `doctor/tests_parts`.
  Evidence: running `rustfmt --edition 2024 --check` over those fragment directories reports formatting differences while Cargo's format check passes.

- Observation: the installed Rust 1.97 Clippy reports three pre-existing `collapsible_if` warnings outside the cancellation slice when warnings are denied.
  Evidence: `cargo clippy -p jig-sh --all-targets -- -D warnings` points to `crates/jig/build.rs` and two sites in `crates/jig-ui`; these files were unchanged by Milestone 1 and will be handled with the gate-cleanup slice if still required by the configured command.

## Decision Log

- Decision: treat the receipt finding as a lifecycle-abstraction defect, not a one-line omission.
  Rationale: fixing only one canceled branch would leave the same post-cancellation Git spawn in workers, review workflows, loops, and batch checks. The durable append must be separated conceptually from optional Git enrichment, with cancellation threaded through every cooperative caller.
  Date/Author: 2026-08-22 / Codex

- Decision: preserve durable receipts after a child exits or mutation occurs, but stop starting external Git processes once cancellation is observable.
  Rationale: cancellation must bound further work while auditability still requires a terminal record. Unavailable metadata can be represented by the receipt's existing collection-error fields.
  Date/Author: 2026-08-22 / Codex

- Decision: monitor the authoritative worker result file from the same observer that supervises the owned child tree.
  Rationale: a post-exit size check cannot enforce a resource bound. Polling file metadata alongside cancellation makes the limit part of the process lifecycle and ensures descendants are terminated through the existing owned-process machinery.
  Date/Author: 2026-08-22 / Codex

- Decision: keep a final result-file size check after worker exit as a race defense.
  Rationale: the process may cross the limit between the final observer poll and exit; defense in depth is cheap and preserves the current validation behavior.
  Date/Author: 2026-08-22 / Codex

- Decision: make every command failure in the supported-host inventory visible, tolerating only grep's documented no-match status.
  Rationale: policy enforcement is meaningful only when a clean result can be distinguished from inability to inspect the source tree.
  Date/Author: 2026-08-22 / Codex

- Decision: extend the repository format command with explicit checks for included source fragments rather than relying on developers to remember a second command.
  Rationale: the missing coverage is structural to Cargo's module discovery. Encoding it in the configured gate permanently reduces recurrence.
  Date/Author: 2026-08-22 / Codex

## Outcomes & Retrospective

Implementation is in progress. On completion, summarize the landed commits, validation evidence, any remaining risks, and whether the structural invariants above proved sufficient.

## Context and Orientation

The `jig-sh` binary lives in `crates/jig`. `crates/jig-owned-process` owns process-tree supervision and already provides polling through `OwnedProcessObserver`; this plan should reuse that boundary rather than replace it. Cooperative runtime execution is orchestrated under `crates/jig/src/runtime`. Receipt construction and append behavior are in `crates/jig/src/state/receipts.rs`, while Git metadata and worktree fingerprinting live in `crates/jig/src/git_receipts.rs`.

Worker execution is in `crates/jig/src/runtime/worker_runner.rs`. Codex workers receive a temporary path through `-o`; that file contains the authoritative final response and is not stdout or stderr. The configured execution-output limit is `EXECUTION_OUTPUT_CAPTURE_LIMIT`.

Supported-host enforcement is implemented by `scripts/check-supported-host-surface.sh` and exercised by `crates/jig/tests/supported_host_surface.rs`. Repository gate commands are configured in `.jig.toml`. Doctor source is split into `include!` fragments under `crates/jig/src/doctor_parts` and `crates/jig/src/doctor/tests_parts`, which Cargo's normal format discovery does not check independently.

The smaller findings are in `crates/jig/src/policy/tests.rs` and `crates/jig/src/runtime/work/pr_manager.rs`. The policy schema fixture currently gives all consumers a one-second execution timeout even though only one timeout test needs it. Pull-request repair cancellation formats a JSON value instead of the already parsed commit string, and successful reconciliation uses mismatch-oriented wording.

## Plan of Work

First, generalize Git receipt collection so blocking and cancellation-aware modes share one implementation. Add cancellation-aware public entry points for Git metadata and receipt recording. Cooperative runtime paths pass their observer to these entry points. Before-action fingerprints remain strict because cancellation means the action must not start. After-action fingerprints and metadata remain best effort so a terminal receipt can still be appended; cancellation is represented as unavailable enrichment rather than permission to launch more Git processes. Add tests that cancellation before collection starts creates no Git child and still permits a durable terminal receipt.

Second, add a worker observer that delegates ordinary cancellation, output, and polling behavior to `ProcessExecutionObserver` while also inspecting the authoritative result path. If the file exceeds the configured capture limit or cannot be inspected, the observer requests termination of the existing owned tree and retains a typed reason. Translate that internally initiated stop into the existing worker execution failure surface, while allowing genuine external cancellation to remain cancellation. Test with a worker that writes beyond the limit and leaves a delayed descendant; the result must fail promptly and the descendant marker must never appear.

Third, rewrite the supported-host script's inventory steps with explicit status handling compatible with Bash 3.2. Exit status 1 from grep means no matches; other statuses fail the check. `git ls-files` itself must succeed before its output is filtered. Narrow the path expression from `windows?` to an actual `windows` path component. Add tests using a temporary fake `git` executable for both command failure and a benign `window.rs` filename.

Fourth, move the one-second execution timeout into the test that exercises timeout supervision, use the parsed repair commit string in cancellation diagnostics, give successful remote reconciliation affirmative wording, and update tests. Add `scripts/check-rust-format.sh`, configure `.jig.toml` to use it, and format all standalone doctor fragments so the new gate begins clean.

Finally, rebuild the development binary, run targeted tests after each slice, run format, Clippy, contract, and the complete configured test gate with `JIG_DEV_BIN=target/debug/jig`, inspect receipts and gate status, update this living plan, and finish structured work.

## Milestones

### Milestone 1: Cancellation-safe receipt lifecycle

At the end of this milestone, every cooperative runtime path can finalize a durable receipt after cancellation without starting new Git processes. Before-action cancellation prevents the action; after-action cancellation degrades optional Git enrichment. Tests demonstrate both no-spawn behavior and receipt durability.

### Milestone 2: Bounded worker authoritative output

At the end of this milestone, the Codex result-file limit is enforced while the worker runs, using the same process-tree termination path as timeout and cancellation. Tests demonstrate prompt failure and descendant cleanup.

### Milestone 3: Reliable policy inventory

At the end of this milestone, the supported-host check fails when its source inventory cannot run, permits empty inventories only when commands succeeded, and no longer mistakes `window.rs` for a Windows-specific path.

### Milestone 4: Reduced fixture, diagnostic, and format drift

At the end of this milestone, unrelated schema tests no longer inherit timeout policy, pull-request messages reflect their actual state, and the normal configured format gate covers included Rust fragments.

### Milestone 5: Repository-wide validation and evidence

At the end of this milestone, all targeted tests and the full configured test suite pass, remaining gates are green, structured evidence is recorded, and the work session is finished.

## Concrete Steps

Run commands from `/home/aa/Documents/jig-sh`.

1. Inspect the diff and commit this plan plus the append-only records created by `scripts/jig work start`.

2. Implement Milestone 1 in `git_receipts.rs`, `state/receipts.rs`, and cooperative runtime callers. Run focused library and runtime tests, then commit as `fix(runtime): keep receipt finalization cancellable`.

3. Implement Milestone 2 in `runtime/worker_runner.rs` and its tests. Run the worker-runner and owned-process regression coverage, then commit as `fix(worker): bound authoritative result while running`.

4. Implement Milestone 3 in `scripts/check-supported-host-surface.sh` and `crates/jig/tests/supported_host_surface.rs`. Run that integration test and the script directly, then commit as `fix(policy): make supported-host inventory fail closed`.

5. Implement Milestone 4 in the policy and pull-request tests/code, add the format wrapper, update `.jig.toml`, and mechanically format included fragments. Run their focused tests and the new format command, then commit as `fix(review): close low-risk review findings`.

6. Rebuild and force the development launcher:

       cargo build -p jig-sh --bin jig
       export JIG_DEV_BIN=target/debug/jig

7. Run repository verification, including the complete test suite:

       scripts/jig check fmt
       scripts/jig check clippy
       scripts/jig check contract
       scripts/jig work check --plan-id plan_01M0NQB6M2VB6J444TDDK592ET

8. Inspect structured results and finish the session:

       scripts/jig work gates --plan-id plan_01M0NQB6M2VB6J444TDDK592ET
       scripts/jig work evidence --plan-id plan_01M0NQB6M2VB6J444TDDK592ET
       scripts/jig work receipts --plan-id plan_01M0NQB6M2VB6J444TDDK592ET
       scripts/jig work status
       scripts/jig work finish

9. Update this plan's Progress, Discoveries, Decision Log if necessary, and Outcomes. Commit only the resulting plan and append-only evidence records as the final documentation slice.

## Validation and Acceptance

Acceptance is behavioral, not merely compilation:

- With cancellation already observable, receipt finalization appends its terminal JSONL record and does not invoke Git metadata or fingerprint subprocesses.
- Cooperative review, worker, loop, direct-tool, and batch-check paths use the cancellation-aware receipt boundary.
- A worker that grows its `-o` file beyond `EXECUTION_OUTPUT_CAPTURE_LIMIT` is terminated before normal exit, its delayed descendants are reaped, and the error identifies the result-file limit rather than user cancellation.
- A genuine external worker cancellation still reports cancellation even if the result-file monitor is active.
- The supported-host script exits nonzero when `git grep` or `git ls-files` fails, treats a successful no-match result as clean, and permits a tracked file named `window.rs`.
- Only the timeout-specific policy test fixture opts into the one-second timeout.
- Pull-request cancellation and reconciliation messages contain an unquoted commit and success-oriented confirmation text.
- `scripts/jig check fmt`, `scripts/jig check clippy`, `scripts/jig check contract`, and the complete `scripts/jig check test` path pass through the development binary.
- `git diff --check` is clean and commits remain separated by the slices described above.

## Idempotence and Recovery

All source edits and tests are repeatable. The format wrapper is read-only; running `rustfmt` again is idempotent. Targeted and full checks may append new receipt records, as designed by the repository harness. Never rewrite existing `.agent/state/*.jsonl` entries; if a check fails, fix the source and rerun it so the new result is appended.

If a commit is interrupted, inspect `git status --short`, retain all append-only records, and stage only the intended slice. Do not reset or discard unrelated work. If the development binary is stale after runtime edits, rebuild it before invoking `scripts/jig` and keep `JIG_DEV_BIN=target/debug/jig` set.

## Artifacts and Notes

- Comprehensive review scope: `origin/master...HEAD`, covering 74 unpublished commits at review time.
- Reproduction of the fail-open guard: exporting a shell `git` function that returns 128 and running `bash scripts/check-supported-host-surface.sh` returned zero before the fix.
- Existing cancellation-aware worktree fingerprinting provides the initial implementation pattern in `crates/jig/src/git_receipts.rs`.
- Existing owned-process polling and descendant cleanup remain the mechanism of enforcement; no second process supervisor should be introduced.

## Interfaces and Dependencies

The receipt slice should expose cancellation-aware equivalents of full Git metadata collection and `state::receipts::record_receipt`, using the repository's existing cancellation callback shape (`&dyn Fn() -> bool` or the established observer abstraction). Blocking entry points remain for non-cooperative commands, implemented through the same internal collection code.

The worker slice should introduce an observer local to `worker_runner.rs` that implements `jig_owned_process::OwnedProcessObserver`, delegates to `ProcessExecutionObserver`, and stores at most one typed result-file failure. `run_worker_command` should accept an optional authoritative result path so existing callers and tests without `-o` retain their current behavior.

The policy script must remain Bash 3.2 compatible and depend only on existing repository tools (`git`, `grep`, and standard shell facilities). The format wrapper depends on the workspace's Cargo toolchain and standalone `rustfmt`; it becomes the value of `.jig.toml`'s `rust_fmt_check_command`.

Plan revision note (2026-08-22): Expanded the structured-work stub into a self-contained implementation and validation plan after tracing each review finding to its owning lifecycle or policy abstraction.

Plan revision note (2026-08-23): Marked the cancellation-safe receipt milestone complete after 1,570 library tests and both runtime signal-policy tests passed, and recorded unrelated Rust 1.97 Clippy drift for the later gate-cleanup slice.

Plan revision note (2026-08-23): Marked the authoritative worker-output milestone complete after proving prompt overflow termination, descendant cleanup, post-exit race defense, and external cancellation precedence.

Plan revision note (2026-08-23): Marked the supported-host inventory milestone complete after proving both Git inventory commands fail closed and a benign `window.rs` path remains allowed; generalized the plan exclusion instead of adding another per-plan exception.
