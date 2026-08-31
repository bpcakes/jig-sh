# Public Contract

`jig` exposes a repo command contract through three surfaces:

- CLI commands from `scripts/jig`
- MCP tools from `scripts/jig mcp`
- `.agent/jig-contract.json`

Generated repositories declare a `contract_version` in `.agent/jig-contract.json`. During discovery, `scripts/jig` accepts a candidate only when the binary supports that contract epoch and requested build profile; this capability-only probe intentionally does not validate unrelated repository policy. Immediately before ordinary command dispatch, the launcher passes its epoch, selected profile, and repository root through hidden root options so the chosen binary validates the complete repository contract in-process and reuses the loaded context; malformed configuration therefore produces its specific validation error without a redundant startup subprocess or reinstall loop. Those `--__launcher-*` options are a private generated-launcher protocol, not a supported user interface; direct callers should select repositories through cwd or `JIG_REPO_ROOT`. `doctor` and `check contract` retain the capability-only final probe because they must remain reachable to report strict repository validation failures themselves. Product releases remain binary identity, but generated repositories do not pin them. Managed caches are trusted only while their source stamp matches the repository's configured source revision; local-source stamps additionally cover the source checkout contents. When `.jig.toml` is unreadable, capability-only commands may use an existing contract-compatible cache without proving source-stamp freshness so diagnostics remain reachable; mutating commands still reparse their required inputs before applying changes. Explicit `JIG_DEV_BIN` values remain authoritative trusted runtime sources. Reuse of an otherwise compatible binary found on `PATH` is disabled unless `JIG_INSTALL_ALLOW_PATH_BINARY=1` is set, in which case the installer reports the selected absolute path on stderr. Opted-in PATH candidates must have a validated ELF or Mach-O header and pass the direct compatibility probe; shell wrappers are never accepted through this path.

That final strict validation is a deliberate fail-closed launcher boundary and runs once inside the selected process before every ordinary command, including MCP startup. The launcher-provided canonical root is authoritative over inherited `JIG_REPO_ROOT`, and the validated context remains available process-wide if dispatch later crosses a worker thread. Validation expands beyond `check contract`: commands such as `check fmt`, `work`, and `info` do not execute while required contract configuration is invalid. Help, version output, `init`, `presets`, `adopt`, `codex`, `doctor`, `update`, and `check contract` remain reachable through capability-only validation so failures can be explained or repaired and account-scoped Codex homes remain independent of repository policy. Bare `check` and selector-shaped values after it require strict repository validation because contract v6 resolves them against checked-in targets; unrecognized top-level commands still reach Clap without strict validation so their usage diagnostics are not hidden by unrelated repository errors.

`_commit` selects the source revision when the installer must build a runtime; it is not a product-version or binary-provenance lock. Once a binary has proven the requested contract and profile, the cache may reuse it while its recorded configured-source state remains current. Git-backed local-source caches recompute their Git/content fingerprint on every resolution so an edit invalidates the runtime immediately. Non-Git and unborn-Git sources first compare a path, identity, size, mode, and nanosecond timestamp summary; unchanged trees avoid rereading file contents, while a metadata change triggers a stable full-content comparison before reuse. Local source inputs under `Cargo.toml`, `Cargo.lock`, and `crates` may not be symbolic links: links fail closed because changing content behind one does not necessarily change the Git blob or link metadata being fingerprinted. This includes tracked regular files replaced by worktree symlinks, not only symlinks recorded in the Git index. The non-Git fallback also fails closed rather than traversing more than 100,000 entries, 512 MiB of regular-file content, or 128 directory levels; unusually large or symlinked source trees should use regular committed source entries or an explicitly rebuilt `JIG_DEV_BIN`. Launcher-only repair seeds are recorded distinctly with the seeded binary digest, a cheap file-identity key used to avoid rehashing an unchanged binary, and the source state they shadow, rather than pretending the binary was built from that source. Full refresh runtime policy combines the rendered source and harness footprint: minimal harnesses do not seed a removed installer, while full repositories rendered from embedded templates try to publish the running binary with durable embedded-runtime provenance. Cache publication occurs after repository rendering is committed; a failure is returned as a warning and does not misreport the durable render as rolled back, and an existing launcher-repair seed is retained as the last known fallback. Repair provenance across supported contract epochs is retired only after replacement is available or the rendered policy no longer manages an embedded runtime. A changed file identity falls back to the recorded digest check, while a source change invalidates either kind of seeded stamp. Deliberately unpinned mutable-source caches emit a periodic refresh reminder; `--refresh` or `JIG_INSTALL_REFRESH=1` forces the installer to recheck the source. Installer `--resolve-only` calls are read-only: they neither refresh seeded identity/source metadata nor write mutable-source reminder state.

Runtime seeding selects Bash and its helper-command path as one platform policy. Linux and macOS accept only root-owned, non-writable Bash and helper directories, preventing repository-local and unrelated ambient directories from entering the seeding path.

Jig also specifies the separate open [`jig.status-provider/v1`](status-provider.md) protocol. It lets a project-specific inspector, including a closed-source provider, publish software-rewrite observations to Jig or any other consumer through a committed JSON Schema. The status-provider protocol is not a command in `.agent/jig-contract.json`, and its major version is independent of the generated command contract's `contract_version`.

Structured work commands, state hygiene commands, first-run setup, the unified doctor, status aggregation, Codex-home selection, and agent tooling checks are runtime-owned conveniences. They are available through commands such as `scripts/jig setup`, `scripts/jig doctor`, `scripts/jig status`, `scripts/jig work ...`, `scripts/jig state ...`, `scripts/jig codex ...`, and `scripts/jig agent doctor`, and MCP tools named `jig.work_*` and `jig.agent_doctor`, but they are not individually declared in `.agent/jig-contract.json`. Contract v6 is the current compatibility epoch, and versions 2 through 5 remain supported through the legacy repository projection. A runtime may add behavior that repositories in an epoch can ignore, but a breaking CLI, JSON/state, configuration, safety, launcher, dev, or vault change requires a contract bump or an explicit end to support for the affected epoch. Status text, JSON, and TUI modes and the `codex` namespace remain CLI-only.

