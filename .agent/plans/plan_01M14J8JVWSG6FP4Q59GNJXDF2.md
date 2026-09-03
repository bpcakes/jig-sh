# Resolve loop review findings without expanding branch scope

This living plan closes the comprehensive-review findings in the current `feat/jig-loop-iteration` working-tree diff. The observable goal is that durable occurrence state never silently reinitializes after it has been observed, renewal failure cannot prevent terminal evidence from being persisted, tick-finalization diagnostics survive workflow failure, and `loop clear-attempt` preserves its schema-version-1 JSON shape while still accepting removed workflow keys.

## Progress

- [x] Research the four prior review questions in repository plans, docs, tests, and Git history.
- [x] Establish a green baseline with `cargo fmt --all -- --check`, 104 focused loop unit tests, and the Fowler changed-code scanner.
- [x] Diagnose the shared design causes and select small Rust/Fowler boundary refactorings.
- [x] Add characterization and regression tests for each finding.
- [x] Apply the structural refactorings, then the separate behavior fixes.
- [x] Run focused suites and the complete `cargo test -p jig-sh` target (2,167 unit tests passed, 2 stress tests ignored, all integration and doc-test targets passed).
- [x] Build the development Jig binary and run configured contract, LOC, format, Clippy, and partitioned test gates.
- [x] Run independent Codex and Cursor comprehensive review on exact working-tree fingerprints; fix branch-scope findings and repeat until the completed native Codex pass is clean. The eighth native Codex pass was clean; the ninth found two in-scope races, now fixed; and the post-remediation pass found no actionable findings or open questions. Cursor was attempted each cycle but its CLI did not complete a review on this host.
- [x] Record the unrelated oversized frontend install-locking test module as Bead `feat-codex-resume-generic-monorepo-7kn` and exclude it from the fix loop. The JSONL-only fallback succeeded; normal `br sync --flush-only` remains blocked by the existing incompatible local Beads runtime schema.

## Surprises & Discoveries

- The legacy migration marker lives under `.agent/.cache/loop/`, even though repository docs explicitly classify that directory as disposable. It cannot remain the sole proof that the durable ledger was initialized.
- `read_json_or_default` is appropriate for disposable leases and attempts but unsafe for an already-initialized durable occurrence ledger; using the same helper hid `NotFound` after a time-of-check/time-of-use race.
- `OccurrenceGuard::finish` and `abandon` stop at a renewal-thread error before attempting the authoritative locked transition, even though expiry-aware transitions already preserve ambiguity and worker evidence safely.
- `ScheduledTick::Errored.error` currently represents either workflow failure or post-work receipt failure, forcing callers to infer error ownership from completion status.
- Historical `clear-attempt` output stores a resolved workflow object in `workflow`. The branch changed it to a string only to support removed workflow keys; this is not an intentional schema cutover.
- The Fowler scanner reported 188 heuristic candidates. The accepted findings are the persistence precheck/default split and the two lifecycle/error-ownership boundaries above. File length, test `unwrap`, DTO fields, orchestration parameters, and unrelated duplication are explicit non-findings for this task.
- The first remediation review found that receipt failure could erase the only workflow-lease-held signal. A typed lease disposition and typed state-error collection now survive receipt output failure, so an unexecuted occurrence is abandoned rather than terminalized.
- A later review found that backward GitHub comment pagination multiplied the per-command timeout by up to 100 pages. Repository history and execution configuration establish the normal lookup budget as the configured command timeout; the scan now shares that one monotonic budget while post-mutation reconciliation retains its distinct 30-second cap.
- The fourth review found that using a new `exhausted` action status to distinguish attempt exhaustion silently changed the schema-version-1 action vocabulary. The historical `needs_attention` status is now preserved, while additive `attention_kind` metadata lets the occurrence classifier distinguish attempt repair from ambiguous execution.
- The same review found that `clear-attempt` resolved built-in aliases before inspecting the exact persisted key. Since the no-op workflow cannot create attempts, an exact persisted alias key without a current matching configuration is evidence of a removed configured workflow and must retain that identity in the tombstone.
- The sixth review found that general expired-claim ambiguity had leaked into the held-workflow-lease deferral path, even though typed lease disposition proves no worker ran. It also found that the documented schema-3 downgrade marker was published only when a legacy ledger already existed, leaving fresh initialization exposed to an older dispatcher.
- The seventh review found two remaining interleavings in those fixes: stale reconciliation could terminalize an expired but demonstrably unexecuted held-lease claim before its owner removed it, and a lock-free reader could observe the newly published legacy marker after initially missing the durable ledger. Both are narrow races within the branch's compatibility and deferral changes, not unrelated cleanup.
- The ninth review found that canonicalizing a prompt and later reopening its absolute path left an intermediate-directory symlink-swap window. It also found that successful unexecuted abandonment after exact stale reconciliation returned the transient terminal record, causing the dispatcher to contradict the public deferred contract. These are deeper boundary leaks rather than isolated typos: path containment must be enforced by the opened directory capability, and the occurrence transition—not its caller—must normalize the superseded reconciliation state.
- The post-fix full gate passed every core, process, vault, and vault-TUI test, then exposed unrelated frontend-fixture isolation failures and left `/tmp` returning `EROFS`. The affected files are unchanged by this branch; the problem is recorded as Bead `feat-codex-resume-generic-monorepo-55h` and excluded from this loop.
- `scripts/jig work check` cannot attest the repository's pre-existing partially staged files without changing the user's index. The same configured gates were therefore run directly against the checked worktree; the real staging state was preserved.

