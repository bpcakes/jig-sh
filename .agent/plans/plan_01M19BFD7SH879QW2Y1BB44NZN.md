# Add the shared Rust-only workspace renderer and templates

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while implementation proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

The structured work identifier is `plan_01M19BFD7SH879QW2Y1BB44NZN`. The owning Beads task is `jig-sh-rust-only-init-presets-zc7.1.2` (B02). The exact Git baseline is commit `8a9fbe8534ce413775e395ded2369c96da6b66cb`.

## Purpose / Big Picture

Bootstrap needs an internal, reusable way to generate a small Rust library workspace or Rust command-line workspace without pretending either project has an HTTP backend, database, frontend, DNS label, JavaScript package manager, or supervised development application. After this change, internal tests can construct either artifact and observe the exact five scaffold-owned files: a root virtual Cargo manifest, root README, seed crate manifest, seed crate guide, and artifact-specific Rust source. The public `jig init --preset` values remain exactly `rust-react`, `go-react`, and `harness-only`; B03 and B04 will expose the new workflows only after their complete CLI validation paths exist.

The same internal construction also exercises the initial harness render. It authors a neutral `workspace` component at repository root with the existing Rust adapter and actions, and root `AGENTS.md` derives Rust-workspace wording from those authored component semantics. Reloading `.jig.toml` during update or recopy must recover the same wording without retaining a preset or artifact-kind field.

## Progress

- [x] (2026-08-30 12:49Z) Verified B01 is closed, claimed B02, built the development `jig` binary from the exact baseline, opened structured work, and linked its identifier from a Beads comment.
- [x] (2026-08-30 12:49Z) Read the repository, crate, ExecPlan, B02, product-design, scaffold, renderer, template, repository-model, and answer-flow guidance before production edits.
- [x] (2026-08-30 13:33Z) Added the private scaffold identity and internal Rust-only project/artifact plan with exact naming, answer defaults, summaries, reports, and no public preset reachability.
- [x] (2026-08-30 13:33Z) Added the shared Rust-only renderer, live templates, embedded snapshots, strict context, exact paths, and internal generation tests for library and CLI artifacts.
- [x] (2026-08-30 13:33Z) Added the non-serialized neutral projection hint, authored `workspace` Rust component, semantic root-guidance predicate, and update/recopy stability tests.
- [x] (2026-08-30 13:33Z) Ran focused template/scaffold/repository tests, formatting, Clippy, the full `jig-sh` tests, and the repository-level `scripts/jig check test` contract.
- [x] (2026-08-30 13:44Z) Passed all eight structured gates with fresh evidence after resolving the LOC finding; one process-test gate was explicitly not applicable by path policy.
- [x] (2026-08-30 13:46Z) Audited every B02 requirement against current source, rendered output, snapshots, compatibility tests, and structured receipts, finished structured work successfully, closed B02, and synchronized Beads state. No product question or user decision remains open.

## Surprises & Discoveries

- Observation: `scripts/jig work start` creates the task-local plan file itself, so this document replaces its one-line body rather than adding a second plan.
  Evidence: the command returned `plan_01M19BFD7SH879QW2Y1BB44NZN` and created `.agent/plans/plan_01M19BFD7SH879QW2Y1BB44NZN.md`.

- Observation: B01 already moved every web-only value into `ReactScaffoldPlan`, leaving `InitScaffoldPlan` with the common repository/package/module/branch/CI names needed by B02.
  Evidence: baseline `crates/jig/src/bootstrap/scaffold.rs` has `ScaffoldProjectPlan::{RustReact, GoReact}` and nests package manager, DNS label, frontends, and notices below `ReactScaffoldPlan`.

- Observation: initial rendering resolves `AnswerOpts` through `RawAnswers` into `RenderAnswers`, while update/recopy loads a complete authored repository from `.jig.toml`.
  Evidence: `crates/jig/src/bootstrap/init.rs` applies scaffold defaults before `BootstrapCopyRequest`; `crates/jig/src/bootstrap/answers.rs` resolves the copy; `RepositoryRenderModel::from_answers` prefers a complete authored model. Therefore the initial projection hint can be skipped during serialization, while later guidance must be derived from ordinary authored component and adapter records.

