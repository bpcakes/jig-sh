# Harden Go backend review findings

This ExecPlan turns the comprehensive-review findings into four independently reviewable fixes. The goal is not only to repair the observed edge cases, but to reduce the number of places that independently encode backend behavior.

## Progress

- [x] Review the branch and classify each finding as a local omission or an ownership/abstraction problem.
- [ ] Preserve the backend-neutral migration directory through rendering and use it in generated policy workflow paths.
- [ ] Centralize derived Go module naming with Go-compatible validation.
- [ ] Align the interactive preset presentation with the retained numeric aliases.
- [ ] Make agent-guide discovery and entrypoint validation backend-aware, and render matching Go guidance.
- [ ] Build the development binary, run focused tests after each slice, then run the full configured test, format, Clippy, and contract gates.
- [ ] Record receipts, inspect the final commit series, and close structured work.

## Surprises & Discoveries

- Runtime configuration already treats `migration_dir` as authoritative, but the bootstrap answer model does not deserialize it and the workflow templates hardcode the generated Go path. This is a split-brain configuration bug rather than one typo.
- Default Go module derivation and explicit validation are separate algorithms. One can produce values the other rejects, while the validator still accepts values rejected by Go itself.
- `check agent-guides` is named generically but discovers only Rust crate roots and requires Rust entrypoint names. The generated Go guide inherited the same vocabulary.
- The numeric preset aliases intentionally retain the former `2 = harness-only` behavior. The defect is presentation ambiguity, not the compatibility mapping.

## Decision Log

- Preserve compatibility-sensitive numeric aliases and make their order explicit instead of remapping them.
- Keep `rust_crate_roots` as the persisted Rust contract. Add a backend-aware guide-policy boundary in the runtime instead of introducing a migration-heavy config rename.
- Treat `migration_dir` as a real project-owned answer because the public runtime and documentation already expose it as configurable.
- Keep Go module validation implemented in Rust so init can reject bad input before destination or vault side effects without requiring a working Go executable.

## Outcomes & Retrospective

Pending. Update this section after all gates pass with the final behavior, commit series, and any residual limitations.

## Context and orientation

`crates/jig/src/bootstrap/answers.rs` loads persisted `.jig.toml` values and renders managed templates. `crates/jig/src/context.rs` loads the same file for runtime commands. `templates/project/` contains managed harness files. `crates/jig/src/cli/init_wizard.rs` owns default project-shape derivation and interactive prompts. `crates/jig/src/bootstrap/scaffold/names.rs` owns scaffold naming validation. `crates/jig/src/policy/agent_map.rs` implements `check agent-guides`.

## Plan of work

First, add `migration_dir` to the answer boundary, preserve explicit values, derive the generated default only when absent, and render workflow filters from that resolved value. Second, introduce one helper for default Go module derivation and strengthen validation for Go-reserved path elements. Third, make the prompt visibly match its numeric compatibility aliases. Fourth, expose backend-aware guide roots and entrypoint-reference validation while keeping existing Rust behavior stable.

## Concrete steps

For every slice, add focused regression tests, run the narrow crate tests, inspect the diff, and create one conventional commit. Do not combine unrelated cleanup with these fixes. After all slices, rebuild `target/debug/jig`, set `JIG_DEV_BIN=target/debug/jig` for harness commands, run `scripts/jig work check`, the configured gates, and `scripts/jig check test`.

## Validation and acceptance

Acceptance requires tests proving that a custom `migration_dir` survives answer resolution and appears in both workflow trigger sections; derived dot-edge and reserved Go module paths are handled before writes; the prompt presentation agrees with numeric aliases; Go package guides are actually inspected with `.go` entrypoint references; existing Rust guide behavior remains unchanged; and the complete repository test gate succeeds.

## Idempotence and recovery

All source edits are ordinary Git commits. Focused tests and full gates are repeatable. If a slice fails, amend only before its commit; after a slice is committed, repair it in a new commit rather than rewriting unrelated history. `.agent/state/*.jsonl` remains append-only.

## Interfaces and dependencies

No new external dependency is planned. Persisted `.jig.toml` compatibility must remain intact: legacy Rust repositories continue falling back from `migration_dir` to `rust_migration_dir`, while Go repositories retain their generated default unless they explicitly configure another path.
