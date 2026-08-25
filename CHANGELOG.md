# Changelog

## Unreleased

### Added
- Add the contract-v6 agent-native repository model with checked-in components, actions, profiles, affected-selection policy, immutable content-addressed run plans, durable target runs, and target/profile evidence gates.
- Add `jig check` selectors with `--profile`, `--affected`, `--explain`, and `--fail-fast`, plus repository-aware `jig info` subjects and `jig migration add`.
- Add the `go-react` scaffold preset, `jig-go` repository adapter, Go test workflow, Huma/Chi API skeleton, optional PostgreSQL/sqlc/Goose integration, and generated React client-contract checks.
- Model SQL migration layout as `flat_migrations` or `versioned_artifacts`, preserving flat compatibility while suppressing migration-add contracts and runtime mutation for versioned schema trees.
- Harden the Rust/React OpenAPI split with an optional, independently deployable admin HTTP crate and binary, transactional all-contract generation, public-artifact boundary scans, and request-ID-bearing JSON API errors.
- Generate split Utoipa public/admin OpenAPI contracts and separately owned Hey API TypeScript clients in the Rust/React scaffold.
- Generate a root Rust/React quickstart, a disposable Docker-backed PostgreSQL integration-test command, and an application-owned admin authorizer whose default deployment policy denies every matched route.

### Changed
- Breaking: Limit the Jig CLI, generated harness, development proxy, Vault, and owned-process supervision to Linux and macOS hosts; native Windows host support is removed in this release.
- Breaking: Contract-v6 MCP clients use the stable `jig.inspect`, `jig.plan_run`, `jig.execute_run`, and `jig.cancel_run` surface instead of directly invoking per-manifest execution tools; submitted plans are re-derived from current checked-in authority before execution.
- Keep read-only work-gate, evidence, and status inspection available when a configured tool, repository target, profile, or catalog no longer resolves; report the affected required gate in-band as `unsupported` with a reason while contract validation, execution, and work finish continue to fail closed.
- Make configured checks non-interactive and uniformly supervised for timeout, cancellation, process-tree cleanup, and bounded stdout/stderr capture.
- Let `jig work check` finish configured evidence targets after a legacy check fails so one invocation records complete gate evidence, while preserving a failing batch result and collecting each configured tool error in structured output.
- Raise the Jig workspace minimum supported Rust version to 1.88 and lift dependency pins that only preserved Rust 1.85 compatibility.
- Update the generated frontend stack to current compatible exact releases, including Node 24.19.0 LTS with Node 24 types, Astro 7.2.2, Vite 8.2.1, React 19.2.8, npm 12.0.2, pnpm 11.22.0, Yarn 4.18.0, and shadcn 4.18.0; keep TypeScript on its supported peer-major line.
- Require Rust 1.94 in every generated Rust/React workspace, update database variants to SQLx 0.9, and make Doctor reject an older Rust runtime or a SQLx CLI from a different minor line.
- Bootstrap the frontend workspace and create its selected root lockfile before checking PostgreSQL configuration, so database availability no longer blocks frontend setup.
- Keep Codex usage projections fixed to their inspection sample while showing the sample age in the selected-account details.
- Keep available-window projections visible and clearly labeled as partial while withholding recommendations until every returned window has projection metadata; usage samples older than 15 minutes are also excluded from ranking.
- Report human Codex usage as quota remaining with window/reset context instead of the former used-percent/window shorthand.

### Fixed
- Keep accepted MCP repository workers alive through transport shutdown, preserve mixed Go and Rust/SQLx contract-v6 models during recopy, reject symlink-redirection in Go component roots, and make generated Go CI observe vendored modules and SQL inputs while reserving Linux for Docker-backed PostgreSQL tests.
- Serialize native migration version allocation and advance collisions by valid UTC seconds so different Goose or SQLx names cannot share a backend version; reject component roots and action inputs under the `.agent/` tree excluded from source identity.
- Keep contract-v6 execution fail-closed across authority changes, source drift, cancellation-poll failures, and work-plan finish; refresh long-lived MCP, status, and UI repository contexts before they consume current configuration.
- Make Vault PTY integration tests own their private controlling terminal so `/dev/tty` sizing and resize-clear assertions stay hermetic under TTY-wrapped gate runners.
- Make run cancellation cursors atomic with queued-event persistence, preserve prior target failures during abandoned-run recovery, reject unsafe run identifiers and live-journal restores, and retain bounded stdout/stderr evidence for configured-command and schema-generator overflow failures.
- Preserve non-UTF-8 untracked paths in schema snapshots, reject truncated Git authority output, and keep receipt-overflow uncertainty anchored to the exact boundary record.
- Preserve both bounded output streams in overflow diagnostics, route native pre-start stops through the shared target finalizer, track Go workspace/module inputs in every generated policy workflow, avoid duplicate frontend artifact scans, and omit database configuration from database-free Go scaffolds.
- Preserve project-owned legacy Go/PostgreSQL `sqlc_check_command` overrides across init and update, validate frontend contract-check app arguments and action working directories before execution, bound archive evidence indexing and fail closed when exact protection cannot be computed, keep superseded duplicate receipt groups closed, terminalize accepted foreground runs after infrastructure errors, and reconcile lease-abandoned runs before applying run-history archival.
- Keep source-epoch trust anchored when a declared worktree-mutating target never executes, and pass schema output directories to Git as literal pathspecs.
- Resolve contract-v6 schema freshness checks through the owning schema-dump action's complete command runner, omit the obsolete duplicate top-level generator from new v6 renders, and preserve executed generator exits as target failures with their child status and output.
- Keep native contract checks behind complete execution authority, require native migration actions to declare their mutating semantics, delegate contract-v6 compatibility aliases to their owning action runners and arity, validate schema action environments and output paths consistently, keep active plan-linked repository runs from racing work-plan closure, and accept built-in action names in multi-selector `jig check` requests regardless of selector order.
- Restore live target phases, stdout/stderr, and heartbeats for foreground contract-v6 checks; preserve feature-specific unavailable-check diagnostics when repository flags are present; and expose the bounded count and elapsed time of per-target source observations.
- Keep affected frontend public-boundary checks sensitive to their ignored-by-default documentation artifact roots, and reject cyclic component or action dependency graphs when the repository catalog loads.
- Let explicit dev stop and replacement recover definitively dead orphaned session records and exact-owned stale routes without signaling persisted PIDs; persist preflight-cleanup and pre-spawn obligations, distinguish uncertain process observations, preserve ambiguous records by default, add a targeted `dev stop --forget-ambiguous-orphans` repair that never signals stored PIDs or overrides a live or uncertain registered identity, and report successful recoveries with retained app diagnostics and typed forgotten ambiguities separately from blocking warnings.
- Name `scripts/check-webapps.sh bootstrap` in missing-dependency failures, preserve lockfiles across repeat bootstrap, tolerate Astro's top-level runtime cache without reinstalling, approve only the reviewed esbuild install script under npm, and emit Vite configs compatible with its native loader.
- Keep Codex projection age, reset countdowns, staleness, and recommendations live after inspection; keep stale state visible at common terminal widths, avoid claiming a usage sample for incomplete inspections, preserve unexpected window durations in projections, mark sampled remaining quota stale with its projection, show sub-minute resets as `<1m`, expire projections at their first contributing reset, exclude generic fallback buckets from recommendation, derive lone-window roles from duration, and normalize rounded time-unit boundaries.

