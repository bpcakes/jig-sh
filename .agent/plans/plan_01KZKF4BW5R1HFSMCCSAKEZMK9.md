Implement three behavior-preserving refactor slices in order: encapsulate brokered-run audit lifecycle, extract brokered process supervision modules, then split vault envelope parse/validate/unlock/seal phases. Preserve public API, persisted JSON, audit ordering, error behavior, zeroization, process cleanup, platform cfg behavior, and Rust 1.85 compatibility. Commit each green slice separately. Validate narrowly after each slice, then run the full configured gates and platform-sensitive CI checks where locally available.
## Refactor jig-vault safety boundaries

Implement three behavior-preserving Rust refactors in crates/jig-vault, in this
order: (1) make the brokered-run audit lifecycle explicit, (2) split process
supervision out of run.rs, and (3) split vault-envelope decoding and crypto
phases from VaultStore orchestration. Each completed slice is a separate Git
commit. The work deliberately changes structure, not user-observable behavior.

## Progress

- [x] Read repository and jig-vault guidance, CI policy, the Fowler Rust
  refactoring guidance, and this plan contract.
- [x] Open this structured work item: plan_01KZKF4BW5R1HFSMCCSAKEZMK9.
- [x] Record a clean pre-refactor baseline for the focused crate: format,
  check, all 97 tests, and Clippy passed.
- [x] Slice 1: introduce a closed brokered-run stage enum and state-owning
  started/prepared lifecycle helpers; add exact wire-level stage tests; run
  focused and crate checks; commit.
- [x] Slice 2a: extract secret-file delivery unchanged; run focused checks.
- [x] Slice 2b: extract capped pipe capture unchanged; run focused checks.
- [x] Slice 2c: extract BrokeredProcess and platform implementations under
  src/run/; run focused and crate checks and Clippy; inspect and commit the
  complete slice.
- [x] Slice 3a: extract parse/decode and header-validation phases behind private
  types while preserving validation order; run focused tests.
- [x] Slice 3b: extract unlock/decrypt, new-envelope seal, and state-reseal
  phases while preserving crypto/AAD/zeroization semantics; run focused tests.
- [x] Slice 3c: keep filesystem and audit orchestration in VaultStore; run
  crate checks and Clippy; inspect and commit the complete slice.
- [ ] Run final crate, jig-sh (if feasible), and Jig harness checks; record
  evidence without finishing this work session.

## Surprises & Discoveries

- The active plan was created by scripts/jig work start as a zero-newline
  one-line file. It is expanded here before implementation so a restart does
  not depend on chat context.
- The workspace has Rust edition 2024 and a declared rust-version = 1.85.
  CI runs cargo check --workspace --all-targets --all-features --locked with
  Rust 1.85 and runs jig-vault tests on macOS and Windows.
- The initial worktree already contains the work-session state, receipts, and
  this plan. Those files are in scope and must be preserved; they are not
  unrelated changes to discard.
- The focused baseline was green: cargo fmt --all -- --check, cargo check -p
  jig-vault --all-targets --locked, cargo test -p jig-vault --locked (97 tests),
  and cargo clippy -p jig-vault --all-targets --locked -- -D warnings.
- Slice 1's focused brokered-run test filter passed 18 tests after the structural
  move; the complete crate suite then passed 98 tests after adding the process
  failure characterization test.
- Slice 2's secret-file and capped-output moves each compiled and passed their
  focused tests before the process move. A mechanical process extraction left a
  trailing cfg attribute during the first format pass; it was corrected before
  the combined module compiled, so no behavioral check was run against that
  malformed intermediate state.
- Slice 2's combined run tests passed: 18 brokered-run, 6 Linux-group, and 6
  Unix-group focused cases; the full jig-vault suite passed all 98 tests.
  cargo check and Clippy with warnings denied passed, as did a Rust 1.85
  all-target check. Only Linux and wasm targets are installed locally.
- The Jig no-mod-rs harness check currently reports the pre-existing tracked
  crates/jig/tests/support/mod.rs. This slice introduced no mod.rs; the
  changed-file LOC harness check passed.
- Slice 3's focused header/KDF ordering and nonce-rotation tests passed. The
  full jig-vault suite passed all 100 tests; cargo check, Clippy with warnings
  denied, format, diff checks, and a Rust 1.85 all-target check passed.
  The first draft of the new ordering tests asserted source-only detail through
  Display; switching to anyhow's alternate chain format correctly tests the
  existing source-preserving error contract.

## Decision Log

