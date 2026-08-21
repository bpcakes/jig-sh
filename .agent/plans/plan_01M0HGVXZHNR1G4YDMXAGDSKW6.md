# Harden Go backend review findings

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while implementation proceeds. Maintain this file in accordance with `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

The Go backend branch introduced useful behavior across the generated repository contract, Doctor, scaffold naming, feature registry, and frontend contract checks. A comprehensive review found five places where those boundaries did not line up. After this work, repositories that use the new backend selectors will be rendered under a new contract epoch; adopted Go repositories will use their standard `go.mod` as the single runtime authority; Rust-only naming rules will not rewrite valid Go or frontend package names; unavailable Go checks will explain the missing capability; and generated CI will prove both halves of OpenAPI/client drift.

The findings are not one accidental typo. The contract-version bug and Go-version bug are structural ownership problems: data was added without advancing the compatibility boundary, and a scaffold-only convenience file was treated as a required repository authority. The naming bug is a smaller abstraction leak caused by reusing a Rust-specific normalizer. The check diagnostics and CI documentation are omissions around capability modeling and verification ownership. The implementation therefore fixes the boundaries instead of adding special cases at individual call sites.

Each independently useful change is committed separately. The complete repository test suite and all configured Jig gates must pass after the final slice.

## Progress

- [x] (2026-08-21) Reproduced the review findings against the clean `feat/go-backend-support` branch, read the repository and crate guides, and opened structured work plan `plan_01M0HGVXZHNR1G4YDMXAGDSKW6`.
- [x] (2026-08-21) Classified the findings and selected long-term boundaries: contract v5, `go.mod` runtime authority, backend-neutral package normalization, explicit Go capabilities, and split backend/client drift proofs.
- [x] (2026-08-21) Advanced the generated-harness epoch to contract v5, documented strict configuration additions as epoch-breaking, rejected mixed Go/SQLx identity at answer and load boundaries, and passed focused compatibility regressions.
- [x] (2026-08-21) Made bounded root `go.mod` parsing the single Go toolchain authority, honored a newer `toolchain` directive, pointed Go and browser CI at `go.mod`, retired generated `.go-version`, refreshed snapshots, and passed focused Doctor/scaffold tests.
- [x] (2026-08-21) Extracted backend-neutral package normalization, confined Rust keyword/identifier prefixing to Rust-react, reused the neutral stem for default Go modules and frontend packages, and proved `loop` remains unchanged in a generated Go workspace.
- [x] (2026-08-21) Added defaulted Go capability queries to the feature boundary and backend/database/stale-manifest diagnostics for lint and sqlc, with feature-unit and end-to-end dispatch coverage.
- [x] (2026-08-21) Added client-only staged regeneration from committed OpenAPI documents, ran it after each generated web build, corrected the workflow-boundary documentation, and proved it neither invokes Go nor mutates the generated repository.
- [x] (2026-08-21) Isolated interactive vault PTY tests from CPU-heavy vault crypto tests after two full-suite runs exposed the same load-dependent failure; the exact 438-test vault partition then passed 438/438.
- [ ] Rebuild the development runtime, pass focused tests after each slice, pass the complete repository gates, update outcomes, and close structured work.

## Surprises & Discoveries

- Observation: `.jig.toml` is deserialized with `#[serde(deny_unknown_fields)]`, while the branch added `backend_language`, `go_database`, and `migration_dir` without changing `CURRENT_CONTRACT_VERSION` from 4.
  Evidence: `crates/jig/src/context.rs` defines both the strict `RepoConfig` and `CURRENT_CONTRACT_VERSION`; `docs/public-contract.md` says a breaking configuration addition requires a contract bump.

- Observation: the strict-reader problem affects every newly rendered repository, not only repositories whose selected backend is Go, because the project template emits the new selector keys for Rust configurations too.
  Evidence: `templates/project/.jig.toml.jinja` emits the selectors unconditionally.

