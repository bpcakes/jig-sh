# Validate unified terminal dashboard from repaired baseline

This ExecPlan is the corrected-baseline continuation of validation-only Task I (`jig-sh-l2x.10`). Comprehensive review rejected the predecessor attempt because its immutable structured baseline predated the Task A parity-oracle repair. This plan records the repaired commit directly and must close Task I without changing product code, test definitions, maintained product documentation, versions, or release files.

Immutable baseline: `cccd917598a3e06424ca3b05def5d6c5971f7a16` on branch `jig-sh-l2x`.

## Progress

- [x] Close the pre-repair-baseline structured attempt as superseded.
- [x] Open this plan with explicit repaired baseline `cccd917598a3e06424ca3b05def5d6c5971f7a16`.
- [x] Reproduce every distinct receipt-backed failed test unchanged in single-threaded isolation and route load-sensitive hermeticity gaps to Beads.
- [x] Rerun and record focused dashboard, shared-terminal, CLI cutover, and specialized-TUI tests.
- [x] Rerun and record both real-PTY entrypoints, exact JSON predicates, retired-option behavior, deletion searches, metadata, and contract epoch.
- [x] Run structured work checks and require all eight gates to be fresh for this baseline.
- [x] Complete comprehensive review round 2 over the corrected evidence and address every actionable finding.
- [x] Finish this plan, close Task I, flush Beads, commit and push; leave parent-epic closure as reported remaining work.

## Surprises & Discoveries

- Jig has no supported rebaseline mutation. Plan baselines are intentionally immutable, so the prior plan was closed as superseded instead of editing its state record.
- The earlier hand-written short SHA was not a Git object. The full repaired commit is `cccd917598a3e06424ca3b05def5d6c5971f7a16`.
- Heavy host contention exposed multiple independent pre-existing timing and process-cleanup flakes. Green reruns do not erase those failures; the exact receipts and dispositions are recorded below.

## Decision Log

- Keep Task I validation-only. Findings in product or test definitions reopen their owning predecessor; out-of-scope harness flakiness is tracked separately.
- Treat the old plan's green state as historical evidence only. Closure evidence for this plan must resolve against its own immutable repaired baseline.
- Use explicit single-threaded isolation to distinguish repeatable behavior defects from parallel-load setup/cleanup failures, while retaining every original nonzero receipt.
- Record focused commands, exact counts/predicates, commit, and disposition in this committed plan because Jig has no arbitrary-command receipt tool. Structured Jig gates remain backed by runtime-owned append-only receipts.
- Stop after Task I is reviewed, committed, and pushed. Do not close epic `jig-sh-l2x` in this task.

## Failure disposition ledger

- `receipt_01M1S8RW3WXF73M1NSDBPGF6DP`: `loop_tick_pr_manager_runs_worker_pushes_and_records_attempt` failed under the full parallel workspace run, then passed unchanged alone in 9.45s. Routed to existing process-hermeticity bug `jig-sh-ccz`.
- `receipt_01M1S9AYSQMN3Y1D5B933H62YY`: `loop_tick_pr_manager_resets_attempt_budget_for_new_head_sha` failed after unsafe process-tree cleanup retained its worktree, then passed unchanged alone in 15.36s. Routed to `jig-sh-ccz`.
- `receipt_01M1SAB6ZSG15FNK2KANA1Z8T1`: `receipt_append_verification_does_not_hold_the_writer_lock` timed out acquiring its test lock under parallel load, then passed unchanged alone in 0.49s. Routed to `jig-sh-7o6`.
- `receipt_01M1SC66XXYS6JH0YJD2EA2Z38`: one aggregate workspace run exceeded its 1,799.99-second target timeout without a test failure. Later aggregate runs passed all 3,937 tests. Routed to `jig-sh-7o6` for timeout-headroom and deterministic-load coverage.
- `receipt_01M1SCK6XHYHJZ31TPNNV6B238`: `scheduled_lease_failure_preserves_worker_receipt_and_retained_worktree` observed an unexpected successful worker after a lease expired under load, then passed unchanged alone in 4.75s. Routed to `jig-sh-ccz`.
- `receipt_01M1SEGCTC9P9J9D5XGDH8Q6Z0`: `timed_out_work_check_records_child_and_batch_failure_receipts` exceeded a hard-coded five-second wall bound, then passed unchanged alone in 2.12s. Routed to `jig-sh-7o6`.
- `receipt_01M1SFE67ABABB3D6AAE3WG54F`: `schema_check_supervises_timeout_and_descendant_cleanup` could not clone its temporary sandbox under the parallel shard, then passed unchanged alone in 2.40s. Routed to `jig-sh-7o6`.
- `receipt_01M1SCWSNWFKVPEQPE0V52ETW4`: `jig.work_check` could not attest `jig-contract` while `.beads/issues.jsonl` differed in both index and worktree. Staging one consistent version removed the split-index ambiguity; corrected-baseline batch `receipt_01M1SJ921PFW6VB9PNFG940V66` passed.
- `receipt_01M1SM43CN3YDYJNHMQBH3XJWS`: after final Beads evidence edits, `scheduled_dispatch_ignores_worker_forged_checkout_schedule_replica` exceeded its one-second occurrence lease during the parallel aggregate run and reported an ambiguous worker result. It passed unchanged in single-threaded isolation in 8.75s and is routed to the existing parallel loop/lease hermeticity bug `jig-sh-ccz`; the frozen aggregate profile is rerun rather than treating isolation as closure evidence.
- The raw-PTY cutover assertion and an earlier lease-cancellation assertion failed outside receipt-producing runs under contention, then passed unchanged in their complete focused targets. The PTY timing gap is routed to `jig-sh-7o6`; lease cancellation is routed to `jig-sh-ccz`. No oracle was weakened.

