# Fix post-review runtime boundary defects

Use Fowler-style preparatory refactoring to reduce the bug surface behind the comprehensive-review findings, then fix the behavior and test gaps. The affected code is internal to `crates/jig` plus the generated installer mirrors and Windows CI. Preserve contract v4, Rust 1.85, Bash 3.2 compatibility, generated-template parity, and existing launcher-only rollback behavior.

## Progress

- [x] Establish a green format/check/bootstrap-test baseline.
- [x] Add failing production-path characterizations for minimal refresh, post-commit seed failure, and tracked-file-to-symlink replacement.
- [x] Replace source/footprint conditionals with one closed full-refresh runtime policy and move finalization beside cache lifecycle.
- [x] Make post-commit embedded cache publication best-effort while retaining the last repair seed on failure.
- [x] Reject worktree symlinks for every tracked source path in all installer mirrors.
- [x] Combine Windows Bash and helper-PATH discovery into one validated repair-tool environment; prefer and validate Git for Windows layouts and narrow ambient Python admission.
- [x] Run configured Jig gates; focused default/no-default tests, generated parity, format, and Clippy are green. Windows-native policy tests are checked into the Windows CI job because only the Linux target is installed locally.

## Surprises & Discoveries

- The unit seed double always published both runtime and default profiles, unlike production's feature-dependent compatibility probe. It now follows the `dev-proxy` feature so no-default tests exercise the real profile shape.
- Cache seeding is necessarily outside the repository render transaction. Treating it as if it could roll back the already committed render produced misleading command failure; the coherent boundary is explicit post-commit finalization with warnings and fallback retention.
- Git index modes cannot detect a regular tracked file replaced by a worktree symlink. The source check must inspect the current filesystem entry for every tracked path before hashing the diff.

## Decision Log

- Use Fowler **Move Function**, **Replace Primitive with Object**, and **Split Phase** for full-refresh finalization. `FullRefreshRuntimePolicy` combines footprint and template source; repository mutation commits before cache finalization, whose failure is non-fatal and observable.
- Keep launcher-only repair transactional and fail-closed because that command replaces the scripts required to recover. Only ordinary full-refresh finalization becomes best-effort.
- Use Fowler **Introduce Parameter Object** for platform tools: `RepairToolEnvironment` couples the selected Bash executable with the only PATH it may execute.
- On Windows, accept only a native PE `bash.exe` in a Git-for-Windows layout containing native `cmd/git.exe`; use standard install roots before PATH. Expose Git-owned tool directories plus one native-PE `python3.exe` directory outside the repository, and document that this is narrower than ambient PATH but not the Unix ownership guarantee.
- Preserve the three installer copies because they are intentional generated/runtime mirrors guarded by `scripts/check-launcher-template.sh`; apply the same small change to all and test parity rather than introducing generation machinery in this bug fix.

## Outcomes & Retrospective

All locally runnable acceptance checks passed. Embedded minimal adopt/update now succeeds without an installer; committed renders remain successful and return warnings when post-commit cache publication fails; the previous recognized repair seed remains available; dirty tracked symlinks fail closed; and Windows selection excludes competing non-Git Bash candidates and unrelated PATH directories. Windows-native selection and verbatim/UNC/long-path cases are covered by focused tests wired into the Windows CI job because only the Linux target is installed locally.

The configured contract and full workspace test gates passed in batch receipt `receipt_01M030V7V37N6078DNZJMQ26FM`. Jig-recorded format and Clippy checks passed in receipts `receipt_01M030VFA00GMH4710BAWMGH3D` and `receipt_01M030W7GY9JH2N117X3PPZV62`. A final diff check and generated-installer parity check were also clean.

## Context and orientation

`crates/jig/src/bootstrap/update.rs` orchestrates update rendering. `crates/jig/src/bootstrap.rs` orchestrates adoption. `crates/jig/src/bootstrap/launcher_repair_cache.rs` owns cache seeding/publication/retirement and platform tool discovery. `scripts/install-jig.sh`, `templates/project/scripts/install-jig.sh.jinja`, and the embedded snapshot are byte-parity mirrors. `crates/jig/tests/launcher_repair.rs` exercises the production binary on Unix; `launcher_repair_windows.rs` and focused library tests run on Windows CI.

## Plan of work

First characterize observable failures. Then introduce closed policy types and move responsibilities without changing launcher-only behavior. Apply behavior changes one boundary at a time, run focused tests after each, update public-contract wording, and finish with the repository's configured checks through a freshly built development binary.

## Concrete steps

1. Add tests that reproduce the three confirmed Unix defects.
2. Introduce and test `FullRefreshRuntimePolicy`; migrate adopt/update callers.
3. Make full-refresh cache finalization return warnings after the durable render boundary.
4. Replace independent Windows Bash/PATH queries with `RepairToolEnvironment` and validated Git/Python capability selection.
5. Inspect tracked worktree entries for symlinks before source hashing in every installer mirror.
6. Add Windows selection/path-conversion tests and run them in the Windows workflow.
7. Run focused, parity, format, Clippy, contract, and full test gates.

## Validation and acceptance

- `cargo test -p jig-sh platform_policy_tests --locked`
- `cargo test -p jig-sh --test launcher_repair --locked`
- `cargo test -p jig-sh --no-default-features --locked`
- `bash scripts/check-launcher-template.sh`
- `cargo build -p jig-sh --bin jig --locked`
- `JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M02Z1H88DEP3EM7PBZQ9PWKP`
- `JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M02Z1H88DEP3EM7PBZQ9PWKP`

Acceptance requires all focused tests and configured gates to pass, with Windows-native tests delegated to the checked-in Windows CI job when no Windows host is locally available.

## Idempotence and recovery

Tests use temporary repositories and caches. Failed cache seeding leaves the rendered repository intact and preserves any recognized repair seed. Installer parity is checked after every mirror edit. If a refactoring step breaks compilation or behavior, revert only that small step and retain the preceding green characterization tests.

## Interfaces and dependencies

No public Rust API or new dependency is introduced. Contract v4 JSON gains only additive `warnings` arrays on successful adopt/update responses. Existing generated launcher and installer protocols, cache stamp formats, feature profiles, and transaction semantics remain otherwise unchanged.
