# `.jig.toml` Configuration

This file is the supported configuration surface for downstream repos and must be committed alongside the generated template output.

> Jig supports Linux and macOS hosts. See [Platform Support](platform-support.md).

`.jig.toml` is also the native renderer answers file.

After changing values in `.jig.toml`, re-render with:

```sh
jig update --recopy
```

To move onto a newer version of the template while keeping the stored answers, run:

```sh
jig update
```

For remote template sources, plain `jig update` advances to the remote default branch unless `--vcs-ref` is provided. `jig update --recopy` re-renders from the stored `_commit`. Clone operations preserve a narrow set of caller-owned Git transport and authentication settings, including explicit SSH, CA, SSL, and proxy controls; repository, object, index, work-tree, command-config, replacement, namespace, quarantine, and trace redirections are scrubbed. Git operations against the prepared checkout use the stricter repository-isolated environment.

The file contains both public settings and the private `_src_path` / `_commit` fields that `jig update` uses to resolve future renders. Repos rendered from local committed template checkouts may also store `_template_mode` and `_template_local_path`. A relative filesystem `_src_path` is resolved from the generated repository root, regardless of the caller's working directory. For ordinary direct `jig` invocations, `JIG_REPO_ROOT` remains an explicit repository override. A generated `scripts/jig` invocation instead passes its own canonical root through the validated launcher handoff; that root is authoritative over an inherited `JIG_REPO_ROOT` for the entire process, including worker threads. Remote runtime installation uses the immutable hexadecimal `_commit`; a missing or invalid revision fails closed instead of silently following the remote default branch. `JIG_INSTALL_ALLOW_UNPINNED_REMOTE=1` is the explicit, warned escape hatch for recovering a repo whose remote source metadata predates that pin. The separately acknowledged embedded-source fallback already warns about and authorizes its default-branch install. A compatible cache from either deliberately mutable source can be replaced with `JIG_INSTALL_REFRESH=1 scripts/jig <command>` or `scripts/install-jig.sh --refresh --profile runtime`; refresh-only resolution fails instead of returning the old cache. Contract-v4-and-later launchers no longer select older product-version-keyed directories such as `.git/jig-tools/0.2.0-runtime`; `doctor` reports them as an optional cleanup advisory, and they may be removed manually after every pre-v4 launcher has been migrated.

`jig update` refuses to overwrite or remove changed template-managed files unless `--force` is passed.

`.agent/jig-managed-paths.json` is the deletion-authority boundary for adopt and full update. It contains a strict, sorted list of the current active template-owned paths and lists itself. A missing manifest authorizes no cleanup and blocks full `jig update`; run `jig adopt . --write` with the current footprint to establish it. A full update adopts the selected template's contract epoch and renders the manifest and launcher for that epoch together. The narrow `jig update <repo> --launcher-only --force` migration may repair only `scripts/jig` and `scripts/install-jig.sh` without a manifest when both existing files match recognizable generated Jig signatures; it never creates ownership metadata or retires paths. The repair requires a non-empty `_src_path`, preserves the repository's supported contract epoch in the launcher, and atomically seeds the currently running, compatibility-proven Jig binary into the managed runtime cache; if seeding fails, both published scripts are restored. Repair always renders the two scripts from the running binary's embedded templates. When `.jig.toml` records another template source, the command warns that source-specific launcher customizations will be replaced until the next full update. This seed is a migration bridge: legacy `_commit` or product-tag metadata may point to a runtime that predates compatibility probes, so `doctor` reports the optional full migration to the current contract while the proven seed is available. A full-to-minimal transition from an older manifest-less harness must establish full ownership first. Invalid, traversal-bearing, duplicate, unsorted, non-regular, or symlinked manifests fail closed even under `--force`.

Launcher-only repair resolves Bash, Python 3, and its standard POSIX helper
commands only from root-owned, non-writable PATH entries; root-owned sticky
ancestors such as the Nix store are accepted. This prevents a writable ambient
PATH from substituting the recovery interpreter or copy/hash tools. Ensure the
required tools are installed in a system-managed location; a seed failure
prints the restricted PATH that was searched.

Root `AGENTS.md` is block-managed instead of file-managed. If the file already exists, `jig adopt` and `jig update` preserve user-authored content and insert or replace only the section between `<!-- BEGIN JIG MANAGED BLOCK -->` and `<!-- END JIG MANAGED BLOCK -->`. Edits inside that managed block are template-owned and may be replaced without `--force`; keep repo-specific guidance outside the markers.

Release builds of `jig init` and `jig adopt` default to the official `jig-sh` template source at `https://github.com/bpcakes/jig-sh.git`, pinned internally to that release's template tag. Pass `--template` when using a local checkout, fork, or private template. Unreleased or dirty local builds use templates embedded in the binary when `--template` is omitted, or an explicit `--vcs-ref` when you intentionally want remote template code. Embedded renders store `_src_path = "embedded:jig-sh"`; generated launchers reuse managed cached binaries that prove contract/profile compatibility. Set `JIG_INSTALL_ALLOW_PATH_BINARY=1` to explicitly allow reuse from `PATH`; the chosen absolute path is reported on stderr. If none exists, `JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1` explicitly permits a non-reproducible install from the current default branch of `template_source_url` or the official source, followed by the same compatibility probe.

For scaffolded init, Jig validates the complete planned managed/scaffold output set before mutating the destination. Existing or broken symlinks in an output leaf or ancestor are always rejected; `--force` authorizes replacement of ordinary files only. Every output component must be valid Unicode; ASCII-case aliases and file/descendant collisions are rejected during init, adopt, and update. Output-collision validation is sorted and bounded rather than pairwise, so large explicit templates retain those guarantees without quadratic preflight work. Files are staged beside their verified destination and published atomically, and the post-scaffold agent-map refresh uses the same boundary.

Init destinations may be absolute, a normal relative path, or exactly `.`; parent-relative components are rejected before wizard/vault interaction and again at the library mutation boundary. Jig resolves the deepest existing destination ancestor before work begins. A wholly missing destination is rendered and Git-initialized in a private same-filesystem tree, then its top missing component is published with one atomic no-replace rename. An existing destination retains its filesystem identity for the transaction; overwritten entries are quarantined before replacement, and committed file identities and content signatures—not later path observations—define Jig-owned output.

Transactional `jig init` publication is supported on Linux and macOS. Some publication primitives remain implemented for unsupported targets, but they are not part of Jig's compatibility contract. Targets without the required primitives reject init before prompts, vault interaction, template resolution, or destination mutation instead of rendering work that cannot be published safely.

If a later template, scaffold, agent-map, or Git step fails in an existing destination, rollback first quarantines the current entry, restores a retained preimage only when the quarantined entry is the exact Jig generation, and publishes every restoration without replacing a concurrent path. Concurrent or foreign content and any contended recovery artifacts are preserved, and an incomplete rollback is reported with the original failure. Git metadata is likewise built privately, initialized and validated with explicit work-tree/metadata paths under a repository-redirection-scrubbed environment, and only then published without replacement.

Existing-destination init preflights its retained file-descriptor budget before it acquires snapshots. Each planned leaf is charged for a possible preimage plus its first Jig generation, repeated publications such as the post-scaffold `agent-map.md` refresh are counted explicitly, and directory identities, per-parent write staging, and transient cleanup headroom are added separately. The generation cap includes repeats. This conservative model permits the default existing-empty scaffold under macOS's ordinary soft limit of 256 while continuing to assume a currently missing path can appear concurrently before its snapshot.

When Git has an explicit `GIT_TEMPLATE_DIR` or configured `init.templateDir`, init mirrors that template into private staging before invoking Git. Only identity-stable regular files and real directories are accepted; symlinks, special files, linked-worktree markers, and object-store redirections are rejected before Git can follow or mutate them.

For local git template checkouts, `jig init` / `jig adopt` use a committed source:

- `--template-mode committed`: explicitly use the clean local `HEAD`
- omit `--template-mode`: use the same committed local-template behavior

## Required Keys

- `repo_name`: display name used in generated docs. During adoption, repo names inferred from Git remotes preserve dots such as `my.app`, while directory-name fallbacks keep the existing dash-sanitized form.
- `default_branch`: branch name used for base-ref comparisons
- `ci_github_runner`: `runs-on` value for GitHub Actions jobs. PostgreSQL browser
  E2E and Go PostgreSQL integration are the exceptions: their generated jobs use
  `ubuntu-latest` because service containers and the Docker daemon require Linux.
  Other generated jobs retain this configured runner and select Bash explicitly
  for repository-owned `run` steps.
- `work.gates`: required work evidence gates evaluated before `scripts/jig work finish`
- `agent_tooling`: agent-client tooling expected for this repository, including Jig Codex skills
- `template_source_url`: optional canonical template source URL for portable recopy/update
- `sqlx_enabled`: whether to generate SQLx and migration-specific contract pieces
- `rust_crate_roots`: repository-relative directories whose direct child directories are considered crates, including in contract-v6 component repositories

When `sqlx_enabled` is `true`, these additional keys are required:

- `rust_migration_dir`: SQL migration directory
- `rust_migration_layout`: closed migration representation. Use `flat_migrations` for ordinary timestamped SQLx migration files or `versioned_artifacts` for complete versioned schema trees. Older configs that omit the key default to `flat_migrations`.
- `rust_sqlx_metadata_dir`: committed SQLx metadata directory

For contracts through version 5, `backend_language = "go"` with `go_database = "postgres"` requires `migration_dir`, and Go backend identity cannot be combined with `sqlx_enabled = true`. Contract 6 does not persist that singular backend identity: components carry composable adapters, so one authored repository may contain Go and Rust/SQLx components. Recopy derives its compatibility-only singular fields from the complete component model rather than letting stale legacy fields reject that valid mixed model. A complete authored model and its string-valued `[commands]` map remain authoritative when loaded through answers-file init or footprint-changing re-adoption. Generated models are recognized against their stored compatibility projection so a footprint change can add or retire generated capabilities, while a structurally customized or mixed-backend model is retained. Explicit answers files with a malformed command map fail closed; automatic re-adoption can still repair malformed generated configuration.

Generated Go repositories use the root `go.mod` as their Go toolchain authority. For contract-v6 repositories, doctor and managed Go CI start at each `go` component root and use the nearest ancestor `go.mod` within the repository, deduplicating components that share a module. Every existing component-root directory segment must be a real directory rather than a symlink, so module discovery cannot escape repository authority. Doctor reads each module's required `go` directive, honors a newer optional `toolchain` directive, and requires the active Go runtime to satisfy the highest discovered version. CI asks Jig for the selector through the same bounded parser before `setup-go`, while its cache watches root and nested module, workspace, and vendor authority. Jig does not generate a second checked-in `.go-version` authority.

## Optional Keys

- `backend_language`: legacy contract-v5 application backend identity; accepted values are `rust` and `go`. Contract-v6 renders omit it and derive capability from component adapters.
- `go_database`: legacy contract-v5 Go database identity. Contract-v6 renders use the composable `go-postgres` adapter instead.
- `migration_dir`: backend-neutral, repository-wide migration policy directory. It takes precedence over the legacy Rust-specific `rust_migration_dir`; generated Go/PostgreSQL repositories use `internal/database/migrations`. A contract-v6 repository that declares native migration authoring must have exactly one native `migration-add` action. Its owning component must carry exactly one format adapter: `sqlx` or `go-postgres`.
- `schema_dump_enabled`: when `true` and `sqlx_enabled` is also `true`, the template renders schema dump and schema freshness commands; when SQLx is disabled, this is rendered as `false`. New init/adopt answers reject explicitly setting this to `true` while SQLx is disabled; `jig update --recopy` normalizes legacy SQLx-disabled configs back to `false`.
- `schema_dump_command`: command behind `scripts/jig sqlx schema dump` when `sqlx_enabled` and `schema_dump_enabled` are both `true`
- `sqlx_check_command`: command behind `scripts/jig check sqlx` when `sqlx_enabled` is `true`
- `bootstrap_command`: implementation behind `scripts/jig bootstrap`; `scripts/jig setup` invokes it between an initial doctor pass and minimum agent/contract verification. The generated harness-only default runs `cargo fetch` only when a root `Cargo.toml` exists, otherwise exits 0 with a stdout note. For database-backed application shapes, export a nonempty `DATABASE_URL` or provide an actual `DATABASE_URL` assignment in `.env`; an empty or unrelated `.env` does not satisfy the preflight. Rust bootstrap creates the Postgres or SQLite database and applies SQLx migrations. Go bootstrap tidies modules, installs frontend dependencies, validates database configuration before PostgreSQL code generation, regenerates sqlc output, creates the database, and applies embedded Goose migrations. `test:postgres` independently proves the embedded migration and generated query against a disposable database. Both perform one dependency install from the authoritative JavaScript scope. Generated SQLite code creates missing parent directories and serializes migrations with an adjacent lock file so concurrent bootstrap processes do not race non-idempotent DDL. Every in-memory SQLite pool permanently retains at least one connection with idle/lifetime reaping and the cancellable pre-acquire health check disabled; private-cache URLs additionally restrict the pool to one connection, while shared-cache forms retain concurrent pooling. Set this explicitly for other project-specific setup. If a root `Cargo.toml` exists, Cargo errors are surfaced instead of skipped.
- `dev_command`: legacy project-owned dev command preserved only for older renders; `scripts/jig dev` uses `[dev]` and `[[dev.apps]]`
- `rust_fmt_check_command`: implementation behind `scripts/jig check fmt`; the generated default exits 0 with a stdout note when no root `Cargo.toml` exists, and otherwise surfaces Cargo errors
- `rust_clippy_command`: implementation behind `scripts/jig check clippy`; the generated default exits 0 with a stdout note when no root `Cargo.toml` exists, and otherwise surfaces Cargo errors
- `rust_test_command`: implementation behind `scripts/jig check test`; the generated default exits 0 with a stdout note when no root `Cargo.toml` exists, and otherwise surfaces Cargo errors
- `rust_test_locked_command`: implementation behind `scripts/jig check test-locked`; the generated default exits 0 with a stdout note when no root `Cargo.toml` exists, and otherwise surfaces Cargo errors
- `[commands].go_fmt_check_command`, `go_lint_command`, `go_test_command`, and `go_test_locked_command`: generated Go implementations behind `check fmt`, `lint`, `test`, and `test-locked`
- `[commands].sqlc_check_command`: PostgreSQL Go implementation behind `scripts/jig check sqlc`
- `web_package_manager`: currently `bun`
- `frontend_apps`: list of app definitions. A frontend app may use `dir = "."` when the app lives at the repository root.
- `dev`: Jig-native local development proxy settings and app definitions
- `status`: read-only software-rewrite status providers executed by `scripts/jig status`
- `execution`: supervision limits for long-running configured commands and workers

The generated no-root-`Cargo.toml` Cargo defaults print a stable stdout prefix that `work check` recognizes as an intentional harness skip. Reworded custom commands still run normally, but they will be summarized as ordinary command output instead of `passed (all skipped)`. Custom commands should not print the exact generated prefix unless they intentionally want to opt into that skip rendering.

Configured command values are committed repo configuration and run through non-login `bash -c` from the repo root with the user's normal process environment. They run in supervised process trees, use `[execution].command_timeout_seconds` (default 1,800; valid range 1–86,400), and retain at most `[execution].command_output_limit_bytes` from each stdout/stderr stream (default 67,108,864; valid range 1–1,073,741,824). Exceeding the capture limit terminates and reaps the process tree as an explicit failure; it is never reported as partial success, and the bounded prefix captured before termination remains in the receipt for diagnosis. Internal Git and GitHub protocol commands keep a separate fixed 4 MiB bound. Codex review, refinement, and PR-repair workers use a separately bounded last-message file as their authoritative result channel; their diagnostic transcripts may truncate at 4 MiB while receipt evidence reports that truncation. Human-mode CLI progress is buffered within 64 KiB and delivered with a bounded best effort after supervision; JSON mode disables progress, while MCP defers progress writes until execution returns and retains at most a 4 KiB preview per stream. Contract 6 writes configured commands under `[commands]` with component-scoped keys such as `api_test_command` and `web_test_command`; action runners refer to those keys, never to agent-supplied shell text. Treat changes to these values like changes to project-owned shell scripts. An action runner's optional `environment` map is the same checked-in execution authority: it intentionally inherits the caller environment and may override sensitive names such as `PATH`, loader controls, or Git variables, just as the reviewed shell command itself can. Jig-owned Bash probes are narrower: frontend dependency readiness and launcher-backed doctor proxy diagnostics remove inherited Bash startup files, directory lookup, shell-option/trace controls, and exported functions before execution so those controls cannot spoof or corrupt structured results. Ordinary configured checks and development commands retain the user's environment. Jig-owned checks such as `scripts/jig check contract`, flat-layout `scripts/jig migration add NAME`, `scripts/jig check schema`, and repo policy checks run natively inside the binary.

## Contract-v6 Repository Model

`[repository]` is the reviewed source of workspace identity. Its generated records are repeated as `components`, `actions`, `profiles`, and `default_check_profile` in `.agent/jig-contract.json`; runtime loading rejects a mismatch.

