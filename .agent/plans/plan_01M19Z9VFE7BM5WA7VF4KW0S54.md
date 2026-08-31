# Integrate guided discovery and strict/default preset interaction

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current in accordance with `.agent/PLANS.md`.

The structured work identifier is `plan_01M19Z9VFE7BM5WA7VF4KW0S54`. The owning Beads task is `jig-sh-rust-only-init-presets-zc7.2.3` (B05). The exact Git baseline is commit `2039030e42fa46ab474eebd97eb2aa489d5c5e38`. The working tree also contains the completed, uncommitted B03/B04 Rust-only prepared-answer and explicit-preset implementation; preserve those dependency changes and treat them as the authoritative foundation for this task.

## Purpose / Big Picture

After this change, a person running `jig init` interactively can discover and select all five public project shapes. Existing choices `1` through `3` remain Rust React, harness-only, and Go React, an empty answer still selects Rust React, and new choices `4` and `5` select the Rust library and Rust CLI presets without asking irrelevant database or frontend questions. Automation continues to use the already completed explicit `--preset`, `--defaults`, `--no-input`, and non-terminal behavior.

Long help, strict-mode errors, invalid wizard input, and Doctor recovery guidance must describe the complete five-preset family without implying that Rust-only presets need database or frontend choices. The public preset descriptors, order, init reports, JSON output, human summaries, generated files, and answer policy established by B03/B04 remain unchanged.

## Progress

- [x] (2026-08-30 18:36Z) Revalidated B05 and its closed B04 dependency, claimed the Bead, built on Git baseline `2039030e42fa46ab474eebd97eb2aa489d5c5e38`, opened structured work, and linked the plan identifier from the Beads record.
- [x] (2026-08-30 18:36Z) Read repository/crate/ExecPlan guidance and mapped the wizard, strict/default policy, package-manager preflight, long help, Doctor recovery message, and existing B03/B04 regression tests.
- [x] (2026-08-30 18:40Z) Added five-choice guided discovery, exact aliases, descriptor-backed summaries, and capability-driven no-prompt behavior while preserving numeric choices `1` through `3`, legacy aliases, and default `1`.
- [x] (2026-08-30 18:41Z) Replaced stale strict-mode, long-help, and Doctor recovery prose with complete capability-accurate five-preset guidance.
- [x] (2026-08-30 18:44Z) Added the interaction/default/strict/non-terminal/preflight/help/Doctor matrix in a dedicated 322-line module; passed 65 wizard tests, focused help and Doctor tests, nine preset tests, B03/B04 JSON/human process regressions, source LOC, formatting, and strict all-target Clippy.
- [x] (2026-08-30 19:06Z) Built and dogfooded the changed runtime; passed the broad Cargo suite, the 3,151-case repository test contract, and all eight fresh structured gates; audited the B05 requirement matrix, final diff, formatting, stale diagnostics, and fixture hygiene.
- [x] (2026-08-30 19:07Z) Reconfirmed all eight gates fresh, finished structured work successfully, closed B05, and flushed the Beads export (already current).

## Surprises & Discoveries

- Observation: B03/B04 already made default application choices, strict completeness checks, interactive database/frontend prompts, and package-manager preflight depend on typed `ScaffoldPreset` capabilities.
  Evidence: `apply_project_shape_defaults`, `validate_project_shape_resolved`, `guide_project_shape`, and `preflight_init_package_manager_with` in `crates/jig/src/cli/init_wizard.rs` call `requires_database_choice`, `requires_frontend_choice`, `requires_go_module`, or `requires_web_package_manager`.

- Observation: the remaining wizard selection is a private three-variant `ScaffoldChoice`, so the public `RustLibrary` and `RustCli` enum values cannot yet be chosen interactively even though explicit strict and default execution already works.
  Evidence: `prompt_scaffold_choice` accepts only choices `1` through `3`, and `guide_project_shape` has only Rust React, harness-only, and Go React match arms.

- Observation: Doctor's missing-repository recovery currently shows only a prompt-free harness-only command, while init long help has examples for both Rust-only presets but its `--no-input` explanation still talks only about application and harness-only shapes.
  Evidence: `crates/jig/src/doctor_parts/part_01.rs` and the `InitOpts` long help in `crates/jig/src/bootstrap_parts/part_01.rs`.

- Observation: the existing general interaction test module and Doctor test part remain below the 1,000-line Rust limit but have limited headroom, while B05's complete matrix is independently meaningful.
  Evidence: after adding only stale-oracle updates, `init_wizard_tests.rs` is 716 lines and `doctor/tests_parts/part_08.rs` is 975 lines; the new `init_wizard_discovery_tests.rs` is 322 lines and the LOC check passes.

