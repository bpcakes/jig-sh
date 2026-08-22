# Harden repository execution invariants

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current as implementation proceeds.

## Purpose / Big Picture

The current branch adds repository actions and a Go backend, but review exposed places where the code models an invariant indirectly or defines the same source boundary more than once. After this change, resumable runs will fingerprint every repository input that schema generation can observe, every target will be checked against an explicit worktree-mutation policy, migration writes will remain inside the repository, generated action inputs will reflect effective configuration, frontend checks will declare their shared repository inputs, and generated CI will honor the configured runner. Focused regression tests will make each boundary executable.

Success is observable by running the relevant Rust tests and the repository's standard fmt, Clippy, test, and contract gates with a development `jig` binary. The final diff should contain no compatibility break to persisted state or authored repository models.

## Progress

- [x] (2026-08-22) Researched the review's open questions against commit history, public-contract documentation, configuration documentation, and upstream Goose guidance.
- [x] (2026-08-22) Classified ignored dotenv/submodule fingerprinting and effect enforcement as design-boundary problems; classified migration containment, dynamic inputs, frontend shared inputs, CI runner propagation, and legacy-ID collision handling as localized omissions.
- [x] (2026-08-22) Added characterization and regression tests for each accepted finding.
- [x] (2026-08-22) Centralized observable-source constants/path validation and made fingerprints cover ignored dotenv files plus initialized submodule worktrees.
- [x] (2026-08-22) Replaced the read-only proxy with an explicit declared-worktree-effect invariant before and after every target.
- [x] (2026-08-22) Validated migration paths at configuration and filesystem boundaries; derived generated migration-action inputs from effective configuration.
- [x] (2026-08-22) Completed frontend shared inputs, runner propagation, and legacy action-ID collision handling.
- [x] (2026-08-22) Refreshed generated snapshots and ran focused tests, workspace check/Clippy, the full suite, an isolated flaky-test rerun, and fresh structured contract/test gates.
- [x] (2026-08-22) Reviewed the finished diff and tightened the final migration-root edge case; commit and push are the handoff operations that follow this plan.

## Surprises & Discoveries

- The schema snapshot intentionally overlays ignored `.env` files and recursively snapshots initialized submodules. The run fingerprint only observes the parent Git worktree, so this is not merely a forgotten test: two definitions of observable repository source have drifted.
- Effect enforcement is expressed through `is_read_only`. That grants every target with any effect an implicit worktree-write exemption, even when its declared effects are only `Process` or `External`.
- Full source/config invalidation is deliberately conservative and documented; it is not a cache key and should remain broad.
- Backend-to-frontend affected propagation is intentionally conservative. The defect is narrower: frontend checks invoke repository-level contract tooling whose inputs are absent from frontend input declarations.
- Durable run records intentionally never expire, and reverse lookup deliberately scans past the first queued event to reject duplicate queued records created by append-only stream merges. Replacing this with early exit would weaken integrity. Indexing is a separate state-format design problem, not part of this bug fix.
- Goose officially supports hybrid timestamp/sequential versioning, so mixed migration filename styles are not a correctness defect.
- A full `cargo test --workspace` run completed 1,724 tests successfully and hit one unrelated timing-sensitive failure in `doctor::tests::sqlx_driver_probe_invokes_shim_safely_and_times_out` under heavy parallel load. The same test passed immediately in isolation; do not broaden this patch into doctor timeout tuning without reproducible evidence.

## Decision Log

- Decision: preserve complete source/config invalidation and dependent propagation.
  Rationale: both behaviors are explicit public-contract choices that favor false invalidation over stale proof.
  Date: 2026-08-22

- Decision: keep durable run lookup semantics unchanged.
  Rationale: the reported scaling issue is a known consequence of append-only merge validation. A safe optimization needs an index/rebuild/compatibility design and should not be mixed with this correctness patch.
  Date: 2026-08-22

- Decision: define authorization in terms of a declared `Worktree` effect, not whether a target is called read-only.
  Rationale: `Process` and `External` effects do not authorize repository mutation; checking every target before execution and every non-worktree target after execution removes the implicit exemption.
  Date: 2026-08-22