## Validation evidence ledger

- Focused Rust validation is operator-attested in the committed append linked by `receipt_01M1SJ8E2G0SYRE7THDPGXERYA`; that receipt proves the append operation, not command execution. Every listed command exited 0: dashboard contract 23/23; dashboard crate 115 plus 23; shared terminal 13 plus 2; `jig-sh ui` filter 284 plus matching integrations; `jig-sh status` filter 87 plus matching integrations; CLI help 29; CLI status 1; architecture/cutover/JSON portability 4 plus 9 plus 1; Codex TUI 68; vault TUI 101. The contract target includes automated adversarial guards `parity_source_resolution_rejects_unsafe_and_uncollected_paths` and `parity_test_parser_rejects_textual_ignored_and_cfg_gated_fakes`, plus the 57-row resolver.
- Real PTY validation is likewise operator-attested and used 120-by-30 terminals. `jig ui` opened Work, restored, and exited 0; transcript SHA-256 `944085a73dcb3294a47240de8258f86362d745e7a4c1042d7417ad577503a060`. `jig status --tui` opened Status, restored, and exited 0; transcript SHA-256 `35f245087c487b67bde2186bdacd59c942082577c5751e686fb5e71617d617a3`. The temporary transcripts are not committed, so their hashes are not independently replayable; `jig-sh-x4n` tracks a bounded immutable arbitrary-command/PTY evidence mechanism.
- Exact `jq -e` predicates passed for recorder schema/kind/boolean `ok`/numeric epoch, plan schema/kind/exact plan ID/numeric basis epoch, and status schema 1. The hidden retired `--port` option exited 2 and named both supported replacements.
- Cargo metadata found `jig-sh` and `jig-ui` at 0.3.0 and no `jig-status-tui`. Tracked-file searches found no dashboard HTML/CSS/JavaScript/TypeScript assets, production HTTP/listener/cookie/capability-token surface, obsolete release entry, or maintained-document claim of an active UI server. Manifest and launcher contract epochs are 7.
- `JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides` and `scripts/jig check agent-map` both exited 0; operator-attested by round-2 append receipt `receipt_01M1SKHKBYNWXM6GPJ2ZPWZ1XM`.
- Corrected-baseline structured check batch `receipt_01M1SJ921PFW6VB9PNFG940V66` passed. The only changed non-`.agent` path is `.beads/issues.jsonl`; contract gate `receipt_01M1SJ8XJ8PCWXWNF61ACKXRGJ` passed and six path-aware source gates recorded fresh not-applicable evidence.
- Aggregate profile run `run_01M1SJ94E8CJWJJTYVPEWPRXD2`, invoked through `JIG_DEV_BIN=target/debug/jig`, passed as one coherent group: Clippy `receipt_01M1SJ9NF33PJA526E8M3F8G28`, format `receipt_01M1SJ9NYJ7H54QH3KG45SPEGQ`, contract `receipt_01M1SJ9PDKJNF818NMPW4J1TW1`, file budget `receipt_01M1SJ9PX1QDRWSBCJZDTEFKCQ`, and all 3,937 workspace tests with 2 configured skips in `receipt_01M1SK336DRD0B77WKF6QNW331`. The current-baseline file-budget target correctly evaluated zero source candidates because Task I changes no source; earlier substantive branch comparison `receipt_01M1S86YRV1EJ27WGV4KRXP54C` evaluated 110 files and reported its warning/notice posture.
- After round-2 Beads/privacy changes, aggregate run `run_01M1SKKSRRFRR18VGDFX3BATHV` exposed the additional one-second scheduled-dispatch lease flake in `receipt_01M1SM43CN3YDYJNHMQBH3XJWS`; it is retained and disposed above. The unchanged final fingerprint then passed as coherent run `run_01M1SM63DQGH05NF3W4BYHT5XX`: Clippy `receipt_01M1SM6A1VTEP0FD9MMK7GH2P7`, format `receipt_01M1SM6AHQGW7RS56R7DS68Z1W`, contract `receipt_01M1SM6B16PCCHGF0SXX3D5FAM`, zero-candidate file budget `receipt_01M1SM6BGFBGYN3ECKD8HV3VRX`, and all 3,937 workspace tests with 2 configured skips in `receipt_01M1SN1PMKEKCSGHAR9PNZ04XF`. Final structured batch `receipt_01M1SM60YK88G1RP5Q0M85EEBD` and contract receipt `receipt_01M1SM5W685ZE8DH353W010AW5` leave all eight required gate states fresh.