CLI commands print human-readable output by default. Long-running human-mode commands collect bounded child-output previews, phase changes, and periodic heartbeats while supervised work runs, then make a deadline-bounded best-effort write of that progress to stderr after supervised execution returns and before any restored terminating signal is redelivered. A stalled presentation sink may therefore lose the remaining preview, but it cannot indefinitely delay command completion or signal retirement. Because delivery is deferred, heartbeat wording is historical (for example, a phase “reached 25s”) rather than a claim that it is still running when rendered. The deferred boundary keeps transport backpressure from suspending timeout, cancellation, or cleanup and preserves already-collected progress during ordinary interruption. Pass global `--json` for structured automation output (for example `scripts/jig doctor --json`, `scripts/jig status --json`, `scripts/jig work status --json`, or `scripts/jig work evidence --json`); JSON mode disables that human progress output. Usage and pre-output command failures in JSON mode write one object to stdout with `ok: false`, `error.kind` (`usage` or `command_failed`), `error.message`, and `exit_status`, while preserving the nonzero process status. Commands that already emitted JSON do not append a second error document, and `scripts/jig mcp` always reserves stdout for MCP framing. `scripts/jig prompt get` prints the bare rendered body without `--json` and the standard `prompt get` command envelope with it. `scripts/jig status --tui` is an explicit interactive consumer and conflicts with `--json`; it requires terminal stdin and stdout. For other commands, output selection is independent of interactivity: `--json` does not suppress terminal prompts. For init automation, `--defaults` applies documented project-shape defaults but can still prompt for initial vault setup; supply `JIG_VAULT_PASSPHRASE` or `--no-vault` when that must be noninteractive. `--no-input` and implicit non-terminal execution require an explicit complete shape such as `--preset harness-only`; stored `harness_footprint = "minimal"` is also a complete harness-only shape. `scripts/jig work start --print-plan-id` remains a shell-capture override that prints only the new plan id. Human text, TUI presentation, and `--print-plan-id` output are for terminal use and are not stable machine-readable contract output; automation should pass `--json` or use MCP tools.

Contract v4 introduced structured runtime identity through `runtime_version`, and later epochs retain it. The former `jig_version` key remains as a compatibility alias in `info`, `doctor`, and UI snapshots: it contains the legacy generated pin for v2/v3 repositories and is `null` for v4 and later repositories. Doctor runtime data likewise retains deprecated `current_version`, `launcher_version`, and `config_jig_version` aliases alongside the clearer epoch-aware fields.

`scripts/jig codex homes --json` returns a runtime-owned `schema_version: 1` report of local Codex home paths, account identity, plan type, and per-home errors. A home's `status` records stable account state such as `not logged in` or `unknown`; `inspection_error` records account/app-server inspection failure, while `usage_error` records a rate-limit failure after a logged-in account was observed. Both are mirrored in the top-level `errors` array with distinct `kind` values. A logged-out home is a complete observation even when usage was requested: rate-limit usage is not applicable, so an app-server usage failure is not surfaced as `usage_error` and does not change `outcome` to `partial`. Add `--usage` to include every rate-limit bucket and the server-reported durations and reset times. If account inspection succeeds for a logged-in account but usage inspection fails, the report retains the account and records the usage failure. Account and usage data come from the Codex app-server API; Jig does not parse `auth.json`. This output contains local paths and account email addresses and should be handled as user data. Its top-level `representation_lossy` boolean reports whether any non-UTF-8 home path or Codex executable value had to be replaced for JSON display. `scripts/jig codex launch HOME --dry-run --json` and `scripts/jig codex resume SESSION_ID --dry-run --json` are `schema_version: 1` structured launch previews; the latter reports `command: "codex resume"` and includes the injected `resume` and normalized session-ID arguments before caller-supplied arguments. Their `representation_lossy` boolean additionally covers forwarded arguments. Human terminal previews replace control characters before display and explicitly warn when the shown shell command is therefore not launch-equivalent; JSON retains the original string values except for reported non-UTF-8 conversion. A real launch or resume replaces the Jig process on Unix and therefore rejects `--json`; forwarded Codex arguments begin after `--` and retain their original boundaries. On platforms without process replacement, Jig waits for Codex and exits with Codex's shell-compatible status.

`scripts/jig status --json` returns a runtime-owned aggregate with `schema_version: 1`. Its top-level sections are `repository`, `work`, `loops`, `providers`, and `errors`; accepted provider documents remain independently discriminated by `protocol: "jig.status-provider/v1"` under `providers[].report`. Top-level `ok: true` means inspection completed, while `outcome` is `complete` or `partial`. Provider failures are data with `providers[].status = "failed"` and `report: null`, so the command can still return an inspectable snapshot. Domain blockers, stale or dirty inputs, and blocked gates are observed facts rather than collection errors. The aggregate schema version is independent of both provider `protocol` and generated `contract_version`.

`scripts/jig info --commands --json` returns the runtime-owned command-availability inventory with `command: "info commands"` and `schema_version: 3`. Its `commands` array follows the visible root-command order and each entry contains `name`, `category`, `status`, `reason_code`, `reason`, and `next_step`. Schema version 3 adds the backend-neutral `migration` command family; schema version 2 grouped migration authoring under `sqlx`, and schema version 1 described the legacy flattened roots. The stable status values are `ready`, `not_configured`, `needs_setup`, and `unavailable`; reason codes are stable within a schema version, while human-facing reason and remediation text may improve without a schema-version change. Status describes whether the root command's primary workflow can dispatch, not whether every argument combination or command-specific preflight will succeed. Setup, status, stop, and diagnostic subcommands or flags can therefore remain usable when the root entry is not ready. Ready entries have null `reason_code`, `reason`, and `next_step` fields.

`repo.context_status` is stable within command-inventory schema version 3: `valid` means strict repository lookup succeeded, `absent` means no repository was found, `invalid` means strict lookup failed without recovering a current repository, and `recovered` means the explicit context was invalid but tolerant lookup found a valid current repository. This field classifies repository lookup only; consumers must use `commands[]` as the authoritative command-availability result because different invalid-context cases can leave different context-tolerant commands usable. Producing the observational inventory is successful even when `repo.context_status` is `invalid`, so callers must inspect `repo.context_status` and `commands[]` rather than treating exit status alone as repository health.

The built-in `noop-status` workflow keeps `loop` ready without configured custom workflows. In a valid adopted repository, the proxy family is ready when either the current binary includes dev-proxy support or an executable full-footprint `scripts/jig` plus `scripts/install-jig.sh` launcher chain can route `dev` and `proxy` through its feature-enabled profile. It remains ready without configured dev apps because its primary ad-hoc run, alias, certificate, service, and diagnostic workflows do not require dev-app configuration; `jig doctor` separately reports whether dev-proxy integration is configured for the repository. Before adoption, the primary `proxy run` workflow remains `needs_setup`, while contextless status, cleanup, certificate, and service diagnostics may still work. The inventory reports other commands that can run without a repository and marks repository-dependent primary workflows `needs_setup` with `reason_code: "repo_context_unavailable"`. When a repository is discovered but its configuration or generated contract is invalid, commands whose dispatch consults optional repository context are also marked `needs_setup`, even if they can run when no repository exists. In valid context, `repo.context_error` is null; in fallback states, `repo.name` and `repo.root` are null and `repo.context_error` contains the load diagnostic. The inventory is read-only. Vault and Codex readiness are machine-local observations: when Codex marketplaces are configured, collection reads the local Codex configuration and may spend up to five seconds probing the configured Codex binary.