- 2026-08-09: Keep VaultFile, VaultHeader, VaultState, public Vault methods,
  and audit record serialization unchanged. Their wire and semver contracts are
  behavior, not implementation detail.
- 2026-08-09: Use private enums/structs and module boundaries rather than trait
  objects, dynamic dispatch, or new dependencies. The variant set and platform
  dispatch are closed and compilation-time selected.
- 2026-08-09: Do not use Drop to append brokered-run terminal audit events. An
  unmatched start after abrupt process death is an intentional audit model, and
  I/O/locking from Drop would alter it.
- 2026-08-09: Preserve existing error construction, operation order, and
  SAFETY documentation mechanically while moving code. Refactors may not change
  error precedence, lock duration, cleanup deadlines, or unsafe preconditions.
- 2026-08-09: Use files such as src/run/secret_files.rs, src/run/output.rs, and
  src/run/process.rs; do not create a mod.rs, per repository policy.
- 2026-08-09: The private BrokeredRunStage enum is the sole production mapping
  to the persisted resolve and process stage strings. StartedBrokeredRun owns
  both locked-resolution failure recording and unlocked terminal audit state;
  PreparedBrokeredRun consumes itself to execute and record normal completion.
- 2026-08-09: Keep run.rs as orchestration and its existing integration tests
  as the stable helper-test owner. Private process, Unix, Linux, Windows,
  output, and secret-file modules expose only pub(super) mechanisms. This keeps
  compile-time dispatch and test helper paths unchanged while avoiding a
  forbidden mod.rs directory root.
- 2026-08-09: Make vault/envelope.rs a child of vault.rs so every phase type is
  pub(super), rather than expanding crate-visible implementation APIs. Decode
  operations remain interleaved with unlock in their original order because
  moving all base64 decoding ahead of KDF or authenticated decryption would
  change error precedence.
- 2026-08-09: New and resealed envelope values retain the existing zeroizing
  plaintext across the audit/write boundary. The field order intentionally
  preserves the prior sensitive-material drop order while moving no plaintext
  into new allocations.

## Outcomes & Retrospective

To be completed after all three commits and final verification. It must list the
commit IDs, exact check results, unrun platform/MSRV coverage, any retained
work-session files, and any deviations from this plan.

## Context and orientation

jig-vault is the machine-local encrypted vault, audit, redaction, and
brokered-child-process boundary used by the Jig runtime. Its public crate
surface begins in crates/jig-vault/src/lib.rs; the affected implementation areas
are:

- src/vault.rs: lock acquisition, vault persistence, audit sequencing,
  encrypt/decrypt orchestration, and brokered run preparation.
- src/run.rs: command setup, temporary secret delivery, capped stdout/stderr
  capture, retained child-tree identity, platform-specific cleanup, and tests.
- src/format.rs / src/crypto.rs: persisted encrypted DTOs and cryptographic
  primitives. They are callers/dependencies to preserve, not rewrite targets
  unless a private phase boundary needs a narrowly scoped helper.
- src/audit.rs: JSONL audit record layout. Its field values and record order are
  persistent behavior.

The scope is internal jig-vault structure plus focused characterization tests.
Exclude feature work, API changes, format migrations, test-only cleanup that
does not prove an invariant, and unrelated workspace changes.

### Observable invariants

1. Public Rust signatures, visibility, error kind/text/source behavior, and
   Debug/secret-redaction behavior stay unchanged.
2. vault.json fields, JSON shape, base64 representation, header AAD bytes,
   payload roles, nonce freshness, KDF parameters, and decryption/validation
   error ordering stay unchanged.
3. audit.jsonl event kind/detail strings stay unchanged. A BrokeredRunStart is
   appended under the vault lock before resolution; resolution failures append a
   resolve failure; process failures append a process failure; success/failure
   is appended only after child execution; unrelated audited operations may
   interleave, and abrupt death may leave a start unmatched.
4. The vault lock is deliberately released before child execution. No new lock
   widening, background work, Drop I/O, or terminal-event synthesis is allowed.
5. Secret values remain zeroized on the same ownership/drop paths. Do not make
   additional plaintext copies or accidental growing reads.
6. Process ownership remains compile-time platform selected. Child identity is
   established before execution; existing leader observation/reap rules,
   deadline checks, group/job cleanup, output EOF/caps, error precedence, and
   SAFETY explanations remain byte-for-byte equivalent except for location.
7. The result compiles on Rust 1.85. No mod.rs may be created.

## Plan of work

### Baseline and restart protocol

