# Correct dev stop recovery accounting

This work corrects two findings from the review of `origin/master...HEAD`: the destructive `jig dev stop --forget-ambiguous-orphans` help omits one class of state it can forget, and `stopped_apps` can count apps whose orphan metadata was retired without any process signal. The observable result is accurate safety guidance and stop reports whose process count reflects supervisor-driven stopping rather than unrelated registry retirement.

## Progress

- [x] Read repository and crate guidance, reproduce the reviewed scope, and inspect the relevant history.
- [x] Run the Fowler Rust refactoring heuristic scanner and manually validate its relevant candidates.
- [x] Establish the documented pre-change baseline.
- [x] Clarify the destructive repair option and add CLI help coverage.
- [x] Extract app-stop accounting without changing behavior.
- [ ] Exclude explicit orphan recoveries from `stopped_apps` and add deterministic regression coverage.
- [ ] Run all configured gates, record evidence, close the plan, and leave a clean worktree.

## Surprises & Discoveries

- The app count predates orphan recovery. Recovery later added a second way for a target session to disappear, but the count still equates disappearance with a supervisor-stopped process.
- `OrphanRecoveryNotice` already carries the exact recovered session ID, so the correction needs no new persisted schema, public API, process probing, or concurrency mechanism.
- The scanner flagged the large management module and transparent records, but those size and DTO signals do not establish a useful broader refactor for this bug.
- The pre-change `fmt`, `clippy`, `contract`, and full `test` Jig checks all passed, providing a green baseline at commit `1a0d1f7`.

## Decision Log

- Treat the help defect as a local omission. Keep the policy implementation authoritative in `jig-dev-proxy`; add a rendered-help regression test instead of coupling the CLI crate to runtime policy strings.
- Treat stop accounting as a structural defect. First apply Fowler's **Extract Function** to centralize the existing derived count, then make the behavior change separately by supplying explicit recovery outcomes to that function.
- Preserve the current JSON schema and `stopped_sessions` semantics. Only `stopped_apps` is proven incorrect because recovery guarantees that Jig did not signal persisted app PIDs.
- Use deterministic unit coverage for accounting rather than a timing-dependent process-liveness race.

## Outcomes & Retrospective

Pending implementation and verification.

## Context and orientation

The CLI option is declared in `crates/jig/src/cli/proxy.rs`, with rendered help tests in `crates/jig/src/cli/help_tests.rs`. Stop orchestration and `StopReport` live in `crates/jig-dev-proxy/src/dev_sessions/management.rs`; lifecycle integration coverage lives in `crates/jig-dev-proxy/src/lib/dev_lifecycle.rs`.

`stopped_apps` starts from the number of target app identities that might be alive, then currently sums every target whose registry record is absent from the final snapshot. A dead-orphan recovery also removes the registry record, but deliberately sends no signal to stored PIDs. `OrphanRecoveryNotice.session_id` distinguishes this outcome.

Compatibility constraints are the Rust 2024 workspace with Rust 1.85 MSRV, existing JSON field names, append-only agent state, and the cross-platform process-safety behavior documented by the proxy crate. No public Rust signature, persistent state format, unsafe code, FFI, async behavior, or dependency should change.

## Plan of work

1. Capture baseline results using the repository-defined Jig checks with the freshly built development binary.
2. Update the CLI help to name both unconfirmed preflight cleanup and unprovable spawn history; test the rendered `jig dev stop` help; run the Jig crate's narrow test; commit.
3. Extract the existing stopped-app calculation into one private function without changing inputs or semantics; run proxy tests; commit.
4. Pass explicit recovery notices into the extracted query and exclude those session IDs; add a deterministic test covering stopped, recovered, and remaining sessions; run proxy tests; commit.
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
- A recovered orphan session contributes zero to `stopped_apps`, while a removed non-recovery target retains its initial maybe-live app count and a remaining target contributes zero.
- `scripts/jig check test`, `check fmt`, `check clippy`, and `check contract` all pass with the current development binary.
- `git diff --check` passes and the worktree is clean after separate commits.

## Idempotence and recovery

All source edits are local and independently committed. If a slice fails its narrow check, amend only that uncommitted slice or stop at the preceding green commit. Jig state is append-only: rerunning a check creates another receipt and must not rewrite earlier events. Use the explicit plan ID for every work command so evidence remains attributable.

## Interfaces and dependencies

No dependency or external service changes. The internal source of recovery identity is `OrphanRecoveryNotice.session_id`; the public JSON interface remains `StopReport` with `matched_sessions`, `stopped_sessions`, `stopped_apps`, `sessions`, `recoveries`, and `warnings`.
