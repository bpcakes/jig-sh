# Build a complete keyboard-first Vault TUI

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must remain current while implementation proceeds. Maintain this file according to `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

Jig Vault already encrypts project and global secrets, manages canonical `jig://ITEM/FIELD` references, imports 1Password dotenv bundles, changes passphrases, creates and restores encrypted backups, injects values into commands and files, and verifies its tamper-evident audit log. Today an operator must remember and repeatedly invoke individual CLI commands. After this change, `jig vault tui` opens a full-screen, keyboard-first manager that lets the operator unlock one selected vault, browse every canonical and legacy entry, search metadata, create and replace values, change field kinds, rename and delete entries, import, back up, rotate the passphrase, inspect verified activity, restore an absent vault, and export or briefly inspect a value through controlled sinks.

The interface must preserve the vault's existing security contract. Plaintext values never enter ordinary Ratatui render buffers, JSON output, logs, errors, audit details, receipts, or debug output. Secret input is held in bounded zeroizing storage. Unlock state exists only inside this `jig` process, can be explicitly locked, and automatically locks after inactivity. `jig vault exec`, `jig vault run`, and `jig vault inject` remain command-line operational workflows because they own child-process and streaming terminal semantics; the TUI is the interactive management plane.

A user can see the completed feature by running `cargo build -p jig-sh --bin jig`, setting `JIG_DEV_BIN=target/debug/jig`, and invoking `scripts/jig vault tui` from an adopted repository. The alternate-screen interface must identify the fixed scope, show an Items / Fields / Details explorer, keep values hidden, expose contextual keys and help, and complete each mutation without leaving the terminal in raw mode.

## Progress

- [x] (2026-08-13 21:47Z) Researched the current Vault v2 surface, existing Jig TUIs, comparable secret-management TUIs, and plaintext/session constraints.
- [x] (2026-08-13 21:47Z) Authored this self-contained ExecPlan and divided the work into independently testable, committable milestones.
- [x] (2026-08-13 21:49Z) Opened structured Jig work `plan_01KZYHNEMNNN5B80EDBFA2WDJ9`, whose body points to this authoritative ExecPlan.
- [x] (2026-08-13 22:02Z) Milestone 1: added verified, disjoint metadata snapshots; newest-first safe activity projection; and atomic kind-change, field/item rename, item removal, and legacy conversion APIs. All 206 `jig-vault` tests and strict all-target Clippy pass.
- [x] (2026-08-13 22:26Z) Milestone 2: added the `jig-vault-tui` crate, protected input and paired bracketed-paste lifecycle, explicit CLI entrypoint, fixed-scope session adapter, unlock/lock/init/migrate states, responsive explorer, legacy visibility, and PTY restoration coverage. Focused tests and strict all-target Clippy pass.
- [x] (2026-08-13 22:53Z) Milestone 3: added bounded protected value entry plus canonical and legacy create/replace/change-kind/rename/delete/convert workflows, atomic stale-state preconditions, exact destructive confirmation, and management PTY coverage. All focused suites and strict all-target Clippy pass.
- [x] (2026-08-13 23:32Z) Milestone 4: added the Tools palette with metadata-only 1Password preview/dry-run and confirmed import, encrypted backup, passphrase rotation, verified Activity/audit views, and Linux absent-home restore. Focused core, adapter, integration, PTY, formatting, and strict Clippy checks pass.
- [x] (2026-08-14 00:04Z) Milestone 5: added private-file export, an exact-confirmation bounded terminal-safe Peek, five-minute idle locking, fail-closed authentication/audit handling, direct-output cleanup on normal exit/signal/panic, large-terminal hardening, documentation, and PTY/sentinel coverage. All focused suites and strict all-target Clippy pass.
- [x] (2026-08-14 00:39Z) Built the development Jig binary and forced the harness through it. Formatting, workspace-wide locked strict Clippy, contract, the complete 1,960-test non-Vault partition, the complete 317-test Vault/crypto partition, structured evidence, both required gates, and final diff/leakage/placeholder audits pass.
- [ ] Close structured work and record final commit identifiers and outcomes.

## Surprises & Discoveries

- Observation: `Vault::list_fields` intentionally omits legacy secret names that cannot be represented as `jig://ITEM/FIELD`.
  Evidence: `crates/jig-vault/src/vault.rs` documents the omission and directs callers to `Vault::list`. The TUI must reconcile both metadata views and present unrepresentable entries under an explicit Legacy section.

- Observation: the current CLI captures a passphrase before runtime work and consumes it for exactly one vault operation.
  Evidence: `crates/jig/src/runtime/vault/lifecycle.rs::passphrase` removes the sole `SecretString` from `CapturedPassphrases`. A multi-operation TUI therefore needs a dedicated process-local session adapter rather than repeatedly dispatching command DTOs.

- Observation: production vault opening intentionally performs an expensive Argon2id derivation using 131,072 KiB, three iterations, and four lanes.
  Evidence: `crates/jig-vault/src/crypto.rs::KdfParams::production`. The implementation must measure ordinary TUI actions before deciding whether a reviewed session-key API is necessary; it must not retain the whole decrypted vault merely to optimize prematurely.

- Observation: Ratatui frames retain owned cell strings and are not a zeroizing plaintext surface.
  Evidence: existing Jig TUI models render ordinary owned text into a reusable terminal buffer, while `crates/jig-vault/AGENTS.md` requires public reveal operations to consume directly into an immediate caller-selected sink. Inline values cannot be implemented as ordinary Paragraph content.

- Observation: Crossterm 0.28.1 is already built with bracketed-paste and event features, but shared `TerminalSession` currently enables only raw mode, alternate screen, and cursor hiding.
  Evidence: `cargo tree -e features -p jig-tui` reports `crossterm/bracketed-paste`; `crates/jig-tui/src/lib.rs::TerminalSession` has no paired paste enable/disable commands.

