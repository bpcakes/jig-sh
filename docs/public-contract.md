# Public Contract

`jig` exposes a repo command contract through three surfaces:

- CLI commands from `scripts/jig`
- MCP tools from `scripts/jig mcp`
- `.agent/jig-contract.json`

Generated repositories may rely on the contract described here when they pin a `jig_version` in `.jig.toml` and keep `scripts/jig`, `.mcp.json`, and `.agent/jig-contract.json` in sync with that version.

Jig also specifies the separate open [`jig.status-provider/v1`](status-provider.md) protocol. It lets a project-specific inspector, including a closed-source provider, publish software-rewrite observations to Jig or any other consumer through a committed JSON Schema. The status-provider protocol is not a command in `.agent/jig-contract.json`, and its major version is independent of the generated command contract's `contract_version`.

Structured work commands, state hygiene commands, first-run setup, the unified doctor, status aggregation, Codex-home selection, and agent tooling checks are runtime-owned conveniences. They are available through commands such as `scripts/jig setup`, `scripts/jig doctor`, `scripts/jig status`, `scripts/jig work ...`, `scripts/jig state ...`, `scripts/jig codex ...`, and `scripts/jig agent doctor`, and MCP tools named `jig.work_*` and `jig.agent_doctor`, but they are not part of the generated command contract and are not declared in `.agent/jig-contract.json`. Status text, JSON, and TUI modes and the `codex` namespace are CLI-only.

CLI commands print human-readable output by default. Pass global `--json` for structured automation output (for example `scripts/jig doctor --json`, `scripts/jig status --json`, `scripts/jig work status --json`, or `scripts/jig work evidence --json`). Usage and pre-output command failures in JSON mode write one object to stdout with `ok: false`, `error.kind` (`usage` or `command_failed`), `error.message`, and `exit_status`, while preserving the nonzero process status. Commands that already emitted JSON do not append a second error document, and `scripts/jig mcp` always reserves stdout for MCP framing. `scripts/jig prompt get` prints the bare rendered body without `--json` and the standard `prompt get` command envelope with it. `scripts/jig status --tui` is an explicit interactive consumer and conflicts with `--json`; it requires terminal stdin and stdout. For other commands, output selection is independent of interactivity: `--json` does not suppress terminal prompts. For init automation, `--defaults` applies documented project-shape defaults but can still prompt for initial vault setup; supply `JIG_VAULT_PASSPHRASE` or `--no-vault` when that must be noninteractive. `--no-input` and implicit non-terminal execution require an explicit complete shape such as `--preset harness-only`; stored `harness_footprint = "minimal"` is also a complete harness-only shape. `scripts/jig work start --print-plan-id` remains a shell-capture override that prints only the new plan id. Human text, TUI presentation, and `--print-plan-id` output are for terminal use and are not stable machine-readable contract output; automation should pass `--json` or use MCP tools.

`scripts/jig codex homes --json` returns a runtime-owned `schema_version: 1` report of local Codex home paths, account identity, plan type, and per-home errors. A home's `status` records stable account state such as `not logged in` or `unknown`; `inspection_error` records account/app-server inspection failure, while `usage_error` records a rate-limit failure after a logged-in account was observed. Both are mirrored in the top-level `errors` array with distinct `kind` values. A logged-out home is a complete observation even when usage was requested: rate-limit usage is not applicable, so an app-server usage failure is not surfaced as `usage_error` and does not change `outcome` to `partial`. Add `--usage` to include every rate-limit bucket and the server-reported durations and reset times. If account inspection succeeds for a logged-in account but usage inspection fails, the report retains the account and records the usage failure. Account and usage data come from the Codex app-server API; Jig does not parse `auth.json`. This output contains local paths and account email addresses and should be handled as user data. Its top-level `representation_lossy` boolean reports whether any non-UTF-8 home path or Codex executable value had to be replaced for JSON display. `scripts/jig codex launch HOME --dry-run --json` is the `schema_version: 1` structured launch preview; its own `representation_lossy` boolean additionally covers forwarded arguments. Human terminal previews replace control characters before display and explicitly warn when the shown shell command is therefore not launch-equivalent; the JSON preview retains the original string values except for reported non-UTF-8 conversion. A real launch replaces the Jig process on Unix and therefore rejects `--json`; forwarded Codex arguments begin after `--` and retain their original argument boundaries. On platforms without process replacement, Jig waits for Codex and exits with Codex's shell-compatible status.

