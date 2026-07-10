# Changelog

## v0.2.0-beta.1 - 2026-05-23

### Changed
- Extract tests into dedicated modules
- Default new and adopted repos to repo-scoped local vaults with `--no-vault` and explicit `--global` escape hatches; in scoped repos, malformed `.jig.toml` vault policy blocks vault commands instead of falling back to legacy/global vaults, `JIG_VAULT_HOME` is now a vault base, and repo vault homes resolve below `$JIG_VAULT_HOME/scopes/` using a trusted local repo namespace.

### Other
- Implement scripts/jig agent doctor/bootstrap for Jig skills setup
- Migrate .jig configuration from YAML to TOML format
- Add GitHub Actions release workflow and CHANGELOG
- Add goal command for structured work harnesses with validation contracts
- Implement goal work harness with input validation and normalization
- Implement local development proxy with HTTP/HTTPS, process supervision, and multi-app support
- Improve README clarity, structure, and documentation
- Default jig init and adopt to official template source; add CONTRIBUTING guide
- Add build-time template pin policy for released vs unreleased Jig builds
- Upgrade jig contract to v2: command-based tools and enhanced validation
- Add progress tracking to bootstrap operations with formatted terminal output
- Add policy module for repository validation checks
- Release v0.2.0-beta.1 with policy module and version management updates
- Enhance CLI, agent runtime, and installation workflows
- Add help text with examples to CLI commands
- Add agent guides and improve launcher binary resolution
- Improve CLI UX and developer onboarding experience
- Add --print-plan-id and --no-receipt flags for improved CLI ergonomics
- Consolidate check commands under unified check subcommand
- Refactor CLI command handling, test organization, and runtime improvements
- Implement Jig Vault - encrypted secret management and brokered execution
- Refine jig work planning and gate handling
- Improve harness-only defaults and work summaries
- Hide SQLx unchecked queries TODO generator command from help
- Split Jig feature metadata crates
- Remove Makefile-backed jig runtime support
- Improve Jig developer workflow diagnostics
- Add developer workflow goal harness and enhance diagnostics
- Improve jig adopt UX defaults
- Improve adopt inference diagnostics
- Improve adopt inference reporting
- Improve adopt UX and embedded templates
- Add Codex review gates and state archive
- Improve adoption and work status UX
- Improve adopt inference and reporting
- Add rust-react scaffold templates
- Improve init preset discovery UX

## Unreleased

### Added
- Add `jig adopt --minimal` to render `.jig.toml` and `.agent/` scaffolding without scripts, workflows, or agent context files; stores `harness_footprint = "minimal"` so `jig update` keeps the thin footprint until a full re-adopt.
- Add `.agent/jig-managed-paths.json` as the strict exact-path authority for managed-file retirement; older adopted repositories establish it with an explicit same-footprint re-adopt before updating or contracting.
- Add `scripts/jig doctor` and `scripts/jig info` / `scripts/jig explain` for repo readiness and configuration snapshots.
- Add `scripts/jig work evidence` and the `jig.work_evidence` MCP tool for fresh/stale gate evidence inspection.
- Add `scripts/jig vault run --file VAR=SECRET` for Unix-only secret-file delivery and human-readable vault run summaries by default (`--json` for the full buffered payload).
- Add Jig local development proxy commands for stable repo-scoped dev hostnames, HTTP/HTTPS forwarding, WebSocket support, workspace app discovery, local certificates, and service file generation.
- Add `scripts/jig dev` and `scripts/jig proxy {start,stop,list,prune,run,alias}` runtime flows for supervised app processes, aliases, and route listing/pruning.
- Add `scripts/jig proxy cert {generate,status,trust,untrust}` and `scripts/jig proxy service {install,status,uninstall}` for certificate trust management and user service installation; trust-store mutations require `--accept-trust-scope`, and `proxy service install` requires `--accept-service-scope`.
- Enable the `dev-proxy` Cargo feature by default while preserving `--no-default-features` builds for contract/MCP-only consumers.

