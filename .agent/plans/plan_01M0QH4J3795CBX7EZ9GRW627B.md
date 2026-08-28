# Complete repository execution boundaries

Harden the contract-v6 repository runtime after a comprehensive branch review found six boundary failures. The observable result is that every configured command is non-interactive, uses the repository execution timeout and fatal output limit, schema checks work before the first commit, durable affected plans stay bounded, public-artifact checks use declared frontend roles, and the regression suite no longer mutates process-global Git routing during parallel tests.

## Progress

- [x] (2026-08-23) Read repository guidance, the contract-v6 design plan, later review plans, public-contract documentation, and the affected implementation and tests.
- [x] (2026-08-23) Classified the findings: execution supervision and affected-plan growth expose incomplete owning abstractions; schema unborn support, frontend role use, and test isolation are local boundary omissions.
- [x] (2026-08-23) Centralized authoritative configured-command supervision and routed contract-v6 target execution through it, including retained overflow diagnostics.
- [x] (2026-08-23) Made schema Git probes environment-safe and added an unborn-repository snapshot path.
- [x] (2026-08-23) Bounded persisted selection reasons with deterministic completeness metadata and shared reason batches.
- [x] (2026-08-23) Made generated web checks apply artifact policy from the declared frontend role.
- [x] (2026-08-23) Isolated the ambient-Git regression in a dedicated test subprocess.
- [x] (2026-08-23) Refreshed embedded templates and passed focused regressions, formatting, configured Clippy, all 2,856 configured tests, contract validation, and structured-work gates/evidence.
- [x] (2026-08-24) Reworked receipt overflow as bounded uncertainty, added source/effect epochs and post-run fingerprint revalidation, and made execution-authority hashing preserve forward-compatible manifest fields.
- [x] (2026-08-24) Made scaffolded frontend contracts an explicit rendering capability so adopted repositories cannot receive always-green required gates; added fail-closed runner checks and end-to-end scaffold/adoption regressions.
- [x] (2026-08-24) Completed the Claude Opus plus independent Codex comprehensive branch review, fixed every reported issue, and repeated fresh native Codex passes after each later finding until the final pass reported no issue.
- [x] (2026-08-24) Made the Vault PTY fixture own its private controlling terminal, reproduced the original gate failure under a TTY-wrapped runner, and repeated the native review after the fix with no remaining finding.
- [x] (2026-08-24) Passed the final isolated full-workspace Jig check, fresh contract/test gates and evidence, strict workspace Clippy, formatting, launcher-template parity, and diff validation.

## Surprises & Discoveries

- Contract v6 reused the existing `[commands]` table, but its new runner called the low-level owned-process primitive directly. That bypassed the older configured-command timeout and fatal-output policy and showed that the policy API did not make stdin/capture obligations unskippable.
- `jig init` creates a Git repository without an initial commit, while the documented first validation pass can include schema checking. The schema snapshot implementation nevertheless assumed `git stash create` always had a `HEAD` commit.
- Selection reasons are part of the queued run event. An unbounded reason list therefore amplifies both planning memory and append-only durable state, and the reverse JSONL scanner magnifies the cost.
- The frontend model already carries the semantic role (`spa`, `admin`, or `astro`); only the generated shell boundary discarded it and treated every bundle as public.
- A regression that changes `GIT_DIR` process-wide can race unrelated test binaries even when guarded by a lock local to its module.
- Fail-closed unclaimed paths were safe but made documentation-only diffs select every target. Contract-v6 now has reviewed `affected_ignore` globs, with generated documentation defaults and protected execution-authority paths.
- Forward-compatible manifest fields must remain readable under the public contract. The canonical digest therefore hashes the complete parsed JSON value, including unknown nested tool fields, instead of rejecting or dropping them.
- Native action timeout overrides cannot honestly promise preemption for bounded in-process mutations. The catalog now rejects those overrides while supervised commands and schema checks retain enforceable overrides.
- A worktree-mutating target cannot reuse the previous target's postcondition as its own precondition because it adopts its post-run fingerprint as the next trusted epoch; it now performs a fresh scan and fails closed when the new epoch is unverifiable.
- The custom-template remediation integration test cloned committed `HEAD` while exercising an uncommitted template-model change, mixing two authority generations. The fixture now applies and commits the current generic template delta before asserting that a downstream custom identity remains stable.
- Strict Rust 1.97 Clippy exposed workspace-wide mechanical cleanup that earlier validation had not exercised. The code was updated rather than weakening `-D warnings`, so the configured gate now passes without broad lint allowances.
- A fixed receipt-index budget cannot safely discard arbitrary incomplete groups: a discarded group may be the newest authoritative run. Overflow is now represented as a bounded uncertainty watermark, and a later complete group is accepted only when its ordering key proves it newer than every discarded candidate.
- Generated frontend contract targets were inferred from the presence of any adopted frontend, even though only this harness's scaffold installs the owning `contracts.mjs` runner. The render model now carries that scaffold capability explicitly; adopted repositories omit those targets, while an authored model that already declares them retains them across update/recopy.
- Required generated contract modes must never degrade to optional shell behavior. Explicit contract invocations now fail before dependency installation when their runner is absent, and scaffolded CI calls the checked-in runner directly.
- A cancellation cursor captured before the queued event can advance past a concurrent cancel request. Run creation now appends the queued event and returns its end offset under one JSONL writer lock, so every later cancellation remains observable.
- Repository authority is process-local only for short-lived CLI commands. Long-lived MCP, status, and UI paths now reload same-version authority at each boundary, including work start/status metadata as well as execution and gate consumers.
- Git path lists are byte protocols on Unix. Schema snapshot enumeration now preserves non-UTF-8 paths instead of lossy-normalizing distinct files, while textual Git results remain strict UTF-8 and all authority-bearing captures fail on truncation.
- Output policy must reach nested configured processes, not only the outer tool wrapper. Legacy work checks and the schema generator now retain bounded prefixes in fatal overflow results and receipts.
- A work plan can become stale after its required gates are evaluated but before close. Finish now rechecks the evaluated worktree fingerprint, canonical execution authority, and gate definitions before mutating plan/session state.
- Redirecting a child process's stdio to a PTY does not replace an inherited controlling terminal. Crossterm queries `/dev/tty` for window size, so the Vault PTY fixture had to create a new session, acquire its slave as the controlling terminal, and close the original PTY descriptors on exec to keep wide-layout and resize-clear assertions independent of the gate runner's terminal.

