# Replace generated runtime version locks with contract compatibility

Newly generated and fully updated repositories must not pin a Jig product release. They declare a Jig contract epoch, and every candidate binary proves that it supports the repository's complete runtime contract and requested build profile before the launcher executes it. Product versions remain useful as binary identity (`jig --version`, diagnostics, and release packaging), but they no longer participate in generated configuration, launcher selection, cache selection, or source fallback.

This is a compatibility cutover, not a field deletion in isolation. Contract v4 must cover both the manifest-declared tools and the runtime-owned launcher/CLI/state surface that older documentation says was protected by `jig_version`. Existing contract v2/v3 repositories remain readable, and a narrow `jig update <repo> --launcher-only --force` repair path lets owners migrate locked projects without overwriting unrelated managed files.

## Progress

- [x] Traced release-version use through generated configuration, contract manifests, launchers, installers, doctor, policy, release automation, and fixtures.
- [x] Define contract v4 as the whole generated-harness compatibility epoch and add a read-only binary compatibility probe.
- [x] Keep v2/v3 legacy fields readable while omitting product versions from all newly rendered files.
- [x] Replace launcher, installer, and cache version matching with contract/profile compatibility checks.
- [x] Add the launcher-only repair path for already locked repositories.
- [x] Update doctor, policy, UI/info output, release automation, documentation, and fixtures.
- [x] Refresh embedded template snapshots and run focused plus repository-level verification through the development binary.

## Surprises & Discoveries

- The exact release is enforced independently by `scripts/jig`, `scripts/install-jig.sh`, `RepoContext`, `doctor`, `policy`, release automation, and fixture cache naming. Changing only the launcher would leave contradictory gates.
- The current public-contract documentation excludes runtime-owned commands, output, state, doctor, status, Codex, dev/proxy, and vault behavior from `.agent/jig-contract.json`; those surfaces currently rely on `jig_version`. Removing the pin is safe only if contract v4 becomes the compatibility epoch for the entire generated harness.
- Default and stripped installer profiles have different compiled capabilities. A binary built with default features can serve every profile, while a `--no-default-features` binary cannot serve `dev` or `proxy`. Compatibility probing therefore needs the requested profile as an input.
- Remote and clean committed template renders record `_src_path` and `_commit`, but embedded renders intentionally use `_src_path = "embedded:jig-sh"` with no commit. Local non-Git sources may also lack a commit. Source fallback cannot assume an immutable revision always exists.
- The existing `jig update` renders the full managed footprint and has no dry-run or subset mode. Using it to unlock an old project could require `--force` and overwrite every locally modified managed file. A narrow repair mode is required before recommending bulk migration.
- Staged updates copy managed paths one at a time. The v4 reader must tolerate a v4 manifest alongside still-present legacy fields so an interrupted update can be rerun directly with the installed binary.
- `scripts/jig mcp` deliberately avoids installation during startup. The compatibility change must not turn MCP resolution into a network or Cargo-install path.
- The dogfood repository and some older adopted repositories predate `.agent/jig-managed-paths.json` even though their two launcher files are recognizably Jig-generated. Requiring the manifest unconditionally would leave the exact locked projects needing repair unable to use the repair command.
- The package's `dev_lifecycle` and `dev_sigint` integration targets explicitly require the `dev-proxy` feature, so an unqualified `cargo test -p jig-sh --no-default-features` cannot be a passing command after the feature-independent tests complete. No-default verification must run the library and the three feature-independent integration targets explicitly.
- Rendering the full harness and filtering `active_paths` afterward is not surgically equivalent to rendering two launchers: full staging seeds agent guides, regenerates the agent map, reconciles managed blocks, writes a staged ownership manifest, and validates the entire rendered contract. Any unrelated malformed managed input could therefore block the repair even though it would never be applied.
- A tracked generated answer file also lives at `examples/full-stack-app/.jig.toml`, outside the flat example and fixture lists touched during the first render pass. The final operational-pin scan and release negative assertions must cover tracked nested `.jig.toml` files so examples cannot silently retain a product lock.

## Decision Log

