# Correct dev stop recovery accounting

This work corrects two findings from the review of `origin/master...HEAD`: the destructive `jig dev stop --forget-ambiguous-orphans` help omits one class of state it can forget, and `stopped_apps` can count apps whose orphan metadata was retired without any process signal. The observable result is accurate safety guidance and stop reports whose process count reflects supervisor-driven stopping rather than unrelated registry retirement.

## Progress

- [x] Read repository and crate guidance, reproduce the reviewed scope, and inspect the relevant history.
- [x] Run the Fowler Rust refactoring heuristic scanner and manually validate its relevant candidates.
- [x] Establish the documented pre-change baseline.
- [x] Clarify the destructive repair option and add CLI help coverage.
- [x] Extract app-stop accounting without changing behavior.
- [x] Exclude metadata-only retirements from `stopped_apps` at the control-phase boundary and add deterministic regression coverage.
- [x] Run all configured gates and record evidence.
- [x] Close the plan and leave a clean worktree.

## Surprises & Discoveries

- The app count predates orphan recovery. Recovery later added a second way for a target session to disappear, but the count still equates disappearance with a supervisor-stopped process.
- The existing phase boundary is stronger than filtering `OrphanRecoveryNotice` IDs: counting before manual retirement excludes recovery and any future metadata-only retirement path by construction.
- The scanner flagged the large management module and transparent records, but those size and DTO signals do not establish a useful broader refactor for this bug.
- The pre-change `fmt`, `clippy`, `contract`, and full `test` Jig checks all passed, providing a green baseline at commit `1a0d1f7`.
- The final default-parallel suite twice exposed an unrelated load-sensitive bootstrap test that passed alone. Running the same complete configured suite with `NEXTEST_TEST_THREADS=1` passed and produced the fresh required test receipt; no bootstrap source was changed in this work.

## Decision Log

- Treat the help defect as a local omission. Keep the policy implementation authoritative in `jig-dev-proxy`; add a rendered-help regression test instead of coupling the CLI crate to runtime policy strings.
- Treat stop accounting as a structural defect. First apply Fowler's **Extract Function** to centralize the existing derived count, then move that query to the boundary after authenticated control retirement and before manual metadata retirement.
- Preserve the current JSON schema and `stopped_sessions` semantics. Only `stopped_apps` is proven incorrect because recovery guarantees that Jig did not signal persisted app PIDs.
- Use deterministic unit coverage for accounting rather than a timing-dependent process-liveness race.

## Outcomes & Retrospective

The safety-sensitive CLI help now names both kinds of ambiguity the repair flag can forget. Stop accounting is centralized in `count_stopped_apps`, and its input is captured at the authenticated control-retirement boundary, so later metadata-only orphan or stale-record cleanup cannot claim that Jig stopped app processes. The public JSON and persisted-state contracts are unchanged.

The implementation was delivered as separate help, behavior-preserving refactor, and behavior-fix commits. The rebuilt development binary passed formatting, Clippy, contract, all 565 proxy tests, the focused CLI test, and the complete configured workspace suite. The Jig work gates report fresh passing contract and test evidence.

## Context and orientation

The CLI option is declared in `crates/jig/src/cli/proxy.rs`, with rendered help tests in `crates/jig/src/cli/help_tests.rs`. Stop orchestration and `StopReport` live in `crates/jig-dev-proxy/src/dev_sessions/management.rs`; lifecycle integration coverage lives in `crates/jig-dev-proxy/src/lib/dev_lifecycle.rs`.

`stopped_apps` starts from the number of target app identities that might be alive, then originally summed every target whose registry record was absent from the final snapshot. A dead-orphan recovery also removes the registry record, but deliberately sends no signal to stored PIDs. The corrected boundary counts only target records retired before manual orphan/stale cleanup begins.

Compatibility constraints are the Rust 2024 workspace with Rust 1.85 MSRV, existing JSON field names, append-only agent state, and the cross-platform process-safety behavior documented by the proxy crate. No public Rust signature, persistent state format, unsafe code, FFI, async behavior, or dependency should change.

## Plan of work

1. Capture baseline results using the repository-defined Jig checks with the freshly built development binary.
2. Update the CLI help to name both unconfirmed preflight cleanup and unprovable spawn history; test the rendered `jig dev stop` help; run the Jig crate's narrow test; commit.
3. Extract the existing stopped-app calculation into one private function without changing inputs or semantics; run proxy tests; commit.
4. Evaluate the extracted query immediately after the authenticated control-retirement wait and before manual orphan/stale retirement; add a deterministic test naming that phase contract; run proxy tests; commit.
5. Rebuild the development binary, run work checks and configured gates, inspect receipts and the final diff, close structured work, and commit generated evidence separately.

## Concrete steps

Run from `/Users/aa/Documents/jig-sh`:

1. `cargo build -p jig-sh --bin jig`
2. `JIG_DEV_BIN=target/debug/jig scripts/jig check fmt --plan-id plan_01M0D7TDA4M0D1QV30C31AZ9Y2`
3. `JIG_DEV_BIN=target/debug/jig scripts/jig check clippy --plan-id plan_01M0D7TDA4M0D1QV30C31AZ9Y2`
4. `JIG_DEV_BIN=target/debug/jig scripts/jig check contract --plan-id plan_01M0D7TDA4M0D1QV30C31AZ9Y2`
5. `JIG_DEV_BIN=target/debug/jig scripts/jig check test --plan-id plan_01M0D7TDA4M0D1QV30C31AZ9Y2`
6. Apply each slice, format it, run its narrow crate test, inspect the staged diff, and commit it independently.
7. Repeat steps 1 through 5 at the end, then run `scripts/jig work check`, `work gates`, `work evidence`, `work receipts`, and `work finish` for this plan with `JIG_DEV_BIN` set.

## Validation and acceptance

- Rendered `jig dev stop` help names both ambiguity classes and retains the no-signal warning.
- Existing stop reports are unchanged by the extraction commit.
- A target still registered after the authenticated control phase contributes zero to `stopped_apps`, even when later recovery removes its metadata; a target retired during that control phase retains its initial maybe-live app count.
- `scripts/jig check test`, `check fmt`, `check clippy`, and `check contract` all pass with the current development binary.
- `git diff --check` passes and the worktree is clean after separate commits.

## Idempotence and recovery

All source edits are local and independently committed. If a slice fails its narrow check, amend only that uncommitted slice or stop at the preceding green commit. Jig state is append-only: rerunning a check creates another receipt and must not rewrite earlier events. Use the explicit plan ID for every work command so evidence remains attributable.

## Interfaces and dependencies

No dependency or external service changes. The internal accounting boundary is the target-session snapshot taken after the authenticated stop wait and before manual metadata retirement; the public JSON interface remains `StopReport` with `matched_sessions`, `stopped_sessions`, `stopped_apps`, `sessions`, `recoveries`, and `warnings`.