## Decision Log

- Decision: Treat configured-command timeout, output, capture, cancellation, and stdin policy as one execution boundary rather than duplicating flags at each caller.
  Rationale: the repository's design history explicitly requires v6 configured runners to preserve existing command semantics; an API that accepts a partially configured `Command` must complete the policy itself.
  Date/Author: 2026-08-23 / Codex.

- Decision: Use `[execution].command_timeout_seconds` as the default target timeout and allow a validated action timeout to override it.
  Rationale: omission is documented to default to 1800 seconds, while action-level timeout is the more specific declared policy.
  Date/Author: 2026-08-23 / Codex.

- Decision: Preserve stash-based snapshots for committed repositories and create a disposable synthetic baseline for an unborn repository.
  Rationale: committed repositories retain exact tracked/staged/untracked behavior; an unborn repository has no stash commit to materialize but still needs a clean, non-mutating schema sandbox.
  Date/Author: 2026-08-23 / Codex.

- Decision: Retain a deterministic prefix of selection reasons and record the full count and digest when truncated.
  Rationale: ordinary DTO bytes remain compact and readable, while large plans become bounded without silently claiming complete explanation.
  Date/Author: 2026-08-23 / Codex.

- Decision: Use the explicit frontend role at the generated checker boundary; only `spa` bundles are public contract artifacts.
  Rationale: the typed configuration already owns this semantic distinction and directory-name or bundle-content inference would reintroduce ambiguity.
  Date/Author: 2026-08-23 / Codex.

- Decision: Run the ambient-Git regression in an exact-filter child test process.
  Rationale: environment changes then cannot affect any parallel fixture while the production path is still exercised under hostile Git routing variables.
  Date/Author: 2026-08-23 / Codex.

- Decision: Keep unclaimed affected paths fail-closed and add explicit `repository.affected_ignore` globs for reviewed non-impacting paths.
  Rationale: silently dropping unknown files recreates the correctness bug, while selecting every target for known documentation paths defeats affected planning. Checked-in ignore authority makes the trade explicit and digest-bound.
  Date/Author: 2026-08-23 / Codex.

- Decision: Canonicalize the complete parsed manifest JSON in the execution-authority digest.
  Rationale: the public contract requires older consumers to ignore unknown manifest fields, so rejecting them would break forward compatibility and typed reserialization would silently omit their authority.
  Date/Author: 2026-08-23 / Codex.

- Decision: Treat receipt-index overflow as ordering uncertainty rather than a permanent global failure or silent eviction.
  Rationale: a bounded reader can recover once a demonstrably newer complete run appears, but must fail closed while an evicted candidate could still supersede the selected evidence.
  Date/Author: 2026-08-24 / Codex.

- Decision: Derive generated frontend contract obligations from explicit scaffold provenance and preserve already-authored repository actions on recopy.
  Rationale: frontend presence alone does not prove that the harness owns a contract runner; required evidence may be emitted only when the owning capability is installed or already declared.
  Date/Author: 2026-08-24 / Codex.

- Decision: Treat long-lived repository contexts as discovery handles, not durable execution authority.
  Rationale: the server root and contract epoch remain stable, but same-version manifest, command, gate, and repository metadata can change while MCP or UI processes are alive and must be reloaded before use.
  Date/Author: 2026-08-24 / Codex.

- Decision: Keep schema and configured-command overflow fatal while preserving a bounded diagnostic prefix.
  Rationale: truncation cannot be accepted as complete execution evidence, but discarding already captured output makes the failure needlessly opaque and weakens receipts.
  Date/Author: 2026-08-24 / Codex.

