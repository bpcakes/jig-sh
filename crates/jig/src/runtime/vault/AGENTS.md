# Vault runtime guide

## Purpose

Own vault scope, passphrase capture, raw-output dispatch, lifecycle, and the CLI-owned Vault TUI adapter. These rules also cover the sibling [vault.rs](../vault.rs), [vault_env.rs](../vault_env.rs), and vault dispatch in [runtime.rs](../../runtime.rs); they do not apply to unrelated runtime modules.

## Key entrypoints

- [vault.rs](../vault.rs): raw and structured command dispatch.
- [lifecycle.rs](lifecycle.rs): passphrase and backup lifecycle.
- [tui.rs](tui.rs): fixed-scope backend adapter.

## Edit here for X

- Change scope, environment capture, and core calls here.
- Change storage/broker internals in [jig-vault](../../../../jig-vault/AGENTS.md).
- Change terminal forms/navigation in [jig-vault-tui](../../../../jig-vault-tui/AGENTS.md).

## Invariants

- Vault references stay project-relative as `jig://ITEM/FIELD`; repository scope, `--global`, or `--home` selects the vault and a reference must never override that selection.
- Validate vault raw input, import sources/destinations, and lifecycle paths before passphrase capture. Revealed values and transparent child output must bypass structured emitters, JSON, MCP, and receipts; errors and recovery commands must remain value-free.
- Keep `vault exec` as transparent inherited-stdin/environment streaming with exact child status, and keep the compatible `vault run` broker constrained, buffered, capped, timed, and process-tree-owned. Successful vault capture and every spawned resolver/child must strip both reserved passphrase variables.
- Backup restore must use the static absent-target path; it may prepare missing private parent directories, but must never resolve or create the selected vault home before restore preflight and installation.
- The Vault TUI fixes one resolved scope for its lifetime, retains only a process-local credential in the CLI-owned backend, and must join its sole action worker before lock or terminal restoration. TUI action results and ordinary Ratatui frames remain metadata-only; private export and transient Peek consume plaintext only in their immediate hardened/terminal-safe sinks and never return it to the model.

## Common commands

Run from the repository root:

- `cargo build -p jig-sh --bin jig`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check source-vault-test`