- `[[repository.components]]` declares `id`, a literal repository-relative `root` (`.` is allowed), optional description/tags/dependencies, affected propagation, adapter ids, guidance, and per-field provenance. A non-root component may not live under `.agent/`, whose harness and runtime contents are deliberately excluded from source identity. Component dependencies must be acyclic; action dependencies form a separate acyclic execution graph.
- `repository.affected_ignore` is a reviewed list of repository-relative globs whose changes do not select executable targets during affected planning. Generated full repositories ignore `.env`, `.env.*`, their nested forms, named guidance files such as `README.md` and `AGENTS.md`, `docs/**`, license files, and `.github/**`; remove or narrow those defaults when a check consumes one of those paths. Patterns may never match `.jig.toml` or `scripts/jig`, and an explicit action input always takes precedence over an ignore. Every remaining unignored, unclaimed path continues to fail closed: generated defaults deliberately do not ignore arbitrary Markdown fixtures, `.gitignore`, `Makefile`, or `justfile`, because those files can change program inputs, source discovery, or invoked commands. The ordinary source fingerprint remains conservative and still records observed ignored dotenv paths for plan identity and evidence freshness, so a dotenv edit invalidates prior evidence even when it does not widen a Git-affected plan. Jig prunes a wholly ignored directory instead of searching generated dependency and build trees; unignore the containing path when it holds an intentional dotenv input.
- `[[repository.actions]]` declares a structured `{ component, action }` target, intent, effects, runner, repository-relative forward-slash input globs, target dependencies, optional `timeout_seconds`, result parser, compatibility aliases, and provenance. Action timeouts use the same valid 1–86,400 second range as `[execution].command_timeout_seconds`; omission inherits that repository default, while an action value is the more-specific override. Overrides are accepted for supervised command runners and the cooperatively supervised native schema runner. Other bounded in-process native operations reject an override because Jig cannot safely preempt them midway through a mutation; they check the deadline before entry, and a returned completion is authoritative because effects may already be durable. Inputs may intentionally name paths outside the component root to declare repository-global inputs, but may not be anchored under the unobserved `.agent/` tree. Affected selection unions action inputs at component scope: a matching path retains every selected candidate target on that component rather than pruning sibling actions independently.
- `[[repository.profiles]]` declares a stable id and exact structured targets. `repository.default_check_profile` selects the profile used by bare `jig check`.

Managed language test workflows resolve compatibility aliases such as `jig.fmt_check`, `jig.lint`, `jig.clippy`, and `jig.test_locked` while rendering, then invoke the resolved canonical `component:action` targets. Recopy retains a language workflow for an authored contract-v6 model only when each required alias resolves exactly once to a read-only check on a component owned by the expected adapter, including a valid read-only dependency closure; an adapter alone does not imply those scaffold conventions. Repository-policy jobs that use native adapter policy remain independently composable.

`result_parser = "json_lines"` is a closed machine-output protocol, not a mixed log parser. Every nonblank stdout line must be one strict `Finding` JSON object containing only the documented fields; banner text, progress output, malformed JSON, and unknown fields fail the target even when the process exits zero. Human-readable logs belong on stderr. Parsed `error` findings also fail an exit-zero target, while `notice` and `warning` findings remain successful diagnostics. Use the default `exit_code` parser when stdout is ordinary tool output.

Contract-v6 roots and input patterns are validated while the repository catalog
loads. `scripts/jig check --affected BASE` resolves the selected/default profile
or explicit target candidates, filters them with the Git changes from `BASE`,
and then adds action dependencies. Direct input and configured component
propagation reasons appear in `--explain` and JSON output. A change to the
checked-in `.jig.toml` source selects every candidate because it can change any
target definition. Ignored `.env` and `.env.*` files beneath directories that
are not themselves ignored are execution inputs in the source fingerprint but
have no committed baseline, so their presence is
tracked separately from Git changes during affected planning. Presence alone is
considered only for actions that explicitly declare the dotenv path as an
input; it never becomes an unclaimed change or component-root fallback, so a
stable local dotenv cannot widen every affected run. Explicit inputs take
precedence over `affected_ignore` policy. The generated `.agent/jig-contract.json` is bound separately
through the canonical configuration digest; all `.agent/` harness and runtime
metadata is excluded from affected-path selection. If no declared input or eligible
component root claims a changed path, Jig fails closed by retaining every
candidate with an `unclaimed_input` reason; a comparison with no relevant
changes may still produce a valid empty plan. Paths matching reviewed
`repository.affected_ignore` patterns are removed before component-root ownership resolution, but never when an explicit action input matches the path;
the generated documentation defaults therefore do not expand an otherwise
source-scoped plan. Plans retain at most 100 reasons
per target and expose the complete reason count and digest when that preview is
truncated. Repository
planning and execution require a Git worktree:
the immutable plan identity, affected-path selection, and evidence freshness all
derive from Git state rather than a best-effort filesystem snapshot.

Generated frontend commands use `scripts/check-webapps.sh check-one` so `web:test` validates only the `web` component while preserving dependency setup and coverage enforcement. Fresh Rust/React and Go/React scaffolds also declare repository-wide contract-drift and public-boundary targets because the same transaction creates their `scripts/contracts.mjs` implementation. Adoption of an existing frontend does not infer those scaffold-specific targets merely from app presence; an existing authored v6 repository model remains authoritative on recopy. A declared contract target fails immediately when its runner file is missing instead of producing empty success evidence. Aggregate `jig.typescript_*` tools remain compatibility actions and are not members of the default profile.

Contracts that declare `"kind": "native"` tools require a runtime that supports the repository contract epoch. Use `scripts/jig`; it probes the repository, required tools, and requested build profile before selecting any development, cached, PATH, or newly installed binary.

`jig adopt --json` includes a `detection_report` object that records inferred values before rendering. It contains `summary`, `scope`, `repo_name`, `default_branch`, `rust_crate_roots`, `sqlx_enabled`, `rust_migration_dir`, `rust_migration_dirs`, `rust_sqlx_metadata_dir`, `web_package_manager`, `frontend_apps`, `ci_github_runner`, `signals`, and `warnings`. Adopt previews by default with `render_mode = "preview"`; pass `--write` to apply the rendered managed files with `render_mode = "copy"`. Pass `--minimal` to render only `.jig.toml` and `.agent/` scaffolding (no scripts, workflows, or agent context files); the render stores `harness_footprint = "minimal"` and the JSON report includes `harness_footprint`. Minimal renders retain frontend and dev metadata but omit TypeScript commands, tools, gates, scripts, workflows, and package validation until a full re-adopt. Package-manager lockfiles are reported and applied only when the full frontend harness is enabled. Scan warnings include up to 19 concrete entries plus an omission notice when more were found. `rust_migration_dirs` is informational; only `rust_migration_dir` is applied. When SQLx is detected without migration or metadata directories, adopt warns and synthesizes the default `migrations` and `.sqlx` paths unless overridden.

## Accepted Key Summary

Jig rejects unknown `.jig.toml` keys so stale template answers fail early. The accepted top-level keys are `_src_path`, `_commit`, `_template_mode`, `_template_local_path`, `repo_name`, `default_branch`, `ci_github_runner`, `template_source_url`, `harness_footprint`, `backend_language`, `go_database`, `sqlx_enabled`, `rust_crate_roots`, `rust_migration_dir`, `migration_dir`, `rust_migration_layout`, `rust_sqlx_metadata_dir`, `schema_dump_enabled`, `schema_dump_command`, `schema_docs_dir`, `schema_check_command`, `sqlx_check_command`, `migration_add_command`, `application_contracts_enabled`, `bootstrap_command`, `contract_check_command`, `dev_command`, `rust_fmt_check_command`, `rust_clippy_command`, `rust_test_command`, `rust_test_locked_command`, `web_package_manager`, `frontend_apps`, `frontend_workspace_roots`, `repository`, `commands`, `vault`, `dev`, `work`, `loop`, `status`, `execution`, and `agent_tooling`. `jig_version` remains a legacy accepted input only so contract v2/v3 repositories can preserve their internal config/manifest consistency; v4 and later renders omit and ignore it. `backend_language`, `go_database`, and the language-shaped command fields remain accepted for v5 migration but are omitted from v6 renders. `schema_check_command`, `migration_add_command`, and `contract_check_command` are likewise legacy accepted keys for older rendered repos; new renders use native binary implementations.

Nested accepted keys are:

- `[commands]`: command names made from lowercase ASCII letters, numbers, and underscores; names must start with a letter and end in `_command`
- `[repository]`: `default_check_profile`, `affected_ignore`, `components`, `actions`, `profiles`
- `[[repository.components]]`: `id`, `root`, `description`, `tags`, `depends_on`, `propagate_affected_to_dependents`, `adapters`, `guidance`, `provenance`
- `[[repository.actions]]`: `target`, `description`, `intent`, `effects`, `runner`, `inputs`, `depends_on`, `timeout_seconds`, `result_parser`, `legacy_aliases`, `provenance`
- `[[repository.profiles]]`: `id`, `description`, `targets`, `provenance`
- `[[frontend_apps]]`: `name`, `dir`, `coverage_threshold`, `kind`, `role`
- `[vault]`: `scope`, `scope_id`, `allow_global`
- `[dev]`: `proxy_port`, `https_port`, `https`, `http2`, `lan`, `tld`, `workspace_discovery`, `apps`
- `[[dev.apps]]`: `name`, `dir`, `kind`, `command`, `argv`, `port`, `host`, `proxy`
- `[status]`: `providers`
- `[[status.providers]]`: `id`, `argv`, `timeout_seconds`
- `[execution]`: `command_timeout_seconds`, `command_output_limit_bytes`
- `[work]`: `checks`, `gates`, `refinements`
- `[[work.gates]]`: `id`, `kind`, `tool`, `target`, `profile`, `conclusion`, `skill`, `fail_on`, `severity`, `scope`, `model`, `required`; check gates also accept `paths`, `paths_ignore`, and `reuse`
- `[[work.refinements]]`: `id`, `skill`, `mode`, `model`
- `[loop]`: `lease_ttl_seconds`, `max_attempts`, `backoff_seconds`, `workflows`
- `[[loop.workflows]]`: `id`, `kind`, `enabled`, `lease_ttl_seconds`, `max_attempts`, `backoff_seconds`, `codex_home`, `schedule`, `timezone`, `prompt_file`, `model`, `sandbox`, `checkout`
- `[agent_tooling.codex]`: `marketplaces`
- `[[agent_tooling.codex.marketplaces]]`: `id`, `source`, `plugins`

## `status` Shape

Generated repositories start with no providers:

```toml
[status]
providers = []
```

Add a project-owned inspector as an argv array:

```toml
[[status.providers]]
id = "factorish.hocr2.migration-readiness"
argv = ["ruby", "scripts/verify_migration_readiness.rb", "--status-provider-v1"]
timeout_seconds = 30
```

`id` is required, must be unique, and must exactly match the report's `provider.id`. `argv` must contain an executable, must not contain control characters, and is passed directly without shell parsing. `timeout_seconds` defaults to 30 and must be between 1 and 3,600. Jig accepts at most 32 providers. The provider must follow the read-only [`jig.status-provider/v1` process contract](status-provider.md#process-contract).

`scripts/jig status` executes configured providers from the repository root with at most four provider processes active at once, preserves configured result order, and combines their validated reports with local Git, work/gate, and loop lease/attempt state. Cancellation stops queued providers before they start and cancels active owned trees. Add `--tui` for the interactive Overview, Packages, and Blockers views; `--refresh-seconds` changes its 30-second refresh interval. The command records no receipt, writes no provider cache, and never fetches remotes. Provider stdout and stderr are bounded, and each invocation runs in an owned process tree. Treat every configured argv as trusted repository executable code.

