# Harden loop run, persistence, and attention policies

This living plan continues the earlier loop-policy work after a comprehensive review found that durable occurrence state and workflow-local completion were still represented by weaker generic mechanisms. The observable goal is that scheduled work is claimed durably before side effects, each occurrence records only its own workflow outcome, unattended subprocesses stay supervised, ambiguous post-push work remains visible, and operator-visible scheduling states are explicit.

## Progress

- [x] Preserve the earlier typed run/dispatch summaries, single-tick scheduled Codex behavior, and attention acknowledgement work.
- [x] Research the intended durability, repository-trust, and first-dispatch contracts from current docs, history, tests, and existing durable writers.
- [x] Establish a green baseline with `cargo fmt --all -- --check` and all 47 `runtime::loops` unit tests.
- [x] Add characterization tests for workflow-local outcomes, durable-state loss, deferred dispatch, review-marker ownership, prompt validation, checkout-process supervision, and retained worktree placement.
- [x] Refactor completion and persistence policies behind typed/internal owners without changing serialized occurrence schema.
- [x] Apply the behavior fixes and update operator documentation.
- [x] Build the development Jig binary and run focused tests, configured format, Clippy, contract, and full test gates through `JIG_DEV_BIN=target/debug/jig`.

## Surprises & Discoveries

- The occurrence ledger was moved from cache to `.agent/runtime/loop/`, but it still reused `state::write_json`, whose cache-oriented atomic rename does not sync either the file or parent directory.
- `ScheduledTick` retains a typed `WorkflowCompletion`, yet `TerminalDetails` ignored that owner and reconstructed occurrence outcome from the tick's presentation JSON. That JSON also contains machine-global attempt attention, allowing another workflow to poison the current occurrence.
- The existing repository contract already calls configured status argv trusted executable code and refinement skills trusted inputs. Loop configuration has the same trust model but does not state it explicitly.
- First dispatch intentionally executes the most recent cron instant. This is documented in `docs/configuration.md` and covered by schedule-window tests, so it is not a defect.
- The Fowler scanner reported 200 truncated heuristic candidates. Manual review accepted typed completion/persistence boundaries and rejected file-length, test `unwrap`, DTO-field, and orchestration-parameter counts as unsupported style signals.
- Extracting the occurrence tests into `occurrence/tests.rs` kept both the production owner and its tests below the repository's 800-line source-size boundary without changing module visibility or behavior.

## Decision Log

- Decision: treat durable occurrence claims as power-loss durable, not merely process-restart durable.
  Rationale: the public docs promise duplicate prevention from durable claims, and established durable writers in `state/jsonl.rs` sync the temporary file and parent directory.
  Date/Author: 2026-08-28 / Codex
- Decision: keep repository configuration as trusted executable input and document that boundary for unattended loops.
  Rationale: Jig already executes repo-configured commands and auto-approved refinements under this trust model; adding a second partial sandbox policy would be misleading.
  Date/Author: 2026-08-28 / Codex
- Decision: preserve first-dispatch execution of the most recent cron instant.
  Rationale: it is an explicit, tested scheduling contract rather than an accidental implementation detail.
  Date/Author: 2026-08-28 / Codex
- Decision: use a closed `WorkflowOutcome` enum inside `WorkflowCompletion` and make scheduled terminal state consume it.
  Rationale: this applies Replace Primitive with Object and Move Function so one owner classifies action statuses, while serialized JSON remains compatible.
  Date/Author: 2026-08-28 / Codex
- Decision: keep cache writers and durable occurrence writers distinct.
  Rationale: syncing every lease/attempt cache renewal would add unnecessary I/O; the stronger invariant belongs to `SchedulePersistence`.
  Date/Author: 2026-08-28 / Codex

## Outcomes & Retrospective

