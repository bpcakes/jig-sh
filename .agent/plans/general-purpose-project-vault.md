# Make Jig Vault a project-scoped 1Password CLI replacement

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current as implementation proceeds, and maintain it in accordance with `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

This ExecPlan turns Jig Vault from a small secret store plus constrained broker into a general local command-line vault for a single project. After this work, a developer can keep a complete environment bundle encrypted in the current repository's vault, refer to fields as `jig://Production/RESTIC_PASSWORD`, reveal a selected field, render a template, or run an ordinary command from a reference-bearing dotenv file. A one-time importer can convert a 1Password-backed dotenv file without printing values. The plan also adds passphrase rotation and encrypted backup/restore so the resulting vault is operable rather than merely writable.

The project is implicit in every reference. `jig://Production/RESTIC_PASSWORD` means item `Production`, field `RESTIC_PASSWORD`, in the vault selected by the current repository, `--global`, or an explicit `--home`. There is deliberately no `jig://IdentityPro/Production/RESTIC_PASSWORD` form and no cross-project reference syntax in this scope. IdentityPro is an end-to-end acceptance example, not a special data model.

This is local CLI replacement scope, not a clone of the whole 1Password product. It covers the workflows analogous to `op read`, `op inject`, and `op run --env-file`, along with field management, one-time import, passphrase change, and encrypted backup/restore. It excludes browser/mobile autofill, remote synchronization, team sharing, TOTP generation, hosted audit, enterprise access control, attachments, arbitrary 1Password item-schema reproduction, clipboard integration, and a long-lived unlock daemon.

## Progress

- [x] (2026-08-09) Read the repository, crate, and ExecPlan guidance and inspect the current vault format, scope resolution, CLI/runtime boundaries, audit model, redactor, and brokered process behavior.
- [x] (2026-08-09) Incorporate the adversarial review and the user's simplification: project context is implicit, all field values remain encrypted, and IdentityPro is only an acceptance example.
- [x] (2026-08-09) Open structured Jig work `plan_01KZKVRN21MBBY6YSJC88D603R`, whose body points to this authoritative ExecPlan.
- [x] (2026-08-09) Record the baseline: format/check and all 100 `jig-vault` tests passed before source edits. The long pre-edit `jig-sh` run was stopped after hundreds of tests to unblock implementation; the immediately preceding work receipt was green, and the complete post-milestone suite later passed.
- [x] (2026-08-09) Milestone 1: commit `cd229af` adds canonical project-local references, encrypted field kinds, vault envelope v2, explicit v1-to-v2 migration, bounded atomic field batches, field CLI, and a static legacy fixture.
- [x] (2026-08-09) Milestone 2: commit `d80f496` adds controlled exact-byte `read` and bounded template `inject`, lifecycle audit events, raw CLI dispatch, and hardened Unix file sinks.
- [ ] Milestone 3: add transparent `exec --env-file` with inherited process behavior and streaming redaction of concealed fields only.
- [ ] Milestone 4: add atomic dotenv batch updates and the one-time 1Password dotenv importer.
- [ ] Milestone 5: add full-reseal passphrase rotation and encrypted backup/restore.
- [ ] Milestone 6: document the cutover, prove the generic examples and IdentityPro acceptance example, and keep the old `secret` and constrained `run` commands working.
- [ ] Run focused, crate, workspace, MSRV, platform-sensitive, and Jig harness validation; update this living plan with outcomes and remaining CI-only coverage.

## Surprises & Discoveries

The current encrypted envelope is version 1. `crates/jig-vault/src/format.rs` stores a `VaultState` whose `secrets` map contains values and timestamps but no field-handling metadata. The complete header, including its version, KDF parameters, and salt, is authenticated as associated data for both the wrapped data-encryption key and encrypted state. Consequently, changing the envelope version or passphrase metadata requires re-encrypting both payloads with fresh nonces; changing only the wrapped key is incorrect.

The current repository vault is already project-scoped in `crates/jig/src/runtime/vault.rs`. Its physical home is derived from the canonical checkout path and `[vault].scope_id`, while `--global` and `--home` are explicit alternatives. This plan keeps that trust boundary. A reference never embeds the repository name, and moving to another checkout is performed explicitly with encrypted backup/restore rather than silently making two paths share a vault.

The existing `jig vault run` is intentionally a constrained agent boundary. It clears the environment, closes stdin, buffers and caps stdout and stderr at 1 MiB each, imposes a 30-minute timeout, and owns and cleans up the child process tree. Those semantics are unsafe for an ordinary long-running or mutating developer command because a cap or timeout can terminate it. The new `exec` path must therefore be separate; `run` remains unchanged for agent-controlled execution.

The current redactor builds raw and encoded needles for every supplied secret and assumes bounded output. Treating encrypted IDs, URLs, booleans, or other context values as redaction needles would replace common strings such as `false` in unrelated output. The v2 field model therefore distinguishes `concealed` and `text` handling. Both kinds are encrypted identically. Only concealed values contribute redaction needles; `text` means display/output handling, never unencrypted storage.

The audit chain is local and keyed from the vault's data-encryption key. It detects edited records and broken links, but not deletion or rollback without an external checkpoint. Appends currently re-verify the full chain and are not designed for high-volume hosted auditing. This plan adds local action records for reveal, injection, execution, migration, import, rekey, backup, and restore without claiming server-grade audit or redesigning audit rotation.

The `jig-sh` crate depends on `dotenvy` 0.15.7, but that parser performs `$VAR` and `${VAR}` substitution from the ambient environment. It therefore cannot implement the fail-closed, no-interpolation grammar required for vault inputs. Milestones 3 and 4 use a small restricted parser instead of `dotenvy`; keeping the existing dependency elsewhere in Jig does not make it suitable at this boundary.

