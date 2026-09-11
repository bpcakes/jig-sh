# Keep verification recovery precise and reuse checked working files

This ExecPlan follows `.agent/PLANS.md`. Owning issue: `jig-sh-yaa`. Structured plan: `plan_01M26JV0DWV299YEKXDA1HTNG1`; immutable Git baseline: `10a3dc9ae63547b09a48b05a463495bce2101f37`.

## Purpose and scope

Agents should see which check is unresolved, why, which commands a repair would execute, and which original passes it would reuse. An observation timeout must recommend a larger read-only inspection before suggesting execution. Checks explicitly declared to consume working-file content should keep their receipts after staging or committing the same bytes; genuine input, command, configuration, prerequisite, Git-state or comparison-baseline changes must still invalidate the affected evidence.

The four requested items are retained. Inspection establishes that generated Rust and Go verification checks are already independent; jig-sh's authored test dependencies need removal and generator regression coverage. Existing JSON diagnostics and native plan-baseline recovery already provide underlying functionality; the implementation will expose and reuse those mechanisms rather than create parallel authority. Existing contract epochs and append-only receipts keep their interpretation. All new repository fixtures, plans and evidence use generic identifiers.

## Progress

- [x] (2026-09-10 21:16 UTC) Read the proposal, current guides, source and existing tests. Verified root artificial dependencies, hidden human target details, misleading timeout next steps and projection-sensitive source identity.
- [x] (2026-09-10 21:16 UTC) Remove root artificial test dependencies; add generated-default and authored-prerequisite preservation coverage. Focused repository-model tests passed (30).
- [x] Add shared native execution/reuse preview and actionable per-target recovery to CLI/MCP gates/evidence, with correct deadline/resource advice.
- [x] Add versioned, explicit working-file source-state policy; preserve old identity semantics, real Git inputs, original execution proof and native comparison authority.
- [x] Prove downstream behavior using disposable generic repositories and validate compatibility, failure supersession, dependencies, races and mutations. Focused Nextest selection: 130 passed.
- [x] Update public documentation, generated contract/launcher and root dogfood configuration as required by the new epoch.
- [x] Build the development runtime, run required harness checks, inspect current receipts and gates, complete the owning issue and structured plan. Full workspace: 4,104 passed, 3 skipped; all five required checks are current and reused.

Completion checkpoint: implementation and verification are complete. All four requested behaviors are implemented and covered by focused tests plus the full workspace suite. The development runtime passed formatting, Clippy, contract epoch 10, file-budget and all 4,104 workspace tests (3 skipped). Final work check reused all five original passing receipts. Gates and evidence passed with complete inspection and an empty execution preview. The owning issue and structured plan are closed. No commit was requested or created.

## Surprises & Discoveries

Generated adapter actions have no artificial test dependencies. The root `.jig.toml` explicitly supplied four such edges; removing those does not remove any required verify member. Authored prerequisites and frontend boundary dependencies remain intact.

`work gates` and `work evidence` already carry per-target reason codes and bounded source previews in JSON. The existing human output suppresses them and unconditionally suggests `work check` on blocked open plans. Collection exhaustion is already `unknown`, not `stale`; recovery advice is the defect.

Contract v9 hashes committed, staged and current projections into exhaustive source identity. Existing `direct_edit_add_delete_rename_staging_and_mode_have_distinct_authority` explicitly proves staging changes its token. Original execution/adoption guards also use a global source snapshot; these guards must remain strict even when later receipt reuse becomes content-based.

## Decision Log

2026-09-10: Keep generated independent defaults and protect them with regression tests; remove only the demonstrated root configuration mistake. Preserve real prerequisites.

2026-09-10: Build recovery from the already collected gate report and extract the existing scheduler into a shared helper. Preview and execution must use the same dependency closure. An incomplete inspection cannot promise reuse.

2026-09-10: Introduce an explicit action source-state declaration, with conservative Git-sensitive behavior when omitted and an opt-in working-file policy. Use a new contract epoch to prevent older runtimes from silently interpreting new authority. Preserve epoch 9 receipt interpretation and append-only history. The exact source collector interface is being finalized against existing bounded filesystem/Git observations; both scope policies must have explicit behavior.

## Outcomes & Retrospective

All four requested outcomes are complete. Generated verify defaults were already independent; root authored dependencies were corrected and real-prerequisite preservation is tested. CLI, MCP and dashboard share actionable recovery data, with read-only timeout remedies and plan-bound native commands. Epoch 10 provides explicit working-file reuse while old epochs, Git-sensitive checks, native comparison authority and original execution guards retain their semantics. Tests prove reuse of original receipt IDs across staging and commits, plus continued invalidation for genuine changes.

Full verification: 4,104 workspace tests passed, 3 skipped. Formatting, Clippy, contract and file-budget passed. The final work check executed no targets and reused all five current original passes. Gates/evidence both reported passed, with complete inspection and no pending native execution. Final test receipt: `receipt_01M26SVYP227XQJBJNQ90933D8`. Plan-close receipt: `receipt_01M26SZ019RJSGJPQMFDEN3J1E`.