`scripts/jig status --json` returns a runtime-owned aggregate with `schema_version: 1`. Its top-level sections are `repository`, `work`, `loops`, `providers`, and `errors`; accepted provider documents remain independently discriminated by `protocol: "jig.status-provider/v1"` under `providers[].report`. Top-level `ok: true` means inspection completed, while `outcome` is `complete` or `partial`. Provider failures are data with `providers[].status = "failed"` and `report: null`, so the command can still return an inspectable snapshot. Domain blockers, stale or dirty inputs, and blocked gates are observed facts rather than collection errors. The aggregate schema version is independent of both provider `protocol` and generated `contract_version`.

Bootstrap command JSON is also runtime-owned. `scripts/jig init --json`, `scripts/jig adopt --json`, and `scripts/jig update --json` include a `render_report` object that summarizes created, modified, unchanged, conflict, backup, managed-block, and todo items for human review. When `jig init --json` runs a project scaffold, its sibling `scaffold` object reports the scaffold preset, sanitized `repo_name`, nullable `repo_name_sanitized_from`, `db`, `frontends[].{name,dir,kind,role}`, `frontend_notices` for bare custom names that are not preset shorthands, and `files_created` / `files_modified` / `files_unchanged` separately from template-managed file counts in `render_report`. Generated shadcn React frontends carry a same-version `ui` provenance object describing the system, CLI version, preset, primitive base, style, and Tailwind major. `scripts/jig adopt` previews by default with `render_mode = "preview"` and only applies managed files with `render_mode = "copy"` when `--write` is supplied. `scripts/jig init`, `scripts/jig adopt`, and `scripts/jig update` print human summaries by default; pass `--json` for the full structured reports. Automation should treat those reports as same-version diagnostics, not as `.agent/jig-contract.json` response schemas.

Agent-guide check JSON keeps `missing_guides` as an empty compatibility field in this contract version and includes `missing_guides_note` to explain that placeholder crate-level `AGENTS.md` files are no longer required. Existing guide files are validated when present. Consumers should stop treating `missing_guides` as the guide-coverage gate; use `missing_sections` and `missing_entry_ref` for existing-guide quality issues.

Dev proxy and vault JSON are also runtime-owned. Proxy status may include machine-local health fields such as `pid`, `pid_alive`, `health_pid`, `handshake_ok`, `pid_matches_proxy`, `running`, listener addresses, and route URLs; status and listing commands may perform a loopback HTTP health probe to populate those fields. Strict cross-machine automation should rely on the stable generated command contract instead of treating those runtime diagnostics as a contract schema.

The structured work namespace includes native check gates. Gates are configured in `.jig.toml`, evaluated from receipts, and enforced by `scripts/jig work finish`. They remain runtime-owned because they compose stable execution tools with append-only work state rather than adding new generated contract tools.

`scripts/jig work gates` and `scripts/jig work evidence` include the current worktree fingerprint in their runtime-owned JSON so humans and agents can tell whether receipts are fresh. That fingerprint is an opaque same-version comparison token, not a stable public hash contract; consumers should compare it for equality only within the same pinned Jig runtime version.

Local development proxy commands are also runtime-owned. `scripts/jig dev`, `scripts/jig dev status`, `scripts/jig dev stop`, and `scripts/jig proxy ...` manage machine-local processes, ports, routes, certificates, and optional user services. They are configured from `.jig.toml` but are intentionally absent from `.agent/jig-contract.json` because they do not represent repository checks.