## v0.2.0 - 2026-08-05

### Added
- Add status-provider v1 protocol
- Manage repo-scoped dev sessions
- Run and aggregate status providers
- Add interactive status TUI
- Add cancellable status collection
- Standardize JSON errors and work validation
- Add interactive Codex home launcher
- Add home-aware Codex session resume
- Configure Codex home for loop workers
- Add command availability inventory
- Namespace SQLx project commands

### Fixed
- Keep dev session cleanup signal-responsive
- Keep runtime installation portable on stock macOS Bash 3.2, source-aware for Git and non-Git checkouts, refreshable for mutable sources, and fail-closed for unpinned remotes or untrusted PATH wrappers.
- Keep help, `doctor`, contract checks, adoption, and launcher repair reachable when repository configuration is malformed, with traceback-free and directly executable recovery guidance.
- Keep bare launcher help reachable under a broken repository contract, anchor relative runtime source paths to the repository, and reject non-file launcher paths in Doctor.
- Make launcher-only repair transactional and self-contained: preflight its recorded source, warn when embedded repair templates replace source-specific launcher customizations, preserve the legacy contract epoch, validate the running repair binary, seed truthfully fingerprinted caches, refresh the cheap identity stamp after a digest fallback, roll back published scripts if real runtime seeding fails, and restore prior caches if the rendered-script transaction cannot commit.
- Drain noisy owned subprocesses promptly while retaining hard time, memory, capture, cancellation, and cleanup bounds.
- Keep stale missing Codex-home candidates from blocking a unique session resume
- Recognize supported Codex app-server missing-thread response variants during resume lookup
- Preserve session-lookup cancellation even when Codex app-server emits stderr

### Breaking
- Generated repositories move from contract v3 to v4 and no longer pin `jig_version`/`JIG_VERSION`. Run a current Jig binary with `jig update <repo> --force` to migrate the full harness. If the legacy wrapper cannot start, first run `jig update <repo> --launcher-only --force`, then perform the full update; `doctor` treats the unmigrated launcher as a required migration and exits nonzero while v2/v3 remain runtime-readable. A repaired legacy launcher depends on its seeded compatible cache until full migration, so fresh clones, cacheless CI, or cache cleanup require a current external Jig binary to repeat the narrow repair.
- Remote runtime installation now requires the repository's immutable hexadecimal `_commit`; legacy unpinned repositories must explicitly acknowledge default-branch installation with `JIG_INSTALL_ALLOW_UNPINNED_REMOTE=1` before migrating their source metadata.
- A usable remote `_src_path` is now the authoritative runtime source ahead of `template_source_url`; repositories that intentionally relied on the fallback URL must update `_src_path` before their next cache install.
- The generated launcher passes its contract epoch, build profile, and repository root to the selected binary, which validates the complete repository contract in-process before ordinary command dispatch and reuses that loaded context process-wide. That launcher-provided root is authoritative over an inherited `JIG_REPO_ROOT`. Contract-invalid repositories cannot run `work`, `dev`, `mcp`, `info`, or ordinary gates such as `check fmt` until repaired; `doctor`, `adopt`, `codex`, `init`, `presets`, `update`, and `check contract` retain a capability-only escape hatch.
- The published `jig-ui` crate changes `HarnessView::jig_version` from `String` to `Option<String>` and adds `runtime_version` so v4 snapshots distinguish a legacy generated pin from the executing runtime. Both fields have Serde defaults so current readers can deserialize older snapshots, and the public `display_runtime_version` helper implements the legacy fallback. Pre-v4 readers cannot deserialize the new v4 snapshot shape.
- Structured `jig info --json` and `jig doctor --json` output now reports nullable `repo.jig_version`, adds `repo.runtime_version`, and replaces product-version runtime statuses with the contract-oriented `compatible`, `migration needed`, `unreadable`, `missing`, `unsupported`, and `outdated` vocabulary.

