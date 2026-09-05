# Validate unified terminal dashboard parity and release readiness

This ExecPlan implements validation-only Task I (`jig-sh-l2x.10`) from `docs/plans/unified-terminal-dashboard.md`. Tasks A through H have implemented, reviewed, deleted, and documented the browser-to-terminal cutover. This task independently proves the delivered branch satisfies every parity, safety, compatibility, deletion, and release criterion without changing implementation, version metadata, tests, product documentation, or release files.

Implementation baseline: `871b501c6443f5383c002f4eaf7f1d173934abf4` on branch `jig-sh-l2x`.

## Progress

- [x] Re-read repository and crate guidance, Task I, the explicit parity matrix, acceptance layers, validation commands, and recovery rules.
- [x] Claim `jig-sh-l2x.10`, rebuild `target/debug/jig`, and open structured work at the exact Task H commit.
- [ ] Run focused contract, parity-registry, golden, differential, source, cancellation, scheduler, renderer, PTY, and specialized-TUI tests.
- [ ] Dogfood both interactive entrypoints in real PTYs and validate all three one-shot JSON commands with `jq`.
- [ ] Run deletion, release-metadata, launcher/contract-epoch, documentation, and retired-option audits.
- [ ] Run all required repository gates and inspect gate receipts against the current baseline.
- [ ] Run up to two comprehensive Claude+Codex reviews of the validation-only working changes and address any evidence defects without weakening an oracle.
- [ ] Close Task I and the epic, flush Beads, finish structured work, and commit only task-local planning and append-only evidence.

## Surprises & Discoveries

- The repository has an eligible open plan—the Task I plan itself—so JSON plan-detail dogfooding does not require a temporary `ExampleProject` fixture.
- The final validation matrix is intentionally redundant: narrow tests prove named behavior, while full gates prove integration. A broad workspace pass alone is not evidence for every parity row.
- The parity registry's acceptance test proves only that 57 `behavioral_test` strings are nonempty and unique. Static resolution found that 56 do not name any test function, and an isolated A2 mutation replacing one with `this_test_does_not_exist` still passed `parity_registry_has_one_named_oracle_for_every_matrix_row`. This is a confirmed weak-oracle defect owned by Task A, so Task I pauses before release gates and routes the repair to the predecessor.

## Decision Log

- Keep Task I validation-only. Any product, test-definition, documentation, metadata, or release defect reopens its owning predecessor bead; do not patch it here.
- Treat `crates/jig-ui/tests/dashboard_contract.rs` and its checked-in fixtures as the exact parity/field/limit/error/golden registry oracle, not merely the crate-wide pass.
- Use the rebuilt `target/debug/jig` through `JIG_DEV_BIN` for every repository command and dogfood invocation.
- Exercise real PTY entrypoints by waiting for dashboard output and sending `q`; successful terminal restoration and exit status are required.
- Permit configured Nextest retries only for already-known timing-sensitive tests under machine contention. A retry may establish a flaky pass but cannot excuse a terminal failure or a failure in cutover-owned tests.
- Do not accept semantic-looking free-form oracle labels as row-by-row evidence. Reopen Task A and bind every parity row to a real collected test/source before rerunning Task I from the corrected commit.

## Outcomes & Retrospective

Pending completion of the acceptance matrix and release gates.

## Context and orientation

The product is the in-process Ratatui dashboard in `crates/jig-ui`, entered by both `jig ui` and the permanent status-first alias `jig status --tui`. Repository access stays in `crates/jig/src/ui/source/`; shared terminal safety and lifecycle stay in `crates/jig-tui`; specialized terminal clients live in `crates/jig-codex-tui` and `crates/jig-vault-tui`. Exact parity names and machine contracts are defined in `crates/jig-ui/src/dashboard/` and checked by `crates/jig-ui/tests/dashboard_contract.rs`. CLI integration, deletion, portability, documentation, and non-TTY behavior are checked in `crates/jig/tests/ui_*.rs` and CLI/unit tests.

## Plan of work

First run the smallest direct oracle for each acceptance class: dashboard contract/goldens, model/render behavior, source differential and cancellation, scheduler serialization, CLI parsing/output, PTY lifecycle, and specialized-TUI regressions. Then dogfood the built binary in real PTYs and parse recorder, plan, and status JSON with exact schema/kind assertions. Audit the repository for deleted browser transport and removed crate/release references, and compare product version 0.3.0 with unchanged contract epoch 7. Finally run the configured structured-work gates and the full repository fmt, Clippy, test, guide/map, and contract checks; inspect receipts and the working diff before review and closure.