- Decision: validate configurable repository paths twice: syntactically when configuration is loaded and against symlink/non-directory ancestors immediately before writes.
  Rationale: lexical validation rejects absolute, root, and parent-traversal destinations, while sink validation blocks pre-existing filesystem redirection and symlink escapes at the destructive boundary.
  Date: 2026-08-22

- Decision: change generated defaults only; do not rewrite authored repository action inputs.
  Rationale: repository actions are an owned public contract. Generated models should reflect effective configuration, while authored customizations remain authoritative.
  Date: 2026-08-22

## Context and Orientation

`crates/jig/src/git_receipts.rs` computes the worktree fingerprint recorded by resumable execution. `crates/jig/src/policy/schema.rs` constructs the source snapshot used by schema generation, including ignored dotenv overlays and initialized submodules. These two modules must agree on what repository state can influence a run.

`crates/jig/src/runtime/run_execution.rs` owns the execution source epoch. The current `is_read_only` helper checks only targets without effects. The replacement must compare the live fingerprint with the trusted epoch before every target, reject post-target mutation unless `ActionEffect::Worktree` was declared, and advance the trusted epoch only for a declared worktree-writing target. Receipt capture may report the live fingerprint without laundering an unauthorized mutation into the trusted epoch.

`crates/jig/src/context.rs` loads `migration_dir` and its legacy `rust_migration_dir` fallback. `crates/jig/src/policy.rs` joins that string to the repository root before creating migration files. A shared repository-path policy should reject absolute paths, `..`, empty/dot destinations, symlink ancestors, and non-directory ancestors.

`crates/jig/src/bootstrap/repository_model.rs` generates default action inputs. Migration actions need the effective configured migration directory, and frontend checks need the repository-level files consumed by `scripts/check-webapps.sh` and `scripts/contracts.mjs`. Adapter descriptors remain the source of adapter-specific defaults.

`templates/project/.github/workflows/go-tests.yml.jinja` produces Go CI. All jobs except the documented PostgreSQL browser-E2E exception should use `ci_github_runner`. `crates/jig/src/repository.rs` converts legacy tools to action IDs and must verify every digest fallback candidate against already occupied IDs.

## Plan of Work

First, add focused tests that demonstrate the accepted failures: ignored dotenv content and successive dirty submodule edits alter fingerprints; stable drift blocks even an effectful first target; undeclared worktree mutation fails after `Process` or `External` targets; unsafe migration directories are rejected lexically and through symlink ancestors; generated migration/frontend inputs match their real dependencies; Go CI uses the configured runner; and a preoccupied legacy digest fallback retries safely.

Second, extract only the policies that already have multiple consumers. Put portable repository-relative normalization and write-ancestor containment in one internal module. Put schema-observable source constants/parsing in one internal seam used by snapshotting and fingerprinting. Avoid a broad new architecture: the goal is one authority per invariant.

Third, refactor execution epoch handling into explicit precondition and postcondition operations. Preserve receipt evidence and fail-fast behavior. An unauthorized mutation must leave the trusted epoch unchanged so a later target cannot silently accept it.

Fourth, make generated model inputs derive from effective answers at the generation boundary, add the missing shared frontend authority files, use `ci_github_runner` in the Go integration job, and harden legacy ID candidate selection.

Finally, refresh embedded template snapshots, run focused tests after each small step, then build the development binary and run `scripts/jig work check`, configured gates, evidence, receipts, and status for this plan. Inspect the final diff for public-contract drift before committing and pushing.

## Concrete Steps

