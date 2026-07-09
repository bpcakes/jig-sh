# `.jig.toml` Configuration

This file is the supported configuration surface for downstream repos and must be committed alongside the generated template output.

`.jig.toml` is also the native renderer answers file.

After changing values in `.jig.toml`, re-render with:

```sh
jig update --recopy
```

To move onto a newer version of the template while keeping the stored answers, run:

```sh
jig update
```

For remote template sources, plain `jig update` advances to the remote default branch unless `--vcs-ref` is provided. `jig update --recopy` re-renders from the stored `_commit`.

The file contains both public settings and the private `_src_path` / `_commit` fields that `jig update` uses to resolve future renders. Repos rendered from local committed template checkouts may also store `_template_mode` and `_template_local_path`.

`jig update` refuses to overwrite or remove changed template-managed files unless `--force` is passed.

Root `AGENTS.md` is block-managed instead of file-managed. If the file already exists, `jig adopt` and `jig update` preserve user-authored content and insert or replace only the section between `<!-- BEGIN JIG MANAGED BLOCK -->` and `<!-- END JIG MANAGED BLOCK -->`. Edits inside that managed block are template-owned and may be replaced without `--force`; keep repo-specific guidance outside the markers.

Release builds of `jig init` and `jig adopt` default to the official `jig-sh` template source at `https://github.com/bpcakes/jig-sh.git`, pinned to the release tag for the installed Jig version. Pass `--template` when using a local checkout, fork, or private template. Unreleased or dirty local builds use templates embedded in the binary when `--template` is omitted, or an explicit `--vcs-ref` when you intentionally want remote template code. Embedded renders store `_src_path = "embedded:jig-sh"`; generated launchers reuse a same-version `jig` on `PATH` and require `JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1` before falling back to `template_source_url` or the official release-tag install path.

For local git template checkouts, `jig init` / `jig adopt` use a committed source:

- `--template-mode committed`: explicitly use the clean local `HEAD`
- omit `--template-mode`: use the same committed local-template behavior

## Required Keys

- `repo_name`: display name used in generated docs. During adoption, repo names inferred from Git remotes preserve dots such as `my.app`, while directory-name fallbacks keep the existing dash-sanitized form.
- `default_branch`: branch name used for base-ref comparisons
- `ci_github_runner`: `runs-on` value for GitHub Actions jobs
- `jig_version`: exact runtime version expected by generated repos
- `work.gates`: required work evidence gates evaluated before `scripts/jig work finish`
- `agent_tooling`: agent-client tooling expected for this repository, including Jig Codex skills
- `template_source_url`: optional canonical template source URL for portable recopy/update
- `sqlx_enabled`: whether to generate SQLx and migration-specific contract pieces
- `rust_crate_roots`: list of directories whose direct child directories are considered crates

When `sqlx_enabled` is `true`, these additional keys are required:

- `rust_migration_dir`: SQL migration directory
- `rust_sqlx_metadata_dir`: committed SQLx metadata directory

## Optional Keys

- `schema_dump_enabled`: when `true` and `sqlx_enabled` is also `true`, the template renders schema dump and schema freshness commands; when SQLx is disabled, this is rendered as `false`. New init/adopt answers reject explicitly setting this to `true` while SQLx is disabled; `jig update --recopy` normalizes legacy SQLx-disabled configs back to `false`.
- `schema_dump_command`: command behind `scripts/jig schema-dump` when `sqlx_enabled` and `schema_dump_enabled` are both `true`
- `sqlx_check_command`: command behind `scripts/jig check sqlx` when `sqlx_enabled` is `true`
- `bootstrap_command`: implementation behind `scripts/jig bootstrap`; the generated default runs `cargo fetch` only when a root `Cargo.toml` exists, otherwise exits 0 with a stdout note, so set this explicitly when bootstrap must install web dependencies, run project-specific setup, or enter a nested Rust project. If a root `Cargo.toml` exists, Cargo errors are surfaced instead of skipped.
- `dev_command`: legacy project-owned dev command preserved only for older renders; `scripts/jig dev` uses `[dev]` and `[[dev.apps]]`
- `rust_fmt_check_command`: implementation behind `scripts/jig check fmt`; the generated default exits 0 with a stdout note when no root `Cargo.toml` exists, and otherwise surfaces Cargo errors
- `rust_clippy_command`: implementation behind `scripts/jig check clippy`; the generated default exits 0 with a stdout note when no root `Cargo.toml` exists, and otherwise surfaces Cargo errors
- `rust_test_command`: implementation behind `scripts/jig check test`; the generated default exits 0 with a stdout note when no root `Cargo.toml` exists, and otherwise surfaces Cargo errors
- `rust_test_locked_command`: implementation behind `scripts/jig check test-locked`; the generated default exits 0 with a stdout note when no root `Cargo.toml` exists, and otherwise surfaces Cargo errors
- `web_package_manager`: currently `bun`
- `frontend_apps`: list of app definitions. A frontend app may use `dir = "."` when the app lives at the repository root.
- `dev`: Jig-native local development proxy settings and app definitions

The generated no-root-`Cargo.toml` Cargo defaults print a stable stdout prefix that `work check` recognizes as an intentional harness skip. Reworded custom commands still run normally, but they will be summarized as ordinary command output instead of `passed (all skipped)`. Custom commands should not print the exact generated prefix unless they intentionally want to opt into that skip rendering.

