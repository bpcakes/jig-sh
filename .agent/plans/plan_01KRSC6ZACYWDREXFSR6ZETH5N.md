# Implement Jig Vault

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

This document follows `.agent/PLANS.md` in this repository. It is self-contained so a future agent or human can restart the work from only this file and the current worktree.

## Purpose / Big Picture

Jig currently turns a repository into an operating environment for coding agents, but secrets still arrive through the caller's normal environment or project files. After this change, a user can create a local encrypted Jig vault, store named secrets outside the repository, list value-free secret metadata, and run a child command with selected secrets injected only into that brokered child process. The command output is redacted before it reaches the terminal, JSON result, or any future receipt path.

This first implementation is deliberately narrow. It builds the hard local vault foundation and one brokered execution path. It does not claim that an arbitrary child process cannot leak a secret after receiving it. That stronger guarantee would require protocol-specific brokering rather than env/file injection.

## Progress

- [x] (2026-05-16T21:48Z) Created structured Jig work plan `plan_01KRSC6ZACYWDREXFSR6ZETH5N` and built `target/debug/jig` for dogfooding.
- [x] (2026-05-16T21:48Z) Wrote the initial architecture and implementation ExecPlan.
- [x] (2026-05-16T22:10Z) Add the `crates/jig-vault` crate and workspace dependencies.
- [x] (2026-05-16T22:10Z) Implement encrypted vault state, local state hardening, atomic writes, and audit records.
- [x] (2026-05-16T22:10Z) Implement redaction primitives and tests for multiple encoded forms.
- [x] (2026-05-16T22:10Z) Add `jig vault` CLI commands in `crates/jig`.
- [x] (2026-05-16T22:10Z) Implement brokered `jig vault run` with a cleaned child environment and redacted output.
- [x] (2026-05-16T22:10Z) Add focused unit and CLI tests.
- [x] (2026-05-16T22:10Z) Run a real CLI smoke test proving `vault run` redacts an injected secret.
- [x] (2026-05-16T22:35Z) Run formatting, crate tests, workspace tests, clippy, Jig work gates, `make test`, and CLI smoke tests.
- [x] (2026-05-16T22:35Z) Run comprehensive Claude Code plus native review on all working changes.
- [x] (2026-05-16T22:35Z) Fix actionable review findings: zeroization gaps, value-free listing allocation, canonical AAD, KDF bounds, set/remove audit locking, stdin semantics, redactor coverage, and child exit propagation.

## Surprises & Discoveries

- Observation: `scripts/jig dev` and `scripts/jig proxy run` currently inherit the caller environment for app children.
  Evidence: `crates/jig-dev-proxy/src/processes.rs` documents this in `spawn_child`, where the child receives inherited env plus `PORT` and `HOST`.

- Observation: command-backed tools currently capture full stdout and stderr before receipt truncation.
  Evidence: `crates/jig/src/runtime.rs` converts command output to strings, returns `result.stdout` and `result.stderr`, and passes them into receipt recording.

## Decision Log

- Decision: Build `crates/jig-vault` as a dedicated crate rather than adding vault internals directly to `crates/jig`.
  Rationale: Vault storage, cryptography, redaction, audit, and process delivery have different invariants from the CLI/runtime dispatcher. A crate boundary keeps security-sensitive code easier to review and test.
  Date/Author: 2026-05-16 / Codex.

- Decision: Store vault state under `~/.jig/vault` or `JIG_VAULT_HOME`, never under `.agent/state`.
  Rationale: `.agent/state` is append-only project memory and may be committed, copied, or shared. Vault state is machine-local sensitive data and needs stricter filesystem rules.
  Date/Author: 2026-05-16 / Codex.

- Decision: Use Argon2id for passphrase-to-key derivation and XChaCha20-Poly1305 for authenticated encryption.
  Rationale: Argon2id is the current recommended memory-hard password KDF. XChaCha20-Poly1305 provides authenticated encryption with a large random nonce, reducing nonce-management risk compared with AES-GCM for a local file format.
  Date/Author: 2026-05-16 / Codex.

- Decision: Bind vault header fields as AEAD associated data.
  Rationale: The file format version, KDF parameters, vault id, and algorithm identifiers must be authenticated along with ciphertext so tampering fails cryptographically rather than being detected only indirectly.
  Date/Author: 2026-05-16 / Codex.