- **Contract v4 is a whole-harness epoch.** A runtime that advertises support for v4 promises compatibility with the generated launcher protocol, runtime-owned CLI commands and JSON/state schemas, configuration parsing, safety flags, tool registry, work gates, dev/vault behavior, and all manifest-declared tools. Future breaking changes to any of those surfaces must bump the contract or explicitly drop the older epoch.
- **The binary owns compatibility logic.** Add the private, read-only protocol `jig __runtime-compatible --profile <default|runtime|mcp> <repo-root>`. Shell launchers use only its exit status; they do not duplicate Rust validation rules.
- **Legacy product fields are input-only.** For contract v2/v3, `.jig.toml` and `.agent/jig-contract.json` must still contain matching non-empty `jig_version` values, preserving existing corruption detection, but the candidate's package version is not compared with them. For v4, both fields are optional and ignored if present so mixed/interrupted updates recover cleanly. New renders omit both fields.
- **Profile compatibility is explicit.** `default` requires a binary compiled with `dev-proxy`; `runtime` and `mcp` accept either a full or stripped binary. The probe performs this check after full repository contract validation.
- **Caches use contract and profile, not release.** Keep the existing full-versus-runtime separation, install locks, and source stamps, but use safe cache segments such as `contract-4` and `contract-4-runtime`. Validate the manifest's numeric contract version before using it in a path and probe every cached or newly installed candidate before reuse.
- **Source provenance is retained without inventing a version fallback.** Prefer the recorded local source or immutable `_commit` when available. For embedded provenance with no commit, preserve the explicit `JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1` opt-in; fallback may install from the configured source's current default branch, must warn that it is not an identical-source reconstruction, and must reject the result if the compatibility probe fails. Legacy v2/v3 may continue using the recorded legacy version tag only as a last-resort source locator, never as a runtime equality check or cache key.
- **Explicit choices remain strict.** A missing, non-executable, or incompatible `JIG_DEV_BIN` is a hard error. An explicit install root is populated and validated rather than silently returning a PATH binary. In a Jig source checkout, locally built/source-stamped candidates are considered before global PATH so dogfooding does not accidentally run the installed release.
- **Locked projects get a surgical migration.** `jig update <repo> --launcher-only --force` uses selected staging to render only the current binary's embedded launcher and installer templates, then changes only `scripts/jig` and `scripts/install-jig.sh`. It does not seed or validate unrelated managed output. When a managed-path manifest exists, it must own both files. When it is absent, both existing paths must be regular, non-symlink files matching conservative old-or-new generated launcher signatures; arbitrary script pairs are rejected and the repair does not create ownership metadata. The command does not change `.jig.toml`, the contract manifest, template source identity, or retired paths. A subsequent normal update performs the v4 data migration.

## Outcomes & Retrospective

Implemented contract v4 as the whole generated-harness compatibility epoch. New and fully updated repositories omit product versions, generated launchers and installers select contract/profile-compatible binaries, caches use `contract-<epoch>` roots, MCP remains installation-free, and v2/v3 repositories retain their matching legacy fields without comparing them to the running product release. Focused coverage proves a stable 0.2.0 runtime accepts a valid legacy `0.2.0-beta.1` contract, a fake compatible PATH binary reporting 99.0.0 is reused, a stripped build is rejected for the default profile, and an incompatible explicit development binary cannot fall back.

The operator migration was exercised through `jig update <repo> --launcher-only --force` against both managed and pre-manifest legacy launcher fixtures, plus this repository's pre-manifest dogfood scripts. It changes only the launcher and installer, rejects arbitrary or symlinked pre-manifest pairs, remains idempotent, and leaves the legacy configuration/contract untouched for a later full update. The deliberate deviation from the initial revision is the conservative pre-manifest signature fallback documented in the Decision Log; without it, the dogfood repository and the oldest locked projects could not use the repair.

Embedded snapshots match the live templates, and the final generated-artifact scan includes nested tracked examples as well as root artifacts, templates, snapshots, and fixtures. Focused Rust tests, the valid no-default library/integration split, fixture validation with real local and immutable Git-source installs, POSIX/shell syntax checks, and the development-binary contract, formatting, Clippy, and full workspace test gates all pass.

## Context and orientation

The generated runtime boundary spans these areas:

