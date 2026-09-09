# Fix argv runner readiness and diagnostic review findings

The user accepted four findings from the independent review of the staged argv implementation. Fix readiness and diagnostics without another contract epoch or historical state changes.

## Progress

- [x] Inspect findings and owning runtime guidance; open issue jig-sh-8l5 and structured work.
- [x] Reproduce doctor and schema remediation defects with focused tests (both failed before fixes).
- [x] Fix doctor argv readiness, schema remediation, runner labels, and macOS PATH errors.
- [x] Run focused regressions, build the runtime, stage reviewed changes, and collect the seven tooling/partition gates; all passed and fresh.
- [x] Finish with `JIG_DEV_BIN=target/debug/jig scripts/jig check --plan-id plan_01M232PX5NFS5B4J7VQ85JDZ1F test` and inspect gates, evidence, receipts, and work status.
- [x] Close structured work and jig-sh-8l5; stage remaining task metadata.

## Surprises & Discoveries

The review's PATH error observation applies to macOS. Apple's execvp skips ELOOP and ENAMETOOLONG; glibc does not. The separate claim about macOS default PATH including sbin was disproved and is outside the accepted findings.

## Decision Log

- Keep the PATH errno change macOS-specific and retain fatal ENOEXEC to prevent implicit shell execution.
- Doctor inspects argv program presence using declared cwd/PATH, without running configured programs or interpreting arguments as shell text.
- Preserve legacy shell diagnostics and use the owning canonical action in schema remediation.
- The starting work is staged. Stage the reviewed follow-up before gate capture because partially staged source cannot receive complete source attestation. Do not commit.

## Outcomes & Retrospective

All four fixes implemented. Fifteen focused tests pass, including argv descendant cancellation and legacy/current shell schema snapshots. Runtime build, native file budget, contract, formatting, and Clippy pass. The macOS-specific success branch is covered by a platform-conditional regression but cannot execute on this Linux host. Required partitions passed: core 3,162, frontend 112, vault 443 plus two additional checks, process 210. The final full test check passed: 3,929 tests passed, two skipped, 936.790 seconds; all five prerequisite/test targets succeeded. All eight required gates are passed and fresh; gate evidence and receipt history were inspected. Historical state prefixes are unchanged.

## Context and plan of work

`crates/jig/src/doctor_parts/part_02.rs` assembles required-tools readiness and currently sees only shell command keys. Extend it to argv actions; use existing literal program resolution and repository working-directory validation. Cover missing/present executables and PATH resolution in `doctor/tests/argv.rs`.

`policy/schema/runner.rs` leaves argv command text empty; retain meaningful program identity and render `scripts/jig run <target>` as remediation. Assert output through the native schema snapshot test and retain explicit-shell coverage.

`repository/runners/literal_exec.rs` performs execve PATH traversal. Preserve macOS continuation errors and add a platform-specific execution regression. `runtime/run_execution/target.rs` must keep runner identity in failure and overflow messages.

## Concrete steps and validation

Use focused nextest runs for these modules before the expensive configured gates. Build with `cargo build -p jig-sh --bin jig`, then use `JIG_DEV_BIN=target/debug/jig` for every harness invocation. Run `work check`, inspect `work gates`, `work evidence`, `work receipts`, and `work status`. Finish with the full test check. Gate status must be passed and fresh before `work finish --outcome success`. Close jig-sh-8l5 and flush Beads export.

## Idempotence, recovery, and interfaces

Changes preserve current v8 and released v2-v7 behavior except the accepted fixes; no persistent schema changes. Preserve all pre-existing staged work and append-only state prefixes. On test failure fix demonstrated causes and rerun only affected checks before final evidence. On interrupted gates retain completed receipts and rerun missing/stale gates. No external publication, commit, or additional review delegation is required.

Closure: structured work completed successfully with receipt `receipt_01M23573XX9JAZG10WR2916R8A`; issue `jig-sh-8l5` is closed. Changes are staged and not committed.
