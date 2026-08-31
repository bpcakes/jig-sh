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
- [x] (2026-08-30 18:40Z) Complete the fifth independent Claude/Codex review against verified fingerprint `65da2db28d1ab8d7e77264a192cae98f5137cbbebdb427fb5fad085ba91a021e`; research all three open questions before adjudicating any finding.
- [x] (2026-08-30 18:42Z) Exclude the pre-existing exhausted-attempt UI field mismatch from the branch loop and record it as Bead `jig-sh-z9h`.
- [x] (2026-08-30 18:52Z) Repair fifth-round shared-checkout authority, manual blocked-result, claim-policy coupling, immutable PR-head, and incomplete-GitHub-observation findings with focused regressions.
- [x] (2026-08-30 19:29Z) Correct the loaded-suite timing oracle exposed by the first structured run, then complete fresh gates: contract, LOC, format, Clippy, 2,514 core tests, and 107 frontend tests pass under batch receipt `receipt_01M1A28NETM5VCKTH3SEAHBP9W`; rebuilt-binary contract receipt `receipt_01M1A2BDG0X0KH5F506ATF1KSR` makes all eight gates fresh.
- [x] (2026-08-30) Complete the sixth independent Claude/Codex review, research its authority and receipt-boundary questions, repair the valid admission-gap, unknown-scope, retained-path, and receipt-serialization findings, and pass fresh structured gates under batch receipt `receipt_01M1A5PHAWXJNVWYJXGNV6195S`.
- [x] (2026-08-30) Complete the seventh independent Claude/Codex review against verified fingerprint `b9a821e2db7bd6dd7fbea44407fa381fe0bf8bd7a553ff1de898641f802d270b` and research every question before editing: nested Jig commands are expected, retained worktree backpressure survives acknowledgement by design, schema-4 requires an operator cutover, and no alternate PR branch authority exists after lease loss.
- [x] (2026-08-30) Refactor the valid seventh-round findings at their owning boundaries and pass focused receipt, checkout, PR-manager, attempt-state, retained-worktree, timeout, and real nested-Jig regressions.
- [x] (2026-08-30) Complete fresh seventh-repair structured gates: contract, LOC, format, Clippy, 2,526 core tests, and 107 frontend tests pass under batch receipt `receipt_01M1A8Q8SCM3VD8ETV7V74BR06`.
- [x] (2026-08-30) Complete the eighth independent Claude/Codex review against verified fingerprint `e55268e0a95b5e8c313298163f7d76e72517049a621d64cad5dd7f35fd17ab2`; record Claude's large-diff coverage limit and use Codex's complete branch review as its counterpart.
- [x] (2026-08-30) Research every eighth-round question before editing: stale repo-mode adoption requires recopy, receipt append does not mutate session history, bounded manual-record pruning is intentional, locked reads intentionally publish the downgrade marker, and PR worktree lifecycle was introduced by this branch.
- [x] (2026-08-30) Repair eighth-round tick-consumption, repo-ignore-upgrade, and PR-worktree-lifecycle findings with focused regressions covering multiple candidates, cleanup success and failure, cancellation, retry, ambiguity, and stale adopted repositories.
- [x] (2026-08-31) Complete fresh eighth-repair structured gates: contract, LOC, format, Clippy, 2,529 core tests, and 107 frontend tests pass under batch receipt `receipt_01M1ABBCB13WAC64TEGAS4NE2D`; all eight configured gates are fresh or explicitly not applicable.
- [x] (2026-08-31) Complete rounds nine through eleven and their fresh structured gates; exclude the pre-existing non-UTF8 Codex probe flake as Bead `jig-sh-s0j` and retain the verified round-eleven source snapshot at `edbd9b8`.
- [x] (2026-08-31) Complete the twelfth independent Claude/Codex review against verified fingerprint `fb6927038e6ccf6f62389f0dd707585fc971644e61f0a0478f78c871b0e82fe2` and research every question before editing: cron's 400-year bound and first-dispatch semantics are deliberate, the exhausted-attempt UI mismatch is already `jig-sh-z9h`, and GitHub snapshot fanout predates the branch and is tracked as `jig-sh-kzq`.
- [x] (2026-08-31) Refactor the valid twelfth-round findings around protected schedule authority, pre-release worktree preparation cleanup, shared abandonment accounting, and file-backed GitHub review replies; add adversarial forgery, migration, symlink, cancellation, counter, and process-argument regressions.
- [x] (2026-08-31) Complete fresh twelfth-repair gates: contract, LOC, format, Clippy, 2,558 core tests, and 107 frontend tests pass under batch receipt `receipt_01M1AVD344BHYVBVV6QW2NBY2R`; the vault and process partitions are explicitly not applicable.
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
- Observation: all fifth-round open questions were answerable without product input, and one reported UI defect predates the branch.
  Evidence: pruning retains recent scheduled chronology and excludes manual sentinels, the first-dispatch rule is explicit in `docs/configuration.md`, post-work state errors intentionally make dispatch unsuccessful while preserving its receipt, and Git history places the mismatched `ExhaustedAttemptView` fields in pre-branch commit `10bfe619`. Bead `jig-sh-z9h` tracks that unrelated defect.
