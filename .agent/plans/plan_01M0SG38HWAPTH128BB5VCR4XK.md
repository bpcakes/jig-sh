# Model SQL migration layouts explicitly

Introduce a closed repository migration-layout setting so Jig distinguishes ordinary flat SQLx migrations from complete versioned schema artifacts. Preserve flat migrations as the compatibility default, retain recursive immutability checks in both layouts, and remove the migration-add capability from generated contracts and runtime admission when versioned artifacts are configured.

## Progress

- [x] Add the closed config model and compatibility default.
- [x] Gate generated contracts plus CLI and MCP execution.
- [x] Add flat and versioned fixtures for rendering, rejection, and recursive immutability.
- [x] Regenerate embedded template snapshots and validate the required contract and test gates.
- [ ] Publish a source commit that downstream consumers can pin.

## Surprises & Discoveries

- Existing migration immutability parsing already handles nested paths; tests must prove that remains true under each layout.
- Downstream consumers need the config, generated contract, managed guidance, and source pin updated together because older Jig binaries reject unknown config fields.
- The repository Clippy command is currently blocked on an unrelated pre-existing `collapsible_if` warning in `crates/jig/build.rs:300` under Rust 1.97. The file is outside this change; the required contract and test gates pass.
- The full Nextest gate passed once before the final capability-condition tightening. After that one-line change, two reruns each passed 2,205 of 2,206 tests but the unrelated `runtime::worker_runner::tests::worker_supervision_rejects_output_beyond_the_capture_limit` failed process-tree cleanup under Nextest; the same test passes with the standard Rust test harness. The complete standard library suite passed 1,577 tests with 2 ignored.

## Decision Log

- Use the serialized values flat_migrations and versioned_artifacts.
- Default omitted config to flat_migrations for backward compatibility.
- Keep rust_migration_dir as the protected recursive root in either layout.
- Permit jig.migration_add only when SQLx is enabled and layout is flat_migrations.

## Outcomes & Retrospective

The implementation, focused acceptance tests, formatting, contract gate, complete standard library suite, and one full structured test gate pass. Later full-gate reruns are blocked only by the documented unrelated Nextest process-cleanup failure. The upstream source commit and downstream pin/regeneration remain pending.

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
6. Commit the upstream source only after validation so a downstream source pin can resolve it.

## Validation and acceptance

Success requires: old configs still expose and execute migration-add; versioned configs omit it from generated contracts; explicit CLI and MCP attempts fail without creating files; nested shipped artifacts are detected as immutable in both layouts; template snapshots match; repository formatting, tests, and contract gates pass. Clippy should be run and any unrelated baseline failure recorded without expanding this plan.

## Idempotence and recovery

All source and fixture edits are deterministic. Template regeneration can be rerun safely. No test may mutate a real schema directory. If a gate fails, fix the source or fixture and rerun the focused command before the full work gate.

## Interfaces and dependencies

The new .jig.toml key is rust_migration_layout with exactly flat_migrations or versioned_artifacts. FeatureContext gains a defaulted compatibility method so external or test implementations keep compiling. No database schema or SQLx metadata is changed.