## Decision Log

- Decision: preserve `clear-attempt.workflow` as an object and add `workflow_id` as an explicit string.
  Rationale: schema-version-1 output is a compatibility boundary. Configured workflows retain the full historical object; removed workflows use a tombstone object containing their ID and explicit removed/configured facts.
  Date/Author: 2026-08-28 / Codex
- Decision: add a durable initialization marker beside `schedule.json` and carry an observed-file expectation into every read.
  Rationale: durable-state loss must fail closed even after disposable cache cleanup, and a file observed before locking must not become a default ledger if it disappears before open.
  Date/Author: 2026-08-28 / Codex
- Decision: make occurrence finalization return the persisted occurrence plus any renewal diagnostic.
  Rationale: renewal shutdown is ancillary cleanup; the authoritative terminal transition must still run. A successful transition should not be misreported as unpersisted, while the renewal error remains dispatch state evidence.
  Date/Author: 2026-08-28 / Codex
- Decision: represent post-work tick failure separately from the workflow command error.
  Rationale: receipt/finalization diagnostics and workflow outcome are independent facts and must not be reconstructed from each other.
  Date/Author: 2026-08-28 / Codex
- Decision: constrain every review iteration to defects introduced by or materially affected by the current branch diff.
  Rationale: this prevents an ever-expanding cleanup loop. Plausible unrelated defects are recorded through `br` and exported with `br sync --flush-only` instead of being fixed here.
  Date/Author: 2026-08-28 / Codex
- Decision: carry workflow-lease disposition and runtime state errors as typed `ScheduledTick` fields independent of optional receipt-backed JSON.
  Rationale: dispatch correctness must not depend on whether presentation/evidence serialization succeeded; making the independent facts mandatory on every tick variant prevents the deferred-execution regression from recurring.
  Date/Author: 2026-08-29 / Codex
- Decision: bound the full pre-mutation reply lookup by the configured command timeout and reuse a generic remaining-budget calculation.
  Rationale: this preserves the documented configurable command budget while preventing page count from multiplying elapsed time; the 30-second reconciliation budget remains intentionally separate because it follows an uncertain external mutation.
  Date/Author: 2026-08-29 / Codex
- Decision: preserve the public exhausted-attempt action status as `needs_attention` and add `attention_kind = exhausted_attempt`; suppress occurrence escalation only when the worker kind, subtype, and persisted exhausted fact all agree.
  Rationale: serialized compatibility and internal occurrence policy are separate responsibilities. A validated additive discriminator prevents either concern from overloading the other's status vocabulary.
  Date/Author: 2026-08-29 / Codex
- Decision: have `AttemptStore` atomically return a removed attempt record and choose the clear-attempt workflow descriptor from current configuration plus that exact persisted fact.
  Rationale: identity must follow the record being repaired, and a locked take operation avoids a check-then-clear race. Resolution is validated before mutation so invalid current configuration cannot erase the attempt first.
  Date/Author: 2026-08-29 / Codex
