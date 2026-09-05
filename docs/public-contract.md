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

Structured work commands, state hygiene commands, first-run setup, the unified doctor, status aggregation, Codex-home selection, and agent tooling checks are runtime-owned conveniences. They are available through commands such as `scripts/jig setup`, `scripts/jig doctor`, `scripts/jig status`, `scripts/jig work ...`, `scripts/jig state ...`, `scripts/jig codex ...`, and `scripts/jig agent doctor`, and MCP tools named `jig.work_*` and `jig.agent_doctor`, but they are not individually declared in `.agent/jig-contract.json`. Contract v7 is the current compatibility epoch. Contract v6 remains supported with its original component-aggregate affected selection, and versions 2 through 5 remain supported through the legacy repository projection. A runtime may add behavior that repositories in an epoch can ignore, but a breaking CLI, JSON/state, configuration, safety, launcher, dev, or vault change requires a contract bump or an explicit end to support for the affected epoch. Status text, JSON, and TUI modes and the `codex` namespace remain CLI-only.

CLI commands print human-readable output by default. Long-running human-mode commands collect bounded child-output previews, phase changes, and periodic heartbeats while supervised work runs, then make a deadline-bounded best-effort write of that progress to stderr after supervised execution returns and before any restored terminating signal is redelivered. A stalled presentation sink may therefore lose the remaining preview, but it cannot indefinitely delay command completion or signal retirement. Because delivery is deferred, heartbeat wording is historical (for example, a phase “reached 25s”) rather than a claim that it is still running when rendered. The deferred boundary keeps transport backpressure from suspending timeout, cancellation, or cleanup and preserves already-collected progress during ordinary interruption. Pass global `--json` for structured automation output (for example `scripts/jig doctor --json`, `scripts/jig status --json`, `scripts/jig work status --json`, or `scripts/jig work evidence --json`); JSON mode disables that human progress output. Usage and pre-output command failures in JSON mode write one object to stdout with `ok: false`, `error.kind` (`usage` or `command_failed`), `error.message`, and `exit_status`, while preserving the nonzero process status. Commands that already emitted JSON do not append a second error document, and `scripts/jig mcp` always reserves stdout for MCP framing. `scripts/jig prompt get` prints the bare rendered body without `--json` and the standard `prompt get` command envelope with it. `scripts/jig status --tui` is an explicit interactive consumer and conflicts with `--json`; it requires terminal stdin and stdout. For other commands, output selection is independent of interactivity: `--json` does not suppress terminal prompts. For init automation, `--defaults` applies documented project-shape defaults but can still prompt for initial vault setup; supply `JIG_VAULT_PASSPHRASE` or `--no-vault` when that must be noninteractive. `--no-input` and implicit non-terminal execution require an explicit complete shape such as `--preset harness-only`; stored `harness_footprint = "minimal"` is also a complete harness-only shape. `scripts/jig work start --print-plan-id` remains a shell-capture override that prints only the new plan id. Human text, TUI presentation, and `--print-plan-id` output are for terminal use and are not stable machine-readable contract output; automation should pass `--json` or use MCP tools.

Contract v4 introduced structured runtime identity through `runtime_version`, and later epochs retain it. The former `jig_version` key remains as a compatibility alias in `info`, `doctor`, and UI snapshots: it contains the legacy generated pin for v2/v3 repositories and is `null` for v4 and later repositories. Doctor runtime data likewise retains deprecated `current_version`, `launcher_version`, and `config_jig_version` aliases alongside the clearer epoch-aware fields.

`scripts/jig codex homes --json` returns a runtime-owned `schema_version: 1` report of local Codex home paths, account identity, plan type, and per-home errors. A home's `status` records stable account state such as `not logged in` or `unknown`; `inspection_error` records account/app-server inspection failure, while `usage_error` records a rate-limit failure after a logged-in account was observed. Both are mirrored in the top-level `errors` array with distinct `kind` values. A logged-out home is a complete observation even when usage was requested: rate-limit usage is not applicable, so an app-server usage failure is not surfaced as `usage_error` and does not change `outcome` to `partial`. Add `--usage` to include every rate-limit bucket and the server-reported durations and reset times. If account inspection succeeds for a logged-in account but usage inspection fails, the report retains the account and records the usage failure. Account and usage data come from the Codex app-server API; Jig does not parse `auth.json`. This output contains local paths and account email addresses and should be handled as user data. Its top-level `representation_lossy` boolean reports whether any non-UTF-8 home path or Codex executable value had to be replaced for JSON display. `scripts/jig codex launch HOME --dry-run --json` and `scripts/jig codex resume SESSION_ID --dry-run --json` are `schema_version: 1` structured launch previews; the latter reports `command: "codex resume"` and includes the injected `resume` and normalized session-ID arguments before caller-supplied arguments. Their `representation_lossy` boolean additionally covers forwarded arguments. Human terminal previews replace control characters before display and explicitly warn when the shown shell command is therefore not launch-equivalent; JSON retains the original string values except for reported non-UTF-8 conversion. A real launch or resume replaces the Jig process on Unix and therefore rejects `--json`; forwarded Codex arguments begin after `--` and retain their original boundaries. On platforms without process replacement, Jig waits for Codex and exits with Codex's shell-compatible status.

`scripts/jig status --json` returns a runtime-owned aggregate with `schema_version: 1`. Its top-level sections are `repository`, `work`, `loops`, `providers`, and `errors`; accepted provider documents remain independently discriminated by `protocol: "jig.status-provider/v1"` under `providers[].report`. Top-level `ok: true` means inspection completed, while `outcome` is `complete` or `partial`. Provider failures are data with `providers[].status = "failed"` and `report: null`, so the command can still return an inspectable snapshot. Domain blockers, stale or dirty inputs, and blocked gates are observed facts rather than collection errors. The aggregate schema version is independent of both provider `protocol` and generated `contract_version`.