- Observation: the first `scripts/jig check test` Nextest run hit two unrelated temporary-process cleanup failures in isolated Git fixtures after both tests had passed in the broad Cargo suite.
  Evidence: failure receipt `receipt_01M1A0T408ENNTCX62643E5G8G` reported only “the process tree could not be cleaned up safely”; both exact tests then passed alone, and the unchanged full harness retry passed all 3,151 tests under receipt `receipt_01M1A0Y92QD1HY20P7HDEMYM8M`.

- Observation: the structured gate engine could reuse frontend, vault, and process receipts because the current changes did not alter their gate-scoped fingerprints.
  Evidence: `work gates` reports all eight required gates fresh; the five current executions and three fingerprint-matched reuses are grouped under batch receipt `receipt_01M1A0ZWQM3B6EDS0BVE01FPS2`.

## Decision Log

- Decision: append `4` as `rust-library` and `5` as `rust-cli`, accepting their exact public spellings as the new text aliases while preserving every existing choice and legacy alias for choices `1` through `3`.
  Rationale: B05 requires appended numeric choices and exact text aliases; using `ScaffoldPreset::as_str()` spellings keeps discovery aligned with Clap and avoids inventing ambiguous `library` or `cli` aliases.
  Date/Author: 2026-08-30, Codex.

- Decision: render Rust-only wizard descriptions from the finalized `ScaffoldPreset::descriptor()` summaries rather than copy new descriptive prose into the wizard.
  Rationale: B05 must discover B03/B04 output without redefining it. Descriptor-backed rows prevent wording drift and make descriptor regressions visible in the interaction tests.
  Date/Author: 2026-08-30, Codex.

- Decision: keep the existing typed capability calls as the sole source for database, frontend, Go module, and package-manager requirements.
  Rationale: those calls already encode the complete five-preset matrix. Adding identity-specific prompt or preflight exceptions would duplicate B03/B04 policy and create drift.
  Date/Author: 2026-08-30, Codex.

- Decision: change the wizard question from “Scaffold an app?” to “Project shape?” and make strict/help prose name application presets separately from harness-only and Rust-only presets.
  Rationale: a library, CLI, or harness-only repository is not an application scaffold, and B05 explicitly forbids diagnostics that imply every non-harness preset requires database/frontend choices.
  Date/Author: 2026-08-30, Codex.

## Outcomes & Retrospective

Implementation, validation, structured-work closure, and Beads closure are complete. The guided wizard now exposes all five public presets in stable order, preserves the old default and aliases, and routes the three non-application shapes through the existing capability model without database, frontend, Go-module, or package-manager reads. Strict and default execution remain shared with explicit presets, while missing-preset errors, long help, and Doctor recovery now describe the complete family without assigning application requirements to Rust-only shapes.

The dedicated 322-line acceptance module directly proves numeric and text selection, retry output, descriptor-backed headers, prompt consumption, defaults, strict/non-terminal completeness, and package-manager behavior. Existing exact B03/B04 preset, scaffold, JSON, and human-summary regressions remain green. `cargo test -p jig-sh --locked` passed 2,106 library tests and every integration target with zero failures; `scripts/jig check test` passed all 3,151 Nextest cases on the clean retry; and all eight plan-linked gates are fresh and passing. No public preset descriptor, order, generated output, or serialized contract needed to change for B05.

## Context and Orientation

`crates/jig/src/bootstrap_parts/part_01.rs` defines `InitOpts`, its Clap help, and the public `ScaffoldPreset` enum. “Long help” is the detailed output from `jig init --help`, including the after-help examples and expanded flag explanations.

`crates/jig/src/bootstrap/presets.rs` owns `ScaffoldPreset` capabilities and finalized descriptors. A capability states whether a preset supports or requires a database, frontends, or a Go module. Rust React and Go React require database/frontend choices; Go React also requires a module. Harness-only, Rust library, and Rust CLI require none. Only presets supporting frontends require a web package-manager executable.

`crates/jig/src/cli/init_wizard.rs` prepares answers once, selects `Interactive`, `Defaults`, or `Strict` policy, and resolves project shape. `apply_project_shape_defaults` preserves bare `--defaults` as Rust React with no database and the `web` frontend. `validate_project_shape_resolved` owns `--no-input` and implicit non-terminal errors. `guide_project_shape` owns terminal prompts. `prompt_scaffold_choice` currently exposes only three private choices. `preflight_init_package_manager_with` is the injectable test seam proving whether a package-manager executable is consulted.

