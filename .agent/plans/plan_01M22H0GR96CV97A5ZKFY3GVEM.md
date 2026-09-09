# Activate argv and explicit shell runners

Historical implementation record: the initial v9 designation below was superseded by plan_01M22Q2G0356NS4PF6BYEYZJDR at the user's request. The final unreleased contract combines argument declarations and runners in v8; no argument-only compatibility epoch is retained. Earlier verification records remain unchanged.

## Purpose

Implement issue jig-sh-generic-monorepo-zac.3.3 so repository actions can execute a literal program with exact arguments, including user-declared bounded strings, without shell parsing. Contract v9 requires explicit shell choice and retains reading v6 commands, v7 file budgets, and v8 arguments. Follow .agent/PLANS.md.

## Progress

- [x] 2026-09-09: Inspected clean baseline, issue dependencies, runner and generator entrypoints; opened structured work.
- [x] 2026-09-09: Implemented v9 DTOs, validation, literal bindings, canonical and alias supervision, and snapshot schema runner support.
- [x] 2026-09-09: Closed libc implicit-shell fallback on canonical, alias and schema snapshot paths; focused execution, output bounds, cancellation and migration tests pass.
- [x] 2026-09-09: Migrated source to v9 (direct checks use argv; compound test command uses shell), rendered sources to explicit shell, and documented the new contract. Recopy test preserves authored argv/shell.
- [x] 2026-09-09: Proved literal bytes, compatibility, authority/cancellation and generated contracts; all eight required gates pass with fresh evidence. Full workspace verification passed 3,925 tests with 2 skipped.
- [x] 2026-09-09: Finished the explicitly required backend command `JIG_DEV_BIN=target/debug/jig scripts/jig check test`: 3,925 passed, 2 skipped, exit 0.
- [x] 2026-09-09: Closed structured work with outcome success and closed issue jig-sh-generic-monorepo-zac.3.3. Completion receipt: receipt_01M22PWPB2WER1MZW1GKYS236V.

## Surprises & Discoveries

The argument epoch is already v8. Compatibility aliases have a separate execution path from canonical runs, and native schema checking reproduces the owning schema-dump runner in a snapshot. Both paths must preserve literal execution.

An isolated Rust executable proved that Command with a PATH override uses libc execvp, which invokes /bin/sh on ENOEXEC files. A no-shebang executable printed IMPLICIT SHELL RAN. Literal execution now installs a pre-exec execve boundary with prepared argv/environment and literal PATH candidates; all failures return before std can fall back. Existing process groups and cleanup remain owned by the same supervisor.

## Decision Log

2026-09-09: Allocate v9. Keep Command readable only before v9; Shell explicitly references a checked-in command key with the same Bash/environment behavior. Argv contains a literal program and ordered args, each a literal string or an object naming a declared argument. A missing optional binding omits that position; an explicitly empty string preserves it. Bindings never select the program, working directory, environment, or shell text. No freshness schema changes.

2026-09-09: Use a Unix execve callback for argv only. Prepare every CString, pointer array, environment entry and search path before fork; the callback only tries execve and reads errno. This is necessary to make the no-implicit-shell claim true even with PATH overrides and on fork-based launch paths. Shell runners retain their existing launch behavior.

## Outcomes & Retrospective

Implementation, configured gates and the explicitly required final backend check are complete. The final `JIG_DEV_BIN=target/debug/jig scripts/jig check test` exited 0 with 3,925 tests passed and 2 skipped. Focused checks proved literal executable names and argv bytes, optional/empty binding semantics, PATH/cwd/environment handling, immutable-plan and effect checks, timeout/cancellation, parser/output limits, no-header execution rejection, and rendered recopy. Seven argument/upgrade tests, five legacy file-budget migration tests, and all 162 Doctor tests pass. Source contract and file-budget checks pass. Full runs found two migration recognition gaps, a stale current-epoch Doctor fixture, and a UI product-version test pinned to the source's previous v7 epoch; these are fixed and their targeted regressions pass. The UI test now proves the intended v9 source/launcher agreement while retaining the unchanged product version assertion.

