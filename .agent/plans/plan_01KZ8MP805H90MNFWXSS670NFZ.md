# Extract owned process, bootstrap, and state crates

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while implementation proceeds. This document follows `.agent/PLANS.md`.

## Purpose / Big Picture

The `jig-sh` package currently compiles the command-line composition layer together with three substantial implementation domains: generic owned-child-process supervision, project initialization and template rendering, and append-only agent state. After this refactor, those domains live in `jig-owned-process`, `jig-bootstrap`, and `jig-state`. Users continue to run the same `jig` commands with identical JSON, help, MCP, template, and persisted-state behavior, while maintainers gain independently testable crates with explicit dependencies and smaller recompilation boundaries.

The result is observable by building the `jig` binary, running its existing focused and workspace tests, and dogfooding the repository contract through `JIG_DEV_BIN=target/debug/jig scripts/jig ...`. No `.agent/state` schema, generated repository file, command name, or public wire format changes in this work.

## Progress

- [x] (2026-08-05 09:39Z) Mapped the existing workspace, crate guides, module fan-in/fan-out, and extraction order.
- [x] (2026-08-05 09:39Z) Opened structured work and created this ExecPlan.
- [x] (2026-08-05 09:47Z) Extracted `crates/jig-owned-process`, migrated all `jig-sh` callers, passed its 28 lifecycle tests, and compiled every `jig-sh` test target.
- [ ] Extract `crates/jig-bootstrap`, relocate template build generation and snapshots, migrate CLI adapters, and pass focused tests.
- [ ] Extract `crates/jig-state`, replace `RepoContext` coupling with a narrow state context, migrate runtime/status/UI callers, and pass focused tests.
- [ ] Update workspace metadata, crate guides, agent map, and any release/package configuration that enumerates crates.
- [ ] Build the development binary and pass formatting, Clippy, focused tests, workspace tests, contract checks, and configured repository gates.
- [ ] Inspect the final diff and close structured work with receipts attached.

## Surprises & Discoveries

- Observation: The extraction is source-only and must preserve multiple high-risk invariants rather than redesign them.
  Evidence: `crates/jig/AGENTS.md` documents fail-closed process-tree cleanup, append-only state compatibility, and generated-template safety as runtime invariants.

- Observation: Owned-process subprocess tests depend on their exact nested unit-test path.
  Evidence: Keeping the implementation under `jig_owned_process::process` preserved helper selectors such as `process::tests::owned_process_output_escape_helper`; all 28 tests passed after the move.

## Decision Log

- Decision: Extract in the order owned process, bootstrap, then state.
  Rationale: `process` has broad fan-in and no production dependency on another `jig-sh` module. Making it a lower-level crate first gives bootstrap and state a legal downward dependency for Git/process work.
  Date/Author: 2026-08-05 / Codex.

- Decision: Keep CLI parsing, MCP dispatch, human/JSON presentation, and top-level runtime orchestration in `jig-sh`.
  Rationale: These are composition responsibilities. Moving them into domain crates would recreate the current coupling instead of establishing reusable boundaries.
  Date/Author: 2026-08-05 / Codex.

- Decision: Preserve implementation and tests mechanically before considering API cleanup.
  Rationale: This refactor crosses template and durable-state boundaries. Minimal visibility and import changes are safer than combining extraction with behavioral redesign.
  Date/Author: 2026-08-05 / Codex.

## Outcomes & Retrospective

Implementation is in progress. At completion this section will record the final crate dependency graph, validation receipts, any compatibility risks discovered, and any follow-up work intentionally left out.

## Context and Orientation

The Cargo workspace is declared in `Cargo.toml`. The `crates/jig` package builds both the `jig` binary and a small library facade. Its `src/lib.rs` currently declares every CLI and runtime module.

`crates/jig/src/process.rs` and `crates/jig/src/process/interaction.rs` implement bounded command output, cancellable waiting, Unix process-group supervision, Windows Job Object supervision, and fail-closed descendant cleanup. A process tree means a child command and every descendant it creates. Owned supervision means Jig establishes a stable operating-system identity before allowing the child to run and confirms cleanup before returning.

`crates/jig/src/bootstrap.rs`, its `bootstrap/` directory, and `crates/jig/build.rs` implement `jig init`, `jig adopt`, and `jig update`. They own template-source selection, embedded project and scaffold templates, answer resolution, rendering, safe filesystem publication, Git initialization, and rollback. `crates/jig/src/cli/bootstrap_run.rs` and `cli/init_wizard.rs` are the command-line adapters and remain in `jig-sh`.

`crates/jig/src/state.rs` and its `state/` directory own `.agent/state/*.jsonl`, including sessions, plans, receipts, decisions, diagnostics, compaction, backup, restore, archive, and timeline projection. These files are durable state: they may outlive any running process and must remain byte/schema compatible. Runtime command dispatch stays in `crates/jig/src/runtime.rs`; status and UI snapshot adapters remain in `crates/jig/src/status.rs` and `crates/jig/src/ui/snapshot.rs`.

Each new crate needs an `AGENTS.md` using the repository’s required headings, a package manifest inheriting workspace version/edition/license/rust-version, and a public library facade limited to the existing caller-facing surface. Existing dedicated implementations in `jig-dev-proxy` and `jig-vault` remain separate because they have different route-ownership and secret-zeroization invariants.

## Plan of Work