- Observation: occurrence-renewal errors after a successful unexecuted abandonment must not be reintroduced as state errors.
  Evidence: abandonment removes the owned record before the renewal thread joins, so its terminal ownership-loss diagnostic is expected. `renewal_error_does_not_turn_a_persisted_abandonment_into_ambiguous_attention` protects this distinction; surfacing that diagnostic would convert a safe retry into false failure evidence.
- Observation: the valid fifth-round symptoms expose three missing authority boundaries plus two local result-model omissions.
  Evidence: shared-root work lacked a durable cross-workflow scope, PR checkout creation consumed the main worktree's mutable `FETCH_HEAD`, and PR-manager policy consumed snapshots whose producers explicitly marked them truncated. Manual claim conflicts were represented as exceptions despite a successfully written receipt, while independent claim predicates were accidentally nested under one boolean.
- Observation: the first fifth-repair core gate exposed a branch-owned test-oracle omission, not a process-cleanup defect.
  Evidence: the failing run lasted 3.49 seconds while the background marker writer woke after only one second, so loaded scheduling could create the marker before overflow detection despite correct later group cleanup. The regression now requires overflow cancellation before three seconds but delays the leak write until four seconds; it passes in isolation and in the 2,514-test parallel core partition.
- Observation: native gate evidence becomes stale when the dev binary is rebuilt from a different source identity after a batch begins.
  Evidence: `gate_signature_with_native_identity` hashes `JIG_BUILD_IDENTITY` for native tools. The final test edit followed the initial binary build, and Clippy produced a binary with the new identity while the in-flight work check retained the old one. Rebuilding once from the final tree and refreshing only `jig-contract` restored all eight gates to fresh without rerunning already-fresh test receipts.
- Observation: seventh-round questions confirmed that the long-held receipt lock and flattened termination outcomes were design defects, while retained-worktree retry failure, post-abandonment renewal suppression, and the schema-4 operator cutover are intentional contracts.
  Evidence: repository guidance explicitly expects workers to use `scripts/jig`; the retained-worktree admission regression remains blocked after attention acknowledgement; abandonment removes the durable claim before joining its renewer; and `docs/configuration.md` already requires stopping older dispatchers before schema 4 is written.
- Observation: the reported multi-process dispatch gap remains already covered, but nested receipt-producing Jig execution was not.
  Evidence: `concurrent_dispatchers_execute_one_due_occurrence_once` exercises competing processes, while the new `repo_worker_can_run_a_nested_receipt_writing_jig_command` runs a real nested `jig bootstrap --json` and proves both inner and outer receipts complete without deadlock.
- Observation: PR snapshot lag exposes an attempt-identity modeling gap rather than a retry conditional omission.
  Evidence: one repair observes the source head and may push a distinct result head before GitHub reflects it. Persisting both ends of that transition lets either snapshot identify the same attempt generation while an unrelated head still resets stale state.
- Observation: eighth-round `needs_attention` symptoms expose a status-model design gap, while the missing repo-mode ignore check is a local upgrade omission.
  Evidence: passive attempt exhaustion and post-side-effect ambiguity shared one status but require opposite tick-consumption policy. Distinguishing them by their existing `attention_kind` prevents a side-effectful first PR from being discarded when a later candidate is scanned. The generated `.agent/runtime/` ignore rule already existed; repo mode simply failed to validate that managed precondition before its own ledger made the checkout dirty.
- Observation: PR repair worktrees had creation and reuse ownership but no terminal lifecycle owner.
  Evidence: successful and retryable outcomes left one checkout per PR indefinitely. Finalizing the branch lease before deciding cleanup allows unambiguous outcomes to remove their worktree while ambiguous lease, push, cancellation, or cleanup states retain the exact diagnostic checkout and consume the tick.
- Observation: the twelfth-round symptoms split into one authority-design defect and three phase-local omissions.
  Evidence: the worker-writable schedule ledger combined authoritative state, its serialization lock, and its diagnostic representation, so valid forged state could erase occurrence history. Moving the ledger and lock into worktree-specific Git metadata makes the checkout file a replaceable replica. In contrast, missing post-add cleanup, two missing `skipped_count` assignments, and a large reply encoded as one argument were local lifecycle/accounting/transport omissions; shared helpers and boundary tests close those surfaces without a schema change.

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
- Decision: make worktree-specific Git metadata the schedule source of truth for Git repositories and treat checkout-local schedule JSON as a compatibility replica.
  Rationale: an in-repository worker must not be able to replace occurrence history or the lock that serializes it. Keeping the protected authority worktree-specific preserves linked-worktree isolation, while migration from the prior protected witness preserves already-published schema-4 state. Public replica directories reject symlink redirection, and malformed replica contents are replaced from protected state.
  Date/Author: 2026-08-31 / Codex
- Decision: carry an explicit unexecuted disposition through `WorkflowCompletion` for worker cancellation before process start.
  Rationale: a cancellation bit is execution-phase evidence, not an ordinary failed outcome. Scheduled dispatch can then abandon the occurrence and retry it, while cancellation after spawn remains terminal because side effects are ambiguous.
  Date/Author: 2026-08-30 / Codex
