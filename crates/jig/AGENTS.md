# jig crate guide

## Purpose

`crates/jig` contains the repo-local `jig` CLI runtime used by generated repositories. It executes the generated command contract, manages append-only `.agent/state` memory, and handles template init/adopt/update flows.

## Key entrypoints

- `src/main.rs`: binary entrypoint.
- `src/lib.rs`: library entrypoint and module wiring.
- `src/root_commands.rs`: the single registry of top-level commands: name, help placement, and generated-launcher scope.
- `src/cli.rs`: clap command definitions.
- `src/cli/run.rs`: CLI startup boundaries and top-level command dispatch.
- `src/cli/run/dev_launch.rs`: `jig dev` and `jig proxy` dispatch, including the private dev-worker handoff whose worker owns the existing dev lifecycle and output; `src/cli/run/dev_unavailable.rs` replaces it in builds without the `dev-proxy` feature.
- `src/cli/run/vault_environment.rs`: CLI-startup boundary that withholds the reserved vault passphrase variables from commands that cannot capture them.
- `src/runtime.rs`: command-backed tool execution.
- `src/state.rs`: run history under `.agent/state`, with its diagnosis, archive, and restore maintenance.
- `src/runtime/loops/evidence.rs`: per-occurrence loop evidence recorded under Git metadata; `src/runtime/loops/show.rs` reports it for `jig loop show`.
- `src/ui.rs`: `jig ui` and `jig status --tui` CLI adapter for the separately owned `jig-ui` terminal crate.
- `src/ui/source.rs`: typed recorder and status source with retained local epochs.
- `src/status.rs`: read-only local repository and loop aggregate snapshots.
- `src/runtime/vault/tui.rs`: fixed-scope, process-local credential adapter for the separately owned `jig-vault-tui` crate.
- `src/bootstrap.rs`: init/adopt/update command surface.
- `src/bootstrap/`: bootstrap support for native template rendering, git, staged renders, and template-source handling.

## Edit here for X

- Change CLI flags or nested subcommands: the command family's module under `src/cli/`; `src/cli.rs` wires the top-level commands.
- Add a top-level command, in this order; the compiler or a test enforces each step:
  1. Declare it once in the `root_commands!` table in `src/root_commands.rs`.
  2. Add its `CommandKind` variant in `src/cli.rs`, taking `name` and `display_order` from the registry.
  3. Add the arms the compiler now requires: `launcher_command` and `run_command` in `src/cli/run.rs`, `may_capture_vault_passphrase` in `src/cli/run/vault_environment.rs`, and both availability matches in `src/info/commands.rs`.
  4. Regenerate the launcher's command lists with `JIG_REFRESH_LAUNCHER_COMMAND_LISTS=1 cargo test -p jig-sh --lib generated_launcher_command_lists`.