### Changed
- Minimal frontend adoption now retains frontend/dev metadata while deferring TypeScript commands, contract tools, work gates, scripts, workflows, and package validation until full-harness adoption.
- Breaking: CLI commands now print human-readable output by default. Pass global `--json` for structured automation output. The per-command `--summary` flag is removed; scripts that parsed default JSON must add `--json`, and scripts that passed `--summary` should drop that flag.
- Breaking: `jig init`, `jig adopt`, and `jig update` now print human-readable summaries by default and only print their full structured reports when `--json` is supplied.
- Default release builds of `jig init` and `jig adopt` to the official `jig-sh` template source pinned to the installed Jig version's release tag; unreleased or dirty local builds now use templates embedded in the binary when `--template` is omitted, with a checked-in snapshot for packaged builds and generated launchers that reuse a same-version `jig` on `PATH` and require `JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1` before falling back to configured or official install sources. `--template /path/to/jig-sh` and `--vcs-ref <ref>` remain available for explicit checkout or remote template code.
- Keep `jig init`, `jig adopt`, and `jig update` terminal output human-oriented by default; scripts that consumed the previous implicit JSON output must now pass `jig init --json`, `jig adopt --json`, or `jig update --json` for the full structured bootstrap report. `jig adopt` now previews by default, returns `render_mode = "preview"` until `--write` applies files, confirms interactive writes unless `--defaults` or `--no-input` is supplied, records `.agent/state/adopt-last.json` with backups for overwritten managed files, and reports conflicts in preview instead of blocking before review.
- Stop generating placeholder crate-level `AGENTS.md` files during adoption; `scripts/jig check agent-guides` now validates existing crate guides instead of requiring low-signal stubs for every crate.
- Remove generated Makefile support and hard cut the runtime to command-backed `scripts/jig` execution. Root `Makefile` files remain project-owned during adoption.
- Route generated TypeScript/web checks through direct `scripts/jig check typescript-*` commands backed by `scripts/check-webapps.sh`.
- Change the default generated `bootstrap_command` from `make deps` to `cargo fetch` so default command-backed repos do not require a project Makefile. Repos with web apps should set an explicit `bootstrap_command` when bootstrap must install web dependencies.
- Render schema-check commands, tools, and gates only when both SQLx and schema dumps are enabled; SQLx-only repos keep `sqlx-check` and migration support without a disabled placeholder schema gate.
- Command-backed `.jig.toml` `*_command` values now run through non-login `bash -c`; put any required toolchain setup in the configured command or project-owned scripts. `scripts/jig bootstrap` is available in supported command-backed repos.
- Generated Cargo command defaults now skip with exit 0 and a stdout note when no root `Cargo.toml` exists, so harness-only repos can verify immediately after `jig init`.
- Regenerating defaults with `jig update --recopy` rewrites `bootstrap_command`, `rust_fmt_check_command`, `rust_clippy_command`, `rust_test_command`, and `rust_test_locked_command` to the no-root-`Cargo.toml` skip form unless the repo has customized those answers.
- `scripts/jig work check` now rejects unknown or closed plan IDs before running tools; `scripts/jig work gates` still reports status for any existing plan, including closed plans.
- `scripts/jig work gates` and `scripts/jig work evidence` keep top-level `ok` as command success and expose gate health through `overall`, `gates_ok`, and per-gate `status`.
- `scripts/jig work gates` now prints freshness reasons, receipt diffs, changed paths, and plan-state-aware next steps; automation should pass `--json` instead of grepping human text.
- `scripts/jig work receipts` and `scripts/jig vault run` preserve short multiline output previews for readability; automation should pass `--json`.
- `scripts/jig vault secret set NAME` now defaults to hidden terminal input when run interactively; non-interactive callers must pass `--value-stdin`.
- `scripts/jig dev` now prints a compact APP / URL / STATUS / PID table and dev-proxy failures include more specific likely-fix guidance.
- Release automation that builds Jig from a git checkout should fetch tags before building, or set `JIG_ASSUME_RELEASE_BUILD=1` after validating the workspace version and release tag.
- BREAKING for local dogfooding: resolve `JIG_DEV_BIN` directly instead of copying it into the Jig cache, so local runtime changes use the current development binary after version validation.
- Hard-fail `scripts/install-jig.sh` when `JIG_DEV_BIN` is set but missing, non-executable, or resolves to a binary whose version does not match the generated repo instead of falling back to cached runtime selection. Direct callers of `scripts/install-jig.sh` should use `scripts/jig`, set a matching `JIG_DEV_BIN`, unset it, or run the normal cached installer path.
- Split the local development proxy runtime into the `jig-dev-proxy` workspace crate used by the `jig-sh` CLI.
- Refuse to share an unrelated proxy found on the requested HTTP port unless it is registered in the same proxy state directory.
- Proxy list/status output now includes loopback health-probe fields such as `health_pid`, `handshake_ok`, `pid_matches_proxy`, and `running`.
- Prune legacy live process routes that do not have process start tokens on platforms where Jig can verify process start identity.
- BREAKING: Strictly reject unknown `.jig.toml` config fields so typos and stale local config fail fast.
- Migration note: remove or rename unknown `.jig.toml` keys reported by the load error before rerunning `scripts/jig`; previously ignored local keys now block startup. This applies to top-level keys plus `[work]`, `[agent_tooling]`, `[agent_tooling.codex]`, `[dev]`, `[[dev.apps]]`, and legacy `[[frontend_apps]]` entries.
- BREAKING migration note: Jig now rejects new `schema_dump_enabled = true` answers unless `sqlx_enabled = true`; `jig update --recopy` normalizes legacy SQLx-disabled repos that still have `schema_dump_enabled = true` back to `false`.
- `jig-sh` now enables the `dev-proxy` feature by default, which pulls in the TLS/HTTP proxy stack for library consumers unless they opt into `default-features = false`.
- MCP/contract-only consumers can build with `default-features = false`; in that profile, `dev` and `proxy` still parse but return clear unsupported-feature errors instead of linking the proxy stack.
- Keep `web_package_manager = "bun"` as the default for legacy `[[frontend_apps]]`; configure `dev.apps` or set explicit commands when legacy apps should launch with another package manager.
- `jig init` and `jig adopt --defaults` now default omitted SQLx answers to a tooling-only profile unless a migration directory is supplied, emit a note about that inference, and keep noninteractive adoption usable without extra SQLx flags.
- Behavior change: SQLx adoption now leaves schema dumps disabled unless `schema_dump_enabled = true` or an explicit `schema_dump_command` is supplied, so first-run `scripts/jig doctor` does not require a repo-owned `scripts/dump-schema.sh` before the repo has implemented one.
- `jig adopt --json` now reports retired cleanup paths separately as `adoption_profile.retired_managed_files` instead of mixing them into active `managed_files`.
- `jig init --json` and `jig adopt --json` now expose the managed-file summary as `render_report`.
- Backend-only adoption no longer writes disabled web workflow/scripts; previously generated backend-only web scaffolding is now treated as retired managed output during refresh.
- Generated frontend coverage enforcement now uses `scripts/enforce-coverage.cjs` so ESM packages with `"type": "module"` can run the gate; old `scripts/enforce-coverage.js` is retired, and generated guidance now names the required `coverage/coverage-summary.json` artifact.
- `jig adopt` now detects nested Rust crates even when a repo has no root `Cargo.toml`, and generated Rust check commands run each inferred nested manifest instead of reporting a false skip.
- `scripts/jig doctor` now includes the detail for required failing checks, and required-tool probing recognizes `cargo sqlx` as needing the `cargo-sqlx` subcommand.
- `scripts/jig work gates` now defaults to the single open work plan, matching `scripts/jig work evidence`; pass `--plan-id` when multiple plans are open or to inspect a closed plan.
- `scripts/jig doctor` now reports missing Codex Jig skills as optional setup instead of blocking overall repo readiness.
- Behavior change: repo-local `scripts/jig` launchers now run the Jig binary from the owning repository root even when invoked by absolute path from another current directory. This makes check/dev/proxy/agent commands consistently operate on the owning repo; `jig init`, `jig adopt`, and `jig update` still resolve relative destination/template paths against the caller's original directory.
- Require `--accept-trust-scope` before installing the Jig Dev Proxy local CA through the platform trust tooling.
- Vite proxy host support relies on Vite's `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS` compatibility hook; configure Vite `server.allowedHosts` explicitly if a Vite release changes that hook.
- Windows builds parse and run non-certificate proxy flows, but automatic HTTPS certificate generation/trust remains unsupported until owner-only ACL hardening for private key files is implemented.
- Document `JIG_PROXY_STATE_DIR`, proxy CA trust scope, and local dev proxy usage more explicitly.
- Generated `--preset rust-react` bootstrap commands now install generated frontend app dependencies before `scripts/jig dev` runs them.
- Generated `--preset rust-react` frontend dev scripts now install missing app dependencies before launching the dev server, so a fresh scaffold can run `scripts/jig dev` even before an explicit `scripts/jig bootstrap`.