- Decision: Implement `jig vault run` as the first brokered execution path and keep generic daemon/MCP secret tools out of v1.
  Rationale: `vault run` demonstrates the essential secure path with less lifecycle risk. A daemon with cross-platform peer identity can be added after the vault format, redactor, and process-injection semantics are proven.
  Date/Author: 2026-05-16 / Codex.

## Outcomes & Retrospective

Implemented `crates/jig-vault` as a dedicated vault crate and integrated it through explicit `jig vault` runtime commands. The vault now supports init, status, byte-exact stdin secret set, value-free list, remove, and brokered `run` with selected UTF-8 env injection. State lives under `~/.jig/vault` or `JIG_VAULT_HOME`, outside `.agent/state`.

The final implementation uses Argon2id with bounded header parameters, XChaCha20-Poly1305, canonical header AAD, a random DEK wrapped by the passphrase-derived key, private local filesystem rules, atomic writes, advisory locks, tamper-evident audit records, and redaction for raw, base64 standard/padded variants, base64 URL variants, hex, percent-encoded, JSON-escaped, and Unicode-escaped forms. Secret metadata listing no longer decodes secret values.

Comprehensive review produced actionable findings and they were fixed in the same loop. Remaining limitations are deliberate v1 boundaries rather than hidden issues: env injection is not a sandbox once a child receives a secret, output is buffered before redaction rather than streamed, and the local audit chain is tamper-evident but not tamper-proof without external anchoring.

Final validation passed: `cargo fmt --all -- --check`, `cargo test -p jig-vault`, `cargo test -p jig-sh --lib`, `cargo test --workspace`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, real CLI smoke tests including redaction and child-status propagation, `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KRSC6ZACYWDREXFSR6ZETH5N`, `JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KRSC6ZACYWDREXFSR6ZETH5N`, and `make test`.

## Context and Orientation

The repository is a Rust workspace with two existing crates:

`crates/jig` is the repo-local CLI and MCP runtime. Its key files are `src/cli.rs` for Clap command definitions, `src/runtime.rs` for command dispatch and tool execution, `src/mcp.rs` for the MCP stdio server, and `src/state.rs` plus `src/state/` for append-only project memory.

`crates/jig-dev-proxy` owns local development process supervision, route state, TLS certificates, and hardening patterns for private machine-local state. The vault should borrow its defensive filesystem ideas: refuse symlinked state directories, require private permissions, write through temp files with atomic replace, and use advisory locks. The vault must not reuse proxy state directly because proxy state is mutable runtime data, while vault state is sensitive encrypted user data.

The new crate `crates/jig-vault` will own all vault storage and redaction logic. It should expose a small API to `crates/jig`:

- Resolve and prepare a local vault home.
- Create an encrypted vault.
- Open an encrypted vault with a passphrase.
- Add, list, and remove named secrets.
- Build a redactor from selected secret values.
- Run one brokered child process with a cleaned environment and selected secret env vars.
- Append local audit events.

A “vault” means one encrypted local file holding secret records and value-free metadata. A “secret record” means a named byte value plus metadata such as creation time and updated time. A “brokered child process” means a command Jig starts after resolving selected secret values; only that child receives injected values. A “redactor” means a scanner that replaces known secret byte strings and common encodings with fixed markers before output is displayed or persisted.

## Plan of Work

First, add `crates/jig-vault` to the workspace. The crate depends on `anyhow`, `serde`, `serde_json`, `time`, `ulid`, `zeroize`, `base64`, `getrandom`, `fs4`, `libc` on Unix, plus new crypto dependencies: `argon2`, `chacha20poly1305`, `hkdf`, `hmac`, `sha2`, and `secrecy`. If a crate is only needed by `crates/jig`, add it there instead of to the vault crate.

Second, implement local state handling in `crates/jig-vault/src/store.rs`. `VaultStore::resolve` should use `JIG_VAULT_HOME` when set, otherwise `dirs::home_dir().join(".jig/vault")`. It should create a private directory, refuse symlinks, refuse shared-writable ancestors on Unix, and expose paths for `vault.json`, `vault.lock`, and `audit.jsonl`. Use `fs4` locks for mutating operations. Writes must go to an exclusive temp file in the same directory and then atomically rename.