- Add a named `jig check` subcommand: `NamedCheck` in `src/command/check.rs` ties its selector to its legacy manifest tool, and `NamedCheckCommand` in `src/cli/check.rs` is its clap variant.
- Change which commands may keep the reserved vault passphrase variables past startup: `src/cli/run/vault_environment.rs` (read the vault runtime guide first).
- Add an agent provider: `src/agent_provider.rs` defines the internal contract; `src/claude/provider.rs` and `src/codex/provider.rs` are implementations. Keep home identity and credential policy provider-owned; `src/cli/agent_run.rs` owns common homes/launch orchestration. See [agent providers](../../docs/agent-providers.md).
- Change shared Claude/Codex path primitives: `src/home_paths.rs`; keep discovery and default-home policy in the provider modules.
- Change Claude credential lookup and read-only subscription usage: `src/claude/usage/`; keep secrets, HTTP, and platform storage out of the TUI and output renderers.
- Change shared operation signal supervision: `src/signal_supervision.rs`, with the process-wide signal session in `src/signal_supervision/session.rs`; `src/cli/home_picker.rs` supplies picker diagnostics and provider adapters supply entries to `jig-codex-tui`.
- Change transparent agent execution: `src/agent_launch.rs`; providers prepare their own commands and environment overrides.
- Change command-preview sanitization and warnings: `src/cli/output/command_display.rs`; provider renderers own layout and JSON interpretation.
- Propagate a child status or an already-reported failure: return `CliExit` from `src/exit.rs`. `src/cli/structured_error.rs` owns only the `--json` error protocol; do not add per-command marker error types there.
- Change manifest-tool behavior around command execution: `src/runtime.rs`.
- Change run history, state maintenance, or `jig state summary`: `src/state.rs` and `src/state/`.
- Change loop occurrence evidence, its retention, or `jig loop show`: `src/runtime/loops/evidence.rs` and `src/runtime/loops/show.rs`.
- Change the data exposed by the unified dashboard, including its run-history timeline and health aggregates: `src/ui/source/`.
- Change dashboard navigation, scheduling, or rendering: `crates/jig-ui/`.
- Change local status aggregation: `src/status.rs` and `src/status/`.
- Change terminal status navigation, refresh runtime, or rendering: `crates/jig-ui/src/terminal/`.
- Change Vault TUI navigation, forms, or rendering: `crates/jig-vault-tui/`; keep scope, environment capture, external tools, and core calls in `src/runtime/vault/tui.rs`.
- Change bounded owned-process execution or process-tree cleanup: [jig-owned-process](../jig-owned-process/AGENTS.md).
- Change init/adopt/update behavior: `src/bootstrap.rs` and `src/bootstrap/`; the legacy file-budget checker retirement evaluates `repo:file-budget` inline in `src/bootstrap/file_budget_lifecycle.rs`.
- Change repository source identity: `src/source_identity.rs`.

## Invariants

- Keep transport layers thin; shared behavior should live in runtime, state, or bootstrap helpers.
- Preserve generated-repo compatibility for `.jig.toml`, `.agent/jig-contract.json`, and `.agent/state/*.jsonl`.
- Treat `.agent/state/*.jsonl` as append-only unless a migration path is explicit.
- Keep execution tools aligned with the generated contract manifest and template outputs.
- Doctor checks whose remediation needs a human-chosen secret (`vault init`) are `operator_only`; never promote them into `next_step`, `next_issue`, `next_required_step`, or `optional_setup`. Report them through `operator_setup` instead.
- Before changing bootstrap entrypoints, toolchain checks, or templates, read the [bootstrap guide](src/bootstrap/AGENTS.md).
- Before changing vault entrypoints or dispatch, read the [vault runtime guide](src/runtime/vault/AGENTS.md).
- New top-level commands must choose a branch in the exhaustive `CommandKind::may_capture_vault_passphrase` match; commands that never unlock the vault withhold the passphrase. Do not add per-spawn passphrase plumbing.
- A top-level command's name and generated-launcher scope are declared only in `src/root_commands.rs`. Never repeat a root command name as a string elsewhere, and regenerate rather than hand-edit the launcher's command-list markers and `case` arms.
- Modules outside `src/cli/` do not depend on `crate::cli`: they take their own request types and return errors the CLI maps to its output protocol.
- Before changing process supervision or Bash probes, read the [process reference](../../docs/process-supervision.md) and [owned-process guide](../jig-owned-process/AGENTS.md).
- Use `scripts/jig` with the repository's selected release for routine checks. Validate edited runtime behavior with `scripts/jig-dev ...`, which incrementally builds the current source before invoking the launcher. `repo:source-runtime-check` is available for current-source contract validation; select checks for the affected behavior.

## Common commands

- `cargo test -p jig-sh`
- `cargo test -p jig-sh --test codex_launcher -- --nocapture` (requires a Unix PTY; set
  `JIG_ALLOW_PTY_TEST_SKIP=1` only when the environment is intentionally exempt)
- `cargo test --workspace`
- `scripts/jig file-budget audit` (standalone diagnostics; no runs or receipts)
- `scripts/jig check repo:source-runtime-check`
- `scripts/jig-dev check contract`
- `scripts/jig-dev check agent-guides`
- `scripts/jig-dev check agent-map`