- Decision: split unexecuted deferral from general abandonment and allow only the typed held-lease path to remove an owner-matched running claim after expiry.
  Rationale: expiry is ambiguous after worker execution but not before the execution lease is acquired. Separate transitions make the proof explicit and prevent either policy from weakening the other.
  Date/Author: 2026-08-29 / Codex
- Decision: retain the legacy schedule lock from migration inspection through durable publication and downgrade-marker publication, including fresh initialization.
  Rationale: repository history and documentation make mixed-version fail-closed behavior an intended compatibility boundary. One lock-ordered cutover closes the window in which a schema-1 dispatcher could create an independent ledger.
  Date/Author: 2026-08-29 / Codex
- Decision: permit unexecuted abandonment to supersede only the exact owner-matched, evidence-free stale-reconciliation state produced for an expired claim.
  Rationale: typed held-lease disposition proves no worker ran, but worker receipts, worktrees, acknowledgements, other owners, and every other terminal shape remain ambiguity barriers. The precise predicate closes the dispatcher race without weakening general terminal-state protection.
  Date/Author: 2026-08-29 / Codex
- Decision: when a lock-free read initially misses durable state but observes its legacy migration marker, re-read and validate the durable ledger once before reporting inconsistency.
  Rationale: publication is ordered durable-ledger first, marker second. Marker observation can therefore race only with a stale initial miss; the consistency re-read accepts the valid publication while still failing closed if durable state is genuinely absent.
  Date/Author: 2026-08-29 / Codex
- Decision: open configured prompt paths through a repository directory capability and let the capability filesystem resolve every component.
  Rationale: canonicalize-then-reopen is a check/use split that cannot preserve containment under concurrent renames. `cap-std` keeps resolution rooted in the already-open repository directory, preserves contained relative symlinks, and rejects absolute or escaping targets without maintaining a custom platform-specific path walker.
  Date/Author: 2026-08-30 / Codex
- Decision: make the unexecuted abandonment transition normalize the exact stale-reconciled record back to running semantics before returning it.
  Rationale: stale reconciliation is an internal competing transition that the typed held-lease proof is allowed to supersede. Returning that transient terminal shape leaked occurrence internals into dispatch reporting; normalization keeps the public deferred contract unconditional after a successful abandon.
  Date/Author: 2026-08-30 / Codex

## Outcomes & Retrospective

The review symptoms were mixed. The clear-attempt JSON regression was a localized compatibility omission. The persistence, finalization, and error-loss failures shared deeper responsibility and temporal-coupling problems: durable initialization was inferred from disposable state, renewal cleanup could short-circuit the authoritative terminal transition, and dispatch reconstructed execution facts from optional JSON. Small boundary refactorings now give each invariant one durable or typed owner.

All focused regressions pass through the ninth-round fixes: 122 loop tests, all 26 CLI JSON tests, formatting, workspace Clippy with warnings denied, and diff hygiene. The current complete `cargo test -p jig-sh` run passed 2,180 unit tests (two deliberate stress tests ignored) plus every integration and doc-test target. The earlier configured gate passed 2,463 core, 209 process, 440 vault, and 2 vault-TUI tests. Its unchanged frontend partition passed 87 tests before two fixture failures (`EROFS` and an unrecoverable generated install lock) cancelled the remaining 18; the prior configured run had passed all 107 frontend tests. Contract and Rust LOC against the branch baseline also passed earlier. The post-remediation native Codex review completed with no actionable findings or open questions; the final plan-only status update is covered by one last exact-fingerprint pass.

An earlier clean review retained a residual test-risk note rather than a finding: no focused test forces `git worktree remove` to time out after completing its side effect. Existing reconciliation coverage made it no evidence of a defect, so it does not justify expanding this branch's fix loop. The later schedule-boundary notes were promoted to findings and are now covered directly.

No unrelated code was changed. The one unrelated LOC defect discovered by a deliberately broader manual comparison is recorded as Bead `feat-codex-resume-generic-monorepo-7kn`. The local Beads database reports an incompatible runtime schema, so `br sync --flush-only` could not run; the issue was written through Beads' supported JSONL-only mode instead of editing tracker files manually.

## Context and orientation