`crates/jig/src/doctor_parts/part_01.rs` returns structured Doctor checks. When repository discovery fails, its `repo` check contains a `fix` string with recovery commands. `crates/jig/src/doctor/tests_parts/part_08.rs` owns the corresponding exact assertion.

`crates/jig/src/cli/init_wizard_tests.rs`, `crates/jig/src/cli/noninteractive.rs`, and the B03/B04 modules `init_wizard_rust_library_tests.rs` and `init_wizard_rust_cli_tests.rs` already cover existing interaction and explicit Rust-only policy. Add a focused sibling test module rather than growing an existing Rust file toward the repository's 1,000-line limit. `crates/jig/src/cli/help_tests.rs` owns rendered Clap help assertions. `crates/jig/src/cli/bootstrap_run_tests.rs` and the `cli_json` integration tests own finalized descriptor, public order, JSON, and human-summary oracles.

## Plan of Work

First, extend `crates/jig/src/cli/init_wizard.rs`. Add `RustLibrary` and `RustCli` to the private selection enum and route them directly to the matching public `ScaffoldPreset`. Append rows `4` and `5` to the header using the finalized descriptor summaries. Expand the question and retry diagnostic to list all five numbers and exact public spellings, but preserve default `1` and every existing alias. Leave database/frontend/Go prompting behind the existing capability methods so the new choices complete after the first answer with no hidden reads.

Second, update user-visible guidance. In `validate_project_shape_resolved`, make the missing-preset error enumerate the complete family and explain that only application presets need explicit database/frontend choices. Preserve the existing capability-specific missing database, frontend, and Go-module errors. In `crates/jig/src/bootstrap_parts/part_01.rs`, keep bare `--defaults` semantics exact while making `--no-input` long help distinguish Rust React/Go React requirements from complete harness-only/Rust-library/Rust-CLI shapes. Keep examples for all five public presets. In `crates/jig/src/doctor_parts/part_01.rs`, retain adoption recovery and enumerate all five init choices plus prompt-free commands for the complete non-application shapes.

Third, add a focused interaction matrix module under `crates/jig/src/cli/`. Test every numeric selection and exact public spelling, old aliases, invalid-input retry text, empty-answer default, exact header order and descriptor summaries, and the absence of database/frontend prompts for Rust library and Rust CLI. Prove Rust React and Go React still prompt for their capability requirements and harness-only remains exact. Test bare and explicit `--defaults`, `--no-input`, and implicit non-terminal combinations across all five presets, including exact incomplete-application errors. Inject an always-false package-manager probe and prove both Rust-only presets bypass it while application presets still fail when their selected manager is missing.

Fourth, update help and Doctor tests with exact complete-family assertions. Rerun the existing B03/B04 preset-order, descriptor, init-summary, JSON, and human-summary tests without changing their expected product output. Run the source LOC gate before broad validation so test organization remains compliant.

Finally, review the diff and every fixture for open-source hygiene, build `target/debug/jig`, force repo commands through `JIG_DEV_BIN=target/debug/jig`, run all applicable structured checks, inspect gates/evidence/receipts, and verify no B05 requirement lacks direct evidence. Only then finish the plan, close the Bead, and flush its JSONL export. No commit or push is part of this ExecPlan unless the user explicitly requests it.

## Concrete Steps

Work from `/home/aa/.herdr/worktrees/jig-sh/worktree-silver-harbor-4827`. During implementation use focused commands such as:

    cargo fmt --all -- --check
    cargo test -p jig-sh cli::init_wizard --lib --no-fail-fast
    cargo test -p jig-sh cli::help_tests::init_help --lib
    cargo test -p jig-sh doctor_reports_invalid_configured_repo_root --lib
    cargo test -p jig-sh presets --lib
    cargo test -p jig-sh --test cli_json rust_library
    cargo test -p jig-sh --test cli_json rust_cli
    cargo clippy -p jig-sh --all-targets --locked -- -D warnings

Build and dogfood the changed runtime before structured checks:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M19Z9VFE7BM5WA7VF4KW0S54
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M19Z9VFE7BM5WA7VF4KW0S54
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M19Z9VFE7BM5WA7VF4KW0S54

At successful completion only:

    JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01M19Z9VFE7BM5WA7VF4KW0S54 --resolution 'B05 guided discovery and strict/default interaction acceptance complete.' --outcome success
    br close jig-sh-rust-only-init-presets-zc7.2.3 --reason 'Completed guided discovery and strict/default preset interaction.' --json
    br sync --flush-only