The local `op read` interface appends a newline unless passed `--no-newline`. The importer must invoke the exact argv `op read --no-newline REF` without a shell and must not surface raw `op` stderr, because an external resolver can print an unknown value Jig cannot redact safely.

`VaultStore::resolve` creates the target directory and lock, which conflicts with restore's promise never to overwrite a target. Restore needs a non-creating, absent-target preflight plus an atomic no-replace directory installation primitive. Likewise, existing string-oriented atomic vault writes are not a safe implementation for arbitrary binary reveal/template output.

The pre-edit `jig-vault` baseline passed 100 tests. The `jig-sh` test binary was compiled from the pre-edit tree and ran hundreds of its 1,338 tests successfully, but the run was interrupted after resource contention made many unrelated bootstrap tests exceed one minute. The prior completed work receipt was green; only a complete post-change run will be treated as final evidence.

The first Milestone 1 implementation allowed a batch of individually valid 1 MiB fields to write a `vault.json` larger than the store's own 16 MiB read limit. Two independent reviews caught this before commit. State edits now seal and serialize the exact final envelope, validate its final byte length before audit append, then write the same prepared envelope; a universal write-side limit and fault/retry tests prevent recurrence.

The first v1 reader deserialized every state through the v2 DTO before forcing kinds to concealed. That incorrectly rejected stray unknown `kind` data which the shipped v1 serde model would have ignored. A dedicated v1 deserialization DTO now ignores such metadata and maps every entry to concealed, while v2 remains strict. A static fixture generated by committed pre-change binary `6e47705` proves compatibility independently of the new v1 writer.

The first CLI wiring carried field/item inputs as raw strings until after passphrase capture. Clap now parses `VaultReference` and `VaultItem`, and accepts only migration target 2, before any prompt, environment clearing, or vault filesystem access.

The first Milestone 2 seam parsed templates only after passphrase capture, so an unreadable, oversized, or malformed template could consume the environment passphrase before failing. `InjectionTemplate::parse` is now a nonrevealing, bounded, opaque prevalidation step; the CLI completes it before repository scope resolution or passphrase capture, and zeroizes partial input on every read path.

The first Milestone 2 API exposed public prepared-reveal handles. Although their `Drop` implementation correctly avoided filesystem I/O, an ordinary caller could abandon a handle and leave a start event that looked like a process crash. The final API instead exposes four direct vault-to-sink methods. Preparation remains private, the vault lock is released before sink I/O, and every ordinary success or error return attempts a matching terminal event; only panic, abort, or process death can strand a start.

Hardened reveal-file installation is implemented on Unix with owner-only same-directory temporaries, exact byte writes and fsync, parent/leaf symlink refusal, atomic hard-link no-clobber, regular-file-only atomic overwrite, and parent-directory fsync. Non-Unix file sinks compile but fail closed until equivalent DACL, reparse-point, and no-clobber guarantees exist; exact stdout remains portable.

## Decision Log

2026-08-09: References are contextual and have exactly the canonical form `jig://ITEM/FIELD`. `ITEM` and `FIELD` are nonempty ASCII identifiers containing letters, digits, `_`, `-`, or `.`; each is at most 64 bytes, their combined internal `ITEM/FIELD` spelling is at most 128 bytes, and neither segment may be `.` or `..`. Thus two simultaneous 64-byte segments are rejected because their separator would exceed the existing durable `SecretName` boundary. Reject extra path segments, empty segments, percent encoding, queries, fragments, credentials, and ports. This deliberately avoids URI normalization ambiguity. `VaultReference::to_secret_name()` maps the reference to the existing internal path-shaped name `ITEM/FIELD`; the serialized map key remains compatible.

2026-08-09: `jig://ITEM` is a distinct validated `VaultItem`, not a partially parsed `VaultReference`. Field listing and the importer use this type so a one-segment selector never falls back to raw string splitting.

2026-08-09: The public terminology for the new API is field, not secret. `FieldKind::Concealed` is the default and participates in output redaction. `FieldKind::Text` is still encrypted and is suitable for URLs, account IDs, flags, usernames, and other values that belong in the encrypted bundle but should not mask ordinary output. The CLI spells the opt-in as `--text`; it does not use `--plain`, because that could imply unencrypted persistence.

2026-08-09: Persisted handling metadata uses an explicit envelope version 2. New vaults use v2. New code continues to open v1 vaults for status, audit verification, listing, reading, injection, and execution, treating every v1 entry as concealed. A v1 vault must be upgraded with `jig vault migrate --to 2` before field mutation, import, passphrase change, or backup. The upgrade is one-way, audited, and atomic. Older binaries reject v2 instead of opening it and silently dropping field kinds.

2026-08-09: Existing public Rust APIs and `jig vault secret {set,list,remove}` remain available. On v2, secret setters create concealed fields. The existing constrained `jig vault run` remains byte-for-byte compatible and resolves both v1 secrets and v2 fields as concealed values. Removal of the legacy vocabulary or command is not part of this plan.

2026-08-09: `jig vault read REF` writes the exact value to stdout for pipelines. If stdout is a terminal it fails unless `--reveal` is present. `--out-file PATH` is an alternative private, no-symlink, no-overwrite sink; `--overwrite` is an explicit opt-in. `--json` is rejected for raw reveal commands so a secret can never appear in a JSON result or command receipt. Clipboard support is deferred.

2026-08-09: Template injection recognizes only `{{ jig://ITEM/FIELD }}` placeholders, allowing surrounding ASCII whitespace inside the braces. It does not replace bare reference-looking text. `jig vault inject --in TEMPLATE` uses the same terminal and file-sink rules as `read`, audits the referenced field names before revealing bytes, and places a 16 MiB limit on input and rendered output to keep secret-bearing allocations bounded.

