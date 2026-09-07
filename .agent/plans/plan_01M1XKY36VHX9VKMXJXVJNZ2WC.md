# Foreground repository action execution

Implement Bead `jig-sh-generic-monorepo-zac.3.1` at Git baseline
`4ef2e90280d053a6fc5a38477429dd3c0a61de44`. A user can run declared actions
through `jig run`, inspect a plan without executing it, explicitly approve its
worktree/external effects, and interrupt execution without orphaned children.

## Progress

- [x] Audit current task and source: generic planning and durable execution exist;
  the foreground CLI is missing. Bead claimed and structured work opened.
- [x] Add initial CLI/request wiring and shared effect/cancellation helpers.
- [x] Complete command discovery, docs and regression tests.
- [x] Build development binary and pass applicable gates and backend tests.
- [x] Run two comprehensive review rounds and address findings.
- [x] Commit the verified foreground-run implementation.

## Surprises & Discoveries

The earlier backlog audit remains uncommitted in metadata and AGENTS.md. Preserve
it. Its full test profile recorded a scheduled-worker lease timing failure, which
passed in isolation; do not claim that earlier failure was fixed by this task.
`jig check` uses a check-only planner and loses native work-plan identity in one
path. Generic run must use `plan_action_run` and carry the supplied identity;
targeted retry repairs remain Bead `jig-sh-qh4`.

## Decision Log

- Keep this an additive CLI change within contract v6/v7. Reject older contracts
  with migration guidance. Do not introduce declared arguments or argv runners.
- Use the existing execution engine and observer for target outcomes and owned
  process cleanup. Share exact effect approval and durable cancellation with MCP.
- CLI waits cooperatively for repository execution ownership; MCP retains its
  nonblocking acquisition so its request loop can service cancellation.

## Outcomes & Retrospective

Implementation and verification are complete. The history below records the
validation and review iterations; the final result appears at the end.

A 23-test focused run passed (Nextest
`4b6f7459-3073-43b8-a689-338d6a34c7cd`). A later 10-test CLI/runtime run passed,
including v6 command and v7 native work-plan receipts and real CLI JSON failure
output (Nextest `a3dabd66-4b3c-452d-a66e-2d2bf039683b`). Durable-request cancellation
coverage was added after that run and is included in the running full gates.
`bash scripts/check-launcher-template.sh` passed. The built dev binary exposes
`run --help` through the launcher. Command inventory is schema v4 because the new
`repository_contract_upgrade_required` reason extends the stable reason enum.

The first full `work check` ended with status 1 because of the core-partition
receipt below. Session 77679 is terminal. Completed passing receipts:
`receipt_01M1XMQCQBA7TSCT4H776NYE8Q` (contract),
`receipt_01M1XMQG24NJ3CHT3EME8ZS46B` (format), and
`receipt_01M1XMRF1R78J67HT5KH7S2ZWT` (Clippy). The core partition failed after 2,576 passing tests because an unchanged evidence
fixture could not safely clean up `git diff --cached --binary` during fingerprint
collection: receipt `receipt_01M1XN79VE2FCG54BN3XTTRDQZ`, Nextest
`9ff88e37-9759-4a64-be93-d9363c183cdd`. The exact test
`runtime::tests::work::evidence::target_evidence_gate_ignores_success_from_an_unrelated_target`
passed unchanged in a single-test rerun (11.229 seconds; Nextest
`02823f7e-96b3-46c7-bb11-3c04e2746a76`). That narrow pass does not satisfy the
failed core gate; rerun the full core partition after the current verify profile.
Frontend (`receipt_01M1XND6JBNFJBM1MR79FKPCYR`), vault
(`receipt_01M1XNEFDJ0KWAV3REY7B6V70T`) and process
(`receipt_01M1XNFVBWVH7DRM1KKKD0FCS4`) partitions passed. The verify profile subsequently passed all targets, including 3,862 tests with
two skips in 853.682 seconds. Its formatting, Clippy, contract and file-budget
targets passed as well. The failed core gate still requires a successful full
partition rerun during final validation. Comprehensive review round one is next;
no review or commit has occurred yet.

## Context and work