- Observation: the existing audit verifier consumed its audit-log `String`, which made a verified activity projection either re-read the file or duplicate the whole log.
  Evidence: Milestone 1 changed the private `verify_chain_text` helper to borrow `&str`; activity now verifies once and reparses only the authenticated valid prefix while the vault lock remains held.

- Observation: the existing shared terminal session could not opt into bracketed-paste events without changing every Jig TUI.
  Evidence: Milestone 2 added `TerminalSession::enter_with_bracketed_paste` alongside the unchanged default entrypoint. Setup failure, normal drop, and unwind all issue the matching disable sequence before leaving the alternate screen and raw mode.

- Observation: an environment-provided passphrase can feed an interactive session without using the existing single-operation global capture slot.
  Evidence: the new TUI capture converts the UTF-8 environment value directly into `SecretBytes`, clears both reserved variables before the action worker starts, and transfers ownership into the backend unlock call; tests prove the ordinary captured slot remains empty.

- Observation: refreshing after a successful mutation is insufficient to prevent a stale TUI form from overwriting a destination created by another process while the form was open.
  Evidence: Milestone 3 added `VaultWriteMode::{Create, Replace, Upsert}` and required-removal APIs whose existence checks run inside the audited vault edit lock. Two-handle tests prove stale creates, replacements, and removals fail without changing state or audit, and the TUI refreshes metadata after collision/not-found errors.

- Observation: exact binary secret input cannot safely use the ordinary text editor path or an unguarded `std::fs::read`.
  Evidence: `SecretInput::from_regular_file` opens a non-symlink regular file with `O_NOFOLLOW` on Unix, rejects metadata larger than one MiB, reads into a fixed-capacity `SecretBytes` allocation with a zeroizing chunk buffer, and never converts the bytes through UTF-8. Tests cover exact binary bytes, oversized files, and symlinks.

- Observation: the configured Vault split used `test(vault)`, which matches test names but not `jig-sh` integration binaries such as `vault_import`, `vault_lifecycle`, `vault_reveal`, and `vault_tui`.
  Evidence: three broad attempts ended with every executed test passing but the same 18 frontend cases unrun because expensive/process-owning Vault integrations were still mixed into the first partition. Replacing that boundary with `test(vault) | binary(/vault_.*/)` in both `.jig.toml` commands and the matching Nextest group produces a proven disjoint union of 1,960 and 317 non-ignored tests. Receipt `receipt_01KZYV949AVVHT08FCCGN3BX39` records both complete partitions passing in 826.568 seconds.

- Observation: a safe 1Password preview can validate mappings, output hardening, recovery-command encodability, and current field collisions without spawning `op` or retaining values.
  Evidence: the TUI adapter reuses the existing dotenv parser and import-entry projection, calls the core metadata-only collision preview, and invokes the owned resolver only after exact `IMPORT` confirmation. A fake-`op` test proves preview and dry-run do not create its log, while commit strips both reserved passphrase variables and writes the expected private reference file.

- Observation: raw PTY output is not a stable plain-text snapshot because Ratatui may split visible labels with cursor-positioning escape sequences.
  Evidence: the lifecycle Tools acceptance initially waited for multiword box titles that were visibly present but non-contiguous in the byte stream. It now synchronizes on stable action identifiers and waits for the browser repaint after closing Activity before resizing and locking.

- Observation: Ratatui 0.28 indexes buffer cells using `u16` multiplication, so a valid 608-by-113 terminal can wrap the computed cell offset even though the backing allocation itself fits in memory.
  Evidence: the required large-terminal regression initially overwrote earlier rows instead of rendering all three metadata headings. The renderer now caps only the active height to `u16::MAX / width`, leaving the already-cleared remainder of the alternate screen unused; the 608-by-113 metadata-only test passes.

- Observation: clearing a transient reveal only on its ordinary return is insufficient because a panic can unwind while direct output is still on the alternate screen.
  Evidence: `TerminalSession::Drop` now clears and homes the alternate screen before restoring the cursor and terminal mode. A self-spawned PTY test writes a direct-output marker, deliberately panics, and proves a later clear, bracketed-paste disable, alternate-screen leave, and terminal flag restoration.

## Decision Log

- Decision: expose the feature as explicit `jig vault tui`, not as terminal-dependent behavior of bare `jig vault`.
  Rationale: explicit invocation preserves stable scripting and help behavior, permits a hard stdin/stdout TTY requirement, and rejects `--json` without ambiguity.
  Date/Author: 2026-08-13 / Codex

- Decision: create `crates/jig-vault-tui` as a feature-specific presentation crate on top of `jig-tui`, with a narrow typed backend implemented by `crates/jig`.
  Rationale: repository policy keeps feature interaction out of `jig-tui` and runtime/repository policy out of presentation crates. This matches `jig-status-tui` and `jig-codex-tui`.
  Date/Author: 2026-08-13 / Codex

- Decision: keep one resolved repo, global, or explicit-home scope for the lifetime of a TUI session and display it persistently.
  Rationale: references intentionally contain no scope identity. In-session scope switching would make destructive actions easier to direct at the wrong vault and would complicate unlock ownership. Operators can exit and relaunch with another scope.
  Date/Author: 2026-08-13 / Codex

- Decision: render a responsive three-pane Items / Fields / Details explorer, collapsing to one focused pane with breadcrumbs on narrow terminals.
  Rationale: the data is naturally hierarchical and this layout is proven by keyboard-first tools such as Yazi and Vau. Details contain metadata only, so selection never decrypts a value.
  Date/Author: 2026-08-13 / Codex

- Decision: retain only a process-local credential in the CLI-owned backend, initially reuse the existing lock-and-reopen `Vault` methods for each operation, and add a derived-key session API only if measured production-KDF latency makes the interface unusable.
  Rationale: per-operation open preserves current concurrent CLI behavior and avoids expanding the core plaintext lifetime. A later optimization must still reload and authenticate state under the vault lock and must never cache the complete decrypted `OpenVault`.
  Date/Author: 2026-08-13 / Codex