2026-08-09: `jig vault exec --env-file FILE -- COMMAND...` is a normal developer process wrapper, not an agent sandbox. It inherits stdin and the ordinary parent environment, applies dotenv assignments in file order with the last duplicate rejected rather than silently winning, removes the captured `JIG_VAULT_PASSPHRASE` and `JIG_VAULT_NEW_PASSPHRASE` from the child, streams stdout and stderr, has no Jig timeout or output cap, and mirrors the child's exit status. It rejects `--json`. The existing `run` command retains its closed stdin, cleaned environment, cap, timeout, and owned-tree cleanup.

2026-08-09: `exec` accepts literal dotenv values for compatibility, but the importer writes references for every assignment, including former literals. This lets a project choose a fully encrypted bundle without forcing every user of `exec` to migrate all nonsecret configuration. Concealed references are masked in streamed child output; text references and literal dotenv values are not masking needles.

2026-08-09: The first importer is intentionally narrow: `jig vault import onepassword --env-file SOURCE --item ITEM --out-env DESTINATION`. Each valid environment variable becomes field `jig://ITEM/VARIABLE`. An `op://...` right-hand side is resolved by direct `op read --no-newline` invocation and stored concealed; a literal right-hand side is stored as encrypted text. The destination contains only `VARIABLE=jig://ITEM/VARIABLE` assignments. Existing target fields or destination files cause a fail-closed error unless the operator explicitly supplies `--replace` and/or `--overwrite`. Dry-run does not call `op` and does not mutate, but it does unlock, verify, and list the target vault so its create/replace report is truthful.

2026-08-09: Passphrase rotation keeps the vault ID, creation time, data-encryption key, state, and audit key, but generates a new salt, current KDF parameters, and fresh nonces. Because the header is associated data, the implementation rewraps the data-encryption key and reseals the state under the new header. It never accepts a passphrase argument; terminal prompts or `JIG_VAULT_PASSPHRASE` plus `JIG_VAULT_NEW_PASSPHRASE` are the supported sources.

2026-08-09: A backup is a separate encrypted envelope containing the exact vault file and audit log. It is encrypted with the current vault passphrase using a fresh backup salt and nonce, written owner-only, and includes no plaintext field values or plaintext audit metadata outside its ciphertext. Restore validates and decrypts into a sibling staging directory, opens the embedded vault, verifies its audit chain, appends a restore record in staging, and atomically installs only when the target vault home is absent. An existing target, even an empty directory, is refused; in-place destructive restore is excluded.

2026-08-09: Project isolation remains path-bound. The repository name is omitted from references, but this plan does not make copied `.jig.toml` files silently share credentials. Encrypted backup/restore is the supported relocation and disaster-recovery mechanism; `--home` remains the explicit expert escape hatch.

## Outcomes & Retrospective

Implementation is active under structured work `plan_01KZKVRN21MBBY6YSJC88D603R`. Milestone 1 completed in commit `cd229af` after two implementation passes and three independent reviews. Milestone 2 completed in commit `d80f496` after an additional adversarial lifecycle pass replaced abandonable public prepared handles with direct sink APIs. Its final evidence was 144 `jig-vault` tests, 1,350 active `jig-sh` unit tests plus all integration suites (including two new binary reveal/injection tests), Rust 1.85 checks for both crates, production-target Jig Clippy, all-target `jig-vault` Clippy, formatting/diff checks, and independent review. The all-target Jig Clippy gate remains blocked only by the pre-existing `needless_return` in `crates/jig/tests/support/mod.rs`; the changed targets introduced no remaining warning. The intended outcome remains a general, local, project-scoped vault CLI with explicit compatibility and recovery behavior, without expanding into a remote password-manager service.

At completion, summarize whether IdentityPro can replace its `op run --env-file` and `op read` use with Jig references while retaining both secret and contextual values in encrypted storage. Also record any CI-only Windows/macOS signal or permission coverage and whether audit growth makes rotation a justified follow-up.

## Context and orientation

The workspace root is `/home/aa/Documents/jig-sh`. Read `/home/aa/Documents/jig-sh/AGENTS.md`, `/home/aa/Documents/jig-sh/agent-map.md`, `/home/aa/Documents/jig-sh/crates/jig-vault/AGENTS.md`, and `/home/aa/Documents/jig-sh/.agent/PLANS.md` before implementation. The workspace uses Rust edition 2024, declares Rust 1.85 as its minimum supported version, and forbids new `mod.rs` files.

`crates/jig-vault` owns encrypted state and secret-bearing operations. Its important files are `src/format.rs` for the serialized envelope and state, `src/vault/envelope.rs` for parse/validate/unlock/seal phases, `src/vault.rs` for lock/audit/write orchestration, `src/types.rs` and `src/lib.rs` for public types, `src/audit.rs` for local tamper-evident events, `src/redact.rs` for raw and encoded value masking, `src/store.rs` for private filesystem operations, and `src/run.rs` plus `src/run/*` for the existing constrained broker.

`crates/jig-sh` owns the command line. `crates/jig/src/cli/vault.rs` defines Clap arguments, `cli/command_conversion.rs` maps them into DTOs in `command/vault.rs`, `cli/vault_run.rs` applies repository scope and chooses output behavior, `runtime/vault.rs` captures passphrases, resolves physical vault homes, and dispatches operations, and `cli/output/vault.rs` formats non-secret human summaries. `tool_defs.rs`, root help tests, and the relevant CLI/runtime tests must be updated with every new command surface.

An item is an operator-chosen environment or bundle such as `Production`, `Staging`, or `Infrastructure`. A field is one encrypted byte value within that item. A reference identifies a field within the already-selected vault. For example:

    jig://Production/RESTIC_PASSWORD
    jig://Staging/ARRAY_APP_KEY
    jig://Infrastructure/HETZNER_API_TOKEN
    jig://Landing/CLOUDFLARE_ACCOUNT_ID

The first three can be concealed. The Cloudflare account ID can be text. Both are encrypted in `vault.json`; the difference only controls output presentation and redaction.