CLI options live in `crates/jig/src/cli/repository_run.rs`; neutral requests in
`command/repository_run.rs`; orchestration in `runtime/repository_run.rs`.
`repository/planner.rs` owns immutable plans (resolved targets and source identity).
`runtime/run_execution.rs` owns leases, execution, terminal results and receipts.
`runtime/run_cancellation.rs` shares incremental durable cancellation polling.
Update `root_commands.rs`, launcher/template command scope and command discovery
alongside the new parser, then document usage in README and public runtime docs.

## Validation and acceptance

Add regressions for CLI parsing, non-check actions, CLI/MCP equivalent plans and
outcomes, exact approvals, explain leaving state untouched, work-plan identity,
selection modes, failure/fail-fast, and cancellation of running/unstarted targets.
Use generic temporary repositories and executable fixtures only.
Run focused tests, `cargo build -p jig-sh --bin jig`, then
`JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M1XKY36VHX9VKMXJXVJNZ2WC`.
Inspect `work gates`, `work evidence`, and `work receipts` for that plan.
Finish backend validation with `JIG_DEV_BIN=target/debug/jig scripts/jig check test`.
Run the comprehensive-review skill on the frozen working tree before committing;
address findings and run at most one further review round. Record actual results.

## Recovery and durable state

No persisted schema changes are intended. Keep state journals append-only. After
an interrupted test, inspect its live process before restarting. Rebuild the dev
binary after source changes before relying on harness receipts. Sync Beads after
mutations. Do not close the Bead or structured work until evidence proves completion.

## Comprehensive review, round one

Claude and Codex completed independent read-only reviews on fingerprint
`806a00e15b4bf141c38a5f1330a68a35fcbe986972527009dec0eac4c6097555`;
all parent/reviewer captures matched and were complete. Claude attested all 45
paged evidence files; page access does not prove review quality. Codex reported
no actionable findings. Claude reported five findings: redundant internal-plan
validation, missing pre-v7 comparison rejection, caller-free approval diagnostics,
undocumented default-profile selection, and workstation paths in Beads exports.

Fixes reuse the existing freshly planned execution helper with optional durable
cancellation, retain config/first-target source validation and the empty-plan
source check, reject pre-v7 comparisons before planning, label approval errors by
caller, and document/test the default profile. Affected coverage now executes the
selected target as well as comparing explain plans. Workstation source_repo_path
metadata was cleared through br across existing records; no historical state
journal was edited. The existing v7 native regression executes strict-inventory
comparison authority, rather than only parsing a flag.

Residual test gaps: no new direct OS-signal, conflicting-lease wait, or parallel
durable-cancellation tests for the added CLI. These reuse existing observer and
execution infrastructure. The cancellation fixture waits for explicit child
output before requesting cancellation and uses a fresh isolated repository;
review raised a possible writer-lock contention concern but established no race
or failing regression. Required action arguments remain a separate task and are
not newly introduced here. Final gate verification and round two remain pending.

The previously passing full verify run is `run_01M1XNG8TH65CAJ8B8PRWSTH8S`,
with full-test receipt `receipt_01M1XPB9GJKS28ZXVHRQWPFCHY` (3,862 passed,
two skipped). It predates the review fixes and does not validate them.

## Review-fix validation

The focused foreground suite passed all ten tests (Nextest
`9b2776c0-8ffd-4b86-a9b9-763e45d9563e`). A subsequent development build passed
without warnings after preserving the non-cursor start path for existing check
callers. `git diff --check`, `scripts/jig check agent-map`, and
`scripts/jig check agent-guides` passed through the development binary.

The second full work check completed successfully in tool session 39079. Its contract,
format and Clippy receipts passed:
`receipt_01M1XQ7M9CBTJ0CSNQJ6DB0PTK`,
`receipt_01M1XQ7QGTQET3S5MDFC3XG40X`, and
`receipt_01M1XQ86R2S2RN1W46TTN1S2TG`.
That session is terminal; review-fix validation and commit remain. No implementation code for the adoption task has been changed.