Integration validation found and corrected legacy fixture downgrades, explicit receipt-metadata exclusions, dashboard recovery projection drift and a stale current-epoch test assertion. No validation assertions or execution safety rules were weakened. Historical receipts and state prefixes remain intact; Beads exports pass the privacy guard.

## Context and orientation

`crates/jig-contract/src/repository.rs` defines authored action configuration, including `inputs_policy` (whole repository versus an exhaustive list of relevant paths). `crates/jig-contract/src/freshness.rs` defines versioned equality tokens and diagnostics. A receipt is an append-only record of original execution. Reuse validates that original result and its recorded dependency execution proof against current inputs; it never fabricates a new target execution.

`crates/jig/src/repository/freshness/source.rs` collects bounded Git and filesystem observations; `source/files.rs` hashes actual bytes through verified file descriptors. `freshness/proof.rs` verifies original execution/dependency identity. `state/receipts/validity.rs` and `archive/dependency_protection.rs` must recognize supported identity epochs without weakening malformed/future-record handling. `context.rs` owns supported manifest epochs, and bootstrap repository-model normalization carries authored policy into generated manifests.

`runtime/work/checks/targets.rs` computes and executes the native repair closure. `runtime/work/gates.rs` and its submodules inspect receipts. `cli/output/work.rs` renders human summaries; MCP uses the shared runtime reports. Native `check TARGET --plan-id ID` derives an exact immutable comparison from the work plan in `repository/native_input.rs`; explicit comparison requests remain distinct authority.

## Plan of work and milestones

First, verify and correct dependency requirements. Keep all required targets in the verification profile, remove only unnecessary execution edges, and prove custom build prerequisites survive generation and reload.

Second, expose the existing execution closure as a read-only recovery preview. Render target-specific freshness reason paths, execution/reuse decisions, and structured native repair commands bound to the plan. Do not call execution or append receipts while inspecting. If observation reaches a deadline below the maximum, the next command must repeat the same inspection with a larger budget. Resource exhaustion or an exhausted maximum must remain diagnostic, without suggesting blind execution. Legacy tool evidence must be explicitly distinguished from native target evidence.

Third, implement the explicit working-file policy under a new manifest epoch. Working-file identity covers the current observable path set, types, executable modes and content bytes independent of committed/index placement. An absent path, new path, rename, deletion, changed executable bit, runner/configuration change and transitive prerequisite change remain authoritative. Preserve comparison baseline and Git-sensitive actions. Keep global execution guards strict against a worktree/index mutation during execution or a prepared-plan race. Old epochs remain readable and retain their old equality rules; migration requires fresh new-epoch evidence once rather than rewriting old records.

Finally, integrate the public configuration/CLI/MCP contract, source docs and root harness. Validate all acceptance behaviors, then run required gates with the rebuilt development binary. Complete only after fresh current evidence and a requirement-by-requirement audit.

## Concrete steps and validation

All source commands run from the repository root. Build with `cargo build -p jig-sh --bin jig`; use `JIG_DEV_BIN=target/debug/jig` for receipt-producing harness commands. Run focused library tests for repository-model defaults, source identity/proof compatibility, runtime work evidence and CLI/MCP recovery during implementation. The final required commands include `scripts/jig check fmt`, `scripts/jig check clippy`, `scripts/jig check contract`, `scripts/jig check test`, and the connected `work check`, `work gates`, `work evidence`, `work receipts` and `work finish` for this plan. Avoid repeating successful expensive checks unless source or configuration changes or a specific unresolved concern requires it.

A generic consumer fixture must count actual executions. After a pass, tracker edits should preserve exhaustive check receipts; after staging and committing unchanged checked contents, working-file actions should reuse the same original receipt IDs. Relevant file changes, additions, deletions, modes and helper changes must invalidate them. A Git-sensitive check must retain its declared index/HEAD dependence; a native comparison check must retain its plan baseline/provenance. A newer failed, unknown, expired or mutating result must never reveal an older pass. Old epoch fixtures must retain their original staging behavior.

For recovery, inspect without writing receipts and compare the advertised execution closure with the subsequent actual run. Execute a generated native repair command and verify it satisfies the relevant plan gate. Exercise deadline and resource failures through actual typed failure paths, including non-default and maximum inspection budgets; CLI and MCP must expose the same recovery facts.

## Idempotence and recovery

All durable history remains append-only. Configuration migration is explicit and older runtime/manifest incompatibility fails before execution. Old receipts are not relabeled. Repeating inspection is read-only; repeating a work check revalidates and reuses original successful executions when their authority still matches. Preserve unrelated user changes and never stage, commit or push without a request. Use `python3 scripts/beads-sync.py` for tracker export; its earlier stale-export error was resolved additively and the final privacy guard passes.

## Interfaces and dependencies

Use the existing typed action model, bounded source collector, original receipt validator and native comparison preparation. The new action declaration separates what paths are relevant from whether Git placement is itself an input. Shared recovery DTOs belong in runtime/report boundaries and may add fields without changing existing statuses or reason codes. No new external libraries or services are needed.