- Observation: the source workspace and existing Rust React scaffold intentionally have different Rust floors: root `Cargo.toml` declares 1.88, while Rust React templates declare 1.94.
  Evidence: `Cargo.toml` has `workspace.package.rust-version = "1.88"`; `templates/scaffolds/rust-react/workspace/Cargo.toml.jinja` has `rust-version = "1.94"`. The user explicitly selected the top-level workspace baseline for the new Rust-only templates.

- Observation: authored neutral workspace components are rooted at `.`, but root guidance must retain the repository's crate-container convention instead of presenting `.` as a crate root.
  Evidence: the first update/recopy test rendered `Treat \`.\` as Rust crate roots`; preserving the already authored `rust_crate_roots = ["crates"]` (with a `crates` fallback) makes initial and recopy guidance identical without persisting preset identity.

- Observation: a final inline MiniJinja conditional can consume an otherwise expected trailing newline even when the environment retains trailing newlines.
  Evidence: the strengthened exact-output test identified only `README.md` as unterminated. Ending its final sentence with unconditional text restored newline termination in both live and snapshot-only rendering.

- Observation: three review passes found and resolved two local issues and no open product question: an unreachable Rust-only panic arm in React dev-app construction, and Clippy's requested let-chain simplification of the replacement.
  Evidence: dev-app construction now returns `None` for Rust-only plans; `cargo clippy -p jig-sh --all-targets -- -D warnings` passes after the third pass.

- Observation: the first structured check passed every functional gate but rejected the enlarged `scaffold.rs` at 965 lines against the repository's 800-line hard limit.
  Evidence: `jig.rust_file_loc` named only `crates/jig/src/bootstrap/scaffold.rs`. A behavior-preserving source reorganization moved project-plan types, implementations, and their focused compatibility test to `scaffold/project.rs`; the files are now 732 and 245 lines, respectively, and the focused LOC check reports `ok: true`.

## Decision Log

- Decision: implement the internal shape as `ScaffoldIdentity::{RustReact, GoReact, RustLibrary, RustCli}`, `ScaffoldProjectPlan::RustOnly`, and `RustOnlyArtifact::{Library, Cli}`.
  Rationale: this is exhaustive for reports and rendering, keeps internal identity separate from public Clap values, and matches the checked-in design without creating a stringly typed artifact switch.
  Date/Author: 2026-08-30, Codex.

- Decision: keep one shared Rust-only template list for root Cargo, README, crate Cargo, and crate guide, and select exactly one source template from the artifact enum.
  Rationale: the two outputs intentionally share all workspace policy; only the package target and source behavior differ. Artifact conditionals are acceptable only where the generated artifact actually differs.
  Date/Author: 2026-08-30, Codex.

- Decision: carry a private neutral-projection enum through skipped `AnswerOpts`, skipped raw-answer merge state, and skipped `RenderAnswers`, then author ordinary repository records. Derive guidance from the resulting model, never from the hint after serialization.
  Rationale: initial render needs an explicit consuming feature input, while update/recopy must remain stable using `.jig.toml` as authority and must not persist historical preset identity or expand the contract schema.
  Date/Author: 2026-08-30, Codex.

- Decision: obtain the Rust-only MSRV from Cargo's `CARGO_PKG_RUST_VERSION` build value, which inherits top-level `workspace.package.rust-version` and is currently 1.88; do not copy Rust React's independent 1.94 floor.
  Rationale: the user explicitly chose the top-level baseline. Using Cargo's package metadata keeps one current authority and automatically follows an intentional future workspace-baseline update.
  Date/Author: 2026-08-30, user and Codex.

## Outcomes & Retrospective

Implementation and acceptance validation are complete. The result adds no partial public preset.

Seven focused renderer tests pass from both live and snapshot-only embedded sources; 22 repository-model tests, guidance recopy tests, preset tests, all 33 wizard tests, existing Rust React end-to-end generation, formatting, and Clippy with warnings denied pass. The complete `cargo test -p jig-sh` run passed 2,065 unit tests plus all integration suites, with only its two existing ignored stress/network tests. `scripts/jig check test` additionally passed all 3,105 nextest cases with two skipped. The initial structured check exposed only the source-file LOC limit; the project-plan split resolves it. The final structured check passed all eight gates with fresh evidence and no unresolved gate. No open question requires user input.

