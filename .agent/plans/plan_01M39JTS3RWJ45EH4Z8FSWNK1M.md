# Streamline generated validation cadence

Make focused implementation validation available in newly initialized repositories,
explain when to run final gates, and document safe reuse and isolation patterns.
The deliverable is a reviewed Jig PR with passing required verification. Existing
final gates, authored workflows, and conservative freshness defaults remain intact.

## Progress

- [x] Inspect bootstrap model, phase execution, generated guidance, and freshness policy.
- [x] Generate an iteration profile for supported application checks and select it in new configurations.
- [x] Update agent and plan guidance; document freshness, dependency, license isolation, and browser teardown patterns.
- [x] Verify generation and preservation of authored workflows; run all required local gates.
- [ ] Obtain a successful local full-suite receipt and finish structured work (local run timed out).
- [x] Commit and open PR #48.

Restart checkpoint: implementation commit `49c35d89` on
`feat/streamline-agent-gates`, baseline `689a512e`, PR #48. Structured work is
`plan_01M39JTS3RWJ45EH4Z8FSWNK1M` and remains open because the local full test
receipt timed out. Source implementation is complete and independently reviewed.
Before attempting closure, inspect current evidence and obtain successful local
full-suite evidence on a host that can finish within the configured limit.

## Surprises & Discoveries

- Phase execution and exhaustive/worktree freshness already exist. Bootstrap only
  generates the final verification profile, and guidance omits validation cadence.
- Generated checks are already independent profile requirements. Authored
  `depends_on` edges must remain real execution prerequisites.
- Adding a generated profile required preserving historical verify-only migration
  recognition and retiring an empty generated profile after capability removal.
- The local installed binary does not match the repository's pinned release;
  use the development binary through `JIG_DEV_BIN` for this task.

## Decision Log

- 2026-09-24: Seed iteration with generated backend formatting/lint/test and
  frontend build/lint/test/typecheck targets. Exclude repository policy, database
  preparation, and deployment checks. This is a starting selection, not a promise
  that arbitrary project tests are fast. Preserve existing authored profiles and
  their explicit selection or omission during update.
- 2026-09-24: Keep freshness opt-ins owner-audited. Document a complete input
  closure including future workspace members instead of copying fixed file lists.
- 2026-09-24: License policy and asynchronous browser observers are application
  concerns absent from Jig's current generated harness. Upstream their reusable
  isolation and completion patterns as recipes without adding mandatory gates or
  a new license-policy subsystem. Runtime pinning already exists.

## Outcomes & Retrospective

Delivered generated application iteration profiles, preserved final requirements
and authored selection/omission, and updated generated guidance and reusable
optimization recipes. Independent review found and resolved historical
file-budget migration and empty-profile capability-cutover cases.

Validation on 2026-09-24:

- 37 focused regression tests and two isolated descriptor-limit tests passed.
- Formatting, Clippy, contract, file-budget, and current-source runtime checks
  passed through plan-linked Jig targets.
- The initial broad bootstrap run used an intermediate build: 661 tests passed,
  four failed, and one was ignored. All four failed cases passed against the final
  build in the focused runs (including a helper whose executable was replaced
  during the earlier rebuild).
- The first local workspace attempt stopped on private-directory policy tests
  under ambient umask 0002. Rerunning with umask 077 reported no test failures
  before reaching the 1800-second limit while vault integration tests remained
  active. This is incomplete local evidence, not a passing full-suite receipt.
- PR #48 CI passed the complete locked workspace suite on Linux and macOS,
  rendered fixtures, formatting, Clippy, file budgets, minimum-Rust checks, and
  all four freshness command benchmark jobs on implementation commit `49c35d89`.
  The macOS no-default-features test job was still running at this checkpoint.

Do not weaken or bypass the final gate to close this plan. The PR can be reviewed
using the completed CI evidence; local structured closure remains outstanding.

## Context and work sequence

`crates/jig/src/bootstrap/repository_model/finish.rs` constructs profiles.
`renderer/render_context.rs` and `templates/project/.jig.toml.jinja` publish the
configuration. `runtime_config.rs` reconciles existing work settings. Templates
have embedded snapshots used by packaged builds. `runtime/work/checks/phase.rs`
already selects the configured iteration profile without changing final gates.

T1 adds generation and regression coverage for Rust, Go, frontend, and authored
models. T2 updates generated/root guidance and plan guidance independently. T3
documents safe configuration recipes independently. T4 integrates T1–T3, checks
the diff for private identifiers, runs focused bootstrap tests and all configured
work gates, then opens the PR. No external contracts or persisted schemas change;
existing repositories keep their authored model and can add a profile explicitly.

## Validation and recovery

From the repository root, use `cargo nextest run -p jig-sh --lib -E 'test(bootstrap::)'` for
bootstrap behavior, plus formatting and template parity checks. Build with
`cargo build -p jig-sh --bin jig`, set `JIG_DEV_BIN=target/debug/jig`, and run
`scripts/jig work check --plan-id ID --phase final`. Inspect gates/evidence, run
`scripts/jig check test` for backend completion, and finish structured work after
source is stable. Report failures and gaps honestly; never weaken required gates.
Changes are reversible Git edits; append-only evidence is retained.