- Decision: block a scheduled isolated task while any retained checkout for that workflow still exists.
  Rationale: deleting failed or dirty evidence automatically is unsafe, while continuing to create unique checkouts is unbounded. Backpressure preserves the existing manual cleanup contract and bounds retained checkout growth to one per workflow.
  Date/Author: 2026-08-30 / Codex
- Decision: validate lease-cache parseability and fail closed, but recover malformed attempt state with structured dispatch evidence.
  Rationale: attempt counters affect retry policy but do not confer mutual exclusion; leases are live safety claims. Treating both files as equally disposable creates an overlap window.
  Date/Author: 2026-08-30 / Codex
- Decision: make worktree-specific Git metadata authoritative for Git-backed lease and attempt state, with a typed first-mutation migration barrier at the legacy cache path.
  Rationale: repo-mode workspace-write is an intentional feature, so coordination state inside that writable checkout lets the worker cancel leases or forge retry budgets. Reusing the schedule authority resolver prevents Git-boundary drift; a migration marker containing recovery state blocks older readers without losing an in-flight cache generation if protected publication fails. Non-Git fixtures keep the no-follow cache implementation.
  Date/Author: 2026-08-31 / Codex
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
- Decision: persist whether an occurrence uses the shared repository checkout and make attention scope an enum owned by the claim transaction.
  Rationale: workflow-local attention is sufficient for isolated work, but repo-mode ambiguity is a property of the shared mutable root and must block every repo-mode workflow. A durable marker survives configuration changes, and an enum prevents attention, chronology, and retained-worktree predicates from becoming accidentally coupled again.
  Date/Author: 2026-08-30 / Codex
- Decision: represent a blocked manual claim as a receipted unexecuted disposition rather than an engine error.
  Rationale: no worker starts, but the command has completed a meaningful audited observation. A distinct `BlockedByAttention` disposition preserves that fact without weakening genuine startup-error handling.
  Date/Author: 2026-08-30 / Codex
- Decision: pin PR worktrees to the validated object ID from the GitHub snapshot and reject incomplete snapshots before policy evaluation.
  Rationale: content-addressed commit IDs cannot be redirected by concurrent fetches, while truncation flags prove only that the observation is incomplete. Failing before attempt cleanup or branch mutation keeps both authorities conservative at one boundary.
  Date/Author: 2026-08-30 / Codex
- Decision: split shared-checkout receipt validation into short baseline and verification phases, with no journal lock held across worker execution.
  Rationale: a worker is expected to invoke nested Jig commands that write durable receipts. Holding a non-reentrant repository lock across an external process deadlocks that supported composition. Final verification still preserves the pre-worker prefix and index entry, bounds the suffix, validates every appended record, and requires exactly one runtime-generated worker receipt ID.
  Date/Author: 2026-08-30 / Codex
- Decision: centralize checkout completion classification by worker termination phase.
  Rationale: dirty or unverifiable shared state is attention regardless of how the process ended, and post-start cancellation or invocation failure is ambiguous even when inspection appears clean. One classifier prevents completed, cancelled, and error paths from drifting again.
  Date/Author: 2026-08-30 / Codex
- Decision: preserve PR branch-lease loss after started work as attention and persist attempt source/result heads as one transition.
  Rationale: no alternate authority can prove whether another owner overlapped remote mutation, and a lagging GitHub snapshot may report either end of the repair transition. Both states must remain explicit rather than becoming an ordinary retry or a fresh budget.
  Date/Author: 2026-08-30 / Codex
- Decision: fully qualify GitHub branch names before passing them to `git fetch`.
  Rationale: this is a localized command-boundary omission. `refs/heads/...` removes option-position ambiguity without widening the branch grammar or changing the pinned-object checkout invariant.
  Date/Author: 2026-08-30 / Codex
- Decision: treat only passive `exhausted_attempt` attention as non-consuming in a PR-manager tick.
  Rationale: skipped, waiting, and exhausted candidates have performed no repair side effect, but cancellation after start, ambiguous push, branch-lease loss, and cleanup failure all preserve operator-owned evidence. Continuing after those states can discard the action, receipt, and retained checkout from the tick result.
  Date/Author: 2026-08-30 / Codex
- Decision: finalize PR worktrees only after branch-lease finalization classifies the outcome.
  Rationale: unambiguous success and ordinary retryable failure can remove their checkout, with force matching the pre-existing retry reset behavior. Ambiguous outcomes retain it; cleanup failure becomes its own attention action so evidence is never silently leaked or lost.
  Date/Author: 2026-08-30 / Codex
- Decision: require the managed runtime ignore for both Codex task checkout modes.
  Rationale: repo mode writes durable occurrence state below `.agent/runtime/loop/` before checkout preflight. A stale adopted repository must receive the same deterministic recopy instruction as isolated mode before runtime-owned state can be mistaken for task-authored dirtiness.
  Date/Author: 2026-08-30 / Codex

## Outcomes & Retrospective

