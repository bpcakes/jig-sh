# Close comprehensive-review findings

Fix the supported-host installer regression, restore the published status-provider v1 contract, make every subprocess in the cooperative schema check cancellable, and remove the Windows-specific artifacts and guidance missed by the earlier sweep. Each behaviorally independent repair lands in its own commit and the final tree passes the complete configured validation matrix.

## Progress

- [x] (2026-08-22) Reproduce and source-trace all six merged review findings.
- [x] (2026-08-22) Repair and behaviorally test the installer path-resolution fallback.
- [ ] Restore status-provider v1 path compatibility and its conformance regression.
- [ ] Supervise schema-check Git probes and add process-directed cancellation coverage.
- [ ] Remove residual Windows artifacts and guidance and add a tracked-file inventory guard.
- [ ] Run focused validation, the full configured test gate, format, Clippy, contract, and template parity checks.
- [ ] Review the final diff, record evidence, and close structured work.

## Surprises & Discoveries

- The existing installer parity gate kept all three copies byte-identical, but therefore propagated the same missing Python import into every copy. Parity is not a substitute for executing the fallback behavior.
- The Windows cleanup crossed the independently versioned status-provider protocol. A drive-looking `C:/...` name is legal relative syntax on supported Unix hosts, but allowing providers to emit it under v1 would make their reports unreadable by older v1 consumers.
- `check schema` supervises its configured dump but drops back to the legacy unbounded `git_text` helper for its final status and diff probes, despite the command-wide cooperative signal policy.

## Decision Log

- Decision: Keep the status-provider v1 drive-prefix rejection as protocol compatibility, not as native-Windows support.
  Rationale: Old v1 consumers reject that value, and the published versioning rules require a new major before providers may rely on changed path semantics.
- Decision: Route schema Git probes through the same owned-process execution abstraction as the dump command.
  Rationale: A command classified as cooperative must not contain hidden subprocess boundaries that ignore cancellation, timeout, or bounded capture.
- Decision: Add one tracked-source platform-surface guard with explicit exclusions for dependency-owned lock metadata, append-only state, and the current removal plan.
  Rationale: The previous manual inventory missed hidden configuration, generated templates, and historical guidance; a narrow executable rule prevents recurrence without flagging unrelated uses of the word `windows` such as slice windows or rate-limit windows.

## Outcomes & Retrospective

Pending implementation and final validation.

## Context and orientation

The canonical runtime installer is `scripts/install-jig.sh`; `templates/project/scripts/install-jig.sh.jinja` and `crates/jig/src/bootstrap/embedded_template_snapshots/scripts/install-jig.sh.jinja` are byte-identical mirrors. Status-provider v1 validation lives in `crates/jig-contract/src/status_provider/v1.rs`. Schema checking lives in `crates/jig/src/policy.rs` and uses process supervision from `crates/jig/src/execution.rs`. Generated scaffold contracts have canonical and embedded copies under `templates/scaffolds/` and `crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/`.

## Plan of work

First restore the installer import and exercise `resolve_executable_path` with a `PATH` containing Python but no `realpath`. Then restore the v1 semantic validator and conformance case. Next extract a small cancellable Git-capture helper for schema probes and prove a process-only signal cancels a deliberately blocked Git command. Finally remove the remaining `.exe` paths, stale nextest selector, and support-shaped plan prose; add an inventory check that searches tracked files rather than relying on ignore behavior.

## Concrete steps

1. Patch all installer mirrors and `scripts/fixtures/source-normalization.sh`; run the source-normalization fixture and launcher parity check; commit.
2. Restore the v1 validator/test and run `cargo test -p jig-contract`; commit.
3. Refactor schema Git execution, add unit/integration cancellation coverage, and run the focused Jig tests; commit.
4. Remove Windows artifacts/guidance, add the inventory guard to repository contract validation, run its focused checks, and commit.
5. Build the dev Jig binary, run structured work checks plus direct format/Clippy/parity validation, inspect receipts and diff, then close and commit evidence.

## Validation and acceptance

Acceptance requires the installer fallback test, status-provider contract tests, schema process-directed cancellation test, platform-surface inventory, launcher/template parity, `scripts/jig check test`, `scripts/jig check fmt`, `scripts/jig check clippy`, and `scripts/jig check contract` to pass with `JIG_DEV_BIN=target/debug/jig`. The final worktree must be clean and commits must remain behaviorally coherent.

## Idempotence and recovery

All source edits are deterministic. Generated mirrors must be refreshed from their canonical source before committing. Focused tests may be rerun freely. Append-only state records are never rewritten; if a gate fails, append new passing evidence after the fix rather than editing prior receipts.

## Interfaces and dependencies

No new dependency or public data format is required. Status-provider v1 retains its old interface. Schema execution reuses `ExecutionControl`, `CommandTimeout`, `CommandOutputLimit`, and the existing owned-process runner. The inventory guard operates only on Git-tracked repository source and excludes dependency-managed lock metadata and append-only state.