- Decision: never prefill an edit form with an existing secret. Replacing a value begins empty and leaves the stored value unchanged until Save succeeds.
  Rationale: the TUI can implement replacement without exposing an accessor or moving current plaintext into presentation state.
  Date/Author: 2026-08-13 / Codex

- Decision: private-file export is the first controlled output feature; transient Peek bypasses Ratatui and writes terminal-safe escaped content through the existing audited immediate-sink API; native clipboard integration is not required by this plan.
  Rationale: the existing vault plan explicitly excluded clipboard support, terminal clipboard protocols behave differently across local terminals, SSH, tmux, and clipboard-history managers, and no portable clearing guarantee exists. Private files and an explicitly timed terminal sink satisfy value access without silently broadening the trust boundary.
  Date/Author: 2026-08-13 / Codex

- Decision: keep `exec`, `run`, and `inject` outside the TUI.
  Rationale: those commands own raw byte streams, inherited stdin, child status, process-tree supervision, and redaction. Starting them under a raw alternate-screen manager would combine incompatible terminal and cancellation ownership. The TUI may show exact reference-oriented command recipes but does not execute them.
  Date/Author: 2026-08-13 / Codex

- Decision: commit after every successful milestone, including the initial plan/structured-work slice.
  Rationale: the user explicitly requested suitable slice commits, and each milestone is designed to leave tests green and provide a restartable boundary. Commits must include only files belonging to that milestone plus append-only Jig work records produced for it.
  Date/Author: 2026-08-13 / Codex

- Decision: give each new atomic management operation a distinct audit action instead of disguising metadata-only moves as field set/remove batches.
  Rationale: kind changes, renames, item removal, and legacy conversion do not expose or replace values. Distinct authenticated actions make the Activity view intelligible and allow action-specific safe metadata whitelisting without returning arbitrary audit JSON.
  Date/Author: 2026-08-13 / Codex

- Decision: preserve existing CLI upsert behavior while exposing explicit create and replace modes for interactive forms, and require existence for TUI removals.
  Rationale: the TUI has a potentially stale metadata snapshot. Lock-scoped preconditions prevent a create from overwriting a concurrent destination and prevent a replace or delete from silently succeeding after its selected entry changed or disappeared, without breaking the established CLI API.
  Date/Author: 2026-08-13 / Codex

- Decision: require exact typed confirmation for permanent deletion and keep replacement editors empty.
  Rationale: storage has no trash or version history, so field and legacy deletion use their full identity while item deletion uses `DELETE` and displays the affected count. Empty replacement state preserves the rule that current plaintext never enters the presentation model.
  Date/Author: 2026-08-13 / Codex

- Decision: make lifecycle operations a version- and state-aware Tools palette, with restore as the only action for an absent Linux target and import/backup/passphrase tools available only for an unlocked v2 vault.
  Rationale: this keeps the everyday explorer compact, prevents unsupported v1 writes, and makes destructive lifecycle operations deliberate without inventing a second scope-selection surface.
  Date/Author: 2026-08-13 / Codex

- Decision: show only the public verified activity projection and audit counts/torn-tail status; never render raw audit JSON or `latest_mac` even though the public verification type carries it.
  Rationale: the activity projection is explicitly whitelisted domain metadata. MAC material is neither operationally useful in the TUI nor part of the requested management view, and omitting it narrows the render boundary.
  Date/Author: 2026-08-13 / Codex

- Decision: require typing `PEEK`, display at most 4 KiB of source bytes through an immediate escaping writer, and clear after the next key or ten seconds without ever returning plaintext or the escaped copy through TUI model state.
  Rationale: this makes disclosure deliberate and bounded, prevents terminal-control injection and Ratatui-buffer retention, and keeps the unavoidable scrollback/multiplexer/remote-recording trust boundary visible to the operator.
  Date/Author: 2026-08-14 / Codex

- Decision: treat only actionable key and paste events as activity, join any owned operation before explicit or idle lock, and lock on both authentication and audit failures.
  Rationale: resize and focus noise must not keep a credential alive indefinitely, while dropping a credential underneath an audited operation would violate the single-worker lifecycle. Authentication or chain-integrity loss invalidates the complete session rather than only the current action.
  Date/Author: 2026-08-14 / Codex

- Decision: partition Vault-heavy tests by both unit-test name and `vault_*` integration-binary identity, retain sequential execution, and apply the same boundary to ordinary, locked, and test-group configuration.
  Rationale: the old name-only filter neither expressed nor enforced its intended isolation. The corrected complementary filters cover every one of the 2,277 non-ignored workspace tests exactly once and allow signal/process-heavy Vault integration tests to complete without canceling the frontend fixture tail.
  Date/Author: 2026-08-14 / Codex

## Outcomes & Retrospective

Structured work is active as `plan_01KZYHNEMNNN5B80EDBFA2WDJ9`. Milestone 1 supplies the complete metadata and atomic domain foundation: one unlock now returns canonical fields, disjoint unrepresentable legacy records, format/identity metadata, and audit verification; verified recent activity contains only whitelisted summaries; and management moves preserve encrypted bytes and creation timestamps under one audited lock/save. Collision, v1, tamper, short-value, timestamp, no-plaintext, and legacy-deduplication tests are included. The complete `jig-vault` suite reports 206 passed in 98.91 seconds.

Milestone 2 adds an explicit TTY-only `jig vault tui`, rejects JSON before scope or credential side effects, fixes scope for the session, and moves a captured environment credential into protected ownership before any worker exists. The CLI-owned backend retains only `SecretString`, reopens current authenticated state for each action, and blocks lock/exit on its one worker. The TUI browses canonical items and disjoint legacy entries in wide three-pane or compact breadcrumb layouts, preserves exact selection across refreshes, supports metadata-only search, makes v1 migration deliberate, and never renders the test sentinel. Focused evidence is 6 `jig-tui` tests, 11 `jig-vault-tui` tests, 92 filtered `jig-sh` vault tests, one end-to-end PTY unlock/resize/lock/quit test, and strict all-target Clippy across the affected crates. Milestones 3–5 and the final repository gates remain.

