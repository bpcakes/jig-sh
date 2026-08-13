# Vault TUI crate guide

## Purpose

`crates/jig-vault-tui` owns the keyboard-first, full-screen Vault presentation. The matching `jig-sh` CLI supplies a fixed-scope backend; this crate does not resolve repositories, inspect process environment variables, invoke 1Password, or emit JSON.

## Key entrypoints

- `src/lib.rs`: same-release metadata-only backend boundary and public runner.
- `src/model.rs`: screen states, exact selection identity, filtering, and form transitions.
- `src/render.rs`: responsive Ratatui presentation; render data must remain metadata-only.
- `src/runtime.rs`: terminal event loop, action worker ownership, and direct controlled-output coordination.
- `src/secret_input.rs`: bounded zeroizing protected input.
- `src/tools.rs`: lifecycle-tool palette, protected forms, and import/restore confirmations.

## Edit here for X

- Change Vault keyboard behavior or forms: `src/model.rs` and `src/runtime.rs`.
- Change wide/compact layouts or help text: `src/render.rs`.
- Change CLI/domain integration: `src/lib.rs` plus the adapter in `crates/jig/src/runtime/vault/tui.rs`.
- Change generic terminal lifecycle: `crates/jig-tui`.

## Invariants

- Never place plaintext vault values or passphrases in model strings, Ratatui buffers, errors, logs, debug output, or action results.
- Keep exact `VaultReference` and legacy-name identities separate from sanitized display text.
- Protected inputs use `SecretInput`; they are neither cloned nor formatted as plaintext.
- Protected file input accepts only a bounded, non-symlink regular file and preserves exact bytes without routing them through text metadata buffers.
- Import dry-run and preview paths resolve no values; a commit requires the separate exact `IMPORT` confirmation and rechecks current collisions and destination state.
- Restore is offered only for an absent target and returns through the ordinary locked/unlock flow; passphrase rotation replaces the session credential only after the atomic core change succeeds.
- At most one backend worker may exist. Join a non-cancellable mutation before terminal restoration.
- Scope is fixed for the session and stays visible.
- Lock drops credentials, snapshots, and all pending protected inputs.
- Peek and export consume plaintext inside an immediate caller-selected sink; plaintext never returns through `VaultActionResult`.

## Common commands

- `cargo test -p jig-vault-tui`
- `cargo clippy -p jig-vault-tui --all-targets -- -D warnings`