`scripts/jig info --commands --json` returns the runtime-owned command-availability inventory with `command: "info commands"` and `schema_version: 3`. Its `commands` array follows the visible root-command order and each entry contains `name`, `category`, `status`, `reason_code`, `reason`, and `next_step`. Schema version 3 adds the backend-neutral `migration` command family; schema version 2 grouped migration authoring under `sqlx`, and schema version 1 described the legacy flattened roots. The stable status values are `ready`, `not_configured`, `needs_setup`, and `unavailable`; reason codes are stable within a schema version, while human-facing reason and remediation text may improve without a schema-version change. Status describes whether the root command's primary workflow can dispatch, not whether every argument combination or command-specific preflight will succeed. Setup, status, stop, and diagnostic subcommands or flags can therefore remain usable when the root entry is not ready. Ready entries have null `reason_code`, `reason`, and `next_step` fields.

`repo.context_status` is stable within command-inventory schema version 3: `valid` means strict repository lookup succeeded, `absent` means no repository was found, `invalid` means strict lookup failed without recovering a current repository, and `recovered` means the explicit context was invalid but tolerant lookup found a valid current repository. This field classifies repository lookup only; consumers must use `commands[]` as the authoritative command-availability result because different invalid-context cases can leave different context-tolerant commands usable. Producing the observational inventory is successful even when `repo.context_status` is `invalid`, so callers must inspect `repo.context_status` and `commands[]` rather than treating exit status alone as repository health.

The built-in `noop-status` workflow keeps `loop` ready without configured custom workflows. In a valid adopted repository, the proxy family is ready when either the current binary includes dev-proxy support or an executable full-footprint `scripts/jig` plus `scripts/install-jig.sh` launcher chain can route `dev` and `proxy` through its feature-enabled profile. It remains ready without configured dev apps because its primary ad-hoc run, alias, certificate, service, and diagnostic workflows do not require dev-app configuration; `jig doctor` separately reports whether dev-proxy integration is configured for the repository. Before adoption, the primary `proxy run` workflow remains `needs_setup`, while contextless status, cleanup, certificate, and service diagnostics may still work. The inventory reports other commands that can run without a repository and marks repository-dependent primary workflows `needs_setup` with `reason_code: "repo_context_unavailable"`. When a repository is discovered but its configuration or generated contract is invalid, commands whose dispatch consults optional repository context are also marked `needs_setup`, even if they can run when no repository exists. In valid context, `repo.context_error` is null; in fallback states, `repo.name` and `repo.root` are null and `repo.context_error` contains the load diagnostic. The inventory is read-only. Vault and Codex readiness are machine-local observations: when Codex marketplaces are configured, collection reads the local Codex configuration and may spend up to five seconds probing the configured Codex binary.

The schema-version 3 `reason_code` values are `agent_readiness_unknown`, `bootstrap_tool_invalid`, `bootstrap_tool_missing`, `codex_marketplace_support_unavailable`, `codex_marketplace_unregistered`, `dev_apps_not_configured`, `dev_proxy_feature_not_built`, `migration_add_tool_invalid`, `migration_add_tool_missing`, `migration_backend_not_configured`, `migration_directory_not_configured`, `repo_context_unavailable`, `sqlx_disabled`, `vault_not_initialized`, and `vault_status_unavailable`. Schema version 2 omitted `migration_backend_not_configured`; schema version 1 additionally used `schema_dump_tool_invalid`, `schema_dump_tool_missing`, and `schema_dumps_disabled` for the former root-level schema entry. The stable category values are `get_started`, `develop`, `structured_work`, `project_data`, `local_services`, and `agent_automation`.

An invalid or stale `JIG_REPO_ROOT` remains a blocker for workflows that use strict repository lookup, including the primary `proxy run` workflow. Workflows using tolerant optional-context lookup instead ignore the invalid override, quietly try the current directory, and fall back to no repository when appropriate. When that lookup recovers a valid current repository, the inventory uses it for `dev` and vault readiness even though `repo.name` and `repo.root` remain null because the explicit override is invalid.

Bootstrap command JSON is also runtime-owned. `scripts/jig init --json`, `scripts/jig adopt --json`, and `scripts/jig update --json` include a `render_report` object that summarizes created, modified, unchanged, conflict, backup, managed-block, authored-seed, and todo items for human review. When `jig init --json` runs a project scaffold, its sibling `scaffold` object reports the scaffold preset, sanitized `repo_name`, nullable `repo_name_sanitized_from`, `db`, `frontends[].{name,dir,kind,role}`, `frontend_notices` for bare custom names that are not preset shorthands, and `files_created` / `files_modified` / `files_unchanged` separately from template-managed file counts in `render_report`. Generated shadcn React frontends carry a same-contract-epoch `ui` provenance object describing the system, CLI version, preset, primitive base, style, and Tailwind major. `scripts/jig adopt` previews by default with `render_mode = "preview"` and only applies managed files with `render_mode = "copy"` when `--write` is supplied. Its runtime-owned `adoption_profile.file_budget` reports bounded policy classification, debt, legacy markers, waiver drafts, and whether human authorization is required; write mode fails before mutation while such a draft remains incomplete. Full-update JSON includes `legacy_file_budget_migration`, whose status, recognized generation, reason, and optional exact rerun command describe whether a known Bash checker was retained, retired, absent, or preserved as authored state. `scripts/jig init`, `scripts/jig adopt`, and `scripts/jig update` print human summaries by default; pass `--json` for the full structured reports. Automation should treat those reports as runtime diagnostics governed by the contract epoch, not as tool entries in `.agent/jig-contract.json`.

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