### Changed
- Replace generated Jig product-version locks with contract-v4 runtime/profile compatibility, contract-keyed caches, explicit PATH-binary trust, and a launcher-only repair path for legacy repositories.
- Keep Codex usage projections fixed to their inspection sample while showing the sample age in the selected-account details.
- Keep available-window projections visible and clearly labeled as partial while withholding recommendations until every returned window has projection metadata; usage samples older than 15 minutes are also excluded from ranking.
- Report human Codex usage as quota remaining with window/reset context instead of the former used-percent/window shorthand.
- Treat `_commit` as an installer source locator rather than a runtime product lock: a proven same-contract runtime, including an explicitly recorded repair seed, may satisfy the cache until the configured source state changes.
- Plan dev session management
- Split dev lifecycle modules
- Close dev session management work
- Close status provider aggregation work
- Centralize test repository fixtures
- Share session id validation
- Deny redundant clones and stack arrays
- Inherit package metadata
- Extract owned process runner
- Consolidate duplicated helpers
- Consolidate shared utility helpers
- Apply tier 1 mechanical cleanups
- Apply tier 2 rust cleanups
- Split embedded templates and reorganize test modules
- Modularize CLI output formatting
- Remove obsolete diagnostic results
- Decompose status validation
- Move diagnostic rendering into CLI
- Extract init mutation transaction
- Isolate receipt archival
- Group invalid review context
- Borrow receipt status projections
- Aggregate required gate failures
- Make crate root inference infallible
- Isolate initial template policy
- Centralize root command categories
- Consolidate SQLx command modules

### Tests
- Run rendered-repository and runtime-source behavioral fixtures in pull-request CI.
- Move doctor tests beside module
- Move adopt inference tests beside module
- Move git bootstrap tests beside module
- Move path bootstrap tests beside module
- Move prompt registry tests beside module
- Move child lifecycle tests beside module
- Move windows launch tests beside module
- Move cleanup tests beside module
- Move dev session tests beside module

### Other
- Add generated gitignore template
- Implement V2 repo-scoped vault model for init, adopt, and vault commands
- Refactor bootstrap answers into modular structure with dev and vault submodules
- Make jig launcher POSIX sh compatible
- Make jig update output human by default
- Refactor doctor output to distinguish required and optional setup steps
- Add prompt registry and management CLI commands
- Add test execution and prompt registry improvements
- Add landing page and improve bootstrap/scaffold structure
- Update bootstrap scaffolding and Rust-React template configuration
- Refactor CLI and command modules into focused submodules
- Refactor dev-proxy service module into focused submodules
- Add loop orchestration runtime
- Update loop orchestration, CLI output, and documentation
- Add runtime config module and refactor bootstrap orchestration
- Extract loopback UI into dedicated jig-ui library crate
- Add full-stack scaffold E2E and harden Jig workflows
- Add error page template and improve proxy server configuration
- Add TanStack Router integration and refactor frontend scaffolds
- Refactor jig dev command for foreground process control and signal handling
- Remove leading blank lines from moved tests
- Normalize moved test files
- Refactor state record storage boundaries
- [verified] Fix Rust 1.97 release lints
- [verified] Fix Linux release test build
- [verified] Repair release gate regressions
- [verified] Apply release formatter output
- [verified] Serialize release test gate
- [verified] Handle non-UTF-8 Linux process names
- [verified] Make launcher test release-version aware
- [verified] Remove legacy service module filename
- [verified] Fix optional installer feature argument
- [verified] Report fixture validation failures
- [verified] Make fixture version assertion release aware
- [verified] Remove fixture ripgrep dependency
- [verified] Preserve fixture failure status
- [verified] Identify crates.io release probes
- [verified] Allow complete release validation
- [verified] Stabilize command inventory checks

## v0.2.0-beta.1 - 2026-05-23