Run from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`.

1. Capture the refactoring scanner baseline:

       python3 /home/aa/.agents/skills/fowler-rust-refactoring/scripts/scan_refactoring_opportunities.py . --git-diff master --format json

2. Add and run focused tests with commands selected from the owning modules, including:

       cargo test -p jig-sh git_receipts
       cargo test -p jig-sh runtime::tests
       cargo test -p jig-sh context::tests
       cargo test -p jig-sh policy::tests
       cargo test -p jig-sh bootstrap::repository_model::tests
       cargo test -p jig-sh repository::tests

3. Refresh generated template snapshots after changing templates:

       JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh

4. Run formatting and focused checks:

       cargo fmt --all -- --check
       cargo clippy --workspace --all-targets -- -D warnings

5. Build the development runtime and run repository gates:

       cargo build -p jig-sh --bin jig
       export JIG_DEV_BIN=target/debug/jig
       scripts/jig work check --plan-id plan_01M0N2RGEA8SQEWQYXDF2R82BZ
       scripts/jig work gates --plan-id plan_01M0N2RGEA8SQEWQYXDF2R82BZ
       scripts/jig work evidence --plan-id plan_01M0N2RGEA8SQEWQYXDF2R82BZ
       scripts/jig work receipts --plan-id plan_01M0N2RGEA8SQEWQYXDF2R82BZ

## Validation and Acceptance

Acceptance requires tests proving behavior, not only implementation coverage. A resumable run must reject any pre-target drift regardless of the next target's effects. Only a declared `Worktree` effect may advance the source epoch after mutation. Changing an ignored dotenv file or modifying an already-dirty initialized submodule again must change the fingerprint. Unsafe migration paths must fail before any external write, while valid nested configured paths still work and appear in generated action inputs. Frontend affected selection must include contract/API-authority changes. Generated Go jobs must consistently use the configured runner. Legacy conversion must never return an occupied action ID.

All focused tests, workspace Clippy, repository tests, formatting, and contract checks must pass. Existing public-contract tests for conservative invalidation, dependent propagation, durable run lookup, and authored model preservation must remain unchanged and green.

## Idempotence and Recovery

All validation commands are repeatable. Snapshot refresh is deterministic and should be followed by a diff inspection. Structured work state is append-only; do not edit prior records to hide a failed gate. If a focused refactor fails, revert only the newly introduced step with a forward patch while retaining its characterization test. No migration or persisted-state format change is planned.

## Artifacts and Notes

The final handoff should name the two deeper invariant fixes separately from the localized omissions, record the deliberately deferred run-index architecture question, and link the committed files. Goose hybrid-versioning guidance used to reject the numbering concern is available from the upstream `pressly/goose` README.

## Interfaces and Dependencies

No new external dependency is required. New internal helpers should remain crate-private. The path API should accept a repository root plus a normalized relative path and return an error before callers create directories or files. The source-epoch API should expose explicit prepare/finish semantics tied to `ActionEffect::Worktree`. Generated repository model changes must continue to produce the existing schema types from `jig-contract`.

## Outcomes & Retrospective

The patch reduced two recurring bug surfaces instead of layering point fixes on them. Schema snapshotting and run fingerprinting now share the ignored-dotenv and initialized-submodule projection policy. Execution separates its trusted epoch from the latest observed fingerprint and grants worktree mutation only to targets declaring `Worktree`; `Process` and `External` no longer act as accidental exemptions.

Repository-relative path normalization now has one crate-private home. Migration authoring validates syntax at configuration load and real directory ancestry at the write boundary. Generated migration inputs use the effective configured directory, frontend actions name their shared contract/package-manager authority, Go PostgreSQL CI uses the configured runner, and legacy action-ID collision fallback rechecks every candidate.

Verification passed through fresh structured gates: `jig.contract_check` receipt `receipt_01M0N4EY8BTVAT57FMVV3AGQEP` and `jig.test` receipt `receipt_01M0N5CBPQW7SRSDZQ321R98ZT`, grouped by batch receipt `receipt_01M0N5CBVYRT5HYB4QEPXJ0MJX`. Direct workspace check and Clippy also passed. The ordinary full `cargo test --workspace` run completed 1,724 tests successfully and exposed one unrelated timing-sensitive SQLx shim timeout under heavy parallel load; that exact test passed immediately in isolation, and the structured Nextest gate subsequently passed all configured tests.

The durable-run reverse-scan performance question remains deliberately separate. Preserving duplicate queued-event detection is more important than an unsafe early exit; a future optimization should design an index and rebuild/compatibility behavior explicitly.