Milestone 3 completes interactive value management. Canonical fields can be created, replaced from an always-empty editor or exact binary regular file, retyped, renamed or moved, and deleted individually or by item; legacy values can be explicitly created, replaced, converted atomically, and removed. Protected input is capped at one MiB, uses non-growing zeroizing storage, rejects an overflowing paste as a unit, and exposes only bullets and byte counts to rendering. Destructive actions require exact typed confirmation. Create, replace, and required-remove preconditions are enforced under the vault lock so concurrent CLI changes cannot be overwritten from a stale form, and conflict/not-found results trigger a metadata refresh without automatic retry. Evidence is 209 `jig-vault` tests in 95.33 seconds, 19 `jig-vault-tui` tests in 7.68 seconds, 93 filtered `jig-sh` vault tests in 65.30 seconds, one 2.45-second PTY unlock/create/resize/lock/unlock/quit test, and warning-free strict all-target Clippy across all affected crates. Milestones 4–5 and the final repository gates remain.

Milestone 4 adds a state-aware Tools palette. Import parses and previews only safe mappings and collisions, supports a true no-`op` dry run, requires exact `IMPORT`, then reparses and rechecks current destinations immediately before the hardened owned resolver. Backup creation is private and no-clobber by default. Passphrase rotation updates the process credential only after the atomic core change. Activity and audit screens render verified whitelisted metadata without MAC material, while Linux restore is available only for an absent target and returns to the ordinary locked flow. Evidence is 209 `jig-vault` tests in 94.12 seconds, 23 `jig-vault-tui` tests in 7.67 seconds, 95 filtered `jig-sh` vault tests in 67.22 seconds, five `vault_import` integration tests in 53.57 seconds, one `vault_lifecycle` integration test in 118.07 seconds, the expanded PTY acceptance in 3.01 seconds, and warning-free strict all-target Clippy. Milestone 5 and the final repository gates remain.

Milestone 5 completes controlled output and session hardening. Canonical fields export through the core owner-only, symlink-refusing, atomic private-file sink with explicit overwrite and a fresh audited metadata snapshot. Peek requires exact confirmation, bypasses Ratatui, escapes controls, invalid UTF-8, directional formatting, and backslashes, emits at most the first 4 KiB, and clears after a key or ten seconds; only source byte count returns. Explicit and five-minute idle lock wait for the sole worker, and authentication or audit failure drops the session. Normal exit, Ctrl-C, SIGTERM, and panic clear the alternate screen before terminal restoration. Evidence is 209 `jig-vault` tests in 93.93 seconds, 6 `jig-tui` unit tests plus its direct-output panic PTY test, 31 `jig-vault-tui` tests in 7.89 seconds, 95 filtered `jig-sh` vault tests in 67.57 seconds, the 50.06-second lifecycle adapter test, two end-to-end Vault PTY tests in 3.36 seconds, and warning-free strict all-target Clippy. Documentation now states the exact disclosure and session boundaries. Final repository evidence follows.

Final verification uses development binary `target/debug/jig` from the committed source. `jig.fmt_check` passes as `receipt_01KZYV9GAHZ7X9CNVW937AJZQT`; workspace-wide locked `jig.clippy` passes as `receipt_01KZYV9TZCBJBHGYFA316XR4SD`; plan-linked contract passes as `receipt_01KZYV9EAPBX72PCQX5ZDSEXF8`; and the full two-part test command passes all 2,277 selected non-ignored tests as `receipt_01KZYV949AVVHT08FCCGN3BX39` in 826.568 seconds. Gate evidence is fresh through batch receipts `receipt_01KZYV9EBTSB5ZKY98TB368KXZ` and `receipt_01KZYV94B1SJZ1CK91H94RJC82`, with no unresolved gate. The completion scan finds no added ignored test, unfinished implementation macro, diff whitespace error, or fixture-value occurrence in Jig receipts/state; the only sentinel literals are deliberately local to tests that assert their absence from frames, errors, debug, audit projections, and PTY output outside the confirmed Peek window.

## Context and Orientation

The repository is a Rust workspace. `crates/jig-vault` owns encrypted state, validation, file locks, atomic persistence, redaction, audit records, controlled reads, execution, import primitives, passphrase rotation, and backup/restore. Its crate guide in `crates/jig-vault/AGENTS.md` is mandatory: listing APIs return metadata only; plaintext reveal and injection consume directly into a selected sink and finish lifecycle audit; encrypted state mutations append audit intent before saving; private paths refuse unsafe permissions and symlinks; and `SecretBytes` is the bounded zeroizing byte owner.

`crates/jig/src/cli/vault.rs`, `crates/jig/src/cli/vault_run.rs`, and `crates/jig/src/cli/command_conversion/vault.rs` define and adapt user-facing vault commands. `crates/jig/src/runtime/vault.rs` and its `lifecycle.rs`, `vault_import.rs`, and `vault_env.rs` modules own scope resolution, passphrase capture, CLI-specific preflight, 1Password process integration, and JSON/human output. The new TUI adapter belongs in this `jig` layer because it needs repository scope and external importer policy, but the renderer must not depend on `jig-sh`.

`crates/jig-tui` owns reusable Crossterm/Ratatui terminal lifecycle, sanitized display text, and cooperative worker ownership. `crates/jig-status-tui` is a read-only dashboard and `crates/jig-codex-tui` is a responsive picker. Both demonstrate the required split among `model.rs`, `render.rs`, and `runtime.rs`, deterministic Ratatui `TestBackend` tests, compact terminal layouts, and joining workers before terminal restoration. The Vault TUI must reuse this foundation without adding vault policy to `jig-tui`.