Runtime-owned local development commands include `dev`, `dev status`, `dev stop`, `proxy start`, `proxy stop`, `proxy list`, `proxy prune`, `proxy run`, `proxy alias`, `proxy cert generate`, `proxy cert status`, `proxy cert trust --accept-trust-scope`, `proxy cert untrust --accept-trust-scope`, `proxy service install --accept-service-scope`, `proxy service status`, and `proxy service uninstall`. Bare `dev` launches apps, while its `--replace` option retires only conflicting registered sessions owned by the same canonical repository; it is not a general process takeover option. Foreground `dev` and `proxy run` interruption is structured same-version output with `interrupted`, numeric `exit_signal`, named `termination_signal`, and shell `exit_status`; SIGINT, SIGHUP, and SIGTERM map to 130, 129, and 143 on Unix. Builds made with `--no-default-features` keep the contract, MCP, and work-receipt runtime but return clear errors for every `dev` action and `proxy`; use that build mode for MCP/contract-only consumers that do not need the TLS/HTTP dev-proxy stack.

Dev-session JSON is same-version runtime output. `dev status` reports the canonical repo identity, resolved state directory, aggregate `running` state, and sanitized registered sessions with supervisor/app health. `dev stop` reports matched/stopped session and app counts, any sessions that remain, and warnings; it is successful and idempotent when no session matches, but returns `ok: false` if exact cleanup remains unconfirmed. Neither response exposes the persisted session-control credential.

Local vault commands are runtime-owned as well. `scripts/jig vault init`, `scripts/jig vault status`, `scripts/jig vault secret ...`, `scripts/jig vault audit verify`, and `scripts/jig vault run ...` manage encrypted machine-local secret state and brokered child-process execution. Generated repos carry non-secret `[vault]` scope metadata in `.jig.toml`; when present, vault commands default to that repo scope rather than the user-level global vault. They are intentionally absent from `.agent/jig-contract.json`, MCP tool listing, and repo-local command receipts in the initial implementation because they manage local secrets rather than repository checks and should not persist child output into `.agent/state`.

Vault JSON is runtime-owned same-version behavior, not generated contract schema. `vault status` currently reports both `exists` and `vault_file_exists`; both mean the encrypted `vault.json` file exists, not that the vault home directory exists. Vault commands always report `vault_scope`, `vault_scope_id`, and `vault_repo_name`; `vault_scope_id` and `vault_repo_name` are null when not applicable. Current `vault_scope` values are `repo`, `global`, `legacy`, and `explicit-home`. `vault run` returns mapping counts plus buffered, redacted, lossy UTF-8 `stdout` and `stderr` strings plus raw process status fields; automation should use `result.exit_signal` to distinguish signal termination when that field is present, and otherwise branch on `result.exit_status`.

LAN mode exposes the Jig proxy listener to the local network, not child app listeners directly. Process routes may be reached from other devices only through the proxy, with the original routed hostname in DNS, a hosts file, or the HTTP `Host` header. Alias routes stay loopback-client-only so LAN clients cannot use Jig as an open forward proxy.

The `tool_defs::cli_command` names for these runtime-owned commands are parser labels only. They do not add generated tools to `.agent/jig-contract.json` and do not expose MCP tools for proxy process or service management.

Because the local development proxy and local vault are runtime-owned, their JSON response fields, machine-local state layouts under `JIG_PROXY_STATE_DIR` / `~/.jig/proxy` and `JIG_VAULT_HOME` / `~/.jig/vault`, service-file contents, certificate files, vault envelope format, route hostname format, and nonzero error exit statuses are not part of `.agent/jig-contract.json`. The vault audit JSONL is tamper-evident but plaintext metadata; secret names, environment variable names, timestamps, run IDs, and vault IDs should be treated as local operational metadata rather than opaque encrypted payload. Generated repos should pin `jig_version` for this behavior and treat those details as same-version runtime behavior rather than as public contract fields.

