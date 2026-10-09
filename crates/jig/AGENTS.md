# jig crate guide

## Purpose

`crates/jig` contains the repo-local `jig` CLI runtime used by generated repositories. It executes the generated command contract, manages append-only `.agent/state` memory, and handles template init/adopt/update flows.

## Key entrypoints

- `src/main.rs`: binary entrypoint.
- `src/lib.rs`: library entrypoint and module wiring.
- `crates/jig-commands/src/root_commands.rs`: the single registry of top-level commands: name, help placement, and generated-launcher scope. `src/launcher_command_lists.rs` keeps the generated launcher in step with it.
- `src/cli.rs`: clap command definitions.
- `src/cli/run.rs`: CLI startup boundaries and top-level command dispatch, one line per command.
- `src/cli/run/launcher_handoff.rs`: generated-launcher handoff validation and the runtime compatibility probe.
- `src/cli/runtime_dispatch.rs`: the one path from a parsed command to `runtime::dispatch`.
- `src/cli/<family>.rs` and `src/cli/<family>/`: one home per command family. The root file holds its clap types and dispatch description; `run.rs`, `convert.rs` and `render.rs` beside it hold its runner, its conversion to runtime requests, and its human-readable output.
- `src/cli/proxy/run.rs`: `jig dev` and `jig proxy` dispatch, including the private dev-worker handoff whose worker owns the existing dev lifecycle and output; `src/cli/proxy/run_unavailable.rs` replaces it in builds without the `dev-proxy` feature.
- `src/cli/run/vault_environment.rs`: CLI-startup boundary that withholds the reserved vault passphrase variables from commands that cannot capture them.
- `src/runtime.rs`: command-backed tool execution.
- `src/ui.rs`: `jig ui` and `jig status --tui` CLI adapter for the separately owned `jig-ui` terminal crate.
- `src/ui/source.rs`: typed recorder and status source with retained local epochs.
- `src/status.rs`: read-only local repository and loop aggregate snapshots.
- `src/runtime/vault/tui.rs`: fixed-scope, process-local credential adapter for the separately owned `jig-vault-tui` crate.

## Edit here for X

- Change CLI flags or nested subcommands: the command family's `src/cli/<family>.rs`; `src/cli.rs` wires the top-level commands. Convert its options to runtime requests in `src/cli/<family>/convert.rs`, or in the root file when the conversion is a few lines.
- Change a command's argument rules (argv normalization, conflicts with global flags, usage hints): that command's module under `src/cli/`. `src/cli/run/argument_parsing.rs` is the generic pipeline that asks each owner; hints read Clap's structured error context, never its rendered text.
- Add a top-level command, in this order; the compiler or a test enforces each step:
  1. Declare it once in the `root_commands!` table in `crates/jig-commands/src/root_commands.rs`.
  2. Add its `CommandKind` variant in `src/cli.rs`, taking `name` and `display_order` from the registry.
  3. Add the arms the compiler now requires: `launcher_command` in `src/cli/run/launcher_handoff.rs`, `run_command` in `src/cli/run.rs`, `may_capture_vault_passphrase` in `src/cli/run/vault_environment.rs`, and both availability matches in `src/info/commands.rs`.
  4. Regenerate the launcher's command lists with `JIG_REFRESH_LAUNCHER_COMMAND_LISTS=1 cargo test -p jig-sh --lib generated_launcher_command_lists`.