Top-level `*_command` values are committed repo configuration and run through non-login `bash -c` from the repo root with the user's normal process environment. Treat changes to these keys like changes to project-owned shell scripts. Jig-owned checks such as `scripts/jig check contract`, `scripts/jig migration-add NAME`, `scripts/jig check schema`, and repo policy checks run natively inside the binary.

Contracts that declare `"kind": "native"` tools require the repo's pinned `scripts/jig` runtime version. Do not run an older cached `jig` binary against a repo after updating its `.agent/jig-contract.json`; use the launcher so the `jig_version` pin is enforced.

`jig adopt --json` includes a `detection_report` object that records inferred values before rendering. It contains `summary`, `scope`, `repo_name`, `default_branch`, `rust_crate_roots`, `sqlx_enabled`, `rust_migration_dir`, `rust_migration_dirs`, `rust_sqlx_metadata_dir`, `web_package_manager`, `frontend_apps`, `ci_github_runner`, `signals`, and `warnings`. Adopt previews by default with `render_mode = "preview"`; pass `--write` to apply the rendered managed files with `render_mode = "copy"`. Package-manager lockfiles are reported and applied only when a frontend app is configured or inferred. Scan warnings include up to 19 concrete entries plus an omission notice when more were found. `rust_migration_dirs` is informational; only `rust_migration_dir` is applied. When SQLx is detected without migration or metadata directories, adopt warns and synthesizes the default `migrations` and `.sqlx` paths unless overridden.

## Accepted Key Summary

Jig rejects unknown `.jig.toml` keys so stale template answers fail early. The accepted top-level keys are `_src_path`, `_commit`, `_template_mode`, `_template_local_path`, `repo_name`, `default_branch`, `ci_github_runner`, `jig_version`, `template_source_url`, `sqlx_enabled`, `rust_crate_roots`, `rust_migration_dir`, `rust_sqlx_metadata_dir`, `schema_dump_enabled`, `schema_dump_command`, `schema_check_command`, `sqlx_check_command`, `migration_add_command`, `bootstrap_command`, `contract_check_command`, `dev_command`, `rust_fmt_check_command`, `rust_clippy_command`, `rust_test_command`, `rust_test_locked_command`, `web_package_manager`, `frontend_apps`, `vault`, `dev`, `work`, and `agent_tooling`. `schema_check_command`, `migration_add_command`, and `contract_check_command` are legacy accepted keys for older rendered repos; new renders use native binary implementations. Older hand-edited v2 manifests that still list these legacy command keys must either keep the matching `.jig.toml` values until updated, or switch the corresponding tools to their native forms.

Nested accepted keys are:

- `[[frontend_apps]]`: `name`, `dir`, `coverage_threshold`
- `[vault]`: `scope`, `scope_id`, `allow_global`
- `[dev]`: `proxy_port`, `https_port`, `https`, `http2`, `lan`, `tld`, `workspace_discovery`, `apps`
- `[[dev.apps]]`: `name`, `dir`, `kind`, `command`, `argv`, `port`, `host`, `proxy`
- `[work]`: `checks`, `gates`, `refinements`
- `[[work.gates]]`: `id`, `kind`, `tool`, `skill`, `fail_on`, `severity`, `scope`, `model`, `required`
- `[[work.refinements]]`: `id`, `skill`, `mode`, `model`
- `[agent_tooling.codex]`: `marketplaces`
- `[[agent_tooling.codex.marketplaces]]`: `id`, `source`, `plugins`

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

Use `scripts/jig doctor` as the first readiness check for a repo. It reports the pinned runtime, `.jig.toml`, contract validity, required command executables, agent skills, proxy status, vault status, and the next setup command. Use `scripts/jig agent doctor` when you only need to report whether the local Codex installation can use the configured marketplace and to show diagnostic plugin enablement flags. Human-readable output is the default. Pass `--json` for stable structured automation output. `agent doctor` exits nonzero until required setup is complete. The agent check requires Codex marketplace support and registered marketplace sources; plugin enablement is reported separately because the supported Codex bootstrap path is marketplace registration. Use `scripts/jig agent bootstrap` to run `codex plugin marketplace add` when exactly one marketplace is configured. If multiple marketplaces are configured, `agent bootstrap` requires `--marketplace <source>` so a repo cannot install several user-level Codex marketplaces by default. `agent bootstrap` mutates user-level Codex config, so it is intentionally separate from the project-owned `bootstrap_command`.

Omitting `agent_tooling`, `agent_tooling.codex`, or `agent_tooling.codex.marketplaces` uses the default Jig skills marketplace. Set `marketplaces = []` to opt out explicitly. In `agent doctor` output, `codex.available` is `true` or `false` when Codex is required, and `null` when the Codex probe is skipped because `marketplaces = []`.

For local development against a sibling checkout, either pass `--marketplace` or set `JIG_SKILLS_MARKETPLACE`. Explicit `--marketplace` wins over `JIG_SKILLS_MARKETPLACE`, and the env var wins over `.jig.toml`; both overrides affect only `agent bootstrap`. Local path sources must be absolute or start with `./` or `../`; they are resolved from the repo root before Codex is invoked, and missing local paths fail before mutating Codex config. Bare `owner/repo` values are treated as marketplace shorthands, not local paths.