- `templates/project/.jig.toml.jinja` and `templates/project/.agent/jig-contract.json.jinja` emit the repository contract.
- `templates/project/scripts/jig.jinja` selects and launches a binary; it must stay POSIX `sh` compatible.
- `templates/project/scripts/install-jig.sh.jinja` installs and caches profile-specific binaries. The dogfood copies are `scripts/jig` and `scripts/install-jig.sh`.
- `crates/jig/src/context.rs` defines `RepoConfig`, `ContractManifest`, and `RepoContext`; it currently supports only contracts 2 through 3 and requires matching version strings.
- `crates/jig/src/policy.rs` already validates required command keys, configured command values, native/command tools, feature-required tools, and work-gate references. This is the correct full repository validator for the compatibility probe.
- `crates/jig/src/cli.rs`, `crates/jig/src/cli/run.rs`, and their command/output tests own the private probe's routing. The probe must not install, update, write state, or contact the network.
- `crates/jig/src/bootstrap/answers.rs`, `opts.rs`, `initial_copy.rs`, `renderer.rs`, `staged_render.rs`, `sync.rs`, and `managed_paths.rs` own render inputs and safe application. The managed ownership file is `.agent/jig-managed-paths.json`.
- `crates/jig/src/bootstrap.rs::run_update` currently performs only a full render. `UpdateOpts` has one positional repository path and `--force`; the new migration syntax is therefore `jig update /path/to/repo --launcher-only --force`.
- Embedded template copies under `crates/jig/src/bootstrap/embedded_template_snapshots` must match the live templates. `crates/jig/build.rs` refreshes them.
- `crates/jig/src/doctor.rs`, `info.rs`, `ui/snapshot.rs`, and `crates/jig-ui` expose the current pinned-version model to users.
- `scripts/release.sh` and fixture scripts under `scripts/fixtures` currently rewrite or infer product pins and cache paths.

The repository is both the Jig source tree and an adopted Jig harness. After template/root artifact changes, build `target/debug/jig` first and set `JIG_DEV_BIN=target/debug/jig` for all `scripts/jig` validation so the old cached binary cannot reintroduce the mismatch being removed.

## Milestone 1: define and test contract compatibility in Rust

Centralize the current/supported contract epoch in the context/contract layer rather than repeating numeric ranges in context and policy. Extend support to versions 2, 3, and 4.

Change `RepoConfig` and `ContractManifest` to deserialize `jig_version` as optional legacy data while retaining `deny_unknown_fields`. Apply version-specific validation:

- v2/v3 require a non-empty value in both files and require the two values to match;
- v4 accepts the fields if an interrupted/partial update left them behind, but does not expose or use them as an operational requirement;
- unsupported versions fail before command/tool execution.

Keep `TestRepoBuilder` capable of producing legacy v2/v3 repositories with matching fields, and make v4 fixtures omit them by default. Do not mechanically delete the many v2/v3 parser fixtures: they are useful backward-compatibility coverage. Add focused cases for a clean v4 repo, v4 with stale/mismatched legacy fields, v2/v3 missing or mismatched fields, contract 5 rejection, and the existing contract 1 rejection.

Add the hidden compatibility command. It must:

1. Resolve the supplied repository root without changing the caller's working directory.
2. Load `RepoContext` and run the same complete contract validation used by `jig check contract`, including commands, tools, work gates, and configured feature requirements.
3. Check the requested binary profile/capability: `default` requires `cfg(feature = "dev-proxy")`; `runtime` and `mcp` need only the base runtime.
4. Exit zero and remain quiet on success; return non-zero with an actionable diagnostic on direct invocation. Launchers/installers may suppress rejected-candidate diagnostics while trying the next candidate.
5. Perform no writes, receipt/state appends, installation, template resolution, or network access.

Add CLI tests for all three profiles under default features and at least a build/test path under `--no-default-features` proving that the stripped binary rejects `default` but accepts `runtime` and `mcp` for a compatible repo. Add a policy regression proving a structurally parseable repo with an invalid required command/tool is rejected by the probe.

## Milestone 2: emit contract v4 without product pins

Update the live templates so `.agent/jig-contract.json` emits `contract_version: 4` and neither it nor `.jig.toml` emits `jig_version`. Preserve `_src_path`, `_commit`, `_template_mode`, `_template_local_path`, and `template_source_url`; these describe template provenance, not runtime selection.

Remove `jig_version` from `RenderAnswers` and the renderer context. Keep legacy answer-file and CLI input compatibility narrowly:

- `RawAnswers.jig_version` may remain an optional ignored input so existing `.jig.toml`/answer files deserialize during update;
- keep `AnswerOpts --jig-version` as a hidden/deprecated ignored option for one compatibility window if existing automation depends on it;
- never copy either value into a newly rendered file or use it to select a runtime/template;
- add tests proving old input is accepted but rendered output contains no product pin.

