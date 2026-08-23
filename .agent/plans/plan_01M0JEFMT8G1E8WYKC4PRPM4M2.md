# Expose progress for long-running agent commands

## Progress

- [x] Audited silent execution and observation paths.
- [x] Added transport-neutral execution events and supervised configured-command execution.
- [x] Added bounded deferred CLI/MCP progress while preserving JSON/MCP protocol safety.
- [x] Added orchestration phase events for checks, review/refine, loops, and setup.
- [x] Reused status fingerprints/receipt scans and added bounded provider concurrency.
- [x] Added configurable timeouts and typed cancellation through orchestration.
- [x] Updated generated templates/docs and passed the configured repository gates.

## Surprises & Discoveries

- Existing configured commands use unbounded Command::output and work-check batches compound silent waits.
- Status recomputes one fingerprint and full receipt scan per open plan.
- Live transport writes would couple process supervision to consumer backpressure, so progress is retained in bounded memory and rendered after supervision returns.
- Cancellation and capture overflow must remain typed control-plane outcomes; reducing either to an exit code or truncation flag makes collect-all and cleanup policy ambiguous.

## Decision Log

- Preserve machine-readable stdout; human progress belongs on stderr and MCP progress must use protocol notifications.
- Preserve captured output for receipts while deferring bounded configured-command previews to explicitly selected human transports.
- Bound status execution to four active providers while retaining configured result order and leaving queued providers unstarted after cancellation.

## Outcomes & Retrospective

The implementation completed across separately reviewed slices. Configured commands and workers now run in owned process trees with finite output policy, deferred progress cannot block timeout/cancellation, phase ownership is explicit, status shares gate inputs and uses a four-worker scheduler, and work checks stop on typed cancellation instead of recording failures for commands that never started. Authoritative command and schema-less worker overflow terminates the process tree promptly; schema-backed Codex workers may truncate diagnostic transcripts because their separately bounded `-o` file remains authoritative.

The original final verification passed formatting, Clippy, contract, and both configured test stages. Later comprehensive review found boundary defects in flush ordering, output-policy typing, cancellation collection, and unbounded provider fan-out; plan `plan_01M0K1W3NHEE9DZ3RJVTA0BJ5Y` records and closes those follow-up corrections. This retrospective replaces the stale “pending” state without rewriting append-only JSONL history.

## Context and orientation

The shared runtime dispatches both CLI and MCP calls. Execution visibility therefore needs an explicit observer passed from the transport boundary rather than terminal detection inside business logic. Configured command output is safe to expose under the ordinary human CLI contract; secret-bearing vault execution remains outside this scope.

## Plan of work

Introduce execution events and a supervised child runner, thread an observer through runtime command execution and orchestration, render deferred bounded events without touching JSON stdout, add protocol progress support where request metadata permits it, optimize status collection to share repository facts and bounded concurrency, and document timeout configuration.

## Validation and acceptance

Focused tests proved early phase previews, historical heartbeat behavior, clean JSON stdout, MCP framing, timeout cleanup, work phase counters, one shared status fingerprint/receipt scan, and bounded concurrent providers. The development binary then passed the configured Jig formatting, Clippy, contract, and test gates through `JIG_DEV_BIN=target/debug/jig`.

## Idempotence and recovery

All new execution observation is ephemeral. Receipt/state formats remain append-compatible. Timeout cleanup must own the child process tree before execution.

## Interfaces and dependencies

Primary files are crates/jig/src/runtime/tool_execution.rs, runtime/work, runtime/worker_runner.rs, status.rs, cli/run.rs, mcp.rs, context/config parsing, templates, and docs.

Plan revision note (2026-08-21): Reconciled the living sections with the completed deferred-progress implementation and the later boundary-hardening follow-up; removed stale live-streaming and pending-verification claims.