The initial implementation slices and eight fully gated review repair rounds are complete. Round-one regressions prove strict lease ownership at finalization, retryable cancellation before workflow and worker start, fail-closed lease corruption, auditable attempt recovery, retained-worktree backpressure, per-workflow status degradation, unchanged-marker no-op writes, and first-error renewal diagnostics. Round two adds transactional stale-claim rejection, attention backpressure, typed setup retry, cancellation receipt phase preservation, retained cleanup evidence, and one-clock status classification. Round three adds transient workflow-lease retry, dirty shared-checkout backpressure, worker-receipt self-exclusion, and durable ambiguity for a started but unreconciled PR push. Round four adds immediate typed occurrence-ownership loss, durable manual and machine-global attention, post-start PR cancellation evidence, authenticated receipt appends, path-contained PR worktrees, and full checkout error chains. Round five adds durable cross-workflow shared-checkout authority, a receipted manual blocked result, independent transactional claim predicates, immutable PR-head checkout, and fail-closed incomplete-snapshot handling. Round six makes durable occurrences authoritative across lease handoff, represents unknown legacy shared scope, fails closed on retained-path inspection, and serializes exact receipt verification. Round seven replaces that over-broad receipt serialization with composable short phases, centralizes checkout ambiguity, preserves branch-lease attention, fully qualifies fetch refs, and models attempt head transitions. Round eight makes side-effectful attention terminal for a tick, validates the managed runtime ignore in repo mode, and gives PR worktrees an explicit remove-or-retain finalization boundary. Fresh structured gates pass contract, Rust file-size policy, formatting, workspace Clippy with warnings denied, 2,529 core tests, and 107 frontend tests. Another comprehensive branch review remains.

## Context and Orientation

`crates/jig/src/runtime/loops/state.rs` owns lease and attempt coordination state. Git repositories keep its authority under the worktree-specific Git metadata directory and leave a downgrade barrier under `.agent/.cache/loop/`; non-Git fixtures use that cache directly. `crates/jig/src/runtime/loops/occurrence.rs` and its `persistence.rs` child own the durable schedule ledger and occurrence state transitions under `.agent/runtime/loop/`. `crates/jig/src/runtime/loops/engine.rs` resolves a workflow, acquires its execution lease, runs one tick, and produces `ScheduledTick`; `crates/jig/src/runtime/loops/schedule.rs` claims a durable occurrence and decides whether to abandon or terminalize it.

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
    cargo test -p jig-sh --lib runtime::loops         # 157 passed after round five
    cargo test -p jig-sh --lib 'runtime::tests::loops::' # 53 passed after round five
    cargo test -p jig-sh --test cli_json loop_        # 5 passed after round five
    worker_result_file_limit_terminates_process_group_while_running # passed in 4.25s and loaded core gate
    receipt_01M1A28NETM5VCKTH3SEAHBP9W               # round-five repair work check passed
    jig.source_core_test                              # 2,514 passed
    jig.source_frontend_test                          # 107 passed
    receipt_01M1A2BDG0X0KH5F506ATF1KSR               # final rebuilt-binary contract refresh passed
    jig.contract_check / rust_file_loc / fmt / clippy # fresh and passed
    cargo test -p jig-sh --lib runtime::loops         # 166 passed after round six
    cargo test -p jig-sh --lib 'runtime::tests::loops::' # 53 passed after round six
    cargo test -p jig-sh --test cli_json loop_        # 5 passed after round six
    exclusive_journal_authority_accepts_one_runtime_receipt # passed
    unrelated_runtime_receipt_waits_for_repo_worker_verification # passed
    worker_injection_before_the_runtime_receipt_is_rejected # passed
    live_shared_occurrence_defers_dispatch_and_manual_tick # passed
    receipt_01M1A5PHAWXJNVWYJXGNV6195S               # round-six repair work check passed
    jig.source_core_test                              # 2,523 passed
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

The persisted schedule schema is version 4. Existing schema-version-1 loop and worker evidence fields remain readable; markerless schedule schemas 1 through 3 migrate conservatively, and new provider-diagnostic fields remain additive. Linux and macOS behavior, Rust 1.88, edition 2024, child-process supervision, cancellation timing, and established receipt preview truncation limits must remain unchanged.

Plan revision note (2026-08-30): replaced the initial one-line work note with a self-contained ExecPlan after resolving the review questions and establishing the green baseline. The plan separates Fowler refactoring steps from behavior fixes and defines the bounded comprehensive-review loop.

Plan revision note (2026-08-30): narrowed disposable-cache recovery from every mutating access to the pre-claim dispatch phase after existing post-work state-error tests exposed the difference.

Plan revision note (2026-08-30): after the second review, extracted pre-execution failure construction, transactional occurrence claiming, and unexecuted tick errors into focused child modules. This keeps the typed phase boundaries explicit and all changed Rust files within the repository's absolute line-count limit without changing behavior.

Plan revision note (2026-08-30): the first post-round-two structured check exposed a direct-CLI adapter regression: a worker start failure had already produced a diagnostic receipt and structured failed tick, but `into_manual_result` discarded it because execution never began. Preserve the structured result when a worker receipt exists while continuing to return raw configuration/setup errors before any worker receipt is available.