A canonical field reference is `jig://ITEM/FIELD`. Items are implicit groupings derived from field references; there is no separate durable item record. A field kind is either `Concealed` or `Text`; both are encrypted, while only concealed values participate in output redaction. A legacy secret is an entry manipulated by the older name-based API. Some legacy names happen to parse as canonical fields and must be deduplicated from the Legacy view; others must remain visible and manageable by exact legacy name.

An immediate sink is an output destination consumed inside the audited reveal call, such as an owner-only private file or a writer that immediately emits a safely escaped transient terminal preview. The TUI must never obtain a public plaintext value and later decide where to put it. A pseudo-terminal, abbreviated PTY, is a test-controlled terminal pair used to prove alternate-screen behavior, input handling, signal cleanup, and restoration.

## Plan of Work

### Milestone 1: Strengthen metadata and atomic management APIs

Extend `crates/jig-vault` before building presentation code. Add a single-open `VaultSnapshot` containing the authenticated format version, canonical `FieldRecord` values, only unrepresentable legacy `SecretRecord` values, and `AuditVerification`. Add `VaultActivityRecord`, a public metadata-only projection of verified audit events with event ID, timestamp, action, and sanitized domain metadata but no MAC keys or values. The snapshot and activity operations must verify the audit chain while the vault lock is held and must not return decrypted values.

Add atomic core methods for changing a field kind without changing its bytes, renaming or moving a field, renaming an item, removing an item, and converting one exact legacy entry into a canonical field. Each method validates every input and collision before audit append, performs one state save under one lock, preserves `created_at_ms`, updates `updated_at_ms` only where appropriate, and returns metadata-only results. A failed validation, collision, stale format, audit failure, or save preparation must leave state unchanged; an append followed by save failure retains the documented audit-leading-state behavior. Item removal must reject an empty/nonexistent selection or return an explicit unchanged result rather than pretending to delete. Legacy conversion must remove and create atomically so a crash cannot duplicate or lose the entry.

Add focused mutation, v1 compatibility, legacy-name, audit tamper, timestamp, collision, and no-plaintext tests in `crates/jig-vault/src/vault_tests`. Run `cargo test -p jig-vault` and strict crate Clippy. Commit this independently as the domain foundation.

### Milestone 2: Add the TUI foundation and complete browsing

Add `crates/jig-vault-tui` to the workspace and as an internal dependency of `crates/jig`. Create its crate-level `AGENTS.md`, `Cargo.toml`, and `src/{lib,model,render,runtime,secret_input}.rs`. Add it to `agent-map.md`. Keep it independent of `RepoContext`, CLI parsing, filesystem scope resolution, 1Password subprocesses, MCP, receipts, and JSON command output.

Define a narrow `VaultBackend` trait in `crates/jig-vault-tui/src/lib.rs`. It returns a public pre-unlock descriptor, accepts a protected passphrase for unlock or initialization, returns `VaultSnapshot` metadata, locks by dropping credentials, refreshes metadata, and accepts typed action requests. All action results visible to the model are metadata only. The `jig` implementation in `crates/jig/src/runtime/vault/tui.rs` owns the resolved `Vault`, fixed scope labels, process-local `SecretString`, and the mapping from TUI actions to core operations.

Add explicit `jig vault tui` CLI parsing, scope options, help, and `--json` rejection. Resolve and validate scope and require terminal stdin/stdout before capturing a passphrase or creating a home. Strip both reserved passphrase environment variables before any worker starts. Support a previously captured environment credential without showing it in the model; otherwise begin at a protected unlock form. Use the existing Unix process-wide signal supervisor so interruption requests cancellation, joins the current worker, drops secret state, restores the terminal, retires handlers, and only then re-delivers the signal.

Extend `jig-tui::TerminalSession` with paired bracketed-paste enablement/restoration if the feature-specific runtime needs it, keeping this as generic terminal mechanics. Implement model states for absent-vault initialization or restore choice, v1 read-only browsing and deliberate migration, locked, loading, browsing, help, confirmation, busy, and error. The wide renderer uses Items / Fields / Details, a persistent header with scope/home/lock/version/audit/count information, and a contextual footer. The compact renderer uses a focused pane and breadcrumb. Filtering searches item, field, reference, and legacy names only. Values never decrypt while moving selection.

Expose arrows plus `h/j/k/l`, `/` search, `Tab` focus, `Enter` open/accept, `Esc` back/cancel, `?` help, `:` tools, `r` refresh, `L` lock, and `q` quit. Preserve selection by exact `VaultReference` or exact legacy name across refreshes. Add pure model tests, wide/compact/minimum-size TestBackend tests, no-secret frame assertions, backend failure tests, and PTY smoke coverage for unlock, resize, lock, quit, and terminal restoration. Commit the browsing foundation after focused tests and Clippy pass.

### Milestone 3: Complete secret management

Implement `SecretInput` with a preallocated `SecretBytes` allocation capped at `MAX_SECRET_VALUE_LEN`, no `Clone`, redacted `Debug`, non-growing paste and typed insertion, character-aware deletion for valid UTF-8 input, exact byte loading from a bounded regular file, explicit zeroization on clear/drop, and bullet/count-only render data. Metadata fields use ordinary sanitized strings; secret values never share their type. Pair `EnableBracketedPaste` and `DisableBracketedPaste`, accept `Event::Paste` atomically, and reject the entire paste when it exceeds the cap rather than truncating silently.

Add canonical flows to create an item/field, add a field to an item, replace a value, change kind without revealing, rename/move a field, rename an item, remove a field, and remove an item. Add legacy create/replace/remove and atomic convert-to-field flows. Replacements start with an empty editor and clearly say the current value remains until Save. Permanent removals require typing the exact reference/name or `DELETE`; item removal shows the field count. Disable bulk delete and undo because current storage has no trash/version history. Refresh after every action and preserve stable identity selection. If a concurrent external CLI mutation invalidates the action, display a metadata-only stale/collision result and refresh rather than retrying a mutation automatically.