### Changed
- Allow `pr_manager` loop workflows to configure the exact Codex home used by unattended repair workers. Conventional bare names resolve deterministically, non-conventional homes require explicit paths, omission continues to inherit ambient `CODEX_HOME`, and same-version loop JSON distinguishes `codex_home_configured` from `codex_home_resolved` without changing the generated contract version.
- Close a Unix process-group late-member race in doctor/preflight probes, supervised development apps, and vault brokered runs. While the exact direct child remains unreaped and pins its PGID generation, forced cleanup now re-sends group `SIGKILL` under the original absolute deadline before each Linux live-member scan or macOS exact sole-leader proof; `ESRCH` and Darwin `EPERM` remain inconclusive, and unpinned numeric-PID stops are not retried.
- Keep generated Rust/React source rustfmt- and strict-Clippy-clean across supported repository names, every database branch, and custom migration paths by isolating rendered crate identifiers behind fixed aliases, rendering dynamic string/macro operands in stable blocks, and narrowly acknowledging the intentional literal-only `concat!` blocks. Reject normalized Rust package stems above Cargo's usable 216-byte generated-artifact boundary before destination mutation, and bound long-name fallback API host labels without changing short-name output.
- Normalize a missing, empty, or exact cache-only real `node_modules` install root to the same dependency-proof state, so ordinary Vite, Vitest, TypeScript, and Playwright caches cannot stale a root-hoisted workspace receipt after bootstrap. Unknown entries, type replacements, nested caches, packages, metadata, symlinks, and launcher bytes/modes remain attested; the v5 stamp and v3/v2 node-modules marker formats remain unchanged, though an older receipt stamped with an already-present cache-only root safely reattests once.
- Route every generated web workflow package script through the configured-app checker boundary, including lint, typecheck, build, coverage, and browser E2E. npm uses an exact current-package execution contract that ignores hostile ambient workspace, global, missing-script, and dependency-omit selectors while preserving explicit application environment plus registry, authentication, layout, peer, and lifecycle policy; Yarn revalidates its authority immediately before execution. User-authored dev commands continue inheriting the caller environment unchanged.
- Keep generated dependency checks portable and identity-safe on the supported Unix shell surface: stock macOS Bash 3.2 handles root and standalone scopes with no workspace-member arguments, Yarn authority enumeration propagates producer failures before stamping, and install coordinators use Bash's owned job table instead of probing a reaped numeric PID. The dependency receipt and node-modules marker versions are unchanged because successful fingerprints and artifact proofs do not change.
- Serialize and reuse cleanly retired doctor signal sessions in long-lived MCP processes while permanently poisoning unsafe retirement, retaining the session guard through restored-signal redelivery, and failing closed if handler quiescence times out after a signal was recorded. Jig-owned Codex capability probes now scrub executable Bash startup, option, trace, and exported-function controls. Existing-destination init uses a conservative two-generations-per-path plus explicit-repeat descriptor budget, allowing the default scaffold under macOS's soft limit of 256 without weakening rollback identity.
- Make generated npm installs immune to ambient dry-run, lock-only, bin-link, workspace-selection, platform, and related install-shaping settings before recording dependency readiness; old receipts from the weaker command contract are invalidated. PostgreSQL browser E2E now uses an Ubuntu service-container runner independently of the repository-wide CI runner, database workflows carry their SQLx metadata authority explicitly, and SQLx 0.8 Rust/React scaffolds reject unsupported custom metadata directories before creating output.
- Follow npm's real dependency authority throughout generated repositories: `npm-shrinkwrap.json` takes precedence over `package-lock.json`, both names schedule and key the relevant workflows, and frozen/bootstrap installs explicitly include development, optional, and peer dependencies despite ambient omit settings. Generated Rust and E2E workflows also watch both supported rustup toolchain filenames.
- Harden Jig-owned Bash probes by removing inherited startup, option, trace, and byte-exact exported-function controls without changing the normal environment inherited by configured repository commands. Linux doctor, vault, and dev-proxy cleanup now check one absolute phase deadline at every procfs/result boundary, and proxy leader exit no longer replenishes TERM or SIGKILL-confirmation time.
- Make native-Windows doctor proxy diagnostics execute the repository launcher through a sanitized Bash boundary, preserving its proxy-capable profile selection. Linux process-group cleanup now carries one absolute deadline through procfs enumeration, stat reads, and fallback membership probes, and generated Node 22 workspaces compile against matching Node 22 typings from one rendered authority.
- Close the final signal and external-process ownership cutovers: post-retirement handlers take the safe conventional-exit claim before stale-generation returns, and doctor supervises SQLx, Codex marketplace, and proxy/service subprocesses under one bounded cancelable session in every feature mode. Windows argv launch now fails when no inspected executable or batch shim resolves instead of delegating a filtered bare name back to ambient PATH lookup; platform-specific regressions, helpers, imports, and metadata predicates compile cleanly on their supported targets, with native Windows default and no-default all-target Clippy enforcing that boundary.
- Close the remaining doctor and path-lifecycle boundaries: one serialized signal owner spans every external check in a doctor invocation, reaps before restored signal redelivery, and permits a later invocation only after clean retirement; proxy JSON has a separate bounded stdout allowance tied to route-state size; escaped-pipe tests use exact PID/start identities and confirmed cleanup. Managed output components now require valid Unicode, Windows PATH search skips drive-relative entries, and foreground termination retires resource/generation state before restoring handlers so a final signal cannot be erased.
- Complete managed-output portability by rejecting Win32-forbidden punctuation, control bytes, and raw backslash aliases before init, adopt, or update mutation. Tooling-only fixture assertions now use fixed-string contract keys, generated API tracing enables its binary target as well as library/HTTP targets, and dev preflight skips only historical checkers that do not advertise dependency readiness.
- Reject Windows drive-relative development executables, retain targeted CTRL+BREAK grace for Job-owned descendants after a wrapper exits, and make termination-session retirement hand late signals to the existing exit claim instead of erasing them. No-default doctor proxy diagnostics now use bounded owned-tree capture, overflowing process deadlines remain genuinely unbounded, and signal/process regressions use atomic markers, kernel-assigned proxy ports, exact identities, and semantic barriers. An explicit `--http-port 0` exposes that ephemeral runtime mode while configured and service ports remain stable and nonzero.
- Make generated Node-version policy fail closed: applicable `.node-version` files must be bounded real regular files reached through stable repository directories and contain one version token, true absence is the only fallback status, and CI writes its pinned fallback only to runner-owned temporary storage. SPA coverage now follows production `src` files instead of enumerating starter modules, generated SQLite locking uses `fs4`, E2E workflow path filters share one template authority, and Bun versions remain YAML strings.
- Preserve ordered doctor semantics across a plain leading `! DATABASE_URL=...` prefix, retain every distinct termination signal observed during one owned SQLx probe while keeping the first reason sticky, and redeliver those signals only after cleanup and handler restoration. Vault supervision now keeps completed capture/observer outcomes ahead of a newly expired deadline and treats platform-proven process-group quiescence as authoritative after a successful direct fallback.
- Resolve Windows `cmd.exe`, `taskkill.exe`, and `icacls.exe` from the native system directory instead of PATH or a hard-coded drive, while requiring an explicit `ComSpec` to be absolute and usable. Generated dependency probes likewise reject missing or relative `ComSpec` values instead of invoking a bare `cmd.exe`. Foreground route cleanup shares one absolute lock deadline across every child, retry, and Drop fallback; process tests use Rust-owned listeners and semantic synchronization instead of Python, free-port handoffs, or scheduler ceilings, and Windows CI pins the generated-test Node runtime.
- Apply portable file-plan collision validation to managed harness-only, adopt, and update outputs even when no scaffold is selected. Remote template clone commands retain a narrow Git transport/authentication environment while repository/object/index/config/trace redirections stay scrubbed and mutating Git commands remain isolated.
- Harden initialization and subprocess authority at the remaining race boundaries: validate both pre-existing and concurrently published Git metadata as the exact destination worktree, preflight the real filesystem's no-replace directory semantics, quarantine identity-checked cleanup entries, and supervise doctor probes until their retained process tree is proven quiescent under one cleanup deadline. Windows foreground cleanup now attempts targeted CTRL+BREAK before its bounded fallback/Job force path, and direct native executables no longer inherit batch-only path limits.
- Make init reject parent-relative and incomplete Windows destinations before prompts or filesystem mutation, canonicalize the deepest existing ancestor, privately stage and atomically publish wholly new repositories, and quarantine existing entries before replacement so rollback restores only exact Jig generations while preserving contention and recovery material. Git initialization now scrubs repository-redirection inputs, uses explicit contained paths, validates self-contained metadata, and publishes without replacement; portable output-collision preflight is O(n log n), and targets without a safe no-replace primitive reject init before interaction. Normalize portable frontend/dev directory identities while keeping canonical filesystem paths through readiness and converting Windows paths only at process spawn.
- Strengthen generated dependency receipts across every authoritative workspace member, content-hash executable launchers, bind pnpm's hook-disabled layout/configuration/environment contract, and exclude only type-checked top-level tool runtime caches from each actual node-modules install root. Dependency queries now preserve their verified-absence versus invalid-authority exit status, pnpm honors the effective `package.json` / `package.json5` / `package.yaml` authority through a bounded hook-disabled parser, and Yarn validates root-lock authority before reporting Node policy. Add explicit dev-only dependency bootstrap guidance and include PID namespaces plus exact worker generations in install ownership. Windows pnpm/Yarn metadata lookup now uses validated `PATHEXT` search, and Bun workflow values are serialized as YAML strings.
- Arm proxy cleanup before startup spawn, preserve graceful handling for the first signal, condition route deletion on exact process ownership, bound unsupported-platform output cleanup, and give Windows Job Objects generation-owned final handles. Astro opts out of agent background mode correctly, and in-memory SQLite pools retain a database-owning connection without idle/lifetime reaping or cancellable pre-acquire health checks; private cache additionally serializes checkouts.
- Treat derived `JIG_DEV_<APP>` coordinates as current-session topology, prefer the managed API origin in generated SPA/admin Vite configs, and make Playwright overwrite both generic and namespaced origins for its isolated real backend.
- Make Windows required-tool PATH lookup prefer validated executable extensions for bare commands while preserving literal-first explicit extensionless paths, preventing an unrelated extensionless file from shadowing an adjacent executable.
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
- Add `jig info --commands` for repository-specific command availability before and after adoption, stable machine-readable status and reason codes, and actionable setup guidance.
- Add `jig adopt --minimal` to render `.jig.toml` and `.agent/` scaffolding without scripts, workflows, or agent context files; stores `harness_footprint = "minimal"` so `jig update` keeps the thin footprint until a full re-adopt.
- Add `.agent/jig-managed-paths.json` as the strict exact-path authority for managed-file retirement; older adopted repositories establish it with an explicit same-footprint re-adopt before updating or contracting.
- Add `scripts/jig doctor` and `scripts/jig info` / `scripts/jig explain` for repo readiness and configuration snapshots.
- Add `scripts/jig work evidence` and the `jig.work_evidence` MCP tool for fresh/stale gate evidence inspection.
- Add `scripts/jig vault run --file VAR=SECRET` for Unix-only secret-file delivery and human-readable vault run summaries by default (`--json` for the full buffered payload).
- Add Jig local development proxy commands for stable repo-scoped dev hostnames, HTTP/HTTPS forwarding, WebSocket support, workspace app discovery, local certificates, and service file generation.
- Add `scripts/jig dev` and `scripts/jig proxy {start,stop,list,prune,run,alias}` runtime flows for supervised app processes, aliases, and route listing/pruning.
- Add a private, canonical-repo-scoped dev-session registry plus `scripts/jig dev status` and idempotent `scripts/jig dev stop`, with sanitized human/JSON diagnostics, authenticated supervisor-owned cleanup, and retained fail-closed evidence when cleanup cannot be confirmed.
- Add `scripts/jig proxy cert {generate,status,trust,untrust}` and `scripts/jig proxy service {install,status,uninstall}` for certificate trust management and user service installation; trust-store mutations require `--accept-trust-scope`, and `proxy service install` requires `--accept-service-scope`.
- Enable the `dev-proxy` Cargo feature by default while preserving `--no-default-features` builds for contract/MCP-only consumers.