The earlier slice replaced duplicated run/dispatch conditionals with explicit summaries, restricted scheduled Codex tasks to one tick, preserved unresolved occurrence attention, and hardened worker waiting. This extension gives durable schedule publication and workflow-local outcome classification explicit owners. All 60 focused loop unit tests, all 40 loop integration tests, the two-process claim regression, formatting, Clippy, contract, source-size policy, and the complete configured source-test matrix pass. No serialized occurrence schema was changed; dispatch adds only `deferred_count` and the `deferred` aggregate status.

## Context and Orientation

`crates/jig/src/runtime/loops/workflow.rs` owns workflow ticks and completion evidence. `engine.rs` executes one workflow and renders aggregate tick JSON. `schedule/policy.rs` converts a scheduled tick into a durable occurrence result. `occurrence.rs` owns occurrence transitions, while `occurrence/persistence.rs` owns the durable ledger. `codex_task.rs` owns prompt and checkout lifecycles. `pr_manager/review_threads.rs` owns GitHub mutation reconciliation.

The persistent JSON schema and CLI JSON are compatibility boundaries. Internal enums may be added, but existing occurrence status strings and fields must remain readable. Additive dispatch fields are acceptable when old fields retain their meaning.

## Plan of Work

First add characterization tests around the reported failures. Then introduce `WorkflowOutcome` and route occurrence terminal policy through `WorkflowCompletion`, leaving presentation JSON as an adapter. Next give `SchedulePersistence` a dedicated sync-file, rename, sync-directory writer and make missing durable state behind a legacy marker fail closed consistently. After those structural steps, change post-push cancellation to attention, split deferred dispatch accounting, supervise Codex checkout Git commands, place retained worktrees under runtime state, validate review-comment ownership, and open prompt files once.

Keep renewal tolerance as a separate behavior change after persistence is green. Retry transient renewal failures only while the existing claim remains valid; never continue beyond expiry or hide a final renewal failure.

## Concrete Steps

1. Add focused unit tests for typed workflow outcome priority and scheduled isolation from unrelated attempt attention; run `cargo test -p jig-sh --lib runtime::loops`.
2. Add durable-writer and migration-loss tests; implement file and directory syncing in `SchedulePersistence`; rerun occurrence tests.
3. Add supervised Git timeout/cancellation/output-limit tests and migrate Codex checkout Git calls to the authoritative execution boundary.
4. Move new retained task worktrees below `.agent/runtime/loop/worktrees/tasks`, preserve existing recorded paths, and update lifecycle tests/docs.
5. Require reconciled review markers to be authored by the authenticated viewer; add owned and spoofed-marker tests.
6. Add deferred accounting/status, post-push attention, prompt validation, and bounded renewal retry tests and fixes.
7. Run focused integration and CLI JSON tests, then repository gates.

## Validation and Acceptance

The focused loop unit and integration tests must pass. New tests must prove: unrelated attempts do not alter an occurrence outcome; post-push cancellation becomes `needs_attention`; missing durable state cannot recreate an empty claim ledger; durable writes sync before publication; deferred work is distinguishable; spoofed markers are rejected; prompt size/type checks use the opened handle; and checkout Git processes obey cancellation, timeout, and output bounds.

Final validation uses `cargo build -p jig-sh --bin jig`, exports `JIG_DEV_BIN=target/debug/jig`, and runs the configured Jig format, Clippy, contract, and test gates plus `scripts/jig work gates`, `work evidence`, and `work receipts` for this plan.

## Idempotence and Recovery

All tests create generic temporary repositories. Durable-state migration remains fail closed: if a marker exists without its target, restore or deliberately reconcile the ledger rather than allowing dispatch to recreate claims. Existing retained-worktree paths in old ledgers remain valid; only newly created worktrees move.

If a behavior step cannot be proven, stop at the preceding green structural state. Do not rewrite `.agent/state/*.jsonl`; plan prose may be updated as this living document.

## Interfaces and Dependencies

No new dependency is planned. Use the existing `run_authoritative_execution_command` boundary for Git and the standard library `File::sync_all` plus a parent-directory sync for durable persistence. Keep all new types crate-internal and preserve Rust 1.88, edition 2024, Linux/macOS owned-process behavior, and current serde schemas.