All seven configured check gates passed in the second work check. The core
partition passed 3,096 tests (Nextest `6a6a280a-7b66-4df3-969b-669ba7a9396b`,
608.926 seconds), receipt `receipt_01M1XQVN48ZWMBHJT5FQXQ6FNK`.
Frontend, vault, and process receipts are respectively
`receipt_01M1XR20ZD3WNHE69K0QHAWSTV`,
`receipt_01M1XR3BQFBGG217SS9EG55G0B`, and
`receipt_01M1XR4QHK92V98MJ0VZ57B51V`.
The successful check batch is `receipt_01M1XR52QBRB8CJC37YN346Q1N`.
The subsequent verify profile has passed Clippy, format, contract and file budget;
its full test target passed 3,863 tests with two skips (Nextest
`3fcbe275-b227-41f9-8e01-23149ae66677`). This validates the round-one fixes,
but predates the following round-two fixes.


## Comprehensive review, round two (final permitted round)

Claude and Codex independently reviewed the frozen working-tree copy at
`/tmp/jig-foreground-review-gnrdlm3a`, HEAD `4ef2e902`, fingerprint
`055958957df80ac1914d446cd9344188ec2cf4e807e6c7272a5685a6868be9e5`.
Parent and reviewer captures were complete and unchanged. The original working
tree differed afterward only by appended runtime receipts and run records.
Claude evidence coverage was LIMITED: 76 of 77 pages were attested; page 0067
was not attested. Codex coverage was complete. No third review will be launched.

Codex identified that a durable-journal lock wait could prevent foreground
signals and supervisor deadlines from being serviced. The fix makes journal
polling nonblocking, keeps the cursor unchanged on contention, and forwards the
foreground cancellation callback into record scanning. Existing MCP private
signals and cached inspection failures remain supported.

Claude findings addressed: restrict the added pre-start cancellation rejection
to foreground runs so existing check evidence remains intact; document both
JSON command values and the fact that explain defers work-plan openness and
effect approvals to execution; use the refreshed context for both contract
guards; clear the remaining absolute workstation paths from two Beads records
through br; move durable cancellation to an independent MCP controller thread
that identifies and checks the actual started run; cover v6 and v7 command
readiness. The cancellation fixture now verifies MCP can cancel a CLI run even
though it has no MCP worker registry entry.

Focused validation initially exposed two incorrect new test setups: a legacy
fixture missing the v6 repository table, and a pre-start test cancelled at the
outer dispatch boundary instead of the execution-service boundary. Both tests
were corrected to exercise the intended contract. A real CLI regression now
holds the run journal exclusively and verifies child cleanup for SIGINT and
SIGTERM before releasing the lock. Final focused and configured checks remain
pending. These review fixes have not been reviewed in a third round.

Round-two focused results: Nextest `9510f968-5322-45f3-8c74-556bf4dc085e`
passed 14 of 15 tests, including real SIGINT/SIGTERM cleanup under an exclusive
journal lock, MCP durable cancellation, and preserved pre-start check evidence.
The one failure was the new inventory fixture invoking `info commands` instead
of `info --commands`. After correcting that invocation, Nextest
`64095bff-49b4-4967-87a8-0372a9d2907e` passed the inventory regression plus both
journal-cursor tests and the cached durable-poll-error regression (4/4).
`cargo build -p jig-sh --bin jig` passed; `git diff --check` passed.

Final work check is LIVE in tool session 57078, with output redirected to
`/tmp/jig-foreground-final-check.log`. The process tree confirms the new dev
binary is running the core Nextest partition. Poll this original handle until
terminal. Then run the explicitly required `scripts/jig check test` through
JIG_DEV_BIN, inspect gates/evidence/receipts, finish the work plan, close/sync the
Bead and commit task one. Task two implementation has not started.

Final validation update: session 57078 was deliberately cancelled (exit 130)
after tightening the new signal fixture's readiness handshake. It now waits for
a parsed positive child PID, not merely creation of the PID file. This prevents
an empty-file observation race. The corrected regression passed for both signals
in Nextest `43d0d990-3ec6-45d9-991b-3578cb766e9f` (10.274 seconds). The cancelled
core check is not final evidence. Its owned Nextest process exited after SIGINT.
A fresh sequential build/work-check/backend-test command has been started;
logs are `/tmp/jig-foreground-final-check-v2.log` and
`/tmp/jig-foreground-backend-test.log`. Do not mutate implementation files during
these final checks. Record the actual new session handle from the tool response.
The final sequential command's live tool session is 3960. It runs the backend
command only after work check succeeds; exit 0 therefore proves both completed.
On failure inspect the corresponding log and durable receipts before any retry.

