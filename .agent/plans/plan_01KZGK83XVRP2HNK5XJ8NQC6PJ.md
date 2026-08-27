# Make the full Rust test suite fast and process-isolated

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while implementation proceeds. This document is maintained in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

The repository's configured Rust suite currently spends more than thirteen minutes executing tests after compilation has already finished. Most of that time is inside the `jig-sh` unit-test process, where tests that mutate process-global environment variables or the process working directory queue behind one mutex. The suite also repeats the production-strength vault password derivation many times, packages five web dependency scenarios into one 89-second test, and runs real process/signal tests after the main unit-test binary because Cargo executes test binaries sequentially.

After this change, `scripts/jig check test` and `scripts/jig check test-locked` will run every Rust test in an isolated test process through cargo-nextest. A test process is an operating-system process created for one test case; its environment variables, current directory, signal handlers, and crashes cannot interfere with another test. Vault behavior tests will use explicitly injected, valid low-cost Argon2 parameters while production vault creation will retain the existing 128 MiB, three-iteration, parallelism-four parameters. The five web dependency cases will be independently scheduled tests. Process/signal tests and memory-heavy vault tests will have explicit runner groups with bounded concurrency. The observable acceptance result is a passing configured full suite whose warm wall time is materially lower than the measured 791.25-second baseline, with production KDF defaults and the legacy Cargo test path also covered by focused checks.

## Progress

- [x] (2026-08-08 14:30Z) Measured the clean baseline: `cargo test --workspace --quiet` passed in 791.25 seconds; a fully warm compile-only pass took 0.27 seconds.
- [x] (2026-08-08 14:32Z) Ranked the baseline: `jig-sh` 697.76 seconds, `jig-vault` 41.42 seconds, `dev_sigint` 21.81 seconds, `jig-dev-proxy` 10.63 seconds, and `cli_json` 10.14 seconds.
- [x] (2026-08-08 14:34Z) Confirmed the web dependency state matrix takes 89.11 seconds alone and recorded 287 `lock_env()` call sites, including 206 bootstrap and 53 runtime call sites.
- [x] (2026-08-08 14:35Z) Opened structured work `plan_01KZGK83XVRP2HNK5XJ8NQC6PJ` and confirmed cargo-nextest 0.9.130 is available on the development machine.
- [x] (2026-08-08 11:20Z) Measured the unchanged suite under nextest: unconstrained Node-backed fixtures failed by signal 11 while 931 tests passed in 102.57 seconds, establishing the need for repository test groups.
- [x] (2026-08-08 11:48Z) Added explicit production and feature-gated fixture KDF constructors, converted vault behavior tests, and kept all production entrypoints on 131,072/3/4.
- [x] (2026-08-08 12:03Z) Split the five-case web dependency state matrix into five named tests sharing the original assertion body; all five passed in 65.81 seconds with bounded parallelism.
- [x] (2026-08-08 12:12Z) Added validated nextest groups for Node-backed bootstrap tests, real process/signal tests, and vault crypto tests.
- [x] (2026-08-08 12:18Z) Cut `.jig.toml` and the release-reachable Jig gates over to nextest, pinned cargo-nextest 0.9.130 in CI, and documented the contributor prerequisite.
- [x] (2026-08-08 12:43Z) Passed focused vault/frontend tests, the 2,052-test locked full suite in 330.07 seconds, the receipt-backed configured gate in 348.49 seconds, format, strict Clippy, contract, agent-map, agent-guide, and Rust 1.85 all-target/all-feature checks.
- [x] (2026-08-08 12:52Z) Reviewed the final diff, confirmed fresh passing contract/test gate evidence, reconciled the added test count, and prepared structured-work closure.

## Surprises & Discoveries

- Observation: Compilation is not the bottleneck. After the initial build, `cargo test --workspace --no-run --quiet` took 0.27 seconds, while test execution took 791.25 seconds.
  Evidence: the baseline `/usr/bin/time` transcript recorded `full_parallel wall=791.25` and `warm_no_run wall=0.27`.

- Observation: The default libtest process used only about 1.36 CPU cores on a 64-core host because many workers waited on `lock_env()`.
  Evidence: the passing full run recorded `cpu=136%`, and dozens of tests reported that they had been running for more than 60 seconds while the process made progress in bursts.