Revision 2026-09-10: initial scope grounded in source inspection and focused generated-default tests; implementation and compatibility details remain active work.

Revision 2026-09-10: implemented epoch 10 with conservative Git defaults and explicit Worktree assertions. Jig's formatting action uses whole-repository Worktree authority. Its build.rs reads Git HEAD/ref/tag metadata, so Cargo checks retain conservative Git authority alongside native contract and file-budget checks. Recovery preview and execution share the complete prerequisite closure, including hidden prerequisites of required roots. Typed deadline/resource diagnostics preserve unknown statuses. Integration review identified and is covering unborn Git HEAD and ignored runner boundaries; existing execution guards remain strict.

2026-09-10 source-policy audit: do not infer working-file purity from a Cargo command. `crates/jig/build.rs` observes Git metadata for build identity; keep Jig Cargo checks Git-sensitive. Only the audited `scripts/check-rust-format.sh` command opts into Worktree in the root configuration. Generated unknown/user-authored commands remain Git by default.

Validation checkpoint 2026-09-10: 130 focused Nextest tests passed with the repository's four-process isolation policy. An earlier unbounded Cargo test run hit existing process-cleanup failures under load; isolated reruns of the same scoped suite passed without relaxing supervision or deadlines. Final Clippy identified and cleared one needless borrow and two redundant test clones. Formatting, Clippy, contract epoch 10 and the plan-bound exact-baseline file-budget gate pass. Bead implementation closure explicitly delegates the outstanding workspace regression gate to this still-open structured plan; no goal completion claim is made before that gate and final evidence pass.

Full-suite checkpoint 2026-09-10: the first workspace run passed 1,254 tests before stopping on two legacy migration fixtures. Those fixtures downgraded a current render to epoch 6/7 but stripped only inputs_policy; source_state and its provenance also need removal to represent real legacy input. Corrected that fixture downgrade without relaxing pre-10 validation. Both migration tests and both epoch-10 proof regressions pass (4/4); the rebuilt runtime is rerunning the full workspace gate. Issue closure links final regression status to this still-open plan.

Metadata compatibility checkpoint 2026-09-10 UTC: final audit found that whole-repository Worktree observation must retain the existing explicit work.receipt_metadata exclusion. Stopped the live workspace test intentionally before editing. Whole-policy digests now omit owned tracker paths and their diagnostics, and shared file collection prunes tracker stores unless an exhaustive sibling declares them. Repository-local runners in excluded metadata cannot gain a proof. This preserves metadata writes during checks without weakening ordinary source mutation guards. Added default-inclusion, tracker staging/commit, nested-fixture and live original-receipt reuse coverage; the nine selected whole-worktree regressions pass. Public documentation states the exclusion and exhaustive-scope distinction. Rebuilding and full verification remain required.

Acceptance audit 2026-09-10 UTC: all four requested behaviors are covered. (1) Independent generated verify membership and preservation of authored execution prerequisites have regression coverage; the root artificial edges are removed. (2) Recovery inspection is read-only, names target/input reasons, shares the execution scheduler, and emits plan-bound commands; native recovery and legacy mismatch tests pass. (3) Deadline and resource limits retain unknown evidence, and incomplete inspection withholds execution advice in favor of a read-only retry where possible. (4) Both Worktree input-scope policies reuse original receipt IDs across staging/commit, while real file/helper/prerequisite changes, Git defaults, native baseline authority, failed newer executions and execution-time mutations retain their guards. The root recovery preview independently confirms that only the pending test target needs evidence and all four other required checks are reusable. Append-only journal prefixes and the Beads export privacy guard pass. The final workspace run is still active.

Dashboard integration checkpoint 2026-09-10 UTC: the full workspace run reached 3,568 passing tests before an existing parity test detected that typed dashboard status omitted the new recovery field. Recovery DTOs now live in jig-contract and are shared by runtime JSON and the typed dashboard snapshot; scheduling remains runtime-owned. All 114 focused UI/source/recovery tests pass. The native recovery integration fixture now also asserts dashboard parity and read-only behavior. The issue was reopened while fixing the regression. A final rebuilt runtime and full suite are required before closure.

Epoch assertion checkpoint 2026-09-10 UTC: the next full workspace run passed 3,679 tests, including dashboard parity, before ui_cutover's product-version test hit a hard-coded epoch-9 manifest/launcher expectation. Updated both expectations to 10 while retaining the independent product-version assertion. Audited remaining epoch-9 references; remaining uses intentionally cover legacy fixtures and old receipt semantics. Running all binaries outside the jig-sh library with no fail-fast to cover the previous run's unexecuted tail before final full verification.

Final verification 2026-09-10 UTC: the clean full run passed all 4,104 tests in 1,070.642 seconds. Work check reused api:clippy, api:fmt, api:test, repo:contract and repo:file-budget. Final gate/evidence inspection passed; work finish closed this plan and its session successfully.