## Validation and Acceptance

The interactive header must contain exactly the existing first three rows followed by `4. rust-library` and `5. rust-cli`, with the Rust-only summaries equal to their finalized descriptors. Inputs `1` through `5` and exact strings `rust-react`, `harness-only`, `go-react`, `rust-library`, and `rust-cli` must select the matching public preset. An empty answer and existing aliases must preserve prior behavior. Invalid input must retry with complete five-choice guidance.

Selecting Rust library or Rust CLI must consume no database, frontend, Go-module, or package-manager availability input. Harness-only must remain similarly prompt-free. Rust React must still prompt for database and frontend, and Go React must still prompt for database, frontend, and module. These conditions must be asserted from exact captured prompt output and resolved `InitOpts`, not inferred only from capability unit tests.

Bare `--defaults` must remain Rust React, database none, and frontend web. Explicit `--defaults` for each preset must preserve that preset and apply only its required defaults. `--no-input` and implicit non-terminal execution must accept each complete explicit shape, accept both Rust-only presets with no shape flags, and produce capability-accurate exact errors for a missing preset or incomplete application preset. The missing-preset error, long help, and Doctor fix must name all five public presets and must not claim Rust library or Rust CLI requires a database or frontend.

An always-false executable probe must not block Rust library or Rust CLI after interaction preparation. It must continue to reject a complete Rust React or Go React shape selecting a missing package manager. B03/B04 order, descriptor, report, generated workspace, JSON, and human-summary tests must retain their current exact oracles. JSON process output must remain parseable without prompt or diagnostic contamination.

Completion requires focused interaction/help/Doctor tests, the current B03/B04 regressions, formatting, strict Clippy, relevant broad `jig-sh` tests, the Rust LOC check, and every applicable structured gate with fresh evidence. The acceptance audit must map every B05 bullet to a named test or direct output.

## Idempotence and Recovery

Source edits, formatting, builds, and tests are repeatable. The task changes no persistent application schema or public serialized state. `.agent/state/*.jsonl` is append-only; retries may append receipts but must not rewrite old entries. Temporary repositories in tests must use generic names such as `ExampleProject`, `ExampleLibrary`, and `ExampleCli` and are removed with their temporary directories.

If a test exposes incomplete explicit Rust-library or Rust-CLI behavior, stop B05 and repair or reopen its owning B03/B04 work rather than adding a guided-path exception. If structured checks fail, leave the plan and Bead in progress, record the failure in this document, repair the shared typed boundary, and rerun the applicable gate. Preserve all pre-existing uncommitted B03/B04 changes; do not reset or overwrite them.

## Interfaces and Dependencies

No new dependency is expected. The public `ScaffoldPreset` order, spelling, capabilities, and descriptors remain defined in `crates/jig/src/bootstrap/presets.rs` and `crates/jig/src/bootstrap_parts/part_01.rs` without product changes.

The private `ScaffoldChoice` in `crates/jig/src/cli/init_wizard.rs` gains `RustLibrary` and `RustCli`. `prompt_scaffold_choice` continues to return that private type and `guide_project_shape` maps it one-to-one to the corresponding public `ScaffoldPreset`. `ScaffoldPreset::requires_database_choice`, `requires_frontend_choice`, `requires_go_module`, and `requires_web_package_manager` remain the only requirement predicates.

`InitInteractionPolicy::resolve`, `apply_project_shape_defaults`, `validate_project_shape_resolved`, `guide_project_shape`, and `preflight_init_package_manager_with` retain their existing roles. Do not add a second answer-file loader, a second Rust-only validator, an identity-specific package-manager bypass, or a CLI-only default path.

Revision note (2026-08-30, Codex): created the self-contained B05 ExecPlan after inspecting the complete Beads/product contract, repository and crate guides, current B03/B04 dependency work, typed interaction capabilities, help tests, and Doctor recovery ownership.

Revision note (2026-08-30, Codex): recorded the implemented five-choice surface, capability-accurate messages, dedicated acceptance matrix, test-file sizing decision, and focused green evidence before broad structured validation.

Revision note (2026-08-30, Codex): recorded the broad Cargo and repository-harness results, the reproduced transient cleanup failure and clean retry, and fresh eight-gate structured evidence before final audit and closure.

Revision note (2026-08-30, Codex): recorded successful structured-work completion and Beads closure after the final fresh-gate check.