All backend calls run in at most one owned worker. Read-only work may observe cooperative cancellation. Once a mutation enters the core call it is non-cancellable; quit or signal displays a finishing state and joins before terminal restoration. Add tests for protected input zeroization and overflow, bracketed paste, every form transition, validations occurring before unlock when possible, metadata-only worker results, atomic core failure paths, concurrent external changes, and absence of sentinels in rendered buffers/errors/debug. Commit this as the management milestone.

### Milestone 4: Add lifecycle and import tools

Implement a Tools command palette and flows for one-time 1Password import, encrypted backup creation, passphrase change, audit verification, and absent-home restore. Implement an Activity view populated only through the new verified public projection. Import first parses and validates the dotenv mapping and destination path, previews canonical references and collisions without values, supports dry-run, replace, overwrite, and explicit final confirmation, and reuses the owned `op` process and recovery-command behavior from `crates/jig/src/runtime/vault_import.rs`. Never place resolved `op` values in action results.

Backup creation preflights a hardened private output and makes overwrite explicit. Passphrase change uses two independent protected inputs, validates confirmation, rotates through the existing atomic lifecycle, replaces the in-process credential only after success, and immediately refreshes. Audit verification shows event count and torn tail; Activity shows action/time/safe reference metadata. Restore appears only when the complete selected target home is absent, remains Linux-only while the core API is Linux-only, accepts a bounded encrypted backup file, repeats the absent-home preflight at commit, and enters the normal unlock flow after installation. Existing homes are never overwritten.

Add focused fake-`op`, dry-run/collision, backup no-clobber, passphrase failure/success, audit tamper, activity sanitation, and absent-target restore tests. Extend PTY acceptance to exercise the Tools palette without logging values. Commit this independently.

### Milestone 5: Controlled output and hardening

Add private-file export for canonical fields through `Vault::read_field_to_file`, including path preflight, explicit overwrite confirmation, byte-count-only completion, and lifecycle audit parity. Legacy entries without canonical references cannot be exported until converted because the core deliberately has no legacy plaintext accessor; explain that in the Legacy details/actions.

Add an explicit Peek action for canonical fields. Before revealing, show a warning that terminal scrollback, multiplexers, and screen recording are external sinks. On confirmation, temporarily stop Ratatui drawing, pass a writer directly to `Vault::read_field_to`, escape control bytes and invalid UTF-8 into visible terminal-safe notation, bound displayed output, wait for one key or a short timeout, overwrite/clear the alternate screen, and redraw only metadata. The writer and temporary buffers use fixed-capacity zeroizing storage, and no plaintext or escaped copy returns in the action result. Binary values display escaped bytes rather than emitting terminal controls. Record matching read start/finish/failure audit events. Do not add OSC52 or native clipboard support in this plan.

Implement explicit `L` lock and a default five-minute input-inactivity lock. Locking drops credentials and pending protected forms, clears snapshot metadata from the model, cancels cancellable work, waits for mutations, and returns to the unlock screen. External passphrase rotation or authentication failure must fail closed into the locked state. Add tests for timer reset, timer expiration, pending form wiping, worker shutdown ordering, Ctrl-C and Unix signal behavior, panics, terminal restoration, 608x113 large-terminal behavior, and no values in stdout/stderr/audit/receipts/repository files. Update CLI help, `docs/configuration.md`, `docs/public-contract.md` where relevant, root/crate guides, and the older general-purpose vault plan's explicit scope note so the new interactive controlled sinks are accurately documented without claiming clipboard or daemon support.

Finish by reviewing every diff and running the development binary through the full Jig workflow. Commit the final slice only after focused tests pass; then run full gates on the committed source and make a dedicated verification/documentation commit only if gate receipts or living-plan updates change tracked files.

## Concrete Steps

Work from `/home/aa/.herdr/worktrees/jig-sh/feat-vault-tui`.

1. Open structured work and record its identifier in `Progress`:

       plan_id="plan_01KZYHNEMNNN5B80EDBFA2WDJ9"
       scripts/jig work status

2. Implement and verify Milestone 1:

       cargo fmt --all
       cargo test -p jig-vault
       cargo clippy -p jig-vault --all-targets -- -D warnings
       git diff --check
       git status --short
       git commit -m "feat(jig-vault): add atomic TUI management APIs"

3. Implement and verify Milestone 2:

       cargo test -p jig-tui
       cargo test -p jig-vault-tui
       cargo test -p jig-sh vault --lib
       cargo clippy -p jig-tui -p jig-vault-tui -p jig-sh --all-targets -- -D warnings
       cargo fmt --all --check
       git diff --check
       git commit -m "feat(jig): add vault TUI browser"

4. Implement and verify Milestone 3:

       cargo test -p jig-vault
       cargo test -p jig-vault-tui
       cargo test -p jig-sh vault --lib
       cargo clippy -p jig-vault -p jig-vault-tui -p jig-sh --all-targets -- -D warnings
       cargo fmt --all --check
       git diff --check
       git commit -m "feat(jig): manage vault secrets in the TUI"

5. Implement and verify Milestone 4:

       cargo test -p jig-vault
       cargo test -p jig-vault-tui
       cargo test -p jig-sh vault --lib
       cargo test -p jig-sh --test vault_import -- --nocapture
       cargo test -p jig-sh --test vault_lifecycle -- --nocapture
       cargo clippy -p jig-vault-tui -p jig-sh --all-targets -- -D warnings
       cargo fmt --all --check
       git diff --check
       git commit -m "feat(jig): add vault TUI lifecycle tools"

6. Implement and verify Milestone 5:

       cargo test -p jig-vault
       cargo test -p jig-vault-tui
       cargo test -p jig-sh vault --lib
       cargo test -p jig-sh --test vault_tui -- --nocapture
       cargo clippy -p jig-vault -p jig-tui -p jig-vault-tui -p jig-sh --all-targets -- -D warnings
       cargo fmt --all --check
       git diff --check
       git commit -m "feat(jig): harden controlled vault TUI output"