- Observation: generated Go workspaces happen to contain `.go-version`, but adopted Go repositories are identified from `go.mod` and are not given that scaffold file. Generated GitHub Actions and Doctor nevertheless require `.go-version`.
  Evidence: `.go-version` is in the Go scaffold inventory, while `templates/project/.github/workflows/go-tests.yml.jinja` and `go_runtime_check` use it unconditionally. The official `actions/setup-go` input supports `go.mod`, including its `toolchain` directive.

- Observation: `sanitize_package_name` contains Rust keyword and identifier checks but is also used for Go repository names and JavaScript frontend package names.
  Evidence: `crates/jig/src/bootstrap/scaffold.rs` and `crates/jig/src/bootstrap/scaffold/frontend_parts/part_01.rs` call the helper outside the Rust package boundary.

- Observation: the local aggregate TypeScript typecheck already performs an end-to-end contract check, but the generated web workflow deliberately calls only package-local scripts.
  Evidence: `templates/project/scripts/check-webapps.sh.jinja` runs `scripts/contracts.mjs check` in aggregate `typecheck` mode, while `webapp-checks.yml.jinja` invokes `run-script "$APP_DIR" typecheck`.

- Observation: the first broad library pass after the epoch bump passed 1,567 tests and failed only assertions whose fixtures combined the newly rendered launcher with a hard-coded v4 manifest or replacement token.
  Evidence: legacy v4 parser and context tests passed unchanged; rerunning the six affected current-launcher tests and the current-render adoption assertion after making their epoch source explicit passed.

- Observation: `actions/setup-go` and the Go toolchain already share the needed authority semantics: setup-go reads `go.mod` and selects its `toolchain` directive when present, otherwise its `go` directive.
  Evidence: official `actions/setup-go` advanced-usage documentation and the Go Modules Reference describe the same precedence; the local parser tests cover newer, default, duplicate, malformed, bounded, and symlinked authorities.

- Observation: the same vault PTY browser test failed twice in the four-thread full vault group after about five seconds, while all other tests passed and the PTY test passed in isolation in 8.1 seconds.
  Evidence: full-suite Nextest runs `1a8b5c78-1069-45d6-962a-1b9e11ab7ed6` and `f37c4675-cda0-4dfe-bec8-6a94723da698` both failed only `browser_unlocks_resizes_locks_and_restores_the_terminal_on_quit`; the exact isolated Cargo test passed unchanged.

## Decision Log

- Decision: Make contract v5 the first epoch that generated repositories may depend on the backend selector fields, while continuing to load v2-v4 repositories.
  Rationale: Relaxing `deny_unknown_fields` would hide typos and would not repair already released v4 readers. A new epoch makes old runtimes reject the manifest before depending on a schema they cannot parse, preserves strict configuration, and states the compatibility boundary truthfully.
  Date/Author: 2026-08-21 / Codex

- Decision: Treat Rust and Go as exclusive backend identities and reject `backend_language = "go"` together with `sqlx_enabled = true`.
  Rationale: The renderer, migration owner, and feature registry all select one backend. Allowing a mixed state creates ambiguous migration ownership and command requirements that no generated preset supports. Rejecting it at answer finalization and repository load reduces unreachable combinations.
  Date/Author: 2026-08-21 / Codex

- Decision: Make root `go.mod` authoritative and retire generated `.go-version` ownership.
  Rationale: `go.mod` exists in both generated and adopted Go modules, is understood by the Go toolchain and `actions/setup-go`, and can express both the required `go` version and an optional newer suggested `toolchain`. Keeping two version files would require permanent drift detection and recovery policy.
  Date/Author: 2026-08-21 / Codex

- Decision: Split lexical package normalization from Rust crate-identifier validation.
  Rationale: Lowercasing and dash normalization are shared output rules, while Rust keywords and leading-character restrictions belong only to Cargo package generation. Naming helpers should encode the consumer whose constraints they enforce.
  Date/Author: 2026-08-21 / Codex

- Decision: Extend `FeatureContext` with defaulted Go capability queries and let the Go feature own its unavailable diagnostics.
  Rationale: Inferring backend identity from a missing command repeats the original abstraction problem. Explicit capability queries let registry behavior describe why a tool is absent while default methods preserve existing implementors.
  Date/Author: 2026-08-21 / Codex