Version `2` is the legacy root-check command-backed contract. Version `3` groups checks under `scripts/jig check ...`. Both legacy epochs require matching `jig_version` fields in `.jig.toml` and the manifest as an internal consistency check, but a compatible runtime does not compare its own product release with that value. Version `4` removes generated product-version fields and makes `contract_version` the whole-harness compatibility epoch. Version `5` adds the strict `backend_language`, `go_database`, and backend-neutral `migration_dir` configuration selectors. Version `6` replaces the singular runtime stack identity with explicit components, actions, profiles, and adapter provenance. Its generated `.jig.toml` records the authored model under `[repository]`, while `.agent/jig-contract.json` records the matching resolved model. Version `7` adds typed native file-budget configuration and durable prepared native inputs, and makes non-empty action inputs target-local for affected selection. Rust, Go, SQLx, Go/PostgreSQL, and TypeScript capabilities are adapter contributions; command keys are component-scoped, such as `api_test_command` and `web_test_command`. Versions 2 through 5 remain readable through the legacy catalog projection, and version 6 retains its original repository behavior. An unmigrated v2/v3 wrapper remains runtime-readable but intentionally fails Doctor's required launcher-shape check; Doctor recommends a full `update --force` first when the repository has intact ownership metadata, with `update --launcher-only --force` reserved as the narrow recovery step when the legacy wrapper cannot start or full ownership is not yet established. That narrow repair leaves the repository on its supported legacy epoch and seeds the proven repair runtime; afterward Doctor exposes migration to the current contract as optional follow-up because the legacy recorded source may not be able to recreate that seed. A compatible change may add optional manifest data, tools, commands, or runtime behavior that older readers in the same epoch can ignore. Strict generated configuration additions and other breaking changes must increment `contract_version` before generated repositories depend on them.

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

## Dashboard And Status Output

The full-screen output from `scripts/jig ui` and `scripts/jig status --tui` is human-only and requires terminal stdin and stdout. `jig ui` starts the unified six-tab dashboard on Work; `jig status --tui` is a permanent status-first alias into the same implementation. Both are read-only and record no receipt. Redirected interactive use exits nonzero with guidance to select one of the JSON forms below.

`scripts/jig --json ui` emits one local recorder document and does not run status providers. `scripts/jig --json ui --plan PLAN_ID` emits one plan document. Every listed root field is present:

| Document | Root fields, in serialization order |
| --- | --- |
| Recorder | `ok`, `command`, `schema_version`, `snapshot_kind`, `generated_at_ms`, `epoch_id`, `repo`, `harness`, `current_session_id`, `counts`, `open_plans`, `history`, `failures`, `tool_stats`, `loops`, `timeline`, `timeline_show`, `timeline_limit`, `limits`, `errors` |
| Plan | `ok`, `command`, `schema_version`, `snapshot_kind`, `generated_at_ms`, `basis_epoch`, `detail_observed_at_ms`, `gates_observed_at_ms`, `decisions_observed_at_ms`, `plan`, `body`, `gates`, `decisions`, `receipts`, `limits`, `errors` |
| Status | `ok`, `command`, `schema_version`, `observed_at_ms`, `outcome`, `repository`, `work`, `loops`, `providers`, `errors` |