Update the source repo's generated `.jig.toml`, `.agent/jig-contract.json`, `scripts/jig`, and `scripts/install-jig.sh` from the authoritative templates. Update example answer files and rendered-repository fixture expectations so newly generated artifacts contain no `jig_version =`, `"jig_version"`, or launcher `JIG_VERSION=` assignment.

Preserve release-binary initial template selection in `crates/jig/src/bootstrap/initial_template.rs`: choosing an official `v<package-version>` template tag is an internal release packaging decision, not a generated runtime lock. Unreleased/dirty development binaries may continue to use embedded templates.

## Milestone 3: make launch and installation contract-driven

Rewrite `templates/project/scripts/jig.jinja` and its dogfood copy while preserving POSIX syntax and current command-to-profile routing:

- normal runtime commands use `runtime`;
- `dev` and `proxy` execution use `default`, while their help-only paths may continue to use `runtime`;
- MCP uses `mcp` and remains prebuilt/non-installing.

Every candidate path—explicit development binary, source-checkout build, profile cache, PATH where allowed, and post-install output—must run the private compatibility probe before execution. Remove the final exact `--version` comparison. `JIG_DEV_BIN` remains authoritative and fails immediately when invalid or incompatible.

Rewrite `templates/project/scripts/install-jig.sh.jinja` and its dogfood copy to read `contract_version` from `.agent/jig-contract.json`, validate that it is numeric, and derive contract/profile cache roots. Preserve install locking and source-stamp invalidation. Candidate order and behavior must be explicit:

1. honor and strictly validate `JIG_DEV_BIN`;
2. in the Jig source repo, prefer `target/debug/jig` and source-stamped local caches before global PATH;
3. reuse a compatible profile cache;
4. reuse a compatible PATH binary only when no explicit install root requires population and the profile's existing behavior permits PATH;
5. install from recorded source provenance, then probe the resulting binary before publishing/reusing it.

For Git provenance, use `_commit` with `cargo install --rev` when it is a valid immutable revision. For a local source path, keep the local source-stamp behavior. For legacy v2/v3 without a commit, permit the recorded version tag only as the compatibility fallback described in the Decision Log. For embedded v4 sources with no commit, require the existing explicit fallback environment variable, warn about default-branch resolution, install from the configured source URL, and validate the result. Never fabricate `v<runtime-version>` for v4.

Keep MCP resolution installation-free and test that a missing compatible prebuilt candidate still fails rather than invoking Cargo or the network. Keep the existing full/runtime cache separation so a stripped binary cannot poison the default-profile cache.

Fixture coverage must include:

- a current stable binary accepting a v2/v3 repository whose two legacy fields say `0.2.0-beta.1`;
- a compatible PATH/cache candidate being reused despite a different product version;
- an incompatible contract candidate being skipped/rejected;
- a stripped candidate accepted for runtime/MCP and rejected for default;
- an explicit incompatible `JIG_DEV_BIN` hard failure;
- explicit install-root population even when PATH has a compatible binary;
- local source-stamp invalidation, immutable Git commit installation, legacy tag fallback, and opt-in embedded/default-branch fallback;
- no Cargo installation on MCP startup.

## Milestone 4: provide a safe launcher-only migration

Add `--launcher-only` to `UpdateOpts`, make it mutually exclusive with source-changing/full-render controls such as `--template`, `--template-mode`, `--recopy`, and `--vcs-ref`, and document that it exists to repair legacy runtime locks. It still requires the positional repository path and normal managed-ownership checks.

Implement the narrow path using the running binary's embedded live-equivalent launcher templates, not the repository's stored remote/fork/default-branch template source; otherwise an old source could regenerate the lock being repaired. Reuse the renderer and conflict machinery through a selected staging path that renders no other templates, seeds no repository files, runs no unrelated post-render task, and sets the staged active set to exactly:

- `scripts/jig`
- `scripts/install-jig.sh`

Before applying, require both paths to be present in the existing `.agent/jig-managed-paths.json`. For pre-manifest repositories only, accept the pair when both paths are regular non-symlink files and the launcher contains the generated installer reference plus either the legacy version marker or the new probe marker, while the installer contains the generated answers-file reference plus either the legacy exact-version helper or the new probe marker. Reject missing, symlinked, or arbitrary script pairs rather than claiming ownership, and do not create a managed-path manifest. Clear retirement paths. Do not stage or rewrite `.jig.toml`, `.agent/jig-contract.json`, `.agent/jig-managed-paths.json`, source identity, or any other managed file. Preserve normal conflict reporting and require `--force` when either launcher file differs.