Structured work `plan_01M19BFD7SH879QW2Y1BB44NZN` finished successfully, and Beads task `jig-sh-rust-only-init-presets-zc7.1.2` is closed. B03 is unblocked to add the first complete public `rust-library` init path on top of this private foundation.

## Context and Orientation

`crates/jig/src/bootstrap/scaffold.rs` converts scaffold choices and answers into `InitScaffoldPlan`, which is an in-memory file plan and is never serialized. B01 introduced `ScaffoldProjectPlan::{RustReact, GoReact}` and put React-only state under each existing project. B02 adds the first non-React branch for tests and future public conversions, but does not edit the public `ScaffoldPreset` enum in `crates/jig/src/bootstrap_parts/part_01.rs`.

`crates/jig/src/bootstrap/scaffold/write.rs` classifies file writes and creates the scaffold report. Its preset spelling currently matches the public enum directly; B02 must instead use the private scaffold identity so internal Rust-only plans can report `rust-library` and `rust-cli` without becoming public parser values.

`crates/jig/src/bootstrap/scaffold/templates.rs` embeds live files below `templates/scaffolds/` through MiniJinja with strict undefined-variable behavior. `crates/jig/src/bootstrap/scaffold/embedded_templates.rs` compares live sources to checked-in snapshots under `crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/`; the generated manifest is `embedded_templates_snapshot.rs`. Refreshing with `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh` updates both authorities.

`crates/jig/src/bootstrap/init.rs` applies `InitScaffoldPlan::apply_answer_defaults` before rendering the harness. `AnswerOpts` in `crates/jig/src/bootstrap/opts.rs` is merged through `RawAnswers` and resolved into `RenderAnswers` in `crates/jig/src/bootstrap/answers.rs`. `crates/jig/src/bootstrap/repository_model.rs` converts those answers into existing component/action/profile records. The current compatibility projection always calls its root Rust or Go component `api` and describes it as an application backend. B02 must preserve that branch while adding an initial-only neutral hint that selects component `workspace` at root with the registered `rust` adapter, the same Rust checks, repository policy actions, and Rust file-LOC action.

`crates/jig/src/bootstrap/renderer.rs` builds the strict project-template context. It already constructs `RepositoryRenderModel`, serializes the authored repository into `.jig.toml`, and computes Rust CI inputs. It must add a boolean for neutral Rust-workspace guidance derived from the authored model. `templates/project/AGENTS.md.jinja` uses that boolean to change only the managed root terminology and command list. For complete authored models loaded by update or recopy, the predicate must recognize `workspace` at `.`, the Rust adapter, and the absence of an `api`/backend identity; it must not inspect a preset or artifact string.

The exact scaffold-owned library files are `README.md`, `Cargo.toml`, `crates/<package>/AGENTS.md`, `crates/<package>/Cargo.toml`, and `crates/<package>/src/lib.rs`. CLI output replaces only the last path with `src/main.rs`. Root Cargo is a virtual workspace with resolver 3, one explicit member, edition 2024, version 0.1.0, and the top-level Jig workspace Rust baseline, currently 1.88. Both seed packages inherit those values, set `publish = false`, and contain no license field. The CLI manifest adds one explicit `[[bin]]`; the library manifest does not invent a public API. No license, environment, database, app, OpenAPI, frontend, package-manager, lock, or dev file is generated.

## Plan of Work

First, extend `crates/jig/src/bootstrap/scaffold.rs` with the private identities and Rust-only artifact plan. Add a test-only constructor because there is deliberately no public preset conversion in B02. Reuse `normalize_rust_react_package_name` so Cargo identifiers and the established artifact-path budget remain identical. Set common Rust tooling answers, `rust_crate_roots = ["crates"]`, SQLx disabled, schema dump disabled, no frontend contracts, no frontend or dev apps, and an explicit Cargo bootstrap command without synthesizing web values. Make summaries, database, backend-language, rendering, output paths, and reports dispatch exhaustively through the private identity and project enum.