The crate's non-negotiable security properties continue to apply. Plaintext values must not enter repository state, `.agent/state`, MCP results, JSON output, error text, audit details, `Debug`, or command receipts. Secret-bearing buffers must be bounded where input is under Jig control and zeroized on drop. Vault state mutations append audit intent before atomic save, so a crash may leave audit leading state but never state leading audit. Private permissions, symlink refusal, locks, and atomic replacement must remain centralized in `VaultStore` or equally hardened reusable helpers.

## Plan of work

### Baseline and work protocol

Start a structured work item with a short body that points to this authoritative plan, rather than copying its full contents into a second living document. Record the resulting plan ID in Progress:

    plan_id="$(scripts/jig work start \
      --title "Make Jig Vault a project-scoped 1Password CLI replacement" \
      --body "Execute and maintain .agent/plans/general-purpose-project-vault.md." \
      --print-plan-id)"

Build a development `jig` binary and force the harness launcher to use it whenever the Jig runtime itself is under test:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig

Before source edits, run and record:

    git status --short
    cargo fmt --all -- --check
    cargo check -p jig-vault --all-targets --locked
    cargo test -p jig-vault --locked
    cargo test -p jig-sh --locked
    cargo clippy -p jig-vault --all-targets --locked -- -D warnings
    cargo +1.85 check -p jig-vault --all-targets --locked
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id "$plan_id"

Record pre-existing failures in Surprises. Do not reset unrelated user changes or append-only `.agent/state/*.jsonl`. Implement and commit one green milestone at a time. After each milestone, update Progress, Decisions if the contract changed, Surprises, and Outcomes so another agent can resume from the last commit without chat history.

### Milestone 1 — references, field kinds, and envelope v2

Add `VaultReference` and `FieldKind` in `crates/jig-vault/src/types.rs` and export them from `src/lib.rs`. `VaultReference` owns validated item and field segments, implements `FromStr` and `Display` with one canonical spelling, exposes non-secret item/field accessors, and maps internally to the existing `SecretName` `ITEM/FIELD`. Its `Debug` output may show the reference because names are audit metadata, never the value. Add parser table tests for the accepted examples and every rejected ambiguity described in the Decision Log.

In `src/format.rs`, introduce explicit version-aware DTO handling. Preserve the exact v1 header validation and AAD byte construction. Add v2 AAD with its own `jig-vault-header-v2` domain separator. Keep the encrypted state map key named `secrets` on disk to minimize schema churn, but make a v2 entry carry `kind`. Defensive serde defaults may interpret a missing kind as concealed, yet version validation remains the primary compatibility boundary. New vault initialization writes v2; new code can parse and unlock v1 and v2.

Add a phase in `src/vault/envelope.rs` that upgrades an unlocked v1 envelope to v2 while retaining vault ID, creation time, salt, KDF parameters, data-encryption key, values, and timestamps. Assign concealed to every existing entry, generate fresh wrapped-key and state nonces, and authenticate both ciphertexts with the v2 header. Do not create plaintext copies merely to rename types. Characterization tests must prove v1 fixtures still decrypt with exact prior error ordering and AAD, v2 round-trips, v1 ciphertext is rejected under v2 AAD, and an old-format validator rejects v2 instead of accepting and dropping metadata.

Add `Vault::migrate(passphrase, target_version)` in `src/vault.rs`. Under the existing vault lock, open and verify the current vault and audit chain, return an idempotent `changed: false` if already v2, append a `vault_format_migrate` event containing only from/to versions, and atomically save the resealed envelope. A failed audit append must leave the vault untouched. A failed save may leave audit leading state, consistent with the existing mutation invariant. Expose `jig vault migrate --to 2`; do not add an automatic surprise migration.

Add field-oriented library methods and records while retaining legacy secret methods. The new surface should include the semantic equivalent of:

    pub enum FieldKind { Concealed, Text }
    pub struct VaultReference { /* private validated representation */ }
    pub struct FieldRecord { reference, kind, value_len, created_at_ms, updated_at_ms }
    pub enum FieldMutation { Set { reference, kind, value }, Remove { reference } }
    pub struct FieldBatchResult { /* names and changed/removed status only */ }

`Vault::list_fields`, `set_field`, `remove_field`, and `apply_field_batch` must not return values. The batch method validates all references, kinds, sizes, duplicates, overwrite policy, and resulting limits before appending one intent event and saving once under one lock. All failure paths drop zeroizing values without partial state changes. Legacy `set_secret` on v2 stores `Concealed`; v1 legacy behavior remains available until explicit migration.

Add the CLI family `jig vault field list [jig://ITEM]`, `field set REF [--text]`, and `field remove REF`. Reuse the hidden prompt and exact-byte stdin inputs. Do not weaken the concealed minimum needed for redaction; text fields may be shorter, including `0`, `1`, or an empty string if the existing encrypted-state size rules are updated deliberately and tested. Listing emits names, kinds, lengths, and timestamps only. A v1 mutation through the field commands fails with the exact migration command to run.

Milestone acceptance is a v1 fixture that still lists and runs, an explicit successful migration, a v2 vault containing both kinds, old CLI secret compatibility, exact v2 wire characterization, and no value in human/JSON/error/audit/Debug output. Commit this milestone before revelation or process work.

### Milestone 2 — controlled read and template injection

Add a private reveal lifecycle in `src/vault.rs` rather than exposing decrypted bytes through generic JSON/runtime DTOs. The library may return `SecretBytes` only through a purpose-specific result whose `Debug` hides the value. Resolution and a `field_read_start` or `template_inject_start` event happen under the vault lock; actual stdout or file I/O happens after lock release; then the prepared operation records `finish` or `failed` with the same operation ID. A missing field or start-audit failure reveals nothing. A crash can leave an unmatched start, which is more truthful than recording a completed read before its sink succeeds.