- Observation: The local Node 22.23.2 binary can segfault on one generated `for ... fs.rmSync` command, and alternate serial/direct test topologies exposed frontend failures that the passing default run did not expose. This makes process isolation a correctness requirement as well as a performance optimization; every isolated failure must be understood rather than hidden.
  Evidence: `strace` observed Node exit by `SIGSEGV` in `scaffold_sqlite_branch_generates_sqlite_db_helper` during a diagnostic exact run.

- Observation: Unconstrained nextest correctly isolated environment and working-directory state, but it also allowed enough simultaneous external Node processes to expose the environment-managed `<private-path>` resource failure. The repository's supported stock Node 22.22.2 binary passed when all 97 Node-backed bootstrap tests shared a two-slot group.
  Evidence: the first configured attempt finished 2,022 passing and 30 signal-11 failures in 69.41 seconds; after expanding the group and using the exact supported stock Node distribution, the full run passed 2,052 tests.

- Observation: Supplying a global `NODE` override is not equivalent to placing Node on `PATH` for this suite because many bootstrap fixtures deliberately install fake `node` executables at the front of a child-specific `PATH`. The supported validation topology leaves `NODE` unset.
  Evidence: a Yarn fixture failed with the global override because the generated checker bypassed its fake executable, then passed exactly when the same stock Node directory was provided through `PATH` alone.

- Observation: `scripts/jig doctor` currently recognizes `cargo` as the executable in `cargo nextest ...`, but does not separately diagnose a missing Cargo subcommand.
  Evidence: doctor reported both test command keys present through `cargo`; contributor documentation therefore uses `cargo nextest --version` for the explicit prerequisite check.

## Decision Log

- Decision: Adopt cargo-nextest as the configured full-suite runner instead of attempting to make libtest's single `jig-sh` process parallel around process-global state.
  Rationale: cargo-nextest runs each test in a separate operating-system process, directly isolating environment variables, current directory, and signal handlers. This preserves tests that intentionally exercise global state without a risky rewrite of 287 call sites and allows the existing mutex to become uncontended inside each test process.
  Date/Author: 2026-08-08 / Codex

- Decision: Keep `lock_env()` for tests that intentionally mutate process-global state, but make the configured runner process-isolated and remove broad lock use only where an explicit command-local seam is introduced.
  Rationale: deleting guards before all affected production APIs accept explicit environment and working-directory inputs would create races for contributors who still invoke `cargo test`. Process isolation achieves the requested safety immediately; focused seams can then reduce unnecessary global mutation without weakening the fallback path.
  Date/Author: 2026-08-08 / Codex

- Decision: Carry vault creation KDF parameters in `jig_vault::store::VaultStore`, use production defaults in all public constructors, and expose a feature-gated test constructor used only by test code.
  Rationale: changing `KdfParams::default()` under `cfg(test)` does not affect `jig-vault` when it is compiled as a dependency of `jig-sh`, and a global environment switch could accidentally weaken real vault creation. Constructor injection keeps production behavior explicit and permits fast, valid fixtures.
  Date/Author: 2026-08-08 / Codex

- Decision: Preserve one direct assertion of the production KDF parameters while using minimum-valid Argon2 parameters for behavioral fixtures.
  Rationale: behavioral tests need encryption and authentication semantics, not repeated password-hardening cost. A dedicated default-parameter test protects the production security contract without spending minutes recomputing it.
  Date/Author: 2026-08-08 / Codex

- Decision: Bound Node-backed bootstrap tests to two concurrent processes, require the four heaviest filesystem/Yarn cases to reserve both slots, serialize the real process/signal group, and allow four concurrent vault-crypto tests.
  Rationale: these limits came from passing focused and full runs. They retain parallel scheduling for the rest of the workspace while avoiding external-process and memory contention rather than hiding or retrying failures.
  Date/Author: 2026-08-08 / Codex

## Outcomes & Retrospective

Implementation and verification are complete. The authoritative locked nextest run passed 2,052 tests with the two pre-existing ignored tests skipped in 330.07 seconds, including a six-second rebuild. The final exact-diff `scripts/jig work check` passed both the contract and 2,052-test suite in 310.39 seconds. Compared with the 791.25-second passing baseline, that final configured gate saves 480.86 seconds (8 minutes 1 second) and reduces wall time by 60.8%. The separate locked measurement saves 461.18 seconds (7 minutes 41 seconds), a 58.3% reduction. Peak measured resident memory fell from 2,371,104 KiB to 1,215,008 KiB.