The final unchanged-source gate batch passed all seven checks: contract, formatting, Clippy, core (3,158 tests), frontend (112), vault (443 plus 2 terminal tests), and process (210). Batch receipt: receipt_01M22P044V9VRPFR651BD6080F. The verification profile separately passed all five targets, including the full workspace suite (3,925 passed, 2 skipped), in run_01M22M870SF83NM6FXTCHHQJN0. work gates and work evidence report all eight required gates passed and fresh, with no missing, failed, stale or unknown evidence. The state audit confirms every historical JSONL file retains its HEAD bytes as a prefix; only normal append-only records were added.

## Context and Orientation

crates/jig-contract/src/repository.rs owns runner DTOs. crates/jig/src/repository.rs validates the catalog; repository/arguments.rs validates declared inputs and planner.rs authenticates planned runners and inputs. runtime/run_execution/target.rs owns supervised canonical process execution. runtime/tool_execution.rs and its command_tool.rs own compatibility aliases. policy/schema/runner.rs resolves snapshot schema execution. bootstrap/repository_model.rs and bootstrap/renderer/render_context.rs generate contracts. context.rs bounds supported epochs. .jig.toml and .agent/jig-contract.json are this repository's dogfood source.

## Plan of Work

First add closed runner types and validate epochs, literal strings and binding names. Extend existing supervised process construction instead of adding a separate lifecycle. Keep planning, approvals, leases, pre/post source checks, result parsing and cancellation around the same execution boundary. Then convert generated legacy command runners to explicit Shell at v9, preserve already-authored Argv and Shell choices, and migrate direct source checkers to Argv where no shell syntax is necessary. Update schemas and launcher epoch. Finally add isolated fixture tests through canonical and alias surfaces, rendering and recopy tests, and old-epoch rejection tests.

## Concrete Steps

From repository root, build with cargo build -p jig-sh --bin jig. Use JIG_DEV_BIN=target/debug/jig for every harness command. Run focused cargo tests while implementing, then scripts/jig work check --plan-id plan_01M22H0GR96CV97A5ZKFY3GVEM, work gates, work evidence, work receipts, and work finish for that plan. Finish backend verification with scripts/jig check test. Fix any applicable failing gates and record the actual outcomes here.

## Validation and Acceptance

An isolated executable records its arguments, proving spaces, quotes, glob characters, semicolons, dollar substitutions, newlines, Unicode and empty strings survive unchanged, and no injected marker runs. Shell receives no generic bindings. v6 command and v7 native inputs remain accepted; v8 rejects v9 runner kinds, and a pre-v9 runtime rejects a v9 manifest. Runtime fixtures cover alias execution, timeout/cancellation, nonzero results and stale plan authority. Generated fixtures and source contract checks prove v9 cutover and stable recopy. Required configured gates must pass or have applicable not-required evidence.

## Idempotence and Recovery

Use generic temporary repositories and no services or credentials. Existing state remains append-only. Rebuild after runtime changes, retry only terminal tests, and do not edit old receipts. If verification fails, keep this plan open and fix the demonstrated cause.

## Interfaces and Dependencies

Use std::process::Command::new(program).args(values) and the existing owned-process supervisor. Add ArgvValue as an untagged literal string or closed argument-reference object. Existing planned runner serialization and argument maps remain the authenticated payload; binding occurs only after validation. No new dependency or durable-state schema is needed. `crates/jig/src/repository/runners/literal_exec.rs::prepare` must run after setting cwd/environment on every argv command, including alias and schema snapshot execution. It precomputes immutable C strings and pointer arrays before fork, then uses execve without an implicit-shell fallback. The existing owned-process supervisor still creates the process group and owns cleanup; unsupported platforms fail closed.

Generated model comparisons normalize only legacy Command to explicit Shell. Argv remains a distinct authored choice. `bootstrap/runtime_config.rs` recognizes Shell command keys when retiring capabilities, while `bootstrap/repository_model/file_budget.rs` recognizes the same generated legacy action across the runner cutover. Old sources used by upgrade tests must retain old Command variants until update; current-epoch fixtures use Shell or Argv.

An installed runtime supporting epochs 2–7 rejected the v9 capability probe and rejected the v9 source before execution. Rendering an authored argv/shell model as v8 also fails, so downgrade cannot accidentally label new runner data with an old epoch.
