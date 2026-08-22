# Harden review-discovered lifecycle boundaries

This work classifies and fixes the current comprehensive-review findings at the abstractions that own them. The observable outcome is that progress shutdown cannot retain the process-wide stderr lock, a pushed PR repair is durably represented even if cancellation arrives immediately afterward, ambiguous GitHub mutations are reconciled, runtime commands have an explicit signal policy, configured commands have an explicit output-capture budget, and the affected tests assert semantic behavior.

## Progress

- [x] Read the repository and crate ownership guides and register structured work.
- [x] Make progress delivery abandonable without retaining shared stderr ownership.
- [x] Model PR repair commit boundaries and reconcile ambiguous GitHub mutations.
- [x] Make signal supervision an exhaustive runtime-command policy.
- [x] Move configured-command output capacity into validated repository configuration.
- [x] Restore semantic test coverage and exact launcher argument coverage.
- [x] Run all configured gates, inspect evidence, and close the work.

## Surprises & Discoveries

- The findings share a contract-shape problem: timeout, cancellation, and capture limits exist, but the interfaces still let callers forget the post-timeout or post-commit obligation.
- The existing `Fix comprehensive review findings` plan is historical and its body describes an earlier launcher-hardening pass, so this pass uses a new plan with precise acceptance criteria.
- A production-binary test with a deliberately constrained stderr pipe reproduces the former shutdown hang and proves later error handling does not write behind an abandoned progress delivery.
- The same missing-policy pattern appeared in signal handling and output capture; exhaustive command policy and typed capture capacity make those choices visible at call sites.
- Nested production-binary tests inherited `JIG_REPO_ROOT` and `JIG_INVOKE_CWD` from the outer `scripts/jig` launcher. That made the progress regression test recursively execute the repository test command instead of its isolated fixture. Removing launcher context at the subprocess boundary fixed the test harness rather than weakening its timeout or assertions.

## Decision Log

- Preserve native signal handling for runtime commands that do not implement cooperative cancellation; select cooperative supervision through an exhaustive match over `RuntimeCommand`.
- Treat the successful push as the PR repair commit boundary. Cancellation after that boundary must return and persist a completed action that describes incomplete follow-up work.
- Give GitHub reply mutations a deterministic hidden marker and reconcile both reply and resolve mutations after any ambiguous command failure.
- Keep bounded internal protocol commands on a conservative fixed capture limit, but make configured repository commands use a validated repository-level limit.

## Outcomes & Retrospective

The findings were not independent typos, but neither did they require replacing the repository's architecture. They were instances of one deeper boundary-completeness problem: abstractions represented the happy-path operation without owning the policy that becomes mandatory at timeout, cancellation, external commit, signal delivery, or output exhaustion. The fixes move those policies into exhaustive or typed owners: an independent stderr handle plus explicit abandonment, a durable post-push outcome, idempotency markers plus reconciliation, an exhaustive runtime signal policy, and a validated configured-command output budget. This reduces the bug surface by making callers unable to silently omit the exceptional-path obligation.

Each concern landed as a separate implementation commit with focused regression coverage. The final development binary passed the configured contract and full test gates, including 2,220 non-Vault tests and the Vault-backed partition. `scripts/jig check fmt`, `scripts/jig check clippy`, and the final freshness check also passed.

## Context and orientation

The main implementation lives in `crates/jig`. Relevant boundaries are `src/progress.rs`, `src/cli/run.rs`, `src/runtime/loops/pr_manager.rs`, `src/execution.rs`, `src/context/execution_config.rs`, and `src/runtime/tool_execution.rs`. The Vault TUI integration test is in `crates/jig/tests/vault_tui_unix.rs`.

## Plan of work

Implement each independent concern as a reviewable commit with focused tests. Do not rewrite append-only `.agent/state/*.jsonl`; include structured-work receipts only as newly appended records. After each slice, run its narrow tests. After all slices, rebuild the development binary, run `scripts/jig work check`, all configured gates, evidence and receipts inspection, and finish the work.

## Concrete steps

1. Replace background writes through `std::io::Stderr` with an independently owned OS handle and record presentation abandonment so later optional stderr writes do not wait behind an abandoned writer.
2. Refactor PR manager outcomes around the push commit point. Persist the final head and action before honoring later cancellation. Validate GraphQL payloads and reconcile ambiguous reply/resolve outcomes with cancellation-independent reads.
3. Add an exhaustive signal-policy method to runtime commands and install the cooperative signal session only for commands whose handlers consume the observer.
4. Add a validated `execution.command_output_limit_bytes` contract field for configured commands while retaining a private bounded limit for Git/GitHub protocol commands.
5. Restore the Vault activity assertion to the operation-specific event and strengthen the refinement stub argument assertion.

## Validation and acceptance

Focused unit and integration tests must prove each regression. `scripts/jig work gates --plan-id plan_01M0N21F70JKR13H44X7KYSA0R` must pass with `JIG_DEV_BIN=target/debug/jig`, and the final diff and receipts must contain no private fixture identifiers.

## Idempotence and recovery

All code edits are ordinary Git changes and each slice is committed separately. Structured state is append-only. GitHub reconciliation queries are read-only and reply idempotency markers are deterministic for a thread and pushed head, so retrying reconciliation does not create a second reply.

## Interfaces and dependencies

No new external service is introduced. Platform stderr-handle duplication uses existing platform dependencies. Configuration changes must update native defaults, serialization tests, templates, and generated contract fixtures together.