Before editing code, inspect git status --short, read the exact affected
functions and callers, and run the documented focused baseline:

    cargo fmt --all -- --check
    cargo check -p jig-vault --all-targets --locked
    cargo test -p jig-vault --locked
    cargo clippy -p jig-vault --all-targets --locked -- -D warnings

If a command is red before the edit, record it in Surprises and do not attribute
that failure to a later slice. The working tree contains structured-work files;
never reset or checkout them. If interrupted, inspect git status, the latest
commit, this plan's Progress, and the last scripts/jig work evidence receipt;
resume at the first unchecked item. Each small extraction must compile before
the next extraction begins.

### Slice 1 — brokered-run audit lifecycle encapsulation

Goal: replace raw failure stage literals with a closed private enum that
serializes to the exact current literals, and make the start → resolve → execute
→ terminal-audit lifecycle live in internal state-owning values rather than in
scattered procedural branches.

Expected target: in src/vault.rs (or a private sibling if the ownership is
clearer), define a BrokeredRunStage enum with Resolve and Process, plus a method
returning exactly resolve / process. Define private StartedBrokeredRun and
PreparedBrokeredRun values carrying only the vault, run ID, and already-resolved
execution request needed for their legal next transition. The prepared value
owns completion/failure recording during normal execution. No type is public and
Vault::run_brokered remains unchanged.

Concrete small steps:

1. Locate existing brokered lifecycle tests and add only characterization cases
   that assert run-id correlation, exact stage strings, success/process failure,
   resolution failure, and allowed event interleaving if they are absent.
2. Add BrokeredRunStage, make the existing failure append helper take it, and
   prove the JSON output remains exact with focused tests.
3. Introduce the started value around the existing locked start/resolve sequence.
   Keep lock guard lifetime explicit; do not let the type accidentally retain it.
4. Introduce the prepared value around the existing unlocked child execution
   match. Move code without changing the success/failure branches.
5. Remove only the now-unused duplicated helper paths. Run focused vault /
   brokered tests, then check/test/Clippy for jig-vault.
6. Inspect git diff --check, formatted diff, and staged diff. Commit with a
   focused refactor(jig-vault): encapsulate brokered run audit lifecycle message.
   Update this plan before staging it if structured-work files belong in this
   commit; otherwise preserve them for the next explicit work commit.

Failure/recovery: a diff in stage strings, record count/order, or lock scope
means revert only the current unstaged small step, return to the prior green
commit/state, and use the original helper as a temporary delegation point. Do
not refactor event representation or add terminal behavior to Drop.

### Slice 2 — extract process-supervision modules

Goal: make src/run.rs the high-level orchestration façade while placing
cohesive, behavior-identical mechanisms under src/run/ without dynamic dispatch
or mod.rs.

Expected target: run.rs declares private file modules. Candidate owners are
run/secret_files.rs for temporary secret delivery and cleanup, run/output.rs for
pipe readers/capped capture/zeroizing buffers, and run/process.rs for
BrokeredProcess plus private cfg platform modules such as run/process_unix.rs,
run/process_linux.rs, run/process_macos.rs, and run/process_windows.rs if that
layout preserves direct access and avoids a directory module's forbidden mod.rs.
The exact names can vary only if Rust module resolution and ownership make a
safer no-mod.rs topology.

Concrete small steps:

1. Record run focused test names and compile baseline. Move the secret-file
   struct/impl/tests unchanged into a private module; compile and run its focused
   tests.
2. Move pipe-drain/output capture unchanged. Preserve cap arithmetic, EOF,
   thread blocking/join behavior, temporary buffer zeroization, and all error
   precedence. Run focused tests.
3. Introduce a private module boundary for BrokeredProcess with the existing
   façade and cfg implementations. Move one OS region at a time and compile
   after each move.
4. Preserve every unsafe block and its SAFETY text, Unix wait/kill/reap ordering,
   Linux deadline probing, macOS identity proof, Windows job cleanup, and all
   target imports. Do not consolidate platform logic merely for shape.
5. Move tests to the owning module only when doing so keeps private access and
   makes the ownership clearer; keep integration-level behavior tests in run.rs.
6. Run cargo fmt, focused tests, all jig-vault tests, check, and Clippy. Inspect
   the diff and commit with refactor(jig-vault): extract brokered process
   supervision.

Failure/recovery: cross-platform imports are easy to break invisibly on Linux.
If a target-specific compile is unavailable, use conditional imports and do not
alter code beyond mechanical moves; record the unrun target. If an extraction
changes a deadline or cleanup branch, stop at the last green move instead of
simplifying it.

