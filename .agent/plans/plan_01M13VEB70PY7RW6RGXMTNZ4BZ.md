# Reduce Dogfood Test Feedback Time

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current as implementation proceeds.

Plan ID: `plan_01M13VEB70PY7RW6RGXMTNZ4BZ`

## Purpose / Big Picture

Jig's complete Rust test gate currently takes roughly fifteen minutes in recent dogfood receipts, which makes ordinary local changes expensive to validate. This work reduces local feedback time without weakening merge coverage. Afterward:

- vault tests initialize new vaults with explicit test KDF parameters except for a small production-KDF contract suite;
- Argon2 itself is optimized in test builds;
- large frontend behavior matrices reuse manager-specific generated scripts instead of rendering the complete project template for every row;
- local Nextest stops after the first failure, while locked CI remains comprehensive;
- dogfood gates run only the Rust test partitions relevant to changed paths and can reuse valid evidence;
- Codex launcher tests wait for a parseable PID, removing a known parallel-test readiness race.

The externally observable application behavior, persisted vault format, CLI contract, and CI merge requirements remain unchanged. These are test, build-profile, and dogfood orchestration changes, so a direct cutover is appropriate.

## Progress

- [x] (2026-08-28) Measured the existing suite and isolated the main costs: frontend fixtures and production-strength Argon2 in tests.
- [x] (2026-08-28) Reproduced the Codex launcher PID race under parallel execution and confirmed the failing test passes alone.
- [x] (2026-08-28) Built `target/debug/jig` and opened structured work at Git baseline `68856a09c11aaf04bd54476df9cfcb6c64cbed39`.
- [x] (2026-08-28) Created and claimed Beads feature `feat-codex-resume-generic-monorepo-i53` with three implementation children and a validation child blocked by them.
- [x] (2026-08-28) Optimized Argon2 test builds, adopted the test KDF throughout behavioral fixtures while retaining two production contracts, and made PID readiness wait for a positive parseable value.
- [x] (2026-08-28) Replaced repeated full frontend scaffolds in the two largest behavior matrices with one immutable generated-script set per package manager and minimal per-row repositories.
- [x] (2026-08-28) Added the fail-fast local Nextest profile, four exact typed partitions, and conservative path-scoped reusable dogfood gates while retaining the locked CI command.
- [x] (2026-08-28) Ran focused benchmarks, all four exact partitions, the complete local suite, and all eight Jig work gates; every result is green and receipt evidence is fresh.
- [x] (2026-08-28) Reviewed the plan for self-containment, dependency order, decision rationale, and steady-state acceptance; corrected one stale frontend path and aligned the documented local-profile inheritance.
- [x] (2026-08-28) Closed and synced the validation task and parent Beads feature after every acceptance criterion passed.

## Surprises & Discoveries

- Recent successful full Rust receipts have a median duration of 916.212 seconds (15 minutes 16 seconds), not merely the reported ten minutes.
- On a warm build, 2,679 non-vault tests took 300.315 seconds, while 428 vault tests took 162.272 seconds.
- One exact Argon2 test took 26.298 seconds with the default dev-style test build and 1.688 seconds with Argon2 optimized. The full vault partition fell from 162.272 seconds to 23.823 seconds with that package override.
- The 107-test frontend group still took 251.756 seconds. Its largest matrices repeatedly call `run_init`, which materializes about 209 template files for each matrix row even though the assertions need only the rendered web-check scripts and small manifest/lock fixtures.
- Raising global test parallelism from four to eight did not improve the non-vault wall time (300.315 seconds versus 296.924 seconds) and exposed a launcher test that read an existing but still-empty PID file.
- The current repository contract is legacy schema v5. Jig supports custom commands and gates there, but a separate existing issue owns the eventual schema-v6 migration. This plan will not absorb that migration.
- `reuse = true` reuses exact scoped evidence across plans; rerunning `work check` inside the same plan deliberately executes its selected gates again.

## Decision Log

- Decision: Preserve the existing locked test command and GitHub workflows as the comprehensive merge boundary.
  Rationale: Local fail-fast behavior and path-scoped dogfood gates are feedback optimizations, not permission to reduce CI coverage.
  Date: 2026-08-28.

- Decision: Add a `local` Nextest profile with `fail-fast = true`; leave `default` comprehensive with `fail-fast = false`.
  Rationale: Local commands should return useful failures quickly, while CI should collect the complete failure set.
  Date: 2026-08-28.

