# Vault runtime guide

## Purpose

Own vault scope, passphrase capture, raw-output dispatch, lifecycle, and the CLI-owned Vault TUI adapter. These rules also cover the sibling [vault.rs](../vault.rs), [vault_env.rs](../vault_env.rs), [vault_withholding.rs](../vault_withholding.rs), vault dispatch in [runtime.rs](../../runtime.rs), and the CLI startup boundary in [vault_environment.rs](../../cli/run/vault_environment.rs); they do not apply to unrelated runtime modules.

## Key entrypoints

- [vault.rs](../vault.rs): raw and structured command dispatch.
- [lifecycle.rs](lifecycle.rs): passphrase and backup lifecycle.
- [tui.rs](tui.rs): fixed-scope backend adapter.
- [vault_withholding.rs](../vault_withholding.rs): startup passphrase withholding and the withheld-passphrase note.

## Edit here for X

- Change scope, environment capture, and core calls here.
- Change storage/broker internals in [jig-vault](../../../../jig-vault/AGENTS.md).
- Change terminal forms/navigation in [jig-vault-tui](../../../../jig-vault-tui/AGENTS.md).

## Invariants

- Vault references stay project-relative as `jig://ITEM/FIELD`; repository scope, `--global`, or `--home` selects the vault and a reference must never override that selection.
- Validate vault raw input, `vault run` mappings, import sources/destinations, and lifecycle paths before passphrase capture. Revealed values and transparent child output must bypass structured emitters, JSON and run or loop records; errors and recovery commands must remain value-free.
- Passphrase-unavailable diagnostics, including init/adopt bootstrap, append the shared `VAULT_PASSPHRASE_OPERATOR_GUIDANCE` from [runtime.rs](../../runtime.rs), through `vault_passphrase_operator_guidance` where a missing passphrase may have been withheld: route passphrase entry to the operator's terminal or operator-managed automation, and never tell callers to choose, request, print, store, export, or set a passphrase themselves.
- Keep `vault exec` as transparent inherited-stdin/environment streaming with exact child status, and keep the compatible `vault run` broker constrained, buffered, capped, timed, and process-tree-owned. Successful vault capture and every spawned resolver/child must strip both reserved passphrase variables.
- Only `vault` commands and vault-requesting `init`/`adopt --write` keep the reserved passphrase variables past CLI startup. [vault_environment.rs](../../cli/run/vault_environment.rs) withholds both from every other command right after parsing, through an exhaustive `CommandKind` match, and sets the non-secret `JIG_VAULT_PASSPHRASE_WITHHELD=1` marker only when it removed one; bootstrap vault preparation clears both before rendering. Code before a capture point (launcher validation including contract validation, repository-scope application, and vault preflight for `vault` commands; init destination and interaction preflight) must not start child processes. The withheld note is value-free and ignored while a passphrase is present.
- Backup restore must use the static absent-target path; it may prepare missing private parent directories, but must never resolve or create the selected vault home before restore preflight and installation.
- The Vault TUI fixes one resolved scope for its lifetime, retains only a process-local credential in the CLI-owned backend, and must join its sole action worker before lock or terminal restoration. TUI action results and ordinary Ratatui frames remain metadata-only; private export and transient Peek consume plaintext only in their immediate hardened/terminal-safe sinks and never return it to the model.

## Common commands

Run from the repository root:

- `scripts/jig-dev check source-vault-test`