Implement `jig vault read REF [--reveal | --out-file PATH [--overwrite]]`. Raw stdout bypasses the standard `serde_json::Value` emitter. Reject global `--json` before unlocking. If stdout is a terminal require `--reveal`; if redirected or piped, write exact bytes with no appended newline. Add a dedicated byte-oriented hardened output installer rather than reusing vault text writes: it uses a same-directory temporary file, exact writes, fsync, owner-only permissions or Windows DACL, parent and leaf symlink/reparse checks, and an atomic no-clobber installation when overwrite is false. If those guarantees are not implemented on a platform, the file sink must reject that platform rather than silently weakening them. Error messages may contain the reference and destination but never the value.

Add `crates/jig-vault/src/template.rs` for a bounded, deterministic byte-template scanner. It recognizes `{{ jig://ITEM/FIELD }}`, rejects malformed Jig placeholders rather than copying them through, deduplicates references for resolution/audit, and preserves all non-placeholder bytes exactly. Do not use a general expression engine. Resolve every placeholder before emitting any output so a late missing reference cannot produce a partial revealed stream. Render into a zeroizing buffer capped at 16 MiB.

Implement `jig vault inject --in PATH [--reveal | --out-file PATH [--overwrite]]` with stdin input permitted only via an explicit `--in -`. Apply the same JSON, terminal, filesystem, and audit-before-reveal rules as `read`. Never allow the output path to be the input path unless `--overwrite` is explicit and the implementation uses atomic replacement after a complete successful render.

Milestone tests must cover binary field reads, TTY refusal, exact no-newline stdout, private file mode on Unix, symlink refusal, overwrite refusal, malformed and repeated placeholders, mixed concealed/text fields, audit-before-output failure, bounded input/output, and proof that neither standard JSON nor structured errors can include revealed bytes. Commit this milestone independently.

### Milestone 3 — transparent dotenv execution

Create a restricted reference-aware dotenv parser in `crates/jig/src/runtime/vault_env.rs`; do not call `dotenvy::from_read_iter`. It accepts bounded UTF-8 input containing blank lines, comments, and exact `NAME=VALUE` assignments with a small documented single-quote, double-quote, and escape grammar. It preserves line numbers and rejects every interpolation form, command substitution, NUL, invalid environment name, duplicate name, malformed `jig://`, and assignment to either Jig passphrase variable. On Windows, environment-name duplicate and collision checks are case-insensitive. Cap file bytes and total decoded environment data. Error messages name only line and variable, never value. Parsing and reference validation happen before vault unlock.

Add a separate transparent execution API to `jig-vault`; do not route through `BrokeredRun`. A purpose-specific request carries the command, decoded literal environment assignments, and field bindings. Under the vault lock, append an `exec_start` event and resolve all fields. Release the lock before spawning. A private prepared-exec type owns zeroizing values and the operation ID, builds redaction needles from concealed bindings only, and records `exec_finish` or `exec_failed` after waiting. A killed Jig process may leave an unmatched start, just as brokered runs do.

Implement streaming redaction in a new sibling such as `crates/jig-vault/src/exec_output.rs`, with a direct `aho-corasick` 1.1 dependency after confirming its license and Rust 1.85 compatibility. Refactor `Redactor` only enough to share its concealed raw/encoded pattern generation. Build a byte-only matcher with separate state for stdout and stderr, deterministic leftmost-longest matching, and at least `max_pattern_len - 1` bytes of overlap across reads. Preserve every nonmatching byte exactly and emit a fixed byte marker; never use lossy UTF-8 conversion or line buffering. Bound field count, field size, total generated needle bytes, overlap, and working memory. The guarantee applies independently to each logical stream; a value deliberately split between stdout and stderr cannot be detected without merging the streams, which this command does not do.

The child inherits stdin and starts with the current process environment, then dotenv assignments override inherited variables. Injected environment values must be valid UTF-8 and contain no NUL. The captured passphrase variables must be removed even if present in the parent. Pipe stdout and stderr only so they can be redacted, write redacted chunks promptly to the corresponding parent streams, and wait without a Jig timeout or output cap. While the leader lives, drain both descriptors. After it exits, perform only a bounded final drain and close readers so a descendant retaining a pipe cannot hang Jig forever; document that such descendant output can be truncated or receive a broken pipe. Handle a parent-output EPIPE without indefinitely joining blocked readers or leaving the direct child deadlocked. On normal platforms the child shares the foreground console/process group so terminal signals reach it naturally; Jig does not create or later kill a separate owned process tree for this command. Mirror normal exit codes and conventional signal-derived codes through a private silent-exit error recognized by `main.rs`, without printing a second Jig error after the child fails.

Expose `jig vault exec --env-file PATH -- COMMAND...`. Reject `--json` before passphrase capture. Keep `jig vault run` and its output JSON unchanged. Update `cli/vault_run.rs` so streaming commands bypass `runtime::dispatch_vault` and the normal emitter, while all scope selection and passphrase clearing stays shared. Add exact help text explaining the difference between `exec` and constrained `run`.

Milestone acceptance must include inherited ordinary environment, dotenv override, passphrase non-inheritance, stdin passthrough, live stdout/stderr, an output secret split at every chunk boundary, encoded concealed values, a common text value such as `false` remaining visible, large output beyond 1 MiB, a command longer than the broker timeout using a test clock or injected deadline rather than a 30-minute test, exit 0, nonzero exit, signal exit, spawn failure, and unmatched-start characterization. Run Unix process tests locally and leave Windows console behavior to CI if the target is unavailable. Commit separately.

### Milestone 4 — atomic batch import from 1Password dotenv