### Security
- `vault run --file` writes each secret to a private Unix `0600` temp file under a `0700` temp directory, wipes brokered temp files before normal cleanup, and removes the temp directory when the brokered process exits; this promotes `tempfile` to a runtime dependency of `jig-vault`.
- Harden proxy stop, certificate writes, CA regeneration, and TLS handshake behavior for local development sessions.
- Harden Vite argument handling, including rejection of mismatched explicit Vite port flags, backend response parsing, WebSocket proxy-header scrubbing, and route-cache invalidation.
- Harden LAN proxy exposure, alias registration, workspace discovery traversal, process-route liveness checks, and route persistence.
- Harden state directory permissions, service-file quoting, and local proxy shutdown behavior.
- Harden background proxy startup, runtime file replacement, request-host validation, installer locking, private-key reads, and workspace config file reads.
- Reverify process-route listener ownership while holding the route lock, restore the previous route file after failed route publication, isolate service temp paths, harden certificate/trust temporary reads, prefer recorded template commits for remote runtime installs, and defer release tag pushes until all crates publish successfully.
- Treat template source metadata as a runtime-install trust boundary: recorded hex `_commit` values pin the exact remote Jig revision used by `scripts/install-jig.sh`, and contract checks now keep the installer script and template mirror in sync.
- Bound the Jig Dev Proxy local CA lifetime to two years, avoid broad bare-TLD CA constraints for non-`.localhost` TLDs, and verify macOS trust installation before recording Jig's trusted-CA marker.
- Reject backend response headers with whitespace before the colon, retry transient TLS leaf cert/key rotation mismatches, escape `$` in systemd `ExecStart` values, fail closed on oversized workspace glob expansion, and extend proxy state lock waits.
- Document that shell-form `[[dev.apps]].command` and top-level `.jig.toml` `*_command` values are trusted repo-configured shell execution; prefer `argv` when app arguments should be passed literally.