Add integration tests that snapshot every managed file, locally modify an unrelated managed file, run launcher-only repair, and prove byte-for-byte that only the two scripts changed. Also cover the pre-manifest recognizable-pair fallback and arbitrary-pair rejection, a manifest that does not own both scripts, no-force conflict, incompatible option combinations, idempotent rerun, and JSON/human output identifying `render_mode: launcher-only`.

The supported operator migration is two-phase:

1. Bypass the blocked wrapper and invoke the newly installed binary directly: `jig update /path/to/repo --launcher-only --force`.
2. Confirm `scripts/jig --version` or a harmless command now resolves a contract-compatible runtime, then run the normal `jig update /path/to/repo --force` when ready to adopt contract v4 and all current managed templates.

This makes multi-project repair scriptable without forcing every project to accept an unrelated full-template diff at the unlock step.

## Milestone 5: align diagnostics and public surfaces

Update doctor so readiness is based on readable configuration plus contract/profile compatibility, not launcher/config/current release equality. Report the running runtime version and repository contract version as separate facts. Detect a legacy launcher containing `JIG_VERSION=` as migration-needed, preserve missing/unreadable launcher errors, and point directly to `jig update <repo> --launcher-only --force`.

Update `policy::contract_check` to report the contract epoch and current runtime identity without requiring a product pin. Update `info.rs` and its human output from `repo.jig_version`/“Pinned Jig” to `runtime_version` plus `contract_version`. Rename `crates/jig-ui::HarnessView.jig_version` to `runtime_version` and update server fixtures, dashboard rendering, snapshots, and serialization tests. These runtime-owned output changes are part of the v4 epoch commitment.

Update `docs/public-contract.md` first so it explicitly defines the contract epoch as covering both manifest-described tools and runtime-owned generated behavior. Then align `docs/configuration.md`, `docs/adoption.md`, `docs/repo-intent.md`, `CONTRIBUTING.md`, and `CHANGELOG.md` with:

- no generated product-release pin;
- immutable source commit when available, and explicit non-reproducible embedded fallback when it is not;
- contract/profile cache and candidate selection;
- v2/v3 compatibility semantics;
- the two-phase migration/repair command;
- the rule that a future breaking runtime-owned change requires a contract bump or support drop.

Preserve unrelated existing edits in documentation and Codex/TUI code while updating overlapping lines.

## Milestone 6: release automation, snapshots, and fixtures

Change `scripts/release.sh::require_version_consistency` and its prepare/check helpers so releases continue checking Cargo/package version consistency but stop rewriting or comparing generated `.jig.toml`, contract, and launcher pins. Replace those checks with negative assertions that authoritative newly generated/root fixture artifacts do not contain operational product-version fields. Keep official release template-tag behavior and fixture/example consistency checks.

Update fixture scripts:

- `scripts/fixtures/runtime-smoke.sh`: derive cache identity from numeric `contract_version`, exercise default/runtime profile probing, and prove stripped/default rejection plus MCP's no-install invariant.
- `scripts/fixtures/source-normalization.sh`: replace version cache assertions with contract cache assertions; name immutable-commit tests accurately; retain local/Git normalization coverage.
- `scripts/fixtures/rendered-repos.sh`: stop expecting version preservation and assert that new/full-updated renders omit product pins.
- `scripts/validate-fixtures.sh`: retain the POSIX launcher gate and all existing fixture orchestration.

Edit live templates first, then refresh embedded snapshots exactly once they stabilize:

```sh
JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
```

Review the snapshot diff to ensure it contains the same contract-driven launcher/installer logic and no stale `JIG_VERSION` assignment.

## Validation and acceptance

Run focused tests while iterating, then the repository gates. Exact test filters may be split where Rust's harness makes a module filter clearer, but failures must not be hidden:

```sh
cargo test -p jig-sh context::contract_tests
cargo test -p jig-sh bootstrap::tests::template_source
cargo test -p jig-sh doctor::tests
cargo test -p jig-sh info::tests
cargo test -p jig-ui
cargo test -p jig-sh --no-default-features --lib
cargo test -p jig-sh --no-default-features --test agent_doctor_mcp --test cli_json --test codex_launcher
bash scripts/validate-fixtures.sh
```

Before using the dogfood wrapper after root generated files change:

```sh
cargo build -p jig-sh --bin jig
export JIG_DEV_BIN=target/debug/jig
scripts/jig check contract
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
```