- Decision: Divide contract drift verification by artifact boundary rather than installing every backend toolchain in every frontend matrix job.
  Rationale: Backend tests already regenerate and compare committed OpenAPI documents. A new client-only mode can regenerate TypeScript clients from those committed documents in web CI. Together the workflows prove backend-to-document and document-to-client drift without duplicating Go or Cargo setup in frontend jobs.
  Date/Author: 2026-08-21 / Codex

- Decision: Give each vault PTY integration test all four slots in the existing vault Nextest group.
  Rationale: The browser test intentionally uses bounded UI-event deadlines and also performs password hashing. Competing crypto tests make those deadlines measure scheduler contention. Group-wide reservation preserves useful production-facing deadlines, serializes only the two interactive PTY cases, and avoids globally slowing the vault suite.
  Date/Author: 2026-08-21 / Codex

## Outcomes & Retrospective

Implementation is in progress. Record the final commit IDs, test receipts, remaining risks, and whether each classified root cause was removed here before closing the plan.

## Context and Orientation

`crates/jig/src/context.rs` owns strict repository configuration and the current/supported contract epoch. `crates/jig/src/context/runtime.rs` checks the supported version range. Project templates receive the current epoch through the bootstrap renderer and write `.jig.toml`, `.agent/jig-contract.json`, and launchers as one coordinated harness.

`crates/jig/src/doctor_parts/part_02.rs` owns language runtime checks. Its Go check currently reads `.go-version` with the generic numeric authority reader. The replacement must safely inspect a bounded real regular `go.mod`, parse one `go` directive and an optional `toolchain goX.Y.Z` directive, select the toolchain directive when it is newer, and keep the existing actual-version probe and remediation behavior.

`crates/jig/src/bootstrap/scaffold/names.rs` owns normalized repository/package names. Rust-react generation needs a valid Cargo identifier after dash-to-underscore conversion and a filesystem-derived length bound. Go-react and frontend package generation need only the common lowercase ASCII/dash stem.

`crates/jig-contract/src/lib.rs` defines `FeatureContext`; `crates/jig-go/src/lib.rs` maps Go command keys to tools; `crates/jig/src/runtime/tool_execution.rs` asks the feature registry for a specific explanation before falling back to the generic undeclared-tool message.

`templates/scaffolds/rust-react/frontend/workspace/contracts.mjs.jinja` transactionally regenerates OpenAPI documents and TypeScript clients. `templates/project/.github/workflows/webapp-checks.yml.jinja` runs package-local lint, typecheck, build, and coverage in a matrix. Embedded template mirrors under `crates/jig/src/bootstrap/embedded_template_snapshots` are generated snapshots and must be refreshed through the repository mechanism, never edited by hand.

## Plan of Work

First, advance `CURRENT_CONTRACT_VERSION` to 5, update compatibility documentation and current-version assertions, and add regression coverage proving v4 remains readable while v6 is rejected. Add answer/config validation for the unsupported Go-plus-SQLx state. Run focused context, answer, bootstrap, launcher, and runtime tests and commit the contract slice.

Second, replace `.go-version` authority with a bounded `go.mod` parser in Doctor. Use the `toolchain` directive when present and otherwise the `go` directive. Point generated Go Actions at `go.mod`, remove `.go-version` from the scaffold inventory, refresh snapshots, and test generated and adopted repositories. Commit the authority slice.

Third, rename the shared lexical helper, add a Rust-only adapter around it, update Go/frontend callers to the neutral helper, and add tests proving a Rust keyword such as `loop` remains `loop` for Go while Rust still becomes `app-loop`. Commit the naming slice.

Fourth, add defaulted `go_backend_enabled` and `go_postgres_enabled` methods to `FeatureContext`, implement them from `RepoContext`, and make `jig-go` explain missing `jig.lint` and `jig.sqlc_check` according to backend/database capability. Add unit and runtime-dispatch tests and commit the diagnostics slice.