The schema-version 3 `reason_code` values are `agent_readiness_unknown`, `bootstrap_tool_invalid`, `bootstrap_tool_missing`, `codex_marketplace_support_unavailable`, `codex_marketplace_unregistered`, `dev_apps_not_configured`, `dev_proxy_feature_not_built`, `migration_add_tool_invalid`, `migration_add_tool_missing`, `migration_backend_not_configured`, `migration_directory_not_configured`, `repo_context_unavailable`, `sqlx_disabled`, `vault_not_initialized`, and `vault_status_unavailable`. Schema version 2 omitted `migration_backend_not_configured`; schema version 1 additionally used `schema_dump_tool_invalid`, `schema_dump_tool_missing`, and `schema_dumps_disabled` for the former root-level schema entry. The stable category values are `get_started`, `develop`, `structured_work`, `project_data`, `local_services`, and `agent_automation`.

An invalid or stale `JIG_REPO_ROOT` remains a blocker for workflows that use strict repository lookup, including the primary `proxy run` workflow. Workflows using tolerant optional-context lookup instead ignore the invalid override, quietly try the current directory, and fall back to no repository when appropriate. When that lookup recovers a valid current repository, the inventory uses it for `dev` and vault readiness even though `repo.name` and `repo.root` remain null because the explicit override is invalid.

Bootstrap command JSON is also runtime-owned. `scripts/jig init --json`, `scripts/jig adopt --json`, and `scripts/jig update --json` include a `render_report` object that summarizes created, modified, unchanged, conflict, backup, managed-block, and todo items for human review. When `jig init --json` runs a project scaffold, its sibling `scaffold` object reports the scaffold preset, sanitized `repo_name`, nullable `repo_name_sanitized_from`, `db`, `frontends[].{name,dir,kind,role}`, `frontend_notices` for bare custom names that are not preset shorthands, and `files_created` / `files_modified` / `files_unchanged` separately from template-managed file counts in `render_report`. Generated shadcn React frontends carry a same-contract-epoch `ui` provenance object describing the system, CLI version, preset, primitive base, style, and Tailwind major. `scripts/jig adopt` previews by default with `render_mode = "preview"` and only applies managed files with `render_mode = "copy"` when `--write` is supplied. `scripts/jig init`, `scripts/jig adopt`, and `scripts/jig update` print human summaries by default; pass `--json` for the full structured reports. Automation should treat those reports as runtime diagnostics governed by the contract epoch, not as tool entries in `.agent/jig-contract.json`.

Agent-guide check JSON keeps `missing_guides` as an empty compatibility field in this contract version and includes `missing_guides_note` to explain that placeholder backend-level `AGENTS.md` files are no longer required. Existing Rust crate and Go package guide files are validated when present. Consumers should stop treating `missing_guides` as the guide-coverage gate; use `missing_sections` and `missing_entry_ref` for existing-guide quality issues.

Dev proxy and vault JSON are also runtime-owned. Proxy status may include machine-local health fields such as `pid`, `pid_alive`, `pid_observation`, `health_pid`, `handshake_ok`, `pid_matches_proxy`, `running`, listener addresses, and route URLs; `pid_alive` means positively observed alive while `pid_observation` preserves an `alive`, `absent`, or `uncertain` result. Status and listing commands may perform a loopback HTTP health probe to populate those fields. Strict cross-machine automation should rely on the stable generated command contract instead of treating those runtime diagnostics as a contract schema.

The structured work namespace includes native check gates. Gates are configured in `.jig.toml`, evaluated from receipts, and enforced by `scripts/jig work finish`. They remain runtime-owned because they compose stable execution tools with append-only work state rather than adding new generated contract tools.

`scripts/jig work gates` and `scripts/jig work evidence` include the current source/worktree fingerprint in their runtime-owned JSON so humans and agents can tell whether receipts are fresh. The token covers the committed non-`.agent/` source tree plus non-`.agent/` staged, unstaged, and untracked state. Commits containing only `.agent/` state therefore preserve source freshness. The token is an opaque same-contract-epoch comparison value, not a stable public hash contract; consumers should compare it for equality only while the repository remains on the same contract epoch.

Local development proxy commands are also runtime-owned. `scripts/jig dev`, `scripts/jig dev status`, `scripts/jig dev stop`, and `scripts/jig proxy ...` manage machine-local processes, ports, routes, certificates, and optional user services. They are configured from `.jig.toml` but are intentionally absent from `.agent/jig-contract.json` because they do not represent repository checks.

Runtime-owned local development commands include `dev`, `dev status`, `dev stop`, `proxy start`, `proxy stop`, `proxy list`, `proxy prune`, `proxy run`, `proxy alias`, `proxy cert generate`, `proxy cert status`, `proxy cert trust --accept-trust-scope`, `proxy cert untrust --accept-trust-scope`, `proxy service install --accept-service-scope`, `proxy service status`, and `proxy service uninstall`. Bare `dev` launches apps, while its `--replace` option retires only conflicting registered sessions owned by the same canonical repository; it is not a general process takeover option. Foreground `dev` and `proxy run` interruption is structured same-contract-epoch output with `interrupted`, numeric `exit_signal`, named `termination_signal`, and shell `exit_status`; SIGINT, SIGHUP, and SIGTERM map to 130, 129, and 143 on Unix. Builds made with `--no-default-features` keep the contract, MCP, and work-receipt runtime but return clear errors for every `dev` action and `proxy`; the launcher profile probe prevents such a binary from serving `dev` or `proxy` execution.

Dev-session JSON is same-contract-epoch runtime output. `dev status` reports the canonical repo identity, resolved state directory, aggregate `running` state, sanitized registered sessions with explicit process observations and spawn tracking, durable `preflight_cleanup_pending` evidence, and a `recoverable` state for dead orphans. Aggregate `running` is true only when at least one session is neither stale nor recoverable; a recoverable record therefore leaves `running` false but remains in `sessions` until explicit cleanup, so callers deciding whether cleanup is needed must inspect the session list. `dev stop` reports matched/stopped session and app counts, any sessions that remain, blocking `warnings`, and structured successful `recoveries`; if a later stop operation fails, its `ok: false` result retains warnings and recoveries already produced and omits completion counts that could not be confirmed. A foreground `dev --replace` result also includes recoveries completed before the new session was claimed, including when replacement subsequently fails or is cancelled. Each recovery preserves diagnostic app names, targets, spawn states, last-known PIDs, and any explicitly forgotten ambiguity after the registry entry is removed. Failed dev and dev-stop results that include recovery or cleanup metadata use the standard `error.kind: "command_failed"` and `error.message` object rather than changing `error` to a string. Stop is successful and idempotent when no session matches, recovers an orphan once cleanup evidence is complete and every exact registered identity is absent, and returns `ok: false` when preflight cleanup is unconfirmed, spawn state is pending or unknown, or a registered process remains live or uncertain. `dev stop --forget-ambiguous-orphans` is an explicit repair for dead-supervisor records blocked only by unconfirmed preflight cleanup or pending or legacy-untracked spawn evidence; it still returns `ok: false` for live or uncertain registered identities and records that an unrecorded process may remain. Neither response exposes the persisted session-control credential, and management never signals from persisted PID data.