7. Build the changed runtime and force the repo harness to exercise it:

       cargo build -p jig-sh --bin jig
       export JIG_DEV_BIN=target/debug/jig
       scripts/jig work check --plan-id "$plan_id"
       scripts/jig work gates --plan-id "$plan_id"
       scripts/jig work evidence --plan-id "$plan_id"
       scripts/jig work receipts --plan-id "$plan_id"
       scripts/jig work status

   Also run the explicit configured checks so their outcomes are visible even if a structured gate stops after one failure:

       scripts/jig check fmt
       scripts/jig check clippy
       scripts/jig check contract
       scripts/jig check test

8. Conduct the completion audit described below, update every living-plan section, inspect `git diff HEAD` and `git status --short`, commit any tracked plan/receipt updates, and finish structured work only when all acceptance evidence is green:

       scripts/jig work finish --plan-id "$plan_id" \
         --resolution "Vault TUI management, lifecycle tools, controlled output, hardening, and full repository verification completed"

## Validation and Acceptance

Completion requires behavior, not merely compiling types. In a temporary private vault home, the PTY acceptance test must initialize and unlock a v2 vault; create concealed and text fields; show item, field, kind, length, and timestamps without plaintext; search and navigate wide and compact layouts; replace a value without preloading the old value; change kind; rename a field and item; delete with typed confirmation; create, replace, convert, and remove a legacy entry; lock and unlock; and restore the terminal on quit, panic, Ctrl-C, and an external Unix termination signal.

The Tools acceptance must import from a deterministic fake `op`, show references and collisions without values, preserve dry-run, create an encrypted backup without clobbering, rotate the passphrase while invalidating the old one, show a verified metadata-only Activity view, reject a tampered audit chain, and restore only into an absent target. Existing CLI commands and JSON contracts must remain unchanged except for the additive explicit TUI command.

Controlled output acceptance must export exact bytes to an owner-only regular file, reject unsafe/symlink destinations, require overwrite confirmation, and return only byte counts. Peek must display printable content plus escaped control/binary bytes only after warning and confirmation, never place those bytes in a Ratatui frame, clear the alternate screen before returning, and produce matching lifecycle audit events. A sentinel scan must prove the value does not occur in command-line arguments, environment after capture, stdout/stderr outside the confirmed Peek window, logs, errors, debug strings, audit JSONL, Jig receipts, repository files, or Git diff.

Concurrency acceptance requires two independent `Vault` handles. If an external CLI operation changes state after the TUI snapshot, a TUI mutation must reopen under the lock and either apply safely against current state or return an explicit collision/stale error; it must never overwrite an unrelated change from cached decrypted state. Quit during a mutation must wait until its audited atomic operation finishes. Wrong credentials or external passphrase rotation lock the session rather than continuing with stale material.

The full workspace scope is proved only by successful `scripts/jig check fmt`, `scripts/jig check clippy`, `scripts/jig check contract`, and `scripts/jig check test` using `JIG_DEV_BIN=target/debug/jig`, plus a clean `git diff --check`. Focused tests are milestone evidence, not substitutes for the final full test suite. Record actual pass counts, platform-specific skips, elapsed time, receipt identifiers, and final commit identifiers in `Outcomes & Retrospective`. Any unexpected failure must be diagnosed and fixed or explicitly shown to be an unchanged external/environmental baseline; this plan does not pre-authorize ignoring known failures.

## Completion Audit

Before closing the plan, inspect current source and test evidence against each original requirement. Confirm that the explicit `jig vault tui` command exists and is documented; all representable fields and unrepresentable legacy entries are visible; every requested create, replace, kind, rename, delete, convert, import, backup, passphrase, audit, restore, export, Peek, lock, and refresh action is reachable and tested; `exec`, `run`, and `inject` remain working CLI workflows; fixed scope is displayed; wide and compact rendering work; protected input never enters frames; all workers and signals restore the terminal; and the four full repo checks pass. Search for unfinished placeholders such as `todo!`, `unimplemented!`, disabled tests, ignored tests added by this work, and stale scope documentation. Treat missing or indirect evidence as incomplete and continue implementation rather than closing structured work.

## Idempotence and Recovery

Source edits and tests are repeatable. Vault tests use temporary owner-only directories and must not touch a real `~/.jig/vault`. Manual demonstrations must pass an explicit temporary `--home`; never delete or overwrite a user's existing vault. Restore tests create a new absent child beneath a private temporary parent. Import tests use a fake `op` executable and bounded fixture values.

Every mutation retains existing core locks, audit ordering, symlink refusal, and atomic writes. Retrying after validation or preflight failure is safe because no state changed. If an audit intent was appended and the state save failed, do not silently retry from the TUI; surface the metadata-only failure, lock or refresh as appropriate, and rely on existing audit verification/recovery semantics. If implementation is interrupted, read this whole plan, inspect `git status`, resume from the first unchecked `Progress` item, and do not discard unrelated user changes or rewrite append-only `.agent/state/*.jsonl` files.

If a PTY test leaves a process, terminate only the exact PID recorded in that test's temporary directory. `CooperativeWorker` must join on drop, so ordinary test cleanup should not leak a thread. If raw mode or alternate-screen entry partially fails, `TerminalSession` must reverse every completed setup command before returning the error. Bracketed paste must be disabled on every ordinary return and unwind.

Commits are additive checkpoints. Do not use destructive resets. If a milestone commit needs correction, make a follow-up commit unless the commit has not yet been shared and the current worktree contains only that milestone's known files. Before each commit, inspect the complete staged diff and ensure append-only work records are included only when generated by this plan.

## Artifacts and Notes