Plan revision note (2026-08-30): after the third review, extracted checkout completion reporting and the shared renewal policy, added typed ambiguous-push evidence, and narrowed repository dirtiness to task-authored changes by excluding only Jig's own worker receipt. This preserves the repository's LOC boundary and prevents the safety fix from backpressuring every otherwise-clean repo-mode task.

Plan revision note (2026-08-30): after the fourth review, extracted manual occurrence lifecycle, one-clock runtime attention aggregation, and PR outcome classification. The receipt-path exception now has an independent append-only proof, and accepted workflow IDs are encoded at the PR-worktree sink. These Fowler-sized boundaries keep every changed Rust file under the repository's 800-line limit while centralizing the new safety invariants.

Plan revision note (2026-08-30): after the sixth review, made the durable occurrence ledger the admission authority across the workflow-lease release/finalization gap, introduced an explicit unknown value for pre-schema-4 shared-checkout scope, made retained-path inspection fail closed, and serialized repo-mode workers with trusted receipt-journal writers. Exact bounded receipt verification now distinguishes runtime-owned appends from worker injection without preventing unrelated Jig commands from appending after the authority window.

Plan revision note (2026-08-30): after the seventh review, replaced whole-worker receipt authority with short locked baseline/verification phases because nested Jig receipt writers are a supported worker behavior. Centralized checkout outcome policy, recorded PR attempt head transitions, and preserved post-start branch-lease loss as attention; retained-worktree and abandonment behavior remain unchanged and explicitly characterized.


Round nine research and remediation: official GitHub CLI behavior confirms failed gh pr checks JSON exits 1, and official GraphQL/status contracts confirm UNKNOWN mergeability and cancelled checks are indeterminate rather than healthy. Those classifiers predate the branch and are tracked separately as jig-sh-mdl, jig-sh-lrk, and jig-sh-200. Branch-scope findings were structural boundary issues: attempt bookkeeping could discard completed repair evidence, Git index observation occupied the receipt-writer lock, isolated checkout policy duplicated only half the documented ignore invariant, and loop JSON cache publication lacked cleanup/reclamation. The implementation now preserves any repair action as needs_attention when attempt persistence fails, narrows receipt locks to journal I/O, centralizes both ignore checks in pre-execution policy, and centralizes locked cache temp reclamation plus immediate failure cleanup. Retained PR worktrees are intentionally protected by occurrence attention until explicit acknowledgement; tests now prove a second dispatch leaves operator evidence unchanged. The configured noop clear-attempt alias is deliberate persisted-key repair, and per-workflow occurrence resnapshotting is a freshness measure backed by transactional claim authority.

Round eleven research and remediation: deleting every checkout-local schedule authority file exposed a deeper ownership-boundary problem, while branch-lease loss after worker failure or before prepared-worktree cleanup was phase information lost through ordinary error handling. Git repositories now publish an additive initialization witness in worktree-specific Git metadata, outside the documented Codex workspace-write surface, so later dispatch remains fail-closed even after a repo-mode worker deletes checkout-local authority. PR outcome handling now retains prepared worktrees whenever lease authority is ambiguous and reports the correct pre-start or post-start attention kind. The unrelated, pre-existing non-UTF8 Codex probe flake observed during full-suite verification is excluded from this branch and tracked as jig-sh-s0j.

Round twelve research and remediation: the additive witness proved only that state once existed, while the worker-writable public ledger still supplied its contents and lock. The protected boundary now owns all three authority elements—ledger, initialized marker, and lock—and publishes the checkout ledger only as a compatibility replica. Existing witness-only repositories upgrade under lock without losing public state. PR preparation now owns cleanup of both partial paths and Git registrations before returning through the branch-lease boundary; cleanup failure retains explicit attention instead of retrying after authority release. Abandonment-error counters share one constructor, and review replies use the GitHub CLI's documented `--field body=@file` transport to avoid per-argument OS limits. Focused regressions exercise valid empty-ledger forgery, malformed replica markers, symlink redirection, protected migration, linked worktrees, cancellation immediately after `git worktree add`, failed abandonment accounting, and a reply larger than Linux's single-argument limit.


Round twelve repairs committed at 52f6489. Focused authority, PR preparation, abandonment-accounting, and large-review-reply regressions pass; the broad library suite passes 2,266 tests with 2 ignored and no failures. Starting fresh structured gates before the next comprehensive branch review.


Round thirteen research classified the unattended review-text trust boundary and merge lifecycle proof as deeper design issues, protected-authority publication as an incomplete source-of-truth cutover, and status selection, output normalization, ignore enforcement, and misleading renewal commentary as local omissions. GitHub's effective collaborator permission endpoint and the Codex sandbox contract support an admin/write prompt-source boundary; attempt persistence, renewal shutdown ordering, read-only snapshots, DST behavior, and retained-worktree pruning were confirmed deliberate. Repairs now project only trusted review input, prove resolved merges before push, commit protected occurrence state before best-effort replica publication, resolve Git metadata without a child process, scope every workflow-owned status section, and enforce the runtime ignore rule at generic mutation entrypoints. Focused loop tests and strict library Clippy pass. The first full 2,279-test library run exposed four stale integration fixtures; all four were corrected and their focused rerun passes.