Local vault commands are runtime-owned as well. The surface includes init/status/audit, an explicit keyboard-first TUI, explicit format migration, field and compatible secret management, controlled read/inject, transparent exec, constrained run, one-time 1Password import, passphrase change, and encrypted backup/restore. Generated repos carry non-secret `[vault]` scope metadata in `.jig.toml`; when present, vault commands default to that repo scope rather than the user-level global vault. A canonical `jig://ITEM/FIELD` reference is relative to that selected scope and never embeds or overrides the project. These commands are intentionally absent from `.agent/jig-contract.json`, MCP tool listing, and repo-local command receipts because local values and child output must not be persisted into `.agent/state`.

`vault tui` is terminal-only, rejects `--json`, and fixes one resolved scope for its process lifetime. Ordinary frames, activity, errors, and action results contain authenticated metadata only. Private-file export and the exact-confirmation Peek path are controlled reveal sinks: Peek bypasses Ratatui, terminal-safely escapes and bounds the displayed source prefix, then clears the alternate screen before metadata redraw. Its deliberately disclosed window may still be retained by terminal scrollback, multiplexers, remote transport, or recording. The process-local credential is removed by explicit or five-minute idle lock and on authentication/audit failure; this is not a clipboard feature, unlock daemon, remote service, or contract/MCP surface.

Vault JSON is runtime-owned same-contract-epoch behavior, not an individually declared manifest tool schema. Structured vault responses contain metadata only, never field values. `vault status` currently reports both `exists` and `vault_file_exists`; both mean the encrypted `vault.json` file exists, not that the vault home directory exists. Structured responses report `vault_scope`, `vault_scope_id`, and `vault_repo_name`; the latter two are null when not applicable. Current `vault_scope` values are `repo`, `global`, `legacy`, and `explicit-home`. Field/import results expose references, kinds, counts, and create/replace actions. Passphrase change reports completion metadata; backup create reports byte count, backup version, and creation time; restore reports the installed vault home, vault ID, and format version. None returns passphrases, field bytes, backup plaintext, or external resolver diagnostics.

`vault read` and `vault inject` are exact-byte output commands. They reject global `--json`, bypass the normal structured emitter, and write only to their controlled stdout or private-file sink. A private-file destination must be outside the selected vault home and must not alias `vault.json` or `audit.jsonl`; an existing regular destination additionally requires explicit overwrite authorization. `vault exec` likewise rejects `--json`, transparently streams independently redacted stdout and stderr, and exits with the child's status without a second Jig error. Its dotenv source, and the corresponding 1Password import source, must be a non-symlink regular file rather than a FIFO, device, directory, or other special file. In contrast, compatible `vault run` returns mapping counts plus buffered, redacted, lossy UTF-8 `stdout` and `stderr` strings and raw process status fields; automation should use `result.exit_signal` to distinguish signal termination when present and otherwise branch on `result.exit_status`.