First create `crates/jig-owned-process`. Move the process sources and their tests without changing function bodies. Promote only the existing `pub(crate)` caller-facing types and functions to `pub`; keep internal supervision helpers private. Add the exact dependencies formerly supplied by `jig-sh`, update every caller to import `jig_owned_process`, remove `mod process` from `jig-sh`, and run the new crate tests plus `jig-sh` tests.

Second create `crates/jig-bootstrap`. Move `bootstrap.rs`, the complete `bootstrap/` tree, and the template-generation portion of `crates/jig/build.rs`. Resolve crate-relative imports by depending downward on `jig-owned-process` and by exposing or relocating only genuinely shared repository helpers. Keep Clap option types temporarily if moving them separately would broaden the change; the acceptance criterion is a one-way dependency from `jig-sh` into `jig-bootstrap`, never the reverse. Move embedded template snapshots with their owning code and keep refresh commands accurate. Update CLI adapters to import `jig_bootstrap` and prove init/adopt/update and snapshot drift tests still pass.

Third create `crates/jig-state`. Introduce a narrow state context owned by that crate containing the repository root and current-session path, or an equivalent trait implemented by `RepoContext`; do not make `jig-state` depend on `jig-sh`. Move state-specific request DTOs if required to break that dependency. Move Git receipt collection with the receipt domain or pass its result into state without changing persisted records. Update runtime, status, and UI callers and prove existing state tests, maintenance recovery tests, and gate receipt behavior are unchanged.

Finally update the root workspace manifest, workspace dependency aliases, `crates/jig/Cargo.toml`, `agent-map.md`, crate guides, package/release scripts, and documentation that enumerate crate paths. Remove obsolete modules only after all callers use the new crates. Format and inspect every moved file, but do not combine this work with unrelated function simplification or schema changes.

## Concrete Steps

Work from `/home/aa/Documents/jig-sh`.

For each milestone, use `apply_patch` for manifest and source edits, then run the narrowest useful checks. The expected sequence is:

    cargo test -p jig-owned-process
    cargo test -p jig-sh process --no-fail-fast
    cargo test -p jig-bootstrap
    cargo test -p jig-sh bootstrap --no-fail-fast
    cargo test -p jig-state
    cargo test -p jig-sh state --no-fail-fast

After all moves, build the development binary and force repository commands through it:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract --no-receipt
    JIG_DEV_BIN=target/debug/jig scripts/jig check test

Use `scripts/jig work check --plan-id plan_01KZ8MP805H90MNFWXSS670NFZ`, `work evidence`, `work gates`, and `work receipts` to attach the configured results. Expected success is exit status zero from each applicable command and no generated-template snapshot drift.

## Validation and Acceptance

`jig-owned-process` is accepted when its normal success, timeout, cancellation, escaped-descendant, incomplete-output, cooperative-interaction, and cleanup-failure tests pass on the current platform, and every former caller compiles against the crate.

`jig-bootstrap` is accepted when init, adopt, update, embedded-template drift, scaffold generation, Git safety, transactional rollback, and template-source tests pass. The development binary must render the same committed template snapshots and existing CLI help/JSON tests must remain unchanged.

`jig-state` is accepted when existing session, plan, decision, receipt, diagnostics, compaction, archive, export, restore, canonical duplicate, cancellation, and UI/status projection tests pass. No migration is allowed: existing `.agent/state/*.jsonl` fixtures must deserialize unchanged, mutations must retain existing locking and backup behavior, and no command may rewrite state merely because the crate boundary changed.

The full work is accepted when the workspace dependency graph contains the three new crates with no dependency back to `jig-sh`, the `jig` binary exposes the same command behavior, all configured gates pass, and the final diff contains only crate extraction, ownership documentation, and necessary build/package metadata.

## Idempotence and Recovery

The source moves are performed one crate at a time and validated before the next milestone. A failed compile can be retried after fixing imports because no user repository or durable state is mutated by compilation. Existing `.agent/state` files are append-only; only normal structured-work records may be appended.

Do not use destructive Git reset or checkout operations. If a move is partially applied, compare the old and new file lists, restore missing content with `apply_patch`, and leave both copies until the new crate compiles. Remove the old module declaration and source only after its replacement passes focused tests.

## Artifacts and Notes

Baseline architectural measurements from the pre-change tree are: `bootstrap` approximately 19,653 source lines across 49 non-dedicated-test files, `state` approximately 7,996 lines across 13 files, and owned process approximately 1,815 lines across two files. These are comparison aids, not acceptance thresholds.

Plan revision note, 2026-08-05: Initial self-contained plan created after the repository architecture review and the user’s explicit authorization to extract all three recommended crates.

## Interfaces and Dependencies

`jig-owned-process` must expose the existing caller-facing `BoundedProcessOutput`, `OwnedProcessTreeError`, `OwnedProcessTreeOutput`, `ProcessOutputLimits`, `format_exit_status`, `require_success`, checked-output helpers, owned-tree execution helpers, and cooperative interaction API. It must not depend on another Jig crate.

`jig-bootstrap` must expose the option/request types and `run_init`, `run_adopt`, `run_update`, preflight/wizard helpers, preset report, Git environment helpers still needed by the composition layer, and repository-file helpers still needed by policy. It may depend on `jig-owned-process` and neutral existing workspace crates, but never `jig-sh`.

`jig-state` must expose the existing state request types, plan/session/receipt/timeline models used by callers, mutation/query functions, `now_ms`, and a state context abstraction. It may depend on `jig-owned-process` if Git fingerprint collection remains state-owned and on `jig-contract` for stable tool identifiers, but never `jig-sh`.
