# Superseded validation attempt for unified terminal dashboard parity

This ExecPlan implements validation-only Task I (`jig-sh-l2x.10`) from `docs/plans/unified-terminal-dashboard.md`. Tasks A through H have implemented, reviewed, deleted, and documented the browser-to-terminal cutover. This task independently proves the delivered branch satisfies every parity, safety, compatibility, deletion, and release criterion without changing implementation, version metadata, tests, product documentation, or release files.

Intended implementation baseline: `cccd917598a3e06424ca3b05def5d6c5971f7a16` on branch `jig-sh-l2x`, after the Task A oracle repair routed from this validation task. The immutable structured-plan event actually records pre-repair baseline `871b501c6443f5383c002f4eaf7f1d173934abf4`; this mismatch was found in comprehensive review round 1 and makes this attempt unsuitable for Task I closure.

## Progress

- [x] Re-read repository and crate guidance, Task I, the explicit parity matrix, acceptance layers, validation commands, and recovery rules.
- [x] Claim `jig-sh-l2x.10`, rebuild `target/debug/jig`, and continue structured work from the reviewed Task A repair commit.
- [x] Run focused contract, parity-registry, golden, differential, source, cancellation, scheduler, renderer, PTY, and specialized-TUI tests.
- [x] Dogfood both interactive entrypoints in real PTYs and validate all three one-shot JSON commands with `jq`.
- [x] Run deletion, release-metadata, launcher/contract-epoch, documentation, and retired-option audits.
- [x] Run all required repository gates and inspect gate receipts against the current baseline.
- [x] Run comprehensive Claude+Codex review round 1; identify the immutable-baseline mismatch and incomplete failure/evidence accounting.
- [x] Close this invalid-baseline attempt as superseded and continue Task I in a fresh structured plan at the repaired commit.

## Surprises & Discoveries

- The repository has an eligible open plan—the Task I plan itself—so JSON plan-detail dogfooding does not require a temporary `ExampleProject` fixture.
- The final validation matrix is intentionally redundant: narrow tests prove named behavior, while full gates prove integration. A broad workspace pass alone is not evidence for every parity row.
- The parity registry's acceptance test proves only that 57 `behavioral_test` strings are nonempty and unique. Static resolution found that 56 do not name any test function, and an isolated A2 mutation replacing one with `this_test_does_not_exist` still passed `parity_registry_has_one_named_oracle_for_every_matrix_row`. This is a confirmed weak-oracle defect owned by Task A, so Task I pauses before release gates and routes the repair to the predecessor.
- Task A's corrected baseline makes every one of the 57 parity rows resolve to a collected, source-backed Rust test. The focused acceptance matrix passes the 23-test dashboard contract target, 115 dashboard unit tests, shared terminal tests, all 9 CLI cutover tests, the JSON portability target, and both specialized TUI suites.
- Under heavy host contention, direct and structured runs recorded several distinct terminal failures rather than only two: PR-manager worker state (`receipt_01M1S8RW3WXF73M1NSDBPGF6DP`), PR-manager retained-worktree cleanup (`receipt_01M1S9AYSQMN3Y1D5B933H62YY`), receipt-writer lock timing (`receipt_01M1SAB6ZSG15FNK2KANA1Z8T1`), an aggregate 1,799.99-second timeout (`receipt_01M1SC66XXYS6JH0YJD2EA2Z38`), scheduled-lease retained-worktree state (`receipt_01M1SCK6XHYHJZ31TPNNV6B238`), a five-second work-check timing bound (`receipt_01M1SEGCTC9P9J9D5XGDH8Q6Z0`), and schema-check sandbox cloning (`receipt_01M1SFE67ABABB3D6AAE3WG54F`). The raw-PTY substring and an earlier lease-cancellation assertion also failed outside receipt-producing runs.
- All six named failing tests represented by test receipts passed unchanged in explicit single-threaded isolation after review: the two PR-manager tests in 9.45 and 15.36 seconds, receipt-writer lock in 0.49 seconds, scheduled-lease state in 4.75 seconds, work-check timeout behavior in 2.12 seconds, and schema-check cleanup in 2.40 seconds. Later configured runs also passed 3,171 core tests and 3,937 aggregate tests. These results establish load sensitivity, not permission to erase the failures; follow-up Beads own hermeticity work.
- Direct dogfooding passed in real 120-by-30 PTYs for both entrypoints. Recorder, plan-detail, and status JSON passed exact `jq` schema checks; the hidden retired port exited with Clap status 2 and named both supported replacements.
- Cargo metadata and tracked-file audits confirm product version 0.3.0, generated contract epoch 7, no obsolete status package or release entry, and no production listener, route, cookie, capability-token, HTML, CSS, or browser-dashboard surface.
- The final full-workspace aggregate passed all 3,937 tests with 2 configured skips. A separate core shard first hit one load-induced sandbox-clone setup failure; that exact test passed unchanged in isolation, and the forced core gate then passed all 3,171 selected tests.

## Decision Log

- Keep Task I validation-only. Any product, test-definition, documentation, metadata, or release defect reopens its owning predecessor bead; do not patch it here.
- Treat `crates/jig-ui/tests/dashboard_contract.rs` and its checked-in fixtures as the exact parity/field/limit/error/golden registry oracle, not merely the crate-wide pass.
- Use the rebuilt `target/debug/jig` through `JIG_DEV_BIN` for every repository command and dogfood invocation.
- Exercise real PTY entrypoints by waiting for dashboard output and sending `q`; successful terminal restoration and exit status are required.
- Permit configured Nextest retries only for already-known timing-sensitive tests under machine contention. A retry may establish a flaky pass but cannot excuse a terminal failure or a failure in cutover-owned tests.
- Do not accept semantic-looking free-form oracle labels as row-by-row evidence. Reopen Task A and bind every parity row to a real collected test/source before rerunning Task I from the corrected commit.
- Stop after Task I is verified, reviewed, committed, and pushed. Do not close the parent epic in this task; report epic closure as remaining work for the next turn.

## Outcomes & Retrospective

This attempt cannot certify Task I because its immutable structured baseline predates the Task A repair, even though its final assembled gate state is green: the final aggregate target passed 3,937 tests; later forced core evidence passed 3,171 after a failed multi-gate sweep; frontend passed 112; vault passed 443 plus 2; and process passed 209. Comprehensive review round 1 correctly rejected the hand-edited baseline claim and the incomplete failure/evidence narrative. A fresh structured plan at the exact repaired commit must rerun or reuse only baseline-compatible gates and durably record the focused, PTY, JSON, deletion, and failure-disposition evidence before Task I closes.

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

Task I depends on closed Task H and has no child successor. Successful completion permits closing epic `jig-sh-l2x`, but epic closure is deliberately deferred per the stop boundary for this task. Task I introduces no interface and changes no product artifact; its only committed artifacts are this ExecPlan, `.agent/state/*.jsonl` evidence, and `.beads/issues.jsonl` transition records.