## Concrete validation steps

Run focused dashboard and contract targets:

    cargo test -p jig-ui --test dashboard_contract
    cargo test -p jig-ui
    cargo test -p jig-tui
    cargo test -p jig-sh ui
    cargo test -p jig-sh status
    cargo test -p jig-sh cli::help_tests
    cargo test -p jig-sh cli::status_tests
    cargo test -p jig-sh --test ui_architecture --test ui_cutover --test ui_json_portability
    cargo test -p jig-codex-tui
    cargo test -p jig-vault-tui

The dashboard contract target must explicitly report passing parity registry, exact root/schema fields, limit identifiers and ceilings, error scopes/codes, bounded rejection, partial-section preservation, and recorder/plan golden tests. The crate tests must cover hostile text, all responsive tiers, selection identity, provider additive fields, scheduler serialization/preemption, cancellation, worker joining, and PTY restoration. The `jig-sh` filters must include typed old-versus-new status equality, provider/source integration, oversized-record compatibility, JSON emission, CLI conflicts, and status cancellation.

Dogfood the binary after confirming `jq` exists:

    jq --version
    JIG_DEV_BIN=target/debug/jig scripts/jig ui
    JIG_DEV_BIN=target/debug/jig scripts/jig status --tui
    recorder_json="$(JIG_DEV_BIN=target/debug/jig scripts/jig --json ui)"
    printf '%s\n' "$recorder_json" | jq -e '.schema_version == 1 and .snapshot_kind == "recorder" and (.ok | type == "boolean") and (.epoch_id | type == "number")'
    plan_id="$(printf '%s\n' "$recorder_json" | jq -er '.open_plans[0].plan_id // .history[0].plan_id')"
    JIG_DEV_BIN=target/debug/jig scripts/jig --json ui --plan "$plan_id" | jq -e '.schema_version == 1 and .snapshot_kind == "plan" and .plan.plan_id == $plan and (.basis_epoch | type == "number")' --arg plan "$plan_id"
    JIG_DEV_BIN=target/debug/jig scripts/jig status --json | jq -e '.schema_version == 1'

Confirm `JIG_DEV_BIN=target/debug/jig scripts/jig ui --port 0` exits with Clap status 2, opens no listener, and mentions both `jig ui` and `jig ui --json`. Run precise tracked-file searches for `jig-status-tui`, web dependencies/assets, listener/routes/cookies/capability-token remnants, browser URLs, and release publication. Historical plan and changelog prose may name retired behavior; production, workspace, release, maintained-guide, and launcher surfaces may not retain it.

Run repository and release gates:

    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides
    JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map
    JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
    JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
    JIG_DEV_BIN=target/debug/jig scripts/jig check test
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M1RS7ST9PF9VEAEW2YV0K248
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M1RS7ST9PF9VEAEW2YV0K248

Success means every required gate is fresh and passing, every non-applicable gate has recorded path-policy evidence, and receipts cover only Task I's plan, Bead transition, and append-only state records.

## Validation and acceptance

Every row in section 5.6 must have a named passing registry oracle and the corresponding owning test layer must pass. Product, architecture, safety, compatibility, deletion, and test criteria in section 21 must each have direct evidence. Both real PTY commands must render and exit cleanly; recorder, plan, and status JSON must parse and retain their documented schema; no retired server surface or crate may remain; product semver must be 0.3.0 while generated contract epoch remains 7. The final diff must contain no implementation, test-definition, product-documentation, version, or release-file changes.

## Idempotence and recovery

All validation commands are read-only except expected append-only Jig receipts and Task I plan/session records. Re-running them is safe. If an oracle fails, record the exact command and evidence, reopen the predecessor that owns the defect, finish or cancel this validation attempt without weakening the check, and rerun Task I only after the predecessor fix is reviewed and committed.

## Interfaces and dependencies

Task I depends on closed Task H and has no child successor. Successful completion permits closing epic `jig-sh-l2x`. It introduces no interface and changes no product artifact; its only committed artifacts are this ExecPlan, `.agent/state/*.jsonl` evidence, and `.beads/issues.jsonl` closure records.
