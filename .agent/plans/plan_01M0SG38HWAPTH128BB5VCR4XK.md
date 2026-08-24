# Model SQL migration layouts explicitly

Introduce a closed repository migration-layout setting so Jig distinguishes ordinary flat SQLx migrations from complete versioned schema artifacts. Preserve flat migrations as the compatibility default, retain recursive immutability checks in both layouts, and remove the migration-add capability from generated contracts and runtime admission when versioned artifacts are configured.

## Progress

- [x] Add the closed config model and compatibility default.
- [x] Gate generated contracts plus CLI and MCP execution.
- [x] Add flat and versioned fixtures for rendering, rejection, and recursive immutability.
- [x] Regenerate embedded template snapshots and validate the required contract and test gates.
- [x] Revalidate the final local commits with the focused migration suite, template consistency, formatting, the complete standard `jig-sh` suite, and a lint pass that exempts only unrelated Rust 1.97 baseline lints.
- [x] Publish the validated branch and open upstream pull request https://github.com/bpcakes/jig-sh/pull/11 so downstream consumers can pin a reachable source revision.
- [x] Resolve the PR's change-sensitive Rust LOC gate by extracting migration/vault config types and the new migration-layout policy tests, then rerun the focused and complete standard suites.
- [x] Resolve the PR's Rust 1.98 CI lint failures across the workspace, including the Linux-only `jig-dev-proxy` lint surface, while retaining Rust 1.88 compatibility.

## Surprises & Discoveries

- Existing migration immutability parsing already handles nested paths; tests must prove that remains true under each layout.
- Downstream consumers need the config, generated contract, managed guidance, and source pin updated together because older Jig binaries reject unknown config fields.
- The moving stable CI toolchain advanced through Rust 1.97 to Rust 1.98 while the PR was open. That promoted pre-existing workspace lint drift to hard failures, primarily `collapsible_if`, plus `manual_is_multiple_of`, Linux-only `chunks_exact_to_as_chunks`, and redundant test imports. The compiler-suggested behavior-preserving rewrites clear both the workspace and no-default-features Clippy jobs and still compile on Rust 1.88.
- The first PR run correctly rejected three touched legacy files under the Rust LOC policy. The final organization returns `cli/tests.rs` to its base contents, reduces `context.rs` to 991 lines, and reduces `policy/tests.rs` to 735 lines; the change-sensitive LOC gate then passes.
- The full Nextest gate passed once before the final capability-condition tightening. Three reruns after that change each passed 2,205 of 2,206 tests but the unrelated `runtime::worker_runner::tests::worker_supervision_rejects_output_beyond_the_capture_limit` failed process-tree cleanup under Nextest; the exact test passes alone under Nextest and in the complete standard harness. The fresh complete standard `jig-sh` suite passed 1,577 tests with 2 ignored plus every integration target.

## Decision Log

- Use the serialized values flat_migrations and versioned_artifacts.
- Default omitted config to flat_migrations for backward compatibility.
- Keep rust_migration_dir as the protected recursive root in either layout.
- Permit jig.migration_add only when SQLx is enabled and layout is flat_migrations.

## Outcomes & Retrospective

The implementation is committed as `dbd12c5` plus the template-output cleanup `2f1a744` and published on `codex/migration-layout` in pull request #11. Focused acceptance tests, formatting, template consistency, the contract gate, the complete standard suite, and the post-publication LOC remediation succeed. The CI follow-up also clears the Rust 1.98 workspace, no-default-features, and Linux-only dev-proxy lint surfaces while preserving the Rust 1.88 MSRV. The complete local Nextest command continues to expose its documented process-cleanup flake under full-suite concurrency; each surfaced test passes immediately in isolation, and the same locked suite passes in Linux and macOS CI. Downstream consumers can pin the reachable PR head and regenerate their managed harness.

## Context and orientation

Repository configuration is loaded in crates/jig/src/context.rs. Bootstrap answers and templates live under crates/jig/src/bootstrap and templates/project. Feature availability and native tool requirements are owned by crates/jig-contract, crates/jig-features, and crates/jig-sqlx. Policy enforcement and migration immutability checks live in crates/jig/src/policy.rs; MCP execution coverage lives under crates/jig/src/runtime/tests.

## Plan of work

Define a serde-backed RustMigrationLayout enum and expose migration-add availability through FeatureContext. Thread the field through bootstrap adoption and rendering. Conditionally render jig.migration_add and managed guidance. Reject invalid direct CLI/MCP calls before filesystem mutation and make contract validation catch an advertised unavailable native tool. Add regression tests for omitted/default flat config, explicit versioned rendering, direct rejection, and nested immutable changes in both modes.

## Concrete steps

1. Update config, bootstrap answers, feature traits, and template context.
2. Update SQLx feature/tool selection and policy admission.
3. Update project templates and embedded snapshots using the repository generator.
4. Add focused policy, runtime, and bootstrap tests.
5. Run formatting, focused tests, contract checks, full tests, and work evidence/gates.
6. Publish the validated upstream source commits so a downstream source pin can resolve them.

## Validation and acceptance

Success requires: old configs still expose and execute migration-add; versioned configs omit it from generated contracts; explicit CLI and MCP attempts fail without creating files; nested shipped artifacts are detected as immutable in both layouts; template snapshots match; repository formatting, tests, and contract gates pass. Clippy should be run and any unrelated baseline failure recorded without expanding this plan.

## Idempotence and recovery

All source and fixture edits are deterministic. Template regeneration can be rerun safely. No test may mutate a real schema directory. If a gate fails, fix the source or fixture and rerun the focused command before the full work gate.

## Interfaces and dependencies

The new .jig.toml key is rust_migration_layout with exactly flat_migrations or versioned_artifacts. FeatureContext gains a defaulted compatibility method so external or test implementations keep compiling. No database schema or SQLx metadata is changed.
