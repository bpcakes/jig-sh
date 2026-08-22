# Close repository execution follow-up findings

This plan applies small Fowler-style refactorings to make the reviewed execution policies explicit, then fixes the confirmed behavior defects and adds boundary-level regression coverage. The public MCP wire schema and append-only run record format remain compatible.

## Progress

- [x] Confirm `c1900d8` is clean, pushed, and synchronized with upstream.
- [x] Read repository guidance and the Fowler Rust refactoring protocol.
- [x] Establish focused baselines and run the heuristic scanner against `c4d73ec..c1900d8`.
- [x] Introduce explicit target-execution epochs and atomically publish leased runs.
- [x] Make work-check scheduling honor report-versus-collect policy and retain hard failures.
- [x] Restore the explicit schema CLI, bound every native result, and centralize check-command names.
- [x] Add regression tests, run repository gates, inspect the diff, commit, and push.

## Surprises & Discoveries

- The scanner inspected 15 changed Rust files and reported 153 heuristic candidates. Most are broad module-size, DTO-field, clone, or test-unwrap signals. Only the mode flag/data-clump signals in execution and work-check orchestration explain the reviewed defects; unrelated scanner cleanup is excluded.
- A combined Cargo command with multiple positional test filters was invalid CLI usage. The same three intended test groups pass when invoked separately; this is not a baseline defect.

## Decision Log

- Treat the findings as three design clusters plus local omissions: target execution has no per-target epoch, work-check failure mode does not own scheduling, and durable run publication is separate from lease ownership.
- Apply **Introduce Parameter Object** and **Split Phase** with a private target execution control value that owns the deadline/cancellation policy and establishes a before/after read-only fingerprint around one target.
- Apply **Remove Flag Argument** by making the existing `CheckFailureMode` determine both loop continuation and final reporting. Hard tool errors become batch failures so evidence execution and durable receipts are preserved.
- Apply **Replace Constructor with Factory Function** by returning a durable run together with its already-held lease; no nonterminal run may be published without an owner.
- Treat completion of an uninterruptible in-process native mutation as authoritative. Cancellation and timeout are safe-point checks before it starts; subprocess-backed schema validation remains cooperatively interruptible.
- Keep serialization, MCP schemas, target ordering, and append-only event records unchanged.

## Outcomes & Retrospective

The findings were a mix of design symptoms and local omissions. Per-target execution now owns one timeout/cancellation epoch and brackets read-only work with a fresh fingerprint, so earlier effectful targets cannot poison later checks. Run construction now returns an already-held lease, eliminating the queued-but-ownerless publication state, and leases are removed after release. Work-check failure mode now governs scheduling as well as reporting; collect mode retains every failure, hard execution errors receive a batch receipt, and evidence targets still run.

The narrower omissions were also closed: contract-v6 `jig check schema` uses declared-action planning, every native result is bounded at the shared execution seam, and CLI normalization consumes the canonical check-subcommand list. Regression tests cover each reviewed failure mode. Focused tests, formatting, Clippy, the full repository test command, contract validation, agent guides, and agent-map validation pass.

## Context and orientation

Repository run orchestration is in `crates/jig/src/runtime/run_execution.rs`; native operation dispatch is in `runtime/tool_execution.rs`; asynchronous MCP publication is in `runtime/mcp_repository.rs`; run persistence and leases are in `state/runs.rs`; mixed work gates are in `runtime/work/checks.rs`. CLI parsing lives in `cli.rs`, `cli/check.rs`, and `cli/run.rs`. SQLx action metadata is owned by `crates/jig-sqlx/src/lib.rs`.

The Rust contract is edition 2024 with MSRV 1.85. There is no unsafe or FFI change. Concurrency, cancellation, persistent append-only state, CLI behavior, and MCP serialization are compatibility-sensitive.

## Plan of work

1. Add characterization tests for a mixed worktree/read-only plan, post-completion cancellation, collect-mode multiple failures, hard legacy errors with evidence, explicit v6 schema CLI routing, run publication/lease cleanup, and bounded in-process native output.
2. Introduce a private execution-control value and move deadline/cancellation classification into it. Establish the read-only fingerprint immediately before and after the target rather than using the plan-time source forever.
3. Change run startup to acquire its lease before appending the queued event and return both values together. Remove the separate post-publication acquisition path and clean normal terminal lease files.
4. Refactor work-check accumulation so execution failures and nonzero results are values in a batch; collect mode continues, report mode may stop legacy scheduling but always records the batch and proceeds to evidence targets.
5. Restore the explicitly requested schema command through the effect-aware direct path, bound all native strings at their common seam, and use the tested canonical check-subcommand list in CLI normalization.

## Concrete steps

- Keep each structural extraction compiling before changing behavior.
- Run the narrow module tests after each cluster.
- Build `target/debug/jig` before dogfood checks and set `JIG_DEV_BIN=target/debug/jig`.
- Modify `.agent/state/*.jsonl` only through Jig commands.

## Validation and acceptance

- A worktree action followed by a passing read-only action succeeds while an actual read-only mutation still fails.
- A completed migration is never reported as not-started cancellation or timeout.
- Collect mode runs every configured legacy check; hard errors still produce a batch receipt and do not suppress evidence targets.
- `jig check schema` has an explicit supported behavior on contract v6.
- Every published nonterminal run holds a lease; normal completion does not leave a lease file.
- All native action output is bounded before MCP responses and receipts.
- Canonical check command names have one tested source of truth.
- Formatting, focused tests, Clippy, full `scripts/jig check test`, contract, guide/map checks, and plan-bound gates pass.

## Idempotence and recovery

Source edits are ordinary Git changes. Lease cleanup is best-effort cache cleanup; durable run state remains the source of truth. Re-running tests and checks is safe. If a structural step cannot be proven green, revert only that step and retain the prior passing seam.

## Interfaces and dependencies

Use existing `jig-owned-process`, fingerprint, receipt, and fs4 APIs. Add no dependencies. Preserve public serde names, MCP schemas, action identities, and `.agent/state/runs.jsonl` event shapes.