Add `jig vault import onepassword --env-file SOURCE --item ITEM --out-env DESTINATION [--replace] [--overwrite] [--dry-run]`. This is a migration adapter, not a runtime dependency. Locate `op` using normal process lookup, invoke it with exact argv `op read --no-newline <exact-reference>` without a shell, close its stdin, and capture stdout and stderr concurrently with strict caps so neither pipe can deadlock. Zeroize captured stdout and temporary decoded values. Never copy raw `op` stderr into a Jig error because it may contain an unknown value; report only the environment variable, reference name, exit status, and a generic bounded diagnostic category.

Parse the source with the same restricted dotenv contract as `exec`. Every left-hand variable name becomes the destination field name. `op://` values are resolved and classified concealed. Literal values are not discarded or left behind: they are classified text and encrypted into the same item. Reject command substitutions, interpolation, duplicate variable names, invalid item/field identifiers, reference collisions, non-UTF-8 or NUL-containing resolved values, fields already present without `--replace`, and an existing destination without `--overwrite` before mutating the vault. Enforce field-count, per-value, total decoded batch, and projected serialized vault-state limits. `--dry-run` validates and prints only source path, destination path, references, kinds, and whether each would create/replace; it calls no `op` process and makes no mutation, but it does unlock, verify, and list the v2 vault read-only so the status is accurate.

After all `op` calls succeed, construct one `FieldMutation` batch and one canonical destination buffer whose every assignment is `VARIABLE=jig://ITEM/VARIABLE`. Prepare the destination temporary file with private permissions but do not rename it yet. Under one vault lock, recheck collisions, append one `onepassword_import` intent listing references and counts only, and atomically save all fields. Then atomically install the destination file. If installation fails after the vault commit, report that the vault import succeeded and give the exact safe rerun command; rerunning with `--replace --overwrite` must converge to the same fields and file without value exposure. Cross-filesystem atomicity between vault home and repository is impossible, so recovery is explicit rather than falsely claimed.

Add integration tests with a fake `op` executable that records argv and returns controlled binary/UTF-8 values. Prove no shell interpretation, literal-to-text encryption, concealed classification, stable output order, private destination permissions, all-or-nothing vault mutation on the last resolution failure, overwrite refusal, bounded child output, dry-run non-execution, rerun convergence, and audit details without values. Commit independently.

### Milestone 5 — passphrase change and encrypted backup/restore

Add `Vault::change_passphrase(old, new)` and `jig vault passphrase change`. Validate the new passphrase with the existing new-vault policy. In `src/vault/envelope.rs`, build a new v2 header with the existing vault identity and creation time, current KDF parameters, and a fresh salt. Derive the new key-encryption key, rewrap the unchanged data-encryption key under v2 wrapped-key AAD, and reseal the unchanged state under v2 state AAD, both with fresh nonces. Under one lock, verify the old passphrase and audit, append `passphrase_change`, and atomically save. Zeroize both passphrases, derived keys, decrypted state, and serialized plaintext on all paths. Tests must prove the old passphrase fails, the new one opens state and audit, values/timestamps/kinds are unchanged, both nonces and salt rotate, wrong old/new inputs leave bytes unchanged, and audit failure prevents the save.

Extend passphrase capture in `crates/jig/src/runtime/vault.rs` without turning command-line arguments into a source. Interactive use prompts once for the current passphrase and twice for the new passphrase. Noninteractive use requires `JIG_VAULT_PASSPHRASE` and `JIG_VAULT_NEW_PASSPHRASE`; clear both process variables immediately after capture and never forward them to a child.

Add `crates/jig-vault/src/backup.rs` with a versioned backup envelope independent of `VaultFile`. The public JSON header contains only backup magic/version, creation timestamp, Argon2id parameters, salt, AEAD name, nonce, and ciphertext. Its AAD uses the independent domain `jig-vault-backup-header-v1\n`, canonical length-delimited fields, and binds every public header field including nonce plus payload role `backup_payload`. Validate magic, version, KDF bounds, base64 decoded lengths, and ciphertext cap before Argon2 or decryption. The authenticated payload has its own version and contains bounded exact byte blobs for `vault.json` and `audit.jsonl` plus source vault ID and format version; compare those identifiers with the embedded vault header. Choose an explicit one-shot archive cap below the combined 16 MiB vault and 256 MiB audit maxima so AEAD and serialization copies remain safe, and fail with an actionable audit-size message rather than pretending the backup streams. Use a fresh salt/nonce on every creation. Model output as `backup_start` followed by `backup_finish` or `backup_failed`; never record a bare success before the output file exists.

Expose `jig vault backup create --out FILE [--overwrite]` and `jig vault backup restore --in FILE`. Creation uses the current vault passphrase for both unlock and backup encryption; a backup remains decryptable with the passphrase current when it was made even after a later rekey. Restore must bypass ordinary `Vault::resolve`, because that creates the destination. Add a non-creating target preflight, validate the parent safely, require the selected home to be absent, decrypt and validate all bounds and identifiers, write to an owned private sibling staging directory, open the staged vault, verify its audit, append `backup_restore` there, verify again, and install the complete directory with an atomic no-replace operation. Refuse symlinked/reparse input, malformed or oversized backups, embedded path data, inconsistent embedded vault ID/version, any existing destination including an empty directory, or cross-filesystem staging. Ordinary `fs::rename` is insufficient where it can replace an empty directory or has platform-dependent behavior. On any pre-install failure, remove only the validated owned staging directory; never remove or overwrite an existing vault.

Tests must cover backup nondeterminism, wrong passphrase, ciphertext/header tamper, truncated/oversized input, permissions, symlinks, no-overwrite, embedded v1 rejection with the instruction to migrate first, audit tamper, staged failure cleanup, successful restore into a new home, restored field behavior, and source/restore audit verification. Commit this lifecycle milestone independently.

### Milestone 6 — compatibility, documentation, and IdentityPro acceptance

