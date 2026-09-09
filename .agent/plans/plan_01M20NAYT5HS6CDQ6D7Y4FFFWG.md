# Recover MCP payload errors and preserve manifest diagnostics

## Progress

- [x] Reconcile the two actionable findings from the completed comprehensive review.
- [x] Separate frame reading from JSON decoding in `crates/jig/src/mcp.rs`.
- [x] Preserve file context across both parse stages in `crates/jig/src/context/runtime.rs`.
- [x] Add regression tests for session recovery, fatal framing, and manifest diagnostics.
- [x] Complete focused tests, configured work gates, and the final backend check.

## Surprises & Discoveries

The MCP reader used one error boundary for framing, I/O, and JSON decoding. Strict
duplicate-key rejection exposed an existing inability to recover from payload
errors. Once a complete frame has been consumed, decoding can fail without losing
the next frame boundary. Invalid headers and incomplete bodies cannot offer that
guarantee and must still terminate request handling.

The manifest version probe applied filename context only to typed deserialization.
An inner early return from strict parsing bypassed that context.

Validation found a transient failure outside the edited paths:
`state::plan_files::tests::append_waits_for_the_verified_sidecar_lock` hit the
existing 250 ms read-lock deadline during the first full workspace run. It passed
in the core partition and on an isolated rerun. Keep the failed receipt and retry
the full `verify` profile without changing tests or replaying the passed check
partitions.

## Decision Log

- Treat the MCP finding as a small structural boundary defect. Read bytes and
  framing first, decode in the serving loop, and return a null-id parse error for
  rejected payloads. Preserve both JSONL and legacy Content-Length replies and
  the existing wait for accepted durable workers on transport shutdown.
- Treat the manifest finding as a local error composition mistake. Chain strict
  decoding and typed conversion before attaching the existing filename context.
- Keep public contracts and migration compatibility unchanged. No broad transport
  rewrite or shared error abstraction is required.

## Outcomes & Retrospective

Implementation and focused validation are complete: 9 focused tests, 3,148 core
tests, and 112 frontend tests passed. Formatting, Clippy, contract validation,
file budgets, and check-gate freshness passed. The full profile retry passed all
3,915 workspace tests with 2 skipped. All six applicable gates are passed and
fresh; vault and process partitions have fresh not-applicable evidence under the
configured path policy. The exact final `JIG_DEV_BIN=target/debug/jig scripts/jig
check test` passed all 3,915 tests with 2 skipped and all five targets successful.
The transient locking failure did not recur in either complete workspace rerun.
Downstream clients can send a valid request after a rejected payload without
reconnecting, and launcher/bootstrap errors identify the manifest requiring repair.

## Validation and recovery

Run focused nextest tests for `mcp::tests::transport` and
`context::tests::contract_version_probe`. The assertions require exact parse-error
and ping responses, matching framing, no rejected tool side effects, fatal broken
framing, and both manifest path and underlying parse cause.

Build `cargo build -p jig-sh --bin jig`. Align completed source edits with the
existing staged changes before receipt-producing checks. Run sequentially with
`JIG_DEV_BIN=target/debug/jig`: `scripts/jig work check --plan-id
plan_01M20NAYT5HS6CDQ6D7Y4FFFWG`, inspect gates/evidence/receipts, then
`scripts/jig check test`. Do not mutate source or launch competing Cargo jobs
during verification. Preserve all append-only receipts if a check fails; correct
the cause and rerun the affected validation. Finish the plan before closing and
syncing issue `jig-sh-generic-monorepo-zac.3.2`.