Third, implement the encrypted file format in `crates/jig-vault/src/crypto.rs` and `src/vault.rs`. The persisted file is JSON with a plaintext header, a wrapped data-encryption key, and encrypted state. The header includes magic string `jig-vault`, format version `1`, vault id, KDF name and parameters, AEAD name, salt, and creation timestamp. The passphrase derives a 32-byte wrapping key with Argon2id. A random 32-byte data-encryption key encrypts the state with XChaCha20-Poly1305. Both the wrapped key and encrypted state authenticate the header bytes as associated data. Plaintext state contains secret metadata and values, but it only exists decrypted in memory while the command is running.

Fourth, implement local audit in `src/audit.rs`. Each append-only record includes an event id, timestamp, action, previous MAC, details with no secret values, and MAC. Derive an audit key from the data-encryption key with HKDF-SHA256. The MAC chain is tamper-evident but not tamper-proof; document that local rollback can still defeat it without external anchoring.

Fifth, implement redaction in `src/redact.rs`. Given secret byte values, build needles for at least these forms in v1: raw UTF-8 when valid, base64 standard, base64 URL-safe without padding, lower hex, upper hex, percent-encoded, JSON escaped, and Unicode escaped ASCII. Replace matches with typed markers while preserving line breaks. This redactor is a backup control, not the primary boundary. Tests must prove no known secret appears in redacted output for these encodings.

Sixth, add a `jig vault` CLI surface in `crates/jig/src/cli.rs` and dispatch in `crates/jig/src/runtime.rs`. Commands:

- `jig vault init`
- `jig vault status`
- `jig vault secret list`
- `jig vault secret set NAME --value-stdin`
- `jig vault secret remove NAME`
- `jig vault run --env VAR=SECRET -- COMMAND [ARGS...]`

For implementation practicality and testability, the passphrase source for v1 is `JIG_VAULT_PASSPHRASE`. If it is missing and a terminal prompt helper is not added, the CLI must fail with a clear message instead of accepting a command-line passphrase. Command-line passphrases are deliberately not supported because they leak through shell history and process lists.

Seventh, implement `vault run` as a brokered child path. It opens the vault, resolves the requested secret names, builds a redactor from the selected secret values, starts the child with a cleaned environment plus a minimal safe allowlist needed for command execution, injects requested env vars, captures stdout and stderr, redacts them, prints or returns redacted JSON, and records audit events for grant-like use. It must not return raw secret values in JSON.

Eighth, add tests. `jig-vault` unit tests should cover successful create/open, wrong passphrase failure, ciphertext tamper failure, header tamper failure, secret CRUD, audit append, and redaction. `crates/jig` CLI tests should cover parsing the new commands. Runtime tests should cover a temp vault, `vault run --env TOKEN=api_token -- sh -c 'printf %s \"$TOKEN\"'`, and assert the command result contains a redaction marker rather than the secret.

Ninth, run validation. At minimum run:

    cargo fmt --all -- --check
    cargo test -p jig-vault
    cargo test -p jig-sh
    cargo test --workspace
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KRSC6ZACYWDREXFSR6ZETH5N
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KRSC6ZACYWDREXFSR6ZETH5N

Then run comprehensive review using the repository's `comprehensive-review` skill. If it reports actionable findings, update this plan, fix them, rerun relevant tests, and repeat review. If it reports no actionable findings, update `Outcomes & Retrospective` and mark the goal complete.

## Concrete Steps

Work from `<private-path>`.

Build the current Jig binary before dogfooding runtime commands:

    cargo build -p jig-sh --bin jig

Create and maintain this work plan:

    JIG_DEV_BIN=target/debug/jig scripts/jig work start --title "Implement Jig Vault" --body "..." --print-plan-id

After adding the crate, verify the workspace sees it:

    cargo metadata --format-version 1 --no-deps

After implementing the vault crate, prove the local API before CLI integration:

    cargo test -p jig-vault

After CLI integration, prove the command path with a temp vault home:

    tmp="$(mktemp -d)"
    export JIG_VAULT_HOME="$tmp/vault"
    export JIG_VAULT_PASSPHRASE="correct horse battery staple"
    target/debug/jig vault init
    printf 'secret-value-123' | target/debug/jig vault secret set api_token --value-stdin
    target/debug/jig vault secret list
    target/debug/jig vault run --env TOKEN=api_token -- sh -c 'printf "%s\n" "$TOKEN"'