The intended wide interface is conceptually:

    Vault: repo jig-sh | unlocked | v2 | audit verified | 12 fields
    + Items ------------+ Fields ----------------+ Details ----------------+
    | > Production      | > RESTIC_PASSWORD      | jig://Production/...    |
    |   Staging         |   API_URL       [text] | concealed               |
    |   CI              |   DATABASE_URL         | 32 bytes                 |
    |   Legacy (2)      |                        | updated 12m ago          |
    +-------------------+------------------------+--------------------------+
    / filter  a add  e replace  D delete  x export  : tools  L lock  ? help

Values are never substituted into this frame. During protected entry the value area shows bullets and a byte count. During Peek, Ratatui drawing is suspended and a direct writer produces a warning-labelled transient view, then clears it before metadata rendering resumes.

Expected CLI behavior includes:

    $ scripts/jig vault tui --json
    error: --json is not supported by vault tui; use the interactive terminal interface

    $ scripts/jig vault tui </dev/null
    error: `jig vault tui` requires terminal input and output; use `jig vault field list` for non-interactive metadata

The exact wording may follow existing CLI style, but it must mention the explicit command and a useful non-interactive fallback without exposing a secret.

## Interfaces and Dependencies

In `crates/jig-vault/src/vault.rs` and exports from `crates/jig-vault/src/lib.rs`, provide metadata-only types with semantics equivalent to:

    pub struct VaultSnapshot {
        pub format_version: u32,
        pub fields: Vec<FieldRecord>,
        pub legacy_secrets: Vec<SecretRecord>,
        pub audit: AuditVerification,
    }

    pub struct VaultActivityRecord {
        pub event_id: String,
        pub timestamp_ms: i128,
        pub action: String,
        pub subject: Option<String>,
        pub outcome: Option<String>,
    }

    pub fn snapshot(&self, passphrase: &SecretString) -> Result<VaultSnapshot>;
    pub fn activity(&self, passphrase: &SecretString, limit: usize)
        -> Result<Vec<VaultActivityRecord>>;
    pub fn change_field_kind(... ) -> Result<FieldBatchResult>;
    pub fn rename_field(... ) -> Result<FieldBatchResult>;
    pub fn rename_item(... ) -> Result<FieldBatchResult>;
    pub fn remove_item(... ) -> Result<FieldBatchResult>;
    pub fn convert_legacy_secret(... ) -> Result<FieldBatchResult>;

Names may be refined to fit existing domain types, but callers must never perform read/remove/set compositions with plaintext to emulate these atomic operations. Activity parsing must occur only after chain verification and must whitelist safe metadata rather than returning arbitrary audit `serde_json::Value` details.

In `crates/jig-vault-tui/src/lib.rs`, define a same-release boundary equivalent to:

    pub trait VaultBackend: Send + Sync + 'static {
        fn descriptor(&self) -> VaultDescriptor;
        fn unlock(&self, passphrase: SecretBytes) -> Result<VaultSnapshot, VaultUiError>;
        fn initialize(&self, passphrase: SecretBytes) -> Result<VaultSnapshot, VaultUiError>;
        fn lock(&self);
        fn refresh(&self) -> Result<VaultSnapshot, VaultUiError>;
        fn execute(&self, action: VaultAction) -> Result<VaultActionResult, VaultUiError>;
        fn peek(
            &self,
            reference: &VaultReference,
            output: &mut dyn Write,
        ) -> Result<usize, VaultUiError>;
    }

    pub fn run(
        backend: impl VaultBackend,
        initial_passphrase: Option<SecretBytes>,
        cancelled: impl Fn() -> bool + Send + Sync + 'static,
    ) -> anyhow::Result<()>;

`VaultAction` owns protected values as `SecretBytes` and ordinary validated metadata separately. Its `Debug` implementation must redact or omit every protected payload. `VaultActionResult` contains only snapshots, references, counts, paths already selected by the operator, and safe status messages. Errors are typed or sanitized and never echo values.

Use existing workspace `ratatui`, `crossterm`, `unicode-width`, `secrecy`, and `zeroize` versions. Prefer existing `SecretBytes`, `PreparedPrivateFile`, `CooperativeWorker`, signal supervision, and owned-process importer machinery. Add no daemon, network service, public plaintext accessor, generic external editor, OSC52 integration, or clipboard dependency. Any newly necessary dependency must be documented here with license, minimum Rust version, and why workspace code cannot provide the behavior before it is added.

Revision note (2026-08-13): Created this plan from the completed Vault v2 and TUI design investigation. It preserves all five recommended delivery stages, makes legacy visibility and controlled plaintext sinks explicit, and turns each stage into a testable commit boundary.

Revision note (2026-08-13): Recorded structured work `plan_01KZYHNEMNNN5B80EDBFA2WDJ9` after opening it with a body that points back to this authoritative plan.

Revision note (2026-08-13): Recorded Milestone 1 implementation and evidence after adding the metadata-only snapshot/activity boundary and atomic management transformations required by the TUI.

Revision note (2026-08-13): Recorded Milestone 2 implementation and evidence after adding the feature crate, CLI/session boundary, responsive metadata browser, protected credential editor, and PTY terminal-restoration coverage.

Revision note (2026-08-13): Recorded Milestone 3 implementation and evidence after adding protected value forms, complete canonical and legacy management workflows, lock-scoped stale-state preconditions, hardened binary file input, and destructive confirmation.

Revision note (2026-08-13): Recorded Milestone 4 implementation and evidence after adding import preview/commit, backup, passphrase rotation, verified activity/audit views, and absent-home restore.

Revision note (2026-08-14): Recorded Milestone 5 implementation and evidence after adding controlled export/Peek sinks, idle and fail-closed locking, terminal cleanup hardening, large-terminal protection, PTY coverage, and operator documentation.

Revision note (2026-08-14): Corrected the Vault integration-binary partition, recorded the complete 2,277-test run and final fresh gate receipts, and completed the diff, placeholder, and fixture-leakage audits.