The isolated `jig-vault` crate now passes 98 tests in 2.29 seconds versus 39-41 seconds before, and the three `jig-sh` dispatch-vault tests pass in 1.58 seconds versus about 66.5 seconds combined. The five dependency-state cases pass independently in 65.81 seconds versus 89.11 seconds as one loop. Format, Clippy with warnings denied, contract, agent map, all 14 crate guides, and Rust 1.85 all-target/all-feature checking pass. No test was removed, ignored, retried, or filtered from the configured full suite.

## Context and Orientation

The repository root `.jig.toml` defines `rust_test_command` for `scripts/jig check test` and `rust_test_locked_command` for `scripts/jig check test-locked`. The latter currently combines Cargo lockfile enforcement with libtest's `--test-threads=1`, causing all tests inside every test binary to execute serially. `.github/workflows/rust-tests.yml` and `scripts/release.sh` call the locked gate.

Rust's standard test harness, called libtest here, runs all unit tests for one library inside one process. `crates/jig/src/test_env.rs` defines `lock_env()`, which serializes mutations to environment variables and the current directory inside the `jig-sh` unit-test process. This is necessary for correctness under libtest but causes a queue because 287 test call sites use it. Cargo-nextest is an installed Cargo subcommand that discovers the same test binaries but starts each test as its own process. Nextest configuration belongs at `.config/nextest.toml`; a test group is a named concurrency limit applied to selected tests.

The largest single test is `generated_web_checks_track_lockfiles_and_yarn_pnp_install_state` in `crates/jig/src/bootstrap/tests/frontend_adoption/dependency_state.rs`. It loops over npm, npm-shrinkwrap, Yarn Berry PnP, Yarn Classic PnP, and Yarn node-modules cases. Each case creates a generated repository, writes fake package-manager executables, and invokes `scripts/check-webapps.sh` many times. Extracting one helper that accepts a case descriptor and declaring five `#[test]` wrappers preserves behavior while allowing process-isolated scheduling.

`crates/jig-vault/src/crypto.rs` defines the production `KdfParams` defaults: Argon2id with 131,072 KiB of memory, three iterations, and parallelism four. `crates/jig-vault/src/store.rs` currently stores only the vault root. `crates/jig-vault/src/vault.rs` constructs a new header with `KdfParams::default()` inside `VaultStore::init_unlocked`. The vault behavior tests construct `VaultStore` directly in `crates/jig-vault/src/vault_tests.rs` and `crates/jig-vault/src/broker.rs`; `jig-sh` runtime tests reach the public `Vault` facade through `crates/jig/src/runtime/vault.rs`. The test seam must therefore work both for the crate's own unit tests and for `jig-sh` as a dev-dependency, without changing any public production constructor.

Real process and signal tests live primarily in `crates/jig/tests/dev_sigint.rs`, `crates/jig-dev-proxy/src/processes`, and `crates/jig-owned-process`. These tests use operating-system signals, listeners, process groups, and bounded cleanup waits. They should remain real integration tests, but nextest must schedule them in a named group with low enough concurrency to avoid machine-level contention while still overlapping them with unrelated tests. Vault tests need a separate bounded group so several 128 MiB production-cost compatibility checks cannot exhaust memory.

## Plan of Work

First, run cargo-nextest against the unchanged workspace and capture all failures. Use exact reruns with `cargo nextest run --workspace -E 'test(...)'` and ordinary Cargo exact reruns to distinguish isolation bugs from external-tool failures. Record findings in this plan before editing behavior.

Second, add explicit vault KDF injection. Extend `VaultStore` with an initialization KDF field. `VaultStore::resolve` and `Vault::resolve` must always select `KdfParams::default()`. Under `cfg(test)` or the `test-utils` Cargo feature, add a test constructor that selects `KdfParams::for_tests()`, defined as Argon2id with the existing minimum accepted memory, one iteration, and parallelism one. Add `test-utils = []` to `crates/jig-vault/Cargo.toml` and enable it only through `crates/jig/Cargo.toml` dev-dependencies. Update vault unit-test constructors and add a test that compares production and testing parameters. In `crates/jig/src/runtime/vault.rs`, add a resolver function seam so unit-test dispatch uses `Vault::resolve_for_test` while non-test dispatch always uses `Vault::resolve`. Convert direct `Vault::resolve` calls in `jig-sh` unit tests to the test constructor. Verify that a production-created header still serializes 131,072/3/4 and a test-created header uses valid lower parameters.