The current explicit acknowledgement flags, including `--accept-trust-scope` and `--accept-service-scope`, are runtime safety gates rather than generated contract fields. Automation should keep using the pinned `jig_version` CLI help and behavior instead of assuming those opt-in prompts are stable across runtime upgrades.

Runtime-owned `.jig.toml` sections are intentionally strict: unknown keys are rejected so local typos fail fast. New keys in `[work]`, `[status]`, `[[status.providers]]`, `[agent_tooling]`, `[agent_tooling.codex]`, `[dev]`, or app tables require a Jig runtime/template update and a documented migration note; they do not require a `.agent/jig-contract.json` version bump unless they also change generated CLI or MCP contract behavior.

## Contract Version

`.agent/jig-contract.json` has these schema versions:

- `contract_version`: version of the generated tool manifest and command surface

Version `2` is the legacy root-check command-backed contract. Version `3` is the current command-backed contract with checks grouped under `scripts/jig check ...`. Moving a repo from version `2` to `3` requires updating CI, scripts, docs, and agent instructions that invoke the old top-level check commands. A compatible change may add optional fields, optional tools, optional commands, or new CLI/MCP commands. A breaking change must increment `contract_version` before generated repos depend on it.

Breaking `contract_version` changes include:

- removing or renaming a stable generated tool
- removing or renaming a stable generated command key
- changing a stable command argument from optional to required
- changing the meaning or type of a stable JSON request or response field
- changing `.agent/jig-contract.json` in a way older runtimes cannot ignore

## Stable Manifest Fields

Generated repos and MCP clients may rely on these top-level fields in `.agent/jig-contract.json`:

- `contract_version`
- `tool_namespace`
- `jig_version`
- `required_commands` for command-backed contract versions `2` and `3`
- `tools`

Each tool entry has these stable fields:

- `name`
- `kind`
- `description`
- `command` for `kind: "command"` tools

For `kind: "command"` tools, `command` is the top-level `.jig.toml` command key the runtime executes from the repo root.

Command-backed contract versions intentionally have no `optional_commands` field. A command-backed tool is valid only when its command key is listed in `required_commands`; optional capability is represented by omitting the tool entirely when the rendered repo profile does not support it.

Consumers should ignore unknown top-level manifest fields and unknown fields inside tool entries.

## Stable Tools

The following tool names are stable in command-backed contract versions when declared in the manifest:

- `jig.bootstrap`
- `jig.fmt_check`
- `jig.clippy`
- `jig.test`
- `jig.test_locked`
- `jig.contract_check`

SQLx-specific tools are stable when the rendered repo profile includes them:

- `jig.sqlx_check`
- `jig.migration_add`
- `jig.schema_check` when schema dumps are enabled
- `jig.schema_dump` when schema dumps are enabled

SQLx-specific tools are stable when `sqlx_enabled` rendered them into the manifest:

- `jig.sqlx_check`
- `jig.schema_check`
- `jig.schema_dump`
- `jig.migration_add`

A generated repo may omit optional tools that do not apply to its configuration. Clients must discover available tools from `.agent/jig-contract.json` or MCP tool listing instead of assuming SQLx or schema-dump support.

## Stable JSON Behavior

All successful stable CLI and MCP command responses are JSON objects unless a runtime-owned command explicitly documents a human-output flag. Stable response fields are additive: existing fields should keep their names, types, and meanings for the current contract version, and new fields may be added.

Stable common response fields:

- `ok`: boolean success indicator
- `receipt_id`: receipt identifier when the command records a receipt

Make-backed tools return:

- `tool`
- `target`
- `args`
- `result.exit_status`
- `result.stdout`
- `result.stderr`
- `receipt_id`

Command-backed tools return the same common fields plus `command_key`, which identifies the `.jig.toml` command key that was executed.

## Runtime State