Current Jig reads both vault envelope versions. Version 1 values are treated as concealed and remain available to listing, reveal, injection, run, and exec; field mutation, import, passphrase rotation, and backup require explicit one-way migration to version 2. Older Jig rejects version 2. On version 2, `vault secret` remains compatible vocabulary over concealed fields, `vault run` retains constrained broker semantics, and `vault exec` is the separate transparent wrapper. See the [Vault Runtime compatibility matrix](configuration.md#references-fields-and-format-compatibility) for the operator-facing policy.

LAN mode exposes the Jig proxy listener to the local network, not child app listeners directly. Process routes may be reached from other devices only through the proxy, with the original routed hostname in DNS, a hosts file, or the HTTP `Host` header. Alias routes stay loopback-client-only so LAN clients cannot use Jig as an open forward proxy.

The `tool_defs::cli_command` names for these runtime-owned commands are parser labels only. They do not add generated tools to `.agent/jig-contract.json` and do not expose MCP tools for proxy process or service management.

Because the local development proxy and local vault are runtime-owned, their detailed JSON response fields, machine-local state layouts under `JIG_PROXY_STATE_DIR` / `~/.jig/proxy` and `JIG_VAULT_HOME` / `~/.jig/vault`, service-file contents, certificate files, vault/backup envelope formats, route hostname format, and nonzero error exit statuses are not individually enumerated in `.agent/jig-contract.json`. The vault audit JSONL is HMAC-chained but plaintext local metadata; field names, environment variable names, timestamps, run IDs, and vault IDs are not opaque payload. It detects edits and broken links but is not remote or independent evidence of deletion, truncation, or rollback. Breaking generated-repository assumptions in these surfaces requires a contract-epoch change even though compatible additions do not require new manifest fields.

The current explicit acknowledgement flags, including `--accept-trust-scope` and `--accept-service-scope`, are runtime safety gates rather than generated contract fields. Automation should use a launcher-selected binary that supports the repository contract; removing or weakening those required flags is a breaking contract-epoch change.

Runtime-owned `.jig.toml` sections are intentionally strict: unknown keys are rejected so local typos fail fast. New optional keys in `[work]`, `[loop]`, `[[loop.workflows]]`, `[status]`, `[[status.providers]]`, `[execution]`, `[agent_tooling]`, `[agent_tooling.codex]`, `[dev]`, or app tables require a Jig runtime/template update and a documented migration note. The `[execution]` keys are backward-compatible in contract v4 through v6: omission defaults `command_timeout_seconds` to 1,800 seconds and `command_output_limit_bytes` to 67,108,864 bytes for configured commands. Internal protocol commands and Codex worker transcripts retain separate fixed limits. Any addition or change that makes an existing repository unreadable or changes generated behavior incompatibly requires a contract bump. Loop workflow keys `schedule`, `timezone`, `prompt_file`, `model`, `sandbox`, and `checkout`, the compiled `codex_task` kind, and the `loop dispatch` CLI are additive runtime behavior for supported legacy repositories; no generated MCP tool is added. A `pr_manager` or `codex_task` workflow may set `codex_home` to choose the exact `CODEX_HOME` for its unattended `codex exec` worker; omission inherits the caller environment for compatibility. Bare names resolve only to their conventional home-directory locations, while non-conventional homes require explicit paths. Same-contract-epoch loop JSON preserves the input as `codex_home_configured`; repair-attempt and task-worker actions and receipts report the canonical worker directory as `codex_home_resolved` when resolved, while actions that do not attempt work omit that field.

## Contract Version

`.agent/jig-contract.json` has these schema versions:

- `contract_version`: version of the generated tool manifest and command surface

Version `2` is the legacy root-check command-backed contract. Version `3` groups checks under `scripts/jig check ...`. Both legacy epochs require matching `jig_version` fields in `.jig.toml` and the manifest as an internal consistency check, but a compatible runtime does not compare its own product release with that value. Version `4` removes generated product-version fields and makes `contract_version` the whole-harness compatibility epoch. Version `5` adds the strict `backend_language`, `go_database`, and backend-neutral `migration_dir` configuration selectors. Version `6` replaces the singular runtime stack identity with explicit components, actions, profiles, and adapter provenance. Its generated `.jig.toml` records the authored model under `[repository]`, while `.agent/jig-contract.json` records the matching resolved model. Rust, Go, SQLx, Go/PostgreSQL, and TypeScript capabilities are adapter contributions; command keys are component-scoped, such as `api_test_command` and `web_test_command`. Versions 2 through 5 remain readable through the legacy catalog projection. An unmigrated v2/v3 wrapper remains runtime-readable but intentionally fails Doctor's required launcher-shape check; Doctor recommends a full `update --force` first when the repository has intact ownership metadata, with `update --launcher-only --force` reserved as the narrow recovery step when the legacy wrapper cannot start or full ownership is not yet established. That narrow repair leaves the repository on its supported legacy epoch and seeds the proven repair runtime; afterward Doctor exposes migration to the current contract as optional follow-up because the legacy recorded source may not be able to recreate that seed. A compatible change may add optional manifest data, tools, commands, or runtime behavior that older readers in the same epoch can ignore. Strict generated configuration additions and other breaking changes must increment `contract_version` before generated repositories depend on them.

Breaking `contract_version` changes include:

- removing or renaming a stable generated tool
- removing or renaming a stable generated command key
- changing a stable command argument from optional to required
- changing the meaning or type of a stable JSON request or response field
- changing `.agent/jig-contract.json` in a way older runtimes cannot ignore
- making an existing generated configuration, launcher protocol, state stream, safety flag, dev, or vault behavior incompatible

## Stable Manifest Fields

Generated repos and MCP clients may rely on these top-level fields in `.agent/jig-contract.json`:

- `contract_version`
- `tool_namespace`
- `required_commands`
- `tools`
- `components`, `actions`, `profiles`, and `default_check_profile` for version `6`

Each tool entry has these stable fields:

- `name`
- `kind`
- `description`
- `command` for `kind: "command"` tools

For `kind: "command"` tools, `command` is the `.jig.toml` `[commands]` key the runtime executes from the repo root. In version 6 these tools are compatibility aliases over component actions rather than the primary execution model.

Command-backed contract versions intentionally have no `optional_commands` field. A command-backed tool is valid only when its command key is listed in `required_commands`; optional capability is represented by omitting the tool entirely when the rendered repo profile does not support it.

Consumers should ignore unknown top-level manifest fields and unknown fields inside tool entries. Jig includes unknown top-level fields in the canonical execution-authority digest even when the current runtime assigns them no behavior, so forward-compatible authority cannot change without invalidating existing plans and evidence.

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
- `jig.migration_add` when `rust_migration_layout` is `flat_migrations`
- `jig.schema_check` when schema dumps are enabled
- `jig.schema_dump` when schema dumps are enabled

SQLx-specific tools are stable when `sqlx_enabled` rendered them into the manifest:

- `jig.sqlx_check`
- `jig.schema_check`
- `jig.schema_dump`
- `jig.migration_add` only for `flat_migrations`; `versioned_artifacts` contracts omit it

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

## Repository Catalog And Check Plans

The runtime exposes one normalized repository catalog independently of the
persisted contract epoch. For contracts 2 through 5, every declared manifest
tool is projected as an action on a synthetic component whose structured id is
`repo`; the original tool name remains a compatibility alias. Known action ids
match the CLI vocabulary, for example `jig.test` becomes `repo:test` and
`jig.fmt_check` becomes `repo:fmt`. Alias collisions from custom legacy names
receive deterministic digest suffixes. The projected default verification
profile includes only read-only check actions. Historical effectful tools that
were configured as `kind: check` gates, notably `jig.schema_dump`, remain
addressable actions and retain direct legacy work-gate behavior but are omitted
from bare repository checks.

Contract 6 reads the resolved records directly. A component declares its root,
adapters, dependency/affected policy, guidance, and field provenance. An action
declares a structured target, intent, effects, configured or native runner,
inputs, execution dependencies, timeout, result parser, compatibility aliases,
and provenance. Profiles contain exact structured targets. Runtime loading
rejects drift between the authored `[repository]` records and the resolved
manifest; it does not rediscover stacks or files during execution.

The following read-only info commands return `schema_version: 1` plus structured
component, target, or profile records:

- `jig info workspace`
- `jig info components` and `jig info component COMPONENT_ID`
- `jig info targets` and `jig info target COMPONENT_ID:ACTION_ID`
- `jig info profiles` and `jig info profile PROFILE_ID`

Target identity is always an object with separate `component` and `action`
fields in JSON. Human output renders its canonical `component:action` text.

`jig check --explain` returns `command: "check plan"`, `executed: false`, and a
`plan` object without running a command or writing a receipt. A plan uses run
plan schema version 2 and includes its derived `id`, configuration digest, source identity,
normalized selectors or profile, sorted targets, selection reasons, declared
effects, input digests, and dependency execution layers. Bare `jig check` uses
the default verification profile. An action selector such as `test` matches
that action across components; a target selector such as `api:test` is exact;
and `*` is the only wildcard and occupies a whole component or action segment.
Profiles and explicit selectors are mutually exclusive. Contract-6 legacy
aliases must not parse as canonical action, target, or wildcard selectors;
canonical selector meaning therefore cannot be shadowed by an alias.

The configuration digest canonicalizes repository execution authority: the
parsed generated contract model, effective command bindings, backend migration
settings, and configured execution limits. Comments, formatting, and unrelated
runtime settings such as local development ports do not change that digest.
The separate source identity remains a conservative snapshot of all
non-`.agent/` repository source, so editing `.jig.toml` still requires planning
again even when the resolved execution authority is unchanged.

On contract 6, `--affected BASE` narrows that ordinary selector/profile
candidate set. Jig safely resolves the explicit Git revision, compares the
merge base with `HEAD`, unions staged, unstaged, and untracked paths, excludes
all `.agent/` harness/runtime metadata, and sorts the result. Ignored `.env` and
`.env.*` files beneath directories that are not themselves ignored participate
in source identity because they can change command behavior; Git provides no
baseline for their contents, so their presence is conservatively treated as a
local affected path. Wholly ignored directories are pruned as generated trees;
repositories must unignore a containing path when it holds an intentional
dotenv input. The generated
contract remains part of the separately canonicalized configuration digest.
Repository-relative action input globs identify
directly affected components, including explicit inputs outside a component
root; when no input matches a path, the most-specific containing component root
is used. A `.` component with explicit action inputs is not a catch-all owner
for paths outside those inputs; a root component without inputs may still own
the repository fallback. If no declared input or eligible component root claims
a changed path, every candidate is retained with an `unclaimed_input` reason so
affected execution fails closed. Contract-v6 repositories may remove reviewed,
non-impacting paths first with `repository.affected_ignore`; patterns matching
`.jig.toml` or `scripts/jig` are rejected, and explicit action inputs take
precedence so an ignore cannot shadow a declared dependency. Generated policy
classifies named repository guidance, documentation, license files, hosted-CI
metadata, and dotenv presence as non-selecting unless an action declares them;
arbitrary fixtures plus build and source-discovery authority remain
fail-closed. Reverse component dependencies propagate only under the checked-in
`propagate_affected_to_dependents` policy. Action dependencies expand afterward.
Every retained target records a deterministic preview of its candidate and
affected reasons. The preview is capped at 100 reasons per target; when more
exist, `selection_reason_count`, `selection_reasons_truncated`, and
`selection_reasons_digest` describe the complete sorted set without allowing a
large change list to amplify the durable queued-run record. Intent and
dependency reasons take preview priority over path-expanded detail. A comparison
with no relevant changes can still produce a valid empty plan. Contracts 2 through 5 reject
affected planning because their projected catalogs have no inspectable input or
propagation policy.

Executing a planned selection on a legacy contract returns an aggregate check
response with `command: "check"`, `executed: true`, the exact plan, per-target
legacy tool responses, a terminal `run`, structured `failed_targets`, and
`source_observations` with the execution-phase fingerprint scan count and
elapsed milliseconds. Foreground execution streams target phase, output, and
heartbeat events while retaining the same bounded output in its result.
Before execution the runtime deterministically resolves the reviewed request
again and rejects a stale or modified plan without creating state. Existing
named v2–v5 check commands without planning flags retain their prior single-tool
response. `--fail-fast` is explicit; aggregate selection otherwise collects
every target failure it can execute. Receipt options are accepted on either
side of external target selectors.

Every planned execution appends lifecycle events to `runs.jsonl`: one queued
event owns the accepted immutable plan, followed by running/target events and
exactly one terminal conclusion. `jig status run RUN_ID` returns that plan and
the current folded `RunResult`. Target conclusions are independent of lifecycle
status and use `success`, `failure`, `cancelled`, `timed_out`, `blocked`, or
`skipped`. Unknown future run event names are ignored; malformed known
lifecycle transitions fail closed.

Run ids are durable inspection handles, so Jig does not silently expire their
events. Exact run lookup scans the journal backward to the requested run's
queued event and materializes only matching lifecycle records. Explicit
`state archive --before ... --include-runs` maintenance moves completed old
runs out of the active journal while retaining completed
runs linked to an open work plan. Run archival is opt-in so the established
receipt-only archive command does not unexpectedly remove inspection handles.

## MCP Repository Operations

Contract v6 advertises four repository operations rather than one MCP tool per
action:

- `jig.inspect` reads the workspace, component, target, and profile catalogs or
  one durable run. Its `kind` discriminator determines whether `id` or `run_id`
  is required.
- `jig.plan_run` resolves explicit `selectors`, a mutually exclusive `profile`,
  optional `affected_base`, and closed per-target `arguments` through the same
  deterministic planner as the CLI. Effectful actions require explicit
  selectors. Native actions that need a name bind it into the immutable plan;
  unsupported or unselected-target arguments are rejected. Planning does not
  execute or write run state.
- `jig.execute_run` accepts the exact returned plan plus optional
  `work_plan_id`, `record_receipts`, and `fail_fast` controls. Plans containing
  `worktree` or `external` effects also require an exact `approved_effects`
  acknowledgement. It validates the plan and approvals again, creates durable
  queued state, and returns an accepted run handle without waiting for target
  execution.
- `jig.cancel_run` durably records an idempotent cancellation request. The
  owning worker observes that event even when it came from another MCP process;
  an in-process registry also signals the owned process tree immediately.

All four descriptors contain strict input and output JSON Schemas and reject
unknown input fields. Their successful MCP responses put the canonical object
in `structuredContent` and include a text rendering for compatibility. After
`jig.execute_run` returns, clients poll with
`jig.inspect {"kind":"run","run_id":"..."}` until the run status is
`completed`; each target then has its own terminal conclusion. A failed,
cancelled, timed-out, skipped, or blocked target is inspectable execution data,
not an MCP protocol error. Invalid arguments, an unknown identity, a stale or
modified plan, corrupt durable state, and failures before a durable handle is
accepted use the MCP error response. If an accepted background worker later
encounters an internal infrastructure failure, Jig best-effort closes its
unfinished targets and run with the `blocked` conclusion so polling does not
silently strand a live-looking handle.

Contract v6 manifest tools are compatibility aliases and are not individually
advertised or callable over MCP. Contracts v2 through v5 retain their existing
per-manifest-tool discovery, calls, and response shapes. The bounded
`jig.work_*` lifecycle tools remain available in every supported epoch.

## Runtime State

`.agent/state/*.jsonl` is runtime-owned append-only memory during normal operation. Generated repos may back up, inspect, or remove these files intentionally, but application code should not edit individual records in place. Runtime-owned maintenance commands may perform validated whole-stream rewrites with recovery artifacts. Generated `.gitattributes` marks those JSONL files with `merge=union` to reduce avoidable merge conflicts between independent append-only records.

Current JSONL state files:

- `sessions.jsonl`
- `plans.jsonl`
- `receipts.jsonl`
- `decisions.jsonl`
- `runs.jsonl`

State readers should tolerate missing files by treating them as empty. JSONL readers should ignore blank lines and fail loudly on malformed nonblank records. Session-start records retain their durable write-time summary, but `summary.recent_sessions` contains shallow event references whose nested `summary` is `null`; historical records that recursively embedded older summaries remain readable. Canonical session readers collapse duplicate IDs with identical event envelopes, as can arise after a line-union merge, and reject the same ID with a conflicting envelope.

Receipt records may include an `evidence` object for structured runtime-owned evidence that does not fit safely in truncated stdout or stderr previews. A target receipt additionally carries optional `run_id`, structured `target`, `config_digest`, `input_digest`, and normalized `findings`; older records deserialize with those fields absent. Receipt Git metadata excludes `.agent/**`; `changed_paths` contains at most 100 sorted paths, while optional `changed_path_count`, `changed_paths_truncated`, and `changed_paths_digest` describe the full path set. Successful stdout and stderr previews use a 512-byte truncation threshold and failed previews use a 4,000-byte threshold. Configured-command timeout, await, cleanup, and capture failures use `evidence.kind = "supervised_command"`, `status = "error"`, and retain the diagnostic in the failed stderr preview. Cancellation after spawn uses the same evidence kind with `status = "cancelled"`; cancellation before spawn records no child receipt, and a work-check batch references only children that actually started. Older receipts without the new evidence or path-summary fields remain readable. A Codex worker receipt uses its separately bounded last-message file as authoritative `stdout_preview`; provider stdout is diagnostic transcript data in additive `evidence.provider_stdout_preview`. `provider_stdout_preview_truncated` reports bounding of that evidence preview, and `provider_stdout_truncated` reports truncation by the process supervisor. The legacy additive `stdout_truncated` evidence field remains an alias for provider-transcript truncation, while `stderr_truncated` continues to describe provider stderr. Codex review receipts use `evidence.kind = "codex_review"` and store normalized findings there, capped to the first 100 findings with long finding fields shortened; raw finding and actionable counts remain available so truncation does not hide a failing gate. Their receipt `exit_status` is the gate verdict, while `evidence.codex_exit_status` is the underlying Codex process status. They also include short stdout/stderr previews for failed review debugging. Codex refinement receipts use `evidence.kind = "codex_refine"` and store the refinement iteration, optional refinement profile metadata, reviewed gate ids, finding fingerprints, and finding count.

The active-session pointer is cache state, currently resolved through git as `jig-current-session.txt` and falling back under `.agent/.cache/`. Generated repos should not treat that path as a durable JSONL record.

Git repositories carry an additive schedule-initialization witness in the checkout's worktree-specific Git metadata. This authority is outside a Codex `workspace-write` worker's writable surface, so deleting every checkout-local schedule marker cannot make a previously initialized ledger look new or permit the same occurrence to rerun.

PR-manager branch-lease loss is phase-sensitive but always fail-closed around a prepared checkout: loss after a worker failure retains even a clean checkout, while loss before worker start retains the checkout rather than force-removing a path that a new lease owner may be using. The latter remains typed as unexecuted but requires occurrence attention because cleanup authority is ambiguous.

Scheduled loop occurrence state is mutable machine-local runtime state under `.agent/runtime/loop/`, not append-only agent memory and not disposable cache. Preserve it with the checkout used by an external scheduler. Retained task worktrees also live below `.agent/runtime/loop/` so cache cleanup cannot destroy reported work, while leases and retry attempts remain under `.agent/.cache/loop/`. Before publishing any durable occurrence claim, dispatch fails closed on unparsable lease JSON and resets unparsable attempt JSON with additive `attempts_reset` state evidence; other cache failures also fail closed, and corruption observed after workflow work begins remains explicit state-error evidence. A setup failure or cancellation after a claim but before worker start removes the unexecuted claim and reports an additive typed retryable pre-execution action. If cleanup retains a checkout, the occurrence instead becomes `needs_attention` with that path. Workflow-lease finalization requires the same unexpired owner under the cache lock; ownership loss after execution begins makes the durable occurrence require attention instead of reporting clean success. A current runtime migrates schema-1 cache and schema-2 or schema-3 durable occurrence ledgers to schema 4 before dispatch. Schema 4 records whether each new occurrence uses the shared checkout; older markerless `running` or `needs_attention` records are conservatively treated as potentially shared until they are finalized or acknowledged. Earlier runtimes reject the resulting ledger and migration marker, preserving the existing downgrade barrier. A durable `schedule.initialized` marker beside the ledger preserves the fail-closed initialization fact even when disposable cache is removed. Operators must stop older dispatchers during that writer cutover and must not downgrade after schema 4 is published. Dispatch keeps ambiguous scheduled occurrences and exhausted per-item attempts as separate, nonduplicated attention sources because they require different repair commands, derives its unsuccessful status from either source, and records cancelled or failed post-work state observations in the dispatch receipt. The durable claim transaction rejects older work after a newer occurrence is recorded, blocks overlapping workflow or shared-repository scope while an occurrence is still `running` or requires attention, and enforces retained-worktree backpressure without a stale dispatch snapshot. Status and acknowledgement share the same claim-expiry predicate, so direct acknowledgement atomically reconciles an expired `running` record before terminalizing it; acknowledgement unblocks the coalesced next due instant. Attempt repair uses the exact persisted workflow and item keys, including after a workflow is removed or renamed; schema-version-1 clear-attempt evidence keeps `workflow` as an object and adds `workflow_id` as its explicit string key. Schema-version-1 dispatch evidence keeps `skipped_count` as the broad number of due occurrences not executed, while additive `deferred_count` identifies authority contention, including a held workflow lease or overlapping live occurrence. `loop tick` and `loop run` also treat machine-global `needs_attention` as unsuccessful even when a workflow selector points at a different workflow; selectors choose work, not the scope of runtime-health reporting. Status uses one sampled clock for schedule and attempt classification. A status schedule-evaluation error is scoped to its workflow and top-level `state_errors`, so other loop state remains inspectable. Codex task checkout fails before starting either repo or isolated mode unless Git confirms that `.agent/runtime/loop` is ignored; isolated mode also verifies its task-worktree root. Stale adopted repositories can refresh the managed rules with `scripts/jig update --recopy`. A retained isolated task worktree blocks another scheduled claim for the same workflow until the operator removes it, bounding evidence growth without automatic data loss.

Manual ticks join this durable safety boundary after acquiring their workflow execution lease: clean manual records are removed, while retained or ambiguous outcomes remain operator-visible and backpressure later manual and scheduled work. A manual tick that overlaps a live occurrence returns structured `waiting` evidence without starting a worker. Occurrence-attention aggregation is machine-global for tick and run, including attention owned by another workflow. Definite occurrence-claim ownership loss is terminal immediately; only transient renewal failures use the bounded retry policy. A PR-manager worker cancelled after process start preserves its receipt and retained worktree as `needs_attention`; malformed worker output, a failed post-worker Git step, and post-start branch-lease loss use the same attention boundary whenever the checkout contains uncommitted changes or a new local commit. A clean unchanged failed checkout is removed and remains an ordinary bounded attempt. PR-manager setup failures and cancellations before worker start are typed as unexecuted, clean their managed checkout, and do not consume the scheduled occurrence or attempt budget. PR-manager worktrees are removed after branch-lease finalization for unambiguous outcomes and retained for ambiguous or cleanup-failed outcomes. Occurrence backpressure protects a retained PR-manager worktree until explicit acknowledgement; acknowledgement is the operator boundary after inspection and permits a later repair to reuse and clean that checkout. Side-effectful attention consumes the tick, while passive `exhausted_attempt` attention can allow another eligible PR to be considered. PR-manager worktree cache names are derived from a digest of the workflow ID, so accepted IDs containing path separators cannot escape the managed cache. Remote PR branch names are fully qualified as `refs/heads/...` before reaching option-parsed Git fetch arguments. Attempt state retains both the observed and pushed head so GitHub snapshot lag cannot reset a repair budget. If attempt-state persistence fails after repair work begins, the action keeps its receipt, push, lease, and worktree evidence as `needs_attention` instead of returning an evidence-free error. For shared-repository Codex tasks, excluding `.agent/state/receipts.jsonl` from ordinary dirtiness is conditional on an append-only proof. Jig uses short exclusive writer windows to open and identity-check receipt journal snapshots; prefix hashing and bounded append parsing run outside the lock, as do Git index probes, the worker, and nested or unrelated Jig receipt writers. The active pre-worker journal is limited to 64 MiB and the snapshotted append to 16 MiB; use `state archive` when the active stream reaches that operational bound. The Git index entry, journal identity, and pre-worker byte prefix must be unchanged, every record in the bounded appended snapshot must match the durable schema, and exactly one must carry Jig's expected worker receipt ID. `loop tick`, `loop dispatch`, and `loop run` map `ok: false` reports to a nonzero process status; diagnostic `loop status` returns zero when it successfully emits a report even if that report says `ok: false`.

`scripts/jig work append` requires exactly one nonblank progress source through `--body` or `--body-file`. `scripts/jig state summary` focuses its human output on persisted record and event counts, while `scripts/jig work status` presents the same runtime-owned state as an operational work overview. `scripts/jig state diagnose` is read-only; `--deep` adds recursive-session and receipt-payload analysis. Diagnostics also report disk usage for local maintenance artifacts under `.agent/.cache/state-backups/` and `.agent/.cache/state-archives/`. `scripts/jig state compact sessions --dry-run` validates and projects a legacy-session rewrite without changing state. Apply mode preserves root summaries and ordered direct references, removes recursively embedded summaries, validates the result, and first writes an exact gzip backup and checksum manifest under ignored `.agent/.cache/state-backups/`. `scripts/jig state restore --backup <directory-or-manifest>` verifies that artifact before restoring the exact pre-compaction stream.

`scripts/jig state archive --before <YYYY-MM-DD|unix-ms>` writes eligible old receipts as gzip JSONL under ignored `.agent/.cache/state-archives/` and rewrites `receipts.jsonl`. With explicit `--include-runs`, it also writes completed run-event groups to a separate artifact and rewrites `runs.jsonl`. Receipt evidence and run history linked to currently open plans remain active. Apply mode first reconciles an abandoned nonterminal run to `blocked` when its stable worker lease proves that no worker remains; ordinary foreground execution errors also terminalize their accepted run before returning. Run archival then refuses while any known run is nonterminal so rewriting cannot invalidate a live reader's durable byte cursor. A read-only preview never performs reconciliation and therefore reports abandoned runs until inspection or apply mode repairs them. Applying both streams prevalidates both and archives the harder run journal first; if the subsequent receipt operation fails, the error identifies the completed run artifact and exact recovery backup. Before each replacement Jig creates a complete manifested stream backup under `.agent/.cache/state-backups/`; `state restore --backup ...` recovers that exact stream's pre-archive bytes and physical order. A changing run-journal restore refuses while any current run is nonterminal or any current run worker still holds its lease; an identical checksum no-op remains safe. Use `--dry-run` to validate the selected streams and inspect counts without mutation. `scripts/jig state export receipts --before <cutoff> --output <file.jsonl.gz>` writes selected receipt records without changing active state and refuses to replace an existing destination. Legacy `.agent/state/archive/` files remain untouched and appear in diagnostics.

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

`work.gates` in `.jig.toml` declares required evidence before structured work can finish. A `kind: evidence` gate names exactly one structured target or profile and currently requires `conclusion: success`. A target gate matches only that exact target. A profile gate requires every current profile target from one run; receipts from separate runs are not combined. `scripts/jig work check --plan-id ...` resolves all evidence gates to exact targets, executes their union in one run, and links every target receipt to the work plan. Contract-v6 templates use a default-profile evidence gate. Legacy `kind: check` gates still reference no-argument execution tools from `.agent/jig-contract.json` and retain their existing receipt and batch semantics; explicit `work check --tool ...` selects that legacy path only. `kind: codex_review` gates reference Codex skills and are run by `scripts/jig work review --plan-id ...`, which records structured `jig.work_review` receipts with normalized findings, prompt/schema hashes, skill metadata, and worktree fingerprints. `scripts/jig work refine --plan-id ...` reads failed review findings, runs a Codex fixer loop, reruns review gates, then reruns all configured check and evidence gates.

Contract 5 and later check gates may declare strict `paths`, `paths_ignore`, and `reuse` policy. Work-plan open records an immutable commit or empty-tree baseline. Scoped checks compare that baseline with the current staged, unstaged, untracked, and committed inputs; their evidence records applicability, gate signature, scope fingerprint, and bounded changed-path metadata. A non-applicable gate closes with explicit evidence rather than a synthetic pass. Reuse is opt-in and accepts only a direct successful execution with the exact current gate and input identity; failed, cancelled, malformed, mutating, or transitively reused batches supersede older proof instead of revealing it. `work check --gate ID` forces a named check to execute while retaining its applicability facts.

`scripts/jig work gates --plan-id ...` reports gate status from the latest compatible evidence for each gate on any existing plan, including a closed plan. `scripts/jig work evidence` presents the same gate evidence as a human inspection report with target/profile identity, run and receipt ids, input match status, changed paths, and stale reasons. Latest evidence entries expose `tool`, `target`, or `profile` for execution evidence and `skill` for review evidence. For an evidence gate, the gate-level `target` is the canonical `component:action` selector string, while each member of its `targets` array carries the structured `{component, action}` receipt identity; profile gates use the same structured member rows and expose their selector in `profile`. A check, target, or profile reference that no longer resolves is an in-band `unsupported` gate with a reason, so read-only gate and open-plan status remain available; contract validation and execution still reject it. For `work gates` and `work evidence`, top-level `ok: true` means the inspection command completed; callers must read `overall`, `gates_ok`, and each gate `status` to detect blocked work. Receipt `changed_paths` are bounded repo-relative previews collected from `git status --porcelain=v1 -z`; they include untracked filenames but exclude `.agent/**`. These commands print human-readable output by default; pass global `--json` for structured automation output.

`scripts/jig work finish --plan-id ...` fails when any required gate is missing, failed, stale, unknown, or unsupported. Older `work.checks` entries are still accepted for compatibility and backfill missing required check gates during migration. If the same tool is declared in `work.gates`, that explicit gate entry is authoritative.

Fresh legacy check evidence means the committed non-`.agent/` source tree and non-`.agent/` worktree projection did not change while `work check` ran and still match the current source. Fresh target evidence additionally requires the receipt's contract digest and target input digest to match the currently resolved catalog and target. Target input digests conservatively cover the same complete source/worktree identity plus declared input patterns, so an unrelated local change can invalidate evidence; they are not cache keys. Append-only `.agent/` state and evidence-only commits are outside that identity. Missing target metadata produces `unknown`; a known mismatch produces `stale`. Generated outputs should therefore be committed, ignored, or settled before required gates are used as finish evidence. If a check creates expected files, review those files and rerun `work check` to record fresh evidence.

After upgrading an in-flight repo from a Jig version that recorded receipts without `worktree_fingerprint` or target digests, rerun `scripts/jig work check --plan-id ...` before `scripts/jig work finish --plan-id ...`. Older receipts deserialize, but their gate freshness is `unknown`.

Unknown non-`check` gate kinds are parsed and reported as unsupported. Required unsupported gates block finish.

## Rollout Rules

Use this sequence for public contract changes:

1. Add the new field, tool, or command in a backward-compatible way.
2. Update `.agent/jig-contract.json.jinja`, runtime dispatch, MCP exposure, and docs in the same change.
3. Keep old fields and commands working for the current contract version.
4. Run the configured release checks before release.
5. Only remove or redefine stable behavior after incrementing `contract_version`.

Generated repos can rely on:

- `scripts/jig` executing only a binary that validates the repository contract and requested profile
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
