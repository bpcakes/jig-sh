# Ship explicit rust-library init

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current in accordance with `.agent/PLANS.md`.

The structured work identifier is `plan_01M19KR0RZZ9TA2DXZ58ZJCSZ9`. The owning Beads task is `jig-sh-rust-only-init-presets-zc7.2.1` (B03). The exact Git baseline is commit `7a83495abd6317eb0aadefe9253b8710a3e1aa6a`.

## Purpose / Big Picture

Expose the already implemented private Rust-library scaffold as the complete public command `jig init <path> --preset rust-library --no-input --no-vault`. The command must create a buildable, documented virtual Cargo workspace with one non-publishable library crate and the full Jig harness, while carrying no database, frontend, API, application-contract, package-manager, or supervised-development state.

This task also closes the init answer-file time-of-check/time-of-use gap. Init must parse and merge its answer input exactly once, retain both the raw top-level shape and effective typed values through interaction, validate the selected Rust-library policy before package-manager/template/vault/destination effects, and render from that same frozen input. Existing presets, adopt, and update retain their current behavior.

## Progress

- [x] (2026-08-30 15:13Z) Verified B02 closed, inspected B03 and repository/crate/ExecPlan guidance, claimed B03, built `target/debug/jig`, opened structured work, and captured the exact Git baseline.
- [x] (2026-08-30 15:16Z) Linked the structured-work identifier from the B03 Beads record.
- [x] (2026-08-30 15:56Z) Added public Rust-library parsing, capabilities, descriptor metadata, private identity conversion, strict/default/no-terminal behavior, and renderer dispatch.
- [x] (2026-08-30 15:56Z) Added the frozen single-parse init-answer handoff and exhaustive Rust-library answer-policy validation before package-manager, template, vault, and publication effects.
- [x] (2026-08-30 15:56Z) Added exact CLI, answer-policy, generated-file, contract, report, update/recopy, build, lint, test, locked-test, and docs coverage; focused Rust-library and process tests plus all-target Clippy pass.
- [ ] Run focused and broad checks, review the complete diff, pass structured gates, audit every B03 requirement, finish structured work, and close/sync B03.

## Surprises & Discoveries

- Observation: B02 already provides `ScaffoldIdentity::RustLibrary`, a strict shared Rust-only renderer, exact output-path planning, neutral `workspace` projection, root-guide semantics, and live/embedded templates, but deliberately leaves the public `ScaffoldPreset` enum unchanged.
  Evidence: `crates/jig/src/bootstrap/scaffold/project.rs`, `scaffold/rust_only_workspace.rs`, and the B02 ExecPlan at `.agent/plans/plan_01M19BFD7SH879QW2Y1BB44NZN.md`.

- Observation: init currently calls `AnswerInput::from_opts_at` in bootstrap after the CLI interaction path has already loaded answer-derived options, so the selected-preset policy cannot prove that validated raw shape and rendered values came from one file read.
  Evidence: `crates/jig/src/bootstrap/init.rs`, `crates/jig/src/cli/init_wizard.rs`, and sections 5.4, 13, and 16.7 of `docs/rust-only-presets-plan.md`.

## Decision Log

- Decision: preserve the task's checked-in complete accept/reject matrix as one selected-preset validation boundary over retained `AnswerInputShape` plus merged `AnswerOpts`; do not scatter Rust-only exclusions through template or renderer branches.
  Rationale: every rejection must name `rust-library`, occur after merging, and precede template resolution, vault capture, package-manager checks, and publication.
  Date/Author: 2026-08-30, Codex.

- Decision: use the top-level workspace Rust baseline inherited by B02's renderer, currently Rust 1.88, rather than the Rust-React-specific 1.94 floor.
  Rationale: this preserves the user's explicit baseline decision and the committed renderer authority.
  Date/Author: 2026-08-30, user and Codex.

## Outcomes & Retrospective

Implementation is in progress. Completion requires current-state evidence for every acceptance item; a compiling enum variant or successful scaffold render alone is insufficient.

## Context and Orientation

`crates/jig/src/bootstrap_parts/part_01.rs` defines `InitOpts`, `ScaffoldOpts`, and the public Clap `ScaffoldPreset`. `crates/jig/src/bootstrap/presets.rs` centralizes typed capabilities and descriptor ordering. `crates/jig/src/cli/init_wizard.rs` resolves explicit/default/strict choices and performs package-manager preflight. `crates/jig/src/bootstrap/init.rs` prepares template and vault work, applies scaffold defaults, renders the harness, publishes scaffold files, and creates the init report.

`crates/jig/src/bootstrap/answers.rs` parses an answer file into `AnswerInput`, retaining `AnswerInputShape` and `RawAnswers`; resolution merges CLI values and produces effective `AnswerOpts`. `crates/jig/src/bootstrap/answers/raw_answers.rs` contains normalization and defaulting. B03 must introduce a crate-private prepared init value that carries this one parsed object across CLI interaction and `run_init`; adopt/update loading is outside scope.

`crates/jig/src/bootstrap/scaffold/project.rs` maps public presets to private project plans. B02 already defines the Rust-only plan and library identity. `crates/jig/src/bootstrap/scaffold.rs` applies defaults, plans exact paths, and dispatches rendering. `scaffold/rust_only_workspace.rs` renders exactly `README.md`, root `Cargo.toml`, crate `AGENTS.md`, crate `Cargo.toml`, and `src/lib.rs` for the library artifact.

The complete product and task contract is checked in at `docs/rust-only-presets-plan.md`, especially sections 13, 14, 16.7, and B03. The Beads description repeats the full accepted and rejected input families and required test oracles; this plan does not narrow them.

## Plan of Work