Round thirteen validation is fully green at the corrected commit: contract, Rust LOC, format, strict Clippy, 2,568 core tests, and 107 frontend tests passed; vault and process partitions were not applicable by configured path policy. The initial gate batch caught two files one and two blank lines above the 800-LOC cap; removing those blank lines restored the policy without behavior changes. A subsequent native contract receipt was briefly stale because the batch began with the prior dev binary and Clippy rebuilt target/debug/jig with a new native build identity; after the required explicit dev build, a forced contract-only check passed and all eight configured gates are fresh.

Round fourteen review and remediation: independent Codex found trusted feedback could be hidden outside the ten-comment suffix; Claude's limited pass additionally identified typed GraphQL string fields, live-end receipt newline validation, indefinite loop-state locking, and worktree cancellation evidence. Official GitHub CLI and GraphQL documentation confirmed raw string fields and connection pagination; Git worktree documentation plus repository contracts confirmed worktree-specific authority. Local contracts confirmed any trusted thread comment, first-dispatch catch-up, downgrade divergence, and fresh per-repair snapshots are intentional. The speculative migration-lock and snapshot-fanout candidates were not concrete current defects. Repairs now fail closed on incomplete nested comment history, use raw GraphQL string variables, validate the captured receipt prefix, bound state-lock acquisition to 30 seconds, and preserve clean worktree cancellation evidence. A branch-added ignore test was isolated from global Git override cross-talk. Focused loop tests pass 272/272, strict Clippy passes, and structured gates pass with 2,573 core plus 107 frontend tests; all eight configured gates are fresh.

Round fifteen research and remediation: receipt schema validity could not prove append provenance because repo workers and nested Jig commands shared one writable journal, and multi-workflow dispatch could combine settings resolved before a clean repo-mode commit with prompts read afterward. These are deeper authority-boundary flaws. Exact worker-receipt append verification now fails closed on any additional record, including schema-valid direct copies and concurrent writers; checkout verification also rejects changes racing its snapshot. Repo-mode checkout completion records HEAD changes, and dispatch stops after a clean commit so the next invocation reloads one coherent repository revision. The PR-manager worker projection, output schema, and mutation allow-list now share one trusted-unresolved-thread predicate; dynamic schema limits, actionable-intent bounds, and pre-network de-duplication cap remote work while preserving explicit unknown-thread skips. Malformed leases, checkout-replica publication, removed alias identity, admin/write trust, cached diff checking, and retained-worktree stats were confirmed deliberate contracts rather than defects. Focused receipt, dispatch, PR-manager, integration, and strict all-target Clippy checks pass; the 2,288-test library run exposed one stale foreign-thread fixture, whose boundary-preserving correction also passes.

Round 16 research classified split migration locking and JSON-derived repository revision cutoff as structural design issues; consolidated schedule publication under one legacy-then-authority lock order and carried repository revision as typed completion state. Corrected bounded cache cleanup, GraphQL pagination, incomplete snapshot execution typing, and added branch-scoped regression coverage. Focused tests, the 222-test loop suite, formatting, LOC, and Clippy pass.

Round seventeen research and remediation: independent review exposed a real ordering defect in manual ticks: the durable occurrence was removed before loop-tick receipt publication, so a successful side effect followed by receipt failure could be retried. Fowler analysis classified this as a lifecycle and authority-boundary flaw rather than a local omission. Clean manual completions now stage their execution evidence while remaining running, retain renewal through receipt publication, and remove the occurrence only after the receipt commits; a receipt failure terminalizes the same occurrence as needs_attention and blocks reentry. The bounded receipt-baseline retry policy now has explicit exhaustion coverage. Official GitHub CLI and collaborator-permission documentation, repository history, and local contract tests confirmed the remaining round questions as intentional or pre-existing: no-check classification remains a fail-closed CLI seam, repo-mode worker receipt exclusivity is deliberate, PR worktrees belong in ignored cache, read-only snapshots already have a distinct API, permission lookup failures fail closed, bounded snapshot retry prevents unbounded pre-execution waiting, and retained-worktree probes predate this branch. Partial JSONL append recovery is pre-existing and tracked separately as jig-sh-v3m. Formatting, LOC, strict all-target Clippy, 223 internal loop tests, and 57 higher-level loop tests pass.

Round 18 researched every reviewer question before editing. The explicit worker-receipt exclusivity, markerless legacy shared-checkout treatment, RawJsonlRewrite::Replace contract, and best-effort public replica publication are intentional documented policies with existing tests. Git's documented explicit --force-with-lease=<ref>:<expect> semantics require the remote ref to equal the exact expected object; the immutable-snapshot contract therefore makes every remote advance, rewind, or deletion stale. GitHub's GraphQL pagination contract caps connection pages at 100 and requires cursor traversal. Fowler assessment: the PR push finding exposed a deeper missing compare-and-swap authority boundary; nested review-comment truncation was a completeness omission best fixed in the extracted review-thread boundary. Implemented exact remote-head preflight, descendant proof, explicit expected-head lease, backward nested-comment pagination with coherent count/ID/cursor safety checks, contract documentation, and regressions for older trusted feedback plus remote advance, rewind, and deletion races. Full library test run reached 2296 passing/2 ignored with one stale fake-Git fixture; repaired that fixture and its focused regression passes. Strict all-target/all-feature Clippy passes.