`.agent/state/*.jsonl` is runtime-owned append-only memory during normal operation. Generated repos may back up, inspect, or remove these files intentionally, but application code should not edit individual records in place. Runtime-owned maintenance commands may perform validated whole-stream rewrites with recovery artifacts. Generated `.gitattributes` marks those JSONL files with `merge=union` to reduce avoidable merge conflicts between independent append-only records.

Current JSONL state files:

- `sessions.jsonl`
- `plans.jsonl`
- `receipts.jsonl`
- `decisions.jsonl`

State readers should tolerate missing files by treating them as empty. JSONL readers should ignore blank lines and fail loudly on malformed nonblank records. Session-start records retain their durable write-time summary, but `summary.recent_sessions` contains shallow event references whose nested `summary` is `null`; historical records that recursively embedded older summaries remain readable. Canonical session readers collapse duplicate IDs with identical event envelopes, as can arise after a line-union merge, and reject the same ID with a conflicting envelope.

Receipt records may include an `evidence` object for structured runtime-owned evidence that does not fit safely in truncated stdout or stderr previews. Receipt Git metadata excludes `.agent/**`; `changed_paths` contains at most 100 sorted paths, while optional `changed_path_count`, `changed_paths_truncated`, and `changed_paths_digest` describe the full path set. Successful stdout and stderr previews use a 512-byte truncation threshold and failed previews use a 4,000-byte threshold. Older receipts without the new path-summary fields remain readable. Codex review receipts use `evidence.kind = "codex_review"` and store normalized findings there, capped to the first 100 findings with long finding fields shortened; raw finding and actionable counts remain available so truncation does not hide a failing gate. Their receipt `exit_status` is the gate verdict, while `evidence.codex_exit_status` is the underlying Codex process status. They also include short stdout/stderr previews for failed review debugging. Codex refinement receipts use `evidence.kind = "codex_refine"` and store the refinement iteration, optional refinement profile metadata, reviewed gate ids, finding fingerprints, and finding count.

The active-session pointer is cache state, currently resolved through git as `jig-current-session.txt` and falling back under `.agent/.cache/`. Generated repos should not treat that path as a durable JSONL record.

`scripts/jig work append` requires exactly one nonblank progress source through `--body` or `--body-file`. `scripts/jig state summary` focuses its human output on persisted record and event counts, while `scripts/jig work status` presents the same runtime-owned state as an operational work overview. `scripts/jig state diagnose` is read-only; `--deep` adds recursive-session and receipt-payload analysis. Diagnostics also report disk usage for local maintenance artifacts under `.agent/.cache/state-backups/` and `.agent/.cache/state-archives/`. `scripts/jig state compact sessions --dry-run` validates and projects a legacy-session rewrite without changing state. Apply mode preserves root summaries and ordered direct references, removes recursively embedded summaries, validates the result, and first writes an exact gzip backup and checksum manifest under ignored `.agent/.cache/state-backups/`. `scripts/jig state restore --backup <directory-or-manifest>` verifies that artifact before restoring the exact pre-compaction stream.

`scripts/jig state archive --before <YYYY-MM-DD|unix-ms>` writes eligible old receipts as gzip JSONL under ignored `.agent/.cache/state-archives/` and then rewrites `receipts.jsonl`, preserving evidence needed by currently open plans. Before replacing active state it also creates a complete, manifested receipt backup under `.agent/.cache/state-backups/`; `state restore --backup ...` recovers the exact pre-archive byte stream and physical order. Use `--dry-run` to inspect counts before mutation. `scripts/jig state export receipts --before <cutoff> --output <file.jsonl.gz>` writes the selected raw records without changing active state and refuses to replace an existing destination. Legacy `.agent/state/archive/` files remain untouched and appear in diagnostics.

Compaction and archiving change only the current working-tree streams. They never rewrite Git history or remove blobs reachable from existing commits. Artifacts under `.agent/.cache/` are ignored local recovery aids, not durable off-machine backups; command output identifies the paths and checksums that should be copied elsewhere when long-term recovery is required.