Expected behavior: `secret list` shows `api_token` but not `secret-value-123`; `vault run` exits successfully and prints a redaction marker rather than `secret-value-123`.

## Validation and Acceptance

Acceptance is behavior-based:

Running `jig vault init` with `JIG_VAULT_PASSPHRASE` creates a private vault directory and encrypted vault file. Running it twice fails or reports the vault already exists without overwriting secrets.

Running `printf value | jig vault secret set name --value-stdin` stores the secret. Running `jig vault secret list` shows the secret name and metadata but not the value. Running with a wrong passphrase fails with an authentication/decryption error and never creates a plaintext fallback.

Running `jig vault run --env TOKEN=name -- sh -c 'printf "%s" "$TOKEN"'` executes the child and returns output with a redaction marker. The raw secret must not appear in stdout, stderr, JSON response, audit JSONL, or test failure messages.

Tampering with any header field covered by associated data or any ciphertext bytes causes open to fail. Tests should mutate one byte and assert failure.

All relevant local checks must pass. For backend changes, finish with `scripts/jig check test` and `make test` if the repository gates still require them after implementation.

## Idempotence and Recovery

The implementation is additive. Re-running tests should use temporary vault homes and must not touch the developer's real `~/.jig/vault`. CLI examples use `JIG_VAULT_HOME` pointing to a temp directory.

If vault creation fails after writing a temp file, retrying should either create a valid vault or report the existing valid vault. Atomic writes must not leave partial `vault.json` contents. Lock files are machine-local runtime artifacts and can be removed only when no Jig process is running.

If an audit append fails after a brokered command ran, `vault run` should report the audit failure and avoid claiming a fully recorded brokered use. It should not rerun the child automatically because that could duplicate side effects.

If comprehensive review finds an issue, update `Progress`, add a `Surprises & Discoveries` or `Decision Log` entry as appropriate, fix the issue, and rerun the focused tests before another review.

## Artifacts and Notes

Initial research anchors:

- Argon2id is the chosen KDF. RFC 9106 recommends Argon2id with 64 MiB and 3 iterations for memory-constrained environments; OWASP's floor is lower but still points to Argon2id.
- XChaCha20-Poly1305 is chosen for AEAD because its large nonce is friendlier for random nonces in a local file format.
- Local audit is tamper-evident, not tamper-proof.
- Env injection is not a hard containment boundary once a child process receives the secret.

## Interfaces and Dependencies

`crates/jig-vault/src/lib.rs` should expose:

    pub struct VaultStore;
    pub struct OpenVault;
    pub struct SecretRecord;
    pub struct Redactor;
    pub struct RunRequest;
    pub struct RunOutput;

    impl VaultStore {
        pub fn resolve(explicit_home: Option<PathBuf>) -> Result<Self>;
        pub fn exists(&self) -> bool;
        pub fn init(&self, passphrase: &SecretString) -> Result<()>;
        pub fn open(&self, passphrase: &SecretString) -> Result<OpenVault>;
    }

    impl OpenVault {
        pub fn list(&self) -> Vec<SecretRecord>;
        pub fn set_secret(&mut self, name: &str, value: SecretVec) -> Result<()>;
        pub fn remove_secret(&mut self, name: &str) -> Result<bool>;
        pub fn save(&self, store: &VaultStore) -> Result<()>;
        pub fn secret_value(&self, name: &str) -> Result<SecretVec>;
    }

    impl Redactor {
        pub fn from_secret_values(values: &[SecretVec]) -> Self;
        pub fn redact_str(&self, input: &str) -> String;
        pub fn redact_bytes_lossy(&self, input: &[u8]) -> String;
    }

`crates/jig/src/cli.rs` should define a `VaultCommand` enum under the top-level `CommandKind`.

`crates/jig/src/runtime.rs` should dispatch `CommandKind::Vault(command)` to a new `runtime/vault.rs` module so runtime dispatch remains thin.

The top-level `.jig.toml` and `.agent/jig-contract.json` should not gain vault tools in v1. Vault commands are runtime-owned, like `dev`, `proxy`, and `work`, because they manage local machine state rather than stable project checks.

Revision note, 2026-05-16 / Codex: initial plan created to guide the full implementation and review loop for Jig Vault.