Round 18 gate follow-up: the core partition exposed nondeterminism in the existing branch-lease-loss regression. Its worker replaced the entire lease cache and used a one-second TTL, simultaneously invalidating workflow and branch authority; under scheduler load the workflow cancellation could win, so the test asserted a branch-specific outcome from a non-branch-specific stimulus. Added a cfg(test)-only, locked exact-key lease revocation seam, raised the fixture TTL to tolerate loaded runners, signaled after worker start, revoked only branch:codex/widgets, and corrected the top-level oracle to needs_attention. The focused regression now deterministically proves branch_lease_lost_after_start in about six seconds, and strict all-target/all-feature Clippy passes.

Round nineteen research and remediation: official GitHub pagination and rate-limit guidance supports a snapshot-wide work budget, while repository contracts confirm exact worker-receipt exclusivity, linked-worktree schedule authority, and bounded reply-intent handling are deliberate. Fowler analysis classified branch-lease release before deterministic worktree cleanup and retained PR evidence under disposable cache as lifecycle and durability-boundary flaws; composed GitHub fan-out was a boundedness omission; stale occurrence shaping and staged diagnostic loss were evidence-fidelity mistakes. PR finalization now refreshes branch authority, performs inspection and cleanup before release, retains paths when refresh fails, and stores PR worktrees under durable loop runtime state. A single snapshot client bounds every GitHub observation request, response byte, review item, and total duration, while incomplete histories skip permission calls and fail closed. Stale reconciliation preserves staged diagnostics, and abandonment returns the state that actually existed. The Rust test-quality audit maps all changed units to direct or higher-level coverage and added a real lease-cache observation at git worktree removal, exact budget boundaries, permission suppression, durable path, and staged-evidence regressions. The unrelated JSONL writer-lock concern remains excluded and tracked as jig-sh-yep.

Round nineteen gate follow-up: the first structured batch executed the core and frontend partitions successfully but refused gate attestation because uncommitted Round 19 edits overlapped files changed since the plan baseline. The standalone LOC check then caught occurrence/tests.rs at 838 lines; the staged-evidence regression moved into the existing occurrence/tests module tree, restoring the source-size contract. After committing the implementation and rebuilding the development binary, the fresh batch passed contract, Rust LOC, formatting, strict Clippy, 2,593 core tests, and 107 frontend tests; vault and process partitions were not applicable. Batch receipt: receipt_01M1BRH8ESSDFYDKXJWJHCCDA0.

Round twenty review research classified two findings as design-level boundary errors and two as local omissions. PR preparation had a second destructive-cleanup owner outside the lease-refreshing finalizer, so cleanup is now centralized in the finalizer and preparation returns only an optional candidate path. GitHub completeness was modeled globally even when only one PR's review history was incomplete, so list-level validity and per-PR validity are now separate. Manual-history ordering and unexecuted renewal diagnostics were local omissions; pruning now uses finish/start recency and finalization suppresses only typed expected ownership loss. Added branch-lease reassignment, mixed complete/truncated PR, full 100-PR snapshot, manual pruning, and renewal evidence regressions. Logged the pre-existing acknowledged-retained-history concern separately as jig-sh-dx5. Focused PR-manager, occurrence, GitHub, and schedule tests pass; full loop verification is being rerun after removing a wall-clock TTL dependency from an existing stale-claim test.

Round twenty verification completed on commit 8ed6d0d. Full cargo test passed 2,309 library tests plus all integration suites with two intentional ignores. The fresh structured batch passed contract, changed-file LOC, source formatting, strict Clippy, 2,600 core tests, and 107 frontend tests; vault and process partitions were not applicable. Batch receipt: receipt_01M1BWF1BW3CCV5MSBK3BACBAB. Work gates report all eight required gates fresh or explicitly not applicable. The existing stale-claim regression now uses deterministic sampled expiry while keeping the production lease long enough for a successful retry under full-suite load.

Round 21 review questions were resolved from repository contracts and Git worktree/pseudo-ref semantics. Implemented centralized managed-path validation, cancellation-aware checkout preflight classification, GitHub repository-environment scrubbing, and an aggregate review-thread update budget, with focused regressions. The full jig-sh package suite, strict Clippy, formatting, and changed-file LOC policy pass before harness gates.

