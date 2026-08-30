# Ship explicit rust-cli init

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current in accordance with `.agent/PLANS.md`.

The structured work identifier is `plan_01M19W9QVK6ETXH73YWHFW5873`. The owning Beads task is `jig-sh-rust-only-init-presets-zc7.2.2` (B04). The exact Git baseline is commit `2039030e42fa46ab474eebd97eb2aa489d5c5e38`.

## Purpose / Big Picture

Expose the already implemented private Rust command-line scaffold as the complete public command `jig init <path> --preset rust-cli --no-input --no-vault`. The command must create a buildable and runnable virtual Cargo workspace with one non-publishable, dependency-free binary crate plus the full Jig harness. Running the generated binary without arguments must exit successfully, print one package-name/package-version line, and write nothing to standard error.

The new preset must reuse the frozen, single-read init-answer boundary delivered by B03. It adds one typed CLI identity and policy branch, not another answer loader or parser. It must preserve the existing and `rust-library` presets, carry no database, frontend, API, application-contract, dev-app, release, or publication authority, and retain initial vault setup through the existing environment-passphrase path.

## Progress

- [x] (2026-08-30 15:43Z) Verified B03 is closed, inspected the complete B04 Beads contract and repository/crate/ExecPlan guidance, claimed B04, built `target/debug/jig`, opened structured work, and captured the exact Git baseline.
- [x] (2026-08-30 15:44Z) Linked the structured-work identifier and ExecPlan path from the B04 Beads record.
- [x] (2026-08-30 15:47Z) Confirmed B02 already contains the private CLI identity, shared renderer branch, templates, explicit binary manifest, `std`-only starter source, run guidance, and embedded snapshot.
- [x] (2026-08-30 16:12Z) Added the public Rust CLI preset, exact typed capabilities and descriptor, public-to-private CLI renderer dispatch, and one shared Rust-only prepared-answer policy parameterized by selected preset identity.
- [x] (2026-08-30 16:28Z) Added exhaustive CLI and answer-file policy tests, exact generated repository and report checks, offline Cargo checks, binary execution, vault ordering/initialization, neutral contract, compatibility, and update/recopy coverage.
- [x] (2026-08-30 18:32Z) Passed focused Rust CLI/library/shared-renderer/descriptor/process tests and the complete locked `jig-sh` suite; built and dogfooded the changed runtime; passed all eight applicable structured gates fresh in batch `receipt_01M19Z3QSA1P9FR35YJPV55B2A`; inspected evidence; and completed the requirement and fixture-hygiene audits.

## Surprises & Discoveries

- Observation: B02 already implements `ScaffoldIdentity::RustCli`, `RustOnlyArtifact::Cli`, the exact five-file renderer path, `[[bin]]`, a `std`-only `src/main.rs`, CLI-specific README guidance, and live plus embedded templates. Only the public preset cannot reach it.
  Evidence: `crates/jig/src/bootstrap/scaffold/project.rs`, `crates/jig/src/bootstrap/scaffold/rust_only_workspace.rs`, `templates/scaffolds/rust-only`, `templates/scaffolds/rust-cli`, and `crates/jig/src/bootstrap/scaffold/embedded_templates_snapshot.rs`.

- Observation: B03 centralized the normative compatibility matrix in `AnswerInput::validate_rust_library` behind `PreparedInitAnswers::validate_selected_preset`; duplicating that body for CLI would create policy drift and violate B04's no-second-validator requirement.
  Evidence: `crates/jig/src/bootstrap/answers.rs`, `crates/jig/src/bootstrap/answers/input.rs`, and the completed B03 ExecPlan `.agent/plans/plan_01M19KR0RZZ9TA2DXZ58ZJCSZ9.md`.

- Observation: explicit strict/no-terminal and `--defaults` behavior is already capability-driven. A Rust CLI capability row with no required database, frontend, or Go-module choices is sufficient; B05 deliberately owns the guided menu and later diagnostic prose.
  Evidence: `ScaffoldPreset::capabilities` in `crates/jig/src/bootstrap/presets.rs` and `InitInteractionPolicy` in `crates/jig/src/cli/init_wizard.rs`.

- Observation: Cargo commands executed from a generated fixture inherit the parent test process's target-directory authority, so a binary is not reliably located beneath `<fixture>/target` even after successful generated checks.
  Evidence: the first process acceptance run passed generated Cargo checks but found no `<fixture>/target/debug/examplecli`; executing `cargo run --quiet --locked -p examplecli` from the fixture gives Cargo ownership of target selection and proves exact process output without compiler noise.