```sh
scripts/jig agent bootstrap --marketplace ../jig-skills
JIG_SKILLS_MARKETPLACE=../jig-skills scripts/jig agent bootstrap
```

## `work` Shape

The `work` block declares agent workflow defaults without adding repo-local launcher scripts:

```toml
[[work.gates]]
id = "contract"
kind = "check"
tool = "jig.contract_check"

[[work.gates]]
id = "tests"
kind = "check"
tool = "jig.test"
```

`kind: check` gates must reference execution tool names declared in `.agent/jig-contract.json`. `scripts/jig work check --plan-id ...` runs configured check gates in order unless one or more `--tool` values are passed explicitly. Human-readable output is the default. Pass `--json` for structured automation output.

`scripts/jig work gates --plan-id ...` reports each configured gate as `passed`, `missing`, `failed`, `stale`, `unknown`, or `unsupported`. Pass `--json` when automation needs the full structured payload. `scripts/jig work evidence` is the higher-level human view: it shows the latest gate evidence per tool, whether the proving receipt matches the current worktree, changed paths covered by the receipt, and the exact stale or unknown freshness reason. For `work gates` and `work evidence`, top-level `ok: true` means the inspection command completed; read `overall`, `gates_ok`, and each gate `status` to decide whether work is blocked. Receipt `changed_paths` are repo-relative names from `git status --porcelain=v1 -z`; they can include `.agent/` state paths and untracked filenames, so do not treat receipt JSON as secret-free metadata if local filenames are sensitive. `scripts/jig work finish --plan-id ...` refuses to close work while required gates are missing, failed, stale, unknown, or unsupported. Check gate freshness is based on the non-`.agent/` worktree fingerprint from the latest check or check-batch receipt that proves the gate.

Required check gates should not create or modify non-`.agent/` files during `work check`. Build outputs, generated metadata, and lockfiles should be committed when they are source-of-truth, ignored when they are disposable, or generated before running the fingerprinted check. If a check does intentionally settle generated files, rerun `scripts/jig work check --plan-id ...` after reviewing those changes so the gate evidence matches the final worktree.

After upgrading an in-flight repo from a Jig version that recorded receipts without `worktree_fingerprint`, rerun `scripts/jig work check --plan-id ...` before `scripts/jig work finish --plan-id ...`. Older successful check receipts deserialize correctly, but their freshness is `unknown` and required gates will block finish until fresh evidence exists.

For compatibility, older repos may still use `work.checks`; Jig backfills entries that are not already declared in `work.gates` as required `kind: check` gates with generated IDs. When a tool is declared in both places, the explicit `work.gates` entry is authoritative. New repos should use `work.gates`.

Generated SQLx-enabled repos include a check gate for `jig.sqlx_check`. Repos with schema dumps enabled also include `jig.schema_check` and `jig.schema_dump`.

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

