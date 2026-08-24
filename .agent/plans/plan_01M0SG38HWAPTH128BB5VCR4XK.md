# Model SQL migration layouts explicitly

Introduce a closed repository migration-layout setting so Jig distinguishes ordinary flat SQLx migrations from complete versioned schema artifacts. Preserve flat migrations as the compatibility default, retain recursive immutability checks in both layouts, and remove the migration-add capability from generated contracts and runtime admission when versioned artifacts are configured.

## Progress

- [x] Add the closed config model and compatibility default.
- [x] Gate generated contracts plus CLI and MCP execution.
- [x] Add flat and versioned fixtures for rendering, rejection, and recursive immutability.
- [x] Regenerate embedded template snapshots and validate the required contract and test gates.
- [x] Revalidate the final local commits with the focused migration suite, template consistency, formatting, the complete standard `jig-sh` suite, and a lint pass that exempts only unrelated Rust 1.97 baseline lints.
- [x] Publish the validated branch and open upstream pull request https://github.com/bpcakes/jig-sh/pull/11 so downstream consumers can pin a reachable source revision.

## Surprises & Discoveries

- Existing migration immutability parsing already handles nested paths; tests must prove that remains true under each layout.
- Downstream consumers need the config, generated contract, managed guidance, and source pin updated together because older Jig binaries reject unknown config fields.
- The repository Clippy command is currently blocked on unrelated pre-existing Rust 1.97 lints: `collapsible_if` in `crates/jig/build.rs:300` and `manual_is_multiple_of` in `crates/jig-vault/src/redact.rs:330`. A targeted all-targets lint pass for `jig-contract`, `jig-sqlx`, and `jig-sh` passes with only those two baseline lints exempted.
- The full Nextest gate passed once before the final capability-condition tightening. Three reruns after that change each passed 2,205 of 2,206 tests but the unrelated `runtime::worker_runner::tests::worker_supervision_rejects_output_beyond_the_capture_limit` failed process-tree cleanup under Nextest; the exact test passes alone under Nextest and in the complete standard harness. The fresh complete standard `jig-sh` suite passed 1,577 tests with 2 ignored plus every integration target.

## Decision Log

- Use the serialized values flat_migrations and versioned_artifacts.
- Default omitted config to flat_migrations for backward compatibility.
- Keep rust_migration_dir as the protected recursive root in either layout.
- Permit jig.migration_add only when SQLx is enabled and layout is flat_migrations.

## Outcomes & Retrospective

The implementation is committed as `dbd12c5` plus the template-output cleanup `2f1a744` and published on `codex/migration-layout` in pull request #11. Focused acceptance tests, formatting, template consistency, the contract gate, the complete standard suite, the isolated Nextest regression, and the scoped lint pass all succeed. Later full-gate reruns are blocked only by the documented unrelated Nextest process-cleanup flake. Downstream consumers can now pin the reachable PR head and regenerate their managed harness.

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