- Decision: Split dogfood Rust tests into core, frontend, vault, and process/signal partitions.
  Rationale: These correspond to the measured cost centers and existing Nextest test groups. Root Cargo/toolchain/test-runner changes deliberately select every partition; area changes select core plus their owning partition.
  Date: 2026-08-28.

- Decision: Keep manager script generation real, but perform it once per package manager in each large frontend matrix and copy only the generated scripts into each row's minimal repository.
  Rationale: The tests continue validating generated production scripts, while eliminating unrelated template materialization from every combinatorial case. Existing scaffold-generation and scaffold-runtime tests remain full `run_init` end-to-end coverage.
  Date: 2026-08-28.

- Decision: Use `Vault::resolve_for_test` for behavior tests that initialize state and retain explicit production-parameter assertions as the production-KDF contract.
  Rationale: Envelope headers remain authoritative when opening fixtures, and behavioral correctness does not require paying the production password-hardening cost hundreds of times.
  Date: 2026-08-28.

- Decision: Poll for a positive, parseable PID rather than mere path existence.
  Rationale: A shell redirection creates the file before writing its contents, so path readiness is not process readiness.
  Date: 2026-08-28.

## Context and Orientation

The workspace is a Rust monorepo. The `jig-sh` binary and most integration tests live under `crates/jig`; vault behavior lives under `crates/jig-vault`; frontend project generation is implemented by `crates/jig/src/bootstrap` and templates under `templates/project`.

Test execution is configured in `.config/nextest.toml`. The repository's typed dogfood contract is `.agent/jig-contract.json`, command implementations are selected by `.jig.toml`, and the structured gate runtime lives in `crates/jig/src/work`. GitHub's complete Linux and macOS test jobs are in `.github/workflows/rust-tests.yml`.

The frontend matrices targeted first are:

- `generated_web_dependency_scope_requires_workspace_membership_and_honors_app_locks` in `crates/jig/src/bootstrap/tests/frontend_adoption/pnpm.rs`;
- `generated_web_dependency_scope_and_fingerprints_use_only_selected_manager_metadata` in `crates/jig/src/bootstrap/tests/frontend_adoption/dependency_receipts_parts/part_01.rs`.

Both use the bootstrap test support in `crates/jig/src/bootstrap/tests.rs`. The generated `scripts/check-webapps.sh` calls `scripts/web-node.cjs`; those two generated artifacts plus the row-specific manifests, lockfiles, fake package-manager executable, and `.agent/tmp` directory form the minimal fixture.

The PID race is in `crates/jig/tests/codex_launcher.rs`. Two tests currently call `wait_for_path` and immediately parse the file.

## Plan of Work

### Milestone 1: Test crypto and launcher foundations

Add `[profile.test.package.argon2]` with `opt-level = 3` to the root `Cargo.toml`. Change behavioral test constructors from `Vault::resolve` to `Vault::resolve_for_test` throughout `crates/jig-vault` test modules and Jig's vault integration tests. Inspect each production-KDF assertion before changing it; retain a small explicit suite that proves production defaults are valid and distinct from test defaults.

In `crates/jig/tests/codex_launcher.rs`, replace the path-existence helper with a bounded poll that reads, trims, parses, and validates a positive PID. Return the PID from the helper so callers cannot reintroduce the read-after-exists race.

Acceptance for this milestone:

- the explicit KDF contract tests pass;
- the full vault partition passes;
- the launcher cancellation test passes repeatedly under parallel execution;
- no production source path calls `resolve_for_test`.

### Milestone 2: Minimal generated-script frontend fixtures

Introduce a focused bootstrap test helper that captures the generated `check-webapps.sh` and `web-node.cjs` artifacts from a real `run_init` and installs them, with executable permissions preserved, into a minimal temporary repository.

Refactor the two large package-manager matrices to render once for each distinct manager and reuse the captured artifacts for all matrix rows for that manager. Keep row-specific manifests, workspace membership, lock metadata, fake package managers, and assertions unchanged. Do not replace the existing full scaffold generation/runtime test families.

Acceptance for this milestone:

- both exact matrix tests pass;
- the complete `frontend-node` group passes;
- timing the group demonstrates a material reduction from the 251.756-second warm baseline, or the plan records why the chosen fixtures did not reduce the cost and revises the implementation.

### Milestone 3: Local profiles and dogfood gate selection

Add `[profile.local]` to `.config/nextest.toml`, inheriting the existing group limits and using `fail-fast = true`. Update only the unlocked local Rust test command to select this profile; the locked command and workflows retain the comprehensive default profile.