### Changed
- Breaking: Treat whitespace-only `rust_migration_dir` values as invalid during contract checks and report affected migration workflows as needing setup with direct re-adoption guidance instead of claiming they are ready.
- Label the proxy readiness check as `Dev proxy` in human and JSON doctor output so it is distinct from generic network proxies; its stable JSON `id` remains `proxy`.
- Breaking: Make global `--json` cover CLI usage errors and pre-output command failures with a stable error envelope on stdout while retaining nonzero exit statuses; scripts that consumed failures from stderr must read the JSON envelope instead. `prompt get --json` now returns the standard command envelope instead of the bare rendered body. Commands that already emitted JSON do not append a second document, and MCP continues to reserve stdout for protocol framing.
- Breaking: Require exactly one nonblank `--body` or `--body-file` when appending structured-work progress; body-less `work append` invocations must provide progress text.
- Validate structured-work start body sources before durable session mutation, label aggregate snapshot completeness as collection state, and make `state summary` report persisted record/event counts instead of duplicating `work status`. Reorder root help around setup, development, structured work, project data, local services, and agent automation, with a compact common-workflows footer.
- Make scaffolded init validate its destination before wizard or vault interaction and again before mutation, reject a symlink destination plus portable exact, case-folded, file/descendant, and Windows-alias output collisions even with `--force`, publish ordinary files atomically within verified parents, and roll back only empty directories atomically created by the failed init. Existing Windows drive/share roots remain accepted when a denied create can be verified as a real directory. Direct `--frontend-app` metadata now fails during CLI parsing, and init/adopt resolve relative answer files from the launcher invocation directory.
- Make required-tool doctor analysis check recognized external `env`, `nohup`, and `time` wrapper chains plus their terminal target in execution order and under each stage's effective lookup context; preserve unsupported or dynamic wrapper ambiguity instead of falsely passing it, model ordered environment assignment/scrubbing, command-local literal PATH, inherited Bash startup/function state, hidden substitutions, expansion-capable heredocs, and prior dispatch-mutating builtins without exposing configured commands or credentials. External `env` assignments use its nonempty-name grammar, and PostgreSQL capability output must match the complete synthetic invalid-`sslmode` diagnostic on one line.
- Make foreground development startup locks and route publication interruptible, apply explicit app selection before selected-directory resolution, and harden Windows PATH, Job Object, graceful-fallback, and portable Unix termination behavior. Windows app launch keeps canonical containment checks while converting supported drive/UNC paths for child use, tries literal explicit extensionless commands before `PATHEXT`, preserves quoted/non-Unicode PATH entries, and rejects unsafe device namespaces.
- Keep bare `scripts/jig dev` as the launch form and add `--replace` for safe same-repository conflict replacement; cross-repository, unregistered, ad-hoc, and concurrently observed ownership is never terminated or silently taken over.
- Align unattended harness-only/tooling-only examples and fixtures, prove harness-only rendering before fixture stubs are added, exercise the generated SQLx command through a controlled executable, and align empty frontend output declarations, historical admin role inference, and dev-app kind error attribution with the actual generated contract.
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
- `scripts/jig doctor` now includes the detail for required failing checks and safely verifies that a trusted direct SQLx CLI includes the SQLite or PostgreSQL driver configured by `DATABASE_URL` without exposing credentials or contacting the configured database. Cargo-dispatched SQLx commands remain visible but unverified because aliases, included configuration, home overrides, and wrappers can change the executable Cargo actually runs; fresh scaffolds therefore default to direct `CARGO=cargo sqlx prepare`, which also supplies the dispatcher input required by SQLx CLI 0.9.
- Preset init now resolves `--answers-file` before the wizard and scaffold plan, with one explicit precedence (`CLI > answers file > preset defaults > renderer defaults`) so repository names, branches, package managers, commands, frontends, Git HEAD, and generated workflows cannot contradict one another.
- Generated database bootstrap accepts an exported `DATABASE_URL` without requiring a physical `.env`, orders database setup before bootstrap in next steps, and bounds default PostgreSQL database identifiers to 63 bytes with a stable suffix hash.
- Generated frontend dependency handling now resolves real workspace membership, records a versioned scope-specific readiness fingerprint, rejects unrelated or stale install artifacts in `scripts/jig dev`, and safely waits for verified live installers without recommending deletion of their locks.
- Fresh Yarn 4 scaffolds use the `node-modules` linker, admin Vite apps honor injected `PORT`, and generated E2E workflows safely serialize branch names containing YAML-significant characters.
- Preserve the advertised Rust 1.85 minimum across all targets and features by avoiding let-chains and disabling unused dependency features that raised transitive MSRV requirements.
- Generated Rust workspaces now declare and inherit `rust-version = "1.85"` and use Cargo resolver 3, so lockfiles created by newer Cargo versions prefer transitive releases compatible with the scaffold's advertised minimum.
- SQLx doctor now honors literal command-local `--database-url` and `DATABASE_URL` values before environment/dotenv defaults, redacts inline URLs from reports, and reports ambiguous or heuristic probe results as present but unverified while retaining a blocking known-missing-driver result.
- SQLx doctor now separates executable presence from probe trust: only bare allowlisted tools resolved through canonical non-repository PATH entries are executed, under a scrubbed isolated environment. Explicit, repo-local, relative, symlink-mediated, or shell-ambiguous tools remain present but unverified; PATHEXT lookup, heredocs, guarded cwd changes, and later URL mutations are modeled conservatively.
- Init automation now distinguishes `--defaults` (Rust React, no database, `web`) from strict `--no-input`/non-terminal execution, adds explicit `--preset harness-only`, and rejects minimal-harness/scaffold contradictions before vault or destination writes.
- Answers files with `harness_footprint = "minimal"` now resolve as a complete harness-only init shape in interactive, defaulted, strict, and non-terminal modes, while explicit Rust/database/frontend combinations still fail before writes. Legacy frontend entries recover kind/role from matching dev metadata and historical admin names, then persist both fields.
- Doctor now recognizes and redacts Cargo-dispatched, direct `sqlx`, and `cargo-sqlx` forms including `-D`, retains shell quote and literal cwd context, probes only trusted direct executables, and fails open as `present_unverified` for Cargo dispatch, wrappers, redirects, missing URLs, substituted values, or ambiguous expansion without leaking secrets.
- Generated frontend dependency receipts now prove the selected manager-specific inputs and installed artifact, and a dedicated identity-checked install worker prevents wrapper death, stale metadata, or unverifiable owners from enabling overlapping installs. A verified coordinator handoff no longer expires after an arbitrary scheduler delay; only unresolved identity has a bounded retry window. Fresh pnpm workspaces explicitly disable the global virtual store so local and CI validation share one repository-local layout, and the normalized setting is contract-bound rather than ambient. Only pnpm 10/11's exact-root regular workspace-validation cache files are excluded; symlink, directory, nested-file, executable-shim, package-tree, semantic metadata, and manager-authority changes remain attested. Browser E2E CI uses the same dependency authority.
- Generated frontend dependency scope now follows real manager boundaries: declared root membership wins for npm, pnpm, Bun, and Yarn Classic, while nested Yarn Berry projects remain independent. Unsupported workspace syntax fails closed; receipts cover every member manifest, patch/config input, complete PnP companions, and truly dependency-free installs; install ownership uses timezone-stable process identities.
- pnpm dependency receipts now bind the configured Corepack spec, actual executable/version, shared-lock mode, lockfile stability, and the pnpm 10/11-specific active patch source. The checker supports pnpm's JSON, JSON5, and YAML manifests; disables hooks during metadata queries with the version-compatible npm/pnpm configuration namespaces; rejects inherited overrides, unsafe workspace metadata, traversal, runtime drift, and scope-local YAML patches that standalone `--ignore-workspace` would silently ignore; and retains selected members named like generated output directories. Generated CI resolves the scope-aware pnpm spec and watches authoritative manifests and patch files. Standalone installs continue to ignore inactive parent patch settings. Forced proxy cleanup also attempts an immediately available route lock before honoring cancellation, so dead routes are not left behind unnecessarily.
- Generated package-manager metadata probes resolve native Windows executables and invoke Corepack/npm `.cmd` or `.bat` shims through an explicit, fixed-argument `cmd.exe` boundary, covering pnpm alternate manifests plus Yarn Classic/Berry configuration without the deprecated `shell: true` argument path.
- Rust React init now reserves the case-insensitive `api` dev identity for its backend before prompting or writing files. Generated admin source is clean under its pinned Prettier configuration, the standard lint gate enforces that check, and derived `dist/` output stays excluded from formatting.
- Generated web and E2E workflows provision a fallback Node before invoking the Node-backed scope checker, synthesize a pinned version file only for the checker's explicit no-file status, preserve all other resolver failures and the configured GitHub runner, and invalidate on authoritative patches/PnP artifacts. SPA/admin Vite configs ignore blank origin overrides, Playwright treats blank numeric variables as omitted, and admin theme state is applied before hydration.
- Yarn readiness now supports the bounded generated PnP state formats used by Yarn 2, 3, and 4, defaults the historically omitted ESM-loader setting correctly, and proves only referenced Berry archives or Classic external-cache packages while keeping local workspace source outside dependency readiness.
- Generated real-backend E2E suites scope duplicate `Ready` text to the accessible Rust API card, clear SQLite rollback/WAL/SHM sidecars, and receive a 30-minute CI job budget around Playwright's computed server/test budget. Git Bash dependency ownership uses validated Cygwin/MSYS procfs identity and process-group data instead of unsupported BSD `ps -o` options.
- Generated workflows quote user-controlled branch, runner, app-name, directory, and path scalars; direct Vite launches use strict ports; and adoption gives recognized Vite/Astro scripts precedence over incidental dependencies.
- Foreground development cleanup now distinguishes SIGINT, SIGHUP, and SIGTERM end to end, reports statuses 130/129/143, closes child-exit races, and normalizes ordinary signal-terminated children. Concurrent generated database creation safely reuses the winner's database.
- Unix foreground signal handlers are now installed transactionally for one session and restore the prior dispositions afterward. The first termination reason is sticky; any later termination signal accelerates owned process-tree cleanup without orphaning children or replacing the selected outcome.
- `proxy run` now shares the structured signal/exit contract with `dev`; repeated matching termination accelerates bounded cleanup during blocked startup, and route-cleanup errors no longer discard a failed child's output tail. Database bootstrap requires an exported URL or a real `.env` assignment rather than accepting any `.env` file.
- Scaffold reports now separate frontend execution `kind` from semantic `role`, and rendered shadcn dependency/docs/components metadata comes from the same provenance constants reported by init.
- `scripts/jig work gates` now defaults to the single open work plan, matching `scripts/jig work evidence`; pass `--plan-id` when multiple plans are open or to inspect a closed plan.
- `scripts/jig doctor` now reports missing Codex Jig skills as optional setup instead of blocking overall repo readiness.
- Behavior change: repo-local `scripts/jig` launchers now run the Jig binary from the owning repository root even when invoked by absolute path from another current directory. This makes check/dev/proxy/agent commands consistently operate on the owning repo; `jig init`, `jig adopt`, and `jig update` still resolve relative destination/template paths against the caller's original directory.
- Require `--accept-trust-scope` before installing the Jig Dev Proxy local CA through the platform trust tooling.
- Vite proxy host support relies on Vite's `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS` compatibility hook; configure Vite `server.allowedHosts` explicitly if a Vite release changes that hook.
- Windows builds parse and run non-certificate proxy flows, but automatic HTTPS certificate generation/trust remains unsupported until owner-only ACL hardening for private key files is implemented.
- Document `JIG_PROXY_STATE_DIR`, proxy CA trust scope, and local dev proxy usage more explicitly.
- Generated `--preset rust-react` bootstrap commands now install generated frontend app dependencies before `scripts/jig dev` runs them.
- Generated `--preset rust-react` frontend dev scripts remain launch-only; run `scripts/jig bootstrap` to install or refresh the dependency fingerprint before `scripts/jig dev`.
- Generated development apps now launch portably on native Windows: the Rust API uses direct Cargo argv and consumes Jig's injected `HOST`/`PORT`, Astro reads the same values with strict-port behavior and stays foreground under Jig even in agent environments, and npm/Corepack `.cmd` or `.bat` shims run through a validated `ComSpec` boundary while native executables remain direct. Jig assigns each suspended app to a kill-on-close Job Object before resuming it, so descendants remain owned after a batch wrapper exits.
- Frontend dependency preflight now follows the actual selected configured/discovered launch plan, including package-manager `run dev` apps such as Astro, and supervises the readiness checker with a bounded owned process tree. Typed cancellation preserves cleanup and checker failures, inherited Bash startup/function overrides cannot spoof readiness, and bounded pipe capture cannot leave detached readers. Historical four-part `admin` / `admin-panel` frontend entries retain their Admin role, empty migration answers resolve consistently across noninteractive policies, and recopy migrates the former generated SQLx command default.
- Generated SQLite bootstrap creates nested database directories, distinguishes semantic in-memory URLs, canonicalizes filesystem aliases for migration locking, and serializes full migrations across processes, including Windows' platform-specific file-lock contention result. Yarn receipts and workflows now cover the closest in-repository package-manager declaration plus every ancestor config/runtime input; external or dynamic Yarn runtime/plugin authorities fail before Yarn executes, an interrupted worker generation retains its lock while descendants survive, install-lock waiters use cheap ownership polling, and browser E2E CI caches its pinned Playwright browser from root and app dependency authorities.
- Generated SPA TypeScript configuration separates browser and Node globals, rejects identical API/web E2E ports, safely passes matrix directories through the shell environment, and serializes workspace paths as JSON/YAML strings.
- Foreground cleanup now lets any later termination signal accelerate cleanup without replacing the first reason, preserves Unix process-group identity until descendants are stopped, and uses deterministic lifecycle barriers in its real-signal regressions.
- Required-tool doctor diagnostics now treat command-local or persistent `PATH` changes as unverified executable identity, emit nullable presence instead of resolving the wrong ambient tool, and skip SQLx capability probes at that boundary. Windows probe children use a separate console process group so Ctrl-C remains a structured cancellation.
- Generated workflow jobs select Bash explicitly on every configured runner. Real-backend Playwright startup overrides `HOST`, `PORT`, and `BIND_ADDR` together, and private-cache in-memory SQLite pools serialize checkouts onto their one schema.
- Foreground signal registration is process-one-shot, with generation-scoped callbacks and fail-closed retirement; another foreground dev/proxy command requires a new Jig process. Doctor probing instead serializes each complete external-check batch and permits later same-process batches after clean handler retirement; an unsafe retirement permanently disables later batches in that host process. Linux process-group cleanup skips unreadable `/proc` entries only when `getpgid` proves they are unrelated or gone, and any uncertain or empty SIGTERM-phase scan escalates to SIGKILL before post-kill absence is accepted.
- Append-only session state now reads legacy recursively embedded start summaries through an iterative metadata-only compatibility path, while new recent-session references are shallow and cannot grow JSON depth with every `work start`. Committed-checkout fixtures create a bare remote through init/push instead of the intermittently failing local no-hardlinks clone path.