- Observation: keeping process coverage in the existing `cli_json.rs` file crossed the repository's 1,000-line Rust source limit even though the implementation itself stayed compact.
  Evidence: the LOC gate identified the oversized file; moving only the new Rust CLI process tests to `tests/cli_json_parts/rust_cli.rs` restored the limit while preserving one integration-test binary and shared helpers.

## Decision Log

- Decision: rename/generalize the existing Rust-library validator into one Rust-only validator parameterized by the selected public preset identity, then route both `RustLibrary` and `RustCli` through it.
  Rationale: the accepted and rejected inputs are identical for both presets; one implementation guarantees shared policy, preserves the frozen prepared input, and makes every error name the actual selected preset.
  Date/Author: 2026-08-30, Codex.

- Decision: leave interactive menu discovery and its aliases unchanged in B04 while making explicit strict/no-terminal and `--defaults` succeed through capabilities.
  Rationale: the owning task assigns guided interaction and diagnostic prose to B05 but explicitly assigns the typed explicit paths to B04.
  Date/Author: 2026-08-30, Codex.

- Decision: reuse B02's exact source and manifest templates without adding a generated test crate or third-party parser/logging dependency.
  Rationale: process-level acceptance in the Jig source tree proves the intentionally replaceable smoke behavior without transferring that behavior into the generated project's permanent test contract.
  Date/Author: 2026-08-30, Codex.

- Decision: execute the generated binary through quiet, locked, offline Cargo rather than assume a fixture-local target directory.
  Rationale: this exercises the exact generated package under Cargo's effective target authority and keeps stdout/stderr assertions independent of the parent test environment.
  Date/Author: 2026-08-30, Codex.

## Outcomes & Retrospective

`rust-cli` is now a public, typed explicit-init preset appended after `rust-library`. It reaches B02's private CLI renderer one-to-one and emits the exact five project-owned files plus the neutral Jig harness. The generated dependency-free binary passes offline setup, fmt, strict Clippy, test, locked test, and docs, then prints exactly `examplecli 0.1.0\n` with empty standard error.

Both Rust-only presets now use one frozen, single-read prepared-answer validator parameterized by selected preset identity. Acceptance covers strict/no-terminal and `--defaults` execution, CLI precedence, supported inert package-manager authority without an executable probe, harness-only nested answers, environment-authorized vault setup, and exact early errors for every incompatible family. Update/recopy leaves project-owned README and source files untouched. Existing Rust library and broader behavior remain compatible.

Validation completed without unresolved gates. Focused tests passed, the full locked `jig-sh` suite passed (2,097 library tests plus all integration and documentation tests), and structured batch `receipt_01M19Z3QSA1P9FR35YJPV55B2A` passed contract, LOC, formatting, strict Clippy, 2,377 core tests, 111 frontend tests, 445 vault tests, and 209 process tests with fresh evidence.

## Context and Orientation

`crates/jig/src/bootstrap_parts/part_01.rs` defines `InitOpts`, `ScaffoldOpts`, and the public Clap `ScaffoldPreset`. `crates/jig/src/bootstrap/presets.rs` centralizes typed capabilities, exact public spelling, backend compatibility, reserved names/roots, descriptor data, and `jig presets` order. `crates/jig/src/cli/init_wizard.rs` loads prepared answers once, resolves explicit/default/strict interaction, validates the chosen preset before package-manager preflight, and deliberately contains the separate guided flow owned by B05.

`crates/jig/src/bootstrap/answers.rs` owns `PreparedInitAnswers`, which retains one parsed `AnswerInput` and its merged effective `AnswerOpts`. `crates/jig/src/bootstrap/answers/input.rs` owns selected-preset policy over the retained raw top-level shape and merged values. `crates/jig/src/bootstrap/init.rs` consumes the prepared value and validates it before template resolution, vault capture, and destination publication. The ordinary CLI path must never reopen `answers_file`.

`crates/jig/src/bootstrap/scaffold.rs` maps public presets to `InitScaffoldPlan`; `crates/jig/src/bootstrap/scaffold/project.rs` already contains private `ScaffoldIdentity::RustCli` and `RustOnlyArtifact::Cli`; `crates/jig/src/bootstrap/scaffold/rust_only_workspace.rs` already renders shared workspace files plus `templates/scaffolds/rust-cli/crate/src/main.rs.jinja`. The scaffold-owned CLI set is exactly root `README.md`, root `Cargo.toml`, `crates/<normalized-package>/AGENTS.md`, `crates/<normalized-package>/Cargo.toml`, and `crates/<normalized-package>/src/main.rs`.