Add a repository script with four explicit test modes:

- `core`: `jig-sh` tests excluding the frontend, vault, and process/signal filters, plus all other workspace packages not owned by those partitions;
- `frontend`: the existing `frontend-node` filter;
- `vault`: the existing vault filter, retaining the single-threaded vault TUI treatment;
- `process`: the existing `process-signals` filter.

Expose those modes as typed legacy contract commands and replace the one broad source-Rust gate with four path-scoped gates. Every Rust source change still selects `core`; frontend, vault, and process ownership paths additionally select their specific gate. Root Cargo manifests, the lockfile, toolchain/config, and the partition script select all four. Mark deterministic local gates `reuse = true`.

Because schema v5 is transitional, document the custom command mapping clearly and validate it with `scripts/jig check contract`. Do not migrate the whole repository contract to v6 in this work.

Acceptance for this milestone:

- the contract validates using the freshly built dev Jig binary;
- each partition command passes independently;
- a gate-selection dry run or focused contract test proves representative core, frontend, vault, process, and root-config paths select the intended gates;
- `rust_test_locked_command` and `.github/workflows/rust-tests.yml` still execute comprehensive coverage.

### Milestone 4: Integrated verification

Format and lint touched code. Run exact tests while iterating, then the complete local Rust suite using `scripts/jig check test`. Rebuild the dev binary after implementation and force all harness checks through it:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig check contract
    scripts/jig work check --plan-id plan_01M13VEB70PY7RW6RGXMTNZ4BZ
    scripts/jig work gates --plan-id plan_01M13VEB70PY7RW6RGXMTNZ4BZ
    scripts/jig work evidence --plan-id plan_01M13VEB70PY7RW6RGXMTNZ4BZ
    scripts/jig work receipts --plan-id plan_01M13VEB70PY7RW6RGXMTNZ4BZ
    scripts/jig work status

Review the final diff for accidental fixture identifiers, stale command documentation, and coverage gaps. Close and sync the dedicated Beads tasks only after all applicable gates are satisfied.

## Concrete Steps

Run all commands from `/Users/aa/Documents/jig-sh`.

1. Record issue ownership:

       br create --title="Reduce dogfood test feedback time" --type=feature --priority=1 --status=in_progress --json

   Create child tasks for the crypto/launcher work, frontend fixtures, local profiles/gates, and final validation. Add final-validation dependencies on the implementation children. Run `br sync --flush-only`.

2. Apply Milestone 1 edits and verify:

       cargo nextest run -P local -p jig-vault
       cargo nextest run -P local -p jig-sh --test codex_launcher -E 'test(sigint_cancels_all_active_home_inspections_before_redelivery)' --stress-count 50

3. Apply Milestone 2 edits and verify the two exact tests, then:

       /usr/bin/time -p cargo nextest run -P local -E 'package(jig-sh) & (test(bootstrap::tests::frontend_adoption) | test(bootstrap::tests::basic::scaffold_generation) | test(bootstrap::tests::basic::scaffold_runtime))'

4. Apply Milestone 3 edits. Validate shell syntax, contract, and all four modes:

       bash -n scripts/test-rust-partition.sh
       scripts/test-rust-partition.sh core
       scripts/test-rust-partition.sh frontend
       scripts/test-rust-partition.sh vault
       scripts/test-rust-partition.sh process

5. Run integrated verification:

       cargo fmt --all -- --check
       scripts/jig check clippy
       scripts/jig check test

   Then rebuild Jig and run the structured work commands listed in Milestone 4.

## Validation and Acceptance

The work is accepted when all of the following are true:

- all 3,109-or-more existing tests still pass, with any newly added regression tests also passing;
- production vault KDF defaults and persisted envelope compatibility are unchanged;
- vault behavioral tests use explicit test parameters unless their purpose is to exercise the production parameters;
- the frontend behavior matrices still cover npm, pnpm, yarn, and bun metadata/workspace cases while avoiding a full template render per row;
- local failures stop early, while locked CI continues collecting complete results;
- changed-path gate selection is conservative: no owned subsystem can change without selecting its partition, and shared build inputs select all partitions;
- the launcher regression no longer observes an empty PID under repeated parallel runs;
- the complete local test command, contract check, configured work gates, evidence, and receipts are green;
- the final measured frontend and vault times are recorded below.

## Idempotence and Recovery