`crates/jig/src/runtime/loops/occurrence/persistence.rs` owns the durable schedule ledger and legacy-cache migration. `occurrence.rs` owns claim renewal and terminal transitions. `schedule.rs` orchestrates scheduled ticks and dispatch evidence; `schedule/policy.rs` converts workflow completion into occurrence state. `engine.rs` owns tick and `clear-attempt` JSON output. The persistent ledger, CLI JSON, cancellation timing, and terminal evidence are compatibility-sensitive boundaries.

The effective scope is these loop modules and their focused unit, integration, CLI JSON, and documentation tests. Generated code, vendored code, other crates, unrelated branch changes, and scanner-only style candidates are excluded.

## Plan of work

First add tests that express the existing compatibility contract and the desired fail-closed/finalization behavior. Then extract an optional JSON read primitive and make persistence carry an explicit `required` expectation instead of defaulting after a precheck. Publish a durable initialization marker only after a valid ledger exists. Next introduce a named occurrence-finalization result so renewal errors cannot skip the locked transition. Then split post-work state error from workflow command error in `ScheduledTick`. Finally restore the `clear-attempt` object shape with an additive workflow ID and a removed-workflow tombstone.

Keep refactoring and behavior changes separate in the patch sequence, compiling and running the narrow loop tests after each boundary moves. Do not change serialized occurrence schema, occurrence status strings, existing configured-workflow JSON fields, worker cancellation timing, or public crate APIs.

## Concrete steps

1. Add persistence tests for cache removal followed by ledger loss and for ledger deletion while a locked reader is waiting.
2. Extract `read_json_if_exists_with_cancellation`; migrate the existing defaulting helper without behavior change and verify loop state tests.
3. Add durable initialization expectation/marker handling in `SchedulePersistence`; run occurrence persistence and lifecycle tests.
4. Add a guard-level renewal-error regression; introduce `OccurrenceFinalization`; update deferred and terminal dispatch paths to retain renewal diagnostics; run occurrence and schedule tests.
5. Add failed-workflow-plus-receipt-failure coverage; split the post-work error channel and run policy/scheduled-failure tests.
6. Add configured and removed workflow JSON compatibility assertions; restore `workflow` object shape, add `workflow_id`, and update the human formatter.
7. Run `cargo fmt`, focused loop tests, `cargo test -p jig-sh`, build `target/debug/jig`, and execute configured Jig gates with `JIG_DEV_BIN=target/debug/jig`.
8. Run comprehensive review with Codex and Cursor against one complete working-tree fingerprint. Research open questions before edits. Fix in-scope findings and repeat; create/sync Beads for unrelated findings.

## Validation and acceptance

Focused tests must prove that an initialized durable ledger cannot default after cache cleanup or a lock-wait deletion; a renewal-thread error still records worker receipt/worktree/outcome; deferred work does not become false ambiguous attention when abandonment persists; tick-receipt failure remains a scoped state error even when workflow completion failed; configured `clear-attempt.workflow` remains the historical object; and removed workflow repair succeeds with an object tombstone plus `workflow_id`.

Final validation uses the crate guide’s commands and the configured `scripts/jig` gates through the freshly built development binary. The final comprehensive review must have a verified fingerprint and no actionable in-scope findings from completed reviewers.

## Idempotence and recovery

Tests use generic temporary repositories. The new marker is additive ignored runtime state and is published only after a valid ledger. If marker publication fails after ledger publication, the ledger itself still establishes the required expectation on the next run. If a behavior step cannot be proven, stop at the preceding green state. Never rewrite append-only `.agent/state/*.jsonl`; Jig commands may append receipts normally.

## Interfaces and dependencies

`cap-std` is added as a private implementation dependency for capability-rooted prompt opening on the repository's supported Linux and macOS hosts. No public API, unsafe code, async runtime, feature, ABI, or MSRV change is planned. Keep Rust 1.88 and edition 2024 compatibility. All new types remain crate-internal. Preserve the schema-version-3 schedule JSON and schema-version-1 command/evidence contracts.

Review round continuation: research resolved the three open questions without new defects. Address reserved-worktree stale recovery, manual late evidence, and observed/resulting PR attempt identity with centralized owner/state transition helpers and focused regression tests; keep unrelated pre-existing timeout failures out of branch scope.