The generated member manifest must retain `publish = false`, omit license authority and dependencies, and contain an explicit `[[bin]]` named after the normalized package with path `src/main.rs`. The binary uses only `std`; with no arguments it exits zero, emits exactly one newline-terminated UTF-8 stdout line containing Cargo's package name and version, and emits empty stderr. The scaffold must omit `lib.rs`, license and environment files, migrations and SQLx metadata, database crates, `apps/`, OpenAPI, JavaScript manifests and lockfiles, frontend contract scripts, dev apps, and release workflows.

The complete input policy accepts destination/template/CI/repository naming flags, answer files, force/default/no-input/no-vault flags, full or omitted harness footprint, Rust backend compatibility, exactly `rust_crate_roots = ["crates"]`, Rust and repository-wide command overrides, false/omitted SQLx/schema/application-contract settings, harness-only vault/status/execution/agent-tooling authority, scalar `[dev]` settings with no apps, and inert `web_package_manager`. It parses but ignores legacy `jig_version`. It rejects database/frontend/dev-app authority, Go and TypeScript authority, SQLx/schema/migration authority, minimal footprint, nonstandard Rust roots, caller-authored repository/commands/work/loop models, and unknown top-level keys. Empty optional strings normalized away by the existing parser remain absent rather than conflicts. Every rejection must name `rust-cli` and the offending input after answer merging but before template resolution, vault capture, or publication.

## Plan of Work

First, append `RustCli` to the public enum and every exhaustive capability/metadata mapping in `bootstrap/presets.rs`. Give it exact spelling `rust-cli`, a Rust backend compatibility value, no reserved dev-app names or backend roots, and a descriptor that promises one binary virtual workspace, run guidance, project ownership, no database/frontend/dev/release layers, and no implied license or publication. Update Clap, preset-order, and human-summary tests while preserving the first four entries exactly.

Second, route `ScaffoldPreset::RustCli` one-to-one to `RustOnlyArtifact::Cli` in `bootstrap/scaffold.rs` and remove the private artifact's dead-code allowance. Add project-plan tests that prove the public/private identity mapping and no database/frontend state. Generalize B03's answer validator so both Rust-only presets use the same retained `AnswerInput`, accepted/rejected matrix, normalization, and error constructor parameterized by exact preset spelling. Extend both CLI interaction and bootstrap fallback preparation errors to identify either selected Rust-only preset without adding a loader.

Third, add focused tests for explicit `rust-cli` strict/no-terminal and `--defaults` success; all accepted CLI and answer-file families; every rejection family; post-merge precedence; error timing before template, vault, and publication effects; one injected answers read; retained raw shape after source mutation; and inert package-manager handling. Split new test modules below the repository's Rust-file line limit rather than copying one oversized B03 file.

Fourth, add end-to-end generation and process tests. Prove exact scaffold and absence sets, parsed Cargo manifests and explicit binary target, no dependencies/license/publication, neutral generated `workspace` component, no API/backend/dev/Go/SQLx/frontend actions, neutral root guide, exact JSON and human reports, update/recopy ownership, Cargo fmt/Clippy/test/locked-test/docs, offline setup, and exact binary stdout/stderr/status. Preserve existing and library preset output through compatibility assertions.

Finally, format and run focused tests while iterating. Review all fixture names for open-source hygiene, build the changed Jig runtime, force `scripts/jig` through `JIG_DEV_BIN=target/debug/jig`, run the task's structured checks/gates/evidence, inspect receipts and the complete diff, and audit the B04 contract requirement by requirement. Only then finish structured work, close the Bead, and flush its JSONL export.

## Concrete Steps

Work from the repository root. During implementation use focused commands such as:

    cargo fmt --all -- --check
    cargo test -p jig-sh bootstrap::presets
    cargo test -p jig-sh cli::init_wizard
    cargo test -p jig-sh rust_cli
    cargo test -p jig-sh --test cli_json rust_cli
    cargo clippy -p jig-sh --all-targets -- -D warnings

Build and dogfood the changed runtime before structured checks:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M19W9QVK6ETXH73YWHFW5873
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M19W9QVK6ETXH73YWHFW5873
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M19W9QVK6ETXH73YWHFW5873

At successful completion only:

    JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01M19W9QVK6ETXH73YWHFW5873 --resolution 'B04 explicit rust-cli init and runnable workspace acceptance complete.' --outcome success
    br close jig-sh-rust-only-init-presets-zc7.2.2 --reason 'Completed explicit rust-cli init and runnable workspace acceptance.' --json
    br sync --flush-only

No commit or push is part of this ExecPlan unless the user explicitly requests it.

