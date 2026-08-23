# Harden Review Boundary Contracts

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current as implementation proceeds. Maintain this document according to `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

Three review findings expose two recurring design weaknesses rather than three unrelated typos. First, the repository's supported-host guard treats historical release notes as if they were current implementation or support guidance, so accurate history had to be deleted to make the guard pass. Second, two runtime boundaries perform deferred observation work without explicit contracts: MCP progress delivery can replace the actual tool failure, and worker-result inspection inherits the owned-process transcript polling frequency.

After this work, the repository can state its current Linux/macOS host policy while retaining truthful released history, an MCP client receives the tool failure as the primary error even when buffered progress delivery also fails, and a chatty worker cannot force filesystem metadata inspection on every transcript poll. Each behavior is protected by a focused regression test, each remediation is committed separately, and the repository's configured full test suite passes at the end.

## Progress

- [x] (2026-08-23 07:38Z) Opened structured work, read `.agent/PLANS.md`, `agent-map.md`, and `crates/jig/AGENTS.md`, and inspected the three affected boundaries and existing regression tests.
- [x] (2026-08-23 07:38Z) Committed this self-contained execution plan and its structured-work start records as `187ba84`.
- [x] (2026-08-23 07:45Z) Restored accurate released host-support history, added a current breaking-change note, narrowed the active-host content guard, and passed all 5 supported-host regression tests; commit remains the immediate next action.
- [x] (2026-08-23 07:49Z) Unified CLI and MCP progress outcome composition, made the primary operation error first on dual failure, and passed 11 MCP plus 12 CLI progress unit tests; commit remains the immediate next action.
- [x] (2026-08-23 07:55Z) Added an independent monotonic 10ms authoritative-result inspection schedule and passed the deterministic cadence, cancelled-before-start, and live process-group overflow regressions; commit remains the immediate next action.
- [x] (2026-08-23 08:48Z) Tested a four-slot Nextest reservation for the PTY browser; focused validation passed, but a later complete gate proved the in-process isolation insufficient.
- [x] (2026-08-23 09:16Z) Tested a 60-second PTY watchdog; focused validation passed, but a later complete gate failed at 62.120 seconds and disproved the slow-operation hypothesis.
- [x] (2026-08-23 09:41Z) Restored the meaningful 30-second watchdog, split the two PTY tests into a dedicated serial Nextest process, proved exact 2,163/437/2 selector coverage, and passed both PTY tests in 4.410 seconds; commit remains the immediate next action.
- [ ] Build the development binary, run focused checks, repository gates, and `scripts/jig check test`, then record evidence and finish structured work.

## Surprises & Discoveries

- Observation: `scripts/check-supported-host-surface.sh` excludes append-only state and plan evidence from its content scan but still scans `CHANGELOG.md`, even though a changelog necessarily records obsolete support commitments.
  Evidence: `git diff origin/master...HEAD -- CHANGELOG.md` shows numerous released Windows entries deleted while the script's `git grep` pathspec has no `:!CHANGELOG.md` exclusion.

- Observation: Restoring the historical record is cleanly separable from current release changes.
  Evidence: after restoration, `git diff --numstat origin/master -- CHANGELOG.md` reports `2 0 CHANGELOG.md`: only the pre-existing Rust-version entry and the new host-support breaking entry differ from the published history.

- Observation: `crates/jig/src/mcp.rs::handle_tool_call` already defers progress output until after tool execution, but sequential `?` operators give the later flush error unconditional precedence over the earlier tool result.
  Evidence: the function stores `tool_result`, calls `observer.flush()?`, and only then evaluates `tool_result?`.

- Observation: The CLI progress path already had a result-composition helper, but its dual-failure rendering put the progress flush context before the operation error.
  Evidence: `crates/jig/src/progress.rs::combine_progress_delivery` used `error.context(...)`, and its regression asserted that the rendered error started with `Execution progress also failed to flush`.

- Observation: `WorkerProcessObserver::cancelled` performs filesystem metadata inspection, and the owned-process runner invokes that callback at the transcript polling cadence, which can be about one millisecond while output is continuously readable.
  Evidence: `cancelled` calls `inspect_authoritative_output` directly; the existing overflow regression proves prompt detection but does not constrain inspection frequency.

- Observation: A 10ms result-file schedule preserves the supervisor's prior idle-path inspection responsiveness while bounding chatty-path metadata calls independently of transcript readiness.
  Evidence: `worker_result_file_inspection_obeys_its_schedule` forces the private monotonic timestamp before and at the interval boundary; the missing file is ignored before the boundary and detected once due. The existing live overflow test still completes in about 1.35 seconds and prevents the escaped marker.

- Observation: The full suite's Vault partition failed the same PTY browser test twice at about 32.1 seconds, while the test passed in 3.25-3.33 seconds under both Cargo test and the exact Nextest selector in isolation.
  Evidence: full-run summaries reported 438 of 439 Vault tests passed and `browser_unlocks_resizes_locks_and_restores_the_terminal_on_quit` failed at 32.127 and 32.111 seconds. The test's per-interaction budget is 30 seconds and `.config/nextest.toml` allowed it to share the four-slot `vault-crypto` group with three other expensive key-derivation tests.

- Observation: Nextest applies the new specific PTY override ahead of the broad Vault override, and the complete Vault partition passes with the reservation.
  Evidence: `cargo nextest show-config test-groups` lists the exact browser test under the specific override. The exact test passed in 3.307 seconds, and a non-interactive failure-reporting run completed 439 of 439 Vault tests in 548.200 seconds. An earlier interactive failure-only invocation exited 100 without identifying a failed test, so the final unchanged repository gate remains the acceptance authority.

- Observation: Slot reservation alone does not make a 30-second PTY watchdog deterministic after the complete broad partition has exercised the host.
  Evidence: the committed full gate still failed the same PTY test at 32.165 seconds, while individual KDF-heavy Vault tests in that run took up to 67.954 seconds. The timeout is a hang detector, not a product latency contract, so tying correctness to a tighter host-load threshold is structurally unsound.

- Observation: Doubling the watchdog shifts the failure one-for-one instead of allowing slow work to complete.
  Evidence: after focused validation passed with a 60-second watchdog, the complete gate failed the same test at 62.120 seconds. This disproves the slow-operation hypothesis and shows that the PTY child remains blocked for the entire watchdog only when embedded in the combined test workflow.

- Observation: Nextest's JSON `test-count` includes mismatching test cases from any partially selected binary, so it cannot directly prove partition coverage for a test-name filter.
  Evidence: counting each testcase whose `filter-match.status` is `matches` yields 2,163 broad tests, 437 non-PTY Vault tests, and 2 PTY tests, exactly matching the prior 2,602-test total. The dedicated serial PTY invocation then passed both tests in 4.410 seconds with the restored 30-second watchdog.

## Decision Log

- Decision: Treat changelog and append-only execution records as historical evidence, not as active host-support surface, while retaining tracked-path inventory across the entire repository.
  Rationale: Current source and current guidance must not advertise or implement an unsupported host, but released history must remain factually accurate. The tracked-path scan remains global so dormant platform-specific artifacts cannot hide in a historically exempt document category.
  Date/Author: 2026-08-23 / Codex

- Decision: Combine the tool execution result and progress flush result in one helper with a complete four-case outcome table.
  Rationale: An explicit composition boundary makes precedence reviewable and testable. The existing `progress::combine_progress_delivery` helper is now the shared authority for CLI and MCP progress instead of creating a divergent MCP-only abstraction. The primary operation failure is first when both fail; a flush failure still fails an otherwise successful call; either isolated failure remains unchanged.
  Date/Author: 2026-08-23 / Codex

- Decision: Rate-limit authoritative-result metadata inspection using monotonic time inside `WorkerProcessObserver`, while checking external cancellation first and retaining the unconditional post-exit read and size check.
  Rationale: A monotonic schedule decouples filesystem work from transcript activity without creating a second thread. Immediate pre-start inspection, prompt in-process overflow detection, external cancellation precedence, and final post-exit validation remain intact.
  Date/Author: 2026-08-23 / Codex

- Decision: Keep the three reviewed product invariants in three implementation commits, with separate planning and evidence commits.
  Rationale: Each product behavior can be reviewed, reverted, and bisected without mixing policy, transport errors, and process supervision. A separately discovered test-infrastructure defect remains outside those product commits.
  Date/Author: 2026-08-23 / Codex

- Decision: Run the `vault_tui` integration binary serially in its own final Nextest process and restore the 30-second watchdog; remove the narrower thread reservation.
  Rationale: The scenario repeatedly passes in about 3.3 seconds in a dedicated Nextest process and repeatedly blocks for exactly the configured watchdog inside the combined workflow. Neither reserving all in-process slots nor doubling the timeout changes that boundary-dependent behavior. A separate process is the demonstrated isolation unit, while serial execution prevents the two PTY tests from contending with each other.
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

Milestone two repairs progress error composition. Generalize `crates/jig/src/progress.rs::combine_progress_delivery` into the shared authority for CLI and MCP progress delivery, accepting `anyhow::Result<()>` plus a boundary-specific combined-failure label. Match all four success/failure combinations. If both fail, return one error whose displayed message begins with the primary operation failure and also reports the progress-delivery failure. In `crates/jig/src/mcp.rs`, change `handle_tool_call` to evaluate both outcomes and pass them through a thin boundary-specific wrapper around the shared helper. Add unit tests proving all four MCP outcomes and change the existing CLI dual-failure regression to require primary-error-first rendering. Run both MCP and CLI progress unit tests and commit this milestone alone.

Milestone three repairs result inspection cadence. In `crates/jig/src/runtime/worker_runner.rs`, add a 10ms inspection interval and a last-inspection `Instant` to `WorkerProcessObserver`. Replace the unconditional metadata call in `cancelled` with an `inspect_authoritative_output_if_due` helper. Check the execution cancellation source first so an already-cancelled command still yields `CancelledBeforeStart`; otherwise inspect immediately on the first callback and at most once per interval. Retain `read_worker_output_file` after process exit as the authoritative final validation. Add a deterministic unit test that controls the observer's private monotonic timestamp and proves a missing output file is ignored before the interval and detected when due. Keep the existing live-overflow and pre-start cancellation regressions passing. Commit this milestone alone.

Milestone four hardens full-suite scheduling without changing product behavior. In `.jig.toml`, keep the existing broad first partition, exclude the `jig-sh` `vault_tui` integration binary from the second Vault partition, and add a third `cargo nextest run -p jig-sh --test vault_tui -j 1` invocation. Apply the same three-part structure to `rust_test_locked_command`. Remove the now-redundant specific override from `.config/nextest.toml` and restore `crates/jig/tests/vault_tui.rs` to its 30-second per-transition watchdog. Validate that the selectors enumerate 2,163 broad tests, 437 Vault tests, and 2 serial PTY tests, then run the exact configured command and commit this process-isolation correction separately from the earlier experiments.

After all four milestones, format the workspace, rebuild the development binary, run the supported-host and crate-focused tests, run the configured contract gate, and run `JIG_DEV_BIN=target/debug/jig scripts/jig check test` as the full-suite acceptance command. Use `scripts/jig work check`, `work evidence`, `work gates`, and `work receipts` to connect the results to this plan. Update every living section of this plan, append a final structured-work progress record, finish the work only after required gates pass, and commit the plan and append-only evidence.

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
    cargo test -p jig-sh progress::tests
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

For the Vault process-isolation slice, edit with `apply_patch`, then run:

    cargo nextest list --workspace -E 'not (package(jig-vault) | package(jig-vault-tui) | (package(jig-sh) & (test(vault) | binary(/vault_.*/))))'
    cargo nextest list --workspace -E '(package(jig-vault) | package(jig-vault-tui) | (package(jig-sh) & (test(vault) | binary(/vault_.*/)))) & not (package(jig-sh) & binary(vault_tui))'
    cargo nextest list -p jig-sh --test vault_tui
    cargo nextest run -p jig-sh --test vault_tui -j 1
    git add .jig.toml .config/nextest.toml crates/jig/tests/vault_tui.rs .agent/plans/plan_01M0PRXMMJ4GK7RCM3ND180V2A.md .agent/state/plans.jsonl .agent/state/receipts.jsonl
    git commit -m "test(vault): isolate PTY suite process"

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

The progress repair is accepted when an MCP unit test supplies both a synthetic tool error and a synthetic progress error and observes both in the result with the tool error first. The complete four-outcome table must demonstrate that a progress error still fails a successful tool call and isolated tool failures remain unchanged. The existing CLI dual-failure regression must also show its operation error first, proving the shared helper carries the same contract across both delivery paths. Existing progress buffering and framing tests must remain green.

The worker repair is accepted when the schedule unit test proves metadata is not re-read before the monotonic deadline and is re-read once due, the existing cancelled-before-start test retains its typed outcome, and the live oversized-result test still terminates the worker process group before its escaped marker can be written.

The Vault scheduling repair is accepted when the configured selectors cover 2,163 broad tests, 437 non-PTY Vault tests, and both PTY tests exactly once; the PTY binary passes serially in its own process with the restored 30-second watchdog; and the complete three-part `scripts/jig check test` command exits zero.

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

No new third-party dependencies are required. The policy test continues to use `tempfile` and `std::process::Command`. `crates/jig/src/progress.rs` exposes a crate-private generic `combine_progress_delivery` authority that accepts a primary `anyhow::Result<T>`, any delivery `Result<(), E>` whose error converts into `anyhow::Error`, and a boundary-specific combined-failure label; MCP retains a thin private wrapper that supplies its label. The worker observer uses `std::time::Instant` and `Duration`; it retains the existing `OwnedProcessObserver` interface and `WorkerResultFileFailure` mapping. No command-line, JSON-RPC schema, durable-state schema, or public Rust API changes are introduced.

Plan revision note (2026-08-23 07:38Z): Replaced the one-line work-start body with a self-contained execution plan after inspecting the affected policy, MCP, and worker boundaries. The plan records the structural causes, fixes, separate commit strategy, and full-suite acceptance criteria so work can resume from this file alone.

Plan revision note (2026-08-23 07:42Z): Marked the planning commit complete and recorded partial progress on the policy milestone after narrowing the guard and adding its regression. Released changelog entries still need restoration before this slice is valid.

Plan revision note (2026-08-23 07:45Z): Marked the policy implementation and focused validation complete after restoring all 22 deleted historical entries. The five-test integration target and the real checker both pass; the slice is ready to commit.

Plan revision note (2026-08-23 07:48Z): Recorded the MCP composition helper and its four-outcome regression coverage. Focused validation remains before the second implementation commit.

Plan revision note (2026-08-23 07:49Z): Expanded the MCP milestone to reuse and correct the existing CLI progress result-composition authority after discovering it encoded the same structural bug. Updated the work, commands, acceptance criteria, and interface description; all 23 focused tests pass.

Plan revision note (2026-08-23 07:54Z): Recorded the worker observer's independent monotonic inspection schedule and deterministic regression. Focused cancellation and live-overflow validation remain before the third implementation commit.

Plan revision note (2026-08-23 07:55Z): Marked the worker scheduling milestone complete after all three focused regressions passed, and recorded why the 10ms interval preserves the prior idle-path behavior while bounding chatty-path filesystem work.

Plan revision note (2026-08-23 08:31Z): Added a fourth test-infrastructure milestone after two unchanged full-suite runs reproduced one 30-second Vault PTY timeout under group load, while two isolated executions passed in about 3.3 seconds. Chose resource isolation over timeout inflation and updated progress, discoveries, decisions, work, commands, and acceptance criteria.

Plan revision note (2026-08-23 08:48Z): Marked the Vault scheduling milestone complete after confirming override precedence, a 3.307-second exact test, and a 439-of-439 loaded partition pass. Recorded the inconclusive earlier failure-only invocation and retained the complete repository gate as final authority.

Plan revision note (2026-08-23 09:03Z): Added a follow-up to widen the isolated PTY watchdog after the committed full gate reproduced the 32-second failure and showed individual Vault KDF tests exceeding 67 seconds. Updated the discovery, rationale, work, and acceptance criteria to distinguish a bounded hang detector from a performance SLA.

Plan revision note (2026-08-23 09:16Z): Marked the watchdog follow-up complete after the exact PTY test and all 439 loaded Vault tests passed. Recorded the successful timings and retained the complete repository gate as the remaining acceptance step.

Plan revision note (2026-08-23 09:32Z): Rejected both the in-process thread reservation and timeout-inflation hypotheses after the complete gate failed at 62.120 seconds. Reworked the fourth milestone around the demonstrated process boundary: a dedicated serial `vault_tui` Nextest invocation with the original 30-second watchdog.

Plan revision note (2026-08-23 09:41Z): Marked the process-isolation correction ready to commit after validating exact partition membership through per-test filter-match statuses and passing both dedicated PTY tests serially with the restored watchdog.


Policy slice complete: separated active supported-host scanning from immutable release history, restored all 22 deleted historical entries, added the Unreleased breaking cutover, and passed scripts/check-supported-host-surface.sh plus 5/5 supported_host_surface tests.


Progress-delivery slice complete: centralized CLI and MCP outcome composition, kept the primary operation error first when delivery also fails, and passed 11 MCP plus 12 CLI progress unit tests.


Worker inspection slice complete: decoupled authoritative result metadata checks from transcript readiness with an immediate then 10ms monotonic cadence. The deterministic cadence test, cancelled-before-start regression, and live overflow/process-group cleanup regression all pass.


Verification infrastructure slice complete: the Vault PTY browser now reserves all four vault-crypto slots, its specific override wins, the exact test passes in 3.307s, and the loaded Vault partition passes 439/439 in 548.200s without changing the 30s interaction bounds.


Vault PTY watchdog follow-up complete: kept every terminal transition bounded while widening the host-load watchdog to 60s. The exact scenario passed in 3.371s and the loaded Vault partition passed 439/439 in 549.356s.


Vault process-isolation correction ready: restored the 30-second PTY watchdog, removed the ineffective in-process slot override, split vault_tui into a dedicated serial Nextest invocation, proved exact 2163/437/2 test coverage, and passed both PTY tests in 4.410 seconds.