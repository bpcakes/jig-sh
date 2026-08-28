# Harden component-native backend policy

This change completes backend boundaries that were left repository-global during the contract-v6 component/action cutover. It also closes two independent execution-boundary omissions found by the branch review. Afterward, a repository can contain multiple language components without diagnostics assuming one root backend, while migration authoring retains one explicit repository-wide owner and fails closed when its format would be ambiguous.

## Progress

- [x] Research the v6 component model and the earlier backend-neutral migration decision.
- [x] Decide whether mixed language components and mixed migration authoring are supported.
- [x] Add focused regression tests for ambiguous migration ownership, nested Go authority, identity-free schema snapshots, and invalid Go subcommands.
- [x] Implement the smallest shared policy changes that make those tests pass.
- [x] Run focused tests, formatting, strict Clippy, the complete `jig-sh` test suite, and Jig contract checks with the development binary.
- [x] Run the Opus comprehensive review and remediate every critical or medium finding.

## Surprises & Discoveries

- Contract v6 intentionally supports authored multi-language component topology. The checked-in v6 recopy test uses a Go component under `services/api` and a Rust component under `services/worker`.
- Migration authoring intentionally remained repository-wide: one `migration_dir`, one public `jig migration add` command, and one stable compatibility tool identifier. The missing rule is that this source of truth must have exactly one format owner.
- The schema sandbox already supplies a synthetic identity for an unborn repository, but the tracked-change `git stash create` path does not.
- Go component roots need not equal Go module roots, so doctor resolves the nearest ancestor `go.mod` within the repository and deduplicates shared module authorities.

## Decision Log

- Decision: Support mixed Go/Rust/TypeScript components in v6 and make diagnostics derive language authority from component roots.
  Rationale: The v6 design and regression fixtures explicitly preserve multi-backend authored topology.
- Decision: Do not infer one migration format from a repository-wide `is_go_backend` boolean.
  Rationale: A mixed catalog makes that boolean lossy and can execute a component target with the wrong format.
- Decision: Preserve one repository-wide migration directory and require an unambiguous migration authoring owner.
  Rationale: Existing compatibility commands and configuration intentionally expose one migration source of truth. Supporting multiple simultaneous formats needs a future component-scoped configuration contract rather than implicit conventions.
- Decision: Preserve legacy contract behavior; apply component-aware ownership rules to contract v6.
  Rationale: Contract versions 2–5 are persisted compatibility boundaries.
- Decision: Expose migration format ownership separately from SQLx migration-layout enablement.
  Rationale: Format selection and layout validation are distinct capabilities; overloading one predicate made unrelated SQLx components able to veto Goose authoring.
- Decision: Reject invalid migration ownership when loading an executable repository catalog while retaining precise feature-admission diagnostics.
  Rationale: Execution must fail closed, but command discovery and error reporting still need enough context to explain the catalog defect.

## Outcomes & Retrospective

The component-aware cutover is complete. Mixed-language v6 repositories now resolve Go toolchain authority from component/module topology, retain Rust guide coverage through declared crate roots, and select migration format only from the single native migration action owner. Ambiguous or incompatible ownership fails closed. Schema checks no longer depend on developer Git identity, and generated Go CLIs reject invalid subcommands before loading runtime configuration.

Focused regressions, formatting, the repository format script, workspace Clippy with warnings denied, and the complete structured work gate passed. The final work receipt is `receipt_01M0TWNX06TB0SJHDYPNPARSRG`: 2,503 primary tests, 438 vault tests, and 2 vault-TUI tests passed. Repeated Opus and independent Codex review found no critical, high, or medium issue in the repaired working-tree scope; the remaining observations are low-severity cleanup or test-hardening suggestions.

## Context and orientation

`crates/jig/src/context.rs` projects both legacy backend configuration and v6 component adapters. `crates/jig/src/policy/migration_add.rs` owns migration file creation. `crates/jig/src/repository.rs` validates component/action catalogs, and `crates/jig/src/runtime/run_execution.rs` executes planned native targets. Doctor version authority lives in `crates/jig/src/doctor_parts/version_checks.rs`; agent-guide policy lives in `crates/jig/src/policy/agent_map.rs`. The schema snapshot is in `crates/jig/src/policy/schema.rs`. Go scaffold process ordering is rendered from `templates/scaffolds/go-react/workspace/cmd/api/main.go.jinja` and its embedded snapshot.

## Plan of work

First add tests that express the intended boundaries. Introduce a typed v6 migration-owner query that resolves the sole declared native migration action to SQLx or Go/Postgres and rejects missing, incompatible, or multiple owners. Route both generic migration commands and planned targets through that authority without changing legacy behavior. Replace root-only Go doctor and guide assumptions with component-derived roots. Add deterministic Git identity to `stash create`. Parse the generated Go command before configuration loading and update the embedded template snapshot.

## Concrete steps

1. Extend context/catalog tests with mixed component and migration-owner fixtures.
2. Extend doctor and policy tests with nested Go roots and identity-free Git configuration.
3. Update context/policy/runtime boundaries and their error messages.
4. Update Go scaffold source and embedded snapshot together.
5. Build `target/debug/jig`, run focused tests, then use `JIG_DEV_BIN=target/debug/jig` for Jig contract checks and work evidence.
6. Run the complete Opus comprehensive review over the branch plus uncommitted repairs and repeat remediation and review until no critical or medium findings remain.

## Validation and acceptance

Success requires focused regression tests to pass, `cargo fmt --all -- --check`, strict Clippy, `cargo test -p jig-sh`, and relevant `scripts/jig` checks through `JIG_DEV_BIN=target/debug/jig`. The final merged Claude/Codex review must contain no critical or medium findings.

## Idempotence and recovery

All source edits are ordinary version-controlled changes. Test fixtures use temporary repositories and generic identifiers. If a gate fails, keep the plan open, record the failure in this document, fix the owning boundary, and rerun only the focused test before repeating the complete gate.

## Interfaces and dependencies

No new external dependency is planned. Contract versions 2–5 and the public `jig.migration_add` compatibility identifier remain supported. Any new internal type should stay below `jig-contract` unless it becomes serialized contract authority.