Update root and vault help examples, the nearest vault documentation, and crate guide invariants. Explain project-relative references, both encrypted field kinds, v1 migration, the safety difference between `exec` and `run`, reveal sinks, import recovery, passphrase rotation, and backup restore. Never put realistic credential-shaped values into examples or snapshots.

Keep all old command parsing and output tests green. Add a compatibility matrix to the documentation: new Jig reads v1 and v2; v1 requires explicit migration before new mutation/lifecycle operations; old Jig rejects v2; `secret` remains an alias over concealed fields in v2; `run` remains constrained; `exec` is transparent. State clearly that local audit is not remote/independent evidence and that backup/restore, not reference qualification, moves a project vault.

Create a temporary acceptance repository rather than reading or modifying `~/Documents/identitypro`. Give it a `.jig.toml` with a test scope ID, initialize and migrate a vault, and import a fixture dotenv containing fake `op://IdentityPro/...` references and fake literals through the fake `op` executable. Prove the generated file resembles:

    RESTIC_PASSWORD=jig://Production/RESTIC_PASSWORD
    RESTIC_REPOSITORY=jig://Production/RESTIC_REPOSITORY
    RESTIC_COMPRESSION=jig://Production/RESTIC_COMPRESSION

Mark the password concealed and the repository/compression values text. Run a fixture backup command with `jig vault exec --env-file ...`, inject a fixture config, read a selected field through a pipe, rotate the passphrase, create an encrypted backup, restore to another explicit test home, and repeat the exec. Assert that common text output is not masked, concealed output is masked, nonzero child status is preserved, and no plaintext appears in the repository, JSON, audit, errors, receipts, or Git diff.

The acceptance test demonstrates how IdentityPro can migrate later, but it must not invoke the real `op`, inspect the real project, or mutate real secrets. Actual IdentityPro cutover is a separate operational change after this generic feature ships.

## Concrete steps

Work from `/home/aa/Documents/jig-sh`. At the start of each milestone, inspect the relevant files and current diff. Prefer focused tests while iterating, then run the complete crate checks before commit. Representative commands are:

    cargo fmt --all -- --check
    cargo check -p jig-vault --all-targets --locked
    cargo test -p jig-vault --locked vault_reference
    cargo test -p jig-vault --locked format_v2
    cargo test -p jig-vault --locked field_batch
    cargo test -p jig-vault --locked read
    cargo test -p jig-vault --locked inject
    cargo test -p jig-vault --locked exec
    cargo test -p jig-sh --locked vault
    cargo clippy -p jig-vault --all-targets --locked -- -D warnings
    cargo clippy -p jig-sh --all-targets --locked -- -D warnings
    git diff --check

Test filters are illustrative; replace them with actual stable test-module names and record those names in Progress. Do not declare a milestone complete from filtered tests alone. Before every milestone commit run full `cargo test -p jig-vault --locked`, `cargo test -p jig-sh --locked`, format, check, and relevant Clippy. Inspect both unstaged and staged diffs for value-shaped fixtures, accidental output, unrelated changes, and forbidden `mod.rs` files.

Use commit boundaries that match the milestones, for example:

    feat(jig-vault): add project field references and format v2
    feat(jig-vault): add controlled field read and injection
    feat(jig-vault): add transparent dotenv exec
    feat(jig-vault): import onepassword dotenv bundles
    feat(jig-vault): add rekey and encrypted recovery
    docs(jig-vault): document local password-manager workflows

These are guidance, not a requirement to commit plan/receipt updates with source. Preserve the repository's append-only work records and follow `scripts/jig work` ownership when deciding where workflow files are committed.

## Validation and acceptance

The feature is complete only when observable CLI behavior, persisted compatibility, secret non-disclosure, and recovery are all demonstrated. The following manual sequence, using fake values and isolated temporary homes, is the minimum smoke test:

    export JIG_DEV_BIN=target/debug/jig
    export JIG_VAULT_HOME="$(mktemp -d)/vault-base"
    export JIG_VAULT_PASSPHRASE='test-only-long-passphrase'
    target/debug/jig vault init --home "$JIG_VAULT_HOME/source"
    printf '%s' 'fake-secret-value' | target/debug/jig vault field set jig://Production/RESTIC_PASSWORD --value-stdin --home "$JIG_VAULT_HOME/source"
    printf '%s' 'false' | target/debug/jig vault field set jig://Production/RESTIC_COMPRESSION --text --value-stdin --home "$JIG_VAULT_HOME/source"
    printf '%s\n' 'TOKEN=jig://Production/RESTIC_PASSWORD' 'FLAG=jig://Production/RESTIC_COMPRESSION' > "$JIG_VAULT_HOME/test.env"
    target/debug/jig vault exec --home "$JIG_VAULT_HOME/source" --env-file "$JIG_VAULT_HOME/test.env" -- sh -c 'printf "%s %s\n" "$TOKEN" "$FLAG"'

The expected exec output contains a redaction marker and the literal word `false`; it never contains `fake-secret-value`. The command exits with the child's status and does not emit a Jig summary. Continue with read through a pipe, injection to a private file, passphrase change, backup, and restore. When writing the actual smoke script, use `mktemp -d`, explicit validated child paths, and a cleanup trap; never use a home directory or repository root as a recursive cleanup target.

Automated validation must include:

    cargo fmt --all -- --check
    cargo check -p jig-vault --all-targets --locked
    cargo test -p jig-vault --locked
    cargo clippy -p jig-vault --all-targets --locked -- -D warnings
    cargo test -p jig-sh --locked
    cargo clippy -p jig-sh --all-targets --locked -- -D warnings
    cargo +1.85 check -p jig-vault --all-targets --locked
    cargo +1.85 check -p jig-sh --all-targets --locked
    cargo test --workspace --locked

