# Fast Jig source development

Ordinary `scripts/jig` commands in this source repository will run a repository-selected 0.4.0 executable without compiling edited source. `scripts/jig-dev` will explicitly build and execute the current implementation. Required verification will exercise the development executable before work can finish.

## Progress

- [x] Inspect launcher selection, Cargo integration tests, repository gates, and CI.
- [x] T-01: Select and cache the pinned installed release for the source repository.
- [x] T-02: Add the explicit development entrypoint, required validation target, CI, and guidance.
- [x] T-03: Verify cold and warm selection, edits and broken source, failures, override behavior, and full required gates.

Restart checkpoint: complete. Structured work closed with fresh passing evidence, then the Bead was closed and its metadata exported. Baseline is `96d92adecd92ef1450207aeead2e5113f4f296e1`. Structured plan ID: `plan_01M2TR4P2AGJJXHVT7TQYEZKWB`; Bead: `jig-sh-y7mo`. There were no pre-existing working-tree changes. The system executable reports 0.4.0.

## Surprises & Discoveries

The source-checkout branch in `scripts/install-jig.sh` invalidates its installed executable on any change covered by `local_source_stamp`, then invokes `cargo install`. Integration tests such as `crates/jig/tests/cli_json.rs` already execute `CARGO_BIN_EXE_jig`, independently of the harness used to start Cargo. The default `verify` profile in `.jig.toml` is enforced by structured work completion.

The source runtime action cannot declare `.agent/jig-contract.json` as an input because `.agent/**` is excluded from source identity; contract authority is already tracked separately. A review found Python startup contamination and a Python 3.9-only string method; the helper now launches with `python3 -I` and uses Python 3.8-compatible operations, with a regression test for startup output. A broad source-normalization run was stopped when it entered unrelated real release installs; the three relevant source-checkout, explicit-override, and PATH-wrapper fixtures passed separately.

## Decision Log

- 2026-09-18: Implement the user-selected stable-runner option. Store the source repository's runtime pin in `.jig/source-runtime-version`, separately from template provenance and the legacy `jig_version` field. Verify the installed binary's exact version and contract capabilities before copying it into a repository-local cache. Source edits do not invalidate this copy. Missing/incompatible runtimes fail with actionable setup guidance; ordinary commands never build one implicitly.
- 2026-09-18: Preserve explicit `JIG_DEV_BIN` precedence and lower-level explicit install roots. Keep generated downstream repositories and historical source checkouts without the new policy on their existing selection behavior. A source checkout carrying either the helper or pin opts into the new policy, so accidentally removing one produces an error instead of a build.
- 2026-09-18: The development entrypoint uses Cargo's compiler-artifact executable path, accommodating custom target directories. Add `repo:source-runtime-check` to the required verify profile; it builds and checks the current contract through the normal launcher without recursive verification. CI exercises this same entrypoint.

## Outcomes & Retrospective

Implementation and validation are complete. All 23 Python tests, three focused source-selection fixtures, and launcher/template parity passed. Required work run `run_01M2TRSN3GQX01V09286C7HZM3` passed all six targets: Clippy, formatting, Rust tests (4,231 passed, 3 skipped), contract, file budget, and the new current-source runtime check. Target validation receipt: `receipt_01M2TSSS2H5QZDS7RE87TAV81D`.

The real `repo:source-runtime-check` passed through the installed release and fresh development binary. On the edited checkout, measured warm `scripts/jig --version` at 0.107s and `scripts/jig --json info` at 0.130s. These are local observations, not a performance guarantee. Regression fixtures assert zero Cargo invocations for ordinary launcher use after source edits, with a current-source check still required for completion. Routine commands keep working with invalid Rust and Cargo files; the development entrypoint refuses stale executables after build failures. Compatibility tests preserve historical source-checkout and downstream selection behavior.

## Milestones and validation

T-01 changes the source-specific installer branch and synchronized templates, adds a small Python runtime selector and the 0.4.0 pin. It resolves only cached binaries for MCP startup and read-only probes; normal first use may copy a compatible installed native binary, with atomic publication under a filesystem lock. `--info` reports selection and path. Test pin changes, cache persistence after source edits and PATH changes, missing/wrong/incompatible binaries, refresh failures, overrides, and normal downstream selection. Rollback consists of restoring the former branch; disposable cache directories can remain unused.

T-02 changes `scripts/jig-dev`, `.jig.toml`, Rust CI, and development guidance. It can proceed independently of T-01 using the existing explicit override boundary. Test custom Cargo artifact paths, build failures, and argument/current-directory propagation. The development check must fail on an unsuccessful build and cannot execute a stale binary.

T-03 depends on both. Run the Python regression suite, launcher-template parity and relevant source-normalization fixtures. Refresh packaged snapshots using the repository's supported build flow. Run `scripts/jig work check --plan-id ID`, inspect gates/evidence/receipts, and finish only with current successful evidence. The verify profile includes `api:test`, satisfying the required backend test check. Use only generic fixture names and preserve append-only state.
