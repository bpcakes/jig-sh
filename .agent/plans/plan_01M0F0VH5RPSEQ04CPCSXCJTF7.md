# Harden Go scaffold invariant ownership

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must stay current as implementation proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

## Purpose / Big Picture

The Go scaffold currently works on its primary path, but several edge cases expose one architectural weakness: preset identity and generated-repository invariants are decided in multiple phases. A selected preset, loaded answers, the derived scaffold plan, and rendered templates can disagree until a late generic validation step. After this work, `jig init` will reject conflicting preset inputs before side effects, every application preset will reserve its generated backend identity consistently, Go module paths will fail before vault preparation, generated bootstrap and CI behavior will reflect their declared authorities, and diagnostics will use the configured toolchain version. Each independently verifiable behavior will be committed separately.

## Progress

- [x] (2026-08-20 07:23Z) Confirmed the branch is clean and reviewed repository, crate, and ExecPlan guidance.
- [x] (2026-08-20 07:23Z) Reproduced the reported failures and completed a full baseline review and test pass.
- [x] (2026-08-20 07:23Z) Recorded this plan and opened structured Jig work as `plan_01M0F0VH5RPSEQ04CPCSXCJTF7`.
- [ ] Slice 1: make application preset identity and generated backend-name reservations early, canonical invariants; add conflict and collision tests; commit.
- [ ] Slice 2: move complete Go module validation before vault preparation; add grammar and ordering tests; commit.
- [ ] Slice 3: align generated Go bootstrap sequencing and documentation with the database preflight contract; add command tests; commit.
- [ ] Slice 4: harden generated Go workflow and OpenAPI test paths; add template and generated-runtime tests; commit.
- [ ] Slice 5: make Go doctor remediation version-aware; add diagnostic tests; commit.
- [ ] Run formatting, Clippy, full repository tests and Jig gates; inspect receipts and working tree; finish structured work.

## Surprises & Discoveries

- Observation: The individual symptoms are not independent typos. `ScaffoldOpts::validate_init_invariants`, `ScaffoldOpts::apply_init_answer_defaults`, `InitScaffoldPlan::from_opts`, and `InitScaffoldPlan::apply_answer_defaults` each own part of preset normalization, so incompatible state can survive until rendering.
  Evidence: A Go-generated answers file combined with `--preset rust-react` produced a Cargo workspace whose `.jig.toml`, bootstrap command, dev app, and CI workflow still selected Go.
- Observation: The generated backend app name is shared by Rust and Go, but its reservation check is conditional on Rust.
  Evidence: `--preset go-react --frontend api:spa` passes preset validation and later fails generic app-directory consistency validation.
- Observation: Go module syntax validation currently lives in scaffold-plan construction, after CLI vault preparation.
  Evidence: `crates/jig/src/cli/bootstrap_run.rs` prepares the vault before `bootstrap::run_init`, while `validate_go_module` is called from `InitScaffoldPlan::go_react`.

## Decision Log

- Decision: Treat preset identity as an invariant to validate, not a default to overwrite silently.
  Rationale: Answer files may intentionally carry project-owned commands, but an explicit backend identity that conflicts with an application preset cannot describe a coherent generated repository. Early rejection preserves user data and makes precedence unambiguous.
  Date/Author: 2026-08-20, Codex
- Decision: Keep harness-only backend answers legal.
  Rationale: Harness-only can describe an existing project and does not generate a backend, so it must not impose Rust or Go identity.
  Date/Author: 2026-08-20, Codex
- Decision: Derive reserved backend names from the selected application preset in one helper.
  Rationale: The collision is about generated dev-app identity, not Rust specifically. Central ownership prevents each new backend from needing a copied reservation branch.
  Date/Author: 2026-08-20, Codex
- Decision: Preserve separate commits for invariant ownership, validation timing, bootstrap lifecycle, generated CI/runtime portability, and diagnostics.
  Rationale: Each slice has a distinct user-visible contract and can be reviewed or reverted independently.
  Date/Author: 2026-08-20, Codex

## Outcomes & Retrospective

Implementation is in progress. Completion requires all five slices, a clean full-suite run, Jig gate evidence, and one commit per slice.

## Context and Orientation

`crates/jig/src/cli/bootstrap_run.rs` is the outer `jig init` command and owns interaction and vault preparation. `crates/jig/src/bootstrap/init.rs` loads answer files, validates options, derives an `InitScaffoldPlan`, and writes the destination. `crates/jig/src/bootstrap_parts/part_02.rs` implements scaffold option invariants and early answer defaults. `crates/jig/src/bootstrap/scaffold.rs` builds the canonical plan and derives generated answers. `crates/jig/src/bootstrap/scaffold/names.rs` validates generated names. Templates under `templates/project` generate the reusable Jig harness; templates under `templates/scaffolds/go-react` generate project-owned Go application code. Embedded template snapshots under `crates/jig/src/bootstrap/**/embedded_template_snapshots` are refreshed by the repository snapshot mechanism and must match source templates. `crates/jig/src/doctor_parts/part_02.rs` checks the Go toolchain declared by `.go-version`.

A preset is the selected starter application shape, such as `rust-react` or `go-react`. An answer file is user input merged with command-line answers. The scaffold plan is the validated, derived representation used to render files. A preflight is a cheap validation performed before expensive or stateful work. The dev-app identity is the stable name used by `scripts/jig dev` and its environment variables.

## Plan of Work