Second, add `crates/jig/src/bootstrap/scaffold/rust_only_workspace.rs` and the live source trees below `templates/scaffolds/rust-only`, `templates/scaffolds/rust-library`, and `templates/scaffolds/rust-cli`. The renderer builds a strict context containing only consumed repository/package/artifact/Rust-version values, validates every template path, substitutes the normalized package in every output path, and returns the common files plus exactly one source. Tests render both artifacts, compare paths to `output_paths`, parse TOML, resolve the member manifest, check publication/license neutrality, run rustfmt over generated source, prove normalization and maximum-boundary acceptance, and exercise precise missing-template and missing-context errors.

Third, add the skipped projection hint to the answer flow and teach `RepositoryRenderModel` to add either its existing backend component or a neutral Rust `workspace` component. Reuse the existing Rust adapter and command values rather than adding adapter, runner, action, schema, or epoch types. Add model tests that compare the exact neutral component/actions/profile and prove existing Rust React bytes/model behavior remain unchanged. Add a semantic predicate on `RepositoryRenderModel` and expose it in the project template context. Condition only the root guide sections required by B02, refresh the base template snapshot, and test initial plus authored reload/update-style rendering.

Finally refresh scaffold snapshots, run targeted and broad checks, inspect all generated snapshot changes for generic fixture hygiene and absolute-path leaks, build the development binary, run path-aware structured gates, and audit the diff requirement by requirement before finishing work or closing B02.

## Concrete Steps

Work from the repository root.

Use focused development checks such as:

    cargo fmt --all -- --check
    cargo test -p jig-sh bootstrap::scaffold
    cargo test -p jig-sh bootstrap::repository_model
    cargo test -p jig-sh bootstrap::tests::basic::rendering
    JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
    JIG_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo test -p jig-sh bootstrap::scaffold

Run broad repository validation with:

    cargo test -p jig-sh
    cargo clippy -p jig-sh --all-targets -- -D warnings
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M19BFD7SH879QW2Y1BB44NZN
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M19BFD7SH879QW2Y1BB44NZN
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M19BFD7SH879QW2Y1BB44NZN

At successful completion only:

    JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01M19BFD7SH879QW2Y1BB44NZN --resolution 'B02 shared Rust-only renderer, neutral projection, templates, snapshots, and compatibility acceptance complete.' --outcome success
    br close jig-sh-rust-only-init-presets-zc7.1.2 --reason 'Completed B02 shared Rust-only renderer and templates.' --json
    br sync --flush-only

No commit or push is part of this ExecPlan unless the user explicitly requests it.

## Validation and Acceptance

Internal library and CLI constructors must each render the exact five-file set and report the exact private identity string. `output_paths()` must equal the rendered relative paths exactly. Both root and package manifests must parse as TOML, the explicit workspace member must exist, the package must set `publish = false`, neither manifest may contain `license` or `license-file`, and no license file may be planned. The library source must be documented, API-free, and rustfmt-stable; the CLI source must be rustfmt-stable and print its Cargo package name and version with no parser dependency.

Plans must prove they have no React context, database, package manager, DNS label, frontend apps, development apps, or application-contract state. The normalization tests must cover a mixed/invalid-input name that normalizes successfully and the established maximum Cargo path-budget boundary. Strict MiniJinja rendering must report a missing embedded template by its path and an undefined context value with the owning template in the error chain. Live and embedded snapshot lists and bytes must match, and snapshot-only compilation must still render both artifact kinds.

The initial neutral projection must contain ordinary `repo` and `workspace` components, with `workspace` rooted at `.` and using only the `rust` adapter. Its description and tags must not call it an API/backend or encode library/CLI artifact identity. Its actions must preserve Rust fmt, Clippy, test, locked-test compatibility aliases and the repository Rust file-LOC policy, with deterministic component/action/profile ordering. It must contain no `api` component, SQLx, Go, TypeScript, frontend, database, or dev authority. Existing compatibility answers and Rust React projections must remain unchanged.

The managed root guide for a semantic neutral Rust workspace must use `## Rust Defaults`, repository/crate wording, `For Rust changes`, and `## Crate Guide Conventions`; it must omit the transport rule and `scripts/jig dev`. Rendering again from the authored `.jig.toml` model must produce identical managed guidance. Existing Rust React, Go React, harness-only, and compatibility guide bytes must remain unchanged.