Final-source check progress in session 3960: contract, format, Clippy and core
passed. Core ran 3,100 tests successfully in 598.198 seconds (Nextest
`44e2ec95-948e-452a-9264-4c34e7fc893b`), receipt
`receipt_01M1XV3SXV8ARCAX0YFKYBNX25`. The first three receipts are
`receipt_01M1XTGH5T8J19A2KQBMKPG3GT`,
`receipt_01M1XTGMHBW4Z514K9W98SXMHK`, and
`receipt_01M1XTH385JPHGTDG7RBXNQ67T`. Frontend and the remaining checks are
still running; full verify and the explicit backend command must also finish.

Session 3960 is terminal with exit 1. All seven check gates passed, including
frontend (112 passed), vault (443 + 2 passed), and process (210 passed), receipts
`receipt_01M1XV9TN41BEWH9J4HC5SZ1HA`,
`receipt_01M1XVATVRSSEX49S84TBN4CWY`, and
`receipt_01M1XVC5QE7265ASY87AMC74FG`. The successful check batch is
`receipt_01M1XVCGJ5YJ1137APRV94TG1A`.

The subsequent verify profile failed file budget before its full test target
started: `state/runs.rs` grew existing line debt by two lines (862 -> 864),
receipt `receipt_01M1XVCXQ6XQV2N98JNKYTFC8M`, run
`run_01M1XVCJVA6FWXDTH7P59W068X`. The chained explicit backend command did not
run. No test assertion failed in this work check.

Remediation moves the existing opaque run cursor, append-with-cursor helper,
and cancellation tail scan into `state/runs/cancellation.rs`, keeping the state
API unchanged. This is a cohesive extraction, not removal of comments or a
policy waiver; the parent is now 840 lines. Focused cursor/cancellation tests,
a development build and the actual foreground file-budget target are running
sequentially in session 43213. Inspect `/tmp/jig-foreground-file-budget.json`
only once that command is terminal. Then rerun final validation on this source.
No additional comprehensive-review round is authorized or needed.

The cursor extraction passed all five focused regressions (Nextest
`e25192c2-b3a9-4e60-a54f-7f3cafd83c43`), and the development build passed.
The actual `jig run repo:file-budget --plan-id ...` now succeeds, run
`run_01M1XVHPTGKW9ZGCPP2K397D91`; remaining budget findings are warnings/notices.
Session 43213 is terminal with exit 0. A new work-check/backend-test sequence is
running with log `/tmp/jig-foreground-final-check-v3.log`. Implementation source
is frozen again; scope-aware gate reuse is left to the configured work checker.
The latest sequential validation command is live in tool session 2439. Its first
command is work check; only on success does it run the explicit backend command.

Session 2439 remains live, but its core partition failed: Nextest
`86449f00-4a01-4935-ad03-9a4fa1deabe8`, receipt
`receipt_01M1XVTNFJQ5A6GSMPPYMYAK15`. The primary failure was
`policy::tests::migration_schema::schema_check_does_not_overlay_dotenv_from_a_wholly_ignored_directory`
while cloning a schema sandbox: "the process tree could not be cleaned up safely".
After fail-fast cancellation began, `git_receipts::tests::supported_gate_globs_select_the_same_tracked_diff_they_classify`
and `policy::tests::migration_schema::schema_check_isolates_unrelated_generator_writes_and_reads_untracked_inputs`
also reported cleanup failures. The partition ran 1,835 tests (1,832 passed,
three failed); 1,265 were not run. Both owning source files have no Git diff.
Do not infer three independent root causes from the cancellation fallout.
The remaining gates/profile are still being executed by the original handle.
Keep this failed receipt; later isolated passes cannot replace a complete
core-partition pass. The chained backend command will not run if work check
returns failure, even if its later full verify profile passes.

