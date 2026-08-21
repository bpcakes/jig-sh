# Expose progress for long-running agent commands

## Progress

- [x] Audited silent execution and observation paths.
- [ ] Add transport-neutral execution events and supervised configured-command execution.
- [ ] Render live CLI progress and preserve JSON/MCP protocol safety.
- [ ] Add orchestration phase events for checks, review/refine, loops, and setup.
- [ ] Reuse status fingerprints/receipt scans and parallelize status providers.
- [ ] Add configurable timeouts and cancellation.
- [ ] Update generated templates/docs and validate all requested behavior.

## Surprises & Discoveries

- Existing configured commands use unbounded Command::output and work-check batches compound silent waits.
- Status recomputes one fingerprint and full receipt scan per open plan.

## Decision Log

- Preserve machine-readable stdout; human progress belongs on stderr and MCP progress must use protocol notifications.
- Preserve captured output for receipts while streaming safe configured-command chunks only to explicitly selected human transports.

## Outcomes & Retrospective

Pending implementation and verification.

## Context and orientation

The shared runtime dispatches both CLI and MCP calls. Execution visibility therefore needs an explicit observer passed from the transport boundary rather than terminal detection inside business logic. Configured command output is safe to expose under the ordinary human CLI contract; secret-bearing vault execution remains outside this scope.

## Plan of work

Introduce execution events and a supervised child runner, thread an observer through runtime command execution and orchestration, render terminal events without touching JSON stdout, add protocol progress support where request metadata permits it, optimize status collection to share repository facts, and document timeout configuration.

## Validation and acceptance

Focused tests must prove early phase output, output streaming, heartbeat behavior, clean JSON stdout, MCP framing, timeout cleanup, work phase counters, one shared status fingerprint/receipt scan, and concurrent providers. Finish with cargo build and the configured Jig test gate using JIG_DEV_BIN=target/debug/jig.

## Idempotence and recovery

All new execution observation is ephemeral. Receipt/state formats remain append-compatible. Timeout cleanup must own the child process tree before execution.

## Interfaces and dependencies

Primary files are crates/jig/src/runtime/tool_execution.rs, runtime/work, runtime/worker_runner.rs, status.rs, cli/run.rs, mcp.rs, context/config parsing, templates, and docs.

Implemented the shared execution event stream, supervised configured commands and Codex workers, CLI stderr/MCP progress, phase counters, concurrent status providers, shared multi-plan gate indexing, configurable timeouts, cancellation, templates, docs, and regression coverage. Focused tests pass; repository gates are in progress.

Implemented agent-visible execution progress across configured checks, work gates/review/refine/loops/setup/status providers, MCP progress notifications, shared status indexing/concurrency, and supervised timeout/cancellation. Human-mode progress now streams through captured stderr as well as terminals; JSON mode remains quiet. Final verification: fmt, Clippy, contract, 2,164/2,164 main tests, and 439/439 feature tests passed (test receipt receipt_01M0JTGWZ4389JT2HNZA1Q7W8D).