Acceptance requires all of the following observable outcomes:

- A current stable binary accepts a valid contract v2/v3 fixture containing matching legacy `0.2.0-beta.1` fields; it does not require its own product version to match.
- A v2/v3 repo still rejects missing or mismatched legacy fields, while v4 accepts absent or stale legacy fields and validates the rest of the contract.
- A contract 5 repo and a structurally valid but unsupported tool/command/profile fail before command execution.
- New init/adopt/full-update output has contract v4 and no operational `jig_version`, JSON `jig_version`, or launcher `JIG_VERSION` field.
- Launchers select only candidates that pass the repository-and-profile probe; stripped binaries never serve `dev`/`proxy`.
- Installer caches are keyed by contract/profile and every cached/new candidate is revalidated.
- MCP resolution performs no installation.
- Launcher-only repair changes exactly the two managed scripts and unlocks a legacy prerelease fixture; normal update can then complete the v4 migration.
- A pre-manifest recognizable legacy launcher pair can be repaired without creating ownership metadata, while an arbitrary or symlinked pair is rejected without mutation.
- Interrupted/mixed v4 states with leftover legacy fields can be rerun directly and converge.
- Doctor, info, UI, release automation, documentation, embedded snapshots, POSIX validation, formatting, Clippy, and backend tests all agree on the contract-driven model.

## Idempotence and recovery

The compatibility probe is read-only and repeatable. Installer caches retain locks, temporary destinations, post-install validation, and source stamps so an incomplete or incompatible install is never published as reusable. Cache entries are always probed, so deleting a bad contract/profile entry is safe but should not be necessary for correctness.

Both launcher-only repair and full update must be idempotent. If launcher-only repair is interrupted, rerun the same direct-binary command. If a full update is interrupted after writing the v4 contract or configuration but before the scripts, bypass the old wrapper, rerun the installed binary directly, and let the tolerant v4 legacy-field parsing converge the render. Do not weaken v2/v3's matching-field invariant merely to accept arbitrary reverse-order partial states.

Live files in `templates/project` are authoritative during development. If a snapshot refresh is interrupted or stale, rerun `JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh`; do not hand-edit embedded snapshot copies. Keep using `JIG_DEV_BIN=target/debug/jig` until the root harness and cached installed runtime are demonstrably compatible.

## Interfaces and dependencies

New private launcher protocol:

```text
jig __runtime-compatible --profile <default|runtime|mcp> <repo-root>
```

Success is exit status zero with no stdout. Failure is non-zero and may explain the rejected contract/profile on stderr. This protocol is internal to generated scripts but is covered by the contract epoch because old launchers must be able to query newer binaries.

New operator migration interface:

```text
jig update <repo-root> --launcher-only --force
```

No new external crate or shell dependency is required. Compatibility reuses repository parsing, policy validation, command/tool metadata, and compile-time feature information already owned by Jig.

## Revision note

Revised after code and lifecycle verification. The original plan correctly chose contract/profile probing but understated three blocking issues: the public contract did not cover runtime-owned behavior, embedded sources have no immutable commit, and full `jig update --force` is unsafe as an unlock mechanism for multiple modified projects. This revision defines v4 as a whole-harness epoch, specifies version-specific legacy parsing and partial-update recovery, adds an embedded-source fallback policy, and introduces a two-file launcher-only migration with concrete tests and commands. It also makes candidate order, profile rules, MCP non-installation, release automation, and final acceptance criteria executable rather than implicit.

Revised during implementation after exercising launcher-only repair against the dogfood repository. The repository has recognizable generated launchers but predates the managed-path manifest, contradicting the unconditional ownership-manifest assumption. The plan now specifies the implemented conservative signature fallback, its non-symlink and no-ownership-creation boundaries, and regression coverage for both accepted legacy pairs and rejected arbitrary scripts.

Revised after validation to replace the impossible unqualified no-default test command with the repository's actual feature-independent target split. The final outcomes now record the stable-runtime/legacy-prerelease probe, different-version PATH reuse, authoritative incompatible development override, real source-install fixtures, dogfood gates, and the exercised pre-manifest migration behavior.

Revised after the final migration-path audit to make staging itself surgical. The earlier render-everything-then-filter approach could be blocked by unrelated malformed managed blocks or agent-map inputs; the implemented selected renderer now materializes only the two launcher templates, and the regression suite proves a malformed unrelated `AGENTS.md` remains byte-identical and cannot block repair.