All source and configuration edits are ordinary Git-tracked changes. Re-running `cargo build`, Nextest partitions, Jig checks, and Beads sync is safe. The plan and Jig state are append-only through the supported commands; do not edit `.agent/state/*.jsonl` directly. If a partition command is misconfigured, restore coverage by comparing its union against the unchanged locked command rather than deleting tests. If a custom legacy command cannot pass contract validation, leave the broad existing gate in place until the replacement is valid.

No database migration, public API migration, persisted-state rewrite, or external rollout is involved.

## Artifacts and Notes

Baseline observations:

- successful full-receipt median: 916.212 seconds;
- warm non-vault partition: 300.315 seconds for 2,679 tests;
- warm vault partition before optimization: 162.272 seconds for 428 tests;
- warm vault partition with optimized Argon2: 23.823 seconds for 428 tests;
- warm frontend partition: 251.756 seconds for 107 tests;
- exact Argon2 test: 26.298 seconds before, 1.688 seconds optimized.

Beads work graph:

- parent: `feat-codex-resume-generic-monorepo-i53`;
- crypto and PID readiness: `feat-codex-resume-generic-monorepo-i53.1`;
- frontend fixtures: `feat-codex-resume-generic-monorepo-i53.2`;
- profiles and gates: `feat-codex-resume-generic-monorepo-i53.3`;
- integrated validation: `feat-codex-resume-generic-monorepo-i53.4`, blocked by the three implementation tasks.

Final observations will be added after validation.

Interim observations:

- the full vault-filter partition passed 430 tests in 15.261 seconds of test execution and 20.29 seconds wall time including a 4.78-second incremental rebuild;
- the launcher PID regression passed 50 of 50 stress iterations at eight test threads.
- the complete 107-test frontend group passed in 244.088 seconds of test execution and 244.46 seconds wall time, down 7.30 seconds (2.9%) from the 251.756-second warm baseline; the remaining cost is dominated by generated-script behavior, especially Yarn PnP and dependency-authority cases.
- the partition filters are pairwise disjoint and cover all 3,109 tests exactly: core 2,365, frontend 107, process/signal 207, and vault 430;
- the actual local gate commands passed in 75.32 seconds (core), 244.04 seconds (frontend), 27.32 seconds (process/signal), and 15.61 seconds (vault).

## Interfaces and Dependencies

No new runtime crate dependency is planned.

The root Cargo profile interface will be:

    [profile.test.package.argon2]
    opt-level = 3

The Nextest profile interface will be:

    [profile.local]
    fail-fast = true

It inherits `test-threads = 4`, the slow timeout, and group overrides from `profile.default`.

The new shell command accepts exactly one positional partition name: `core`, `frontend`, `vault`, or `process`; an unknown or missing mode exits nonzero with usage.

The bootstrap test helper owns captured generated script bytes and their executable modes and exposes an install operation for a target temporary repository. It remains test-only.

The launcher helper accepts a PID file path and timeout and returns a positive parsed process ID or panics with a diagnostic containing the path and last observed state.

## Outcomes & Retrospective

The implementation achieved the intended local-feedback cutover without reducing coverage:

- the complete `scripts/jig check test` run passed all 3,109 tests in 374.14 seconds (6 minutes 14 seconds), 59.2% below the recent successful dogfood-receipt median of 916.212 seconds;
- the four filters are pairwise disjoint and their counts sum exactly to the full suite;
- an ordinary Rust source change selects the 2,365-test core partition, which passed in 75–76 seconds, while frontend, vault, and process changes add only their owning partition;
- the vault partition passed 430 tests in 15.8 seconds in the authoritative gate, versus a 162.272-second warm baseline before the Argon2/profile and test-KDF changes;
- the frontend partition passed 107 tests in 245.8 seconds. Minimal fixtures saved about seven seconds, but real package-manager behavior remains its long tail; path isolation is the larger win;
- the launcher readiness regression passed 50 of 50 stress iterations at eight test threads;
- the locked command and GitHub workflows remain unchanged and comprehensive.

The final work check executed all eight applicable gates successfully under the freshly built `target/debug/jig`; batch receipt `receipt_01M13Z5C80CDDFMBXKBJRRAFCQ` is fresh. Reusable cross-plan evidence is enabled for formatting, Clippy, and all four test partitions.

The main follow-up opportunity is within frontend behavior execution, especially Yarn PnP and dependency-authority scenarios that individually take 25–47 seconds. Any future optimization there should preserve their real process/lock semantics rather than merely replacing them with mocks.