For recorder and plan documents, `ok` is boolean and is `true` for a successfully emitted snapshot; `command` is the string `"ui"`; `schema_version` is the unsigned integer `1`; `snapshot_kind` is the string `"recorder"` or `"plan"`; timestamps and epoch identities are unsigned integers. Observation and `limits` fields are objects, collection fields and `errors` are arrays, and identity/status/filter fields are strings unless their DTO says otherwise. `current_session_id`, `loops`, `body`, and `gates` are object/string-or-null fields and remain present when null. Empty arrays remain present. The Status document instead uses command `"status"`, schema version 1, and `outcome` string `"complete"` or `"partial"`; its detailed provider aggregate is defined by the [status-provider protocol](status-provider.md#jig-runner-and-aggregate).

Nested bounded rows serialize as `{"items": [...], "applied": N, "omitted": N|null}`. Bounded text serializes as `{"text": "...", "applied_chars": N, "omitted_chars": N|null}` and counts Unicode scalar values. Recorder and plan root arrays remain ordinary arrays; the root `limits` object maps each root collection name to `{"applied": N, "omitted": N|null}`.

| Limit identifier | Ceiling |
| --- | ---: |
| `open_plans` | 1000 |
| `history` | 10 |
| `failures` | 10 |
| `failure_stderr_chars` | 400 |
| `tool_stats` | 256 |
| `loop_workflows` | 1000 |
| `loop_leases` | 1000 |
| `loop_attempts` | 1000 |
| `loop_scheduled_occurrences` | 1000 |
| `loop_waiting_attempts` | 1000 |
| `loop_exhausted_attempts` | 1000 |
| `timeline` | 1000 |
| `timeline_decision_rationale_chars` | 300 |
| `gate_rows` | 256 |
| `gate_changed_paths` | 100 |
| `gate_matching_paths` | 100 |
| `gate_findings` | 100 |
| `plan_body_chars` | 20000 |
| `plan_body_input_bytes` | 80004 |
| `plan_decisions` | 100 |
| `plan_receipts` | 50 |
| `receipt_changed_paths` | 20 |
| `receipt_stdout_chars` | 1000 |
| `receipt_stderr_chars` | 1000 |

`--timeline-limit 1..1000` controls recorder activity rows and defaults to 120, so the applied `timeline` limit can be below its ceiling. `--timeline-limit` is invalid with plan JSON, and either refresh option is invalid with all UI JSON modes. Argument conflicts use the standard usage envelope and exit status 2. An unknown plan is a command failure with exit status 1.

Each partial collection error is `{"scope": string, "code": string, "subject_id": string|null, "message": string}`. Scopes are `repository`, `state.sessions`, `state.plans`, `state.decisions`, `state.receipts`, `loops`, `gates`, and `body`. Codes are `git_observation_failed`, `git_upstream_comparison_failed`, `git_upstream_output_invalid`, `stream_open_failed`, `stream_read_failed`, `record_too_large`, `record_decode_failed`, `loop_observation_failed`, `gate_observation_failed`, `body_not_found`, `body_unsafe_path`, `body_unsafe_type`, `body_read_failed`, `body_invalid_utf8`, and `unsupported_platform`.

A nonempty `errors` array is partial observation, not command failure: recorder and plan documents retain `ok: true`, preserve usable data, and exit 0 after one complete JSON document is written. Status JSON likewise preserves usable data, exits 0 after successful collection, and changes `outcome` to `"partial"`. Failures before a snapshot can be constructed use the ordinary command-error envelope and a nonzero exit.

Dashboard and status readers cap each logical record in `sessions.jsonl`, `plans.jsonl`, `decisions.jsonl`, and `receipts.jsonl` at 1048576 bytes. An oversized record is skipped without allocating proportionally and yields a `record_too_large` partial error. This 0.3.0 safety tightening does not change the append-only state format, but a schema-valid oversized legacy record that an older runtime attempted to allocate now makes UI recorder or status observation partial. Use `scripts/jig state diagnose` to identify the affected stream, stop Jig writers, and use the applicable compaction, archive, restore, or manual state-repair workflow before retrying.

Version 0.3.0 ends support for the browser server, its bookmarked URLs, and its HTTP JSON endpoints. `jig ui --json` now emits the recorder document directly instead of a URL envelope. A hidden `--port` parser exists only to return a migration diagnostic with exit status 2 and may be removed in 0.4.0. This workflow cutover does not change generated launcher command scope or contract version 7; callers needing the old transport must use an older Jig release.

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
`plan` object without running a command or writing a receipt. A newly written
plan uses run-plan schema version 3 and includes its derived `id`, configuration digest, source identity,
normalized selectors or profile, sorted targets, selection reasons, declared
effects, input digests, and dependency execution layers. Bare `jig check` uses
the default verification profile. An action selector such as `test` matches
that action across components; a target selector such as `api:test` is exact;
and `*` is the only wildcard and occupies a whole component or action segment.
Profiles and explicit selectors are mutually exclusive. Contract-6 legacy
aliases must not parse as canonical action, target, or wildcard selectors;
canonical selector meaning therefore cannot be shadowed by an alias.

For a selected contract-v7 action that still uses the built-in
`jig.file_budget` runner, the target also carries one bounded
`prepared_native_input`. It independently records authenticated policy and
comparison preparation, current view, the original typed comparison request,
fully defaulted checked-in resource ceilings and fallback policy, and optional
work-plan identity. Planning resolves this authority only after selection;
unrelated targets and command replacements do not require it. Submitted plans
are replay-authenticated before durable acceptance, while an accepted worker
uses the persisted object IDs rather than resolving symbolic refs again.
Schema-2 plans and pre-native target records remain readable with the new field
absent.

When that prepared input is ready, `jig.file_budget` executes in-process and
returns ordinary normalized target findings with source `jig.file_budget`, a
complete finding count and digest, bounded previews and human output, an
evaluation digest, comparison object identities, `evaluated_at_ms`, and the
earliest active-waiver `valid_until_ms`. Invalid policy preparation is a policy
failure; unavailable comparison authority, incomplete scope, unsupported file
types, mutation during reads, and exhausted resource bounds are blocked. The
engine measures arbitrary regular bytes and LF-delimited physical lines without
UTF-8 or binary heuristics and does not follow symlinks.

The independent `jig file-budget check|audit|explain|validate` family always
uses the built-in implementation and creates no run or receipt, even when the
checked-in action was replaced or removed. Its JSON output schema is
`jig.file_budget/report-v1`; its stable exits distinguish success/informational
audit (0), policy violations (1), invalid invocation or policy (2), and blocked
authority (3). Repository `jig check` planning accepts the same explicit
`--comparison-base`, `--comparison-exact-tree OID --comparison-provenance
explicit|push_before`, `--comparison-staged`, and
`--comparison-strict-inventory` vocabulary for native checks. The prefix keeps
repository comparison authority distinct from flags owned by configured
checker commands. Exact-tree
authority is never converted into a merge base. Push adapters must pass the
event's exact before identity rather than relying on ambient provider variables;
an unavailable nonzero before identity receives one bounded exact-object fetch
attempt and otherwise follows the authenticated checked-in block-or-inventory
fallback policy.

The configuration digest canonicalizes repository execution authority: the
parsed generated contract model, effective command bindings, backend migration
settings, and configured execution limits. Comments, formatting, and unrelated
runtime settings such as local development ports do not change that digest.
The separate source identity remains a conservative snapshot of all
non-`.agent/` repository source, so editing `.jig.toml` still requires planning
again even when the resolved execution authority is unchanged.

On contract 6 and later, `--affected BASE` narrows that ordinary selector/profile
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
fail-closed. Contract v7 matches a non-empty action input directly to its owning
target, while contract v6 retains component-aggregate matching. Actions without
inputs retain component-root fallback behavior. This lets a repository-wide
`"**"` action cover hidden and ordinary source paths without selecting unrelated
sibling actions in the same component. Reverse component dependencies propagate only under the checked-in
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

Current full-harness repositories own `.jig/file-budget.toml` and expose the
language-neutral `repo:file-budget` native action. `scripts/jig check
repo:file-budget` therefore uses the same selector, affected-planning, receipt,
and evidence path as every other repository action while Jig supplies the
versioned evaluator. The checked-in policy owns path matching, line and byte
budgets, exclusions, and bounded waivers. Repositories may replace or remove the
action, its `jig.file_budget` compatibility alias, or profile membership.
Direct diagnostics live under `scripts/jig file-budget`: `check` evaluates an
explicit comparison, `audit` inventories current files, `explain` reports one
path, and `validate` checks policy structure and waiver targets.

Contracts 2–5 that declare `jig.rust_file_loc` remain readable and executable
through their declared command authority. The compatibility projection does
not restore Rust-specific native LOC dispatch or checker-specific flags. A
contract-v7 recopy migrates exact generated authority to `repo:file-budget`;
the bounded two-update lifecycle can recognize and retire a generated checker
without retaining its source in the binary.

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
  optional `affected_base`, optional typed `comparison`, optional
  `work_plan_id`, and closed per-target `arguments` through the same
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

PR conflict validation uses independent evidence: `AUTO_MERGE` remains the worker-only conflict/whitespace baseline, while an observed-head comparison disables whitespace rules and rejects conflict markers added by the merge even when they are unchanged from `AUTO_MERGE`.

A clean manual loop tick keeps its durable occurrence live through loop-tick receipt publication and removes it only after that receipt commits. If receipt publication fails, the same occurrence becomes `needs_attention` with available execution evidence and continues to backpressure manual and scheduled reentry.

`.agent/state/*.jsonl` is runtime-owned append-only memory during normal operation. Generated repos may back up, inspect, or remove these files intentionally, but application code should not edit individual records in place. Runtime-owned maintenance commands may perform validated whole-stream rewrites with recovery artifacts. Generated `.gitattributes` marks those JSONL files with `merge=union` to reduce avoidable merge conflicts between independent append-only records.

Current JSONL state files:

- `sessions.jsonl`
- `plans.jsonl`
- `receipts.jsonl`
- `decisions.jsonl`
- `runs.jsonl`

State readers should tolerate missing files by treating them as empty. JSONL readers should ignore blank lines and fail loudly on malformed nonblank records. Session-start records retain their durable write-time summary, but `summary.recent_sessions` contains shallow event references whose nested `summary` is `null`; historical records that recursively embedded older summaries remain readable. Canonical session readers collapse duplicate IDs with identical event envelopes, as can arise after a line-union merge, and reject the same ID with a conflicting envelope.

Receipt records may include an `evidence` object for structured runtime-owned evidence that does not fit safely in truncated stdout or stderr previews. A target receipt additionally carries optional `run_id`, structured `target`, `config_digest`, `input_digest`, normalized `findings`, complete `finding_count`/`findings_truncated`/`findings_digest` metadata, `evaluated_at_ms`, and `valid_until_ms`; older records deserialize with those fields absent. A validity boundary is fresh only while `now_ms < valid_until_ms`, so equality is expired. That boundary is enforced, not merely displayed, by direct target status, work-check batch and scoped evidence, reusable and latest evidence, and archive protection. Historical receipts without the field retain their prior semantics, except new file-budget evidence proving active waivers without a required boundary is unknown rather than indefinitely fresh. Receipt Git metadata excludes `.agent/**`; `changed_paths` contains at most 100 sorted paths, while optional `changed_path_count`, `changed_paths_truncated`, and `changed_paths_digest` describe the full path set. Successful stdout and stderr previews use a 512-byte truncation threshold and failed previews use a 4,000-byte threshold. Configured-command timeout, await, cleanup, and capture failures use `evidence.kind = "supervised_command"`, `status = "error"`, and retain the diagnostic in the failed stderr preview. Cancellation after spawn uses the same evidence kind with `status = "cancelled"`; cancellation before spawn records no child receipt, and a work-check batch references only children that actually started. Older receipts without the new evidence or path-summary fields remain readable. A Codex worker receipt uses its separately bounded last-message file as authoritative `stdout_preview`; provider stdout is diagnostic transcript data in additive `evidence.provider_stdout_preview`. `provider_stdout_preview_truncated` reports bounding of that evidence preview, and `provider_stdout_truncated` reports truncation by the process supervisor. The legacy additive `stdout_truncated` evidence field remains an alias for provider-transcript truncation, while `stderr_truncated` continues to describe provider stderr. Codex review receipts use `evidence.kind = "codex_review"` and store normalized findings there, capped to the first 100 findings with long finding fields shortened; raw finding and actionable counts remain available so truncation does not hide a failing gate. Their receipt `exit_status` is the gate verdict, while `evidence.codex_exit_status` is the underlying Codex process status. They also include short stdout/stderr previews for failed review debugging. Codex refinement receipts use `evidence.kind = "codex_refine"` and store the refinement iteration, optional refinement profile metadata, reviewed gate ids, finding fingerprints, and finding count.

The active-session pointer is cache state, currently resolved through git as `jig-current-session.txt` and falling back under `.agent/.cache/`. Generated repos should not treat that path as a durable JSONL record.

Worktree-specific loop lease authority applies to workflow execution leases and attempt budgets. PR-manager branch leases instead use `jig/loop/branch_leases.json` below the repository's common Git directory, serializing mutation of one remote branch across linked worktrees; manual and scheduled PR-manager runs validate that repository-common authority before claiming an occurrence, and operators must stop older dispatchers during this writer cutover. GitHub snapshot normalization derives PR head identity strictly from `headRepositoryOwner.login` plus `headRepository.name`, and policy code never falls back to a version-dependent raw composite field. Review-reply idempotency binds the trusted-feedback generation and reply intent in addition to the repair commit. An unexecuted PR retry may clean only a worktree created by that retry; a pre-existing retained checkout remains operator evidence and is never force-removed by the later pre-execution failure.

Review-thread replies and resolution re-fetch the complete bounded comment history and live PR head together, compare the ordered ID, update-time, and body generation with the worker snapshot, and require the head to equal the pushed repair version; a current-intent Jig reply is the only excluded addition. Retained-worktree filesystem authority never comes from lossy display text: non-UTF-8 Unix path bytes use a tagged reversible JSON representation, and malformed encodings fail closed as retained.

Git repositories keep the authoritative schedule ledger, initialization marker, and lock in the checkout's worktree-specific Git metadata. This authority is outside a Codex `workspace-write` worker's writable surface and is the mutation commit point. Legacy-ledger migration and ordinary transitions use one lock order—legacy cache first, protected authority second—and every authoritative publication occurs while the protected lock is held. Authority resolution accepts Git's documented `.git` directory and regular `gitdir:` pointer-file layouts; a symbolic-link `.git` entry fails closed because following mutable metadata redirection would weaken that boundary. Protected initialization is a recoverable two-phase cutover: Jig first durably refreshes the public recovery ledger and marks protected cutover pending, then publishes protected state, and only then records final protected authority before a mutation may run. Pending cutover resumes from that recovery ledger only with resolvable Git authority; pending and final markers both fail closed when Git metadata is unavailable, so a surviving replica cannot become authoritative after a partial cutover or protected-authority loss. Deleting, replacing, or temporarily preventing later ledger-replica publication cannot erase occurrence history, fail an already committed transition, or permit the same occurrence to rerun; a later authoritative write retries the replica.

PR-manager outcome finalization explicitly refreshes the branch lease before inspecting or removing its deterministic checkout. A failed refresh retains the checkout without touching it because cleanup authority is ambiguous. The shared cleanup boundary revalidates the exact lease owner before every inspection and removal step, and in-flight Git cleanup is cancelled if renewal loses ownership. Cleanup finishes before the branch lease is released; authority loss or a later release failure remains explicit attention without retrying cleanup. PR action JSON uses the same reversible Unix path representation as retained-worktree authority, while cleanup receives the native path directly instead of reconstructing filesystem authority from JSON; Git metadata paths remain native byte sequences through pointer parsing and command output.

An active occurrence owner durably reserves its deterministic task or PR worktree path before Git may create or reuse that checkout. A crash after reservation therefore leaves any created path attached to stale attention, so acknowledgement cannot admit a shared-root worker while the checkout remains. Schedule locks are opened relative to no-follow directory capabilities after managed-path validation, and every cleanup, read, marker update, and durable publication under those locks stays relative to the retained capabilities. Read-only status projections retain their lock-free atomic snapshots, while a PR-manager attempt read that decides whether work may start serializes with compensating attempt repair and cannot observe provisional cleared state.

Protected lease and attempt replacements sync both the new file and its containing Git-metadata directory before publication returns.

Scheduled loop occurrence state is mutable machine-local runtime state, not append-only agent memory and not disposable cache. In Git repositories its source of truth and serialization lock live in worktree-specific Git metadata, with a compatibility replica under `.agent/runtime/loop/`; non-Git fixtures use that checkout-local path directly. Preserve Git metadata and retained worktrees with the checkout used by an external scheduler. Retained task and PR-manager worktrees also live below `.agent/runtime/loop/` so cache cleanup cannot destroy reported work. Lease ownership and retry-attempt budgets use sibling protected `leases.json` and `attempts.json` authorities in the same worktree-specific Git metadata directory; non-Git fixtures retain the `.agent/.cache/loop/` representation. Coordination JSON reads and writes are limited to 8 MiB per file, including growth observed after open, so damaged state fails as a bounded diagnostic instead of exhausting process memory. This is a deliberate safety tightening: pre-existing files above 8 MiB fail closed and identify the exact file that must be inspected or repaired while loop dispatchers are stopped; Jig does not destructively discard coordination authority during upgrade. Every Git-backed entrypoint that can mutate the schedule ledger first proves that the runtime root is ignored, regardless of workflow kind; isolated Codex checkout additionally verifies its task-worktree root. Schedule storage and retained task or PR worktree paths reject every symlinked managed component, and a dangling `.git` entry is an invalid repository boundary rather than a non-Git fallback. An existing deterministic PR path is reusable only when Git's stable NUL-delimited worktree registry resolves to the same directory and its no-follow regular `.git` pointer and administrative back-pointer identify a linked worktree in the repository's common Git directory. The first Git-backed lock-taking lease or attempt access migrates any legacy checkout-cache value under an ordered lock pair and replaces the old record with a migration marker that earlier runtimes cannot deserialize. Later reads and mutations use only protected authority, so a repo-mode workspace-write worker cannot release leases, forge attempt budgets, or redirect the parent through checkout-local cache paths. Read-only attempt and dispatch-window observations use atomic snapshots without cleanup or lock-taking side effects; mutation entrypoints establish or migrate protected authority before writing. Before publishing any durable occurrence claim, dispatch fails closed on unparsable lease JSON and resets unparsable attempt JSON with additive `attempts_reset` state evidence; other coordination-state failures also fail closed, and corruption observed after workflow work begins remains explicit state-error evidence. A setup failure or cancellation after a claim but before worker start removes the unexecuted claim and reports an additive typed retryable pre-execution action. If cleanup retains a checkout, the occurrence instead becomes `needs_attention` with that path. Workflow-lease finalization requires the same unexpired owner under the protected lock; ownership loss after execution begins makes the durable occurrence require attention instead of reporting clean success. Renewal lock waits are capped by the remaining cancellation/finalization window; schedule-ledger transitions apply one deadline to the ordered legacy and authority locks rather than allowing each lock a fresh timeout. If stale reconciliation records generic unacknowledged attention before a worker returns, only the original owner may enrich that exact evidence-free record with its late terminal evidence, and the original reconciliation time remains authoritative. A current runtime migrates schema-1 cache and schema-2 or schema-3 durable occurrence ledgers to schema 4 before dispatch. Schema 4 records whether each new occurrence uses the shared checkout; older markerless `running` or `needs_attention` records are conservatively treated as potentially shared until they are finalized or acknowledged. Earlier runtimes reject the resulting schedule, lease, and attempt migration markers, preserving the downgrade barrier. A protected `schedule.initialized` marker beside the authoritative ledger preserves the fail-closed initialization fact even when disposable cache or the checkout-local replica is removed. Operators must stop older dispatchers during these writer cutovers and must not downgrade after protected state is published. Dispatch keeps ambiguous scheduled occurrences and exhausted per-item attempts as separate, nonduplicated attention sources because they require different repair commands, derives its unsuccessful status from either source, and records cancelled or failed post-work state observations in the dispatch receipt. The durable claim transaction rejects older work after a newer occurrence is recorded, keeps any shared-repository claim mutually exclusive with every live or unacknowledged workflow claim, and blocks a shared-root worker while any retained managed worktree still exists, including acknowledged evidence. Workflow-local claims remain mutually exclusive within their workflow and also wait for a live or unacknowledged shared-root claim. Status and acknowledgement share the same claim-expiry predicate, so direct acknowledgement atomically reconciles an expired `running` record before terminalizing it; acknowledgement releases the occurrence-state blocker, subject to retained-worktree backpressure. The schedule locks remain held through acknowledgement receipt publication; its lightweight state receipt deliberately omits Git metadata so this transactional critical section does not run repository inspection. Schedule and receipt lock acquisition share one bounded operation deadline and observe cancellation, and a receipt failure known to precede any append restores the prior attention state before another dispatcher can observe the transition. Attempt repair uses the exact persisted workflow and item keys, including after a workflow is removed or renamed; schema-version-1 clear-attempt evidence keeps `workflow` as an object and adds `workflow_id` as its explicit string key. Clear-attempt state and its receipt use the same compensating boundary, so a receipt failure known to occur before any append restores the exact prior attempt record rather than reporting an unrecorded repair. A post-write receipt failure retains the committed state because the receipt may already be visible; returning an error without compensation avoids publishing success evidence for state that was deliberately reverted. Schema-version-1 dispatch evidence keeps `skipped_count` as the broad number of due occurrences not executed, including abandonment-state failures, while additive `deferred_count` identifies authority contention, including a held workflow lease or overlapping live occurrence. `loop tick` and `loop run` also treat machine-global `needs_attention` as unsuccessful even when a workflow selector points at a different workflow; selectors choose work, not the scope of runtime-health reporting. `loop status --workflow` instead scopes every workflow-owned section in its diagnostic projection. Status uses one sampled clock for schedule and attempt classification. A status schedule-evaluation error is scoped to its workflow and top-level `state_errors`, so other loop state remains inspectable in an unfiltered status report. Stale adopted repositories can refresh the managed rules with `scripts/jig update --recopy`. A retained isolated-task or PR-manager worktree blocks another manual or scheduled claim for the same workflow until the operator removes the reported path, bounding evidence growth without automatic data loss.

Bounded terminal occurrence history uses finish/start recency rather than scheduled time alone, so timestamp-zero manual occurrences retain the newest records. The latest scheduled occurrence is reserved within that bound as the dispatch watermark, preventing newer manual history from making an already executed cron instant due again. UI projections label manual records as manual runs and use their start time instead of displaying the zero sentinel as the Unix epoch. Successful unexecuted abandonment suppresses only the expected typed ownership-loss diagnostic created by deliberately removing that claim; any other renewal shutdown error remains state evidence.

Normalized GitHub loop evidence is limited to 16 MiB after serialization and omits duplicate raw payloads. A dispatch receipt references its detailed tick receipt rather than copying that observation a second time; dispatch command output retains the detailed nested tick. Cancellation records every remaining review-thread intent as unattempted evidence. Snapshot and review-thread request supervision preserves the subsecond remainder of aggregate deadlines, including the minimum valid one-second command timeout. Post-push review updates additionally give each unique actionable intent its own command-timeout and request slice within the aggregate cap, and snapshot request counts include only requests that passed pre-launch budget validation.

Manual ticks join this durable safety boundary after acquiring their workflow execution lease: clean manual records are removed, while retained or ambiguous outcomes remain operator-visible and backpressure later manual and scheduled work. If a staged manual record expires before receipt publication completes, stale reconciliation preserves the staged diagnostic and adds expiry context instead of replacing evidence with a generic message. A manual tick that overlaps a live occurrence returns structured `waiting` evidence without starting a worker. Occurrence-attention aggregation is machine-global for tick and run, including attention owned by another workflow. Definite occurrence-claim ownership loss is terminal immediately; only transient renewal failures use the bounded retry policy. A PR-manager worker cancelled after process start preserves its receipt and retained worktree as `needs_attention`; malformed worker output, a failed post-worker Git step, and post-start branch-lease loss use the same attention boundary whenever the checkout contains uncommitted changes or a new local commit. A clean unchanged failed checkout is removed and remains an ordinary bounded attempt. PR-manager setup failures, globally incomplete GitHub candidate lists, and cancellations before worker start are typed as unexecuted and do not consume the scheduled occurrence or attempt budget. Worktree preparation only returns a partial or registered checkout as a cleanup candidate; the shared outcome finalizer performs any cleanup after an explicit branch-lease refresh. Cleanup failure retains the exact path as attention rather than retrying destructively after releasing authority. Other unambiguous PR-manager worktrees are removed after the same branch-lease refresh and before lease release, while ambiguous, authority-lost, or cleanup-failed outcomes remain retained. Occurrence backpressure protects a retained PR-manager worktree after acknowledgement until the operator removes the reported path, matching isolated-task admission and bounding retained history. Side-effectful attention consumes the tick, while passive `exhausted_attempt` attention can allow another eligible PR to be considered. PR-manager worktree names below `.agent/runtime/loop/worktrees/prs/` are derived from a digest of the workflow ID, so accepted IDs containing path separators cannot escape the durable managed root. Remote PR branch names are fully qualified as `refs/heads/...` before reaching option-parsed Git arguments. Worktree preparation requires that ref to equal the immutable head from the GitHub snapshot; publication requires the worker result to descend from that head and uses an exact expected-head lease, so an intervening advance, rewind, or deletion fails stale rather than recreating or overwriting the remote state. The workspace-write worker edits files only; before commit, the parent stages resolutions, requires an index with no unmerged entries, checks worker-authored whitespace against the cached pre-worker tree, and rejects conflict-marker diagnostics present relative to both merge parents. This preserves marker examples inherited from either parent without allowing Git-introduced merge markers through. Attempt state retains both the observed and pushed head so GitHub snapshot lag cannot reset a repair budget. If attempt-state persistence fails after repair work begins, the action keeps its receipt, push, lease, and worktree evidence as `needs_attention` instead of returning an evidence-free error. Review text reaches the unattended worker only after GitHub reports the comment author's effective repository permission as `admin` or `write`; permission lookup fails closed, untrusted threads do not trigger repair, and the worker projection omits PR titles, raw GitHub payloads, and untrusted comment bodies. Nested review-comment connections are paged backward through older comments to a bounded limit; a missing cursor, changing count, duplicate, or exhausted limit marks that PR incomplete and prevents repair only for that PR, while completely observed PRs remain eligible. The top-level review-thread connection applies the same stable-count, unique-ID, and cursor-progress checks, and resolution is skipped when the comment count or latest comment differs from the worker snapshot. Empty review-thread IDs are rejected before deduplication, and a requested reply or resolution skipped by witness revalidation is reported as skipped at both the operation and post level. A truncated open-PR list still prevents all repair because the repository-wide candidate set is unknown. One snapshot client also bounds the composed observation to 256 GitHub requests, 16 MiB of cumulative responses, 10,000 normalized review items, and at most ten minutes; exhaustion fails before attempt or branch mutation. Incomplete comment histories skip collaborator-permission lookups and remain untrusted. Review-thread replies pass their potentially large body to `gh api` through a temporary file field rather than one process argument. For shared-repository Codex tasks, excluding `.agent/state/receipts.jsonl` from ordinary dirtiness is conditional on an exact append proof. Jig creates and locks the receipt journal inode even for the first append so current and legacy writers retain a common cutover lock. After both cutover locks are held, Jig verifies that the locked inode is still the current journal, releases both handles and waits for the bounded poll interval if an atomic rewrite replaced the inode while lock acquisition waited, and appends through the verified handle. Jig uses short exclusive writer windows to open and identity-check receipt journal snapshots; prefix hashing and bounded append parsing run outside the lock, as do Git index probes and the worker. The active pre-worker journal is limited to 64 MiB and the snapshotted append to 16 MiB; use `state archive` when the active stream reaches that operational bound. The Git index entry, journal identity, and pre-worker byte prefix must be unchanged, and Jig's expected worker receipt must be the only appended record; another schema-valid append is indistinguishable from a worker forgery and makes the result require attention. Post-work lease, attempt, and occurrence observations use the read-only cancellation-aware query paths, so cancellation does not wait behind coordination locks or initialize authority. A clean repo-mode commit stops the current multi-workflow dispatch so the next invocation reloads settings and prompts from one new repository revision. Dirty or unverifiable final repository state also stops that dispatch and requires attention; the cutoff is carried as typed completion authority even when presentation or tick-receipt publication fails. `loop tick`, `loop dispatch`, and `loop run` map `ok: false` reports to a nonzero process status; diagnostic `loop status` returns zero when it successfully emits a report even if that report says `ok: false`.

An authenticated existing PR repair worktree is removed and recreated after branch-head and occurrence reservation preflight; it is never treated as a cache because ignored files and nested repositories are outside ordinary Git cleanup. The path's branch component combines a bounded readable prefix with a digest of the complete branch name, preventing both filesystem component overflow and sanitization collisions. Jig supplies its PR-manager author identity only to the merge or commit command, leaving repository-local identity configuration unchanged. A conflicted `ort` merge validates the worker result against Git's `AUTO_MERGE` tree, so incoming base-branch whitespace is not misclassified as worker output. Immediately before commit, Jig recollects the complete bounded pull-request review-thread snapshot and retains the completed local repair without pushing if the PR head or any actionable thread's membership, trusted-author projection, content generation, or viewer capability differs from the worker snapshot. Before either a later reply or resolution mutation, Jig recollects the complete live review-thread witness and skips the mutation when feedback was edited, added, or resolved after that snapshot. Only GitHub-confirmed viewer-authored marker comments are excluded from the trusted-feedback generation, so trusted human quotations of marker text still advance the witness. A missing or false observed viewer capability skips the corresponding reply or resolution without issuing a known-impossible mutation.

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

`work.gates` in `.jig.toml` declares required evidence before structured work can finish. A `kind: evidence` gate names exactly one structured target or profile and currently requires `conclusion: success`. A target gate matches only that exact target. A profile gate requires every current profile target from one run; receipts from separate runs are not combined. `scripts/jig work check --plan-id ...` resolves all evidence gates to exact targets, executes their union in one run, and links every target receipt to the work plan. Contract-v6-and-later templates use a default-profile evidence gate. Legacy `kind: check` gates still reference no-argument execution tools from `.agent/jig-contract.json` and retain their existing receipt and batch semantics; explicit `work check --tool ...` selects that legacy path only. `kind: codex_review` gates reference Codex skills and are run by `scripts/jig work review --plan-id ...`, which records structured `jig.work_review` receipts with normalized findings, prompt/schema hashes, skill metadata, and worktree fingerprints. `scripts/jig work refine --plan-id ...` reads failed review findings, runs a Codex fixer loop, reruns review gates, then reruns all configured check and evidence gates.

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
