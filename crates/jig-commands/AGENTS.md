# jig-commands crate guide

## Purpose

`crates/jig-commands` is Jig's command registry: every top-level CLI command with its help category, display order, and generated-launcher scope, plus the contract tool kinds and runtime subcommand names that the CLI, bootstrap, and policy checks share.

## Key entrypoints

- `src/root_commands.rs`: the `root_commands!` table, `LauncherScope`, and categorized help.
- `src/tool_defs.rs`: contract tool kinds, `cli_command` labels, and execution-tool predicates.

## Edit here for X

- Add a top-level command: declare it once in the `root_commands!` table, then follow the remaining steps in [the jig crate guide](../jig/AGENTS.md).
- Change which tools count as execution tools: `src/tool_defs.rs`.

## Invariants

- A top-level command's name and launcher scope are declared only in `src/root_commands.rs`; never repeat a root command name as a string elsewhere.
- The generated launcher's command lists are regenerated from this registry, never hand-edited: `JIG_REFRESH_LAUNCHER_COMMAND_LISTS=1 cargo test -p jig-sh --lib generated_launcher_command_lists`.
- `launcher_subcommands` and `LEGACY` exist only under `cfg(test)` or the `test-support` feature, which dependents enable from `[dev-dependencies]` only.

## Common commands

- `cargo test -p jig-commands`
- `cargo test -p jig-sh --lib generated_launcher_command_lists`
