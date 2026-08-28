# Harden repository action boundaries

This plan closes the comprehensive-review findings on branch `feat/go-backend-support`. It first separates behavior-preserving restructuring from bug fixes, then adds regression coverage at the public CLI and MCP boundaries.

## Progress

- [x] Confirm the pre-fix commit `c4d73ec` is clean and pushed to `origin/feat/go-backend-support`.
- [x] Read repository guidance and the Fowler Rust refactoring protocol.
- [x] Record the pre-change baseline and validate each review finding against current callers.
- [x] Refactor shared boundary types and execution policy without changing observable behavior.
- [x] Fix confirmed defects and add regression tests.
- [x] Run repository checks and inspect receipts and the final diff; commit and push during handoff.

## Surprises & Discoveries

- The requested pre-fix commit and push were already complete when this work began; upstream and `HEAD` both pointed to `c4d73ec`.
- The heuristic refactoring scanner produced 200 truncated candidates, mostly test `unwrap` calls and DTO/public-field signals. Those are not findings by themselves and are excluded unless they explain one of the reviewed defects.
- The bounded process runner does not implicitly pipe child streams. Repository command execution had requested bounded capture while inheriting stdout and stderr, so output could escape receipts; the shared execution boundary now pipes explicitly and rejects incomplete captures.
- The initial full `jig-sh` baseline was accidentally contended by other Cargo test processes. Its timing- and signal-sensitive failures were not used as evidence; the final full suite is run alone.

## Decision Log

- Treat the findings as two design clusters plus several local omissions. The design clusters are (1) external JSON represented directly with a structured map-key type, and (2) native execution bypassing the bounded process policy used by command actions.
- Use Fowler's **Split Phase**, **Replace Primitive with Object**, and **Move Function** to create narrow conversion/execution seams before changing behavior.
- Keep wire formats and public command behavior compatible unless the current format is unusable; cover any corrected behavior with an end-to-end test.
- Do not broaden this work into unrelated scanner cleanup.
- Classify schema validation as a worktree effect because the configured dump command may update generated files; independently enforce every declared read-only action by comparing the post-action worktree fingerprint with the plan source identity.
- Keep execution deterministic and sequential within each dependency layer, and correct the documentation instead of adding unproven concurrency to a correctness-focused fix.
- Reconcile orphaned asynchronous runs with per-run advisory worker leases stored in the ignored `.agent/.cache` tree; inspection may append a terminal blocked result when the lease proves the worker disappeared.

## Outcomes & Retrospective

The review findings were a mix of two boundary-design weaknesses and local omissions. External target identifiers now remain strings on the MCP wire and convert once into domain `TargetId` values. Native and command actions now share cancellation, timeout, owned-process cleanup, bounded complete output capture, and post-action effect enforcement. Per-run leases make orphan recovery durable instead of relying on the in-process worker registry.

The remaining correctness fixes preserve existing architecture: global check flags normalize around external selectors, legacy and evidence checks aggregate failures, parser success is represented out of band, stale guidance was corrected, and scheduling documentation now matches deterministic sequential execution. Regression coverage exercises every reviewed boundary, including non-empty MCP arguments, worktree mutation, native cancellation/timeout/cleanup, mixed gates, CLI flags/help, parser-source collisions, output capture, and orphan reconciliation.

Validation passed with `cargo fmt`, focused crate tests, Clippy with warnings denied for `jig-sh` and `jig-sqlx`, agent guide/map checks, and fresh plan-bound `jig.contract_check` and `jig.test` receipts. The full test receipt covered 2,284 tests across 25 binaries.

## Context and orientation

Repository action types live in `crates/jig-contract/src/repository.rs`; MCP request DTOs and dispatch live under `crates/jig/src/tool_defs/` and `crates/jig/src/runtime/mcp_repository.rs`; planning and execution live under `crates/jig/src/repository/` and `crates/jig/src/runtime/run_execution.rs`. CLI normalization is in `crates/jig/src/cli/command_conversion.rs`, mixed checks are in `crates/jig/src/runtime/work/checks.rs`, and SQLx action metadata is in `crates/jig-sqlx/src/lib.rs`.

Compatibility-sensitive surfaces are MCP JSON schemas, `.agent/state/*.jsonl`, CLI ordering/error behavior, action effects, cancellation/timeout semantics, and process-tree cleanup. The workspace uses Rust edition 2024 with Rust 1.85 as its repository MSRV; generated Rust/React scaffolds have their own Rust 1.94 requirement.

## Plan of work

1. Add characterization tests for the current valid MCP no-argument path, command execution, CLI global flags, mixed legacy/evidence checks, and result-parser outcomes.
2. Introduce an explicit wire-to-domain conversion for action argument maps. Keep `TargetId` as the domain key and parse string keys at the MCP boundary.
3. Extract a common execution outcome/policy seam so native and command runners share cancellation, timeout, bounded-output, and parser-status semantics. Where a native action launches a process, route it through `jig-owned-process` rather than `Command::output`.
4. Correct SQLx schema validation effects or make the check provably non-mutating; choose based on the configured schema command contract and existing callers.
5. Correct global option normalization, mixed-check failure aggregation, parser status representation, stale `jig run` guidance, and concurrency documentation/behavior.
6. Add a durable recovery or explicit reconciliation path for repository runs whose in-process worker disappears, without rewriting append-only history.

## Concrete steps

- Run `cargo fmt --all -- --check`, relevant crate tests, and existing Jig checks before edits.
- After each structural step, run the narrow unit or integration test for the touched module.
- Build `target/debug/jig` before every dogfood validation and set `JIG_DEV_BIN=target/debug/jig` for `scripts/jig` commands.
- Keep append-only `.agent/state/*.jsonl` changes generated by Jig commands only.

## Validation and acceptance

- MCP plan requests with non-empty action arguments deserialize and validate through the published schema.
- A read-only action cannot mutate the worktree without policy approval.
- Native execution honors cancellation, timeout, output caps, and owned process cleanup.
- Mixed legacy and evidence checks both run and aggregate failures.
- Global flags and help work before and after external selectors.
- A tool-provided finding source cannot spoof parser success/failure.
- User guidance names an available execution path.
- Concurrency documentation matches implementation.
- Interrupted/orphaned repository runs have a tested recovery path.
- `cargo fmt`, relevant Clippy checks, `scripts/jig check test`, `scripts/jig check contract`, and required work gates pass.

## Idempotence and recovery

All source edits are ordinary Git changes. If a step fails, restore only that step's patch while preserving earlier green changes and append-only Jig state. Re-running tests and Jig checks is safe; plan/session/receipt records append rather than overwrite.

## Interfaces and dependencies

Prefer existing `jig-owned-process` APIs and existing contract types. Do not add a dependency unless the current crates cannot express the required bounded execution or serialization conversion. Preserve public serde names and persisted record schemas unless a migration is explicitly documented and tested.
