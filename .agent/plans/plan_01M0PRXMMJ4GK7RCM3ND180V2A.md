# Harden Review Boundary Contracts

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current as implementation proceeds. Maintain this document according to `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

Three review findings expose two recurring design weaknesses rather than three unrelated typos. First, the repository's supported-host guard treats historical release notes as if they were current implementation or support guidance, so accurate history had to be deleted to make the guard pass. Second, two runtime boundaries perform deferred observation work without explicit contracts: MCP progress delivery can replace the actual tool failure, and worker-result inspection inherits the owned-process transcript polling frequency.

After this work, the repository can state its current Linux/macOS host policy while retaining truthful released history, an MCP client receives the tool failure as the primary error even when buffered progress delivery also fails, and a chatty worker cannot force filesystem metadata inspection on every transcript poll. Each behavior is protected by a focused regression test, each remediation is committed separately, and the repository's configured full test suite passes at the end.

## Progress

- [x] (2026-08-23 07:38Z) Opened structured work, read `.agent/PLANS.md`, `agent-map.md`, and `crates/jig/AGENTS.md`, and inspected the three affected boundaries and existing regression tests.
- [x] (2026-08-23 07:38Z) Committed this self-contained execution plan and its structured-work start records as `187ba84`.
- [x] (2026-08-23 07:45Z) Restored accurate released host-support history, added a current breaking-change note, narrowed the active-host content guard, and passed all 5 supported-host regression tests; commit remains the immediate next action.
- [ ] Introduce explicit MCP tool/progress outcome composition, cover simultaneous failure ordering, and commit that slice.
- [ ] Add an independent time-based authoritative-result inspection schedule, cover its cadence and cancellation behavior, and commit that slice.
- [ ] Build the development binary, run focused checks, repository gates, and `scripts/jig check test`, then record evidence and finish structured work.

## Surprises & Discoveries

- Observation: `scripts/check-supported-host-surface.sh` excludes append-only state and plan evidence from its content scan but still scans `CHANGELOG.md`, even though a changelog necessarily records obsolete support commitments.
  Evidence: `git diff origin/master...HEAD -- CHANGELOG.md` shows numerous released Windows entries deleted while the script's `git grep` pathspec has no `:!CHANGELOG.md` exclusion.

- Observation: Restoring the historical record is cleanly separable from current release changes.
  Evidence: after restoration, `git diff --numstat origin/master -- CHANGELOG.md` reports `2 0 CHANGELOG.md`: only the pre-existing Rust-version entry and the new host-support breaking entry differ from the published history.

- Observation: `crates/jig/src/mcp.rs::handle_tool_call` already defers progress output until after tool execution, but sequential `?` operators give the later flush error unconditional precedence over the earlier tool result.
  Evidence: the function stores `tool_result`, calls `observer.flush()?`, and only then evaluates `tool_result?`.

- Observation: `WorkerProcessObserver::cancelled` performs filesystem metadata inspection, and the owned-process runner invokes that callback at the transcript polling cadence, which can be about one millisecond while output is continuously readable.
  Evidence: `cancelled` calls `inspect_authoritative_output` directly; the existing overflow regression proves prompt detection but does not constrain inspection frequency.

## Decision Log

- Decision: Treat changelog and append-only execution records as historical evidence, not as active host-support surface, while retaining tracked-path inventory across the entire repository.
  Rationale: Current source and current guidance must not advertise or implement an unsupported host, but released history must remain factually accurate. The tracked-path scan remains global so dormant platform-specific artifacts cannot hide in a historically exempt document category.
  Date/Author: 2026-08-23 / Codex

- Decision: Combine the tool execution result and progress flush result in one helper with a complete four-case outcome table.
  Rationale: An explicit composition boundary makes precedence reviewable and testable. Tool failure is primary when both fail; a flush failure still fails an otherwise successful call; either isolated failure remains unchanged.
  Date/Author: 2026-08-23 / Codex

- Decision: Rate-limit authoritative-result metadata inspection using monotonic time inside `WorkerProcessObserver`, while checking external cancellation first and retaining the unconditional post-exit read and size check.
  Rationale: A monotonic schedule decouples filesystem work from transcript activity without creating a second thread. Immediate pre-start inspection, prompt in-process overflow detection, external cancellation precedence, and final post-exit validation remain intact.
  Date/Author: 2026-08-23 / Codex

- Decision: Use three implementation commits, one for each independent invariant, with separate planning and evidence commits.
  Rationale: Each behavioral change can be reviewed, reverted, and bisected without mixing policy, transport errors, and process supervision.
  Date/Author: 2026-08-23 / Codex

## Outcomes & Retrospective

Implementation is not complete. At completion this section will record the exact commits, focused tests, full-suite result, any remaining unrelated gate failures, and whether the changes met the intended reduction in bug surface.

## Context and Orientation

The repository root is `/home/aa/Documents/jig-sh`. `scripts/check-supported-host-surface.sh` is a committed contract check. Its first `git grep` rejects current tracked content associated with an unsupported host; its second `git ls-files` pipeline rejects tracked artifact names. `crates/jig/tests/supported_host_surface.rs` executes the real script and substitutes a small fake `git` program to verify failure handling. `CHANGELOG.md` contains both an `Unreleased` section describing the current cutover and versioned sections that are immutable historical claims.

`crates/jig/src/mcp.rs` implements the standard-input/standard-output Model Context Protocol server. A tool call runs through `handle_tool_call`. `McpProgressObserver` buffers lifecycle messages and bounded stdout/stderr previews during the call, then `flush` writes JSON-RPC progress notifications. A deferred finalizer is work that runs after the primary operation to publish observations or clean up. The primary operation's error must not disappear merely because that finalizer also failed.

`crates/jig/src/runtime/worker_runner.rs` launches a Codex worker under the `jig-owned-process` supervisor. The worker's transcript is diagnostic, while a temporary last-message file is the authoritative result. `WorkerProcessObserver` checks the authoritative file during execution so an oversized result terminates the complete owned process group promptly. The supervisor also polls stdout and stderr; transcript readiness can make those polling iterations much more frequent than the normal idle interval. A monotonic deadline uses `std::time::Instant`, which cannot move backward when the wall clock changes, to decide when metadata inspection is due.

The root `AGENTS.md` requires runtime changes to be built as `target/debug/jig` and exercised with `JIG_DEV_BIN=target/debug/jig`. It also requires backend work to finish with `scripts/jig check test`. Structured-work records under `.agent/state/*.jsonl` are append-only and must be changed only through the Jig workflow commands.

## Plan of Work

Milestone one repairs the policy/history boundary. Add `CHANGELOG.md` to the content scan's excluded historical records, leaving current source, current documentation, and the global tracked-path inventory covered. Add a fake-git regression in `crates/jig/tests/supported_host_surface.rs` that succeeds only when the changelog exclusion is passed to `git grep`; construct any unsupported-host fixture token in pieces so the real repository scan does not reject its own regression. Restore every deleted released entry from the merge-base diff, and add one explicit breaking entry under `Unreleased` explaining that current Jig host execution supports Linux and macOS only. Run the focused integration test and the actual script. Commit this milestone alone.

Milestone two repairs MCP error composition. In `crates/jig/src/mcp.rs`, add a private generic helper that accepts the primary tool `Result<T>` and the progress flush `Result<()>`. Match all four success/failure combinations. If both fail, return one error whose displayed message begins with the tool failure and also reports the progress-delivery failure. Change `handle_tool_call` to call this helper after both operations have completed. Add unit tests proving the dual-failure order and the successful-tool/failed-progress case. Run the crate's MCP unit tests and commit this milestone alone.

Milestone three repairs result inspection cadence. In `crates/jig/src/runtime/worker_runner.rs`, add a small constant inspection interval and an `Instant` deadline to `WorkerProcessObserver`. Replace the unconditional metadata call in `cancelled` with an `inspect_authoritative_output_if_due` helper. Check the execution cancellation source first so an already-cancelled command still yields `CancelledBeforeStart`; otherwise inspect immediately on the first callback and at most once per interval. Retain `read_worker_output_file` after process exit as the authoritative final validation. Add a deterministic unit test that controls the observer's private deadline and proves a missing output file is ignored before the deadline and detected when due. Keep the existing live-overflow and pre-start cancellation regressions passing. Commit this milestone alone.

After all three milestones, format the workspace, rebuild the development binary, run the supported-host and crate-focused tests, run the configured contract gate, and run `JIG_DEV_BIN=target/debug/jig scripts/jig check test` as the full-suite acceptance command. Use `scripts/jig work check`, `work evidence`, `work gates`, and `work receipts` to connect the results to this plan. Update every living section of this plan, append a final structured-work progress record, finish the work only after required gates pass, and commit the plan and append-only evidence.

## Concrete Steps

Run every command from `/home/aa/Documents/jig-sh`.

Create the structured plan and establish the development binary:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work start --title "Harden review boundary contracts" --body "..." --print-plan-id

For the policy slice, edit with `apply_patch`, then run:

    cargo test -p jig-sh --test supported_host_surface
    scripts/check-supported-host-surface.sh
    git diff --check
    git add CHANGELOG.md scripts/check-supported-host-surface.sh crates/jig/tests/supported_host_surface.rs
    git commit -m "fix(policy): separate support policy from release history"

For the MCP slice, edit with `apply_patch`, then run:

    cargo test -p jig-sh mcp::tests
    git diff --check
    git add crates/jig/src/mcp.rs
    git commit -m "fix(mcp): preserve tool errors across progress flush"

For the worker slice, edit with `apply_patch`, then run:

    cargo test -p jig-sh worker_supervision_preserves_cancellation_with_result_monitor
    cargo test -p jig-sh worker_result_file_limit_terminates_process_group_while_running
    cargo test -p jig-sh worker_result_file_inspection_obeys_its_schedule
    git diff --check
    git add crates/jig/src/runtime/worker_runner.rs
    git commit -m "fix(worker): decouple result inspection cadence"

For final acceptance and structured evidence, run:

    cargo fmt --all -- --check
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig check test
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0PRXMMJ4GK7RCM3ND180V2A
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0PRXMMJ4GK7RCM3ND180V2A
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0PRXMMJ4GK7RCM3ND180V2A
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M0PRXMMJ4GK7RCM3ND180V2A
    JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01M0PRXMMJ4GK7RCM3ND180V2A --summary "..."

Expected focused commands exit zero and name the requested regression as passed. The full test command may partition the workspace internally; every configured partition must exit zero. `work gates` must show every required gate satisfied before `work finish` is run.

## Validation and Acceptance

The policy repair is accepted when released unsupported-host entries remain in `CHANGELOG.md`, the `Unreleased` section explicitly records the support cutover, `scripts/check-supported-host-surface.sh` exits zero on the real checkout, and `cargo test -p jig-sh --test supported_host_surface` passes. The regression must demonstrate that changelog content is outside the active-content scan without weakening the global path inventory.

The MCP repair is accepted when a unit test supplies both a synthetic tool error and a synthetic progress error and observes both in the result with the tool error first. A separate outcome must demonstrate that a progress error still fails a successful tool call. Existing progress buffering and framing tests must remain green.

The worker repair is accepted when the schedule unit test proves metadata is not re-read before the monotonic deadline and is re-read once due, the existing cancelled-before-start test retains its typed outcome, and the live oversized-result test still terminates the worker process group before its escaped marker can be written.

The overall work is accepted only when `cargo fmt --all -- --check`, the supported-host checker, the contract gate, and `JIG_DEV_BIN=target/debug/jig scripts/jig check test` all exit zero. Any unrelated optional lint failure must be recorded with evidence but does not replace the required full test result.

## Idempotence and Recovery

All focused tests, format checks, builds, contract checks, and full-suite checks are safe to repeat. The script change is declarative. The observer scheduling and result-composition changes do not alter durable file formats or public payload schemas.

Do not rewrite `.agent/state/*.jsonl`; rerun Jig workflow commands so they append new records. If a focused test fails, edit only the uncommitted milestone and rerun it before committing. If a later milestone fails, preserve earlier commits and repair the current slice in a follow-up commit only if it has already been committed. If the full suite exposes a regression belonging to one slice, fix it in a dedicated follow-up commit rather than rewriting published local commits. Never restore unrelated worktree files or reset the branch.

## Artifacts and Notes

Initial branch state:

    ## master...origin/master [ahead 80]
    baccfe7 docs(agent): record execution review evidence
    5d1ac97 fix(review): close low-risk review findings
    8483c58 fix(policy): make supported-host inventory fail closed
    1401143 fix(worker): bound authoritative result while running
    6575fbe fix(runtime): keep receipt finalization cancellable

Initial problematic MCP sequence:

    let tool_result = call_tool_with_observer(...);
    observer.flush()?;
    let tool_result = tool_result?;

Initial problematic worker callback:

    self.execution.cancelled()
        || self.result_file_failure.is_some()
        || self.inspect_authoritative_output()

## Interfaces and Dependencies

No new third-party dependencies are required. The policy test continues to use `tempfile` and `std::process::Command`. The MCP helper remains private to `crates/jig/src/mcp.rs` and has the conceptual signature `fn combine_tool_and_progress_results<T>(tool_result: anyhow::Result<T>, progress_result: anyhow::Result<()>) -> anyhow::Result<T>`. The worker observer uses `std::time::Instant` and `Duration`; it retains the existing `OwnedProcessObserver` interface and `WorkerResultFileFailure` mapping. No command-line, JSON-RPC schema, durable-state schema, or public Rust API changes are introduced.

Plan revision note (2026-08-23 07:38Z): Replaced the one-line work-start body with a self-contained execution plan after inspecting the affected policy, MCP, and worker boundaries. The plan records the structural causes, fixes, separate commit strategy, and full-suite acceptance criteria so work can resume from this file alone.

Plan revision note (2026-08-23 07:42Z): Marked the planning commit complete and recorded partial progress on the policy milestone after narrowing the guard and adding its regression. Released changelog entries still need restoration before this slice is valid.

Plan revision note (2026-08-23 07:45Z): Marked the policy implementation and focused validation complete after restoring all 22 deleted historical entries. The five-test integration target and the real checker both pass; the slice is ready to commit.


Policy slice complete: separated active supported-host scanning from immutable release history, restored all 22 deleted historical entries, added the Unreleased breaking cutover, and passed scripts/check-supported-host-surface.sh plus 5/5 supported_host_surface tests.