[[frontend_apps]]
name = "admin-panel"
dir = "admin-panel"
coverage_threshold = 0
```

Each configured app directory is expected to support:

- install: `bun install --frozen-lockfile`
- lint: `bun run lint`
- typecheck: `bun run typecheck`
- build: `bun run build:bundle`
- test coverage: `bun run test:coverage`

The coverage command must write `coverage/coverage-summary.json` in the app directory; generated local checks and web CI enforce each app's `coverage_threshold` from that summary.

## `dev` Shape

The `dev` table configures `scripts/jig dev` and `scripts/jig proxy`. This is runtime-owned local machine behavior, not a generated contract tool. Generated repos include a `[dev]` table with conservative defaults; repos that remove it use the runtime defaults from the pinned `jig_version`.

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

`proxy_port` is the TOML name for the HTTP listener. The matching CLI override is `--http-port`. `https_port` is the HTTPS listener; the matching CLI override is `--https-port`.

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

Each `[[dev.apps]]` table accepts `name`, `dir`, `kind`, `command`, `argv`, `port`, `host`, and `proxy`. Unknown app keys are rejected. `name` is required; `dir`, `command`, `argv`, `port`, and `host` are optional; `kind` defaults to `"env-port"`; `proxy` defaults to `true`.

`tld` must use a private/local suffix: `localhost`, `local`, `test`, `internal`, or one subdomain beneath one of those suffixes such as `demo.test`. Public or browser-owned TLDs such as `dev`, `com`, and `io` are rejected so the proxy cannot mint routes that collide with routable DNS names.

Supported dev app kind values are `env-port` and `vite`.

`kind = "env-port"` starts the command with `PORT=<free-port>` and `HOST=127.0.0.1`, overriding inherited values so Jig controls apps that bind from those conventional variables. Framework-specific variables such as `VITE_PORT` or `SERVER_PORT` are not rewritten; configure those apps to honor `PORT`/`HOST`, fail on a busy port, or use a structured app kind that injects framework flags. `kind = "vite"` injects Vite-style `--port`, `--host`, and `--strictPort` flags when they are not already present. Jig also applies the same Vite flags to argv forms that directly invoke Vite, such as `vite`, `npx vite`, `bunx vite`, or `pnpm exec vite`. If a Vite argv already includes `-p` or `--port`, that value must match the Jig-assigned app port. For package-manager commands such as `bun run dev`, `pnpm run dev`, `npm run dev`, or `yarn run dev`, Jig inserts the `--` separator before Vite flags. Vite apps must use `argv`; shell-form `command` is rejected for Vite because safe flag injection requires structured arguments. If both `argv` and `command` are present, `argv` is used.

The Vite integration also sets Vite's `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS` environment hook for the generated dev hostnames. Treat that hook as a Vite compatibility boundary: if your Vite version changes or removes it, use explicit Vite `server.allowedHosts` config or `kind = "env-port"` until Jig is updated.

Generated repos may contain both legacy `[[frontend_apps]]` and matching `[[dev.apps]]` entries. In that shape, `[[frontend_apps]]` keeps CI and coverage metadata, while `[[dev.apps]]` owns local `scripts/jig dev` settings and takes precedence. Every frontend app must have a same-name dev app with the same `dir`; extra dev-only apps are allowed. Legacy `[[frontend_apps]]` entries are still supported as a fallback only when no `dev.apps` are configured, so older generated repos can use the proxy.

To migrate a legacy frontend entry, create a matching `[[dev.apps]]` entry with the same `name` and `dir`, set `kind = "vite"` for Vite-style frontends, and set `argv` to the package-manager dev command such as `["bun", "run", "dev"]`. `coverage_threshold` stays with the older frontend check workflow and is not used by `scripts/jig dev`; keep any build, lint, typecheck, or coverage commands in project-owned scripts or Make targets.

Jig rejects unknown top-level `.jig.toml` keys and unknown keys inside known tables. During upgrades, remove experimental keys or move repo-local notes outside `.jig.toml`; template-owned compatibility keys are listed in the required and optional sections above.

When `workspace_discovery = true`, Jig discovers common JavaScript workspace package globs under the repo root after `JIG_DEV_ALLOW_WORKSPACE_DISCOVERY=1` is present in the environment, because discovered package `dev` scripts are executable repo code. The matching one-shot CLI override is `scripts/jig dev --discover-workspace`. Discovery supports `*`, `**`, and leading `!` exclusions, but not brace expansion such as `apps/{web,admin}`. Discovery skips `node_modules`, dot-directories, symlinked package directories, and canonical paths outside the repo root. Glob expansion fails closed after 10,000 matches; narrow workspace globs in very large monorepos.

`scripts/jig dev` only launches configured `[[dev.apps]]`, legacy `[[frontend_apps]]`, or workspace-discovered apps. It does not run the generic top-level `dev_command`; keep repo-wide commands that do not bind a supervised app port in project-owned scripts or Make targets.

Each dev app defaults to `host = "127.0.0.1"` and `proxy = true` unless a `[[dev.apps]]` entry overrides them. This is the backend target host that Jig forwards to, not the proxy listener address. Proxied dev apps, including legacy `[[frontend_apps]]` entries, must target loopback IP literals such as `127.0.0.1` or `::1`; use `scripts/jig proxy alias` for deliberate proxied non-loopback local tunnels. A `proxy = false` app is launched directly without publishing any Jig proxy route, so its `host` value is only passed to the child process as the app bind target. Set `port` only when an app must use a fixed backend port; otherwise Jig assigns a free port in the local app range.

When running multiple apps with `scripts/jig dev`, Jig treats them as a tied dev session: when the first child process exits, Jig removes the session routes and terminates the remaining child processes.

Jig chooses automatic app ports with a local bind probe against every socket address resolved from each app's configured target host, then starts the child command and waits for the target port before publishing the process route. The probe does not reserve the socket across arbitrary package scripts, so Jig verifies that the observed listener belongs to the spawned child process group before publishing. If a concurrent local process steals the port or the app rebinds to a different port, Jig reports an app readiness failure instead of publishing the bad route.

`scripts/jig proxy run` uses `--` before the command to run, for example `scripts/jig proxy run web -- vite --open`. Use `scripts/jig proxy run web --no-proxy -- <command>` when you only want Jig to assign `PORT`/`HOST` and supervise the process without registering a proxy route. App execution options such as `--kind` and `--port` still affect that direct child process; proxy listener options such as `--http-port`, `--https-port`, `--https`, `--lan`, and `--tld` are rejected with `--no-proxy`.

`scripts/jig dev` and `scripts/jig proxy run` execute repository-configured commands and package scripts. Only run them in repositories you trust.
`[[dev.apps]]` commands and `proxy run -- <command>` are executed verbatim from the configured app directory with Jig-provided `PORT` and `HOST` values, and they inherit the invoking shell environment so ordinary dev credentials and toolchain variables keep working. The string `command` form runs through the platform shell from committed repo configuration; use it only for trusted repos and prefer `argv` for literal argument passing. The long-running background proxy is different: Jig launches it with a cleared environment plus explicit proxy state and minimal toolchain variables, and Unix background starts detach from the caller's working directory. Apps that prefer framework-specific port variables must be configured to derive them from `PORT` or use a Jig app kind that supplies equivalent flags.

The proxy process is shared local runtime state and can outlive a `scripts/jig dev` session. Use `scripts/jig proxy stop` when you want to shut down the background proxy listener. Proxy commands print their JSON response before returning; when a response contains `ok: false`, including stop refusals that deliberately keep runtime files to avoid terminating an unrelated process, the CLI exits nonzero so scripts should inspect the JSON warning field.

The proxy is intended for trusted local development and trusted LAN testing only; it does not provide authentication or multi-tenant isolation.

Routes and aliases are repo-scoped by default. For example, in a repository named `demo`, `scripts/jig proxy alias api --port 8080` registers `api.demo.localhost`, matching the same certificate wildcard used by `scripts/jig dev` apps.

Aliases default to `127.0.0.1`. If you pass `--host`, it must be an IP literal; DNS names are rejected so alias routing cannot depend on mutable hostname resolution. Non-loopback alias targets require `--accept-non-loopback-target`; treat those aliases as local access grants to that target IP and avoid pointing them at sensitive internal services. In LAN mode, aliases may only target loopback IP literals so `0.0.0.0` proxy binding cannot expose arbitrary internal hosts.

Forwarded HTTP and WebSocket requests use the routed development hostname in `Host` and `x-forwarded-host`. Jig replaces inbound `x-forwarded-for` with the direct client address instead of trusting client-supplied chains. Apps that enforce hostnames should allow the generated route names.

Proxy forwarding appends a standard `Via` hop marker to HTTP and WebSocket requests and responses, using the inbound protocol version such as `1.1 jig` or `2.0 jig`. HTTP request bodies are streamed with a 100 MiB forwarding limit, so large upload workflows should bypass the proxy or raise that limit in code. Backend HTTP requests are normalized to HTTP/1.1 even when the client reaches the TLS listener over HTTP/2; HTTP/1 keep-alive is disabled so each HTTP/1 client request uses a fresh connection. The health endpoint requires a loopback client address, a loopback `Host` value, and the per-run health token stored in the private proxy state directory. When the connection limit is reached, the proxy applies backpressure before accepting more sockets, so clients may wait in the OS backlog or time out; slow TLS handshakes are bounded by a short handshake timeout. HTTP/2 is additionally bounded by the configured max concurrent streams per connection and a global active-request limit.

Non-upgrade WebSocket backend responses are drained with a bounded 10 MiB body limit so error pages can be returned without allowing unbounded buffering.

The proxy stores mutable local state under `~/.jig/proxy` by default, or `JIG_PROXY_STATE_DIR` when set. Commands that accept proxy runtime flags also accept `--state-dir <path>` for explicit per-call isolation. This state is deliberately outside `.agent/state` because routes, PID files, port files, certificates, and advisory lock files are mutable machine-local runtime data. Route state is versioned JSON. Route hostnames are shared state-dir keys: if multiple repos use the same state directory and hostname, Jig treats that as the same route and refuses live process-route replacement. Shared state directories reuse one leaf certificate and add hostnames for live routes and aliases, so use separate state directories when many repos or aliases would otherwise make the certificate SAN list too large. State mutations use advisory locks and wait up to 30 seconds before reporting a lock timeout. On Unix, Jig makes the newly created default state parent (`~/.jig`) and the default, newly created, or empty explicit state directory mode `700`; existing default parents and existing non-empty explicit state directories must already be mode `700`. Windows ACL hardening is not implemented.

A proxy 404 lists routes in the active state directory to help local loopback debugging. Non-loopback clients receive a hidden route list. Use separate `JIG_PROXY_STATE_DIR` values when you do not want unrelated repos to share route listings.

Route and certificate caches include file-content signatures plus a short freshness window. Normal route and certificate writes are picked up immediately; a stale read should last no longer than about 500 ms. Dead process routes are filtered from live reads; run `scripts/jig proxy prune` when you want that cleanup persisted to the route file immediately.

`lan = true` binds the proxy to the IPv4 wildcard address `0.0.0.0` and reports the detected LAN IP address when one is available. The detected IPv4 LAN address is captured when the proxy starts and is also used by the proxy self-loop guard, so restart the proxy after changing networks before relying on the new address. Other devices still need a DNS or hosts-file entry for repo-scoped names such as `web.demo.localhost`, or they must send that hostname as the HTTP `Host` header.
LAN IP detection connects an unbound UDP socket to `8.8.8.8:80` to select the outbound interface without sending application data to that address.

On the local machine, the default `.localhost` names resolve to loopback automatically. If you configure `tld = "test"`, `tld = "local"`, or `tld = "internal"`, add hosts-file, DNS, or mDNS resolution for the generated repo-scoped names before expecting browsers to resolve them. Certificates are generated for Jig route hostnames and explicitly configured additional DNS names; custom multi-label TLDs do not imply that the bare TLD itself is covered. Wildcard additional DNS names such as `*.demo.localhost` add the stripped subtree (`demo.localhost`) to the local CA name constraints, so keep them repo-scoped rather than broad.

Generated leaf certificate PEM files contain the leaf certificate. Trust-aware clients should import the Jig local CA through `scripts/jig proxy cert trust --accept-trust-scope` or configure their trust store with the generated CA certificate.

LAN mode exposes the Jig proxy, not the child app process directly. Jig still starts child apps on loopback IP literals (`HOST=127.0.0.1` or Vite `--host 127.0.0.1`) and routes LAN traffic through the proxy. Health and administrative endpoints remain loopback-only. When HTTPS and LAN mode are both enabled, Jig includes the detected IPv4 LAN address in the generated certificate names when one is available; switching networks can change that IP, so regenerate the leaf certificate if browsers report a name mismatch after moving networks. For `tld = "local"` in LAN mode, Jig does not add broad `local` or `*.local` certificate names; use repo-scoped route hostnames or explicit additional DNS names instead.

In LAN mode, Jig-owned process routes remain reachable because Jig starts and supervises their child apps on loopback IP literals. Alias routes remain loopback-client-only; local loopback clients can still use those aliases for remote tunnels or shared development services.

On Windows and BSD-like platforms, child-process tree cleanup is best-effort, and process-owned proxy routes are refused because high-confidence process start-token verification is not implemented yet. `scripts/jig proxy stop` on Windows only terminates a process after the authenticated loopback health endpoint reports the stored PID, but it cannot recheck a process start token before `taskkill`; stop long-running Windows proxies promptly if you are relying on PID identity. HTTPS dev proxying is not supported on Windows because Jig does not yet harden private-key ACLs there; use plain HTTP, `scripts/jig proxy run --no-proxy`, or `scripts/jig proxy alias` for manually managed loopback services.

If a proxy is already running without HTTPS and a later command asks for HTTPS, stop and restart the proxy with HTTPS using the same `JIG_PROXY_STATE_DIR`. Use separate state directories for worktrees that need different HTTP/HTTPS listener settings.

Ports below `1024`, including `80` and `443`, usually require elevated bind privileges on Unix-like systems. Jig attempts the bind and reports the OS error with an actionable hint when it fails. On Linux, grant the installed Jig binary `cap_net_bind_service`; on macOS, use a root-owned LaunchDaemon or forward 80/443 to unprivileged Jig proxy ports.

Useful commands:

- `scripts/jig dev`
- `scripts/jig dev --app web`
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

`scripts/jig proxy cert untrust --accept-trust-scope` removes matching Jig Dev Proxy Local CA certificates by fingerprint where the platform tooling can manage them after you acknowledge platform trust-store mutation. On macOS this deletes matching certificates from the login keychain rather than only toggling trust settings; manually remove copies installed in other keychains, and run the command again if it reports that more matching certificates may remain. On Linux, Jig removes the current CA's exact p11-kit trust anchor when `trust list --filter=ca-anchors` reports it and refreshes the system CA bundle with `update-ca-trust extract` or `update-ca-certificates` when one is available; distribution policy determines whether those steps need privileges. Automatic certificate generation, trust, and untrust are not implemented on Windows until owner-only ACL hardening for private key files is available.

If the local CA key may be compromised, run `scripts/jig proxy cert untrust --accept-trust-scope` before `scripts/jig proxy cert generate --force`, then trust the regenerated CA only if needed. On macOS, `generate --force` refuses to replace a currently trusted Jig CA by fingerprint so an old trusted root is not orphaned. On Linux, Jig also checks p11-kit's trusted CA anchor list for a Jig Dev Proxy CA label when `trust` is available. On other platforms, Jig records successful Jig-managed trust operations in the state directory and refuses forced replacement while that marker still matches the current CA.

## Vault Runtime

Quick start:

```sh
scripts/jig vault init
scripts/jig vault secret set api_token --value-prompt
scripts/jig vault run --env TOKEN=api_token -- sh -c 'printf "%s\n" "$TOKEN"'
scripts/jig vault run --file TOKEN_FILE=api_token -- sh -c 'cat "$TOKEN_FILE"'
scripts/jig vault audit verify
```

`jig init` and `jig adopt --write` initialize a repo-scoped local vault by default. Pass `--no-vault` to skip that local setup. `jig adopt` without `--write` remains a side-effect-free preview and does not create vault state.

At a glance:

- Terminal commands prompt for the vault passphrase; non-interactive commands use `JIG_VAULT_PASSPHRASE`.
- `jig init --no-input` and `jig adopt --write --no-input` never prompt for the vault passphrase; export `JIG_VAULT_PASSPHRASE` or pass `--no-vault`.
- `--defaults` is also treated as automation intent for vault setup: when vault setup is enabled and no passphrase environment variable is present, Jig captures the new vault passphrase before rendering so it can initialize the vault after repo files are written.
- Generated repos default to a repo scope declared in `[vault]`; `--global` is an explicit logical escape hatch and is rejected unless `[vault].allow_global = true`.
- `--home` is an explicit physical vault-home override for diagnostics and tests; it bypasses repo scoping and `[vault].allow_global`.
- `vault secret set NAME` defaults to the hidden prompt when run from an interactive terminal; `--value-stdin` is the byte-exact automation path for piped or redirected stdin.
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

When a command runs inside a repo with `scope = "repo"`, Jig resolves secrets under `~/.jig/vault/scopes/` by default. The physical scope directory is derived from the canonical local repo root plus the non-secret `scope_id`, rather than from `scope_id` alone, so copying `.jig.toml` to another repo cannot select the first repo's vault namespace. Moving or renaming a repo changes that trusted local namespace; copy secrets intentionally if you want the moved checkout to inherit the old local vault. If `JIG_VAULT_HOME` is set, it is treated as the local vault base for repo scopes, so the scoped home is below `$JIG_VAULT_HOME/scopes/`. Repos without `[vault]` keep legacy user-level behavior and resolve the physical vault home directly from `--home`, `JIG_VAULT_HOME`, or `~/.jig/vault`. Re-adopting a legacy repo adds a new repo scope and reports that migration in the command notes; existing global-vault secrets are not copied automatically. The `--home` flag is an explicit physical-home override for diagnostics and tests and bypasses repo scoping plus `[vault].allow_global`; use it only when you intentionally want a specific vault directory. The `--global` flag selects the user-level global vault, but scoped repos reject it unless `[vault].allow_global = true`.

If automatic vault initialization fails after `jig init` or `jig adopt --write` has rendered repo files, Jig leaves the repo files in place and reports the vault error with a recovery command. Fix the reported vault or config issue, then run `jig vault init` from the repo root.

Jig creates or tightens vault directories to owner-only permissions, so do not point `JIG_VAULT_HOME` at a shared directory. The vault derives its passphrase wrapping key with Argon2id using 128 MiB memory, 3 iterations, 4 lanes, and a 32-byte output. New vault passphrases must be at least 12 bytes; Jig enforces a floor, not a password-strength meter, so operators are still responsible for choosing high-entropy passphrases. Passphrases are byte-exact after UTF-8 capture: Jig does not trim whitespace, strip trailing newlines, normalize Unicode, or otherwise rewrite prompt or `JIG_VAULT_PASSPHRASE` input. Terminal use prompts for hidden passphrase entry. Non-interactive use reads the unlock passphrase from `JIG_VAULT_PASSPHRASE` and clears that child-process environment variable after successful capture; this is best-effort process hygiene and does not prove the OS or C runtime overwrote every previous environment backing byte. Command-line passphrases are intentionally unsupported because they leak through shell history and process listings.

Keep `JIG_VAULT_PASSPHRASE` exported or re-export it for every non-interactive command that unlocks the vault, including `secret list`, `run`, and `audit verify`; `vault status` is the only vault command that does not require the passphrase. `vault status` is a non-creating probe: it refuses a symlinked vault home, but it does not create missing directories or tighten permissions. Its `exists` and `vault_file_exists` fields report whether `vault.json` exists, not whether the home directory exists.

The vault file is encrypted at rest with a passphrase-derived wrapping key and a random data-encryption key. Secret listing commands return names and metadata, never values. `scripts/jig vault secret set NAME` defaults to hidden UTF-8 terminal entry without a trailing newline when run interactively; pass `--value-prompt` to request that path explicitly. `scripts/jig vault secret set NAME --value-stdin` requires piped or redirected stdin and stores those bytes exactly as provided, including a trailing newline from commands such as `echo`; use `printf` when the newline is not part of the secret. Non-interactive `secret set NAME` without `--value-stdin` fails instead of waiting for input. Secret values must be between 4 bytes and 1 MiB so redaction can match them safely without unbounded local memory use. `scripts/jig vault run --env VAR=SECRET -- <command>` resolves named secrets, starts a child process with a cleaned environment plus the requested secret variables, captures stdout and stderr, and redacts known secret forms before returning JSON. On Unix, `scripts/jig vault run --file VAR=SECRET -- <command>` writes each requested secret to a private temporary file with mode `0600`, injects the file path as `VAR`, and removes the temp directory when the brokered process exits normally through Jig; abrupt process termination such as `SIGKILL` can leave temp files behind for OS temp cleanup. Non-Unix platforms reject `--file` because Jig cannot guarantee equivalent secret-file permissions there; use `--env` or a platform-specific wrapper instead. Environment injection necessarily gives the standard library and child process an OS-owned environment copy that Jig cannot zeroize afterward. File delivery keeps the value out of the environment but still gives the child filesystem access to the secret bytes while it runs. Each captured stream is capped at 1 MiB; exceeding the cap fails the brokered run instead of buffering unbounded output. The cleaned environment preserves only a small allowlist of process basics and locale variables, not arbitrary `LC_*`, `SSH_AUTH_SOCK`, `XDG_*`, or `TZ` variables; the child uses the preserved `PATH` inherited by the `jig` process to resolve command names. `vault run --env` and `vault run --file` reject mappings that would overwrite preserved environment variables such as `PATH`, `HOME`, `TMPDIR`, or locale variables. The broker does not sandbox the child's filesystem view. Environment variable names must match `[A-Za-z_][A-Za-z0-9_]*`.

`vault run` buffers the child process' full stdout and stderr before displaying them because redaction is applied to the captured output. This keeps v1 redaction simple but means long-running commands do not stream output. Redaction can allocate intermediate output buffers that are not zeroized; it is a safety net for displayed output, not an in-memory secrecy boundary. The child is non-interactive: stdin is closed/null, so commands that prompt for input should fail or hang instead of asking the operator. A non-zero child exit is returned in the JSON result and the Jig CLI exits with that child status code, clamped to the portable process-exit range after vault runtime values have unwound through `main`. On Unix, signal-terminated children report both `exit_signal` and shell-style `128 + signal` status.

Vault operations append local HMAC-chained audit events. Audit details record secret names in cleartext, so names should be operational labels rather than sensitive values; path-like names using `/` or `.` are allowed but will appear in the JSONL audit log. `scripts/jig vault audit verify` recomputes the chain and fails if existing event contents or links were edited. The audit log is tamper-evident against external file edits, not tamper-proof against someone who has both vault access and the passphrase; detecting deletion, truncation, or rollback still requires external checkpointing or backups. The audit log is append-only and has no v1 rotation or archival mechanism, so each mutation and brokered run increases the verification work; very large audit logs will make append and verify operations slower until a future rotation/checkpoint workflow exists. Vault mutations append the audit intent before saving the new vault state, so a crash can leave audit leading state but should not leave state leading audit. A hard crash during audit append can leave a torn final line; verification reports `torn_tail_bytes`, and the next append truncates only that unterminated tail before adding a new event with `truncated_torn_tail_bytes` in its audit details. Vault mutations serialize on one local advisory lock with a 30-second acquisition ceiling. Unlocking derives an Argon2id key each time, so tight loops of `vault run` or repeated mutations intentionally pay that local KDF cost instead of caching unlocked key material. There is no v1 unlock rate limiter or lockout counter; the KDF is the local offline-guessing cost control, not an online account-protection system. `vault run` keeps open, secret resolution, and the start audit append under the same lock, then releases the lock while the child runs, so concurrent runs may interleave start and finish/failure events in the audit log while keeping the chain valid; each brokered run event includes a `run_id` for pairing, and failure events include a `stage` such as `resolve` or `process`. A resolve-stage failure writes a failure event without a start event because no child process received secrets. If the `jig` process is killed after a brokered run start event, that start event can remain without a matching finish/failure event. Directory-entry durability is strongest on Unix, where parent directories are fsynced after atomic writes and audit creation; Windows currently relies on the platform rename/write guarantees available through the standard library. Vault home canonicalization is intentional: Jig hardens the resulting vault tree and the first existing creation ancestor, not every ancestor above a user-selected path. If `vault init` writes the first audit event but fails before writing the vault file, the next init fails closed on stale `audit.jsonl`; inspect the vault home and remove stale init artifacts before retrying. If `vault init` reports rollback cleanup failures, inspect or remove the vault home before retrying.

This is a blast-radius reducer, not a sandbox. Once a child process receives a secret in its environment, that process can use or disclose it. The redactor is a backup control for accidental output, not a guarantee against malicious transformations or side channels.

Useful commands:

- `scripts/jig vault init`
- `scripts/jig vault status`
- `scripts/jig vault secret set api_token --value-prompt`
- `printf '%s' 'secret-value' | scripts/jig vault secret set api_token --value-stdin`
- `scripts/jig vault secret list`
- `scripts/jig vault secret remove api_token`
- `scripts/jig vault audit verify`
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
- `scripts/jig migration-add NAME`

When both `sqlx_enabled` and `schema_dump_enabled` are `true`, it also exposes:

- `scripts/jig check schema`
- `scripts/jig schema-dump`

`scripts/jig check schema` reruns `schema_dump_command`, then checks `SCHEMA_DOCS_DIR` for drift. `SCHEMA_DOCS_DIR` defaults to `docs/schema` when the environment variable is unset.

Generated repos also get these runtime-owned files:

- `.mcp.json`
- `.agent/jig-contract.json`
- `scripts/jig`
- `scripts/install-jig.sh`

The generated `scripts/jig` launcher enforces the exact `jig_version` pinned in `.jig.toml`. On first use it installs that version into a repo-local cache and then exposes the configured command contract as:

- CLI commands such as `scripts/jig check fmt`
- MCP tools such as `jig.fmt_check`

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
- `scripts/jig work start --title ...`
- `scripts/jig work start --title ... --print-plan-id`
- `scripts/jig work append --plan-id ...`
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
- `scripts/jig state archive --before YYYY-MM-DD --dry-run`
- `scripts/jig state archive --before YYYY-MM-DD`

`work finish` closes the plan with `--resolution`. If an active session is also open, it closes that session with `--outcome`; when `--outcome` is omitted, the session outcome falls back to `--resolution`.

Contract tools and work checks intentionally append receipts under `.agent/state/`.
Read-only inspection commands such as `work status` and `work gates` do not add
new receipts. For one-off contract command runs that should not record evidence,
pass `--no-receipt`; `--no-receipt` conflicts with `--plan-id` because
plan-linked checks must leave evidence for `work finish` gate enforcement. When
receipt recording is skipped, command JSON still includes `"receipt_id": null`.
Use `scripts/jig state archive --before ...` when `receipts.jsonl` grows too
large; `--before YYYY-MM-DD` is interpreted as a UTC cutoff date. Archiving
keeps latest gate evidence active and moves older unprotected receipt records
into `.agent/state/archive/`. The retention model preserves the latest
work-linked receipt for each plan/tool/gate plus the check receipts that support
that latest evidence, so old closed or abandoned plans can keep their most recent
gate evidence in the active file. Archiving limits historical receipt growth; it
does not guarantee that every receipt for an old plan is moved out of
`receipts.jsonl`.

For local runtime development, set `JIG_DEV_BIN` to an already-built `jig` binary. The installer resolves that explicit binary to an absolute path before returning it, and verifies that its reported version matches `.jig.toml`. A stale or mismatched `JIG_DEV_BIN` is a hard error rather than a fallback to the cached runtime. In the `jig-sh` source checkout, the installer also keeps the repo-local cache tied to the current checkout so same-version release caches do not hide local runtime changes. Avoid rebuilding that binary while a long-running `JIG_DEV_BIN` process, such as `jig proxy start --foreground`, is still active.

## SQLx Metadata Directory

This section applies only when `sqlx_enabled` is `true`.

`rust_sqlx_metadata_dir` is wired into the generated `sqlx-check` target via `SQLX_OFFLINE_DIR`. Use `.sqlx` unless the repository has already standardized on a different committed metadata path.

## Template Source

For portable shared repos, set:

```toml
template_source_url = "git@github.com:your-org/jig-sh.git"
```

When `template_source_url` is set, the renderer writes it into `_src_path` for portable update and install behavior. If it is blank, local template renders keep the local source path.
