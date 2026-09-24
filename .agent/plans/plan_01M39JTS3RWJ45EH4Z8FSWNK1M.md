# Streamline generated validation cadence

Make focused implementation validation available in newly initialized repositories,
explain when to run final gates, and document safe reuse and isolation patterns.
The deliverable is a reviewed Jig PR with passing required verification. Existing
final gates, authored workflows, and conservative freshness defaults remain intact.

## Progress

- [x] Inspect bootstrap model, phase execution, generated guidance, and freshness policy.
- [x] Generate an iteration profile for supported application checks and select it in new configurations.
- [x] Update agent and plan guidance; document freshness, dependency, license isolation, and browser teardown patterns.
- [ ] Verify generation, preservation of authored workflows, and required repository gates.
- [ ] Commit and open a PR.

Restart checkpoint: branch `feat/streamline-agent-gates`, baseline
`689a512e`. Structured work is `plan_01M39JTS3RWJ45EH4Z8FSWNK1M`; the development binary is available. Two bounded agents own guidance and documentation;
the main agent owns runtime, tests, integration, and delivery.

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

Implementation complete. Independent review found and resolved historical
file-budget migration and empty-profile capability-cutover cases. Focused and
required verification remain in progress.

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