## Validation and Acceptance

The public parser must accept exactly `rust-cli`, reject misspellings, and append it after `rust-library` in `ScaffoldPreset::value_variants()`, JSON presets output, and human presets output. Explicit `--no-input`, non-terminal strict mode, and explicit `--defaults` retain `rust-cli`; the guided menu remains unchanged for B05.

An init fixture must contain the exact five scaffold-owned files plus the full Jig harness. The root is a virtual Rust 2024 workspace at the repository's package Rust baseline. The member manifest has `publish = false`, no license fields or dependencies, and one exact `[[bin]]`. No forbidden file or generated dev-app authority exists. `.jig.toml` and the contract project a neutral component named `workspace` rooted at `.`, with Rust fmt, Clippy, test, locked-test, bootstrap, contract, and policy actions and no backend/API identity or Go, SQLx, frontend, dev, database, or application-contract actions. The root `AGENTS.md` contains no backend-only transport rule or `scripts/jig dev` recommendation.

The generated repository must pass offline setup or lock generation, Cargo formatting, strict all-target Clippy, workspace test, locked test, and docs with warnings denied. Running the exact generated package without arguments must return status zero, produce one newline-terminated UTF-8 line containing its normalized package name and `0.1.0`, and produce empty stderr. Tests must inspect the source/manifest to prove there is no parser/logging dependency or generated smoke-output test.

Every accepted and rejected input family in the context above needs an exact oracle for CLI and answer-file authority where applicable. Rejections must prove answer-file merging occurred and must precede template resolution, passphrase capture, and destination publication. A read-count/mutation test must prove the ordinary CLI path consumes B03's prepared answer once and cannot reopen its source. Existing preset and `rust-library` behavior must remain compatible, and update/recopy must update only the harness without claiming scaffold-owned files.

Completion requires focused tests, formatting, Clippy with warnings denied, generated Cargo/runtime checks, the relevant broad `jig-sh` suite, and every applicable structured gate with fresh evidence. A green narrow test cannot substitute for any missing task-local requirement.

## Idempotence and Recovery

Source edits, formatting, builds, and tests are repeatable. Temporary generated repositories use unmistakably generic names such as `ExampleCli` and are removed by their test directories. `.agent/state/*.jsonl` remains append-only; retrying structured checks may append receipts but must never rewrite history.

If a failure appears, keep the Bead and plan in progress, record the evidence here, and repair the shared typed boundary rather than weakening tests or adding a CLI-only parser. If generation fails after staging, the existing init transaction owns rollback; tests must not bypass it. If a product decision is absent from the complete task contract, stop and request user direction rather than inventing new authority.

## Interfaces and Dependencies

No new third-party dependency is expected. The public interface addition is `ScaffoldPreset::RustCli` with Clap spelling `rust-cli`. `ScaffoldPreset::as_str`, `generated_backend_language`, capabilities, reserved names/roots, and `descriptor` must all cover it. `InitScaffoldPlan::from_opts` maps it to `RustOnlyArtifact::Cli`, whose existing `identity()` returns `ScaffoldIdentity::RustCli` and whose existing template selection returns `rust-cli/crate/src/main.rs.jinja`.

`PreparedInitAnswers::validate_selected_preset(&ScaffoldOpts, &AnswerOpts)` remains the sole selected-preset entrypoint. Both Rust-only public variants call one `AnswerInput` validator with the selected preset spelling; the validator merges retained raw answers with effective CLI values and returns errors through one parameterized constructor. `PreparedInitAnswers::from_opts_at` and `AnswerInput::from_init_opts_at_with_reader` remain the only init-answer preparation path and test seam.

The renderer continues to use `RustOnlyScaffoldPlan { artifact: RustOnlyArtifact::Cli }`, the common `render_rust_only_workspace_files`, and the existing strict template context. Reports continue to use the exact private identity string and the compatible `db: none` plus empty `frontends` shape. No persistent schema migration, staged rollout, or compatibility adapter is required because this is an appended public enum value and internal code-only cutover.

Revision note (2026-08-30, Codex): replaced the structured-work stub with the initial self-contained B04 ExecPlan after reading repository/crate guidance, the complete task and product contracts, B03's completed plan, and the private B02 renderer foundation.

Revision note (2026-08-30, Codex): recorded the completed implementation and focused acceptance evidence, plus the generated-binary target-directory discovery and its Cargo-owned execution decision, before broad repository validation.

Revision note (2026-08-30, Codex): recorded final end-to-end acceptance, the test-file LOC split, the complete locked suite, and fresh structured receipt evidence after the B04 contract audit.