Public compatibility is exact: `ScaffoldPreset::value_variants()`, Clap possible values, numeric/text wizard choices, public descriptors, and `jig presets` remain unchanged. No public parser, descriptor, serialized preset/artifact field, persisted answer, contract field, adapter kind, runner kind, or epoch is added.

Completion requires focused tests, snapshot parity in live and snapshot-only modes, formatting, Clippy with warnings denied, the full relevant `jig-sh` test suite, and every applicable structured gate to pass. Generated fixtures and snapshots must use only generic names and contain no local absolute path or downstream identifier.

## Idempotence and Recovery

Source edits, formatting, builds, tests, and snapshot refresh are safe to repeat. Snapshot refresh is mechanical but must be reviewed; if a refresh changes unrelated files, stop and investigate the build input rather than accepting drift. `.agent/state/*.jsonl` is append-only and must never be rewritten or truncated. Structured checks may append new receipts after fixes.

If tests expose compatibility drift, preserve the old backend-projection and root-guide branch exactly and narrow the neutral branch by authored semantics. If a renderer test fails at the Cargo path boundary, reuse the existing normalization/path-budget helper instead of weakening the limit. If validation cannot complete, leave the Bead and structured plan in progress, update this document with the exact remaining failure, and do not close or claim acceptance.

## Artifacts and Notes

The baseline development binary built successfully before production edits. The live Beads record is in progress and includes comment `Structured work: plan_01M19BFD7SH879QW2Y1BB44NZN`.

The checked-in product design is `docs/rust-only-presets-plan.md`, especially sections 9 through 12 and 16 through 20. B02 intentionally implements only the private renderer/projection foundation; B03 and B04 own public preset parsing and end-to-end init exposure.

## Interfaces and Dependencies

No new crate dependency is allowed. The new scaffold types stay beneath `crate::bootstrap::scaffold` and are not serialized or exported from the crate.

The private plan boundary must be equivalent to:

    enum ScaffoldIdentity {
        RustReact,
        GoReact,
        RustLibrary,
        RustCli,
    }

    enum ScaffoldProjectPlan {
        RustReact(RustReactScaffoldPlan),
        GoReact(GoReactScaffoldPlan),
        RustOnly(RustOnlyScaffoldPlan),
    }

    struct RustOnlyScaffoldPlan {
        artifact: RustOnlyArtifact,
    }

    enum RustOnlyArtifact {
        Library,
        Cli,
    }

`ScaffoldIdentity::as_str()` returns the exact report value. Public `ScaffoldPreset` conversion remains exhaustive for its three current variants and cannot produce internal Rust-only identities yet.

The skipped render hint may be named `RepositoryProjectionHint` and needs only a neutral Rust-workspace value plus the existing default behavior. It is copied from `InitScaffoldPlan::apply_answer_defaults` through `AnswerOpts`, raw merging, and `RenderAnswers`; all serialization paths must omit it. `RepositoryRenderModel::from_answers` uses it only when no complete authored repository is already authoritative.

The renderer module exposes only methods on `InitScaffoldPlan` for `render_rust_only_workspace_files` and `rust_only_workspace_relative_paths`, plus test access needed for strict failure oracles. The template context uses MiniJinja's existing strict syntax and the established embedded-template source; it does not create another template engine or runtime lookup path.

Revision note (2026-08-30, Codex): replaced the structured-work stub with the initial self-contained B02 ExecPlan after inspecting the exact baseline, checked-in product design, answer/render pipeline, repository projection, templates, report path, and compatibility boundaries.

Revision note (2026-08-30, Codex): recorded the user's decision that Rust-only workspaces inherit the top-level Jig workspace Rust baseline, currently 1.88, rather than the Rust React scaffold's separate 1.94 floor.

Revision note (2026-08-30, Codex): recorded completed implementation, review findings, focused/live/snapshot/full-suite evidence, and the remaining structured-gate work.

Revision note (2026-08-30, Codex): recorded the structured LOC-gate finding and behavior-preserving project-plan module split required to satisfy it.

Revision note (2026-08-30, Codex): recorded successful structured-work completion and Beads closure.