## v0.1.0 - 2026-05-12

### Added
- Scaffold agentic-rust-kit
- Add jig CLI tool and migrate to jig.sh branding
- Add template mode support for local git templates
- Add agent planning and workflow infrastructure
- Add state-summary tool and enhance receipts filtering
- Add block-managed root AGENTS.md to preserve repo-specific content during adoption

### Fixed
- Address extraction review findings
- Persist full copier template ref
- Unify kit config and update flow
- Make template source normalization safe

### Changed
- Split bootstrap module into separate concerns
- Split state module into separate concerns
- Extract tool definitions and remove memory tools from contract
- Extract bootstrap module concerns into separate files
- Improve encapsulation in template_source module
- Extract request parsing into runtime/requests module
- Extract work dispatch logic and improve runtime architecture
- Improve type safety, reduce bootstrap test cost, and organize runtime modules
- Move MCP tests to tests/mcp module and add Cargo.toml metadata

### Documentation
- Make copier update example noninteractive
- Distinguish recopy from update
- Make recopy noninteractive

### Tests
- Make fixture update check clean
- Fix update assertion
- Drop pyyaml dependency
- Add fixture infrastructure and agent documentation
- Refactor receipt creation and add plan state validation tests

### Other
- Improve work gate validation and receipt tracking
- Settle Cargo workspace in fixtures and document gate evidence requirements
- Add release script and normalize jig-sh package name