- Decision: Give spawned full-screen TUI fixtures their own controlling terminal rather than weakening viewport or clear-sequence assertions.
  Rationale: redirecting fd 0-2 alone leaves `/dev/tty` bound to the caller when one exists; a private session and controlling slave preserve the production terminal contract under both TTY and non-TTY test runners.
  Date/Author: 2026-08-24 / Codex.

## Outcomes & Retrospective

The implementation now treats execution supervision, planning authority, durable state, and generated repository policy as end-to-end boundaries. The original Claude Opus plus native Codex comprehensive review and every subsequent native-review iteration are complete; the final fresh native pass reported no issue. Focused regressions, strict workspace Clippy, formatting, launcher-template parity, and diff validation pass. The final isolated Jig run passed all configured partitions: 2,482 main tests, 438 Vault tests, and both PTY tests. Structured evidence reports both required gates fresh against worktree fingerprint `5c93d1b1854a349bc44fba587d5887ddcb05c5e0`, with batch receipt `receipt_01M0TJ41SFNTE7D59R5HF1A6BT`, contract receipt `receipt_01M0TH5BBRRAZAYPHYKMWB5K8C`, and test receipt `receipt_01M0TJ40QMYJZ7HPS02ZJFCPCV`.

## Context and Orientation

`crates/jig/src/execution.rs` owns configured-command supervision; `crates/jig/src/runtime/run_execution.rs` runs contract-v6 repository targets. `crates/jig/src/policy.rs` and `crates/jig/src/policy/schema.rs` own schema subprocesses and disposable worktrees. `crates/jig-contract/src/run.rs` defines the persisted plan DTO, and `crates/jig/src/repository/planner.rs` constructs it. `templates/project/scripts/check-webapps.sh.jinja` is the generated web checker; its embedded mirror is generated by `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`. The ambient Git regression is under the receipt/source-authority tests in `crates/jig/src`.

## Plan of Work

First, introduce a single non-interactive configured-command function that always replaces stdin with null, captures stdout and stderr, applies a fatal byte limit, observes cancellation, and reports timeout/cancellation/overflow distinctly. Keep the legacy progress wrapper on this function and migrate target execution to it. Validate action timeout values in the repository catalog and derive the default from `RepoContext`.

Second, scrub repository-routing Git environment variables from schema Git commands. Detect whether `HEAD` exists. Continue to use `git stash create` when it does; otherwise enumerate cached and non-ignored files, copy them safely into a temporary initialized repository, make a synthetic local commit, and run the generator only there. Add a fresh-repository regression proving the live worktree is unchanged and stale output returns the normal check conclusion rather than an infrastructure error.

Third, cap persisted reasons after deterministic ordering, preserving higher-value non-path reasons ahead of path-expanded reasons. When truncation occurs, include the full reason count and a domain-separated SHA-256 digest. Extend DTO and planner tests, schema/documentation, and plan-id stability checks.

Fourth, pass each frontend application's declared role into the generated check invocation and gate `contracts.mjs artifact-check` on `spa`. Extend generated configuration and shell behavior tests, then refresh the embedded template snapshot.

Fifth, convert the global Git environment test to a parent/child exact-test pattern. Run focused regressions, formatting, Clippy, the full configured test gate, contract gate, and structured-work evidence. Finally run the comprehensive-review skill with Claude Opus over branch scope plus an independent Codex pass, fix every critical and medium finding, and repeat until none remain.

## Concrete Steps

From the repository root, use `apply_patch` for source edits. After each slice, run the narrow `cargo test -p jig-sh <test-filter>` or `cargo test -p jig-contract <test-filter>` that proves it. Refresh templates with `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`. Build the runtime with `cargo build -p jig-sh --bin jig`, set `JIG_DEV_BIN=target/debug/jig`, then run `scripts/jig work check --plan-id plan_01M0QH4J3795CBX7EZ9GRW627B`, configured gates, evidence, receipts, and status.

## Validation and Acceptance

Acceptance requires focused regressions for all six findings, `cargo fmt --all -- --check`, warning-free Clippy for the affected workspace, `scripts/jig check test`, `scripts/jig check contract`, `git diff --check`, and matching embedded templates. The final comprehensive branch review must contain no critical or medium finding from either reviewer. The live repository must contain only intentional source, documentation, template, plan, and append-only state changes.

## Idempotence and Recovery

All checks and snapshot refreshes are repeatable. Schema regressions operate in temporary directories and must never modify their source repository. If a test or review fails, retain the structured-work record, update this living plan with the discovery, fix the owning boundary, and rerun the narrow proof before restarting broad gates. Do not hand-edit embedded snapshots or historical JSONL entries.

## Interfaces and Dependencies

No new external dependency is expected. Reuse `jig-owned-process` output policies, existing context timeout/output types, `sha2` already used by the planner, `tempfile` for disposable schema worktrees, and the existing bootstrap Git-environment scrubber. DTO changes must use serde defaults or skipped empty fields so non-truncated plans retain the existing representation where practical.