Third, refactor `dependency_state.rs`. Define a `DependencyStateCase` descriptor containing the case name, manager, lockfile, artifact, initial lockfile contents, Classic PnP flag, and optional Yarn configuration. Move the existing loop body into `run_dependency_state_case(case)`. Add five named tests, each acquiring `lock_env()` and calling the helper with one descriptor. Do not remove assertions or combine manager cases. Run all five through Cargo and nextest, and record individual times.

Fourth, create `.config/nextest.toml`. Define a memory-bounded vault group and a process/signal group. Select all `jig-vault` tests plus `jig-sh` vault runtime tests for the vault group. Select `dev_sigint`, `jig-dev-proxy` process/cleanup tests, and owned-process tests for the process group. Start with conservative concurrency and use nextest's configuration validator and full run to tune it. Do not mark tests slow, ignored, or skipped merely to improve the number.

Fifth, cut over the repository contract. Change `.jig.toml` so normal tests use `cargo nextest run --workspace` and locked tests use `cargo nextest run --workspace --locked`; remove libtest's global `--test-threads=1` because process isolation now supplies correctness. Update `.github/workflows/rust-tests.yml` to install a pinned cargo-nextest release and run the configured locked gate. Remove per-job `RUST_TEST_THREADS=1` where nextest now supplies process isolation only for jobs that use the configured full gate; preserve platform-specific package jobs that continue to use Cargo. Update `scripts/release.sh` only if command names or prerequisites change. Update contributor/bootstrap documentation so a fresh checkout can install or diagnose the required runner. Keep ordinary `cargo test` focused commands supported for crate development.

Finally, build the current `jig` binary and force `JIG_DEV_BIN=target/debug/jig` for all harness checks. Run focused tests for vault production defaults, fast fixtures, all five web dependency cases, and nextest configuration. Run format, strict Clippy, contract, agent-map, and agent-guide checks. Run both configured test gates and time the locked full suite at least twice after a warm compile. Inspect the diff for accidentally weakened vault defaults, skipped tests, stale CI names, or test-count loss.

## Concrete Steps

Work from `.`.

Capture the process-isolated baseline:

    /usr/bin/time -f 'nextest-baseline wall=%e user=%U sys=%S cpu=%P maxrss_kb=%M' cargo nextest run --workspace

Implement and verify vault fixtures:

    cargo test -p jig-vault
    cargo test -p jig-sh runtime::tests::dispatch_vault
    cargo test -p jig-sh runtime::vault::tests

Verify the split web dependency cases:

    cargo test -p jig-sh bootstrap::tests::frontend_adoption::dependency_state
    cargo nextest run -p jig-sh -E 'test(bootstrap::tests::frontend_adoption::dependency_state)'

Build and dogfood the current runtime:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig work check --plan-id plan_01KZGK83XVRP2HNK5XJ8NQC6PJ
    scripts/jig check fmt
    scripts/jig check clippy
    scripts/jig check contract
    scripts/jig check test
    scripts/jig check test-locked

Measure the final warm suite:

    cargo nextest run --workspace --locked --no-run
    /usr/bin/time -f 'nextest-final wall=%e user=%U sys=%S cpu=%P maxrss_kb=%M' cargo nextest run --workspace --locked

Expected final output reports every discovered test passing, no skipped tests beyond the repository's pre-existing ignored tests, and a wall time substantially below 791.25 seconds.

## Validation and Acceptance

The work is accepted only when all five recommendations are demonstrably implemented.

Process-global test state is isolated when the configured full suite uses nextest, exact isolated tests no longer depend on another test's environment/current directory, and `.config/nextest.toml` validates. The fallback Cargo-focused tests must continue to pass for edited crates.

The mega-test is decomposed when nextest lists five separately named dependency-state cases and each passes with the same assertions that covered lockfile changes, install failures, PnP artifacts, receipts, symlinks, and corrupted installs.

Vault tests are fast without weakening production when production KDF defaults remain exactly 131,072 KiB, three iterations, and parallelism four; test fixtures use at least the accepted 19,456 KiB minimum, one iteration, and parallelism one; the vault format/authentication/audit tests all pass; and release builds do not enable the test utility feature.

Process/signal tests are separated when nextest assigns them to a bounded group and all real signal, cleanup, listener, and process-tree assertions pass. They must not be ignored or removed.