- Run a command through the runtime: implement `into_dispatch` beside its clap type, returning a `RuntimeDispatch` that names its renderer and whether `ok: false` fails the command. A command with its own flow gets a `run_<name>_command` in its own `src/cli/` module; keep `run_command` to one line per command.
- Add a named `jig check` subcommand: `NamedCheck` in `src/command/check.rs` ties its selector to its legacy manifest tool, and `NamedCheckCommand` in `src/cli/check.rs` is its clap variant.
- Change which commands may keep the reserved vault passphrase variables past startup: `src/cli/run/vault_environment.rs` (read the vault runtime guide first).
- Add an agent provider, or change Claude/Codex home discovery, credentials, or usage: [jig-agents](../jig-agents/AGENTS.md). `src/cli/agent_run.rs` owns common homes/launch orchestration and signal supervision. See [agent providers](../../docs/agent-providers.md).
- Change shared operation signal supervision: `src/signal_supervision.rs`, with the process-wide signal session in `src/signal_supervision/session.rs`; `src/cli/home_picker.rs` supplies picker diagnostics and provider adapters supply entries to `jig-agents-tui`.
- Change transparent agent execution: `src/agent_launch.rs`; providers prepare their own commands and environment overrides.
- Change how a command's result is shown to people: its formatter in `src/cli/<family>/render.rs`, or in `src/cli/output.rs` for the summaries that still live there (check, run, setup, migration, agent-map and manifest tools). The command passes that function to `emit` (or names it in its `RuntimeDispatch`); there is no central output table to extend.
- Change command-preview sanitization and warnings: `src/cli/output/command_display.rs`; provider renderers own layout and JSON interpretation.
- Propagate a child status or an already-reported failure: return `CliExit` from `src/exit.rs`. `src/cli/structured_error.rs` owns only the `--json` error protocol; do not add per-command marker error types there.
- Change manifest-tool behavior around command execution: `src/runtime.rs`.
- Change run history, state maintenance, or `jig state summary`: [jig-state](../jig-state/AGENTS.md); `src/cli/state.rs` is the CLI adapter.
- Change loop scheduling, occurrences and their evidence, `jig loop show`, Codex task workers, or the PR manager: [jig-loops](../jig-loops/AGENTS.md); `src/cli/loops.rs` is the CLI adapter.
- Change the data exposed by the unified dashboard, including its run-history timeline and health aggregates: `src/ui/source/`.
- Change dashboard navigation, scheduling, or rendering: `crates/jig-ui/`.
- Change local status aggregation: `src/status.rs` and `src/status/`.
- Change terminal status navigation, refresh runtime, or rendering: `crates/jig-ui/src/terminal/`.
- Change Vault TUI navigation, forms, or rendering: `crates/jig-vault-tui/`; keep scope, environment capture, external tools, and core calls in `src/runtime/vault/tui.rs`.
- Change bounded owned-process execution or process-tree cleanup: [jig-owned-process](../jig-owned-process/AGENTS.md).
- Change init/adopt/update behavior, templates, or the embedded template snapshots: [jig-bootstrap](../jig-bootstrap/AGENTS.md); `src/cli/bootstrap_run.rs` and the init wizard in `src/cli/` are the CLI adapters.
- Change policy checks (contract, agent guides, agent map, SQLx, schema): [jig-policy](../jig-policy/AGENTS.md).
- Change the repository action catalog, run planning, affected selection, or repository source identity: [jig-repository](../jig-repository/AGENTS.md).
- Change `.jig.toml` or manifest loading and validation, contract-version support, or the shared test fixtures in `crate::test_env`: [jig-context](../jig-context/AGENTS.md).
- Change Git program selection (`JIG_GIT_BIN`), known-repository `GIT_*` scrubbing, or Git metadata-file reads: [jig-git](../jig-git/AGENTS.md).

## Invariants

- Keep transport layers thin; shared behavior should live in runtime, state, or bootstrap helpers.
- Preserve generated-repo compatibility for `.jig.toml`, `.agent/jig-contract.json`, and `.agent/state/*.jsonl`.
- Treat `.agent/state/*.jsonl` as append-only unless a migration path is explicit.
- Keep execution tools aligned with the generated contract manifest and template outputs.
- Doctor checks whose remediation needs a human-chosen secret (`vault init`) are `operator_only`; never promote them into `next_step`, `next_issue`, `next_required_step`, or `optional_setup`. Report them through `operator_setup` instead.
- Before changing bootstrap entrypoints, toolchain checks, or templates, read the [bootstrap guide](../jig-bootstrap/AGENTS.md).
- Before changing vault entrypoints or dispatch, read the [vault runtime guide](src/runtime/vault/AGENTS.md).
- New top-level commands must choose a branch in the exhaustive `CommandKind::may_capture_vault_passphrase` match; commands that never unlock the vault withhold the passphrase. Do not add per-spawn passphrase plumbing.
- A top-level command's name and generated-launcher scope are declared only in `crates/jig-commands/src/root_commands.rs`. Never repeat a root command name as a string elsewhere, and regenerate rather than hand-edit the launcher's command-list markers and `case` arms.
- Modules outside `src/cli/` do not depend on `crate::cli`: they take their own request types and return errors the CLI maps to its output protocol.
- Before changing process supervision or Bash probes, read the [process reference](../../docs/process-supervision.md) and [owned-process guide](../jig-owned-process/AGENTS.md).
- Use `scripts/jig` with the repository's selected release for routine checks. Validate edited runtime behavior with `scripts/jig-dev ...`, which incrementally builds the current source before invoking the launcher. `repo:source-runtime-check` is available for current-source contract validation; select checks for the affected behavior.

## Common commands

- `cargo test -p jig-sh`
- `cargo test -p jig-sh --test codex_launcher -- --nocapture` (requires a Unix PTY; set
  `JIG_ALLOW_PTY_TEST_SKIP=1` only when the environment is intentionally exempt)
- `cargo test --workspace`
- `scripts/jig file-budget audit` (standalone diagnostics; no runs)
- `scripts/jig check repo:source-runtime-check`
- `scripts/jig-dev check contract`
- `scripts/jig-dev check agent-guides`
- `scripts/jig-dev check agent-map`