Fifth, add a `client-check` mode to `contracts.mjs` that regenerates only clients from committed OpenAPI input, run it after package-local typecheck in generated web CI, and revise the README to describe the two-workflow proof accurately. Refresh snapshots and render/tests, then commit the CI slice.

Finally, rebuild `target/debug/jig`, export `JIG_DEV_BIN=target/debug/jig`, run the complete `scripts/jig check test` suite plus format, Clippy, contract, and structured work gates, inspect the final commit range and worktree, update this plan, and close the work.

## Concrete Steps

Run all commands from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`.

Refresh embedded templates whenever live templates change:

    JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh

For every slice, run its focused package/tests plus:

    cargo fmt --all -- --check
    git diff --check
    git status --short

Build and validate the final runtime with:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig check test
    scripts/jig check fmt
    scripts/jig check clippy
    scripts/jig check contract
    scripts/jig work check --plan-id plan_01M0HGVXZHNR1G4YDMXAGDSKW6
    scripts/jig work gates --plan-id plan_01M0HGVXZHNR1G4YDMXAGDSKW6
    scripts/jig work evidence --plan-id plan_01M0HGVXZHNR1G4YDMXAGDSKW6
    scripts/jig work receipts --plan-id plan_01M0HGVXZHNR1G4YDMXAGDSKW6

## Validation and Acceptance

A current rendered Rust or Go repository must declare contract 5. A minimal legacy contract-4 Rust fixture must still load in the current runtime, and a manifest newer than 5 must fail. A Go/SQLx mixed answer or loaded config must fail with a direct invariant message before command selection.

Doctor must accept an adopted Go module with only `go.mod`, report that path as its authority, respect a newer `toolchain` directive, and fail closed for missing, malformed, oversized, symlinked, or duplicate version directives. Generated Go CI must select its toolchain from `go.mod`, and fresh Go scaffolds must no longer own `.go-version`.

The Go scaffold name `loop` must remain `loop`; the Rust scaffold name `loop` must remain protected as `app-loop`. Existing normalization, DNS length, database identifier, and Cargo artifact-length tests must continue to pass.

An undeclared lint check in a Rust repository must point to Go backend configuration, an undeclared sqlc check in a Go/no-database repository must point to PostgreSQL configuration, and a Go/PostgreSQL repository with a stale manifest must recommend recopying the Go tool declaration.

Rendered web CI must execute the client-only drift check. Tests must prove that mode neither invokes a backend exporter nor changes committed output, while the existing full `check` mode retains end-to-end behavior. Documentation must state that backend and web workflows jointly cover the two edges.

The complete configured test suite, format check, warnings-denied Clippy check, contract check, and structured work gates must all exit zero with fresh receipts.

## Idempotence and Recovery

Template snapshot refresh is deterministic and safe to repeat. Each slice is committed only after focused tests pass, so later failures can be repaired without rewriting completed commits. Do not rewrite `.agent/state/*.jsonl`; structured-work commands append records. Use only generic fixture names and `mktemp -d` for any generated repository smoke test.

If a contract-version test reveals a fixture intentionally pinned to v4, preserve it when it is testing legacy compatibility; only assertions of the current generated epoch should move to v5. If a final gate fails, repair the owning slice in a follow-up commit, rebuild the development binary, and rerun the affected gate before recording final evidence.

## Interfaces and Dependencies

No new third-party Rust, Go, or JavaScript dependency is planned. `CURRENT_CONTRACT_VERSION` becomes 5 while `MIN_SUPPORTED_CONTRACT_VERSION` and `LAST_VERSION_LOCKED_CONTRACT_VERSION` remain 2 and 3. `FeatureContext` gains default-false Go capability methods. Doctor gains an internal bounded `go.mod` authority parser. The generated contract script gains one CLI mode, `client-check`; existing `generate`, `check`, and `public-check` modes remain unchanged.

Plan revision note (2026-08-21, Codex): Replaced the one-line work-start body with a self-contained root-cause analysis, implementation sequence, compatibility policy, and observable acceptance criteria after tracing all five review findings.