This section is part of the renderer answers round trip: `jig update --recopy` preserves configured provider entries. The status aggregate is described under [Jig runner and aggregate](status-provider.md#jig-runner-and-aggregate).

## `loop` Shape

Git repositories keep the authoritative schedule ledger, initialization marker, and lock below the checkout's worktree-specific Git metadata directory. Codex `workspace-write` protects Git metadata. The protected ledger is the mutation commit point. The checkout-local `.agent/runtime/loop/schedule.json` is a compatibility and diagnostic replica: current runtimes ignore worker changes to that replica, retry its publication on later authoritative writes, and never let a replica publication failure discard or ambiguously fail an already committed protected transition.

PR-manager branch-lease loss is phase-sensitive but always fail-closed around a prepared checkout. Loss after a worker failure retains even an otherwise clean checkout; loss before worker start retains the checkout as `needs_attention` instead of racing a new lease owner by force-removing that shared path.

The default `noop-status` workflow is read-only and does not invoke Codex. Configured `github_pr_status` workflows inspect GitHub without Codex, while `pr_manager` workflows may run an unattended `codex exec` worker to repair an eligible pull request:

```toml
[loop]
lease_ttl_seconds = 900
max_attempts = 3
backoff_seconds = 300

[[loop.workflows]]
id = "pr-manager"
kind = "pr_manager"
codex_home = "work"
```

`codex_home` is optional and valid for `pr_manager` and `codex_task`. It accepts the same conventional home names and explicit path forms as `scripts/jig codex launch`: `work` resolves only as `~/.codex-work`, `codex` and `default` resolve only as `~/.codex`, and explicit relative paths such as `./.codex-automation` resolve from the repository root. Use an explicit path for a home outside those conventional locations. Configured bare names never fall back to ambient `CODEX_HOME`. A repository-relative home is repository-controlled and may load Codex configuration such as MCP servers and model providers from tracked content; use one only in a repository you trust. Jig validates and canonicalizes the configured directory once at the workflow boundary, then sets `CODEX_HOME` explicitly for each `codex exec` worker. A missing or invalid home fails the tick. When `codex_home` is omitted, the worker preserves existing behavior by inheriting ambient `CODEX_HOME`. PR-repair worktree cache names use a digest of the workflow ID rather than the ID as a path component, so otherwise-valid IDs containing path separators cannot redirect worktree creation outside the managed cache.

Use the compiled `codex_task` kind to run a durable prompt on a five-field cron schedule:

```toml
[[loop.workflows]]
id = "nightly-maintenance"
kind = "codex_task"
schedule = "0 2 * * *"
timezone = "Europe/Prague"
prompt_file = ".agent/tasks/nightly-maintenance.md"
codex_home = "work"
sandbox = "workspace-write"
checkout = "worktree"
```

Loop execution fails closed at its mutable boundaries. Every Git-backed entrypoint that can publish schedule state verifies that `.agent/runtime/loop` is ignored before mutation, independent of workflow kind. A manual tick blocked by an occurrence requiring acknowledgement or by a retained worktree still writes its loop-tick receipt and returns a structured `needs_attention` result without starting the worker; overlap with a live occurrence similarly returns structured `waiting` evidence. Repo-mode Codex tasks require the shared checkout to be clean apart from the runtime-owned receipt journal before worker launch, so existing user work is never adopted as task output. An ambiguous repo-mode occurrence carries a durable shared-checkout marker and blocks repo-mode occurrences from every workflow until acknowledgement while leaving isolated workflows available. PR-manager repair checkouts detach at the immutable head object observed in the GitHub snapshot; a concurrent fetch or branch update can make preparation fail, but cannot redirect the worker to another PR head. Before committing, Jig requires Git's index to contain no unresolved merge entries and uses `git diff --cached --check` to reject staged conflict markers and Git-defined whitespace errors. A truncated open-PR list, review-thread connection, or nested review-comment history is an incomplete observation, so `pr_manager` reports a failed action and performs no attempt cleanup or branch mutation until a complete snapshot is available.

Review text is an instruction boundary for the unattended PR-manager worker. Jig resolves each review-comment author's effective repository permission through GitHub's collaborator-permission API and treats only effective `admin` or `write` permission as trusted. Permission lookup failures fail closed. Untrusted unresolved threads remain observable in the GitHub status snapshot but do not make a PR repairable. The worker prompt receives an explicit projection that omits raw GitHub payloads, PR titles, and all untrusted comment bodies; only trusted unresolved comments can be replied to or resolved. The Codex sandbox remains an enforcement boundary, not a substitute for trusted prompt sources.

`schedule` uses minute, hour, day-of-month, month, and day-of-week fields; seconds, years, and expressions with no possible calendar occurrence are rejected. `timezone` is an IANA name and defaults to `UTC`. A nonexistent spring-forward wall time is skipped, while a repeated fall-back wall time runs once. A `codex_task` requires both `schedule` and a repository-relative `prompt_file`. Repository-relative prompt symlinks are supported only while their full resolution remains inside the repository; absolute symlink targets and escapes are rejected. Prompt files are limited to 1 MiB of UTF-8 text. `sandbox` defaults to `read-only` and permits only `read-only` or `workspace-write`; `checkout` defaults to an isolated detached `worktree` and may be explicitly set to `repo`. The generic loop mutation preflight verifies that `.agent/runtime/loop` is ignored by Git; isolated Codex mode additionally verifies its task-worktree root. Repositories adopted before those rules were managed must run `scripts/jig update --recopy`; Jig fails before publishing loop state or task output when the ignore rule is absent. `model` is optional. A scheduled task runs unattended and auto-approved, so treat its loop configuration and prompt file as trusted repository executable input; the configured sandbox limits the worker but does not make unreviewed repository instructions safe to execute.

Manually validate a configured task with `scripts/jig loop tick --workflow nightly-maintenance`. A manual tick publishes a transient durable occurrence after it acquires the workflow execution lease. Jig removes that record after a clean result. A retained checkout remains visible to `loop status` and `jig ui` and blocks another manual or scheduled run for that workflow until cleanup; an ambiguous result requires inspection and the same acknowledgement as scheduled attention. Scheduled `codex_task` workflows deliberately do not support `loop run`, because repeated reconcile ticks would execute the same task more than once; use `loop tick` for one manual execution or `loop dispatch` for its durable schedule. Run `scripts/jig loop dispatch` every minute from cron, systemd, launchd, or CI to execute due occurrences. On the first dispatch, the most recent scheduled instant is due; after that, missed intervals coalesce to the most recent unclaimed occurrence. Before publishing a durable occurrence claim, dispatch validates the lease and attempt caches. Malformed attempt JSON is reset to empty and reported as `attempts_reset` state evidence; malformed lease JSON fails closed because an unreadable file may still represent a live mutual-exclusion claim. Other cache I/O failures also fail before the claim, and loop-state lock acquisition times out after 30 seconds instead of blocking a scheduler indefinitely. A setup failure or cancellation after claiming but before the worker starts abandons the claim and reports a typed retryable pre-execution action, so a later dispatch can retry the same scheduled instant. If checkout cleanup leaves a worktree behind, Jig instead records the unexecuted occurrence as `needs_attention` with its retained path so evidence remains discoverable. In Git repositories, successfully published claims are synced to the authoritative worktree-specific Git metadata ledger before worker side effects begin and mirrored under ignored `.agent/runtime/loop/` for compatibility and inspection. Preserve the repository's Git metadata and `.agent/runtime/loop/` retained worktrees when an external scheduler recreates or deploys the checkout. Non-Git fixtures use the checkout-local ledger as their authority. Schedule authority and retained task worktrees are deliberately separate from `.agent/.cache/loop/` leases and attempts. Corruption observed after workflow work begins remains an explicit state error rather than being silently reset. A retained worktree remains linked to the original Git checkout; copying or moving a checkout does not make those links portable by itself. When the main checkout and linked worktrees move together, run `git worktree repair` in the new location before relying on retained-worktree paths. Current Jig migrates legacy schema-1 and prior durable schema-2 or schema-3 ledgers to schema 4 before dispatch, leaves a schema-4 downgrade marker in the former cache location, and publishes `schedule.initialized` beside each durable ledger. New schema-4 claims always record whether they use the shared checkout. Markerless `running` or `needs_attention` records from earlier schemas conservatively block shared-checkout admission until finalization or acknowledgement, while known isolated records remain workflow-local. The protected initialization marker remains authoritative after cache or replica cleanup; it makes a missing protected ledger fail closed rather than defaulting to empty state. Earlier runtimes reject the schema-4 ledger and marker. Stop older Jig dispatchers during the upgrade and do not downgrade after a schema-4 dispatcher has written the ledger. If protected schedule authority is lost after initialization, dispatch and status reads fail closed; restore the worktree-specific Git metadata ledger from durable storage before dispatching again.

Losing a workflow or occurrence lease terminates its worker, and finalization verifies that the workflow lease is still owned and unexpired. A lost workflow lease after execution begins makes the occurrence `needs_attention`; an expired running claim does the same rather than being retried automatically because its side effects may be ambiguous. Transient workflow-lease and occurrence-claim renewal errors are retried on a shorter bounded interval while at least one normal renewal interval remains for cancellation and terminal recording; definite ownership loss remains immediately terminal. A live durable occurrence retains admission authority after its workflow lease is released and until terminal finalization, so overlapping manual or scheduled work is deferred rather than entering that gap. Outstanding `needs_attention` occurrences are never history-pruned and keep later dispatch results unsuccessful. The claim transaction blocks overlapping workflow or shared-repository scope while an occurrence is live or requires acknowledgement, which bounds ambiguous-history growth and prevents overlapping dispatchers from executing stale work after a newer occurrence is recorded. After inspecting the receipt and any retained worktree, run `scripts/jig loop acknowledge-occurrence --occurrence <reported-id>`. Status and acknowledgement use the same expiry rule: an expired record still stored as `running` can be acknowledged directly, and the acknowledgement reconciles it atomically. Acknowledgement is an explicit terminal transition: it clears the alert but keeps the schedule instant recorded, so the task cannot rerun; if a newer instant became due while attention was unresolved, the next dispatch may then run that coalesced occurrence. One workflow's schedule-evaluation failure is attached to its workflow as `schedule_state_error` and to the top-level `state_errors`, leaving other workflow, lease, attempt, and occurrence status visible. Post-work aggregation failures are recorded in the dispatch receipt as `state_errors` instead of discarding evidence for already completed work. `loop run` continues after workflow-reported failed actions and preserves an unsuccessful aggregate result; configuration, persistence, receipt, and other engine-level errors abort the command. A PR-manager worker cancelled after process start is recorded as `needs_attention` with its worker receipt and retained worktree; malformed worker output or a later failed Git step also retains a dirty checkout or local commit, while a clean unchanged checkout is removed as an ordinary failed attempt. A push that started but cannot be reconciled with the remote additionally preserves its candidate commit. Losing the PR branch lease after a worker or remote mutation starts is also `needs_attention`; the completed action, worker receipt, worktree, and pushed head remain available for reconciliation instead of being flattened into an ordinary retry. Scheduled execution stops at either ambiguous occurrence until acknowledgement instead of treating it as an ordinary retryable failure. Because every `checkout = "repo"` task mutates the same repository root, repo-mode tasks share one repository-local execution lease even when their workflow IDs differ; isolated worktree tasks retain per-workflow leases. A repo-mode task that leaves the shared checkout dirty, whose final checkout state cannot be verified, or whose worker ends ambiguously after starting becomes `needs_attention` and blocks another scheduled occurrence until acknowledgement. If a successful repo-mode task creates a clean commit, the current dispatch stops after that task; the next dispatch reloads workflow settings and prompts from the new repository revision instead of combining old settings with new files. The final checkout check treats `.agent/state/receipts.jsonl` separately from ordinary dirtiness. Jig opens identity-pinned journal snapshots under short exclusive writer windows, then performs prefix hashing and bounded append parsing after releasing the lock. Verification requires the Git index entry, journal identity, and every pre-worker byte to remain unchanged, bounds the snapshotted appended region, and requires the runtime-generated worker receipt to be the only appended record. An additional nested, concurrent, or directly forged receipt makes provenance ambiguous and therefore makes the repo-mode task require attention. Only after that exact append proof does Jig exclude the journal from task-authored dirtiness. A clean successful worktree is removed, while a worktree with uncommitted changes, new commits, or a failed worker is retained and reported by `loop status` and `jig ui`. Retained-worktree occurrence records are excluded from bounded history pruning while their paths exist; inspection errors fail closed as retained. Acknowledging an ambiguous occurrence clears its alert but does not discard a retained worktree; the same claim transaction continues reporting a retryable hard failure until that path is removed. This prevents a scheduled isolated task from claiming another occurrence while any retained worktree for that workflow still exists, bounding retained checkout growth without deleting diagnostic work. After preserving or discarding the result, remove a clean retained checkout with `git worktree remove <reported-path>`; Git refuses a dirty checkout unless you deliberately add `--force`. Once the path is gone, a later dispatch can run the still-due occurrence and a loop state update can prune old history normally. The external scheduler and machine must be running for local execution; Jig does not install or run a resident scheduler.

PR-manager worktrees are removed after branch-lease finalization for unambiguous success and ordinary retryable failure. Jig retains them for post-start cancellation, unconfirmed pushes, branch-lease loss after side effects, and cleanup failure; the action reports `worktree_retained: true` and requires attention. Such side-effectful attention consumes the workflow tick, so Jig does not start another PR from the same snapshot. Passive `exhausted_attempt` attention may still allow another eligible PR to be considered.

`max_attempts` and `backoff_seconds` apply to workflows that maintain per-item attempt records, currently `pr_manager`. A repair attempt records both the GitHub head it observed and the final head it pushed, so a temporarily lagging snapshot matches the same attempt generation instead of resetting its budget. Once worker execution begins, each `codex_task` schedule occurrence is run at most once; a failed occurrence is recorded for inspection and is not retried automatically. Dispatch JSON keeps its two attention sources separate: `needs_attention_count` counts scheduled occurrences with ambiguous side effects that require `acknowledge-occurrence`, while `exhausted_attempt_count` counts per-item retry records that require `clear-attempt`; retry exhaustion does not also create a scheduled-occurrence alert, and either source makes dispatch status `needs_attention`. `clear-attempt` accepts the exact workflow and item keys reported in attempt state even after that workflow is removed or renamed. If a due occurrence cannot acquire execution authority, dispatch reports `status: "deferred"` with an additive `deferred_count`. A held workflow lease causes the new claim to be abandoned; an overlapping live occurrence remains the durable authority and prevents the new claim from being published. For schema-version-1 compatibility, `skipped_count` remains the broad count of due occurrences that were not executed and therefore includes deferrals, retryable pre-execution failures, and already-recorded occurrences; `deferred_count` identifies the authority-contention subset. Deferral and pre-execution failure are observations of that dispatch attempt, not durable occurrence state, so later `loop status` output does not preserve them. `loop tick`, `loop dispatch`, and `loop run` return a nonzero process status when their JSON result reports `ok: false`, including `needs_attention`. `loop status` is diagnostic and continues returning process status zero when it can produce its report, even when the report's `ok` field is false. `loop status --workflow` scopes workflows, leases, attempts, attempt-attention sections, and occurrences to that workflow. Tick/run idleness and attention remain machine-global even when `--workflow` selects one workflow, so attention or an exhausted attempt owned by another workflow keeps those execution commands unsuccessful until its corresponding acknowledgement or `clear-attempt` repair resolves it.

PR-manager reply idempotency is scoped to the identity authenticated in `gh`: Jig accepts its marker only when GitHub reports `viewerDidAuthor` for that review comment. This direct authorship fact works without assuming that the authentication principal is a user or that the comment still has a non-null author. Keep a stable automation identity across retries. Changing identities can produce one reply from the new identity because trusting another author's predictable marker would let a contributor suppress the managed reply. Thread resolution does not depend on viewer identity or comment-history access.

Loop workflow JSON reports the original setting as `codex_home_configured`. PR-manager repair-attempt actions include the canonical directory selected for the worker as `codex_home_resolved`; actions that do not attempt a repair omit the field. Worker receipts written by this version include `codex_home_resolved`; older `schema_version: 1` receipts may omit this additive field, so readers must tolerate its absence. In repair-attempt actions and current worker receipts, a `null` value means the process inherits ambient `CODEX_HOME`. For Codex workers, the receipt `stdout_preview` and a `codex_task` action's `output` contain the authoritative last-message result. Provider stdout is diagnostic transcript data and is exposed separately as `provider_stdout` or `evidence.provider_stdout_preview`, with additive truncation flags.

The loop does not route automation through the interactive Codex picker. `JIG_CODEX_BIN` selects the executable for both interactive launches and unattended workers; worker timeouts, structured output, process cleanup, and receipts remain owned by the shared worker runner.

## `agent_tooling` Shape

The default rendered config declares the Jig Codex skills marketplace:

```toml
[[agent_tooling.codex.marketplaces]]
id = "jig-skills"
source = "bpcakes/jig-skills"
plugins = [
  "jig-rust@jig-skills",
  "jig-swift@jig-skills",
  "jig-typescript@jig-skills",
  "jig-exec-plans@jig-skills",
]
```

Jig Codex skills are optional Codex plugin bundles used by agents working in generated Jig repos; the default marketplace source is `bpcakes/jig-skills`.

Use `scripts/jig doctor` as the first readiness check for a repo. It reports runtime/contract compatibility, `.jig.toml` validity, required command executables, agent skills, proxy status, vault status, and the next setup command. Raw configured command bodies are always redacted because arguments may contain credentials. Fresh SQLx-enabled scaffolds use a direct `sqlx prepare` command so a trusted CLI can be capability-probed. For recognizable direct `sqlx`, `cargo-sqlx sqlx`, and Cargo-dispatched forms, doctor honors literal long or `-D` database-URL flags, then the effective command-prefix environment reaching the executable, then the captured environment and nearest dotenv file in a proven literal repo-contained cwd. Prefix analysis is ordered: `env -i`, `env -u DATABASE_URL`, and `exec -c` remove earlier assignments, while a later literal `env DATABASE_URL=...` can restore one. External `env` assignment operands follow `env` grammar rather than Bash identifier grammar, so names such as `FOO.BAR` do not get mistaken for executables. Recognized literal external `env`, `nohup`, and external `time` chains are reported in execution order, with every wrapper and terminal target checked under the lookup context that applies at that stage. Bash `command`, `exec`, and `builtin` remain shell syntax, as does bare keyword `time`; unsupported or dynamic wrapper targets retain any known external wrapper checks while the unresolved portion stays visibly unverified. Quoted or escaped heredoc delimiters make their bodies inert for this analysis. Expansion-capable heredocs containing command substitution, command substitutions hidden in arguments or redirections, ambiguous control flow, redirects, cwd or environment mutation, unsupported wrapper options, `env -S` / `--split-string`, and alternate wrapper search paths are visible as present but unverified and point to the authoritative `scripts/jig check sqlx` gate. Prior `hash`, `enable`, and active `trap` mutations also taint later dispatch. Inherited `BASH_ENV`, `ENV`, `CDPATH`, or exported Bash functions make the whole configured command unverified while retaining any literal wrapper and executable presence rows doctor can still prove. Ambiguity never becomes the “No external executable required” pass; JSON uses `present: null` without exposing the command payload. Capability probing is stricter than executable presence: doctor only executes a bare `sqlx` or `cargo-sqlx` found through a trusted absolute PATH entry outside the repository. A literal command-local `PATH` is resolved with the repository root as the command cwd, including relative and empty entries, but those cwd-sensitive entries are never trusted for a SQLx capability probe. Dynamic, cleared, alternate, or persistently mutated lookup state is reported with JSON `present: null`; doctor neither substitutes the captured ambient path nor probes through that boundary. An explicit executable path remains independently checkable after such a mutation. A `cargo sqlx` dispatcher remains present but unverified because Cargo aliases, included configuration, home overrides, or a PATH wrapper can change what the real command runs. Explicit, relative, repo-local, symlink-mediated, identity-ambiguous, or inherited-shell-state-sensitive executables are likewise presence-only. Authorized probes run in a bounded process tree with an isolated home/temp directory and scrubbed environment, never the repository command's ambient credentials. The PostgreSQL probe treats only one driver-specific diagnostic line containing the synthetic sentinel, the `sslmode` option, and an invalid-value error as proof of support; unrelated output cannot produce a compatible result. On Unix, one serialized signal owner covers every external check in a doctor invocation: SQLx capability probes, the configured Codex marketplace-support probe, and the launcher-backed proxy diagnostic in either feature mode. Its mutex guard remains held through process-tree retirement, handler restoration, and restored-signal redelivery. A clean retirement permits a later invocation in the same process with a fresh generation; unsafe handler, generation, or quiescence retirement permanently poisons later sessions, and a recorded termination request always redelivers after safe restoration or exits fail-closed with its conventional signal status. Ctrl-C cancels and reaps the exact active tree before the original signal behavior is restored and redelivered, and a retained cancellation prevents later check families from starting. Codex probe output uses the ordinary 16 KiB-per-stream diagnostic bound and a finite timeout. Jig removes Bash startup, option, trace, and exported-function controls from that owned capability child while preserving ordinary Codex/authentication environment. Proxy-list stdout is capped separately at 8 MiB so every valid 4 MiB route-state document plus its status envelope remains representable, while diagnostic stderr retains the 16 KiB cap. A confidently detected missing SQLx driver still blocks readiness. No command, URL, credential, cwd, probe environment, or probe output is included in human or JSON reports. Use `scripts/jig agent doctor` when you only need to report whether the local Codex installation can use the configured marketplace and to show diagnostic plugin enablement flags. Human-readable output is the default. Pass `--json` for stable structured automation output. `agent doctor` exits nonzero until required setup is complete. The agent check requires Codex marketplace support and registered marketplace sources; plugin enablement is reported separately because the supported Codex bootstrap path is marketplace registration. Use `scripts/jig agent bootstrap` to run `codex plugin marketplace add` when exactly one marketplace is configured. If multiple marketplaces are configured, `agent bootstrap` requires `--marketplace <source>` so a repo cannot install several user-level Codex marketplaces by default. `agent bootstrap` mutates user-level Codex config, so it is intentionally separate from the project-owned `bootstrap_command`.

Doctor also treats inherited `SHELLOPTS`, `BASHOPTS`, `PS4`, and `BASH_XTRACEFD` as shell-state ambiguity. Exported Bash functions are recognized by their byte-exact environment keys, including non-UTF-8 names, so none of these controls can authorize a SQLx capability probe.

A syntactically plain leading `!` is part of Bash command-prefix grammar, so doctor keeps scanning later literal assignments such as `! DATABASE_URL=sqlite:... sqlx prepare`; quoted or escaped `!` remains an ordinary command word. If several supported termination signals arrive during one owned doctor subprocess, the first stays sticky for cancellation/result selection, every distinct signal is retained, and the restored dispositions receive them in deterministic first-then-remainder order only after the process tree is clean and the handlers are quiescent.

Omitting `agent_tooling`, `agent_tooling.codex`, or `agent_tooling.codex.marketplaces` uses the default Jig skills marketplace. Set `marketplaces = []` to opt out explicitly. In `agent doctor` output, `codex.available` is `true` or `false` when Codex is required, and `null` when the Codex probe is skipped because `marketplaces = []`.

For local development against a sibling checkout, either pass `--marketplace` or set `JIG_SKILLS_MARKETPLACE`. Explicit `--marketplace` wins over `JIG_SKILLS_MARKETPLACE`, and the env var wins over `.jig.toml`; both overrides affect only `agent bootstrap`. Local path sources must be absolute or start with `./` or `../`; they are resolved from the repo root before Codex is invoked, and missing local paths fail before mutating Codex config. Bare `owner/repo` values are treated as marketplace shorthands, not local paths.

```sh
scripts/jig agent bootstrap --marketplace ../jig-skills
JIG_SKILLS_MARKETPLACE=../jig-skills scripts/jig agent bootstrap
```

## Codex Homes

Use `scripts/jig codex homes` to list `~/.codex`, `~/.codex-*`, and a configured current home outside those conventional paths, together with the account authenticated in each one. Jig queries `codex app-server` for account data instead of reading `auth.json`; add `--usage` to fetch every current rate-limit bucket, including the durations and reset times reported by Codex. Inspections are bounded and use a rolling pool of four homes. Human terminal runs show inspection progress on stderr; `--json` keeps that progress disabled. A logged-out home is reported as account status rather than an inspection or usage failure. For a logged-in account, unavailable requested usage keeps the observed account while making the report partial. Pass `--json` for the runtime-owned schema described in [Public Contract](./public-contract.md).

Use `scripts/jig codex launch [HOME]` to launch Codex with the selected directory as `CODEX_HOME`. A home can be a discovered name such as `codex-1`, an absolute path, or a relative path. Bare names are home names, not relative paths: `work` resolves as `~/.codex-work` or an exact discovered home name, and never as `./work`. Spell a relative directory explicitly as `./work`. The aliases `codex` and `default` both select `~/.codex`. Explicit absolute, relative, and `~/...` paths are resolved directly without scanning discovered homes.

Use `scripts/jig codex resume SESSION_ID` when the session's home is unknown. Jig validates the UUID, queries every discovered home concurrently with Codex app-server `thread/read` without loading turns, reports lookup progress on an interactive terminal, and launches `codex resume SESSION_ID` with the single matching directory as `CODEX_HOME`. A missing session reports every checked home and any inspection or discovery failures. A session found in multiple homes—possible after copying a home—is ambiguous and requires `--home HOME`. Jig also requires `--home` when one home matches but another discovered home could not be inspected or home enumeration was incomplete, because automatic lookup cannot prove the match is unique. A candidate that disappeared or was already a dangling symlink cannot contain the session and therefore remains a warning without blocking a unique match. The `--home` option bypasses app-server lookup for a non-conventional home or one whose app-server is currently unavailable; the directory itself must exist. Arguments after `--` are forwarded to `codex resume` without shell parsing. Add `--dry-run` to inspect the resolved launch; `--json` is accepted only with `--dry-run` and suppresses terminal progress.

With no home, Jig opens a full-screen picker immediately after directory discovery. Every home is usable at once while account and usage details load through `codex app-server` in a four-worker background pool. Use arrows or `j`/`k` to move, `/` to search names, paths, account emails, plans, and states, Ctrl-U to clear the search, Home/End or Page Up/Page Down for larger lists, and Enter to launch the highlighted exact path—even if its usage is still loading. Search results prioritize home-name matches before account metadata and shared absolute-path prefixes. Tab focuses the selected-home pane so the same movement and page keys scroll long, wrapped usage details; Tab returns to the home list. Escape leaves search first and cancels on the next press; `q` or Ctrl-C also cancels. The detail pane shows the exact display path, account, plan, all returned usage buckets and windows, reset timing, and inspection errors. Codex usage windows are labeled from their reported duration, including `5h` and `weekly` for the known limits. Non-interactive callers must provide a home. Use `--dry-run` to inspect the resolved launch, and place Codex arguments after `--` so Jig forwards them without shell parsing. Human dry-run output replaces terminal-control characters and warns when that makes its displayed shell command differ from the exact launch; use `--json` when exact values are required:

```sh
scripts/jig codex homes --usage
scripts/jig codex launch codex-1
scripts/jig codex launch codex-work -- --profile deep-review --search
scripts/jig codex launch ~/.codex-2 --dry-run -- --search
scripts/jig codex resume 019fe6e4-972f-7392-aaf3-58cb652a4e20 -- --search
scripts/jig codex resume 019fe6e4-972f-7392-aaf3-58cb652a4e20 --home codex-work
```

The picker keeps each inspected account email visible at every supported layout width. It shows quota remaining now alongside a linear **at current pace** projection in its wide list; the compact list combines home, account, and projection into one stacked row while the selected-home pane retains full details. It derives elapsed time from the server-reported reset and window duration, then projects the observed used percentage across the complete window. The calculation is fixed to the time that usage was inspected, so navigation or a later redraw cannot change the result without a new sample; reset countdowns, sample ages, stale labels, and recommendations continue to refresh while the picker remains open. The selected-home details tell the user to reopen the picker for a new usage sample. After 15 minutes—or as soon as any contributing usage window resets—sampled remaining quota and projections become muted and explicitly `stale`, and stale samples no longer receive the `+` recommendation marker. A projection appears as the approximate percentage left at reset, how early the quota is expected to run out, or an explicit exhausted state. Values at or above 100% are exhausted; sub-minute reset countdowns and overruns are shown as `<1m` instead of zero minutes.

`+` marks the visible inspected account with the best projected outcome: the most percentage headroom, or the least percentage overrun when every complete projection runs out early. The unfiltered list never reorders while inspection results arrive; an active search can gain or rerank matches when newly inspected account details match the filter. Only the Codex bucket is eligible for recommendation; when it is absent, the first returned generic bucket may still supply the presentation-only remaining and projection values. When Codex returns multiple active windows, the tightest complete percentage projection determines that account's recommendation score regardless of window duration. A timed window with zero usage projects 100% headroom immediately. A nonzero window still in the first 10% of its duration shows its current remaining quota with a `collecting` label and remains unranked. If another window has a complete projection, that projection remains visible with a `partial` label; a sibling with missing timing also makes a complete projection partial. Exhausted accounts are also unranked. A window with no projectable sibling is shown as unavailable. This is a pace indicator from a point-in-time usage sample, not a guarantee about future consumption.

Codex `--profile` selects a configuration profile inside the chosen home; it does not select a separately authenticated account. Separate accounts require separate `CODEX_HOME` directories, which is why this command calls them homes rather than profiles. `CODEX_HOME` marks the current home and is replaced for the launched process. Set `JIG_CODEX_BIN` to override the `codex` executable during local testing or when it is installed at a nonstandard path. A real launch or resume cannot be combined with `--json`; `--json` is supported for `codex homes` and launch or resume dry runs.

Both the picker and `codex homes --usage` human output show quota remaining. Jig labels Codex windows whose reported durations are five hours and one week as `5h` and `weekly`, whether returned alone or together; unexpected durations show their duration without receiving a contradictory role. Other rate-limit buckets show their server-reported durations without imposing Codex-specific labels.

## `work` Shape

The `work` block declares agent workflow defaults without adding repo-local launcher scripts:

```toml
[[work.gates]]
id = "verify"
kind = "evidence"
profile = "verify"
conclusion = "success"
```

An `evidence` gate names exactly one canonical `target = "api:test"` or checked-in `profile = "verify"`. `conclusion` currently accepts only `success` and defaults to it. A target gate requires a successful receipt for that exact target. A profile gate requires receipts for every current profile target from one compatible run; Jig never stitches individually successful targets from different runs into profile evidence.

Contract-v6 repositories generate a single gate for their default verification profile. `scripts/jig work check --plan-id ...` expands all configured evidence gates to exact targets, runs their union once, and records target receipts linked to the work plan. Legacy `kind: check` gates remain supported and must reference no-argument execution tools declared in `.agent/jig-contract.json`; they run in configured order. When both forms exist, default `work check` runs both. Passing one or more `--tool` values explicitly selects only those legacy tools. Human-readable output is the default. Pass `--json` for structured automation output.

Check gates may narrow applicability with repository-relative `paths` and `paths_ignore` globs. Each newly opened work plan records an exact Git baseline; Jig classifies the baseline-to-current changes once, emits explicit `not_applicable` evidence when no scoped input changed, and fails closed when applicability cannot be proven. `reuse = true` permits only exact-input evidence from a direct successful execution to be reused across plans. `scripts/jig work check --gate ID` forces named check gates to execute, but forced execution does not turn unknown applicability into closure evidence. These fields require contract version 5 or later and apply equally when legacy check gates coexist with contract-v6 evidence gates.

`scripts/jig work gates --plan-id ...` reports each configured gate as `passed`, `missing`, `failed`, `stale`, `unknown`, or `unsupported`. A syntactically valid gate whose check tool, target, or profile was renamed remains inspectable as `unsupported` with a reason; contract validation and `work check` still reject the stale reference before execution. Pass `--json` when automation needs the full structured payload. `scripts/jig work evidence` is the higher-level human view: it shows the latest gate evidence per tool, target, or profile, whether it matches current inputs, changed paths covered by its receipts, and the exact stale or unknown reason. For `work gates` and `work evidence`, top-level `ok: true` means the inspection command completed; read `overall`, `gates_ok`, and each gate `status` to decide whether work is blocked. Receipt `changed_paths` are bounded repo-relative previews from `git status --porcelain=v1 -z`; they exclude `.agent/**` but can include untracked filenames, so do not treat receipt JSON as secret-free metadata if local filenames are sensitive. `scripts/jig work finish --plan-id ...` refuses to close work while required gates are missing, failed, stale, unknown, or unsupported. Legacy check freshness uses the non-`.agent/` worktree fingerprint from its latest check or check-batch receipt. Target evidence additionally requires the current contract digest and deterministic target input digest; receipts missing any of those metadata fields are `unknown`, not passing.

Required check gates should not create or modify non-`.agent/` files during `work check`. Build outputs, generated metadata, and lockfiles should be committed when they are source-of-truth, ignored when they are disposable, or generated before running the fingerprinted check. If a check does intentionally settle generated files, rerun `scripts/jig work check --plan-id ...` after reviewing those changes so the gate evidence matches the final worktree.

After upgrading an in-flight repo from a Jig version that recorded receipts without `worktree_fingerprint` or target digests, rerun `scripts/jig work check --plan-id ...` before `scripts/jig work finish --plan-id ...`. Older successful receipts deserialize correctly, but their freshness is `unknown` and required gates block finish until fresh evidence exists.

For compatibility, older repos may still use `work.checks`; Jig backfills entries that are not already declared in `work.gates` as required `kind: check` gates with generated IDs. When a tool is declared in both places, the explicit `work.gates` entry is authoritative. New repos should use `work.gates`.

Generated v6 profiles include applicable SQLx, sqlc, schema, language, frontend, and contract targets. Generated legacy repositories continue to emit the corresponding tool gates such as `jig.sqlx_check`, `jig.schema_check`, and `jig.schema_dump`. The legacy catalog's derived default verification profile excludes the one known effectful historical gate, `jig.schema_dump`; that gate retains its direct `work check` behavior instead of becoming part of bare `jig check`. Any other configured legacy gate that is not a read-only check is rejected instead of being silently omitted.

Review gates are intentionally separate from native check gates. A `codex_review` gate runs a configured Codex skill through `codex exec review --output-schema`, records a structured `jig.work_review` receipt, and is enforced by `work gates`, `work evidence`, and `work finish` like check evidence:

```toml
[[work.gates]]
id = "rust-error-handling"
kind = "codex_review"
skill = "jig-rust:rust-error-handling-review"
severity = "high"
required = true
```

Use `scripts/jig work review --plan-id ...` to run all configured review gates, or pass `--gate <id>` to run a subset. Review findings are normalized to `critical`, `warning`, or `suggestion`; both `fail_on` and `severity` accept the normalized names plus these aliases:

| alias | normalized threshold |
| --- | --- |
| `high` | `critical` |
| `medium` | `warning` |
| `low` | `suggestion` |

Omitted thresholds default to `critical`. If both `fail_on` and `severity` are present, `fail_on` chooses the active threshold, but both values must be valid. `scope` defaults to `uncommitted`; supported values are `uncommitted`, `base:<ref>`, `base=<ref>`, `commit:<sha>`, and `commit=<sha>`. `model` is passed to Codex when present.

`scripts/jig work refine --plan-id ...` runs a review-driven fixer loop. It runs review gates, passes actionable findings to `codex --ask-for-approval never exec --sandbox workspace-write` for direct repository edits, reruns review gates, then reruns normal check gates. Enabling refinement opts into unattended Codex workspace writes: the prompt tells the fixer not to run git, but the sandbox still permits repository edits. Review skills used with refinement are trusted inputs because their finding text is handed to an auto-approved workspace-writing fixer; keep refinement-enabled review skills sourced from trusted Codex marketplaces or repos and review the resulting diff before closing work. Refinement requires one explicit `[[work.refinements]]` entry before Jig will invoke the workspace-writing fixer. Without a refinement `model`, the fixer uses the first selected review gate model when present. `--max-iterations` controls fixer attempts and defaults to 1, meaning Jig fixes once and then verifies. Passing `--gate` narrows only the review gates; the final verification step still runs all configured check gates. An optional `[[work.refinements]]` entry provides a repo-local refinement profile for the fixer prompt:

```toml
[[work.refinements]]
id = "rust-simplify"
skill = "jig-rust:rust-simplify"
mode = "fix-actionable-review-findings"
```

## `frontend_apps` Shape

Each entry in `frontend_apps` must be an object:

```toml
[[frontend_apps]]
name = "frontend"
dir = "frontend"
coverage_threshold = 40
kind = "vite"
role = "spa"

[[frontend_apps]]
name = "admin-panel"
dir = "admin-panel"
coverage_threshold = 80
kind = "vite"
role = "admin"
```

`kind` records the frontend execution family (`vite` or `env-port`) while `role` records semantic scaffold/config metadata (`spa`, `admin`, or `astro`). New renders persist both fields. An explicit role is preserved. When role is omitted, `env-port` maps to Astro, the historical generated `admin` / `admin-panel` names map to the admin role, and other Vite apps map to SPA. Jig recovers an omitted kind from an exact same-name/same-directory `[[dev.apps]]` before applying those role defaults. During adoption, the dev script is authoritative: a direct Vite command yields Vite/SPA unless the historical admin name applies, a direct Astro command yields env-port/Astro, and every other server stays env-port/SPA even when Astro appears only as an incidental dependency.

The `rust-react` preset renders a private root JavaScript workspace and pins Node plus the selected package manager. Fresh Yarn 4 scaffolds explicitly use the `node-modules` linker; adopted Yarn Classic and Berry PnP layouts remain project-owned and supported. Workspace membership is manager-specific: pnpm reads `pnpm-workspace.yaml`, npm and Bun read package.json workspaces, and Yarn additionally accepts its legacy object form. Unsupported or malformed authoritative workspace syntax fails closed instead of silently selecting another install boundary. For declared members, npm, pnpm, Bun, and Yarn Classic use the root project even when a nested lock exists; a nested modern Yarn Berry lock remains its own project. A standalone pnpm package beneath an unrelated parent workspace is installed with pnpm's explicit workspace opt-out. Contradictory metadata for an unused manager is ignored.

For node-modules installs, readiness covers the install tree of the root and every authoritative workspace member, including launcher contents and execution metadata; a member-local install or same-size shim replacement cannot hide behind a valid root tree. A missing, empty, or ignored-only real install root uses the same absent proof, so a root-hoisted install remains ready when ordinary tooling later creates only cache output in a previously absent member `node_modules`. Within each actual install root, only real top-level `.cache`, `.vite`, `.vite-temp`, and `.tmp` directories plus a regular `.DS_Store` are excluded as tool runtime output. The same names nested below a package, replaced with another filesystem type, or placed in a separately configured virtual store remain attested, as do unknown directories, package manifests, dependency metadata, symlinks, and executable launchers. Stamp v5 and marker v3/v2 remain compatible with receipts created while a member root was absent; a historical receipt created while an ignored-only container was already present safely requires one reattestation. A stale configured frontend is repaired by `scripts/jig bootstrap`, while a stale dev-only or discovered manager-owned directory reports its explicit per-directory checker bootstrap command. Dev never installs implicitly. On Linux, lock ownership includes the PID namespace as well as boot/start identity, and worker handoff stays bound to the exact captured process generation so PID reuse cannot authorize or spin a waiter.

The generated checker fingerprints every authoritative member manifest plus manager configuration and patch inputs. For Yarn, it also walks every in-repository ancestor between the selected app and its install scope, binding the nearest package-manager pin, Yarn configuration, and invoked runtime version into the receipt. Before any Yarn subprocess runs, intermediate authority paths plus configured `yarnPath`, plugin paths, and equivalent environment overrides must be literal, real in-repository paths; symlink-mediated, escaping, or unsupported dynamic forms fail closed. Its versioned receipt proves the installed structure: a node-modules receipt and deterministic structure digest, the complete effective Yarn PnP loader/data/cache companions, or an explicit empty-package proof when every effective manifest has no dependency sets. For Yarn Classic, the fingerprint includes the runtime's effective PnP/cache/path inputs, version, platform, and relevant environment overrides; after installation Jig stamps the exclusive artifact actually produced instead of reimplementing Yarn 1 flag precedence. Classic PnP proof binds local workspace manifests without hashing workspace source and recursively attests only referenced external-cache or unplugged package directories. Yarn Berry asks the pinned Yarn runtime for its effective linker and artifact paths, supports shared global and custom caches, and fingerprints only cache archives and unplugged directories referenced by the PnP runtime state plus the configured install state and required data/ESM loader. Unrelated shared-cache additions therefore leave readiness intact. Replacing, truncating, symlinking, or removing any authoritative input or required artifact invalidates readiness. Bootstrap, web CI, and browser E2E CI use the same checker and scope; generated workflows first provision a fallback Node so the Node-backed scope resolver can choose the exact version before cached setup. They synthesize the pinned fallback version file only when the resolver reports that no applicable file exists; malformed scope or authority errors keep their failing status. Local checks accept dependencies only when both proofs match. Before launch, `scripts/jig dev` applies workspace discovery and `--app` selection, then checks matching configured frontends, every Vite app, and discovered package-manager `run dev` apps with a package manifest in that resolved plan; unrelated direct env-port services do not become web dependency scopes. The preflight owns and bounds its checker process tree and removes Bash startup/function overrides that could spoof readiness. Historical managed checkers that do not advertise `dependencies-ready` retain their pre-preflight behavior until update, while a checker advertising that command must return its three-way readiness status or fail startup visibly. Install ownership is generation- and process-identity-aware: a dedicated worker keeps the lock while descendants run, including after its coordinator exits; a verified live coordinator remains authoritative for as long as its exact process generation is alive, so scheduler delay cannot expire a valid handoff. Kernel/UTC-stable start identities prevent live-owner theft, proved-stale locks can be recovered, and unresolved identities time out without destructive recovery. Linux identities combine the boot UUID with validated process start ticks. The generated pnpm workspace narrowly authorizes esbuild's pinned platform-binary install script; adding another `allowBuilds` entry authorizes dependency code execution and requires review.

The generated checker supports the stock macOS Bash 3.2 surface as well as newer Bash releases. Root and standalone structural proofs never expand an empty array under nounset. Yarn authority enumeration is captured synchronously so a producer error blocks readiness and receipt publication, and an install coordinator decides whether an interrupted `wait` still owns a live worker through Bash's job table rather than probing a reaped, recyclable PID. Generated web and browser-E2E workflows run every package script through `scripts/check-webapps.sh run-script <app-dir> <script>`; the separate pre-install validation step still reports missing required scripts before network work.

For npm, `npm-shrinkwrap.json` is the active lock whenever it exists and `package-lock.json` is the fallback; only the selected lock enters the readiness receipt, matching npm's precedence. Both names remain workflow/cache inputs so creating, removing, or switching authority schedules validation. Frozen and first-time npm installs explicitly select the complete root or standalone workspace scope, project location, real writes, lock creation, executable links, the current platform, and development, optional, and peer dependency classes. Hostile `NODE_ENV` plus inherited npm omit, dry-run, lock-only, global/prefix/location, platform, bin-link, or workspace-selection settings therefore cannot publish a partial or stale tree as ready. Managed package-script execution similarly pins the exact app, local project location, enabled current-workspace selection, required-script behavior, and complete dependency classes while removing ambient selectors that could redirect execution or synthesize `NODE_ENV=production`; an explicit application `NODE_ENV` remains intact. Use `scripts/check-webapps.sh run-script <app-dir> <script>` for that boundary. Registry/authentication, dependency layout, peer-resolution, and npm 12 install-script approval or denial remain project-owned; the downstream lint/build/test gates prove executable behavior. The dependency preflight additionally removes Bash directory, option, trace, and byte-exact exported-function controls as well as startup-file overrides before invoking the checker.

Fresh pnpm scaffolds set `enableGlobalVirtualStore: false` explicitly so local and CI pre-run validation use the same repository-local installed layout and never reconcile an environment-only default by rewriting executable shims. The checker binds that normalized setting into its pnpm runtime contract: global-store `true`, pnpm 11's unset value, and inherited npm/pnpm environment overrides fail before install; pnpm 10's legacy unset behavior normalizes to false. The structural proof excludes only regular files at the exact root paths `node_modules/.pnpm-workspace-state.json` and `node_modules/.pnpm-workspace-state-v1.json`, the volatile pnpm 10/11 validation caches. Creating, deleting, or rewriting those files therefore does not force a reinstall, but a symlink, directory, nested same-named entry, `.modules.yaml`, executable shim, package/link tree, or authoritative pnpm manifest, lock, configuration, hook, patch, or runtime change remains attested. pnpm's own pre-run dependency verification remains enabled.

The pnpm checker makes one bounded, hook-disabled configuration query and binds a normalized nonsecret layout contract—including linker, modules/virtual-store placement, hoisting, symlink/import behavior, cache and dependency-verification settings, and case-insensitive npm/pnpm environment overrides—alongside the exact pnpm version and platform. PnP, external artifact authority, unsupported custom layouts, and ambient overrides fail before installation. pnpm's receipt and workspace-state exclusions apply only at the actual scope root; the general tool-runtime cache exclusions above apply independently to each actual node-modules install root. Executable-shim content or mode and every remaining root/member package tree stay attested.

Each configured app directory is expected to support:

- install: `bun install --frozen-lockfile`
- lint: `bun run lint`
- typecheck: `bun run typecheck`
- build: `bun run build:bundle`
- test coverage: `bun run test:coverage`

The coverage command must write `coverage/coverage-summary.json` in the app directory; generated local checks and web CI enforce each app's `coverage_threshold` from that summary. The generated SPA Vitest configuration includes production `src/**/*.{ts,tsx}` by default and excludes only tests, test setup, the render-only entrypoint, and generated UI primitives, so adding an untested feature or API module affects the advertised threshold instead of silently falling outside coverage.

## `dev` Shape

The `dev` table configures `scripts/jig dev` and `scripts/jig proxy`. This is runtime-owned local machine behavior, not an individually declared generated contract tool. Generated repos include a `[dev]` table with conservative defaults; repos that remove it use the defaults of the runtime selected for their contract epoch.

```toml
[dev]
proxy_port = 1355
https_port = 1443
https = false
http2 = true # HTTPS listener ALPN only; cleartext proxy traffic remains HTTP/1.1
lan = false
tld = "localhost"
workspace_discovery = false

[[dev.apps]]
name = "api"
kind = "env-port"
command = "cargo run --bin api"
port = 4000
proxy = true

[[dev.apps]]
name = "web"
dir = "apps/web"
kind = "vite"
argv = ["bun", "run", "dev"]
host = "127.0.0.1"
```

`proxy_port` is the TOML name for the HTTP listener and must be a stable nonzero port. The matching CLI override is `--http-port`; an explicit `--http-port 0` requests a kernel-assigned ephemeral port for that runtime, while generated service files still require a stable nonzero port. `https_port` is the HTTPS listener; the matching CLI override is `--https-port`.

The `[dev]` table accepts these keys. Unknown keys are rejected so typos do not silently change local runtime behavior.

| Key | Type | Default | Scope |
| --- | --- | --- | --- |
| `proxy_port` | integer TCP port | `1355` | HTTP proxy listener |
| `https_port` | integer TCP port or omitted | `1443` | HTTPS proxy listener when `https = true` |
| `https` | boolean | `false` | enable the HTTPS listener |
| `http2` | boolean | `true` | enable HTTP/2 ALPN on the HTTPS listener |
| `lan` | boolean | `false` | bind the proxy to `0.0.0.0` for trusted LAN testing |
| `tld` | string | `"localhost"` | private/local route suffix |
| `workspace_discovery` | boolean | `false` | discover JavaScript workspace apps |
| `apps` | array of `[[dev.apps]]` tables | `[]` | supervised app definitions |

CLI runtime flags override listener defaults for a single invocation: use `--https`/`--no-https`, `--lan`/`--no-lan`, and the diagnostic `--http2`/`--no-http2` pair when needed.

Each `[[dev.apps]]` table accepts `name`, `dir`, `kind`, `command`, `argv`, `port`, `host`, and `proxy`. Unknown app keys are rejected. `name` is required; `dir`, `command`, `argv`, `port`, and `host` are optional; `kind` defaults to `"env-port"`; `proxy` defaults to `true`. Both `[[dev.apps]].dir` and `[[frontend_apps]].dir` use repository-relative spelling with `/` separators (`.` is allowed); absolute, parent-traversing, and backslash-separated forms fail during config validation. Selected relative paths are then canonicalized and must remain inside the repository, so a symlink cannot bypass containment.

`tld` must use a private/local suffix: `localhost`, `local`, `test`, `internal`, or one subdomain beneath one of those suffixes such as `demo.test`. Public or browser-owned TLDs such as `dev`, `com`, and `io` are rejected so the proxy cannot mint routes that collide with routable DNS names.

Supported dev app kind values are `env-port` and `vite`.

`kind = "env-port"` starts the command with `PORT=<free-port>` and `HOST=127.0.0.1`, overriding inherited values so Jig controls apps that bind from those conventional variables. Framework-specific variables such as `VITE_PORT` or `SERVER_PORT` are not rewritten; configure those apps to honor `PORT`/`HOST`, fail on a busy port, or use a structured app kind that injects framework flags. `kind = "vite"` injects Vite-style `--port`, `--host`, and `--strictPort` flags when they are not already present. Jig also applies the same Vite flags to argv forms that directly invoke Vite, such as `vite`, `npx vite`, `bunx vite`, or `pnpm exec vite`. If a Vite argv already includes `-p` or `--port`, that value must match the Jig-assigned app port. For package-manager commands such as `bun run dev`, `pnpm run dev`, `npm run dev`, or `yarn run dev`, Jig inserts the `--` separator before Vite flags. The exact generated npm dev argv also neutralizes ambient npm package-routing, missing-script, and omit/include selectors while retaining explicit application environment and project npm policy; a custom npm argv continues inheriting all caller settings. Vite apps must use `argv`; shell-form `command` is rejected for Vite because safe flag injection requires structured arguments. If both `argv` and `command` are present, `argv` is used.

The Vite integration also sets Vite's `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS` environment hook for the generated dev hostnames. Treat that hook as a Vite compatibility boundary: if your Vite version changes or removes it, use explicit Vite `server.allowedHosts` config or `kind = "env-port"` until Jig is updated.

Generated repos may contain both legacy `[[frontend_apps]]` and matching `[[dev.apps]]` entries. In that shape, `[[frontend_apps]]` keeps CI and coverage metadata, while `[[dev.apps]]` owns local `scripts/jig dev` settings and takes precedence. Every frontend app must have a same-name dev app with the same `dir`; extra dev-only apps are allowed. Legacy `[[frontend_apps]]` entries are still supported as a fallback only when no `dev.apps` are configured, so older generated repos can use the proxy.

To migrate a legacy frontend entry, create a matching `[[dev.apps]]` entry with the same `name` and `dir`, set `kind = "vite"` for Vite-style frontends, and set `argv` to the package-manager dev command such as `["bun", "run", "dev"]`. `coverage_threshold` stays with the older frontend check workflow and is not used by `scripts/jig dev`; keep any build, lint, typecheck, or coverage commands in project-owned scripts or Make targets.

Jig rejects unknown top-level `.jig.toml` keys and unknown keys inside known tables. During upgrades, remove experimental keys or move repo-local notes outside `.jig.toml`; template-owned compatibility keys are listed in the required and optional sections above.

When `workspace_discovery = true`, Jig discovers common JavaScript workspace package globs under the repo root after `JIG_DEV_ALLOW_WORKSPACE_DISCOVERY=1` is present in the environment, because discovered package `dev` scripts are executable repo code. The matching one-shot CLI override is `scripts/jig dev --discover-workspace`. Discovery supports `*`, `**`, and leading `!` exclusions, but not brace expansion such as `apps/{web,admin}`. Discovery skips `node_modules`, dot-directories, symlinked package directories, and canonical paths outside the repo root. Glob expansion fails closed after 10,000 matches; narrow workspace globs in very large monorepos.

`scripts/jig dev` only launches configured `[[dev.apps]]`, legacy `[[frontend_apps]]`, or workspace-discovered apps. It does not run the generic top-level `dev_command`; keep repo-wide commands that do not bind a supervised app port in project-owned scripts or Make targets. When `--app` is present, Jig validates the complete semantic configuration first, applies the name selection, and only then resolves selected app directories. A stale or missing unselected directory therefore cannot block a valid explicit selection, while an invalid name, kind, command, host, proxy setting, or legacy/dev relationship still fails globally; without `--app`, every configured directory remains required.

Each dev app defaults to `host = "127.0.0.1"` and `proxy = true` unless a `[[dev.apps]]` entry overrides them. This is the backend target host that Jig forwards to, not the proxy listener address. Proxied dev apps, including legacy `[[frontend_apps]]` entries, must target loopback IP literals such as `127.0.0.1` or `::1`; use `scripts/jig proxy alias` for deliberate proxied non-loopback local tunnels. A `proxy = false` app is launched directly without publishing any Jig proxy route, so its `host` value is only passed to the child process as the app bind target. Set `port` only when an app must use a fixed backend port; otherwise Jig assigns a free port in the local app range.

On Unix, foreground `dev` and `proxy run` sessions distinguish SIGINT, SIGHUP, and SIGTERM, clean every owned child/process group and route, report the signal in human and JSON output, and exit with 130, 129, or 143 respectively. These cancellations are structured stopped states rather than generic runtime failures. Startup route, certificate, runtime, proxy-start, and readiness waits observe the same termination intent; interruption during route publication cleans the ready-but-unpublished child and does not leave a route. The first signal requests graceful cleanup; any later signal escalates to prompt forced process-tree cleanup while preserving the first signal's reported reason and exit status. Once an app process can exist, the handler publishes only atomic intent and child termination plus route mutation stay on the ordinary foreground thread; before any owned resource exists, a later signal may use the platform's conventional immediate-exit path. Foreground termination registration is process-one-shot: callbacks remain generation-scoped and retirement is fail-closed, but another foreground dev/proxy command in the same host process is rejected with guidance to start a new Jig process. This avoids reusing mutable signal state while a callback selected under an earlier operating-system registration might still enter. On Linux, every unreadable or malformed `/proc/<pid>/stat` entry is checked with `getpgid`; it is skipped only when that proves a different group or a vanished process. One absolute cleanup deadline is checked throughout procfs enumeration, stat reads, and fallback membership probes; expiry is an error rather than evidence that the group is empty. Owned or unclassifiable entries remain fail-closed, and an uncertain or empty SIGTERM-phase scan escalates to SIGKILL before two post-kill empty scans may prove the group is gone. On macOS, non-consuming child observation treats stop, trap, and continue records as running; group `EPERM` is accepted only after a fresh terminal observation and an atomic two-slot membership snapshot prove the exact unreaped leader is the sole remaining member. Additional members—including transient zombies—stay pending under the fixed cleanup deadline, so an unsignalable live member still fails closed. Unix targets without a process-group membership scanner retain a bounded SIGTERM grace period before forcing the group; a later signal bypasses the remainder.

TERM grace and SIGKILL confirmation each receive one absolute process-cleanup phase deadline. Direct-leader exit and transitions between running/exited helpers do not replenish that budget; procfs opening, iterator advances, stat/fallback classification, final live/empty acceptance, and polling sleeps all remain bounded by the same phase value. TERM exhaustion begins a new forced-cleanup phase rather than reusing an exhausted deadline.

Cleanup is armed before any proxy startup child can spawn. Published process routes carry the exact owner PID/start token, and every success or maybe-committed error path removes only that owned route; an older session cannot delete a newer replacement. The first supported signal remains graceful while owned finalization is active, and only a later signal forces that cleanup. After every resource owner has finished, Jig retires the resource/generation state before restoring handlers; a signal entering after that cutover takes the platform's conventional immediate-exit path instead of being cleared by teardown. Targets without safe process-tree supervision reject app launch before spawning.

When running multiple apps with `scripts/jig dev`, Jig treats them as a tied dev session: when the first child process exits, Jig removes the session routes and terminates the remaining child processes.

The proxy state directory also holds a private registry for active tied dev sessions. Records are keyed to the canonical repository root and carry supervisor/app observations, durable pre-spawn state, a durable preflight-cleanup obligation, an explicit cleanup-required marker, and an authenticated loopback control endpoint; `scripts/jig dev status` and `scripts/jig dev stop` expose only sanitized session data, never the control credential. Status is read-only and limited to the current repository. Stop targets all registered sessions for that repository, succeeds when none exist, and asks the live supervisor to use its retained child handles for cleanup. If the supervisor is gone while preflight cleanup is unconfirmed, an app spawn is pending or unknown, or a registered identity remains live or cannot be classified safely, Jig returns `ok: false` and retains the record without signaling persisted numeric PIDs. If cleanup evidence is complete and every exact registered identity is absent, status labels the session `recoverable`; explicit `dev stop` or `dev --replace` then retires the dead orphan, removes only process routes still owned by its recorded PID/start-token pairs under the shared state lock, and emits a structured recovery notice that preserves app targets, spawn states, last-known PIDs, and explicitly forgotten ambiguities for diagnosis. Successful recoveries are separate from blocking warnings, and foreground `dev --replace` results retain recoveries completed before the replacement session was claimed. After independently confirming that no unrecorded process from interrupted preflight or spawn work remains, `scripts/jig dev stop --forget-ambiguous-orphans` may forget only dead-supervisor records blocked by unconfirmed preflight cleanup or pending or untracked spawn evidence. It still refuses any live or uncertain registered process identity, never signals a stored PID, and records that process absence could not be proved.

Bare `scripts/jig dev` remains the launch form. `scripts/jig dev --replace` resolves conflicts before launch by stopping only overlapping registered sessions from the same canonical repository, then atomically retrying the claim. It refuses cross-repository ownership, unregistered live process routes, and newly observed concurrent claims. This means a process launched by a pre-registry Jig version or by an ad-hoc command is never killed merely because its route hostname conflicts; stop that process explicitly and run `scripts/jig proxy prune` once.

Session lookup uses the same proxy state-directory resolution as launch. With the default, no extra flag is needed; with an isolated state directory, use `scripts/jig dev --state-dir <path>` for launch, `scripts/jig dev status --state-dir <path>`, and `scripts/jig dev stop --state-dir <path>`. The status and stop subcommands do not accept launch-only app, discovery, replacement, or listener flags. `--forget-ambiguous-orphans` is a stop-only repair option and is never implied by `--replace`.

Jig chooses automatic app ports with a local bind probe against every socket address resolved from each app's configured target host, then starts the child command and waits for the target port before publishing the process route. Readiness has no fixed wall-clock timeout because first-time compilation can legitimately take several minutes; the wait ends when the app listens, exits, or the user interrupts the session. The probe does not reserve the socket across arbitrary package scripts, so Jig verifies that the observed listener belongs to the spawned child process group before publishing. If a concurrent local process steals the port or the app rebinds to a different port, Jig reports an app readiness failure instead of publishing the bad route.

App stdout and stderr are collapsed during a dev session. On an interactive terminal, Jig replaces the raw stream with one animated, colored status line per starting app, using the latest meaningful child-process message (for example, `[..] api · Compiling example-app`). The line follows the actual stderr terminal width and clips Unicode by display width. It is cleared when the app becomes ready, leaving only the compact app table. If an app fails during startup or exits unsuccessfully, Jig stops its process group and then prints that app's captured output tail (up to 2 MiB) before the failure summary. Successful app output, interrupted output, and output from apps stopped only because another app failed remain hidden. `NO_COLOR` disables color while preserving the live status line.

`scripts/jig proxy run` uses `--` before the command to run, for example `scripts/jig proxy run web -- vite --open`. Use `scripts/jig proxy run web --no-proxy -- <command>` when you only want Jig to assign `PORT`/`HOST` and supervise the process without registering a proxy route. App execution options such as `--kind` and `--port` still affect that direct child process; proxy listener options such as `--http-port`, `--https-port`, `--https`, `--lan`, and `--tld` are rejected with `--no-proxy`.

`scripts/jig dev` and `scripts/jig proxy run` execute repository-configured commands and package scripts. Only run them in repositories you trust.
`[[dev.apps]]` commands and `proxy run -- <command>` are executed from the configured app directory with Jig-provided `PORT` and `HOST` values, and they inherit the invoking shell environment so ordinary dev credentials and toolchain variables keep working. Derived `JIG_DEV_<APP>_{HOST,PORT,ORIGIN,URL}` values are runtime-owned coordinates for the current selection: inherited copies are removed before Jig injects the selected apps, preventing an omitted or stale service from impersonating current topology. Unrelated controls such as `JIG_DEV_BIN` and `JIG_DEV_ALLOW_WORKSPACE_DISCOVERY`, generic project variables such as `API_ORIGIN`, and the rest of the caller environment remain inherited. Generated SPA/admin Vite configs prefer the current `JIG_DEV_API_ORIGIN`, then a nonblank `API_ORIGIN`, then their stable proxy hostname; use `API_ORIGIN=... scripts/jig dev --app web` when the backend is managed separately. The generated Playwright server sets both origin variables to its isolated backend. The string `command` form runs through the platform shell from committed repo configuration; use it only for trusted repos and prefer `argv` for literal argument passing. The long-running background proxy is different: Jig launches it with a cleared environment plus explicit proxy state and minimal toolchain variables, and Unix background starts detach from the caller's working directory. Apps that prefer framework-specific port variables must be configured to derive them from `PORT` or use a Jig app kind that supplies equivalent flags.

Equivalent portable config spellings such as `web`, `./web`, and repeated separators use one lexical identity for frontend/dev relationships; absolute, parent-relative, case-only, and symlink-alias spellings do not become portable aliases. Selected app paths remain canonical through containment and readiness checks.

The proxy process is shared local runtime state and can outlive a `scripts/jig dev` session. Use `scripts/jig dev stop` to stop this repository's registered app sessions; use `scripts/jig proxy stop` when you want to shut down the shared background proxy listener itself. Proxy and dev-management commands print their JSON response before returning when `--json` is supplied; when a response contains `ok: false`, including stop refusals that deliberately retain runtime files to avoid terminating an unrelated process, the CLI exits nonzero so scripts should inspect its `warning` or `warnings` field. Successful orphan retirement is reported separately in the `recoveries` array.

The proxy is intended for trusted local development and trusted LAN testing only; it does not provide authentication or multi-tenant isolation.

Routes and aliases are repo-scoped by default. For example, in a repository named `demo`, `scripts/jig proxy alias api --port 8080` registers `api.demo.localhost`, matching the same certificate wildcard used by `scripts/jig dev` apps.

Aliases default to `127.0.0.1`. If you pass `--host`, it must be an IP literal; DNS names are rejected so alias routing cannot depend on mutable hostname resolution. Non-loopback alias targets require `--accept-non-loopback-target`; treat those aliases as local access grants to that target IP and avoid pointing them at sensitive internal services. In LAN mode, aliases may only target loopback IP literals so `0.0.0.0` proxy binding cannot expose arbitrary internal hosts.

Forwarded HTTP and WebSocket requests use the routed development hostname in `Host` and `x-forwarded-host`. Jig replaces inbound `x-forwarded-for` with the direct client address instead of trusting client-supplied chains. Apps that enforce hostnames should allow the generated route names.

Proxy forwarding appends a standard `Via` hop marker to HTTP and WebSocket requests and responses, using the inbound protocol version such as `1.1 jig` or `2.0 jig`. HTTP request bodies are streamed with a 100 MiB forwarding limit, so large upload workflows should bypass the proxy or raise that limit in code. Backend HTTP requests are normalized to HTTP/1.1 even when the client reaches the TLS listener over HTTP/2; HTTP/1 keep-alive is disabled so each HTTP/1 client request uses a fresh connection. The health endpoint requires a loopback client address, a loopback `Host` value, and the per-run health token stored in the private proxy state directory. When the connection limit is reached, the proxy applies backpressure before accepting more sockets, so clients may wait in the OS backlog or time out; slow TLS handshakes are bounded by a short handshake timeout. HTTP/2 is additionally bounded by the configured max concurrent streams per connection and a global active-request limit.

Non-upgrade WebSocket backend responses are drained with a bounded 10 MiB body limit so error pages can be returned without allowing unbounded buffering.

The proxy stores mutable local state under `~/.jig/proxy` by default, or `JIG_PROXY_STATE_DIR` when set. Commands that accept proxy runtime flags also accept `--state-dir <path>` for explicit per-call isolation. This state is deliberately outside `.agent/state` because routes, PID files, port files, certificates, and advisory lock files are mutable machine-local runtime data. Route state is versioned JSON. Route hostnames are shared state-dir keys: if multiple repos use the same state directory and hostname, Jig treats that as the same route and refuses live process-route replacement. Shared state directories reuse one leaf certificate and add hostnames for live routes and aliases, so use separate state directories when many repos or aliases would otherwise make the certificate SAN list too large. State mutations use advisory locks and wait up to 30 seconds before reporting a lock timeout. On Unix, Jig makes the newly created default state parent (`~/.jig`) and the default, newly created, or empty explicit state directory mode `700`; existing default parents and existing non-empty explicit state directories must already be mode `700`.

A proxy 404 uses the Jig landing-site visual style and links to routes in the active state directory to help local loopback debugging. Other Jig-generated proxy errors use the same error-page shell. Non-loopback clients receive a hidden route list. Use separate `JIG_PROXY_STATE_DIR` values when you do not want unrelated repos to share route listings.

Route and certificate caches include file-content signatures plus a short freshness window. Normal route and certificate writes are picked up immediately; a stale read should last no longer than about 500 ms. Dead process routes are filtered from live reads; run `scripts/jig proxy prune` when you want that cleanup persisted to the route file immediately.

`lan = true` binds the proxy to the IPv4 wildcard address `0.0.0.0` and reports the detected LAN IP address when one is available. The detected IPv4 LAN address is captured when the proxy starts and is also used by the proxy self-loop guard, so restart the proxy after changing networks before relying on the new address. Other devices still need a DNS or hosts-file entry for repo-scoped names such as `web.demo.localhost`, or they must send that hostname as the HTTP `Host` header.
LAN IP detection connects an unbound UDP socket to `8.8.8.8:80` to select the outbound interface without sending application data to that address.

On the local machine, the default `.localhost` names resolve to loopback automatically. If you configure `tld = "test"`, `tld = "local"`, or `tld = "internal"`, add hosts-file, DNS, or mDNS resolution for the generated repo-scoped names before expecting browsers to resolve them. Certificates are generated for Jig route hostnames and explicitly configured additional DNS names; custom multi-label TLDs do not imply that the bare TLD itself is covered. Wildcard additional DNS names such as `*.demo.localhost` add the stripped subtree (`demo.localhost`) to the local CA name constraints, so keep them repo-scoped rather than broad.

Generated leaf certificate PEM files contain the leaf certificate. Trust-aware clients should import the Jig local CA through `scripts/jig proxy cert trust --accept-trust-scope` or configure their trust store with the generated CA certificate.

LAN mode exposes the Jig proxy, not the child app process directly. Jig still starts child apps on loopback IP literals (`HOST=127.0.0.1` or Vite `--host 127.0.0.1`) and routes LAN traffic through the proxy. Health and administrative endpoints remain loopback-only. When HTTPS and LAN mode are both enabled, Jig includes the detected IPv4 LAN address in the generated certificate names when one is available; switching networks can change that IP, so regenerate the leaf certificate if browsers report a name mismatch after moving networks. For `tld = "local"` in LAN mode, Jig does not add broad `local` or `*.local` certificate names; use repo-scoped route hostnames or explicit additional DNS names instead.

In LAN mode, Jig-owned process routes remain reachable because Jig starts and supervises their child apps on loopback IP literals. Alias routes remain loopback-client-only; local loopback clients can still use those aliases for remote tunnels or shared development services.


If a proxy is already running without HTTPS and a later command asks for HTTPS, stop and restart the proxy with HTTPS using the same `JIG_PROXY_STATE_DIR`. Use separate state directories for worktrees that need different HTTP/HTTPS listener settings.

Ports below `1024`, including `80` and `443`, usually require elevated bind privileges on Unix-like systems. Jig attempts the bind and reports the OS error with an actionable hint when it fails. On Linux, grant the installed Jig binary `cap_net_bind_service`; on macOS, use a root-owned LaunchDaemon or forward 80/443 to unprivileged Jig proxy ports.

Useful commands:

- `scripts/jig dev`
- `scripts/jig dev --app web`
- `scripts/jig dev status`
- `scripts/jig dev --replace`
- `scripts/jig dev stop`
- `scripts/jig proxy start`
- `scripts/jig proxy stop`
- `scripts/jig proxy list`
- `scripts/jig proxy prune`
- `scripts/jig proxy run web -- vite`
- `scripts/jig proxy alias api --port 8080`
- `scripts/jig proxy cert generate`
- `scripts/jig proxy cert status`
- `scripts/jig proxy cert trust --accept-trust-scope`
- `scripts/jig proxy cert untrust --accept-trust-scope`
- `scripts/jig proxy service install --accept-service-scope`
- `scripts/jig proxy service status`
- `scripts/jig proxy service uninstall`

`scripts/jig proxy service install --accept-service-scope` writes the user service file and attempts to load/start it with the platform service manager after you acknowledge that scope. Jig invokes the service manager from fixed system tool locations rather than the invoking shell's `PATH`. It refuses to overwrite an existing service file with different contents; uninstall or remove that file before installing a changed definition. `scripts/jig proxy service uninstall` attempts to stop/unload it before removing the file and keeps the file in place when unloading fails.

Service installation records the canonical path of the currently running `jig` executable. Verify the launcher path before installing or reinstalling the service, especially after replacing a local development binary.

`scripts/jig proxy cert trust --accept-trust-scope` installs a local CA through the platform trust tooling after acknowledging the trust scope. On macOS Jig targets the login keychain. On Linux Jig uses p11-kit `trust anchor` when available and then refreshes CA bundles with the distribution command it finds in fixed system tool directories; depending on distribution policy, those Linux steps may use a user trust store or require privileges. The CA is name-constrained to configured Jig development DNS names plus loopback and detected IPv4 LAN addresses, but `ca-key.pem` is still sensitive local TLS material. Keep it private, exclude the proxy state directory from backup or sync tools that may copy private keys outside local filesystem permissions, use a dedicated `JIG_PROXY_STATE_DIR` when isolation matters, and do not trust the CA unless HTTPS proxying needs browser trust.

`scripts/jig proxy cert untrust --accept-trust-scope` removes matching Jig Dev Proxy Local CA certificates by fingerprint where the platform tooling can manage them after you acknowledge platform trust-store mutation. On macOS this deletes matching certificates from the login keychain rather than only toggling trust settings; manually remove copies installed in other keychains, and run the command again if it reports that more matching certificates may remain. On Linux, Jig removes the current CA's exact p11-kit trust anchor when `trust list --filter=ca-anchors` reports it and refreshes the system CA bundle with `update-ca-trust extract` or `update-ca-certificates` when one is available; distribution policy determines whether those steps need privileges.

If the local CA key may be compromised, run `scripts/jig proxy cert untrust --accept-trust-scope` before `scripts/jig proxy cert generate --force`, then trust the regenerated CA only if needed. On macOS, `generate --force` refuses to replace a currently trusted Jig CA by fingerprint so an old trusted root is not orphaned. On Linux, Jig also checks p11-kit's trusted CA anchor list for a Jig Dev Proxy CA label when `trust` is available. On other platforms, Jig records successful Jig-managed trust operations in the state directory and refuses forced replacement while that marker still matches the current CA.

## Vault Runtime

Quick start:

```sh
scripts/jig vault init
scripts/jig vault migrate --to 2
scripts/jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt
printf '%s' 'local' | scripts/jig vault field set jig://Production/MODE --text --value-stdin
scripts/jig vault exec --env-file .env.jig -- command
scripts/jig vault audit verify
```

`jig init` and `jig adopt --write` initialize a repo-scoped local vault by default. Pass `--no-vault` to skip that local setup. `jig adopt` without `--write` remains a side-effect-free preview and does not create vault state.

At a glance:

- Terminal commands prompt for the vault passphrase; guided `jig init` asks its project-shape questions first. Non-interactive commands use `JIG_VAULT_PASSPHRASE`, and init errors point to `--no-vault` when local vault setup should be skipped.
- `jig init --no-input` and `jig adopt --write --no-input` never prompt for the vault passphrase; export `JIG_VAULT_PASSPHRASE` or pass `--no-vault`.
- `--defaults` is also treated as automation intent for vault setup: when vault setup is enabled and no passphrase environment variable is present, Jig captures the new vault passphrase before rendering so it can initialize the vault after repo files are written.
- Generated repos default to a repo scope declared in `[vault]`; `--global` is an explicit logical escape hatch and is rejected unless `[vault].allow_global = true`.
- `--home` is an explicit physical vault-home override for diagnostics and tests; it bypasses repo scoping and `[vault].allow_global`.
- Canonical references are `jig://ITEM/FIELD`; the selected vault supplies the project context.
- Concealed and text fields are both encrypted. Only concealed fields are output-redaction needles.
- `vault tui` is the keyboard-first management plane for one fixed scope; it keeps ordinary frames metadata-only and auto-locks after five minutes without terminal input.
- `vault read` and `vault inject` are controlled exact-byte reveal paths and never produce JSON values.
- `vault exec` is a transparent streaming developer wrapper; the compatible `vault run` remains a constrained, buffered broker.
- `vault secret` remains compatible vocabulary for concealed fields.
- On Unix, `vault run --file VAR=SECRET` writes a secret to a private `0600` temporary file and passes the path through `VAR`; non-Unix platforms reject `--file`.
- `vault run` returns redacted JSON and mirrors the child process exit status, but output is buffered before display.
- Vault reduces accidental secret exposure; it is not a child-process sandbox.

`scripts/jig vault ...` manages machine-local encrypted secret state. Vault state is runtime-owned and deliberately lives outside `.agent/state`. Generated repos store non-secret scope metadata in `.jig.toml`:

```toml
[vault]
scope = "repo"
scope_id = "01J..."
allow_global = false
```

When a command runs inside a repo with `scope = "repo"`, Jig resolves fields under `~/.jig/vault/scopes/` by default. The physical scope directory is derived from the canonical local repo root plus the non-secret `scope_id`, rather than from `scope_id` alone, so copying `.jig.toml` to another repo cannot select the first repo's vault namespace. Moving or renaming a repo changes that trusted local namespace; encrypted backup and absent-home restore are the supported way to move the vault intentionally. If `JIG_VAULT_HOME` is set, it is treated as the local vault base for repo scopes, so the scoped home is below `$JIG_VAULT_HOME/scopes/`. Repos without `[vault]` keep legacy user-level behavior and resolve the physical vault home directly from `--home`, `JIG_VAULT_HOME`, or `~/.jig/vault`. Relative `--home` and `JIG_VAULT_HOME` values are supported and resolve from Jig's process working directory; a generated `scripts/jig` launcher runs Jig from its owning repository root, while a direct `jig` invocation uses the caller's current directory. Leading parent traversal such as `../recovery-vault` remains valid when its prefix exists, but restore rejects `..` reached only after a missing component—such as `missing/../recovery-vault`—before creating anything, matching operating-system pathname resolution rather than silently rewriting the path. Re-adopting a legacy repo adds a new repo scope and reports that migration in the command notes; existing global-vault fields are not copied automatically. The `--home` flag is an explicit physical-home override for diagnostics, recovery, and tests and bypasses repo scoping plus `[vault].allow_global`; use it only when you intentionally want a specific vault directory. The `--global` flag selects the user-level global vault, but scoped repos reject it unless `[vault].allow_global = true`.

### References, fields, and format compatibility

Every canonical reference has exactly two project-local segments: `jig://ITEM/FIELD`. Each segment is an ASCII identifier containing letters, digits, `_`, `-`, or `.`, subject to the CLI length limits. A reference never names a repository and never changes the selected scope. For example, `jig://Production/RESTIC_PASSWORD` means `Production/RESTIC_PASSWORD` in the vault chosen by the current repo, `--global`, or `--home`; `jig://Project/Production/RESTIC_PASSWORD` is invalid.

Version 2 stores a handling kind with every field. `concealed` is the default and contributes its raw and supported encoded forms to output redaction. `text`, selected with `field set --text`, is still encrypted with the same vault state but is not treated as a masking needle. Use text for contextual values that may legitimately appear in output; the distinction never means plaintext-at-rest.

| Vault or command | Current Jig behavior | Compatibility consequence |
| --- | --- | --- |
| Version 1 vault | Reads, lists, audits, reveals, injects, runs, and executes; every value is treated as concealed | Run `vault migrate --to 2` before field mutation, import, passphrase rotation, or backup |
| Version 2 vault | Supports concealed/text fields and all current lifecycle operations | Older Jig rejects version 2 rather than silently discarding field kinds |
| `vault secret` on version 2 | Compatible API and CLI vocabulary over concealed fields | Existing scripts can migrate independently of field-oriented callers |
| `vault run` | Retains the constrained cleaned-environment broker | Existing agent-oriented execution semantics do not change |
| `vault exec` | Transparent inherited-environment, inherited-stdin, streaming execution | Use it for ordinary developer commands, not when `run`'s constraints are required |

Migration is explicit, one-way, atomic, and idempotent when the vault is already version 2. There is no downgrade command. To use a pre-migration vault with an older binary, restore a pre-migration encrypted backup into a different explicit home.

If automatic vault initialization fails after `jig init` or `jig adopt --write` has rendered repo files, Jig leaves the repo files in place and reports the vault error with a recovery command. Fix the reported vault or config issue, then run `jig vault init` from the repo root.

Jig creates or tightens vault directories to owner-only permissions, so do not point `JIG_VAULT_HOME` at a shared directory. The vault derives its passphrase wrapping key with Argon2id using 128 MiB memory, 3 iterations, 4 lanes, and a 32-byte output. New vault passphrases must be at least 12 bytes; Jig enforces a floor, not a password-strength meter, so operators are still responsible for choosing high-entropy passphrases. Passphrases are byte-exact after UTF-8 capture: Jig does not trim whitespace, strip trailing newlines, normalize Unicode, or otherwise rewrite prompt or environment input. Terminal use prompts for hidden passphrase entry. Non-interactive use reads the unlock passphrase from `JIG_VAULT_PASSPHRASE`; `vault passphrase change` requires both that variable and `JIG_VAULT_NEW_PASSPHRASE`, with no environment/prompt mixing. Successful capture removes both reserved variables before vault children can start. This is best-effort process hygiene and does not prove the OS or C runtime overwrote every previous environment backing byte. Command-line passphrases are intentionally unsupported because they leak through shell history and process listings.

Keep `JIG_VAULT_PASSPHRASE` exported or re-export it for every non-interactive command that unlocks the vault, including list, read, inject, exec, run, audit, import, rotation, and backup/restore. `vault tui` may consume it once as its initial interactive credential; successful capture removes it before the TUI worker starts. `vault status` is the only vault command that does not require the passphrase. `vault status` is a non-creating probe: it refuses a symlinked vault home, but it does not create missing directories or tighten permissions. Its `exists` and `vault_file_exists` fields report whether `vault.json` exists, not whether the vault home directory exists.

`scripts/jig vault passphrase change` fully reseals a version 2 vault with a new salt, current KDF parameters, and fresh nonces while retaining the vault ID, data-encryption key, fields, and timestamps. Interactive use asks for current, new, and confirmation; automation sets both reserved environment variables. If a crash makes the reported result uncertain, try the new passphrase first and then the old before attempting another mutation; atomic replacement leaves one complete envelope authoritative.

### Keyboard-first Vault manager

`scripts/jig vault tui` opens one resolved repo, global, or explicit-home scope for the lifetime of the process. It requires terminal stdin/stdout, rejects `--json`, and keeps the selected scope visible. The browser shows canonical items/fields and unrepresentable legacy entries using authenticated metadata only; selecting or filtering an entry never decrypts its value. Version 1 vaults remain browsable and revealable but are read-only until the deliberate `m` migration. Version 2 supports create/replace, kind changes, field and item renames, typed-confirmation deletion, legacy conversion, 1Password import preview/commit, private backup, passphrase rotation, verified Activity/audit views, and Linux absent-home restore from the Tools palette. Private-output actions (field export, 1Password destination installation, and backup creation) are available only on Unix; Peek and passphrase rotation remain portable. Import previews show authenticated prior-to-proposed field kinds. Ordinary commits require exact `IMPORT`; a batch that replaces any concealed field with text requires exact `IMPORT TEXT` because the replacement will no longer be an output-redaction needle.

`x` exports the selected canonical field through the same hardened private-file sink as `vault read --out-file`; an existing regular file requires explicit overwrite. Legacy values must first be converted because the core has no legacy plaintext accessor. `p` is an explicit controlled Peek: after a warning and exact `PEEK` confirmation, Ratatui drawing stops and the audited reveal writes directly to a terminal-safe escaping sink. It displays at most the first 4 KiB of source bytes, escapes controls, invalid UTF-8, backslashes, and directional-format characters, waits for one key or ten seconds, clears the alternate screen, and then redraws metadata. Printable bytes in that window are intentionally disclosed; terminal scrollback, tmux/screen, SSH infrastructure, and screen recording remain external sinks that Jig cannot revoke.

The TUI retains only a process-local credential, reopens and authenticates current state for every operation, permits one owned operation at a time, and joins in-flight work before lock or terminal restoration. `L`, five minutes without keyboard/paste input, authentication loss, audit failure, signal shutdown, and ordinary exit drop the credential and pending protected forms. There is no clipboard/OSC52 integration, unlock daemon, remote synchronization, or cross-scope navigation. `vault exec`, `vault run`, and `vault inject` remain separate CLI workflows because they own child-process and raw streaming semantics.

### Reveal, injection, transparent execution, and import

`scripts/jig vault read jig://ITEM/FIELD` writes the field's exact bytes without adding a newline. Interactive terminal output is refused unless `--reveal` is explicit. `--out-file` uses a private, no-symlink, atomic destination and refuses replacement unless `--overwrite` is present. The destination must be outside the selected vault home and must not be a hard-link alias of that vault's `vault.json` or `audit.jsonl`; these checks are repeated immediately before installation. `vault inject --in PATH` applies the same sink rules while replacing only `{{ jig://ITEM/FIELD }}` placeholders; `--in -` is the explicit stdin spelling. Template input and rendered output are each bounded at 16 MiB. Both raw commands reject global `--json`, keeping revealed bytes out of structured responses and receipts. Exact stdout is portable; private file sinks currently require Unix filesystem guarantees.

`scripts/jig vault exec --env-file FILE -- COMMAND...` parses a bounded restricted UTF-8 dotenv file before passphrase capture. The source must be a non-symlink regular file; FIFOs, devices, directories, and other special files are rejected without waiting for a producer. It accepts blank/full-comment lines and exact `NAME=VALUE` assignments with a documented small quote/escape grammar, rejects duplicates, interpolation, substitution, NUL, malformed references, and assignments to either reserved passphrase variable, and rejects `--env-file -` so stdin remains inherited by the child. A decoded whole value of `jig://ITEM/FIELD` binds that field; any other accepted value is a literal. Jig invokes the command directly without a shell, inherits the ordinary environment and stdin, applies the file's assignments, removes the passphrase variables, independently streams/redacts stdout and stderr, imposes no timeout or output cap, and mirrors nonzero or signal status without appending a second Jig error. Only concealed referenced fields become redaction needles. `exec` is transparent process plumbing, not process-tree containment or a sandbox.

`scripts/jig vault import onepassword --env-file SOURCE --item ITEM --out-env DESTINATION` is a one-time conversion tool, not synchronization. Its source must be a non-symlink regular file and uses the same restricted dotenv grammar. A decoded whole `op://VAULT/ITEM/FIELD` or `op://VAULT/ITEM/SECTION/FIELD` value is resolved by exact direct argv `op read --no-newline REF` with null stdin and bounded output; raw `op` diagnostics are never surfaced. Resolved values become concealed fields, literals become encrypted text, and the destination contains only `NAME=jig://ITEM/NAME` assignments in stable source order. `--dry-run` validates paths and input, invokes no `op`, unlocks the version 2 vault read-only, and reports create/replace metadata without mutation. Normal import resolves every external value before one atomic vault batch. Existing fields need `--replace`; an existing destination needs `--overwrite`. If final destination installation loses a race after the vault commit, Jig explicitly reports that the import succeeded and prints a safe exact rerun using `--replace --overwrite`. Private destination installation currently requires Unix filesystem guarantees, so other platforms fail before passphrase capture or `op` execution.

The vault file is encrypted at rest with a passphrase-derived wrapping key and a random data-encryption key. Field and secret listing commands return names, kinds, lengths, and timestamps, never values. `scripts/jig vault field set REF` defaults to hidden UTF-8 terminal entry without a trailing newline; `--value-stdin` is the byte-exact automation path. Concealed values must be between 4 bytes and 1 MiB so redaction can match them safely; text values may be empty and are bounded at 1 MiB. The compatible `scripts/jig vault secret set NAME` has the same input modes and always writes a concealed field on version 2. Piped input stores bytes exactly, including a trailing newline from `echo`; use `printf` when a newline is not part of the value. Non-interactive set commands without `--value-stdin` fail instead of waiting for input.

`scripts/jig vault run --env VAR=SECRET -- <command>` resolves named secrets, starts a child process with a cleaned environment plus the requested secret variables, captures stdout and stderr, and redacts known secret forms before returning JSON. On Unix, `scripts/jig vault run --file VAR=SECRET -- <command>` writes each requested secret to a private temporary file with mode `0600`, injects the file path as `VAR`, and removes the temp directory when the brokered process exits normally through Jig; abrupt process termination such as `SIGKILL` can leave temp files behind for OS temp cleanup. Non-Unix platforms reject `--file` because Jig cannot guarantee equivalent secret-file permissions there; use `--env` or a platform-specific wrapper instead. Environment injection necessarily gives the standard library and child process an OS-owned environment copy that Jig cannot zeroize afterward. File delivery keeps the value out of the environment but still gives the child filesystem access to the secret bytes while it runs. Each captured stream is capped at 1 MiB; exceeding the cap fails the brokered run instead of buffering unbounded output. The cleaned environment preserves only a small allowlist of process basics and locale variables, not arbitrary `LC_*`, `SSH_AUTH_SOCK`, `XDG_*`, or `TZ` variables; the child uses the preserved `PATH` inherited by the `jig` process to resolve command names. `vault run --env` and `vault run --file` reject mappings that would overwrite preserved environment variables such as `PATH`, `HOME`, `TMPDIR`, or locale variables. The broker does not sandbox the child's filesystem view. Environment variable names must match `[A-Za-z_][A-Za-z0-9_]*`.

`vault run` buffers the child process' full stdout and stderr before displaying them because redaction is applied to the captured output. This keeps the constrained broker deterministic but means long-running commands do not stream output. Brokered execution has a 30-minute wall-clock deadline. On Linux and macOS, Jig starts the command as an isolated session/process-group leader and observes its exit without reaping until the owned group has been terminated. Targets without a retained process-tree identity reject brokered execution before the command starts. On macOS, a process-group `EPERM` result remains unverified until an atomic membership snapshot finds the exact exited leader as the sole member; any additional or unclassifiable member makes cleanup fail closed. Leader exit also ends remaining owned descendants, and success requires complete EOF from both captured pipes. A descendant that deliberately escapes Unix process-group ownership can make capture fail as incomplete, but `vault run` is not a sandbox and cannot revoke secrets or reliably terminate a process that intentionally escapes the supported ownership boundary; only run trusted commands and do not daemonize from a brokered invocation. Redaction can allocate intermediate output buffers that are not zeroized; it is a safety net for displayed output, not an in-memory secrecy boundary. The child is non-interactive: stdin is closed/null, so commands that prompt for input should fail or remain blocked until the run deadline instead of asking the operator. A non-zero child exit is returned in the JSON result and the Jig CLI exits with that child status code, clamped to the portable process-exit range after vault runtime values have unwound through `main`. On Unix, signal-terminated children report both `exit_signal` and shell-style `128 + signal` status.

On Linux, vault cleanup retains one absolute deadline through the full procfs scan and its live-or-empty result proof. A late iterator, fallback membership lookup, or final classification therefore fails closed instead of declaring the secret-bearing process tree clean after the budget expires.

### Encrypted backup and absent-home restore

`scripts/jig vault backup create --out FILE` packages the exact version 2 vault file and audit log inside a separate encrypted envelope protected by the vault's current passphrase. The backup has a fresh salt and nonce, exposes no plaintext field values or audit records, and remains tied to the passphrase used when it was created even after the live vault is rekeyed. Choose a private destination outside the repository and never commit the backup. Creation refuses an existing file unless `--overwrite` is explicit. Private backup creation currently requires Unix filesystem guarantees.

From a destination checkout configured with `[vault].scope = "repo"`, `scripts/jig vault backup restore --in FILE` restores into that checkout's automatically selected repo-scoped vault home. Jig creates a missing private vault-base and `scopes` parent chain while keeping the selected vault home itself absent. This is the normal machine-to-machine relocation path. Before restoring, run no destination vault command other than the non-creating `vault status`; commands that resolve a missing vault home create it and therefore make it ineligible as an absent restore target.

Without repo-scoped vault configuration, pass `--global` to choose the legacy user-level vault deliberately, or pass `--home NEW_HOME` to restore into a specific recovery or test location. Omitting both continues to select the legacy user-level vault for contract-v4 compatibility; changing that default to fail closed requires a future contract epoch.

Restore validates and decrypts a bounded regular backup, verifies its embedded vault and audit chain, appends a restore event in private sibling staging, and atomically publishes only when the complete target home is absent. It never restores over an existing directory, even an empty one, and never reads backup bytes from stdin. Missing parents are created one component at a time, explicitly restricted and verified as owner-only independently of the process `umask`, after existing ancestors are checked for symlinks. Each new directory and its containing directory entry are synced before restore proceeds, so a successful fresh-machine restore does not rely on unsynced parent creation. A group- or other-writable creation boundary is rejected unless it has the sticky bit and is owned by the current user or root; sticky protection prevents other writers from renaming the newly owned entry, while excluding an untrusted directory owner that retains rename authority. The completed parent chain is then rechecked before staging or installation. Parent preparation intentionally occurs before passphrase capture, and verified empty parents remain after a cancelled prompt, wrong passphrase, authenticated archive failure, or later restore failure; the selected vault home stays absent, retry remains safe, and automatic rollback cannot race another process that started using the prepared directories. Remove such parents manually only after confirming they are empty and not in use. Restore currently requires Linux's atomic absent-directory installation guarantee; other targets fail before passphrase capture or target creation. Verify the restored vault with `scripts/jig vault audit verify` before use.

### Local audit limits

Vault operations append local HMAC-chained audit events. Audit details record field names in cleartext, so names should be operational labels rather than sensitive values; path-like names using `/` or `.` appear in the JSONL audit log. `scripts/jig vault audit verify` recomputes the chain and fails if existing event contents or links were edited. This is local tamper-evidence, not remote or independent evidence: someone who can roll back or delete both local vault files can also remove later records, and someone with the vault and passphrase controls the audit key. Detecting deletion, truncation, or rollback requires an externally retained checkpoint or backup. The audit log has no rotation or archival mechanism, so each operation increases future verification work and very large logs will make append and verify slower.

Vault mutations append audit intent before saving the new state, so a crash can leave audit leading state but should not leave state leading audit. A hard crash during audit append can leave a torn final line; verification reports `torn_tail_bytes`, and the next append truncates only that unterminated tail before adding a new event with `truncated_torn_tail_bytes` in its audit details. Vault mutations serialize on one local advisory lock with a 30-second acquisition ceiling. Unlocking derives an Argon2id key each time, so tight loops of vault commands intentionally pay that local KDF cost instead of caching unlocked key material. There is no unlock rate limiter or lockout counter; the KDF is the local offline-guessing cost control, not an online account-protection system. `vault run` and `vault exec` release the vault lock while the child runs, so concurrent operations may interleave start and finish/failure events; correlate them by run ID rather than adjacency. If Jig is killed after a start event, that event can remain unmatched. Parent directories are fsynced after atomic writes and audit creation. Vault home canonicalization is intentional: Jig hardens the resulting vault tree and the first existing creation ancestor, not every ancestor above a user-selected path. If `vault init` writes the first audit event but fails before writing the vault file, the next init fails closed on stale `audit.jsonl`; inspect the vault home and remove stale init artifacts before retrying. If `vault init` reports rollback cleanup failures, inspect or remove the vault home before retrying.

This is a blast-radius reducer, not a sandbox. Once a child process receives a secret in its environment, that process can use or disclose it. The redactor is a backup control for accidental output, not a guarantee against malicious transformations or side channels.

Useful commands:

- `scripts/jig vault init`
- `scripts/jig vault tui`
- `scripts/jig vault status`
- `scripts/jig vault migrate --to 2`
- `scripts/jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt`
- `printf '%s' 'local' | scripts/jig vault field set jig://Production/MODE --text --value-stdin`
- `scripts/jig vault field list jig://Production`
- `scripts/jig vault read jig://Production/RESTIC_PASSWORD | command`
- `scripts/jig vault inject --in config.template --out-file config`
- `scripts/jig vault exec --env-file .env.jig -- command`
- `scripts/jig vault import onepassword --env-file .env.op --item Production --out-env .env.jig`
- `scripts/jig vault passphrase change`
- `scripts/jig vault backup create --out ../ExampleProject-vault.backup`
- `scripts/jig vault backup restore --in ../ExampleProject-vault.backup` (from a repo-scoped destination checkout)
- `scripts/jig vault backup restore --in ../ExampleProject-vault.backup --home restored-vault` (explicit recovery location)
- `scripts/jig vault audit verify`
- `scripts/jig vault secret set api_token --value-prompt` (compatible concealed-field vocabulary)
- `scripts/jig vault run --env TOKEN=api_token -- sh -c 'printf "%s\n" "$TOKEN"'`
- `scripts/jig vault run --file TOKEN_FILE=api_token -- sh -c 'cat "$TOKEN_FILE"'`

## Generated Contract

The compatibility policy for generated CLI commands, MCP tools, and `.agent/jig-contract.json` is defined in [Public Contract](./public-contract.md).

`scripts/jig` is the stable command surface for generated repos. It exposes configured project checks as:

- `scripts/jig bootstrap`
- `scripts/jig check fmt`
- `scripts/jig check clippy`
- `scripts/jig check test`
- `scripts/jig check test-locked`
- `scripts/jig check contract`

When `sqlx_enabled` is `true`, it also exposes:

- `scripts/jig check sqlx`

When Go/PostgreSQL migrations are configured, or when SQLx uses `rust_migration_layout = "flat_migrations"`, it also exposes:

- `scripts/jig migration add NAME`

When both `sqlx_enabled` and `schema_dump_enabled` are `true`, it also exposes:

- `scripts/jig check schema`
- `scripts/jig sqlx schema dump`

Contract 6 implements `migration add` as a native action; custom command
runners deliberately do not receive the migration-name argument. The legacy
`migration_add_command` extension remains available only through contract 5.

`scripts/jig check schema` is a read-only freshness gate. It requires
`SCHEMA_DOCS_DIR` to be clean and reruns the owning component's `schema-dump`
action in a disposable snapshot of the current repository, resolving that
action's command key through `[commands]` and preserving its working directory
and environment. Pre-v6 contracts use the legacy `schema_dump_command` binding.
The check reports any drift without letting the generator write to the live
worktree. The snapshot includes current tracked and
staged content, non-ignored untracked files, ignored `.env`/`.env.*` files under
directories that are not themselves ignored, and
the working trees of initialized local submodules; unrelated special untracked
files are ignored, while untracked symlinks are recreated without following
them. In a repository with commits, Git materializes the tracked/staged snapshot
as an unreferenced, garbage-collectable object without moving refs or changing
the live index or worktree; the explicit file overlay supplies untracked inputs.
Use `scripts/jig sqlx schema dump` to apply the generated update.
`SCHEMA_DOCS_DIR` defaults to `docs/schema`, must remain repository-relative,
and is included in the generated default verification profile.

The legacy `scripts/jig migration-add NAME`, `scripts/jig sqlx migration add NAME`, and `scripts/jig schema-dump` paths remain accepted as compatibility shims. New migration automation should use `scripts/jig migration add NAME`; SQLx schema commands remain under `scripts/jig sqlx schema ...`. Every migration-add path rejects `versioned_artifacts` repositories before creating files.

Generated repos also get these runtime-owned files:

- `.mcp.json`
- `.agent/jig-contract.json`
- `scripts/jig`
- `scripts/install-jig.sh`

The generated `scripts/jig` launcher embeds the contract epoch it was rendered for and executes only a binary whose private compatibility probe accepts that epoch plus the requested `default`, `runtime`, or `mcp` profile. Ordinary commands then require that embedded epoch to equal `.agent/jig-contract.json` before the selected runtime strictly validates the complete repository contract; `doctor` and repair commands use the embedded epoch only for runtime selection so a malformed or missing manifest can reach its own diagnostic. Repo-local cache directories are keyed by contract epoch and profile rather than product release, while a source stamp inside each cache binds remote installs to the configured source and immutable `_commit` (or the legacy source tag for v2/v3) and binds local installs to their canonical source identity and relevant source-tree contents, including non-Git directories. Advancing `_commit`, editing local source, or switching its path within the same contract epoch invalidates the old stamp and refreshes the runtime; help and MCP resolution apply the same stamp check without installing during MCP startup. Generated launchers are never accepted as runtime binaries through the explicit `JIG_INSTALL_ALLOW_PATH_BINARY=1` escape hatch. On first use the launcher may install a compatible runtime from the recorded template source and then exposes the configured command contract as:

- CLI commands such as `scripts/jig check fmt`
- bounded MCP tools such as `jig.plan_run` and `jig.execute_run` in contract v6; contracts v2 through v5 retain direct tools such as `jig.fmt_check`

For help requests, the launcher first looks for an existing matching repo-local
binary so `scripts/jig --help` and nested `--help` calls stay fast after the
first install. On a cold checkout it prints an explicit first-run install
message before preparing the runtime needed to render command help.

It also provides runtime-owned append-only memory under `.agent/state/*.jsonl` through the structured work namespace:

- `scripts/jig doctor`
- `scripts/jig doctor --json`
- `scripts/jig agent doctor`
- `scripts/jig agent doctor --json`
- `scripts/jig agent bootstrap`
- `scripts/jig codex homes`
- `scripts/jig codex homes --usage --json`
- `scripts/jig codex launch [HOME]`
- `scripts/jig codex launch HOME --dry-run --json -- [CODEX_ARGS...]`
- `scripts/jig codex resume SESSION_ID [--home HOME] -- [CODEX_ARGS...]`
- `scripts/jig codex resume SESSION_ID [--home HOME] --dry-run --json -- [CODEX_ARGS...]`
- `scripts/jig work start --title ...`
- `scripts/jig work start --title ... --print-plan-id`
- `scripts/jig work append --plan-id ... --body "Progress update"`
- `scripts/jig work check --plan-id ...`
- `scripts/jig work check --plan-id ... --json`
- `scripts/jig work gates --plan-id ...`
- `scripts/jig work gates --plan-id ... --json`
- `scripts/jig work evidence`
- `scripts/jig work evidence --plan-id ...`
- `scripts/jig work review --plan-id ...`
- `scripts/jig work review --plan-id ... --json`
- `scripts/jig work refine --plan-id ...`
- `scripts/jig work refine --plan-id ... --json`
- `scripts/jig work decide --plan-id ...`
- `scripts/jig work receipts --plan-id ...`
- `scripts/jig work receipts --plan-id ... --json`
- `scripts/jig work status`
- `scripts/jig work status --json`
- `scripts/jig work finish --plan-id ...`
- `scripts/jig state summary`
- `scripts/jig state diagnose`
- `scripts/jig state diagnose --deep`
- `scripts/jig state compact sessions --dry-run`
- `scripts/jig state compact sessions`
- `scripts/jig state restore --backup <backup-directory-or-manifest>`
- `scripts/jig state export receipts --before YYYY-MM-DD --output <file.jsonl.gz>`
- `scripts/jig state archive --before YYYY-MM-DD --dry-run`
- `scripts/jig state archive --before YYYY-MM-DD`

`work finish` closes the plan with `--resolution`. If an active session is also open, it closes that session with `--outcome`; when `--outcome` is omitted, the session outcome falls back to `--resolution`. Gate evaluation and plan closure hold a shared checkout lease as one decision window, so an effectful repository action cannot invalidate accepted evidence immediately before the close commit point.

Contract tools and work checks intentionally append receipts under `.agent/state/`.
Read-only inspection commands such as `work status` and `work gates` do not add
new receipts. For one-off contract command runs that should not record evidence,
pass `--no-receipt`; `--no-receipt` conflicts with `--plan-id` because
plan-linked checks must leave evidence for `work finish` gate enforcement. When
receipt recording is skipped, command JSON still includes `"receipt_id": null`.
Timeout, process-await, cleanup, and output-capture failures after a configured
command starts append a failed child receipt. In-flight cancellation appends a
child receipt with supervised evidence status `cancelled`; cancellation before
spawn appends no child receipt because no command ran. A cancelled or failing
`work check` batch records that child receipt ID in `args.receipt_ids` and keeps
the supervision diagnostic in its failed stderr preview, so gate evaluation
does not confuse an interrupted check with missing evidence.

Use `scripts/jig state diagnose` for a read-only size and integrity report.
`--deep` additionally analyzes recursive session summaries, projected
compaction savings, receipt payload categories, and archive recommendations
for oversized receipt or run journals. The report also includes
local disk usage from maintenance backups under
`.agent/.cache/state-backups/` and compressed receipt archives under
`.agent/.cache/state-archives/`. Repair legacy recursive summaries with
`state compact sessions`: run `--dry-run` first, then apply the rewrite. Apply
mode validates the replacement and first stores an exact gzip backup plus
checksum manifest under ignored
`.agent/.cache/state-backups/<id>/`. Pass either that directory or its
`manifest.json` to `state restore --backup ...` to verify and restore the exact
pre-compaction stream.

Use `scripts/jig state archive --before ...` when `receipts.jsonl` grows too
large, and add `--include-runs` when `runs.jsonl` also needs retention;
shared-repository Codex task preflight refuses an active receipt journal larger
than 64 MiB, so archive before it reaches that bound.
`--before YYYY-MM-DD` is interpreted as a UTC cutoff date. Archiving keeps
evidence and completed run history linked to open plans active, never selects
nonterminal runs, and refuses run-journal maintenance while any known run is
nonterminal so live byte cursors remain valid. It writes eligible receipt records and opt-in whole run-event
groups as separate gzip JSONL artifacts under ignored
`.agent/.cache/state-archives/`, and only then rewrites each active stream.
Before each rewrite, Jig also stores a complete stream backup with a checksum
manifest under `.agent/.cache/state-backups/`; `state restore --backup ...` can
therefore recover that stream's exact original physical order. A changing
run-journal restore refuses while any current run is nonterminal or any current
run worker still holds its lease; an identical checksum no-op remains safe.
Existing legacy files under
`.agent/state/archive/` are left untouched and reported by diagnostics. For a
non-mutating copy, use `state export receipts --before ... --output ...`; export
preserves the selected raw JSONL records and refuses to overwrite its
caller-selected destination.

Maintenance backups and archives under `.agent/.cache/` are local, ignored
recovery aids rather than durable or off-machine backups. Copy an artifact to
durable storage before relying on it for long-term recovery. Human command
output reports the relevant recovery path, compressed size, checksum, and
whether active state changed.

New receipt Git metadata excludes `.agent/**`. `changed_paths` is a sorted
preview capped at 100 entries; `changed_path_count`,
`changed_paths_truncated`, and `changed_paths_digest` describe the full set.
Successful stdout and stderr previews use a 512-byte truncation threshold, while
failed previews retain the existing 4,000-byte diagnostic threshold. These
limits constrain future growth without weakening worktree fingerprints or gate
relationships.

These commands repair or reduce the current working-tree files only. They never
rewrite Git history, so blobs already reachable from commits remain until a
separate, deliberately coordinated Git-history cleanup.

Before an applying compaction, archive rewrite, or restore, stop any Jig process
that was launched with an older runtime which wrote through a pre-opened state
file handle. Current runtimes coordinate through the repository state lock, but
a writer already queued on the legacy file inode cannot follow an atomic rename.
Dry-run diagnosis and compaction do not need this writer cutover.

Treat `.agent/.cache/state-backups/` as short-lived rollback storage: keep the
latest pre-rewrite backup until the repaired state has been verified, copy any
artifact needed for long-term recovery outside the ignored cache, then remove
obsolete cache artifacts. Keep selected receipt archives only for as long as
their historical evidence is useful. `state diagnose` reports backup and archive
bytes separately so this local cache does not become a second unbounded state
store.

For local runtime development, set `JIG_DEV_BIN` to an already-built `jig` binary. The installer resolves that explicit binary to an absolute path and requires its repository/profile compatibility probe to pass. A missing, non-executable, or incompatible `JIG_DEV_BIN` is a hard error rather than a fallback to the cache. The `jig-sh` source checkout does not implicitly trust `target/debug/jig`: set `JIG_DEV_BIN=target/debug/jig` after building when that exact artifact should be authoritative. Without the override, source-stamped caches stay tied to the current checkout so a compatible release cache cannot hide local runtime changes. Avoid rebuilding that binary while a long-running `JIG_DEV_BIN` process, such as `jig proxy start --foreground`, is still active.

## SQLx Metadata Directory

This section applies only when `sqlx_enabled` is `true`.

Managed Rust workflows watch both `rust_migration_dir` and `rust_sqlx_metadata_dir` and expose the metadata directory as an absolute `SQLX_OFFLINE_DIR` for offline compilation. The upstream SQLx CLI 0.9 still checks `.sqlx` for `cargo sqlx prepare --check`. Because the Rust + React scaffold pins SQLx 0.9, its database variants require `.sqlx` and reject a custom metadata directory before creating the destination.

Adopted repositories may keep a different committed metadata directory when their SQLx version and build setup support it. Supply a project-owned `sqlx_check_command` that actually checks that directory; Jig preserves explicit command overrides rather than pretending upstream `prepare --check` supports a custom output path.

## Template Source

For portable shared repos, set:

```toml
template_source_url = "git@github.com:your-org/jig-sh.git"
```

When `template_source_url` is set, the renderer writes it into `_src_path` for portable update and install behavior. If it is blank, local template renders keep the local source path. At install time a usable recorded `_src_path` is authoritative, including a remote URL; `template_source_url` is only a fallback when that recorded source cannot identify a usable local or remote source. This keeps the source provenance that rendered the repository ahead of a later fallback setting.
