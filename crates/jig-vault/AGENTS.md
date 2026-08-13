# jig-vault crate guide

## Purpose

`crates/jig-vault` contains the local encrypted vault, redaction, audit, and brokered child-process primitives used by the Jig runtime. It owns machine-local secret state and must keep plaintext values out of repository state, MCP results, and command receipts.

## Key entrypoints

- `src/lib.rs`: public crate API.
- `src/broker.rs`: brokered run orchestration across vault unlock, audit, secret resolution, and child execution.
- `src/error.rs`: crate-owned error type and public result boundary.
- `src/secret.rs`: public secret byte wrapper that hides zeroization storage details.
- `src/types.rs`: validated domain names used across the public API.
- `src/store.rs`: vault home resolution, filesystem hardening, locks, and atomic file operations.
- `src/crypto.rs`: Argon2id key derivation and XChaCha20-Poly1305 helpers.
- `src/format.rs`: encrypted vault file format, serialized state, and AEAD associated data.
- `src/vault.rs`: public vault facade, unlocked handle internals, and audited secret CRUD.
- `src/redact.rs`: output redaction for raw and encoded secret forms.
- `src/run.rs`: child process execution with resolved secrets, cleaned environment, and redacted output.
- `src/audit.rs`: local tamper-evident audit JSONL records.

## Edit here for X

- Change vault file layout or KDF/AEAD behavior: `src/crypto.rs`, `src/format.rs`, and `src/vault.rs`.
- Change private local state rules: `src/store.rs`.
- Change public secret byte handling: `src/secret.rs`.
- Change redaction coverage: `src/redact.rs`.
- Change brokered run authorization/audit orchestration: `src/broker.rs`.
- Change child-process secret delivery after resolution: `src/run.rs`.
- Change audit record shape: `src/audit.rs`.
- Change metadata snapshots, verified activity projection, or atomic field/item transformations: `src/vault.rs` and `src/audit.rs`.
- Add passphrase rotation or recovery flows: `src/crypto.rs`, `src/format.rs`, and `src/vault.rs`; v1 intentionally has no passphrase-change API.

## Invariants

