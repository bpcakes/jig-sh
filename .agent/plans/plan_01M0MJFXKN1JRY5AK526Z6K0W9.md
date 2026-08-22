# Unify execution policy boundaries

This work closes the remaining review findings in the execution-supervision changes. The observable goal is that every repository-owned external command uses one typed supervision boundary, every caller chooses output authority explicitly, cancellation stops work before the next unit starts, noisy output cannot erase structural progress, and scheduler/setup lifecycle reports remain truthful.

## Progress

- [x] Reviewed the merged Claude/Codex findings and grouped them by ownership boundary.
- [x] Made Codex transcript authority explicit so verbose refinement diagnostics truncate without terminating the edit-authoritative worker.
- [x] Introduce a typed repository command-supervision result and migrate configured commands, native schema dumps, and GitHub loop commands.
- [x] Check cancellation at orchestration boundaries before native work starts.
- [x] Separate structural progress capacity from lossy child-output preview capacity.
- [ ] Remove zero-plan status work, preserve concurrent panic diagnostics, and make setup phases monotonic.
- [ ] Run focused checks and every configured repository gate through the development binary.

## Surprises & Discoveries

- `output_schema.is_some()` currently decides whether worker stdout/stderr is authoritative, but refinement is schema-less while its authoritative result is the edited worktree.
- The native `schema_check` path invokes the configured schema dump with raw `Command::output`, so classifying the outer tool as native bypasses timeout, output, cancellation, and process-tree guarantees.
- The GitHub loop branch accepts an execution observer at the orchestration boundary but does not pass it to its blocking `gh` subprocesses. Because the CLI now records terminating signals for cooperative cleanup, those subprocesses can delay signal redelivery indefinitely.
- CLI structural events and child output share one append-only byte budget, allowing lossy preview data to evict the phase lifecycle that the feature exists to report.

## Decision Log

- Treat the findings as an execution-policy abstraction defect with smaller lifecycle omissions, not as independent typos.
- Represent repository command cancellation as typed stages until orchestration chooses receipt and stop behavior; translate other supervision failures into context-rich execution failures at the repository boundary.
- Select worker transcript overflow behavior explicitly from result authority. Do not infer it from schema presence.
- Keep `jig-owned-process` repository-agnostic. Repository timeout, progress, and error translation belong in `crates/jig/src/execution.rs`.
- Preserve the documented fatal 4 MiB policy for authoritative configured-command output; configurability is a product choice, not a correctness repair in this plan.
- Give structural progress a distinct bounded allocation rather than letting arbitrary output consume it.

## Outcomes & Retrospective

Pending implementation and final verification.

## Context and orientation

`crates/jig-owned-process/src/process.rs` owns platform process trees and bounded drains. `crates/jig/src/execution.rs` adapts that generic crate to repository execution events and cancellation. `runtime/tool_execution.rs` runs contract tools, `policy.rs::schema_check` runs a configured schema dump, `runtime/loops/github.rs` runs GitHub CLI probes, and `runtime/worker_runner.rs` runs Codex workers. `progress.rs`, `status.rs`, and `cli/setup_run.rs` own the remaining presentation and lifecycle findings.

## Plan of work

First, replace the schema-presence inference in worker execution with an explicit transcript overflow policy. Refinement will select bounded truncation because edits are authoritative, while schema-less stdout-authoritative workers retain fatal overflow. Ensure a verbose refinement still records both worker and iteration evidence.

Second, add a repository-level supervised-command adapter in `execution.rs`. It will apply the configured timeout, standard capture limits, progress/cancellation observation, and typed cancellation stages while keeping the generic owned-process crate independent. Migrate configured commands to it, then migrate the nested native schema dump and GitHub CLI calls. Add cancellation checks before each work-check item and loop tick so native work cannot begin after cancellation.

Third, split CLI progress into bounded structural and output-preview storage. Preserve phase and heartbeat lines after output truncation, retain deterministic rendering, and keep total memory bounded.

Fourth, short-circuit status gate collection when there are no open plans, retain every provider panic diagnostic, and derive setup phase positions from the actual marketplace registration count.

Finally, run focused tests after each slice, build the development Jig binary, run format, Clippy, contract, and the complete configured test gate, inspect fresh evidence, update this plan, and close structured work.

## Concrete steps

1. Implement explicit worker transcript policy; run worker/review tests; commit.
2. Implement the repository execution adapter and migrate configured/native/GitHub paths with cancellation regressions; run execution, work, loop, and policy tests; commit in coherent migration slices.
3. Implement structural progress reservation and regressions; run progress and CLI tests; commit.
4. Implement status/setup lifecycle fixes and regressions; run status/setup tests; commit.
5. Build `target/debug/jig`; run all checks through `JIG_DEV_BIN=target/debug/jig`; record gates/evidence and finish the plan.

## Validation and acceptance

Acceptance requires: verbose refinement truncates diagnostics without terminating or losing iteration evidence; schema dumps and `gh` children honor cancellation, timeout, bounded capture, and owned-tree cleanup; cancellation prevents later native gates/ticks from starting; structural phase lines survive a full output preview; empty-plan status avoids gate fingerprint work; all concurrent panic identities are reported; setup phase positions are monotonic; and all configured repository gates pass.

## Idempotence and recovery

Source/test changes are repeatable. Process regressions use temporary generic fixtures and must reap descendants on failure. `.agent/state/*.jsonl` remains append-only; failed checks are preserved as receipts. If a slice fails, repair that slice without rewriting prior commits or durable records, rebuild the development binary, and rerun the affected gate.

## Interfaces and dependencies

No new dependency is required. `jig-owned-process` retains its public generic API. The new repository-level execution result remains crate-private. Existing CLI, MCP, receipt, configuration, and JSON shapes remain compatible; only incorrect cancellation, supervision, and progress behavior changes.
