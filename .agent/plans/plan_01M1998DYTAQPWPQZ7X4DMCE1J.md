# Close scheduled-loop review findings at their owning boundaries

This ExecPlan is a living document and must be maintained in accordance with `.agent/PLANS.md`. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current so another contributor can resume from this file alone.

## Purpose / Big Picture

Scheduled loop work must never lose a due occurrence before a worker starts, hide a Codex task's final answer behind provider diagnostics, or disagree about which occurrences require operator attention. Existing repositories must also fail clearly before creating task worktrees in an unignored path. After this work, focused regressions demonstrate those behaviors, the repository's configured gates pass through the freshly built Jig binary, and repeated comprehensive branch reviews report no actionable finding introduced by this branch.

## Progress

- [x] (2026-08-30 12:40Z) Research the prior review's open questions in repository documentation, plans, tests, contract-check implementation, and Git history.
- [x] (2026-08-30 12:45Z) Establish a green baseline: `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, and 122 `runtime::loops` unit tests pass.
- [x] (2026-08-30 12:47Z) Run the Fowler changed-code scanner and manually reject unsupported style-only candidates.
- [x] (2026-08-30 13:02Z) Add characterization tests for disposable-cache recovery and retryable pre-execution failures.
- [x] (2026-08-30 13:08Z) Refactor scheduled tick startup errors into a typed unexecuted boundary, then change dispatch finalization without changing the durable occurrence schema.
- [x] (2026-08-30 14:05Z) Make Codex result and provider-diagnostic channels explicit and persist the authoritative result.
- [x] (2026-08-30 14:20Z) Centralize occurrence-attention policy and make acknowledgement reconcile expired claims atomically.
- [x] (2026-08-30 14:45Z) Enforce the task-worktree ignore precondition, delegate observer flushing, and repair the PID readiness test cleanup path.
- [x] (2026-08-30 16:30Z) Run focused tests and the complete structured-work gates: contract, LOC, format, Clippy, 2,476 core tests, and 107 frontend tests pass; inspect fresh gate evidence.
- [x] (2026-08-30 16:45Z) Commit a clean branch snapshot and complete the first independent Claude/Codex branch review against fingerprint `37531ea1ccccac412ada5bc6821eb316a7bcbb198094934509a49922f35574e9`.
- [x] (2026-08-30 17:05Z) Research and adjudicate every first-round finding and open question against the branch history, public contract, Git command semantics, and existing cross-process coverage.
- [x] (2026-08-30 18:10Z) Repair the valid first-round lease, cancellation, retention, cache-recovery, status-degradation, marker-write, and renewal-diagnostic findings with focused regressions.
- [x] (2026-08-30 18:35Z) Split the enlarged occurrence and schedule regression modules after the first repeated gate run exposed the repository's 800-line Rust-file limit; 135 focused loop tests and workspace Clippy pass after the split.
- [x] (2026-08-30 18:50Z) Complete a fresh structured work check: contract, LOC, format, Clippy, 2,485 core tests, and 107 frontend tests pass under batch receipt `receipt_01M19HAKRWJYQN319QQ5JHKS9G`.
- [x] (2026-08-30 19:15Z) Complete the second independent Claude/Codex review against verified fingerprint `b8255ceffff53d8a96e0e8b73c60b141aca5f3ccd83230873717989dbc20c6eb` and research all three open questions before adjudicating findings.
- [x] (2026-08-30 20:00Z) Repair the valid second-round stale-claim, pre-worker phase, retained-cleanup, attention-growth, receipt-failure, and mixed-clock findings with 140 passing focused loop tests.
- [x] (2026-08-30) Complete the third independent Claude/Codex review against verified fingerprint `85e97f2d1431953c5c96da0ed8d838bc949edf2dae4425dbd34c942190af30ba`; record that the Claude adapter could inspect only the current changed files after its branch diff exceeded the adapter limit.
- [x] (2026-08-30) Research all four third-round open questions before adjudication, then repair the valid workflow-renewal, dirty-repository, and ambiguous-started-push findings with focused unit and scheduled-dispatch regressions.
- [x] (2026-08-30) Correct three repo-mode test fixtures exposed by the full core gate: ignore managed runtime/cache paths and keep test-only Codex executables and observation markers outside the repository whose cleanliness they assert.
- [x] (2026-08-30) Replace a fixed-delay authority-corruption test race with condition-based synchronization after a loaded core run allowed the queued target to start first; the focused regression passes three consecutive runs.
- [x] (2026-08-30) Complete fresh structured gates for the third repair: contract, LOC, format, Clippy, 2,495 core tests, and 107 frontend tests all pass under batch receipt `receipt_01M19SXVWVVFX71ZREYCCE16NF`; the final rebuilt-binary contract receipt is `receipt_01M19SZ5VMBQ6B2PK0MN7PN5CC`.
- [x] (2026-08-30) Complete the fourth independent Claude/Codex review against verified fingerprint `2cff7c3bc6977eda7b29ddce073b05de17d3a78c98822bf673d31c59cb0a8ed2`; record Claude's large-diff coverage limit and use Codex's complete branch-diff review as the full-scope counterpart.
- [x] (2026-08-30) Research all three fourth-round open questions before changing code, then repair typed occurrence ownership loss, durable manual/global attention, post-start PR cancellation, append-only receipt verification, workflow-ID path containment, and checkout error-chain reporting with focused regressions.
- [x] (2026-08-30) Complete fresh structured gates for the fourth repair: contract, LOC, format, Clippy, 2,507 core tests, and 107 frontend tests all pass under batch receipt `receipt_01M19YMRZ4EHGPBCVAVEQTGKB1`.
- [ ] Repeat full gates and fresh comprehensive Claude/Codex branch reviews until no actionable in-scope findings remain; record unrelated findings in Beads and exclude them from the loop.

## Surprises & Discoveries

- Observation: the four prior review questions have repository-defined answers and require no product input.
  Evidence: `docs/configuration.md` distinguishes retryable unexecuted lease deferral from ambiguous post-side-effect failure, documents deliberate operator-owned retention, and the root `AGENTS.md` plus prior plans require append-only `.agent/state/*.jsonl` dogfooding memory. `crates/jig/src/policy.rs::validate_contract` does not inspect managed `.gitignore` content.
- Observation: the reported multi-process claim-exclusivity test gap is already covered.
  Evidence: `crates/jig/tests/cli_json_parts/loop_commands.rs::concurrent_dispatchers_execute_one_due_occurrence_once` starts concurrent CLI dispatchers against one fixture.
- Observation: the Fowler scanner returned 200 truncated heuristic candidates across 59 changed files, but file length, test `unwrap`, DTO field counts, clone counts, and orchestration parameter counts do not establish maintenance defects here.
  Evidence: the accepted candidates are instead tied directly to the reviewed failures and repeated policy reconstruction across module boundaries.
- Observation: the old scheduler counted a tick as executed as soon as occurrence renewal started, before workflow resolution or execution-lease acquisition.
  Evidence: the pre-execution regression initially persisted a failed occurrence with `executed_count = 1`; the typed startup error now keeps that count at zero and abandons the claim.
- Observation: recovering malformed cache JSON inside every mutating store access hid corruption created after workflow execution.
  Evidence: three existing post-work state-error regressions failed under the first implementation. Recovery now runs once before durable occurrence claims; the post-work corruption regression and all 173 focused loop tests pass together.
- Observation: a branch-added one-second renewal test intermittently failed only under the heavily parallel full suite because it slept beyond the TTL and assumed timely thread scheduling.
  Evidence: two earlier full runs passed, a later run took 5.1 seconds and observed `NeedsAttention`, and the production renewal algorithm's deterministic tests remained green. Lease and occurrence integration tests now use a test-only fast interval, a 60-second safety TTL, and poll the persisted expiry extension; the subsequent 2,476-test core gate passed.
- Observation: a retained isolated task checkout is intentionally preserved for diagnosis, but scheduled `codex_task` occurrences have no attempt budget and can create one distinct retained checkout per cron instant.
  Evidence: `docs/configuration.md` limits attempt budgets to `pr_manager`; `codex_task` hashes the occurrence id into each checkout path; pruning deliberately preserves every record whose checkout still exists. Automatic deletion would risk user data, so the safe bound is to stop a workflow from claiming another occurrence until its retained checkout is removed.
- Observation: malformed lease JSON is not safely disposable while a worker may still own an unexpired lease.
  Evidence: cache recovery can erase the only coordination record, after which another dispatcher acquires the same key before the original renewal thread observes the loss. Attempt state can be reset with explicit evidence, but lease corruption must fail closed.
- Observation: the reported root-checkout isolation race is not supported by the invoked Git operations.
  Evidence: task worktree creation records `rev-parse HEAD` and passes that same immutable object id to `git worktree add`; concurrent root HEAD movement therefore cannot create the claimed recorded-HEAD mismatch. `git fetch` and `git worktree add` operate on refs/common worktree metadata and a separate worktree index, not the main worktree index.
- Observation: the first repeated structured gate run passed contract, format, Clippy, all 2,485 core tests, and all 107 frontend tests, but the new regressions pushed two existing test modules above the configured 800-line limit.
  Evidence: `occurrence/tests.rs` measured 835 lines and `schedule/tests.rs` measured 880 lines. Moving cohesive renewal diagnostics and review regressions into child modules reduced them to 787 and 629 lines without changing behavior; compilation, 135 focused loop tests, and workspace Clippy then passed.
- Observation: all second-round open questions were answerable without product input.
  Evidence: Git's documented branch-name validation forbids a leading dash, the loop operator guide names durable-ledger restoration and manual reconciliation, and occurrence ids are opaque ledger keys with no `@` parser. The first-dispatch behavior is also explicitly documented and remains intentional.
- Observation: refreshing a schedule snapshot before each workflow narrows but cannot close the overlapping-dispatch race.
  Evidence: another dispatcher can still terminalize a newer occurrence between the refresh and claim. The durable claim lock must reject newer chronology, unresolved attention, and retained-worktree evidence in the same transaction that inserts a claim.
- Observation: a boolean `unexecuted` flag could not distinguish cancellation from setup failure and allowed retained cleanup evidence to conflict with retry.
  Evidence: prompt/home/checkout failures and cancellation-receipt failures reached different error paths, while both could precede worker start. Replacing the flag with `WorkflowExecution::Unexecuted(UnexecutedReason)` makes the retry reason explicit; a retained path changes finalization to `needs_attention` instead of deleting the only durable evidence.
- Observation: all third-round open questions were answerable without product input.
  Evidence: Git documents that deleting a linked-worktree directory leaves administrative metadata for `git worktree prune`, and ordinary reuse can remain blocked until that metadata is removed; the cache ignore predates the PR worktree while the durable runtime ignore was added later; the valid-calendar cron scan starts at leap year 2000 and short-circuits rather than walking 400 years for the cited February 29 case; and the public loop guide explicitly classifies cancellation during post-work aggregation as a state error so completed evidence survives.
- Observation: the third-round symptoms are lifecycle design gaps, not three unrelated omitted conditionals.
  Evidence: workflow and occurrence renewers had separately evolved error policies, task orchestration owned checkout reporting and completion classification together, and a started push was flattened into the same error type as a pre-start failure. Shared renewal policy, extracted checkout completion reporting, and a phase-specific push error make the unsafe states representable once at their owning boundaries.
- Observation: repository dirtiness cannot include Jig's own worker receipt.
  Evidence: `run_codex_exec` appends `.agent/state/receipts.jsonl` before checkout finalization. The first broad loop run therefore classified an otherwise clean repo task as attention. The final check excludes exactly that runtime-owned receipt path for shared-repository tasks, while isolated worktrees still inspect every path and a regression proves all other untracked changes remain visible.
- Observation: three existing repo-mode tests encoded cleanliness while placing their own machinery inside the checkout or omitting a managed ignore.
  Evidence: the first full core gate after round three reported two `needs_attention` results. One fixture created a Codex stub and completion marker under the repository; another created its stub there; the shared fixture ignored `.agent/runtime/` but not the managed `.agent/.cache/`. Moving test machinery to a separate temporary directory and matching the generated ignore policy made both focused tests pass without weakening production dirtiness detection.
- Observation: a branch-added parallel authority test synchronized on elapsed time instead of the state whose ordering it asserted.
  Evidence: seven initial workers slept 500 ms after observing all start markers, while worker zero was merely eligible to corrupt the contract. Under loaded nextest scheduling, a sleeping worker finished and claimed the queued target before worker zero resumed. Making those workers wait until the contract actually contains the corruption makes pre-start authority failure deterministic; three consecutive focused runs pass.
- Observation: all fourth-round open questions were answerable from repository history, the original plan, and the public state-maintenance contract.
  Evidence: per-dispatch receipts are an intentional audit boundary with explicit archive/export tooling; `attempts_reset` deliberately permits work while keeping the invocation unsuccessful and auditable; and workflow IDs historically permit `/`, while only the branch-added raw PR-worktree join turned that accepted value into a path escape.
- Observation: the fourth-round findings cluster at lifecycle and authority boundaries rather than representing independent missing conditionals.
  Evidence: manual and scheduled ticks did not share one durable attention authority, occurrence renewal erased ownership-loss semantics into `anyhow`, PR cancellation flattened pre-start and post-start phases, and shared-checkout dirtiness excluded a path without first proving the excluded journal's exact append. Extracting manual occurrence lifecycle, runtime attention aggregation, PR outcome classification, and receipt-baseline verification gives each invariant one owner.
- Observation: the diagnostic `loop status` exit behavior is intentional and already protected by a CLI adapter test.
  Evidence: the public guide names tick, dispatch, and run as the commands that map `ok: false` to nonzero; `run_tests.rs` explicitly keeps status outside that classification. The documentation now states the distinction directly.

## Decision Log

- Decision: classify the review symptoms as three boundary-design problems plus three localized omissions.
  Rationale: generic startup errors erase proof that no workflow ran; raw output fields let callers confuse the authoritative last-message channel with provider stdout; and attention policy is reconstructed independently by status, dispatch, and acknowledgement. The PID race and missing observer delegation are local test/adapter omissions, while the ignore check is a missing upgrade precondition.
  Date/Author: 2026-08-30 / Codex
- Decision: keep `.agent/.cache/loop/` disposable and recover malformed JSON only during dispatch preflight.
  Rationale: the public contract and prior design decisions distinguish disposable leases/attempts from the fsynced occurrence ledger. Preflight recovery under the cache locks is safe before a durable occurrence claim; ordinary accesses remain strict so corruption introduced after work is reported rather than hidden. Durable schedule state continues to fail closed.
  Date/Author: 2026-08-30 / Codex
- Decision: retry only errors that the typed execution boundary proves occurred before `run_workflow_tick` started.
  Rationale: ambiguity after worker execution remains terminal `failed` or `needs_attention`; resolving a workflow, acquiring its execution lease, or starting lease renewal has no workflow side effect and must abandon the occurrence so the due instant can be claimed again.
  Date/Author: 2026-08-30 / Codex
- Decision: preserve all persistent schema versions and existing status strings.
  Rationale: the schedule ledger and schema-version-1 CLI/receipt JSON are compatibility boundaries. New diagnostics may be additive, but the fixes do not require a migration or public Rust API change.
  Date/Author: 2026-08-30 / Codex
- Decision: require exact, unexpired workflow-lease ownership during `LeaseGuard` finalization.
  Rationale: renewal cancellation alone cannot prove the lease stayed live while a paused process resumed; a strict transition under the lease lock prevents a stale owner from reporting clean success after expiry and reacquisition.
  Date/Author: 2026-08-30 / Codex
- Decision: carry an explicit unexecuted disposition through `WorkflowCompletion` for worker cancellation before process start.
  Rationale: a cancellation bit is execution-phase evidence, not an ordinary failed outcome. Scheduled dispatch can then abandon the occurrence and retry it, while cancellation after spawn remains terminal because side effects are ambiguous.
  Date/Author: 2026-08-30 / Codex
- Decision: block a scheduled isolated task while any retained checkout for that workflow still exists.
  Rationale: deleting failed or dirty evidence automatically is unsafe, while continuing to create unique checkouts is unbounded. Backpressure preserves the existing manual cleanup contract and bounds retained checkout growth to one per workflow.
  Date/Author: 2026-08-30 / Codex
- Decision: validate lease-cache parseability and fail closed, but recover malformed attempt state with structured dispatch evidence.
  Rationale: attempt counters affect retry policy but do not confer mutual exclusion; leases are live safety claims. Treating both files as equally disposable creates an overlap window.
  Date/Author: 2026-08-30 / Codex
- Decision: enforce schedule chronology, unresolved-attention backpressure, and retained-worktree backpressure inside the occurrence claim transaction.
  Rationale: per-dispatch and even per-workflow snapshots are observations, not synchronization. One locked precondition prevents stale runs and bounds ambiguous evidence to one unresolved occurrence per workflow without deleting or collapsing operator evidence.
  Date/Author: 2026-08-30 / Codex
- Decision: replace the workflow's unexecuted boolean with a typed execution disposition and preserve process-start facts through worker receipt errors.
  Rationale: cancellation and setup failures share retry policy but require distinct evidence, while a retained checkout makes automatic retry unsafe. The typed disposition prevents invalid flag/reason combinations and lets finalization keep the occurrence discoverable when cleanup fails.
  Date/Author: 2026-08-30 / Codex
- Decision: use one bounded renewal policy for workflow leases and occurrence claims, with typed ownership loss as an immediately terminal cause.
  Rationale: filesystem and cache failures may recover while TTL headroom remains; treating every error as ownership loss cancels healthy work, while retrying actual ownership loss delays the only safe response. A shared runner also prevents the two renewal loops from drifting again.
  Date/Author: 2026-08-30 / Codex
- Decision: make a dirty or unverifiable shared repository checkout a durable scheduled occurrence requiring acknowledgement.
  Rationale: the repository root cannot be retained as a diagnostic linked worktree, and automatically running another occurrence compounds unknown state. Backpressure is the same safety contract already used for retained isolated worktrees and other ambiguous occurrences. The runtime-generated worker receipt is excluded exactly because it is evidence about the task, not a task-authored change.
  Date/Author: 2026-08-30 / Codex
- Decision: preserve an unconfirmed started PR push as `needs_attention` with the worker receipt, candidate SHA, and worktree.
  Rationale: a nonzero, cancelled, or transport-failed process that started may still have changed the remote. Converting it to an ordinary failed attempt discards the side-effect phase and allows unsafe recurrence; the typed `PrPushError` prevents that collapse.
  Date/Author: 2026-08-30 / Codex
- Decision: publish a transient durable occurrence for manual ticks once the execution lease is held, and remove it only for an unambiguous clean result.
  Rationale: manual execution can retain the same worktree and ambiguous side effects as scheduled execution. Using the durable occurrence store makes status, UI, acknowledgement, and backpressure share one authority; filtering its sentinel schedule instant from chronology preserves the existing cron identity contract.
  Date/Author: 2026-08-30 / Codex
- Decision: verify the worker receipt journal as an exact append before excluding it from repo-task dirtiness.
  Rationale: excluding the whole path hid worker truncation or rewrite. Capturing the original byte prefix and Git index entry, then validating complete appended JSON including the expected receipt, distinguishes Jig-owned evidence writes from task-authored mutation without broadening the dirtiness exception.
  Date/Author: 2026-08-30 / Codex
- Decision: keep the established workflow-ID grammar and encode IDs at the new PR-worktree filesystem sink.
  Rationale: tightening a long-standing identifier contract would be an unnecessary compatibility break. A fixed digest component contains absolute and parent traversal syntax while remaining deterministic and collision-resistant.
  Date/Author: 2026-08-30 / Codex

## Outcomes & Retrospective

The initial implementation slices and four review repair rounds are complete. The focused runtime-loop unit suite now passes 150 tests. Round-one regressions prove strict lease ownership at finalization, retryable cancellation before workflow and worker start, fail-closed lease corruption, auditable attempt recovery, retained-worktree backpressure, per-workflow status degradation, unchanged-marker no-op writes, and first-error renewal diagnostics. Round-two regressions add transactional stale-claim rejection, attention backpressure, typed setup retry, cancellation receipt phase preservation, retained cleanup evidence, and one-clock status classification. Round three adds transient workflow-lease retry, dirty shared-checkout backpressure, worker-receipt self-exclusion, and durable ambiguity for a started but unreconciled PR push. Round four adds immediate typed occurrence-ownership loss, durable manual and machine-global attention, post-start PR cancellation evidence, authenticated receipt appends, path-contained PR worktrees, and full checkout error chains. The fourth-round focused unit and end-to-end regressions pass, and fresh structured gates pass contract, Rust file-size policy, formatting, workspace Clippy with warnings denied, 2,507 core tests, and 107 frontend tests. Another comprehensive branch review remains.

## Context and Orientation

`crates/jig/src/runtime/loops/state.rs` owns disposable JSON leases and attempts under `.agent/.cache/loop/`. `crates/jig/src/runtime/loops/occurrence.rs` and its `persistence.rs` child own the durable schedule ledger and occurrence state transitions under `.agent/runtime/loop/`. `crates/jig/src/runtime/loops/engine.rs` resolves a workflow, acquires its execution lease, runs one tick, and produces `ScheduledTick`; `crates/jig/src/runtime/loops/schedule.rs` claims a durable occurrence and decides whether to abandon or terminalize it.

`crates/jig/src/runtime/worker_runner.rs` runs Codex. Codex provider stdout is a diagnostic transcript, while the file passed with `-o` is the authoritative result. The runner currently places that result in `std::process::Output.stdout`, but `codex_task.rs` and worker receipts persist provider stdout instead. `crates/jig/src/runtime/loops/codex_task.rs` also creates detached task worktrees below `.agent/runtime/loop/worktrees/tasks`.

`crates/jig/src/runtime/loops/engine.rs`, `schedule/attention.rs`, and `occurrence.rs` each currently reconstruct part of the attention policy. An expired `Running` record is actionable according to status, but acknowledgement only accepts a record already persisted as `NeedsAttention`. `crates/jig/src/execution.rs::AdditionalCancellationControl` is an observer/cancellation decorator that forwards events and cancellation but currently inherits the no-op `flush`. The frontend install-locking regression lives in `crates/jig/src/bootstrap/tests/frontend_adoption/install_locking.rs`.

The repository uses Rust edition 2024 with Rust 1.88 as its minimum supported version. All changes remain crate-internal. There is no unsafe or async code in this scope, but file locking, renewal threads, child-process cleanup, persisted JSON, and CLI evidence are concurrency and compatibility-sensitive. Generated and vendored code, unrelated crates, and scanner-only style candidates are excluded.

## Plan of Work

First add failing characterization tests. In `state.rs`, create tests showing that truncated `leases.json` and `attempts.json` no longer wedge their next mutating operation. In the schedule tests, inject failures before workflow execution and assert that the claimed occurrence is abandoned and the same due instant can be claimed on a later dispatch. Keep the existing tests proving that errors after workflow execution remain terminal.

Then apply Fowler's **Split Phase** and Rust's typed-error form. Introduce a crate-private scheduled-tick startup error whose type means that `run_workflow_tick` did not start. Make manual tick adapt that error back to `anyhow::Error`, while scheduled dispatch handles it by abandoning the occurrence and returning a failed, retryable dispatch action. Use the same unexecuted policy when `OccurrenceGuard::start` fails. Separately add a dispatch preflight that may recover JSON parse failures in disposable caches to their default values under the existing locks, but continues propagating I/O errors. Ordinary store access remains strict so post-work corruption is observable, and the durable schedule reader remains unchanged.

Next apply **Encapsulate Record** and **Move Function** in `worker_runner.rs`. Make `CodexExecOutput` expose named methods for process status, authoritative result bytes, and provider diagnostics rather than public same-shaped fields. Record the authoritative result as the worker receipt's stdout preview and expose a separately labeled, bounded provider diagnostic preview plus its truncation fact in evidence. In `codex_task.rs`, persist bounded authoritative result text as `output`; keep provider diagnostics under an additive, explicit name. Add a regression in `worker_runner/tests.rs` and a codex-task test where provider stdout and the `-o` result differ.

Next apply **Move Function** and **Consolidate Conditional Expression** to attention policy. Add one `ScheduleOccurrence` predicate for actionability at a supplied timestamp and use it from loop status and dispatch attention aggregation. In `OccurrenceStore::acknowledge`, take one timestamp under the ledger lock, convert an expired owner-independent `Running` record to the existing `NeedsAttention` shape, and acknowledge it in the same critical section. Live claims remain rejected. Add unit, integration, and CLI JSON tests for status followed directly by acknowledgement, and add a deterministic dispatch-attention test for an expired running record.

Finally make the three local boundary repairs. Before creating a task worktree, run the existing supervised Git command boundary with `git check-ignore` for the final target and return a clear `scripts/jig update --recopy` repair hint if the path is not ignored. Add an upgrade-style fixture that lacks the new managed ignore rule. Delegate `ExecutionObserver::flush` through `AdditionalCancellationControl` and test event, flush, base cancellation, and additional cancellation. Restore content-aware PID-file polling as a result-returning helper and always kill and reap the paused test child before asserting on the observed PID.

After each behavior slice, run its narrow tests and formatting. Build `target/debug/jig`, set `JIG_DEV_BIN=target/debug/jig`, and run the configured format, Clippy, contract, test, and structured-work gates. Review fixture names before receipt-producing commands. Commit the completed branch state locally so branch-scope review has a clean checkout. Run the comprehensive-review skill with default Claude and Codex reviewers against the pinned original branch base. Research every new review question first. Fix only findings whose root cause is introduced or materially changed by this branch. Create a Bead for every actionable unrelated finding, run `br sync --flush-only`, and exclude it from subsequent rounds. Repeat until the completed same-scope review has no actionable in-scope findings.

## Concrete Steps

All commands run from `/home/aa/.herdr/worktrees/jig-sh/feat-vault-tui`.

1. Run focused characterization tests while editing:

       cargo test -p jig-sh --lib runtime::loops
       cargo test -p jig-sh --lib execution::tests
       cargo test -p jig-sh --lib bootstrap::tests::frontend_adoption::install_locking
       cargo test -p jig-sh --test cli_json loop_

   Each newly added regression must fail for the reviewed behavior before its fix and pass afterward. The existing concurrent-dispatcher test remains the multi-process exclusivity proof.

2. After every slice, run:

       cargo fmt --all -- --check
       cargo check -p jig-sh --all-targets --locked

3. Before repository gates, build the current runtime and force the launcher to use it:

       cargo build -p jig-sh --bin jig
       export JIG_DEV_BIN=target/debug/jig
       scripts/jig work check --plan-id plan_01M1998DYTAQPWPQZ7X4DMCE1J
       scripts/jig work gates --plan-id plan_01M1998DYTAQPWPQZ7X4DMCE1J
       scripts/jig work evidence --plan-id plan_01M1998DYTAQPWPQZ7X4DMCE1J
       scripts/jig work receipts --plan-id plan_01M1998DYTAQPWPQZ7X4DMCE1J

4. Run the project-defined checks explicitly if a structured gate reports not-applicable or cannot attest the checkout:

       JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
       JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract
       JIG_DEV_BIN=target/debug/jig scripts/jig check test

5. Review and commit the diff without pushing, confirm `git status --short` is empty, then run the comprehensive branch review against base `2c0496fade6d9d9d8a24dadced92ed875689bfbb`. Repeat only for in-scope findings.

## Validation and Acceptance

Acceptance requires all of the following observable proofs. A truncated disposable cache is repaired during dispatch preflight and does not consume a scheduled instant. A startup error before `run_workflow_tick` leaves no recorded occurrence and a later dispatch can claim the same instant. A post-execution failure remains terminal. A Codex task action and worker receipt expose the authoritative last message even when provider stdout contains different diagnostics. `loop status` followed directly by `loop acknowledge-occurrence` succeeds for an expired claim without dispatching other work. Dispatch and status apply the same attention predicate. A stale adopted repository receives a clear update instruction before any task worktree is created. The observer wrapper forwards flush errors. The install-lock test cannot read an empty PID file or leak its paused child on assertion failure.

Formatting, compilation, Clippy, contract, all configured tests, and structured-work gates must pass. The final branch checkout must be clean, the scope fingerprint must be verified, and the last completed comprehensive review must report no actionable finding introduced by this branch.

## Idempotence and Recovery

All tests use generic temporary repositories and can be rerun. Disposable cache recovery is intentionally limited to JSON parse corruption under the cache lock; permission, cancellation, and other I/O failures remain visible. Durable occurrence state is never reset. If an implementation step cannot be proven, stop at the preceding green state and record the failed approach here. Do not rewrite append-only `.agent/state/*.jsonl`; Jig commands may append normal plan, session, and receipt records.

The review loop never fixes unrelated findings. Use `br create --title=... --type=bug --priority=<0-4> --json`, then `br sync --flush-only`, and record the Bead id in this plan without repeating private identifiers. No review command may push or modify remote state.

## Artifacts and Notes

Baseline evidence:

    cargo fmt --all -- --check                         # passed
    cargo check --workspace --all-targets --locked    # passed
    cargo test -p jig-sh --lib runtime::loops         # 122 passed
    Fowler changed-code scanner                       # 200 candidates, truncated; 59 files; no read errors

Focused implementation evidence:

    cargo test -p jig-sh --lib runtime::loops         # 135 passed
    cargo test -p jig-sh --lib runtime::tests::loops  # 47 passed
    cargo clippy --workspace --all-targets --locked -- -D warnings # passed
    generated_web_checks_recover_interrupted...       # passed
    occurrence_reported_as_attention...               # passed
    cargo test -p jig-sh --lib runtime::loops         # 140 passed after round two
    cargo test -p jig-sh --lib runtime::worker_runner::tests # 12 passed after round two
    cargo test -p jig-sh --lib runtime::tests::loops  # 47 passed after round two
    cancelled_before_start_keeps_its_phase...         # passed
    cargo check -p jig-sh --all-targets --locked      # passed after phase extractions
    cargo clippy --workspace --all-targets --locked -- -D warnings # passed after round two
    scripts/jig check rust-file-loc --changed-against 73075be... # passed
    cargo test -p jig-sh --lib runtime::loops         # 142 passed after round three
    scheduled_repo_task_blocks_after_leaving_the_shared_checkout_dirty # passed
    scheduled_pr_manager_preserves_unconfirmed_push_as_attention       # passed
    push_execution_error_distinguishes_started_and_unstarted_failures   # passed
    scheduled_worker_observes_its_published_running_claim_before_start  # passed after fixture repair
    scheduled_repo_checkouts_are_serialized_and_not_reported_as_worktrees # passed after fixture repair
    parallel_target_that_fails_authority_before_start_keeps_specific_receipt_evidence # 3 consecutive passes
    cargo test -p jig-sh --lib runtime::loops         # 150 passed after round four
    retained_manual_task_persists_and_blocks_manual_and_scheduled_reentry # passed
    tick_and_run_report_attention_owned_by_another_workflow              # passed
    scheduled_pr_manager_preserves_post_start_cancellation_as_attention   # passed
    scheduled_repo_task_detects_receipt_history_rewrites                  # passed
    receipt_01M19YMRZ4EHGPBCVAVEQTGKB1               # round-four repair work check passed
    jig.source_core_test                              # 2,507 passed
    jig.source_frontend_test                          # 107 passed
    jig.contract_check / rust_file_loc / fmt / clippy # fresh and passed

Final structured evidence:

    receipt_01M19DHVW36SQT9YZDTHFEHE8W               # work check passed
    jig.source_core_test                              # 2,476 passed
    jig.source_frontend_test                          # 107 passed
    jig.contract_check / rust_file_loc / fmt / clippy # passed
    receipt_01M19HAKRWJYQN319QQ5JHKS9G               # post-review work check passed
    jig.source_core_test                              # 2,485 passed
    jig.source_frontend_test                          # 107 passed
    jig.contract_check / rust_file_loc / fmt / clippy # fresh and passed
    receipt_01M19N04DG2QCJ2N6QG64AMC21               # round-two repair work check passed
    jig.source_core_test                              # 2,491 passed
    jig.source_frontend_test                          # 107 passed
    jig.contract_check / rust_file_loc / fmt / clippy # fresh and passed

## Interfaces and Dependencies

Keep all new types and methods crate-private. Do not add dependencies. `ScheduledTick` remains the representation of a tick that reached workflow execution or later reporting; a new typed startup error represents the mutually exclusive unexecuted path. `ScheduleOccurrence` owns the shared attention predicate, and `OccurrenceStore` owns atomic reconciliation plus acknowledgement. `CodexExecOutput` owns the distinction between process status, authoritative result bytes, and provider diagnostics. `AdditionalCancellationControl` must implement the entire `ExecutionObserver` contract by forwarding both `event` and `flush`.

The persisted schedule schema stays version 3. Existing schema-version-1 loop and worker evidence fields remain readable; new provider-diagnostic fields are additive. Linux and macOS behavior, Rust 1.88, edition 2024, child-process supervision, cancellation timing, and receipt truncation limits must remain unchanged.

Plan revision note (2026-08-30): replaced the initial one-line work note with a self-contained ExecPlan after resolving the review questions and establishing the green baseline. The plan separates Fowler refactoring steps from behavior fixes and defines the bounded comprehensive-review loop.

Plan revision note (2026-08-30): narrowed disposable-cache recovery from every mutating access to the pre-claim dispatch phase after existing post-work state-error tests exposed the difference.

Plan revision note (2026-08-30): after the second review, extracted pre-execution failure construction, transactional occurrence claiming, and unexecuted tick errors into focused child modules. This keeps the typed phase boundaries explicit and all changed Rust files within the repository's absolute line-count limit without changing behavior.

Plan revision note (2026-08-30): the first post-round-two structured check exposed a direct-CLI adapter regression: a worker start failure had already produced a diagnostic receipt and structured failed tick, but `into_manual_result` discarded it because execution never began. Preserve the structured result when a worker receipt exists while continuing to return raw configuration/setup errors before any worker receipt is available.

Plan revision note (2026-08-30): after the third review, extracted checkout completion reporting and the shared renewal policy, added typed ambiguous-push evidence, and narrowed repository dirtiness to task-authored changes by excluding only Jig's own worker receipt. This preserves the repository's LOC boundary and prevents the safety fix from backpressuring every otherwise-clean repo-mode task.

Plan revision note (2026-08-30): after the fourth review, extracted manual occurrence lifecycle, one-clock runtime attention aggregation, and PR outcome classification. The receipt-path exception now has an independent append-only proof, and accepted workflow IDs are encoded at the PR-worktree sink. These Fowler-sized boundaries keep every changed Rust file under the repository's 800-line limit while centralizing the new safety invariants.