The three cleanup-failure tests passed unchanged in a focused, single-threaded
Nextest run `778185c7-bf28-4edb-9915-f41608565e43` (16.368 seconds). Session
38689 is terminal with exit 0. This establishes that the reported failure is not
reproduced by that narrow run; it does not establish a fix or replace the failed
core receipt. After session 2439 finishes its current full work-check sequence,
rerun the complete core gate and then the explicit backend command. Preserve
this failure under the existing schema/cleanup flake backlog rather than changing
unrelated runtime code as part of foreground run.

The verify profile within session 2439 passed Clippy, format, contract, and
file budget on the extracted source. Receipts are
`receipt_01M1XW3G709K3A6M1VCHW0PVBT`,
`receipt_01M1XW3GNV5FJ13ZP51HV27VZM`,
`receipt_01M1XW3H4WMM5JGW1CB81V9S45`, and
`receipt_01M1XW3HM5SNG5EP4QZ3C8HRWC`.
The full workspace test is live under the original work-check process; the core
partition's failed receipt is still unresolved. The configured gate ID for the
complete core retry is `source-rust-core-tests`; `verify` is the evidence gate.

Session 2439 is terminal with exit 1 solely because of the earlier failed core
partition. Its full verify profile passed all five targets, including 3,867
workspace tests with two skipped tests: Nextest
`4d38a256-7362-4247-94f3-d0ac3c05de3f`, full-test receipt
`receipt_01M1XWVT2XNY4RSC5G9MDHB45J`, run
`run_01M1XW36P6FF6JEVFS5GMCNEZV`. The chained backend command did not execute.
A deliberate complete retry of only `source-rust-core-tests` is now running,
followed on success by the explicit backend command. Logs are
`/tmp/jig-foreground-core-retry.log` and `/tmp/jig-foreground-backend-test.log`.
No source change occurred between the failed core partition and the passing
full-workspace run; retain the cleanup failure as unresolved historical evidence.
The core-retry/backend sequence is live in tool session 75115. The passing full
workspace run completed in 790.828 seconds; do not rerun that evidence profile
unless new source changes, a failure, or gate status requires it.

The complete core retry passed 3,100 tests in 563.627 seconds (Nextest
`ae2b3801-4d00-4c11-b986-148569cb6f61`), receipt
`receipt_01M1XXE0WA4AHXHV3GT5MS13HW`; successful batch
`receipt_01M1XXE5F05950CD8R383K91TG`. Session 75115 then started the literal
`JIG_DEV_BIN=target/debug/jig scripts/jig check test` command, which remains live.

`work gates`, `work evidence`, and `work receipts` were inspected through the
new development binary. Gates and evidence both report `overall=passed` and
`gates_ok=true`: all eight required gates are passed/fresh, with no failed,
missing, stale, unknown, or unsupported required gates. The current fingerprint
is `sha256:4d6f56d91062655f3c39c8d46ba28b118ed0a9f6a696c906e3e2cb58d007f962`.
The plan remains open until the literal backend command finishes. Read-only
inspection sessions 21293 and 59082 completed with exit 0; receipt inspection
also completed successfully. Output snapshots are in `/tmp/jig-foreground-`
`gates.json`, `evidence.json`, and `receipts.json`.


## Final result

The literal `JIG_DEV_BIN=target/debug/jig scripts/jig check test` passed all
five targets and all 3,867 workspace tests (two skipped) in 800.004 seconds:
Nextest `55cb351b-d09a-4b44-afa2-4f199d284f59`, receipt
`receipt_01M1XY74H7WSGMPH8WVWNRRCVV`, run
`run_01M1XXE80HJ99DTFC4M779J1MD`. Session 75115 is terminal with exit 0.
Together with the fresh plan-associated verify profile and the successful
complete core retry, all required validation is satisfied. The earlier cleanup
flake remains recorded; no unrelated cleanup implementation was changed.

The CLI, discovery, launcher, docs and cancellation regressions are complete.
Two review rounds were performed; the second Claude review's one-page metadata
coverage limitation remains disclosed above. No third round was performed.
This plan accompanies the foreground-run implementation commit. Adoption is the
next separate task and has not been implemented in this change.