### Security
- Supervise brokered vault commands with a retained process-tree identity and one wall-clock deadline: Linux/macOS leaders remain unreaped until their isolated group is signaled, Windows children enter a kill-on-close Job Object before resume, unsupported targets fail before execution, and nonblocking capped output drains cannot hang on inherited pipes or signal a recycled PID/PGID. Non-consuming Unix observation distinguishes exit, signal, stop, trap, and continue records; macOS accepts group `EPERM` only after an atomic snapshot proves the exact exited leader is the sole remaining member.
- Apply the same retained-identity proof to macOS development-app cleanup: stopped, trapped, and continued leaders remain running, and group `EPERM` is accepted only after an atomic snapshot proves the exact exited-unreaped leader is the sole member. Additional members remain pending through bounded confirmation so transient zombies can drain while live unsignalable members still fail closed. Lifecycle regressions no longer probe a recyclable numeric process group after reaping its leader, recover their serialization lock after an earlier panic, and release test helpers before reporting cleanup failures.
- `vault run --file` writes each secret to a private Unix `0600` temp file under a `0700` temp directory, wipes brokered temp files before normal cleanup, and removes the temp directory when the brokered process exits; this promotes `tempfile` to a runtime dependency of `jig-vault`.
- Harden proxy stop, certificate writes, CA regeneration, and TLS handshake behavior for local development sessions.
- Harden Vite argument handling, including rejection of mismatched explicit Vite port flags, backend response parsing, WebSocket proxy-header scrubbing, and route-cache invalidation.
- Harden LAN proxy exposure, alias registration, workspace discovery traversal, process-route liveness checks, and route persistence.
- Harden state directory permissions, service-file quoting, and local proxy shutdown behavior.
- Harden background proxy startup, runtime file replacement, request-host validation, installer locking, private-key reads, and workspace config file reads.
- Reverify process-route listener ownership while holding the route lock, restore the previous route file after failed route publication, isolate service temp paths, harden certificate/trust temporary reads, prefer recorded template commits for remote runtime installs, and defer release tag pushes until all crates publish successfully.
- Treat template source metadata as a runtime-install trust boundary: recorded hex `_commit` values pin the exact remote Jig revision used by `scripts/install-jig.sh`, and contract checks now keep the installer script and template mirror in sync.
- Bound the Jig Dev Proxy local CA lifetime to two years, avoid broad bare-TLD CA constraints for non-`.localhost` TLDs, and verify macOS trust installation before recording Jig's trusted-CA marker.
- Treat only `ESRCH` as proof that an owned process or process group is gone; `EPERM` and unexpected probe failures remain live/unverified so cleanup and dependency-lock recovery fail closed instead of signaling or stealing a recycled identity.
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