Then rebuild the dev binary and run the repository harness through it:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig work check --plan-id "$plan_id"
    scripts/jig work evidence --plan-id "$plan_id"
    scripts/jig work gates --plan-id "$plan_id"
    scripts/jig work receipts --plan-id "$plan_id"
    scripts/jig work status

Success means all configured gates are fresh and passing, or every environment-only exception is recorded precisely in Surprises and left for CI. CI must cover macOS and Windows compilation/tests for the new streaming process and private-file branches. Check repository policy for no new `mod.rs` and changed-file size. Search the final diff and test artifacts for the fake plaintext sentinel and ensure it occurs only in deliberately local test-process input, never in snapshots, JSON, audit fixtures, docs, or work receipts.

## Idempotence and recovery

Every command in this plan must fail closed before revealing or mutating when validation is incomplete. `migrate --to 2` is idempotent and reports no change on v2. Field batch import is atomic within the vault; rerunning an import with explicit replacement converges after a destination-file installation failure. Backup creation refuses overwrite unless requested and uses atomic replacement. Restore never overwrites a populated vault, so recovery is to choose another empty `--home`, verify it, and only then perform any manual directory move.

If v1-to-v2 migration appends audit intent but fails to save, rerun the migration. The audit may contain a leading intent record, which is valid under the existing model; the subsequent successful event records the eventual transition. Never edit the audit log to make it look adjacent to state. If a v2 vault must be used by an old binary, restore a pre-migration encrypted backup to a different explicit home; there is no downgrade command.

If `exec` starts but Jig is killed, the audit can contain an unmatched start. Do not synthesize completion in `Drop` or hold the vault lock for the child's lifetime. A normal nonzero child exit is not a vault corruption and requires no recovery. If output redaction or pipe forwarding fails, record failure and return promptly; do not reuse the constrained broker's tree-kill promises unless the implementation explicitly creates and owns such a tree.

If passphrase change fails before atomic save, the old passphrase remains authoritative. If it fails after save but before the CLI reports success, try the new passphrase first, then the old, without making further mutations; atomic replacement means one complete envelope should open. Record the observed result in Surprises before retrying the command.

For backup restore, create staging only as a concrete sibling of the validated destination, record the exact path in a guard, and clean up only that path on failure. A crash may leave a recognizable staging directory; a later restore may remove it only after confirming its generated prefix, ownership, non-symlink status, and destination relationship. Never recursively delete an unresolved environment variable, glob, `~`, the repository root, or the vault base.

If a milestone cannot preserve v1 compatibility, Rust 1.85, Windows/macOS compilation, or secret non-disclosure, stop at the last green commit and update this plan. Do not paper over the failure with a broad API break or a weaker security rule.

## Interfaces and dependencies

The stable new library concepts are `VaultReference`, `FieldKind`, field metadata records, atomic field mutations, controlled reveal/inject requests, transparent exec requests/results, migration, passphrase change, and backup/restore. Public structs should be `#[non_exhaustive]` where downstream construction is not required, own validated data, implement redacted `Debug` for anything that can carry values, and use crate-owned error variants with actionable context that never includes a value. Preserve `SecretBytes` as the zeroizing byte owner rather than introducing public `String` secrets.

The stable CLI surface is:

    jig vault migrate --to 2
    jig vault field list [jig://ITEM]
    jig vault field set jig://ITEM/FIELD [--text] [--value-prompt|--value-stdin]
    jig vault field remove jig://ITEM/FIELD
    jig vault read jig://ITEM/FIELD [--reveal|--out-file PATH [--overwrite]]
    jig vault inject --in PATH [--reveal|--out-file PATH [--overwrite]]
    jig vault exec --env-file PATH -- COMMAND...
    jig vault import onepassword --env-file PATH --item ITEM --out-env PATH [--replace] [--overwrite] [--dry-run]
    jig vault passphrase change
    jig vault backup create --out PATH [--overwrite]
    jig vault backup restore --in PATH

All commands retain the existing flattened `--home` and `--global` scope selectors subject to repository `allow_global` policy. A reference never overrides that selected scope.

Use existing workspace dependencies wherever possible. `dotenvy` is already in `jig-sh`; crypto, serde, zeroization, temporary files, hashing, and private storage are already in `jig-vault`. The `op` executable is optional and used only by the importer. If streaming redaction needs a multi-pattern crate, document its license, minimum Rust version, dependency graph, and why the current bounded redactor cannot serve unbounded streams before adding it. Do not add a network service, keyring, clipboard, archive utility subprocess, shell invocation, or 1Password SDK in this plan.

Revision note (2026-08-09): Initial ExecPlan created from the agreed general-purpose scope. It makes project context implicit in references, keeps contextual values encrypted as `text` fields, separates transparent `exec` from constrained `run`, and incorporates the adversarial findings on format compatibility, streaming redaction, full-reseal rekeying, local-audit limits, atomic import recovery, and path-bound project scope.

Revision note (2026-08-09): Updated during implementation after a Terra adversarial architecture pass. The revision adds a validated item selector, replaces ambient-expanding dotenv parsing with a restricted grammar, fixes `op read` newline behavior and dry-run truthfulness, makes reveal/backup audit events lifecycle-based, defines byte-preserving streaming redaction and descendant-pipe behavior, adds total batch limits, and changes restore from an empty-target rename to absent-target atomic no-replace installation.

Revision note (2026-08-09): Recorded completed Milestone 1 and its commit/evidence. The revision documents the review-found aggregate-size, v1 DTO, and CLI validation defects and their fixes, clarifies the combined 128-byte durable reference-name boundary, and records the static pre-change v1 fixture used for compatibility proof.

Revision note (2026-08-09): Recorded completed Milestone 2 and commit `d80f496`. The revision documents opaque pre-passphrase template validation, direct sink lifecycle terminalization, the hardened Unix file-installer policy, non-Unix file-sink fail-closed behavior, and consolidated test/MSRV/Clippy evidence.