First, centralize application-preset identity in methods on `ScaffoldPreset` and use those methods from early invariant validation and generated answer derivation. Explicit `backend_language` values that disagree with `rust-react` or `go-react` will fail before destination writes. The same preset-owned helper will supply reserved backend dev-app names, so both application presets reject frontend names whose normalized environment prefix collides with generated backend processes. Tests will exercise command-line and answer-file sources.

Second, expose complete Go module validation at the bootstrap boundary. After interaction resolves defaults, `run_init_command` will validate the effective Go module before vault preparation. The validator will reject path elements with leading or trailing dots in addition to empty, dot, and dot-dot components. Lower-level scaffold construction will retain validation as defense in depth. Unit tests will cover malformed path elements, while CLI tests will prove invalid input does not invoke vault preparation or create a destination.

Third, make the generated Go bootstrap command follow the same lifecycle contract as Rust: fetch backend dependencies, install frontend dependencies when present, preflight database configuration when PostgreSQL is selected, then generate SQLC code and initialize the database, followed by contract generation. Update `docs/configuration.md` to describe the actual bootstrap-time Goose migration behavior. Rendering tests will assert the meaningful ordering rather than only command presence.

Fourth, add `.go-version` to both Go workflow path filters and replace source-file-path discovery in the generated OpenAPI freshness test with a repository-root-relative path based on Go test working-directory semantics. Refresh embedded snapshots. Tests will assert both workflow filters and, where environment permits, run the generated Go test with `GOFLAGS=-trimpath`.

Fifth, interpolate the parsed `.go-version` requirement into missing and incompatible Go doctor remediation. Extend doctor tests with a non-default authority so the user-visible fix cannot drift from the check result.

After every slice, run focused tests and inspect the diff before committing. At the end, build the development `jig` binary, set `JIG_DEV_BIN=target/debug/jig` for harness commands, run formatting, Clippy, the complete configured test gate, contract check, structured work gates, evidence, receipts, and status. Update this plan after each milestone and at completion.

## Concrete Steps

All commands run from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`.

For each implementation slice, run the focused Rust test target discovered beside the edited code, then:

    cargo fmt --all -- --check
    git diff --check
    git status --short
    git commit -m "<slice-specific message>"

Before harness validation, rebuild the runtime under development:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig

Run final validation:

    scripts/jig work check --plan-id plan_01M0F0VH5RPSEQ04CPCSXCJTF7
    scripts/jig check fmt
    scripts/jig check clippy
    scripts/jig check test
    scripts/jig check contract
    scripts/jig work gates --plan-id plan_01M0F0VH5RPSEQ04CPCSXCJTF7
    scripts/jig work evidence --plan-id plan_01M0F0VH5RPSEQ04CPCSXCJTF7
    scripts/jig work receipts --plan-id plan_01M0F0VH5RPSEQ04CPCSXCJTF7
    scripts/jig work status

Success means every command exits zero, generated Go tests pass with trimmed paths, all plan gates are recorded as passing, and the working tree contains only expected append-only Jig state updates before the final plan bookkeeping commit.

## Validation and Acceptance

An answer file containing `backend_language = "go"` combined with `--preset rust-react` must fail with a preset identity conflict before creating the destination. The inverse conflict must also fail. Harness-only answers remain accepted. Both application presets must reject a frontend named `api`, while non-colliding names remain accepted.

Go module paths such as `example.com/ExampleProject.` and `example.com/.ExampleProject` must fail with an actionable module-path error. CLI invalid input must fail before vault setup and destination creation. A normal module such as `github.com/acme/example-project` remains valid.

The generated PostgreSQL Go bootstrap command must install frontend dependencies before checking or changing the database, must perform the documented `DATABASE_URL` preflight before SQLC generation and database initialization, and must generate the client contract only after backend initialization. The no-database and no-frontend variants must remain minimal and valid.

The generated Go workflow must contain `.go-version` twice, once for pull requests and once for pushes. A generated Go project must pass `GOFLAGS=-trimpath go test ./...`, proving the OpenAPI freshness test no longer depends on compiler-recorded source paths.

When `.go-version` requires a version other than 1.26, both missing-runtime and incompatible-runtime doctor fixes must name that configured requirement. Existing 1.26 behavior remains unchanged.

## Idempotence and Recovery

All source edits and tests are repeatable. Template snapshot refreshes are deterministic. If a focused test fails, fix the current slice before committing; do not mix later slices into it. Each completed slice is a standalone commit, so a failed later slice can be repaired without rewriting earlier history. Jig state files are append-only and must never be truncated; rerunning work commands adds receipts safely. Generated temporary repositories must use generic fixture names and temporary directories so no private identifiers enter durable state.

## Artifacts and Notes

Baseline evidence from the review pass:

    cargo fmt --all -- --check                         # passed
    cargo clippy -p jig-sh -p jig-go -p jig-features --all-targets --locked -- -D warnings
                                                        # passed
    cargo test -p jig-sh                               # passed
    GOFLAGS=-trimpath go test ./internal/httpapi       # failed before fix: committed OpenAPI path used module import text

## Interfaces and Dependencies

No new external dependency is required. `ScaffoldPreset` will expose crate-private methods for its generated backend identity and reserved backend dev-app names. `ScaffoldOpts::validate_init_invariants` remains the public bootstrap-layer guard and will call those methods. `validate_go_module` remains the single grammar validator and will be made visible to the outer bootstrap command only as far as needed. Generated shell command construction continues using the existing `DATABASE_CONFIG_GUARD`. Doctor continues using its existing parsed `GoVersion`; only remediation formatting changes.

Plan revision note (2026-08-20): Expanded the initial structured-work body into a self-contained implementation plan after reproducing and classifying the comprehensive-review findings.