- Never store plaintext secrets outside encrypted vault state.
- Never return plaintext secret values from public metadata/listing APIs.
- `VaultSnapshot` must keep canonical fields disjoint from unrepresentable legacy entries and verify the audit chain under the same lock before returning metadata. `VaultActivityRecord` must project only action-specific safe metadata after complete chain verification; never expose arbitrary audit detail JSON through it.
- Do not expose plaintext secret values through errors, logs, audit details, runtime JSON, or `Debug` output.
- Canonical references are contextual `jig://ITEM/FIELD` names. Scope selection belongs to the caller; do not add repository identity or cross-vault routing to a reference.
- Version 1 remains readable with every value treated as concealed. Version 2 adds encrypted handling kinds; require explicit one-way migration before field mutation, import, passphrase change, or backup, and keep older readers failing closed on version 2.
- Concealed and text fields are encrypted identically. Only concealed values generate output-redaction patterns; text never means unencrypted persistence.
- Public reveal/injection operations must consume directly into an immediate caller-selected sink and finish their lifecycle audit. Do not expose abandonable prepared reveals or plaintext accessors.
- Transparent exec inherits ordinary stdin/environment, streams independently redacted byte output, and preserves normal child status; it must not inherit the constrained broker's timeout, output cap, or cleaned-environment contract. Keep brokered run behavior compatible.
- Backup envelopes keep vault and audit bytes encrypted, and restore installs only into a proven absent home through private sibling staging; no restore path may resolve or overwrite an existing target.
- `SecretBytes::extend_from_slice` must remain non-growing; callers should preallocate to their hard cap before reading secret-bearing streams.
- Authenticate vault header bytes as AEAD associated data and include a payload role so wrapped-key and state ciphertexts do not share an AEAD context.
- Keep vault state outside `.agent/state`.
- Secret names are operator metadata, not secret material. They may appear in audit details, may contain path-shaped labels like `/` and `.`, and must never be treated as filesystem-safe path components without a separate encoding/newtype.
- New vault passphrases must remain at least 12 bytes; existing vault unlocks must not impose a stricter retroactive floor without migration.
- Use private filesystem permissions, symlink refusal, locks, and atomic writes for local state.
- Filesystem hardening assumes the vault parent is controlled by the same local user; same-user directory-entry races are mitigated but not a full OS isolation boundary.
- Treat redaction as a backup control, not as the core security boundary.
- Verify the audit chain before appending new audit events.
- Vault mutations append audit intent before saving the new state; crashes may leave audit leading state, but state should not lead audit.
- Field kind changes, field/item renames, item removal, and legacy conversion must remain single-lock atomic mutations. They must never be implemented by exposing plaintext or composing separate public read/remove/set calls.
- The local HMAC audit chain detects edited records and broken links, but deletion, truncation, or rollback still requires external checkpoints or backups to prove.
- Brokered env injection must not override the cleaned child process' preserved environment allowlist, such as `PATH`, `HOME`, `TMPDIR`, and locale variables.
- Child-process environment injection necessarily gives `std::process::Command` a non-zeroizable copy of each injected secret; prefer future OS-specific delivery primitives for stronger isolation.
- Brokered child execution uses a 30-minute wall-clock timeout and capped pipe capture, so stdin is closed/null. Establish the platform's retained process-tree identity before the child can run, observe a Unix leader without consuming its wait status, classify only `CLD_EXITED`/`CLD_KILLED`/`CLD_DUMPED` as terminal, terminate the owned tree before reap, and never signal a numeric PID or PGID after that identity is lost. While the exact unreaped leader pins the PGID generation, forced confirmation must re-send group `SIGKILL` before every membership proof so a concurrently exposed member cannot outlive a one-shot signal. Linux procfs confirmation must check the fixed cleanup deadline around every signal, enumeration, stat read, fallback membership probe, and before accepting either a live or empty result, with a re-signal between its two required empty scans. `ESRCH` and Darwin `EPERM` alone never prove cleanup: macOS acceptance still requires a fresh exact terminal leader observation plus one atomic group-membership snapshot containing that leader and no second member. Successful output requires complete EOF on both streams; final drain/drop must stay bounded even when a descendant deliberately escapes and retains a pipe. Temporary read buffers and final captured bytes are zeroized, but redaction itself can allocate intermediate `String`/`Vec<u8>` copies that are not zeroized; it is an output safety net, not an in-memory secrecy boundary.
- Brokered run open, `BrokeredRunStart` audit append, and secret resolution must stay serialized under the vault lock. `BrokeredRunStart` is written before secret references resolve and before the child command starts. Resolve failures append `BrokeredRunFailed` after the start event. If the `jig` process is killed after start and before completion, the audit log can contain a start event without a finish/failure event. Other audited operations may interleave before the finish/failure event, so consumers must correlate brokered run events by `run_id`, not adjacency. Failure events include the failure stage.
- Non-interactive unlock reads the passphrase from process environment and clears both reserved passphrase variables after successful capture; terminal CLI use may prompt instead. This depends on the vault CLI path reading or prompting before starting background threads, and environment clearing is best-effort process hygiene rather than guaranteed overwriting of libc/shell environment backing storage.
- JSON serialization in `OpenVault::save_unlocked` can allocate internal serde scratch buffers containing base64 secret material before the final serialized state buffer is wrapped in `Zeroizing`.
- If `init` appends the first audit record but crashes or fails before writing `vault.json`, the next `init` fails closed on stale `audit.jsonl`; manual recovery is to inspect the vault home and remove the stale audit file before retrying.
- Audit MAC input canonicalizes JSON object keys before serialization; preserve that canonicalization if serde_json features or audit detail shapes change.
- Updating a secret preserves `created_at_ms`; keep that stable unless the audit model deliberately changes.
- This crate assumes zeroize is built with its standard allocation support so `String` and `Vec<u8>` wiping is available.

## Common commands

- `cargo test -p jig-vault`
- `cargo test -p jig-sh`
- `cargo test --workspace`