Rounds 22 through 25 continued the same branch-scoped authority audit. Loop schedule and retained-worktree paths now use centralized managed-path checks; renewal and stale-finalization state preserve first-error evidence and bounded lock deadlines; receipt journals use no-follow directory capabilities; PR validation checks only worker-authored history after a clean base merge; parent code owns staging, validation, commit, and push; duplicate review intents are collapsed before effects; acknowledgement uses a compensating schedule transition; retained-worktree backpressure is one workflow policy; bounded pruning reserves the latest scheduled dispatch watermark; and PR cleanup remains behind renewed branch authority through removal. Each repair slice passed focused loop tests, strict Clippy, formatting, LOC, and fresh structured gates. Pre-existing or target-base concerns were excluded and recorded in Beads, including jig-sh-3lj, jig-sh-3vs, jig-sh-3fx, jig-sh-73n, and jig-sh-t9n.

Round 26 research resolved every reviewer question before editing. The lease/attempt cache write-and-rename behavior predates the branch and remains intentionally disposable; its crash-recovery concern is excluded as jig-sh-ji9. The installed Codex CLI explicitly supports `codex exec review -o`. The bounded 400-year invalid-cron validation runs at repository configuration load and completes in roughly 30 ms in the debug regression. Protected Git metadata is the documented schedule commit point, so checkout replica publication remains intentionally best-effort during both forward commit and compensation. Fowler analysis classified the actionable findings as misplaced authority rather than unrelated mistakes: cache path safety belonged to one capability owner, PR writability required repository identity rather than a permissive boolean default, manual identity namespace belonged to the occurrence store, and state commands had dropped their execution observer and performed Git receipt inspection inside a transactional schedule-lock boundary. The implementation now opens every disposable cache component without following links and performs locks/reads/temp cleanup/replacement relative to that capability; requires an explicit same-repository PR head identity; namespaces manual occurrences as `workflow@manual:<item>`; threads cancellation into clear-attempt and acknowledgement; omits Git metadata from their lightweight state receipts; and gives skipped duplicate review posts the stable evidence shape used by other outcomes. Focused regressions and the complete 263 internal plus 60 higher-level loop partitions pass. The first structured core gate caught that the capability opener created an absent cache during read-only UI status; the API now separates existing-only reads from create-on-mutation access, and the original non-creation regression plus the symlink regression pass. Fresh strict gates and another comprehensive branch review remain.

Round 26 structured validation is fresh and fully green at commit 62490bd. Batch receipt receipt_01M1CGP3FQQ5PHKFNZTTYH8QD4 records contract, changed-file Rust LOC, formatting, strict Clippy, 2,633 core tests, 107 frontend tests, 442 vault tests, and 209 process tests. All eight configured gates report fresh evidence with no unresolved gate.

Round 27 research resolved all reviewer questions before editing. Repo-mode workspace-write is intentional; successful PR repair rounds deliberately consume their item budget; markerless legacy occurrences deliberately block shared checkout conservatively; and the installed `codex exec review` supports `-o`. The destructive PR-worktree reuse, cache-fsync, and partial-JSONL findings predate this branch and remain excluded under Beads jig-sh-cdi, jig-sh-ji9, and jig-sh-v3m. The claimed tight `loop run` retry is unsupported because pre-execution snapshot failures abort through the engine while executed failures first publish attempt/backoff state. Fowler analysis identified the remaining branch-scoped symptom as an authority-placement defect: a repo-mode worker could rewrite lease and attempt files inside its writable checkout. The shared Git metadata resolver is now a loop-level owner, and a typed JSON persistence boundary migrates legacy lease/attempt state under locks, writes an older-runtime rejection marker with recovery state, and thereafter reads and mutates only protected worktree-specific authority. Migration and domain mutation are separate locked phases, so a failed protected publication cannot strand an operation whose caller observed an error. Non-Git fixtures retain capability-anchored cache behavior. The test-quality audit added migration, checkout-tampering, protected-corruption, schema, marker-target, failed-publication, cleanup-order, and dual-failure compensation regressions; stale higher-level fixtures now inject deliberate failures at the protected authority. Focused tests and all 60 higher-level loop tests pass; full structured gates remain to be rerun.

Round 27 structured validation is fresh and fully green at commit 99b8ec0. Batch receipt receipt_01M1CM902SEBEGPBZAYPTECT6V records contract, changed-file Rust LOC, formatting, strict Clippy, 2,640 core tests, 107 frontend tests, 442 vault tests, and 209 process tests. All eight configured gates passed with fresh evidence and no unresolved gate.

Round 28 research separated two branch-scoped transaction/lock-boundary defects from target-base and intentional behavior. Manual clear-attempt removed state before its receipt could fail, and occurrence acknowledgement held its schedule locks while waiting indefinitely for the receipt journal. Both state transitions now use compensating persistence and carry one operation deadline into cancellation-aware receipt locking. Read-only attempt and dispatch-window observations use atomic snapshots without lock-taking cleanup. Worker-receipt verification now directly covers unreported and unterminated appends. The target-base cancelled/unknown GitHub-check classifier is excluded as jig-sh-ter; the existing disposable-cache durability issue remains jig-sh-ji9. Successful repair budget consumption, skipped/neutral check treatment, exact expected-head force leases, worktree-specific authority, bounded cron validation, and operator-managed retained evidence were confirmed intentional or already covered. Focused state, schedule, loop-integration, receipt-journal, and checkout-verification suites pass; strict gates and the next comprehensive review remain.