First, make `RustLibrary` a public Clap preset at the end of the existing order. Add one-to-one conversion to B02's private library identity, complete capability metadata, descriptor text, and scaffold dispatch. Preserve existing preset ordering and behavior. Ensure explicit strict/no-terminal and `--defaults` paths retain the requested library preset without adding the later B05 guided-menu work.

Second, refactor init preparation so the answer file is parsed once at the outer CLI/init boundary. Retain raw top-level shape and merged effective values through interaction, validate the selected preset after interaction/default resolution, and hand the identical parsed input to bootstrap rendering. The ordinary init path must have no API capable of reopening the answer path. Add an injectable read-count or mutation oracle proving frozen authority.

Third, implement the full Rust-library input policy. Accept ordinary repository/template/CI/Rust command/harness-only authority and inert web-package-manager compatibility. Reject database/frontend/dev/application-contract/Go/SQLx/schema/migration/TypeScript/custom-model/unknown-key authority, minimal footprint, and nonstandard Rust roots. Normalize empty optional strings consistently. Each error must identify `rust-library` and the offending field before template resolution, passphrase capture, or destination creation.

Fourth, add end-to-end generation and process-facing tests. Prove exact scaffold files and absence set, neutral generated `.jig.toml` and contract semantics, root guidance, exact human/JSON report, truthful next steps, no scaffold ownership during update/recopy, lock creation, Cargo fmt/Clippy/test/locked-test/docs, and compatibility of the existing presets.

Finally, format and run focused tests while iterating, review generated fixtures for open-source hygiene, build the development runtime, run all applicable structured checks and gates, inspect receipts and the complete diff, and audit B03 requirement by requirement before finishing work or closing the Bead.

## Concrete Steps

Work from the repository root. During development use focused commands such as:

    cargo fmt --all -- --check
    cargo test -p jig-sh bootstrap::presets
    cargo test -p jig-sh cli::init_wizard
    cargo test -p jig-sh bootstrap::answers
    cargo test -p jig-sh rust_library
    cargo clippy -p jig-sh --all-targets -- -D warnings

Build and dogfood the changed runtime before structured checks:

    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M19KR0RZZ9TA2DXZ58ZJCSZ9
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M19KR0RZZ9TA2DXZ58ZJCSZ9
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M19KR0RZZ9TA2DXZ58ZJCSZ9

At successful completion only:

    JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01M19KR0RZZ9TA2DXZ58ZJCSZ9 --resolution 'B03 explicit rust-library init and frozen answer handoff acceptance complete.' --outcome success
    br close jig-sh-rust-only-init-presets-zc7.2.1 --reason 'Completed explicit rust-library init and frozen answer handoff.' --json
    br sync --flush-only

No commit or push is part of this ExecPlan unless the user explicitly requests it.

## Validation and Acceptance

The public enum and Clap parser must accept exactly `rust-library` and reject misspellings. `jig presets` appends its descriptor after the three existing values. Explicit `--no-input` and explicit `--defaults` must retain the preset, while existing default/guided behavior remains unchanged until B05.

The generated scaffold-owned set is exactly five files. The root is a virtual Cargo workspace; the seed crate is a documented library with `publish = false`, no license field or file, no parser dependency, and Rust 2024 using the top-level workspace baseline. The combined repository has the full harness and lock file but no database, migrations, SQLx metadata, apps, frontends, OpenAPI, JavaScript files, environment sample, release workflow, or dev app.

Generated `.jig.toml` and contract records must express a neutral root `workspace` Rust component with ordinary fmt, Clippy, test, locked-test, bootstrap, contract, and policy actions. Root guidance must use neutral Rust-workspace language and omit backend transport and `scripts/jig dev`. Human and JSON reports must say `rust-library`, `db = none`, empty frontends, and list exact scaffold files. Update and recopy may update the harness but must not claim scaffold ownership.

Every accepted/rejected family in the Beads contract needs an exact test oracle. Rejection tests must additionally prove no template resolution, vault passphrase capture, or destination publication. Single-read tests must prove one parse and stable rendering if the source answer file changes after interaction. Broad compatibility checks must cover all pre-existing presets.

Completion requires focused tests, formatting, Clippy with warnings denied, generated Cargo fmt/Clippy/test/locked-test/docs execution, the relevant full `jig-sh` suite, and every applicable structured gate with fresh evidence.

## Idempotence and Recovery

Source edits, formatting, builds, and tests are repeatable. `.agent/state/*.jsonl` remains append-only. Structured checks may append receipts after fixes. Test destinations must use temporary generic fixtures and leave no external mutation.

If a validation or compatibility failure appears, retain the Bead and structured plan in progress, document the exact evidence here, and fix the shared typed boundary rather than bypassing policy in tests. Do not close B03 until every task-local acceptance criterion has authoritative evidence. If a product decision is genuinely absent from the checked-in task/design, stop and request user input before making that decision.

## Interfaces and Dependencies

No new dependency is expected. The public addition is `ScaffoldPreset::RustLibrary` with the exact Clap spelling `rust-library`, mapped to `ScaffoldIdentity::RustLibrary` and `RustOnlyArtifact::Library`.

The prepared-answer boundary must retain one `AnswerInput` containing raw top-level shape and merged values. CLI interaction may update the merged typed options, but rendering and validation must consume the same parsed file state and must not reopen `answers_file`. Rust-library policy should be expressible as a selected-preset validator used after resolution and before external or mutating preflights so B04 can add its sibling policy without duplicating the handoff.

Revision note (2026-08-30, Codex): replaced the structured-work stub with the initial self-contained B03 ExecPlan after reading repository/crate guidance, the complete Beads contract, the checked-in product design, and B02's committed foundation.
