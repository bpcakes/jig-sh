# jig-owned-process crate guide

## Purpose

`crates/jig-owned-process` owns generic, bounded child-process execution and fail-closed process-tree cleanup for the Jig runtime.

## Key entrypoints

- `src/lib.rs`: public facade used by other Jig crates.
- `src/process.rs`: bounded output, cancellation, timeout, and platform process-tree supervision.
- `src/process/interaction.rs`: cooperative stdin/stdout interaction with an owned child process.

## Edit here for X

- Change checked command-output helpers: `src/process.rs`.
- Change process-tree identity, waiting, or cleanup: `src/process.rs`.
- Change long-lived child protocol interaction: `src/process/interaction.rs`.

## Invariants

- Establish a verifiable process-tree identity before starting child work and retain the direct-child identity until descendant cleanup is confirmed.
- Keep one absolute cleanup deadline across normal, error, and drop paths; fail closed when cleanup or bounded output completion cannot be proved.
- Never signal a recycled numeric PID or process group after identity loss or reap.
- Keep this crate independent from repository context, state, CLI, MCP, templates, proxy routing, and vault secret handling.
- Do not replace the specialized process ownership in `jig-dev-proxy` or `jig-vault`; those crates have additional route and secret invariants.

## Common commands

- `cargo test -p jig-owned-process`
- `cargo clippy -p jig-owned-process --all-targets -- -D warnings`
- `cargo test -p jig-sh`