Applying compaction, archive rewrites, and restore require a writer cutover from pre-cache-lock Jig runtimes: stop older Jig processes before mutation. Current runtimes serialize on the repository state lock, while a legacy writer already waiting on a pre-opened inode cannot safely follow an atomic replacement. Keep the newest recovery backup until the rewrite is verified, copy long-lived artifacts outside `.agent/.cache/`, and remove obsolete cache backups or archives; diagnostics report their separate disk usage.

Structured work commands use the `jig.work_*` CLI and MCP namespace, but state-operation receipts keep their historical tool names for compatibility with existing receipt history and filters:

- `jig.session_start`
- `jig.session_end`
- `jig.plans_open`
- `jig.plans_append`
- `jig.plans_close`
- `jig.decisions_add`

## Work Gates

`work.gates` in `.jig.toml` declares required evidence before structured work can finish. `kind: check` gates reference execution tools from `.agent/jig-contract.json`; `scripts/jig work check --plan-id ...` runs them and records normal receipts for an open plan. `kind: codex_review` gates reference Codex skills and are run by `scripts/jig work review --plan-id ...`, which records structured `jig.work_review` receipts with normalized findings, prompt/schema hashes, skill metadata, and worktree fingerprints. `scripts/jig work refine --plan-id ...` reads failed review findings, runs a Codex fixer loop, reruns review gates, then reruns normal check gates. `scripts/jig work gates --plan-id ...` reports gate status from the latest fresh receipt for each gate on any existing plan, including a closed plan. `scripts/jig work evidence` presents the same gate evidence as a human inspection report with the latest gate evidence, current-worktree match status, changed paths, and stale reasons. Latest evidence entries expose either `tool` for check gates or `skill` for review gates. For `work gates` and `work evidence`, top-level `ok: true` means the inspection command completed; callers must read `overall`, `gates_ok`, and each gate `status` to detect blocked work. Receipt `changed_paths` are bounded repo-relative previews collected from `git status --porcelain=v1 -z`; they include untracked filenames but exclude `.agent/**`. These commands print human-readable output by default; pass global `--json` for structured automation output.

`scripts/jig work finish --plan-id ...` fails when any required gate is missing, failed, stale, unknown, or unsupported. Older `work.checks` entries are still accepted for compatibility and backfill missing required check gates during migration. If the same tool is declared in `work.gates`, that explicit gate entry is authoritative.

Fresh check evidence means the non-`.agent/` worktree fingerprint did not change while `work check` ran and still matches the current worktree. Generated outputs should therefore be committed, ignored, or settled before required gates are used as finish evidence. If a check creates expected files, review those files and rerun `work check` to record fresh evidence.

After upgrading an in-flight repo from a Jig version that recorded receipts without `worktree_fingerprint`, rerun `scripts/jig work check --plan-id ...` before `scripts/jig work finish --plan-id ...`. Older receipts deserialize, but their gate freshness is `unknown`.

Unknown non-`check` gate kinds are parsed and reported as unsupported. Required unsupported gates block finish.

## Rollout Rules

Use this sequence for public contract changes:

1. Add the new field, tool, or command in a backward-compatible way.
2. Update `.agent/jig-contract.json.jinja`, runtime dispatch, MCP exposure, and docs in the same change.
3. Keep old fields and commands working for the current contract version.
4. Run the configured release checks before release.
5. Only remove or redefine stable behavior after incrementing `contract_version`.

Generated repos can rely on:

- `scripts/jig` enforcing the exact `jig_version` from `.jig.toml`
- `scripts/jig check contract` detecting missing generated runtime wiring
- stable command keys listed in `required_commands` for command-backed contract versions
- tool availability being discoverable from `.agent/jig-contract.json` and MCP
- state files being runtime-owned append-only records

Generated repos should not rely on:

- private Rust module layout inside `crates/jig`
- unlisted Make targets or project scripts
- undocumented JSON fields
- physical ordering of fields in JSON objects
- SQLx or schema-dump tools unless present in the manifest
- versioned state-file schemas under `.agent/state/*.jsonl`