## Outcomes & Retrospective

Corrected-baseline focused validation is green. All eight structured gate requirements are fresh and satisfied with accurate applicability: final aggregate `verify` run `run_01M1SM63DQGH05NF3W4BYHT5XX` and contract gate `receipt_01M1SM5W685ZE8DH353W010AW5` executed successfully, while six path-aware source gates recorded fresh not-applicable evidence for the Beads-only non-`.agent` diff. Round 1 exposed and caused repair of the stale-baseline design, underreported failures, split-index ambiguity, and missing focused evidence. Round 2 completed with Claude and Codex over verified fingerprint `8c24e730e7e387b6a84351f4ce03a54c6563efb1d7c41518825bda31f387a493`; it found only evidence-labeling, omission, scope-wording, and privacy issues. Those are corrected, no product finding remains, the structured plan finished successfully, and Task I is closed. Parent-epic closure remains deliberately deferred.

## Context and orientation

The in-process Ratatui dashboard lives in `crates/jig-ui` and is entered by both `jig ui` and `jig status --tui`. Repository collection lives in `crates/jig/src/ui/source/`; terminal lifecycle is shared through `crates/jig-tui`; specialized clients live in `crates/jig-codex-tui` and `crates/jig-vault-tui`. Exact parity and schema contracts are checked by `crates/jig-ui/tests/dashboard_contract.rs`; CLI cutover and portability are covered by `crates/jig/tests/ui_*.rs`.

## Plan of work

Rerun the smallest direct oracle for each acceptance class from the repaired commit: dashboard contract/goldens, model and renderer behavior, source differential/cancellation, scheduler behavior, CLI parsing/output, PTY lifecycle, and specialized-TUI regressions. Dogfood both entrypoints in real PTYs, parse recorder/plan/status JSON with exact predicates, and audit the repository for retired transport, assets, crate references, documentation claims, version, and contract epoch. Then run structured gates, inspect receipts, complete the second comprehensive review round, finish structured work, and close only Task I.

## Idempotence and recovery

Validation commands are read-only except runtime-owned append-only Jig state and Beads transitions. If a cutover-owned oracle fails, stop Task I and reopen the predecessor that owns it. Do not weaken the oracle or repair implementation in this task. Out-of-scope load failures must retain their original receipt and a tracked disposition.

## Interfaces and dependencies

This plan introduces no interface. Its expected committed artifacts are this plan, the superseded plan, append-only `.agent/state/*.jsonl` evidence, and `.beads/issues.jsonl` task/follow-up transitions. Successful completion permits but deliberately does not perform closure of parent epic `jig-sh-l2x`.


## Immutable validation append (commit cccd917598a3e06424ca3b05def5d6c5971f7a16, all exit 0) — Focused Rust commands: cargo test -p jig-ui --test dashboard_contract (23/23, including all 57 source-backed parity rows); cargo test -p jig-ui (115 unit + 23 contract); cargo test -p jig-tui (13 unit + 2 terminal-session); cargo test -p jig-sh ui (284 unit plus matching integration tests); cargo test -p jig-sh status (87 unit plus matching integration tests); cargo test -p jig-sh cli::help_tests (29); cargo test -p jig-sh cli::status_tests (1); cargo test -p jig-sh --test ui_architecture --test ui_cutover --test ui_json_portability (4 + 9 + 1); cargo test -p jig-codex-tui (68); cargo test -p jig-vault-tui (101). Real PTYs: scripts/jig ui at 120x30 opened Work, exited 0, restored; transcript SHA-256 944085a73dcb3294a47240de8258f86362d745e7a4c1042d7417ad577503a060. scripts/jig status --tui at 120x30 opened Status, exited 0, restored; transcript SHA-256 35f245087c487b67bde2186bdacd59c942082577c5751e686fb5e71617d617a3. Exact jq predicates passed for recorder schema/kind/ok/epoch, plan schema/kind/id/basis_epoch, and status schema 1. scripts/jig ui --port 0 exited 2 and named jig ui plus jig ui --json. Cargo metadata found jig-sh and jig-ui 0.3.0 and no jig-status-tui. Searches found no dashboard web assets, production HTTP/listener/cookie/token surface, obsolete release entry, or maintained-doc claim of an active server. Manifest and launcher contract epoch are 7. Follow-up routing: jig-sh-ccz and jig-sh-7o6.


## Round 2 remediation append — Both final reviewers completed on frozen fingerprint 8c24e730e7e387b6a84351f4ce03a54c6563efb1d7c41518825bda31f387a493. scripts/jig check agent-guides and scripts/jig check agent-map both exited 0 under JIG_DEV_BIN=target/debug/jig. Corrective edits distinguish executed aggregate/profile evidence from six fresh not-applicable source gates, label focused/PTY/JSON evidence operator-attested, dispose split-index failure receipt_01M1SCWSNWFKVPEQPE0V52ETW4, route raw-PTY and lease failures, disclose zero-candidate current file-budget scope, cite the automated parity mutation guards, and track arbitrary-command receipts in jig-sh-x4n. No third review round is permitted.