### Slice 3 — split validated vault-envelope phases

Goal: keep VaultStore responsible for locks, files, and audit ordering; move
pure/crypto envelope transformation into private, phase-typed operations.

Expected target: keep serde DTOs in format.rs exactly as they are. Add private
values/functions in vault.rs or a private vault_envelope.rs sibling that model:
decoded file bytes; decoded and validated envelope/header; unlocked
envelope/state; new envelope sealing; and resealing state with a fresh nonce.
The types must not expose unvalidated header/ciphertext data to downstream
operations and must use the existing zeroizing ownership wrappers.

Concrete small steps:

1. Identify or add precise tests for malformed JSON, invalid header/version,
   base64/fixed-array decoding, swapped payload roles/AAD, wrong passphrase,
   ciphertext tampering, nonce freshness, and audit-before-save rollback.
2. Extract parse plus fixed-array/base64 decode while retaining the current
   caller's exact validation order and every existing VaultError creation.
3. Introduce the validated phase type only after the identical header checks.
   It should own decoded data so later operations cannot repeat decode or bypass
   validation.
4. Extract key derivation, key unwrap, state decrypt/parse, and audit-key
   derivation into the unlock phase. Preserve source errors and zeroization; do
   not clone plaintext for convenience.
5. Extract new-vault sealing and existing-state resealing from I/O orchestration.
   Retain header bytes/AAD roles, new nonce generation, KDF behavior, and
   serialization output unchanged.
6. Leave VaultStore methods visibly responsible for lock → audit intent →
   atomic vault write → rollback sequencing. Run focused tests, then all crate
   checks and Clippy. Inspect and commit with refactor(jig-vault): split
   validated vault envelope phases.

Failure/recovery: persisted format, cryptographic validation sequence, and error
priority are compatibility boundaries. If any characterization test changes,
undo the current unstaged extraction and reintroduce a private wrapper that
delegates to original ordering. Never use a format migration or change a public
error just to make the types prettier.

## Validation and acceptance

After each slice, run the narrow tests touched by that slice plus:

    cargo fmt --all -- --check
    cargo check -p jig-vault --all-targets --locked
    cargo test -p jig-vault --locked
    cargo clippy -p jig-vault --all-targets --locked -- -D warnings

After all commits, run the above again, then, if local time/tooling permits:

    cargo test -p jig-sh --locked
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01KZKF4BW5R1HFSMCCSAKEZMK9
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01KZKF4BW5R1HFSMCCSAKEZMK9
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01KZKF4BW5R1HFSMCCSAKEZMK9
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01KZKF4BW5R1HFSMCCSAKEZMK9
    JIG_DEV_BIN=target/debug/jig scripts/jig work status

Also run the no-mod.rs check through the Jig harness. If Rust 1.85 or
macOS/Windows targets are not installed, report those precisely; CI remains the
authoritative cross-platform/MSRV proof. Acceptance means three independent
commits in the mandated order, no mod.rs, no API/wire/audit/crypto/process
semantic drift, all locally runnable checks green, and an open—not finished—work
session for the primary agent's independent audit.

## Idempotence and recovery

These are source-only moves. They must neither read nor rewrite a user's real
vault during tests beyond existing temporary fixtures. Rerunning commands is
safe. Reapplying an extraction after interruption requires first checking
whether the target symbol/module already exists and resuming from its next
unchecked substep; do not duplicate modules or tests. Each commit is a clean
rollback boundary: to abandon a later slice, reset/revert only that slice after
first preserving structured-work receipts; do not erase the active plan/session
state.

## Interfaces and dependencies

- Public interface retained: all existing exports from jig_vault, including
  Vault brokered-run entry points and public error types.
- Persistent interfaces retained: vault.json serde structs/field names and
  audit.jsonl record JSON, including literal stage values.
- Internal lifecycle interface: a private closed BrokeredRunStage maps to the
  two existing audit detail values; started/prepared values make invalid normal
  transitions hard to represent while retaining the intentional abrupt
  termination gap.
- Internal process interface: run.rs remains the single high-level
  orchestration caller; extracted modules expose only pub(super)/private values
  needed by it. Platform selection remains cfg based.
- Internal envelope interface: private phase types own decoded/validated
  representations and expose explicit parse, validate, unlock, seal, and
  reseal transitions; VaultStore remains the only I/O/audit coordinator.
- Dependencies: do not add a crate. Existing zeroize, chacha20poly1305, Argon2,
  serde, libc, and windows-sys behavior remains the implementation base.