The runner cutover is complete when local normal and locked Jig gates, GitHub's full Rust test job, and release checks all use the process-isolated full suite with locked dependency resolution where required. A fresh checkout has a documented/installable cargo-nextest prerequisite and `scripts/jig doctor` can identify its absence from the configured command.

The full suite must pass and be significantly faster. The target is at least a 50% reduction from 791.25 seconds, with all test counts reconciled. If the first nextest result does not meet that threshold, continue profiling and optimizing rather than declaring completion.

## Idempotence and Recovery

All test and formatting commands are safe to rerun. The vault test constructor writes only temporary directories and cannot open a production vault unless a test explicitly passes that path. If nextest configuration filters select the wrong tests, use `cargo nextest list --workspace --show-groups` before rerunning; do not disable a failing test. If CI installation is unavailable, keep the Cargo commands intact until the pinned installer is validated rather than merging a runner command that fresh machines cannot execute.

The repository may contain unrelated open Jig plans, but the worktree was clean when this plan began. Preserve unrelated append-only `.agent/state` records and other plans. Use `git diff` rather than resetting. If an experimental runner change fails, revert only the explicit changed hunks with `apply_patch` and retain the evidence in `Surprises & Discoveries`.

## Artifacts and Notes

Baseline passing suite:

    2,047 passed; 2 ignored across approximately 2,049 discovered tests
    cargo test --workspace --quiet
    wall=791.25s user=754.13s sys=324.33s cpu=136% maxrss=2,371,104 KiB

Largest baseline binary and test:

    jig-sh unit test binary: 697.76s (88.2% of suite)
    generated_web_checks_track_lockfiles_and_yarn_pnp_install_state: 89.11s isolated

Vault baseline:

    jig-vault binary: 39-41s wall, 2.37 GiB peak RSS
    three jig-sh vault dispatch tests: 24.23s, 24.01s, and 18.28s when isolated

## Interfaces and Dependencies

In `crates/jig-vault/src/crypto.rs`, provide production and test parameter constructors while keeping `Default` production-strength:

    impl KdfParams {
        fn production() -> Self;
        #[cfg(any(test, feature = "test-utils"))]
        fn for_tests() -> Self;
    }

In `crates/jig-vault/src/store.rs`, `VaultStore` must carry the KDF used only when initializing new state. Existing vault opens always trust and validate the KDF stored in the authenticated header:

    pub(crate) struct VaultStore {
        root: PathBuf,
        initialization_kdf: KdfParams,
    }

    pub(crate) fn resolve(explicit_home: Option<PathBuf>) -> Result<Self>;
    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn resolve_for_test(explicit_home: Option<PathBuf>) -> Result<Self>;

In `crates/jig-vault/src/vault.rs`, expose `Vault::resolve_for_test` only under the `test-utils` feature or the crate's own unit-test build. It must be documented as test fixture support and must not be called by production runtime paths.

In `crates/jig/src/runtime/vault.rs`, define a resolver seam used by dispatch:

    type VaultResolver = fn(Option<PathBuf>) -> jig_vault::Result<Vault>;
    fn dispatch_with_resolver(command: VaultCommand, resolver: VaultResolver) -> Result<Value>;

The production `dispatch` passes `Vault::resolve`. A `#[cfg(test)]` entrypoint passes `Vault::resolve_for_test`, and `crates/jig/src/runtime.rs` selects that entrypoint only in unit-test builds.

In `crates/jig/src/bootstrap/tests/frontend_adoption/dependency_state.rs`, define `DependencyStateCase` and `run_dependency_state_case`. Five test wrappers must cover the existing case descriptors without reducing assertions.

`.config/nextest.toml` is the only new runner dependency configuration. No new Rust runtime crate dependency is required. The CI installer must pin cargo-nextest, and the repository's configured commands must continue to use Cargo's `--locked` option for release/CI reproducibility.

Revision note (2026-08-08): Created the initial self-contained plan from the measured baseline and the five requested recommendations. The design selects process isolation plus explicit vault injection because those directly address the measured mutex convoy and cryptographic cost without weakening production behavior.

Revision note (2026-08-08): Updated the living plan with the implemented KDF seam, split frontend matrix, validated nextest groups, runner/CI cutover, exact passing counts and timings, compatibility findings, and fresh structured-work gate evidence.
