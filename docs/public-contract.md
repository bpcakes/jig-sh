# Public Contract

`jig` exposes a repo command contract through three surfaces:

- CLI commands from `scripts/jig`
- MCP tools from `scripts/jig mcp`
- `.agent/jig-contract.json`

Generated repositories declare a `contract_version` in `.agent/jig-contract.json`. During discovery, `scripts/jig` accepts a candidate only when the binary supports that contract epoch and requested build profile; this capability-only probe intentionally does not validate unrelated repository policy. Immediately before ordinary command dispatch, the launcher passes its epoch, selected profile, and repository root through hidden root options so the chosen binary validates the complete repository contract in-process and reuses the loaded context; malformed configuration therefore produces its specific validation error without a redundant startup subprocess or reinstall loop. Those `--__launcher-*` options are a private generated-launcher protocol, not a supported user interface; direct callers should select repositories through cwd or `JIG_REPO_ROOT`. `doctor` and `check contract` retain the capability-only final probe because they must remain reachable to report strict repository validation failures themselves. Generated repositories may pin an exact published release with `.jig/runtime-version`; release caches require that version plus contract/profile compatibility, independently of template provenance. Without a release pin, managed caches are trusted only while their source stamp matches the repository's configured source revision; local-source stamps additionally cover the source checkout contents. When `.jig.toml` is unreadable, capability-only commands may use an existing contract-compatible cache without proving source-stamp freshness so diagnostics remain reachable; mutating commands still reparse their required inputs before applying changes. Explicit `JIG_DEV_BIN` values remain authoritative trusted runtime sources. Without a release pin, reuse of an otherwise compatible binary found on `PATH` is disabled unless `JIG_INSTALL_ALLOW_PATH_BINARY=1` is set, in which case the installer reports the selected absolute path on stderr. Opted-in PATH candidates must have a validated ELF or Mach-O header and pass the direct compatibility probe; shell wrappers are never accepted through this path.

That final strict validation is a deliberate fail-closed launcher boundary and runs once inside the selected process before every ordinary command, including MCP startup. The launcher-provided canonical root is authoritative over inherited `JIG_REPO_ROOT`, and the validated context remains available process-wide if dispatch later crosses a worker thread. Validation expands beyond `check contract`: commands such as `check fmt` and `info` do not execute while required contract configuration is invalid. Help, version output, `init`, `presets`, `adopt`, `codex`, `claude`, `doctor`, `update`, and `check contract` remain reachable through capability-only validation so failures can be explained or repaired and account-scoped agent homes remain independent of repository policy. Bare `check` and selector-shaped values after it require strict repository validation because contract v6 resolves them against checked-in targets; unrecognized top-level commands still reach Clap without strict validation so their usage diagnostics are not hidden by unrelated repository errors.

Without `.jig/runtime-version`, `_commit` selects the source revision when the installer must build a runtime; it is not a product-version or binary-provenance lock. Once a binary has proven the requested contract and profile, the cache may reuse it while its recorded configured-source state remains current. Git-backed local-source caches recompute their Git/content fingerprint on every resolution so an edit invalidates the runtime immediately. Non-Git and unborn-Git sources first compare a path, identity, size, mode, and nanosecond timestamp summary; unchanged trees avoid rereading file contents, while a metadata change triggers a stable full-content comparison before reuse. Local source inputs under `Cargo.toml`, `Cargo.lock`, and `crates` may not be symbolic links: links fail closed because changing content behind one does not necessarily change the Git blob or link metadata being fingerprinted. This includes tracked regular files replaced by worktree symlinks, not only symlinks recorded in the Git index. The non-Git fallback also fails closed rather than traversing more than 100,000 entries, 512 MiB of regular-file content, or 128 directory levels; unusually large or symlinked source trees should use regular committed source entries or an explicitly rebuilt `JIG_DEV_BIN`. Launcher-only repair seeds are recorded distinctly with the seeded binary digest, a cheap file-identity key used to avoid rehashing an unchanged binary, and the source state they shadow, rather than pretending the binary was built from that source. Full refresh runtime policy combines the rendered source and harness footprint: minimal harnesses do not seed a removed installer, while full repositories rendered from embedded templates try to publish the running binary with durable embedded-runtime provenance. Cache publication occurs after repository rendering is committed; a failure is returned as a warning and does not misreport the durable render as rolled back, and an existing launcher-repair seed is retained as the last known fallback. Repair provenance across supported contract epochs is retired only after replacement is available or the rendered policy no longer manages an embedded runtime. A changed file identity falls back to the recorded digest check, while a source change invalidates either kind of seeded stamp. Deliberately unpinned mutable-source caches emit a periodic refresh reminder; `--refresh` or `JIG_INSTALL_REFRESH=1` forces the installer to recheck the source. Installer `--resolve-only` calls are read-only: they neither refresh seeded identity/source metadata nor write mutable-source reminder state.

Runtime seeding selects Bash and its helper-command path as one platform policy. Linux and macOS accept only root-owned, non-writable Bash and helper directories, preventing repository-local and unrelated ambient directories from entering the seeding path.

State hygiene commands, first-run setup, the unified doctor, status aggregation, Codex-home selection, and agent tooling checks are runtime-owned conveniences. They are available through commands such as `scripts/jig setup`, `scripts/jig doctor`, `scripts/jig status`, `scripts/jig state ...`, `scripts/jig codex ...`, and `scripts/jig agent doctor`, and the MCP tool `jig.agent_doctor`, but they are not individually declared in `.agent/jig-contract.json`. The former runtime-owned `scripts/jig work ...` commands and `jig.work_*` MCP tools were removed without a contract-version bump; see [Removed Work Commands](#removed-work-commands). Contract v8 is the current compatibility epoch. Contract v6 remains supported with its original component-aggregate affected selection, and versions 2 through 5 remain supported through the legacy repository projection. A runtime may add behavior that repositories in an epoch can ignore, but a breaking CLI, JSON/state, configuration, safety, launcher, dev, or vault change requires a contract bump or an explicit end to support for the affected epoch. The removal of the runtime-owned `jig work` commands and `jig.work_*` MCP tools is such an explicit end of support in every epoch; see [Removed Work Commands](#removed-work-commands). Status text, JSON, and TUI modes and the `codex` and `claude` namespaces remain CLI-only.

CLI commands print human-readable output by default. Long-running human-mode commands collect bounded child-output previews, phase changes, and periodic heartbeats while supervised work runs, then make a deadline-bounded best-effort write of that progress to stderr after supervised execution returns and before any restored terminating signal is redelivered. A stalled presentation sink may therefore lose the remaining preview, but it cannot indefinitely delay command completion or signal retirement. Because delivery is deferred, heartbeat wording is historical (for example, a phase “reached 25s”) rather than a claim that it is still running when rendered. The deferred boundary keeps transport backpressure from suspending timeout, cancellation, or cleanup and preserves already-collected progress during ordinary interruption. Pass global `--json` for structured automation output (for example `scripts/jig doctor --json`, `scripts/jig status --json`, or `scripts/jig state summary --json`); JSON mode disables that human progress output. Usage and pre-output command failures in JSON mode write one object to stdout with `ok: false`, `error.kind` (`usage` or `command_failed`), `error.message`, and `exit_status`, while preserving the nonzero process status. Commands that already emitted JSON do not append a second error document, and `scripts/jig mcp` always reserves stdout for MCP framing. `scripts/jig status --tui` is an explicit interactive consumer and conflicts with `--json`; it requires terminal stdin and stdout. For other commands, output selection is independent of interactivity: `--json` does not suppress terminal prompts. For init automation, `--defaults` applies documented project-shape defaults but can still prompt for initial vault setup; supply `JIG_VAULT_PASSPHRASE` or `--no-vault` when that must be noninteractive. `--no-input` and implicit non-terminal execution require an explicit complete shape such as `--preset harness-only`; stored `harness_footprint = "minimal"` is also a complete harness-only shape. Human text and TUI presentation are for terminal use and are not stable machine-readable contract output; automation should pass `--json` or use MCP tools.

Contract v4 introduced structured runtime identity through `runtime_version`, and later epochs retain it. The former `jig_version` key remains as a compatibility alias in `info`, `doctor`, and UI snapshots: it contains the legacy generated pin for v2/v3 repositories and is `null` for v4 and later repositories. Doctor runtime data likewise retains deprecated `current_version`, `launcher_version`, and `config_jig_version` aliases alongside the clearer epoch-aware fields.

`scripts/jig claude homes --json` returns a runtime-owned `schema_version: 1` document with `ok`, `command`, `outcome` (`complete` or `partial`), `current_home`, `representation_lossy`, `homes` (each with `name`, `path`, `default_config`, and `current`), and `warnings`. A successfully emitted partial discovery report exits zero. Adding `--usage` opts into credential and network access and adds `usage_included: true`. Each home then has `account` (a normalized object or null), `status`, `rate_limits`, `inspection_error`, and `usage_error`. Rate-limit buckets have `id`, `name`, `primary`, and `secondary`; each available window has `used_percent`, `duration_minutes`, and `resets_at` (Unix seconds or null). Unsupported or failed inspections mark the report `partial` without turning unknown limits into zero; a successfully emitted partial report exits zero. Credential values and raw service responses are never part of this schema. `scripts/jig claude launch HOME --dry-run --json` returns `schema_version`, `ok`, `command`, `dry_run`, `home`, `config_dir`, `claude_bin`, `args`, and `representation_lossy`. `config_dir: null` means the launch removes `CLAUDE_CONFIG_DIR` to preserve Claude's native default global configuration; a string sets it explicitly. The native-default entry is always available, including when `~/.claude` does not exist; listing and dry runs do not create it. Explicit configuration paths still require an existing directory. Directory discovery can show both native-default and explicit-override modes for the same directory when the override is current. Non-UTF-8 values use lossy display strings in JSON while real launches preserve native paths and arguments. Both commands use the standard JSON failure envelope and nonzero status on errors. JSON launch requires both an explicit HOME and `--dry-run`. Human picker cancellation exits zero. Claude homes are an additive CLI-only feature with no repository configuration or persisted-state migration; updated launchers classify `claude` as capability-only and preserve the invocation directory. Existing repositories must refresh their generated launcher with `scripts/jig update --recopy` after upgrading the runtime; older launchers change to the repository root before dispatching Claude.

`scripts/jig codex homes --json` returns a runtime-owned `schema_version: 1` report of local Codex home paths, account identity, plan type, and per-home errors. A home's `status` records stable account state such as `not logged in` or `unknown`; `inspection_error` records account/app-server inspection failure, while `usage_error` records a rate-limit failure after a logged-in account was observed. Both are mirrored in the top-level `errors` array with distinct `kind` values. A logged-out home is a complete observation even when usage was requested: rate-limit usage is not applicable, so an app-server usage failure is not surfaced as `usage_error` and does not change `outcome` to `partial`. Add `--usage` to include every rate-limit bucket and the server-reported durations and reset times. If account inspection succeeds for a logged-in account but usage inspection fails, the report retains the account and records the usage failure. Account and usage data come from the Codex app-server API; Jig does not parse `auth.json`. This output contains local paths and account email addresses and should be handled as user data. Its top-level `representation_lossy` boolean reports whether any non-UTF-8 home path or Codex executable value had to be replaced for JSON display. `scripts/jig codex launch HOME --dry-run --json` and `scripts/jig codex resume SESSION_ID --dry-run --json` are `schema_version: 1` structured launch previews; the latter reports `command: "codex resume"` and includes the injected `resume` and normalized session-ID arguments before caller-supplied arguments. Their `representation_lossy` boolean additionally covers forwarded arguments. Human terminal previews replace control characters before display and explicitly warn when the shown shell command is therefore not launch-equivalent; JSON retains the original string values except for reported non-UTF-8 conversion. A real launch or resume replaces the Jig process on Unix and therefore rejects `--json`; forwarded Codex arguments begin after `--` and retain their original boundaries. On platforms without process replacement, Jig waits for Codex and exits with Codex's shell-compatible status.

`scripts/jig status --json` returns a runtime-owned local aggregate with `schema_version: 4`. Its top-level sections are `repository`, `loops`, and `errors`. Top-level `ok: true` means inspection completed, while `outcome` is `complete` or `partial`. Dirty repositories are observed facts rather than collection errors. Schema version 4 replaced each scheduled occurrence's `worker_receipt_id` with the boolean `worker_invoked`. Schema version 3 removed the `work` section, which reported work plans, sessions, decisions, and gates. The aggregate schema version is independent of generated `contract_version`.

`scripts/jig info --commands --json` returns the runtime-owned command-availability inventory with `command: "info commands"` and `schema_version: 4`. Its `commands` array follows the visible root-command order and each entry contains `name`, `category`, `status`, `reason_code`, `reason`, and `next_step`. Schema version 4 adds foreground `run` availability and upgrade guidance; schema version 3 added the backend-neutral `migration` command family; schema version 2 grouped migration authoring under `sqlx`, and schema version 1 described the legacy flattened roots. The stable status values are `ready`, `not_configured`, `needs_setup`, and `unavailable`; reason codes are stable within a schema version, while human-facing reason and remediation text may improve without a schema-version change. Status describes whether the root command's primary workflow can dispatch, not whether every argument combination or command-specific preflight will succeed. Setup, status, stop, and diagnostic subcommands or flags can therefore remain usable when the root entry is not ready. Ready entries have null `reason_code`, `reason`, and `next_step` fields.

`repo.context_status` is stable within command-inventory schema version 4: `valid` means strict repository lookup succeeded, `absent` means no repository was found, `invalid` means strict lookup failed without recovering a current repository, and `recovered` means the explicit context was invalid but tolerant lookup found a valid current repository. This field classifies repository lookup only; consumers must use `commands[]` as the authoritative command-availability result because different invalid-context cases can leave different context-tolerant commands usable. Producing the observational inventory is successful even when `repo.context_status` is `invalid`, so callers must inspect `repo.context_status` and `commands[]` rather than treating exit status alone as repository health.

The built-in `noop-status` workflow keeps `loop` ready without configured custom workflows. In a valid adopted repository, the proxy family is ready when either the current binary includes dev-proxy support or an executable full-footprint `scripts/jig` plus `scripts/install-jig.sh` launcher chain can route `dev` and `proxy` through its feature-enabled profile. It remains ready without configured dev apps because its primary ad-hoc run, alias, certificate, service, and diagnostic workflows do not require dev-app configuration; `jig doctor` separately reports whether dev-proxy integration is configured for the repository. Before adoption, the primary `proxy run` workflow remains `needs_setup`, while contextless status, cleanup, certificate, and service diagnostics may still work. The inventory reports other commands that can run without a repository and marks repository-dependent primary workflows `needs_setup` with `reason_code: "repo_context_unavailable"`. When a repository is discovered but its configuration or generated contract is invalid, commands whose dispatch consults optional repository context are also marked `needs_setup`, even if they can run when no repository exists. In valid context, `repo.context_error` is null; in fallback states, `repo.name` and `repo.root` are null and `repo.context_error` contains the load diagnostic. The inventory is read-only. Vault and Codex readiness are machine-local observations: when Codex marketplaces are configured, collection reads the local Codex configuration and may spend up to five seconds probing the configured Codex binary.

The schema-version 4 `reason_code` values are `agent_readiness_unknown`, `bootstrap_tool_invalid`, `bootstrap_tool_missing`, `codex_marketplace_support_unavailable`, `codex_marketplace_unregistered`, `dev_apps_not_configured`, `dev_proxy_feature_not_built`, `migration_add_tool_invalid`, `migration_add_tool_missing`, `migration_backend_not_configured`, `migration_directory_not_configured`, `repo_context_unavailable`, `repository_contract_upgrade_required`, `sqlx_disabled`, `vault_not_initialized`, and `vault_status_unavailable`. Schema version 3 omitted `repository_contract_upgrade_required`; schema version 2 also omitted `migration_backend_not_configured`; schema version 1 additionally used `schema_dump_tool_invalid`, `schema_dump_tool_missing`, and `schema_dumps_disabled` for the former root-level schema entry. The stable category values are `get_started`, `develop`, `structured_work`, `project_data`, `local_services`, and `agent_automation`. The inventory no longer lists `work`; `loop` is now the only command in `structured_work`, whose human label is "Workflows".

An invalid or stale `JIG_REPO_ROOT` remains a blocker for workflows that use strict repository lookup, including the primary `proxy run` workflow. Workflows using tolerant optional-context lookup instead ignore the invalid override, quietly try the current directory, and fall back to no repository when appropriate. When that lookup recovers a valid current repository, the inventory uses it for `dev` and vault readiness even though `repo.name` and `repo.root` remain null because the explicit override is invalid.

Bootstrap command JSON is also runtime-owned. `scripts/jig init --json`, `scripts/jig adopt --json`, and `scripts/jig update --json` include a `render_report` object that summarizes created, modified, unchanged, conflict, backup, managed-block, authored-seed, and todo items for human review. When `jig init --json` runs a project scaffold, its sibling `scaffold` object reports the scaffold preset, sanitized `repo_name`, nullable `repo_name_sanitized_from`, `db`, `frontends[].{name,dir,kind,role}`, `frontend_notices` for bare custom names that are not preset shorthands, and `files_created` / `files_modified` / `files_unchanged` separately from template-managed file counts in `render_report`. Generated shadcn React frontends carry a same-contract-epoch `ui` provenance object describing the system, CLI version, preset, primitive base, style, and Tailwind major. `scripts/jig adopt` previews by default with `render_mode = "preview"` and only applies managed files with `render_mode = "copy"` when `--write` is supplied. Its runtime-owned `adoption_profile.file_budget` reports bounded policy classification, debt, legacy markers, waiver drafts, and whether human authorization is required; write mode fails before mutation while such a draft remains incomplete. Full-update JSON includes `legacy_file_budget_migration`, whose status, recognized generation, reason, and optional exact rerun command describe whether a known Bash checker was retained, retired, absent, or preserved as authored state. `scripts/jig init`, `scripts/jig adopt`, and `scripts/jig update` print human summaries by default; pass `--json` for the full structured reports. Automation should treat those reports as runtime diagnostics governed by the contract epoch, not as tool entries in `.agent/jig-contract.json`.

Adoption's `detection_report.component_candidates` is a deterministic list of
`root`, `proposed_id`, `ecosystem`, `evidence`, `confidence`, `disposition`, and
`reason` records. Disposition is `included`, `excluded`, or `review_required`.
The human review uses these same decisions. Initial adoption writes accepted
candidates into the existing authored component contract; this adds no contract
epoch. Repeated `--include-component ROOT` / `--exclude-component ROOT` options
select exact normalized repository roots, not globs or runtime ignores. Invalid,
unknown, escaping or conflicting selections fail before managed-file writes.
Existing complete authored models remain authoritative during readoption and
update; selection flags on those models fail with guidance to edit `.jig.toml`.


`scripts/jig info freshness` is a read-only CLI adoption preview with runtime-owned
`schema_version: 1` JSON. Each `targets` entry has `target`, `current`, `proposed`,
`reason`, `inputs`, `proposed_inputs` and `exhaustive_requires_owner_assertion`;
`changed_targets` identifies proposed authority changes. Repeat exact `--target
component:action` selectors to narrow the report. `--assert-worktree` and
`--assert-exhaustive` require explicit targets and record selected owner assertions
as declared policy; `--input` requires the exhaustive assertion and appends unique
patterns. Assertions are restricted to read-only non-native checks. No assertion
proves installed tools, ambient environment or live services. `--patch` emits a
paired unified diff for `.jig.toml` and `.agent/jig-contract.json` (an empty diff
for a no-op); with `--json`, it adds the string `patch` to the report. The preview
executes no configured action and writes no repository files. Patch generation
and assertions require epoch 8 or later. Existing inspection projections and MCP
tools are unchanged. See [declaration adoption](target-freshness-integration.md#preview-and-apply-declarations)
for qualification boundaries and the review/apply workflow.

Agent-guide check JSON keeps `missing_guides` as an empty compatibility field in this contract version and includes `missing_guides_note` to explain that placeholder backend-level `AGENTS.md` files are no longer required. Existing Rust crate and Go package guide files are validated when present. Consumers should stop treating `missing_guides` as the guide-coverage gate; use `missing_sections` and `missing_entry_ref` for existing-guide quality issues.

Dev proxy and vault JSON are also runtime-owned. Proxy status may include machine-local health fields such as `pid`, `pid_alive`, `pid_observation`, `health_pid`, `handshake_ok`, `pid_matches_proxy`, `running`, listener addresses, and route URLs; `pid_alive` means positively observed alive while `pid_observation` preserves an `alive`, `absent`, or `uncertain` result. Status and listing commands may perform a loopback HTTP health probe to populate those fields. Strict cross-machine automation should rely on the stable generated command contract instead of treating those runtime diagnostics as a contract schema.

Local development proxy commands are also runtime-owned. `scripts/jig dev`, `scripts/jig dev status`, `scripts/jig dev recover`, `scripts/jig dev stop`, and `scripts/jig proxy ...` manage machine-local processes, ports, routes, certificates, and optional user services. Repository-scoped forms use `.jig.toml`; the contextless selectors use persisted state. These commands are intentionally absent from `.agent/jig-contract.json` because they do not represent repository checks.

Runtime-owned local development commands include `dev`, `dev status`, `dev recover`, `dev stop`, `proxy start`, `proxy stop`, `proxy list`, `proxy prune`, `proxy run`, `proxy alias`, `proxy cert generate`, `proxy cert status`, `proxy cert trust --accept-trust-scope`, `proxy cert untrust --accept-trust-scope`, `proxy service install --accept-service-scope`, `proxy service status`, and `proxy service uninstall`. Bare `dev` launches apps, while its `--replace` option retires only conflicting registered sessions owned by the same canonical repository; it is not a general process takeover option. Foreground `dev` and `proxy run` interruption is structured same-contract-epoch output with `interrupted`, numeric `exit_signal`, named `termination_signal`, and shell `exit_status`; SIGINT, SIGHUP, and SIGTERM map to 130, 129, and 143 on Unix. Builds made with `--no-default-features` keep the contract, MCP, and check runtime but return clear errors for every `dev` action and `proxy`; the launcher profile probe prevents such a binary from serving `dev` or `proxy` execution.

`dev status --all` and `dev status --session ID` inspect the selected proxy state directory without repository discovery, including saved roots of deleted repositories and sessions with no hostname. `--all` and `--session` cannot be combined. Every session includes its saved repository name and root. `dev recover --session ID` performs strict metadata-only retirement of one eligible exact record and its exact-owned routes; it never requests process shutdown. Missing IDs return success with zero retired sessions. `dev stop --session ID` selects one record without repository discovery and retains authenticated live-supervisor shutdown and the explicit stop-only `--forget-ambiguous-orphans` repair. Exact selectors never expand prefixes or wildcards, never signal persisted PIDs, and accept `--state-dir` to choose an isolated registry. Bare status and stop remain scoped to the current canonical repository.

Dev-session JSON is same-contract-epoch runtime output. Bare `dev status` reports the canonical repo identity and resolved state directory, aggregate `running` state, sanitized registered sessions with explicit process observations and spawn tracking, durable `preflight_cleanup_pending` evidence, and a `recoverable` state for dead orphans. Aggregate `running` is true only when at least one session is neither stale nor recoverable; a recoverable record therefore leaves `running` false but remains in `sessions` until explicit cleanup or an eligible overlapping same-repository launch. The compatibility field `running` retains that historical rule. New aggregate and per-session `activity` values distinguish `verified` process or control evidence, `possible` activity or incomplete spawn/cleanup evidence, and `none`. Aggregate `cleanup_required` and each session's `cleanup_required`, `retention_reason`, `retention_app`, and `recoverable` explain why a record remains and whether explicit metadata recovery is eligible. An app's `route_present` means an exact-owned route record is persisted; it does not prove that the proxy listener or target is reachable. Status inspection does not mutate session or route state. `dev stop` reports matched/stopped session and app counts, any sessions that remain, blocking `warnings`, and structured successful `recoveries`; if a later stop operation fails, its `ok: false` result retains warnings and recoveries already produced and omits completion counts that could not be confirmed. Foreground `dev` results include eligible same-repository orphan recoveries completed while claiming the new session; `dev --replace` also includes recoveries completed while stopping live conflicts. Completed recoveries remain in the result if later startup fails or is cancelled. Each recovery preserves diagnostic app names, targets, spawn states, last-known PIDs, and any explicitly forgotten ambiguity after the registry entry is removed. Failed dev and dev-stop results that include recovery or cleanup metadata use the standard `error.kind: "command_failed"` and `error.message` object rather than changing `error` to a string. Stop is successful and idempotent when no session matches, recovers an orphan once cleanup evidence is complete and every exact registered identity is absent, and returns `ok: false` when preflight cleanup is unconfirmed, spawn state is pending or unknown, or a registered process remains live or uncertain. `dev stop --forget-ambiguous-orphans` is an explicit repair for dead-supervisor records blocked only by unconfirmed preflight cleanup or pending or legacy-untracked spawn evidence; it still returns `ok: false` for live or uncertain registered identities and records that an unrecorded process may remain. Neither response exposes the persisted session-control credential, and management never signals from persisted PID data.

The session file reader accepts versions 1 and 2. A missing version 1 `preflight_cleanup_pending` field is unknown cleanup evidence, even when every app has tracked spawn state; strict orphan retirement retains that record. Status keeps the existing boolean `preflight_cleanup_pending` field and adds `preflight_cleanup_evidence` (`pending`, `clear`, or `unknown`) so callers can distinguish missing legacy evidence. Version 2 requires explicit cleanup, preflight, and per-app spawn/process fields. Older version 1 readers reject version 2 before mutation. The version 2 writer cutover is enabled alongside contextless exact-session discovery and repair. Only an empty legacy store may be promoted under the shared state lock as part of a new claim. A populated legacy store remains readable and explicitly cleanable, but new claims must wait until its sessions are drained or repaired; no other repository's session is stopped automatically.

Local vault commands are runtime-owned as well. The surface includes init/status/audit, an explicit keyboard-first TUI, explicit format migration, field and compatible secret management, controlled read/inject, transparent exec, constrained run, one-time 1Password import, passphrase change, and encrypted backup/restore. Generated repos carry non-secret `[vault]` scope metadata in `.jig.toml`; when present, vault commands default to that repo scope rather than the user-level global vault. A canonical `jig://ITEM/FIELD` reference is relative to that selected scope and never embeds or overrides the project. These commands are intentionally absent from `.agent/jig-contract.json`, MCP tool listing, and repo-local state records because local values and child output must not be persisted into `.agent/state`.

`vault tui` is terminal-only, rejects `--json`, and fixes one resolved scope for its process lifetime. Ordinary frames, activity, errors, and action results contain authenticated metadata only. Private-file export and the exact-confirmation Peek path are controlled reveal sinks: Peek bypasses Ratatui, terminal-safely escapes and bounds the displayed source prefix, then clears the alternate screen before metadata redraw. Its deliberately disclosed window may still be retained by terminal scrollback, multiplexers, remote transport, or recording. The process-local credential is removed by explicit or five-minute idle lock and on authentication/audit failure; this is not a clipboard feature, unlock daemon, remote service, or contract/MCP surface.

Vault JSON is runtime-owned same-contract-epoch behavior, not an individually declared manifest tool schema. Structured vault responses contain metadata only, never field values. `vault status` currently reports both `exists` and `vault_file_exists`; both mean the encrypted `vault.json` file exists, not that the vault home directory exists. Structured responses report `vault_scope`, `vault_scope_id`, and `vault_repo_name`; the latter two are null when not applicable. Current `vault_scope` values are `repo`, `global`, `legacy`, and `explicit-home`. Field/import results expose references, kinds, counts, and create/replace actions. Passphrase change reports completion metadata; backup create reports byte count, backup version, and creation time; restore reports the installed vault home, vault ID, and format version. None returns passphrases, field bytes, backup plaintext, or external resolver diagnostics.

`vault read` and `vault inject` are exact-byte output commands. They reject global `--json`, bypass the normal structured emitter, and write only to their controlled stdout or private-file sink. A private-file destination must be outside the selected vault home and must not alias `vault.json` or `audit.jsonl`; an existing regular destination additionally requires explicit overwrite authorization. `vault exec` likewise rejects `--json`, transparently streams independently redacted stdout and stderr, and exits with the child's status without a second Jig error. Its dotenv source, and the corresponding 1Password import source, must be a non-symlink regular file rather than a FIFO, device, directory, or other special file. In contrast, compatible `vault run` returns mapping counts plus buffered, redacted, lossy UTF-8 `stdout` and `stderr` strings and raw process status fields; automation should use `result.exit_signal` to distinguish signal termination when present and otherwise branch on `result.exit_status`.

Current Jig reads both vault envelope versions. Version 1 values are treated as concealed and remain available to listing, reveal, injection, run, and exec; field mutation, import, passphrase rotation, and backup require explicit one-way migration to version 2. Older Jig rejects version 2. On version 2, `vault secret` remains compatible vocabulary over concealed fields, `vault run` retains constrained broker semantics, and `vault exec` is the separate transparent wrapper. See the [Vault Runtime compatibility matrix](configuration.md#references-fields-and-format-compatibility) for the operator-facing policy.

LAN mode exposes the Jig proxy listener to the local network, not child app listeners directly. Process routes may be reached from other devices only through the proxy, with the original routed hostname in DNS, a hosts file, or the HTTP `Host` header. Alias routes stay loopback-client-only so LAN clients cannot use Jig as an open forward proxy.

Proxy reuse authenticates the existing PID health response and a versioned capability response using the same private health token and loopback address/Host restrictions. The capability response reports the serving process's PID, LAN bind scope, HTTPS listener, and effective HTTPS HTTP/2 setting; LAN clients cannot read it. Reuse rejects unknown capabilities, PID/token generation changes, either LAN mismatch, and either HTTP/2 mismatch when HTTPS is requested. A caller requesting only HTTP may reuse an additional HTTPS listener. These checks occur before app spawn or route publication and never restart the shared proxy; errors give the selected state directory and an explicit matching-settings or restart action.

The `tool_defs::cli_command` names for these runtime-owned commands are parser labels only. They do not add generated tools to `.agent/jig-contract.json` and do not expose MCP tools for proxy process or service management.

Because the local development proxy and local vault are runtime-owned, their detailed JSON response fields, machine-local state layouts under `JIG_PROXY_STATE_DIR` / `~/.jig/proxy` and `JIG_VAULT_HOME` / `~/.jig/vault`, service-file contents, certificate files, vault/backup envelope formats, route hostname format, and nonzero error exit statuses are not individually enumerated in `.agent/jig-contract.json`. The vault audit JSONL is HMAC-chained but plaintext local metadata; field names, environment variable names, timestamps, run IDs, and vault IDs are not opaque payload. It detects edits and broken links but is not remote or independent evidence of deletion, truncation, or rollback. Breaking generated-repository assumptions in these surfaces requires a contract-epoch change even though compatible additions do not require new manifest fields.

The current explicit acknowledgement flags, including `--accept-trust-scope` and `--accept-service-scope`, are runtime safety gates rather than generated contract fields. Automation should use a launcher-selected binary that supports the repository contract; removing or weakening those required flags is a breaking contract-epoch change.

Runtime-owned `.jig.toml` sections are intentionally strict: unknown keys are rejected so local typos fail fast. New optional keys in `[work]`, `[loop]`, `[[loop.workflows]]`, `[execution]`, `[agent_tooling]`, `[agent_tooling.codex]`, `[dev]`, or app tables require a Jig runtime/template update and a documented migration note. The `[execution]` keys are backward-compatible in contract v4 through v6: omission defaults `command_timeout_seconds` to 1,800 seconds and `command_output_limit_bytes` to 67,108,864 bytes for configured commands. Internal protocol commands and Codex worker transcripts retain separate fixed limits. Any addition or change that makes an existing repository unreadable or changes generated behavior incompatibly requires a contract bump. Loop workflow keys `schedule`, `timezone`, `prompt_file`, `model`, `sandbox`, and `checkout`, the compiled `codex_task` kind, and the `loop dispatch` CLI are additive runtime behavior for supported legacy repositories; no generated MCP tool is added. A `pr_manager` or `codex_task` workflow may set `codex_home` to choose the exact `CODEX_HOME` for its unattended `codex exec` worker; omission inherits the caller environment for compatibility. Bare names resolve only to their conventional home-directory locations, while non-conventional homes require explicit paths. Same-contract-epoch loop JSON preserves the input as `codex_home_configured`; repair-attempt and task-worker actions and receipts report the canonical worker directory as `codex_home_resolved` when resolved, while actions that do not attempt work omit that field.

Current source accepts an optional strict `[work.tracker]` extension with
`kind = "beads"`, a required canonical portable ULID `workspace_id`, fixed root
`.beads`, manual export, and optional display-only manual guidance. Omission preserves
all existing configuration and installation behavior and does not require `br`. The
section is part of execution authority, while `work.receipt_metadata = ["beads"]`
remains the independent declaration that excludes the tracker from source identity. Existing current-epoch templates
do not generate the tracker section, and T2 adds no linked lifecycle command; the later
epoch-11 lifecycle cutover owns journal writes. An older strict runtime rejects a newly
configured tracker instead of silently discarding its authority. Update and write-mode
readoption preserve valid tracker and receipt-metadata authority independently of unrelated
configuration validity; malformed optional authority blocks refresh with a field-specific
diagnosis.

## Contract Version

`.agent/jig-contract.json` has these schema versions:

- `contract_version`: version of the generated tool manifest and command surface

Version `2` is the legacy root-check command-backed contract. Version `3` groups checks under `scripts/jig check ...`. Both legacy epochs require matching `jig_version` fields in `.jig.toml` and the manifest as an internal consistency check, but a compatible runtime does not compare its own product release with that value. Version `4` removes generated product-version fields and makes `contract_version` the whole-harness compatibility epoch. Version `5` adds the strict `backend_language`, `go_database`, and backend-neutral `migration_dir` configuration selectors. Version `6` replaces the singular runtime stack identity with explicit components, actions, profiles, and adapter provenance. Its generated `.jig.toml` records the authored model under `[repository]`, while `.agent/jig-contract.json` records the matching resolved model. Version `7` adds typed native file-budget configuration and durable prepared native inputs, and makes non-empty action inputs target-local for affected selection. Version `8` adds declared bounded string arguments, literal argv and explicit compatibility-shell runners, and the `inputs_policy` and `source_state` action declarations. Generic bounded strings bind only whole argv positions; shell runners accept no generic interpolation. V8 sources reject the implicit command runner, default action declarations to `inputs_policy = "whole_repository"` and `source_state = "git"`, and add reviewed `exhaustive` and `worktree` opt-ins that native actions cannot use; see [Action Input Declarations](#action-input-declarations). Epochs 9 and 10 are reserved and are not supported repository contracts. Earlier repositories retain their recorded epoch semantics. Runtimes supporting only older epochs reject v8 manifests and launchers before execution. Rust, Go, SQLx, Go/PostgreSQL, and TypeScript capabilities are adapter contributions; command keys are component-scoped, such as `api_test_command` and `web_test_command`. Versions 2 through 5 remain readable through the legacy catalog projection, and version 6 retains its original repository behavior. An unmigrated v2/v3 wrapper remains runtime-readable but intentionally fails Doctor's required launcher-shape check; Doctor recommends a full `update --force` first when the repository has intact ownership metadata, with `update --launcher-only --force` reserved as the narrow recovery step when the legacy wrapper cannot start or full ownership is not yet established. That narrow repair leaves the repository on its supported legacy epoch and seeds the proven repair runtime; afterward Doctor exposes migration to the current contract as optional follow-up because the legacy recorded source may not be able to recreate that seed. A compatible change may add optional manifest data, tools, commands, or runtime behavior that older readers in the same epoch can ignore. Strict generated configuration additions and other breaking changes must increment `contract_version` before generated repositories depend on them.

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

Make-backed tools return:

- `tool`
- `target`
- `args`
- `result.exit_status`
- `result.stdout`
- `result.stderr`

Command-backed tools return the same common fields plus `command_key`, which identifies the `.jig.toml` command key that was executed.

Jig no longer records receipts. Check, run, manifest-tool, `migration add`, policy-check, and loop responses no longer include `receipt_id`; run target results no longer include `receipt_id`, `reused_from`, or `target_freshness`; and planned targets no longer include `target_identity` or `target_identity_error`. `loop tick` instead returns the `occurrence_id` whose evidence `jig loop show` reports. The `--no-receipt` option and the MCP `record_receipts` field are rejected. These removals shipped without a contract-version bump.

Common usage errors include contextual recovery without executing a correction.
`--summary` points to the existing `--projection agent-v1` only on commands
supporting it, otherwise to scoped help. Top-level `contract` points to `check
contract`. CLI retries are parse-checked, checked against the existing info
projection policy, and shell-quoted, with private launcher handoff arguments
omitted. Launcher-backed retries and scoped help name the owning repository's
absolute `scripts/jig` path, so they work without a global installation and from
another directory. Direct CLI recovery preserves the invoked executable. Other
invalid arguments still require attention. These are diagnostic hints, not
aliases or automatic retries. Standard error envelope fields and exit statuses
remain unchanged.

## Dashboard And Status Output

`jig ui` and `jig status --tui` are one unified read-only terminal dashboard. Upgrading from 0.3.0 replaces the browser dashboard and external status providers. Remove retired `[status]` and `[[status.providers]]` tables as described in [Configuration](configuration.md#accepted-key-summary). Callers that still need the 0.3.0 browser transport must stay on that release.

The full-screen output from `scripts/jig ui` and `scripts/jig status --tui` is human-only and requires terminal stdin and stdout. `jig ui` starts the unified three-tab dashboard on Timeline; `jig status --tui` starts the same implementation on Status. Both are read-only and record nothing. Redirected interactive use exits nonzero with guidance to select one of the JSON forms below.

`scripts/jig --json ui` emits one local recorder document. Every listed root field is present:

| Document | Root fields, in serialization order |
| --- | --- |
| Recorder | `ok`, `command`, `schema_version`, `generated_at_ms`, `epoch_id`, `repo`, `harness`, `failures`, `target_stats`, `loops`, `timeline`, `timeline_show`, `timeline_limit`, `limits`, `errors` |
| Status | `ok`, `command`, `schema_version`, `observed_at_ms`, `outcome`, `repository`, `loops`, `errors` |

For the recorder document, `ok` is boolean and is `true` for a successfully emitted snapshot; `command` is the string `"ui"`; `schema_version` is the unsigned integer `4`; timestamps and epoch identities are unsigned integers. Observation and `limits` fields are objects, collection fields and `errors` are arrays, and identity/status/filter fields are strings unless their DTO says otherwise. `loops` is an object-or-null field and remains present when null. Empty arrays remain present.

The recorder reads finished targets from run history, the `target_completed` events in `runs.jsonl`:

- Each `timeline` row is one target result with `stable_identity`, `timestamp_ms`, `run_id`, `target`, `status`, `conclusion`, `exit_code`, `started_at_ms`, `ended_at_ms`, `duration_ms`, `finding_count`, and `output_tail`. `target` is the `component:action` string. `output_tail` is bounded text for a target that did not succeed and `null` otherwise.
- `failures` lists the newest results whose conclusion is `failure`, `timed_out`, or `blocked`, each with `run_id`, `target`, `conclusion`, `exit_code`, `ended_at_ms`, and bounded `output_tail` text.
- `target_stats` aggregates results per target: `target`, `runs`, `failures`, `last_conclusion`, `last_ended_at_ms`, and `avg_duration_ms`.

`output_tail` keeps the final characters of the target's stderr, or of its stdout when stderr is empty. Recorder schema version 4 replaced each scheduled loop occurrence's `worker_receipt_id` with the boolean `worker_invoked`. Recorder schema version 3 moved the recorder from receipts to run history: `target_stats` replaced `tool_stats`, target-result rows replaced receipt rows and their `kind` field, `output_tail` replaced each failure's `stderr_preview`, and the `state.runs` error scope replaced `state.receipts`. Recorder schema version 2 removed `snapshot_kind`, `current_session_id`, `counts`, `open_plans`, `history`, the separate plan document, and the session, plan, and decision timeline rows, which were recorded only by the removed structured-work commands. The Status document instead uses command `"status"`, schema version 4, and `outcome` string `"complete"` or `"partial"`.

Nested bounded rows serialize as `{"items": [...], "applied": N, "omitted": N|null}`. Bounded text serializes as `{"text": "...", "applied_chars": N, "omitted_chars": N|null}` and counts Unicode scalar values. Recorder root arrays remain ordinary arrays; the root `limits` object maps each root collection name to `{"applied": N, "omitted": N|null}`.

| Limit identifier | Ceiling |
| --- | ---: |
| `failures` | 10 |
| `failure_output_chars` | 400 |
| `target_stats` | 256 |
| `loop_workflows` | 1000 |
| `loop_leases` | 1000 |
| `loop_attempts` | 1000 |
| `loop_scheduled_occurrences` | 1000 |
| `loop_waiting_attempts` | 1000 |
| `loop_exhausted_attempts` | 1000 |
| `timeline` | 1000 |

`--timeline-limit 1..1000` controls recorder activity rows and defaults to 120, so the applied `timeline` limit can be below its ceiling. `--refresh-seconds` is invalid with UI JSON. Argument conflicts use the standard usage envelope and exit status 2. The removed `--plan` option is rejected as an unknown argument.

Each partial collection error is `{"scope": string, "code": string, "subject_id": string|null, "message": string}`. Scopes are `repository`, `state.runs`, and `loops`. Codes are `git_observation_failed`, `git_upstream_comparison_failed`, `git_upstream_output_invalid`, `stream_open_failed`, `stream_read_failed`, `record_too_large`, `record_decode_failed`, and `loop_observation_failed`.

A nonempty `errors` array is partial observation, not command failure: recorder documents retain `ok: true`, preserve usable data, and exit 0 after one complete JSON document is written. Status JSON likewise preserves usable data, exits 0 after successful collection, and changes `outcome` to `"partial"`. Failures before a snapshot can be constructed use the ordinary command-error envelope and a nonzero exit.

Dashboard readers cap each logical record in `runs.jsonl` at 1048576 bytes. An oversized record is skipped without allocating proportionally and yields a `record_too_large` partial error. This safety tightening does not change the append-only state format, but a schema-valid oversized legacy record that an older runtime attempted to allocate now makes UI recorder observation partial. Use `scripts/jig state diagnose` to identify the affected stream, stop Jig writers, and use the applicable archive, restore, or manual state-repair workflow before retrying.

The 0.3.0 cutover ends support for the browser server, its bookmarked URLs, and its HTTP JSON endpoints. `jig ui --json` emits the recorder document directly instead of a URL envelope. A hidden `--port` parser exists only to return a migration diagnostic with exit status 2 and may be removed in a later release. This workflow cutover does not change generated launcher command scope and remains compatible with contract version 7.

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
addressable actions but are omitted from bare repository checks.

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

Catalog inspection has two explicitly selected response projections. Omitting
the option, or selecting `--projection standard`, preserves the existing CLI
JSON shape. The opt-in `--projection agent-v1` adds a typed
`freshness_policy` object to every target returned by the workspace, component,
targets, and target views. CLI use with another info view is rejected rather
than silently ignoring the selection:

```sh
scripts/jig --json info target api:test --projection agent-v1
scripts/jig --json info workspace --projection agent-v1
```

The object reports `contract_epoch`, a `mode` of `target_freshness_v1` or
`legacy_global`, and separate `inputs_policy` and `source_state` records. Each
policy record contains its effective value, a `defaulted` boolean, and nullable
field `provenance`. Effective input values remain `whole_repository` or
`exhaustive`; effective source-state values remain `git` or `worktree`.
Generated epoch-8 defaults normally report `defaulted: true` with `inferred`
provenance, while authored declarations report `defaulted: false` with
`declared` provenance. A non-default effective value is never labeled defaulted,
even if a stale provenance record calls it inferred. Pre-8 targets report
`legacy_global`, the conservative effective defaults, and null provenance;
policy provenance keys from an epoch that did not support those policies are
not projected. Human output renders the same effective values and metadata.
This is configuration inspection only: Jig records no freshness evidence from
these policies.

MCP uses the same typed projection. Starting the server with
`scripts/jig mcp --surface agent-v1` advertises a matching strict output schema
for `jig.inspect`; omitting the option, or selecting `standard`,
keeps the baseline descriptor and response shapes. Surface selection is fixed
for the process lifetime and is not negotiated through MCP `initialize`.
Unknown projection or surface values fail during command parsing, before an
inspection runs or an MCP server starts. A caller can roll back by omitting the
option. The catalog schema remains version 1 because the baseline schema is
unchanged and the additive shape is isolated behind an explicitly versioned
projection.

Input globs alone do not describe source authority. These otherwise identical
policies declare different source authority:

```json
{
  "inputs_policy": {"effective": "exhaustive", "defaulted": false, "provenance": "declared"},
  "source_state": {"effective": "git", "defaulted": false, "provenance": "declared"}
}
```

```json
{
  "inputs_policy": {"effective": "exhaustive", "defaulted": false, "provenance": "declared"},
  "source_state": {"effective": "worktree", "defaulted": false, "provenance": "declared"}
}
```

Both may describe the same `inputs`, but the first declares that Git placement,
HEAD, and branch identity can affect the result while the second declares that
only the checked working files can.

`jig check --explain` returns `command: "check plan"`, `executed: false`, and a
`plan` object without running a command or writing run state. A newly written
plan uses run-plan schema version 4 and includes its derived `id`, configuration digest, source identity,
normalized selectors or profile, sorted targets, selection reasons, declared
effects, input digests, and dependency execution layers. These layers describe
the dependency topology. In entirely read-only plans with parallel dependency
chains, ordinary targets can start once their actual prerequisites have
validated and published successful results, even while unrelated targets in
an earlier layer remain active. Resource batches, effectful execution and
explicit fail-fast retain their ordering and safety constraints. Bare `jig check` uses
the default verification profile. An action selector such as `test` matches
that action across components; a target selector such as `api:test` is exact;
and `*` is the only wildcard and occupies a whole component or action segment.
Profiles and explicit selectors are mutually exclusive. Contract-6 legacy
aliases must not parse as canonical action, target, or wildcard selectors;
canonical selector meaning therefore cannot be shadowed by an alias.

Affected Rust plans may include optional, digest-bound `cargo_impacts`: bounded
portable package and test-target candidates from locked/offline Cargo metadata.
Generic affected selection still determines authored actions first; these facts
alone do not change execution. Ambiguous ownership, topology changes, incomplete
graphs, unsupported context, or discovery limits produce broad/unavailable
reasons. Raw Cargo IDs and absolute checkout paths are never persisted. Existing
schema-2/3 records remain readable; submitted plans must use current schema 4.

The opt-in runner tag `rust_nextest_v1` and argument tag `rust_focus_v1` are
strict versioned capabilities under contract 8 or later. Unsupported runtimes
reject the tags before execution; no shell command is inferred to be Cargo.
A selected runner carries `prepared_rust_input` schema 1: literal Cargo argv,
portable packages/target selectors, feature/platform context, scope disposition,
fallback reasons, and optional exact comparison base. Planner replay authenticates
that input. The runner retains normal supervised execution and run history. A
zero-match Nextest result is a failed
target with finding source `empty_selection`, never a passing test requirement.
Explicit target existence is independent of Cargo's default test-participation
flag. Automatic focus compares against the merge base with the default branch
(the empty tree before the first commit), the same base native checks use, and
falls back to workspace scope when that comparison cannot be resolved. Its
retired `plan_id` field is accepted and ignored. Automatic package narrowing
preserves the configured feature policy, falling back to workspace scope when
its meaning for the subset is unproved.
Metadata discovery always enforces `--locked`, independently of execution's
lock-update policy, so planning cannot create or rewrite Cargo.lock.

Actions and planned targets may also carry a bounded `resources` list. The
strict `cargo_v1` variant declares a workspace manifest, execution directory and
Cargo context; omitted lists retain legacy behavior. Declarations participate
in configuration and replay authority. Unsupported runtimes reject
the field or variant rather than silently dropping the scheduling promise.
Resource ownership is machine-local scheduling state, never an evidence
dependency. A resource wait never substitutes an earlier result for execution.
See
[Cargo resource coordination](cargo-resource-coordination.md) for supported
aliases, partial coordination, deadline and ownership boundaries.

The strict fieldless `playwright_servers_v1` resource variant explicitly opts an
authored generic read-only runner into the generated Playwright environment
contract. It owns individual loopback endpoints only without a trimmed nonempty
`E2E_BASE_URL`, and shares the same admission/lease owner. Older runtimes reject
the tag; omitted declarations keep prior behavior. See
[browser endpoint coordination](browser-resource-coordination.md) for authority,
compatibility, external URL and current-readiness limits.

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
every target failure it can execute. Execution options such as `--fail-fast`
are accepted on either side of external target selectors.

Current full-harness repositories own `.jig/file-budget.toml` and expose the
language-neutral `repo:file-budget` native action. `scripts/jig check
repo:file-budget` therefore uses the same selector, affected-planning, run-history,
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

A finished target result records its status, conclusion, timing, `exit_code`,
configuration and input digests, and findings. A target that did not succeed
also records `output_tail`: the final 4,000 bytes of each of its `stdout` and
`stderr` streams, cut at a character boundary, with `stdout_omitted_bytes` and
`stderr_omitted_bytes` counting the earlier bytes when output was cut. The field
is absent for successful targets and for targets that wrote no output. Run
records written by earlier runtimes may carry `receipt_id`, `reused_from`, and
`target_freshness` on results and `target_identity` or `target_identity_error`
on planned targets; readers drop those fields, so those records stay readable.

Run ids are durable inspection handles, so Jig does not silently expire their
events. Exact run lookup scans the journal backward to the requested run's
queued event and materializes only matching lifecycle records. Explicit
`state archive --before ... --include-runs` maintenance moves completed old
runs out of the active journal. Run archival is opt-in so the established
receipt archive command does not unexpectedly remove inspection handles.

## MCP Repository Operations

Contract v6 advertises four repository operations rather than one MCP tool per
action:

- `jig.inspect` reads the workspace, component, target, and profile catalogs or
  one durable run. Its `kind` discriminator determines whether `id` or `run_id`
  is required.
- `jig.plan_run` resolves explicit `selectors`, a mutually exclusive `profile`,
  optional `affected_base`, optional typed `comparison`, and closed per-target
  `arguments` through the same
  deterministic planner as the CLI. Effectful actions require explicit
  selectors. Native actions that need a name bind it into the immutable plan;
  unsupported or unselected-target arguments are rejected. Planning does not
  execute or write run state.
- `jig.execute_run` accepts the exact returned plan plus an optional
  `fail_fast` control. The retired `record_receipts` field is rejected as an
  unknown field, as is a plan that carries fields a current runtime no longer
  produces, such as `target_identity`; plan again before executing. Plans containing
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
per-manifest-tool discovery, calls, and response shapes. The 13 `jig.work_*`
lifecycle tools were removed without a contract-version bump; calling one returns
the ordinary `Unsupported tool` JSON-RPC error. `jig.agent_doctor` is the only
remaining tool outside the repository operations. Successful tool results always
report `isError: false`; it was previously true only for a failing
`jig.work_check`. Both planning and execution still accept the retired
`work_plan_id` field and ignore it.

## Runtime State

PR conflict validation uses independent evidence: `AUTO_MERGE` remains the worker-only conflict/whitespace baseline, while an observed-head comparison disables whitespace rules and rejects conflict markers added by the merge even when they are unchanged from `AUTO_MERGE`.

A manual loop tick keeps its durable occurrence live until the tick's [loop evidence](#loop-evidence) is recorded, then records the occurrence's terminal state in history. If recording the evidence fails, the same occurrence becomes `needs_attention` with the error `Failed to record loop occurrence evidence` and continues to backpressure manual and scheduled reentry.

`.agent/state/*.jsonl` is runtime-owned append-only memory during normal operation. Generated repos may back up, inspect, or remove these files intentionally, but application code should not edit individual records in place. Runtime-owned maintenance commands may perform validated whole-stream rewrites with recovery artifacts. Generated `.gitattributes` marks those JSONL files with `merge=union` to reduce avoidable merge conflicts between independent append-only records.

The current JSONL state file is `runs.jsonl`. Jig no longer writes
`receipts.jsonl`; an existing journal stays readable by `state export receipts`,
`state archive`, `state restore`, and `state diagnose`.

Repositories adopted before structured work was removed may also keep
`sessions.jsonl`, `plans.jsonl`, and `decisions.jsonl`. Jig no longer writes or
reads them; `state diagnose` still reports their size and integrity, and they
may be removed intentionally.

State readers should tolerate missing files by treating them as empty. JSONL readers should ignore blank lines and fail loudly on malformed nonblank records.

Jig no longer writes receipts: checks and runs record their results in run history, loop occurrences record [loop evidence](#loop-evidence), and directly executed manifest tools, `migration add`, and policy checks record nothing. Receipts written by earlier runtimes remain readable. Their records may include an `evidence` object for structured runtime-owned evidence that did not fit safely in truncated stdout or stderr previews. Check receipts also carry optional `run_id`, structured `target`, `config_digest`, `input_digest`, normalized `findings`, `finding_count`/`findings_truncated`/`findings_digest` metadata, `evaluated_at_ms`, `valid_until_ms`, and `target_freshness`; readers ignore those fields except `run_id`, which deep diagnosis joins to run history. Historical `supervised_command` evidence describes configured-command failures from those check receipts. Receipt Git metadata excludes `.agent/**`; `changed_paths` contains at most 100 sorted paths, while optional `changed_path_count`, `changed_paths_truncated`, and `changed_paths_digest` describe the full path set. Successful stdout and stderr previews used a 512-byte truncation threshold and failed previews a 4,000-byte threshold. Historical work-check batch receipts reference only children that actually started. Older receipts without the evidence or path-summary fields remain readable. Loop receipts use the tool names `jig.loop_tick`, `jig.loop_dispatch`, `jig.loop_clear_attempt`, `jig.loop_acknowledge_occurrence`, and `jig.worker_run`. A historical Codex worker receipt used its separately bounded last-message file as authoritative `stdout_preview`; provider stdout is diagnostic transcript data in additive `evidence.provider_stdout_preview`. `provider_stdout_preview_truncated` reports bounding of that evidence preview, and `provider_stdout_truncated` reports truncation by the process supervisor. The legacy additive `stdout_truncated` evidence field remains an alias for provider-transcript truncation, while `stderr_truncated` continues to describe provider stderr. Historical Codex review receipts from the removed `work review` command use `evidence.kind = "codex_review"` and store normalized findings there, capped to the first 100 findings with long finding fields shortened; raw finding and actionable counts remain available so truncation does not hide a failing review. Their receipt `exit_status` is the review verdict, while `evidence.codex_exit_status` is the underlying Codex process status. They also include short stdout/stderr previews for failed review debugging. Historical Codex refinement receipts from the removed `work refine` command use `evidence.kind = "codex_refine"` and store the refinement iteration, optional refinement profile metadata, reviewed gate ids, finding fingerprints, and finding count.

A legacy `jig-current-session.txt` session pointer, resolved through git or under `.agent/.cache/`, is stale cache state. No current command reads or writes it.

Worktree-specific loop lease authority applies to workflow execution leases and attempt budgets. PR-manager branch leases instead use `jig/loop/branch_leases.json` below the repository's common Git directory, serializing mutation of one remote branch across linked worktrees; manual and scheduled PR-manager runs validate that repository-common authority before claiming an occurrence, and operators must stop older dispatchers during this writer cutover. GitHub snapshot normalization derives PR head identity strictly from `headRepositoryOwner.login` plus `headRepository.name`, and policy code never falls back to a version-dependent raw composite field. Review-reply idempotency binds the trusted-feedback generation and reply intent in addition to the repair commit. An unexecuted PR retry may clean only a worktree created by that retry; a pre-existing retained checkout remains operator evidence and is never force-removed by the later pre-execution failure.

Review-thread replies and resolution re-fetch the complete bounded comment history and live PR head together, compare the ordered ID, update-time, and body generation with the worker snapshot, and require the head to equal the pushed repair version; a current-intent Jig reply is the only excluded addition. Retained-worktree filesystem authority never comes from lossy display text: non-UTF-8 Unix path bytes use a tagged reversible JSON representation, and malformed encodings fail closed as retained.

Git repositories keep the authoritative schedule ledger, initialization marker, and lock in the checkout's worktree-specific Git metadata. This authority is outside a Codex `workspace-write` worker's writable surface and is the mutation commit point. Legacy-ledger migration and ordinary transitions use one lock order—legacy cache first, protected authority second—and every authoritative publication occurs while the protected lock is held. Authority resolution accepts Git's documented `.git` directory and regular `gitdir:` pointer-file layouts; a symbolic-link `.git` entry fails closed because following mutable metadata redirection would weaken that boundary. Protected initialization is a recoverable two-phase cutover: Jig first durably refreshes the public recovery ledger and marks protected cutover pending, then publishes protected state, and only then records final protected authority before a mutation may run. Pending cutover resumes from that recovery ledger only with resolvable Git authority; pending and final markers both fail closed when Git metadata is unavailable, so a surviving replica cannot become authoritative after a partial cutover or protected-authority loss. Deleting, replacing, or temporarily preventing later ledger-replica publication cannot erase occurrence history, fail an already committed transition, or permit the same occurrence to rerun; a later authoritative write retries the replica.

PR-manager outcome finalization explicitly refreshes the branch lease before inspecting or removing its deterministic checkout. A failed refresh retains the checkout without touching it because cleanup authority is ambiguous. The shared cleanup boundary revalidates the exact lease owner before every inspection and removal step, and in-flight Git cleanup is cancelled if renewal loses ownership. Cleanup finishes before the branch lease is released; authority loss or a later release failure remains explicit attention without retrying cleanup. PR action JSON uses the same reversible Unix path representation as retained-worktree authority, while cleanup receives the native path directly instead of reconstructing filesystem authority from JSON; Git metadata paths remain native byte sequences through pointer parsing and command output.

An active occurrence owner durably reserves its deterministic task or PR worktree path before Git may create or reuse that checkout. A crash after reservation therefore leaves any created path attached to stale attention, so acknowledgement cannot admit a shared-root worker while the checkout remains. Schedule locks are opened relative to no-follow directory capabilities after managed-path validation, and every cleanup, read, marker update, and durable publication under those locks stays relative to the retained capabilities. Read-only status projections retain their lock-free atomic snapshots, while a PR-manager attempt read that decides whether work may start serializes with attempt mutations under the attempt lock.

Protected lease and attempt replacements sync both the new file and its containing Git-metadata directory before publication returns.

Scheduled loop occurrence state is mutable machine-local runtime state, not append-only agent memory and not disposable cache. In Git repositories its source of truth and serialization lock live in worktree-specific Git metadata, with a compatibility replica under `.agent/runtime/loop/`; non-Git fixtures use that checkout-local path directly. Preserve Git metadata and retained worktrees with the checkout used by an external scheduler. Retained task and PR-manager worktrees also live below `.agent/runtime/loop/` so cache cleanup cannot destroy reported work. Lease ownership and retry-attempt budgets use sibling protected `leases.json` and `attempts.json` authorities in the same worktree-specific Git metadata directory; non-Git fixtures retain the `.agent/.cache/loop/` representation. Coordination JSON reads and writes are limited to 8 MiB per file, including growth observed after open, so damaged state fails as a bounded diagnostic instead of exhausting process memory. This is a deliberate safety tightening: pre-existing files above 8 MiB fail closed and identify the exact file that must be inspected or repaired while loop dispatchers are stopped; Jig does not destructively discard coordination authority during upgrade. Every Git-backed entrypoint that can mutate the schedule ledger first proves that the runtime root is ignored, regardless of workflow kind; isolated Codex checkout additionally verifies its task-worktree root. Schedule storage and retained task or PR worktree paths reject every symlinked managed component, and a dangling `.git` entry is an invalid repository boundary rather than a non-Git fallback. An existing deterministic PR path is reusable only when Git's stable NUL-delimited worktree registry resolves to the same directory and its no-follow regular `.git` pointer and administrative back-pointer identify a linked worktree in the repository's common Git directory. The first Git-backed lock-taking lease or attempt access migrates any legacy checkout-cache value under an ordered lock pair and replaces the old record with a migration marker that earlier runtimes cannot deserialize. Later reads and mutations use only protected authority, so a repo-mode workspace-write worker cannot release leases, forge attempt budgets, or redirect the parent through checkout-local cache paths. Read-only attempt and dispatch-window observations use atomic snapshots without cleanup or lock-taking side effects; mutation entrypoints establish or migrate protected authority before writing. Before publishing any durable occurrence claim, dispatch fails closed on unparsable lease JSON and resets unparsable attempt JSON with additive `attempts_reset` state evidence; other coordination-state failures also fail closed, and corruption observed after workflow work begins remains explicit state-error evidence. A setup failure or cancellation after a claim but before worker start removes the unexecuted claim and reports an additive typed retryable pre-execution action. If cleanup retains a checkout, the occurrence instead becomes `needs_attention` with that path. Workflow-lease finalization requires the same unexpired owner under the protected lock; ownership loss after execution begins makes the durable occurrence require attention instead of reporting clean success. Renewal lock waits are capped by the remaining cancellation/finalization window; schedule-ledger transitions apply one deadline to the ordered legacy and authority locks rather than allowing each lock a fresh timeout. If stale reconciliation records generic unacknowledged attention before a worker returns, only the original owner may enrich that exact evidence-free record with its late terminal evidence, and the original reconciliation time remains authoritative. A current runtime migrates schema-1 cache and schema-2 or schema-3 durable occurrence ledgers to schema 4 before dispatch. Schema 4 records whether each new occurrence uses the shared checkout; older markerless `running` or `needs_attention` records are conservatively treated as potentially shared until they are finalized or acknowledged. Earlier runtimes reject the resulting schedule, lease, and attempt migration markers, preserving the downgrade barrier. A protected `schedule.initialized` marker beside the authoritative ledger preserves the fail-closed initialization fact even when disposable cache or the checkout-local replica is removed. Operators must stop older dispatchers during these writer cutovers and must not downgrade after protected state is published. Dispatch keeps ambiguous scheduled occurrences and exhausted per-item attempts as separate, nonduplicated attention sources because they require different repair commands, derives its unsuccessful status from either source, and records cancelled or failed post-work state observations in the dispatch receipt. The durable claim transaction rejects older work after a newer occurrence is recorded, keeps any shared-repository claim mutually exclusive with every live or unacknowledged workflow claim, and blocks a shared-root worker while any retained managed worktree still exists, including acknowledged evidence. Workflow-local claims remain mutually exclusive within their workflow and also wait for a live or unacknowledged shared-root claim. Status and acknowledgement share the same claim-expiry predicate, so direct acknowledgement atomically reconciles an expired `running` record before terminalizing it; acknowledgement releases the occurrence-state blocker, subject to retained-worktree backpressure. The schedule locks remain held through acknowledgement receipt publication; its lightweight state receipt deliberately omits Git metadata so this transactional critical section does not run repository inspection. Schedule and receipt lock acquisition share one bounded operation deadline and observe cancellation, and a receipt failure known to precede any append restores the prior attention state before another dispatcher can observe the transition. Attempt repair uses the exact persisted workflow and item keys, including after a workflow is removed or renamed; schema-version-1 clear-attempt evidence keeps `workflow` as an object and adds `workflow_id` as its explicit string key. Clear-attempt state and its receipt use the same compensating boundary, so a receipt failure known to occur before any append restores the exact prior attempt record rather than reporting an unrecorded repair. A post-write receipt failure retains the committed state because the receipt may already be visible; returning an error without compensation avoids publishing success evidence for state that was deliberately reverted. Schema-version-1 dispatch evidence keeps `skipped_count` as the broad number of due occurrences not executed, including abandonment-state failures, while additive `deferred_count` identifies authority contention, including a held workflow lease or overlapping live occurrence. `loop tick` and `loop run` also treat machine-global `needs_attention` as unsuccessful even when a workflow selector points at a different workflow; selectors choose work, not the scope of runtime-health reporting. `loop status --workflow` instead scopes every workflow-owned section in its diagnostic projection. Status uses one sampled clock for schedule and attempt classification. A status schedule-evaluation error is scoped to its workflow and top-level `state_errors`, so other loop state remains inspectable in an unfiltered status report. Stale adopted repositories can refresh the managed rules with `scripts/jig update --recopy`. A retained isolated-task or PR-manager worktree blocks another manual or scheduled claim for the same workflow until the operator removes the reported path, bounding evidence growth without automatic data loss.

Bounded terminal occurrence history uses finish/start recency rather than scheduled time alone, so timestamp-zero manual occurrences retain the newest records. The latest scheduled occurrence is reserved within that bound as the dispatch watermark, preventing newer manual history from making an already executed cron instant due again. UI projections label manual records as manual runs and use their start time instead of displaying the zero sentinel as the Unix epoch. Successful unexecuted abandonment suppresses only the expected typed ownership-loss diagnostic created by deliberately removing that claim; any other renewal shutdown error remains state evidence.

Normalized GitHub loop observation is limited to 16 MiB after serialization and omits duplicate raw payloads; an occurrence's evidence document drops that snapshot when it exceeds the evidence size limit. Dispatch command output retains each detailed nested tick, and dispatch itself records no separate evidence. Cancellation records every remaining review-thread intent as unattempted evidence. Snapshot and review-thread request supervision preserves the subsecond remainder of aggregate deadlines, including the minimum valid one-second command timeout. Post-push review updates additionally give each unique actionable intent its own command-timeout and request slice within the aggregate cap, and snapshot request counts include only requests that passed pre-launch budget validation.

Manual ticks join this durable safety boundary after acquiring their workflow execution lease: finished manual records stay in occurrence history with their evidence, while retained or ambiguous outcomes also backpressure later manual and scheduled work. If a staged manual record expires before its evidence is recorded, stale reconciliation preserves the staged diagnostic and adds expiry context instead of replacing evidence with a generic message. A manual tick that overlaps a live occurrence returns structured `waiting` evidence without starting a worker. Occurrence-attention aggregation is machine-global for tick and run, including attention owned by another workflow. Definite occurrence-claim ownership loss is terminal immediately; only transient renewal failures use the bounded retry policy. A PR-manager worker cancelled after process start preserves its worker evidence and retained worktree as `needs_attention`; malformed worker output, a failed post-worker Git step, and post-start branch-lease loss use the same attention boundary whenever the checkout contains uncommitted changes or a new local commit. A clean unchanged failed checkout is removed and remains an ordinary bounded attempt. PR-manager setup failures, globally incomplete GitHub candidate lists, and cancellations before worker start are typed as unexecuted and do not consume the scheduled occurrence or attempt budget. Worktree preparation only returns a partial or registered checkout as a cleanup candidate; the shared outcome finalizer performs any cleanup after an explicit branch-lease refresh. Cleanup failure retains the exact path as attention rather than retrying destructively after releasing authority. Other unambiguous PR-manager worktrees are removed after the same branch-lease refresh and before lease release, while ambiguous, authority-lost, or cleanup-failed outcomes remain retained. Occurrence backpressure protects a retained PR-manager worktree after acknowledgement until the operator removes the reported path, matching isolated-task admission and bounding retained history. Side-effectful attention consumes the tick, while passive `exhausted_attempt` attention can allow another eligible PR to be considered. PR-manager worktree names below `.agent/runtime/loop/worktrees/prs/` are derived from a digest of the workflow ID, so accepted IDs containing path separators cannot escape the durable managed root. Remote PR branch names are fully qualified as `refs/heads/...` before reaching option-parsed Git arguments. Worktree preparation requires that ref to equal the immutable head from the GitHub snapshot; publication requires the worker result to descend from that head and uses an exact expected-head lease, so an intervening advance, rewind, or deletion fails stale rather than recreating or overwriting the remote state. The workspace-write worker edits files only; before commit, the parent stages resolutions, requires an index with no unmerged entries, checks worker-authored whitespace against the cached pre-worker tree, and rejects conflict-marker diagnostics present relative to both merge parents. This preserves marker examples inherited from either parent without allowing Git-introduced merge markers through. Attempt state retains both the observed and pushed head so GitHub snapshot lag cannot reset a repair budget. If attempt-state persistence fails after repair work begins, the action keeps its worker, push, lease, and worktree evidence as `needs_attention` instead of returning an evidence-free error. Review text reaches the unattended worker only after GitHub reports the comment author's effective repository permission as `admin` or `write`; permission lookup fails closed, untrusted threads do not trigger repair, and the worker projection omits PR titles, raw GitHub payloads, and untrusted comment bodies. Nested review-comment connections are paged backward through older comments to a bounded limit; a missing cursor, changing count, duplicate, or exhausted limit marks that PR incomplete and prevents repair only for that PR, while completely observed PRs remain eligible. The top-level review-thread connection applies the same stable-count, unique-ID, and cursor-progress checks, and resolution is skipped when the comment count or latest comment differs from the worker snapshot. Empty review-thread IDs are rejected before deduplication, and a requested reply or resolution skipped by witness revalidation is reported as skipped at both the operation and post level. A truncated open-PR list still prevents all repair because the repository-wide candidate set is unknown. One snapshot client also bounds the composed observation to 256 GitHub requests, 16 MiB of cumulative responses, 10,000 normalized review items, and at most ten minutes; exhaustion fails before attempt or branch mutation. Incomplete comment histories skip collaborator-permission lookups and remain untrusted. Review-thread replies pass their potentially large body to `gh api` through a temporary file field rather than one process argument. For shared-repository Codex tasks, excluding `.agent/state/receipts.jsonl` from ordinary dirtiness is conditional on an exact append proof. Jig creates and locks the receipt journal inode even for the first append so current and legacy writers retain a common cutover lock. After both cutover locks are held, Jig verifies that the locked inode is still the current journal, releases both handles and waits for the bounded poll interval if an atomic rewrite replaced the inode while lock acquisition waited, and appends through the verified handle. Jig uses short exclusive writer windows to open and identity-check receipt journal snapshots; prefix hashing and bounded append parsing run outside the lock, as do Git index probes and the worker. The active pre-worker journal is limited to 64 MiB and the snapshotted append to 16 MiB; use `state archive` when the active stream reaches that operational bound. The Git index entry, journal identity, and pre-worker byte prefix must be unchanged, and Jig's expected worker receipt must be the only appended record; another schema-valid append is indistinguishable from a worker forgery and makes the result require attention. Post-work lease, attempt, and occurrence observations use the read-only cancellation-aware query paths, so cancellation does not wait behind coordination locks or initialize authority. A clean repo-mode commit stops the current multi-workflow dispatch so the next invocation reloads settings and prompts from one new repository revision. Dirty or unverifiable final repository state also stops that dispatch and requires attention; the cutoff is carried as typed completion authority even when presentation or tick-receipt publication fails. `loop tick`, `loop dispatch`, and `loop run` map `ok: false` reports to a nonzero process status; diagnostic `loop status` returns zero when it successfully emits a report even if that report says `ok: false`.

An authenticated existing PR repair worktree is removed and recreated after branch-head and occurrence reservation preflight; it is never treated as a cache because ignored files and nested repositories are outside ordinary Git cleanup. The path's branch component combines a bounded readable prefix with a digest of the complete branch name, preventing both filesystem component overflow and sanitization collisions. Jig supplies its PR-manager author identity only to the merge or commit command, leaving repository-local identity configuration unchanged. A conflicted `ort` merge validates the worker result against Git's `AUTO_MERGE` tree, so incoming base-branch whitespace is not misclassified as worker output. Immediately before commit, Jig recollects the complete bounded pull-request review-thread snapshot and retains the completed local repair without pushing if the PR head or any actionable thread's membership, trusted-author projection, content generation, or viewer capability differs from the worker snapshot. Before either a later reply or resolution mutation, Jig recollects the complete live review-thread witness and skips the mutation when feedback was edited, added, or resolved after that snapshot. Only GitHub-confirmed viewer-authored marker comments are excluded from the trusted-feedback generation, so trusted human quotations of marker text still advance the witness. A missing or false observed viewer capability skips the corresponding reply or resolution without issuing a known-impossible mutation.

`scripts/jig state summary` reads run history. It returns `counts` with `runs` (queued runs), `target_results`, and `failed_target_results` (conclusion `failure`, `timed_out`, or `blocked`), plus `recent_target_results`: up to 10 of the newest finished targets, newest first, each with `run_id`, `target`, `status`, `conclusion`, `exit_code`, `started_at_ms`, and `ended_at_ms`. Human output prints `Runs: N` and `Target results: N (F failed)`. A run-history record above 1048576 bytes fails the summary rather than being read into memory; `state diagnose` identifies it. `scripts/jig state diagnose` is read-only; `--deep` adds receipt-payload analysis and a receipt-to-run linkage check. Deep diagnosis joins each receipt `run_id` and each child receipt named by historical `jig.work_check_targets/v1` or `jig.work_check/v2` batch evidence to the active run journal, then to run archives under `.agent/.cache/state-archives/runs-*.jsonl.gz` and manifested run backups under `.agent/.cache/state-backups/` for any run the journal lacks. The `run_linkage` object lists affected run IDs, child receipt IDs, and batch receipt IDs with explicit counts and truncation flags, and distinguishes `missing` (absent from every local source, which does not prove deletion elsewhere), `unverifiable` (damaged journal, unreadable or tampered source, incomplete archived lifecycle, or unrecognized events), `inconsistent` (journal events that do not form a valid lifecycle), and `recoverable_from_backup` (an exact manifested backup holds the lifecycle). Recovery status proves exact-source availability only: its structured facts require manual destination preflight and current-journal comparison, expose nonterminal runs and held worker leases that block whole-stream replacement, and diagnosis does not emit a directly runnable restore command. Live and completed journal lifecycles, verified complete archived lifecycles, and receipts without a run reference are never reported as orphans. Diagnosis never reconciles runs, creates leases, caches, or indexes, or rewrites a stream; a failed or truncated scan yields `incomplete` rather than `clean`. Shallow mode reports the check as `not_checked`. `ok` reports command completion only; integrity is summarized in the `integrity` object. Recommendations preserve existing evidence: export affected receipts and record a decision naming the affected IDs, or restore a verified exact backup after preserving newer appends. They never fabricate `queued`, `target_completed`, or `completed` events, and rebuilding derived caches never restores missing history. Diagnostics also report disk usage from local maintenance artifacts under `.agent/.cache/state-backups/` and `.agent/.cache/state-archives/`. `scripts/jig state compact sessions` was removed with work sessions. `scripts/jig state restore --backup <directory-or-manifest>` still verifies a sessions backup it created and restores the exact pre-compaction stream.

`scripts/jig state archive --before <YYYY-MM-DD|unix-ms>` writes eligible old receipts as gzip JSONL under ignored `.agent/.cache/state-archives/` and rewrites `receipts.jsonl`. With explicit `--include-runs`, it also writes completed run-event groups to a separate artifact and rewrites `runs.jsonl`. Apply mode first reconciles an abandoned nonterminal run to `blocked` when its stable worker lease proves that no worker remains; ordinary foreground execution errors also terminalize their accepted run before returning. Run archival then refuses while any known run is nonterminal so rewriting cannot invalidate a live reader's durable byte cursor. A read-only preview never performs reconciliation and therefore reports abandoned runs until inspection or apply mode repairs them. Applying both streams prevalidates both and archives the harder run journal first; if the subsequent receipt operation fails, the error identifies the completed run artifact and exact recovery backup. Run archive and restore prevalidation applies the same complete queued-plan structure contract as execution, including unique targets, complete execution-layer coverage, and dependency ordering; structurally invalid hand-edited or cross-version journals are rejected before any replacement. Before each replacement Jig creates a complete manifested stream backup under `.agent/.cache/state-backups/`; `state restore --backup ...` recovers that exact stream's pre-archive bytes and physical order. A changing run-journal restore refuses while any current run is nonterminal or any current run worker still holds its lease; an identical checksum no-op remains safe. Use `--dry-run` to validate the selected streams and inspect counts without mutation. `scripts/jig state export receipts --before <cutoff> --output <file.jsonl.gz>` writes selected receipt records without changing active state and refuses to replace an existing destination. Legacy `.agent/state/archive/` files remain untouched and appear in diagnostics.

Compaction and archiving change only the current working-tree streams. They never rewrite Git history or remove blobs reachable from existing commits. Artifacts under `.agent/.cache/` are ignored local recovery aids, not durable off-machine backups; command output identifies the paths and checksums that should be copied elsewhere when long-term recovery is required.

Applying compaction, archive rewrites, and restore require a writer cutover from pre-cache-lock Jig runtimes: stop older Jig processes before mutation. Current runtimes serialize on the repository state lock, while a legacy writer already waiting on a pre-opened inode cannot safely follow an atomic replacement. Keep the newest recovery backup until the rewrite is verified, copy long-lived artifacts outside `.agent/.cache/`, and remove obsolete cache backups or archives; diagnostics report their separate disk usage.

The removed work commands used the `jig.work_*` CLI and MCP namespace, but their state-operation receipts keep their historical tool names and remain readable for compatibility with existing receipt history and filters:

- `jig.session_start`
- `jig.session_end`
- `jig.plans_open`
- `jig.plans_append`
- `jig.plans_close`
- `jig.decisions_add`

`jig.plans_close` covers both terminal transitions. Its receipt `args.operation` is `plan_close` for a successful completion and `plan_retire` for a non-success retirement, and a retirement receipt additionally carries `disposition`, `reason`, and `superseded_by`. Existing receipt filters and history keep working because the tool name is unchanged.

`work-links.jsonl` is an additive, versioned join and does not change the legacy plan journal. A version-1 record names its event and plan, the `beads` provider, a portable tracker-workspace identity, an exact issue ID, tracker root `.beads`, observation time, and bounded title, description, and acceptance-criteria fields. The snapshot retains those task fields separately and covers them with one domain-separated digest; it does not collapse them into an ambiguous presentation string. The record also identifies whether the link was created at plan start or attached later. A plan has at most one distinct link. Exact retries are idempotent. Line-union records for the same plan and issue converge on the lowest event ID as the canonical immutable snapshot; a different issue or a repeated event ID with different semantic JSON is a conflict, never last-write-wins. Snapshot text is historical context rather than synchronized issue authority. Absolute database, checkout, and source paths are invalid durable fields.

A work-link record is committed only when its physical line ends in a newline. Appends sync file data and the containing directory before returning. A malformed or unterminated record blocks authoritative writes; an exact visible retry re-confirms both durability boundaries. Readers enforce the 2-MiB record ceiling while streaming, and state diagnosis uses that ceiling before semantic inspection. Projection retains a fixed semantic fingerprint rather than the full JSON value for each unique event, folds each plan to compact authority state, bounds stored diagnostic samples, and retains a full canonical record only for the one requested plan. At most 100,000 unique event identities and 100,000 known plan identities may participate in one projection. A writer admits its candidate to the same in-memory projection under the journal lock before appending, so crossing either ceiling is unsupported authority and blocks the write without modifying the journal. Exact retries at a ceiling remain successful because they add no identity. Unknown schema or provider data remains inspectable history within those limits but cannot authorize a link. Maintenance does not compact, archive, or restore this journal and must preserve its unknown bytes.

Snapshot conversion preserves the normalized issue's title, description, and acceptance criteria exactly, including text above 256 KiB. Description and acceptance criteria have no separate field-size ceiling narrower than the serialized journal record. The Beads reader's 1-MiB record limit leaves room for the work-link metadata within the 2-MiB journal limit. The writer checks the complete serialized record, including JSON escaping, and rejects overflow without truncating requirements or appending partial history. Exact retries continue to return the original snapshot after the exported issue changes.

Repository contract epoch 11 is reserved for work-link writes. The runtime can read that capability while generated repositories remain at current epoch 8 until the public linking workflow is complete. Epochs 9 and 10 remain reserved historical receipt-freshness semantics and are not valid manifest epochs. A repository below epoch 11 can diagnose absent or existing link history but cannot append it. Until epoch 11 becomes current, launcher compatibility does not advertise it as active.

### Loop evidence

Each loop occurrence's tick records one durable evidence document: what the tick
observed and did, its actions, and the worker runs those actions describe. It is
stored as `<git-dir>/jig/loop/evidence/<sha256 of the occurrence id>.json`
beside the protected schedule ledger, outside the checkout a worker can modify;
repositories without Git metadata use the ignored `.agent/runtime/loop/evidence/`.
The document carries `schema_version` 1, `occurrence_id`, `workflow_id`,
`started_at_ms`, `ended_at_ms`, and `tick`, which holds the tick's `status`,
`idle`, `observed`, `actions`, lease, attempt, attention, and state-error facts.
A document above 4 MiB replaces the observed snapshot with an omission summary,
then each action with its kind, status, item key, and error. Evidence is kept
while its occurrence remains in the schedule history, so it follows that
history's retention; each later write drops documents whose occurrence has left
the schedule. Scheduled ticks record evidence for the occurrence dispatch
claimed; a manual tick records it only when it claimed its own occurrence.
Loop maintenance commands (`clear-attempt` and `acknowledge-occurrence`) change
state directly and record no evidence.

A worker run appears in its action as `worker`, a `worker_run` object with
`schema_version` 2 describing the run: `purpose`, `status` (`passed`, `failed`,
`cancelled`, or `error`), `started_at_ms`, `ended_at_ms`, `exit_status`, the
model, approval policy, sandbox, and other invocation settings, `error`, and
provider stdout and stderr previews bounded to 4,000 bytes with truncation
flags. Codex task actions also keep the authoritative last message as `output`
and the provider transcript as `provider_stdout`. Scheduled occurrences record
`worker_invoked` when a worker invocation was attempted, including one
cancelled before its process started. Occurrences recorded by earlier runtimes
keep their `worker_receipt_id`, which also counts as invoked.

`scripts/jig loop show <occurrence>` reports one occurrence: `occurrence` (its
status view, or `null` once it has left the schedule), `evidence` (the document
above, or `null` when none was recorded), and `legacy_worker_receipt_id` when an
earlier runtime recorded the worker run as a receipt. It fails with a pointer to
`scripts/jig loop status` when neither the occurrence nor its evidence exists.
Human output summarizes the status, errors, each action, worker status and
exit, and the last lines of worker output. `loop tick` returns the
`occurrence_id` it recorded, or `null` when it did not own an occurrence, such as
a manual tick blocked by attention.

### Beads-compatible task snapshot

The optional `[work.tracker]` configuration selects a repository-local, read-only Beads JSONL snapshot. Jig's task-data contract is the export format, not a `br` executable or SQLite schema. Current `.beads/issues.jsonl` and legacy `.beads/beads.jsonl` are supported only when exactly one exists. The reader opens the repository, then opens `.beads` once without following a link, and performs export selection, leaf opening, and final identity witnessing relative to that pinned directory capability. On Unix the leaf is acquired with no-follow and nonblocking flags before descriptor type validation, so replacing a validated regular file with a FIFO cannot hang the reader. Replacing the `.beads` pathname cannot redirect an in-progress read outside the repository. A non-regular or changing leaf, or an export selection that becomes ambiguous during the observation, is rejected rather than combined across generations.

The `beads-rust-jsonl-v1` profile validates the entire bounded export, rejects duplicate JSON keys and duplicate issue IDs, and exposes exact-ID normalized issue snapshots. Required known fields retain strict type, Beads prefix/hash identity, timestamp, and title-size checks; `:` and `#` are valid issue-prefix characters. Long producer text and additional string fields are accepted up to the profile's record and export ceilings, while consumed text remains NUL-free. Status and issue-type values are NUL-free producer-owned strings; workflow-specific operations may interpret particular values, but the read boundary does not reject an export merely because the producer extended either vocabulary. Tombstoned and missing IDs remain distinct lookup failures. The normalized semantic revision covers workspace and issue identity plus title, description, and acceptance criteria; mutable status, assignment, and update time remain observations.

Doctor is the only public consumer in this foundation milestone. It validates JSONL without spawning a process, locating `br`, opening SQLite, importing, exporting, or writing task data. Tracker configuration alone does not start Doctor's process-wide signal session, and process-session retirement cannot invalidate a completed tracker result. Configured tracker results include `freshness: "not_checked"`; `ready` means that the export is readable, not that it contains the latest tracker edits. A configured manual export can therefore be stale relative to a local Beads database; the repository's documented export handoff remains responsible for freshness and privacy cleanup.

Native task mutation is intentionally absent. A later `jig beads` writer must preserve fields it does not own, publish a complete snapshot atomically, serialize ownership handoff with `br`, and report divergent state instead of assuming ordinary auto-import performs a three-way merge. Claim, backlink, comment, and close behavior will be Jig operations with their own acceptance policy; they are not promised to emulate every `br` behavior.

## Removed Work Commands

`jig work` and its 13 subcommands (`goal`, `start`, `append`, `check`, `gates`,
`evidence`, `review`, `refine`, `decide`, `receipts`, `status`, `finish`, and
`retire`) were removed without a contract-version bump. A hidden `work` stub
remains only so every former invocation, including `jig work --help`, fails as a
usage error with exit status 2 and the message "`jig work` was removed; validate
changes with `jig check COMPONENT:ACTION` and inspect recorded state with `jig
state summary`". In `--json` mode it writes the standard `ok: false`,
`error.kind: "usage"` envelope. `jig state summary` now summarizes run
history. The `partial_completion` error data that
`work finish` and `work retire` reported no longer occurs in CLI JSON or MCP
errors. The matching MCP tools are covered in
[MCP Repository Operations](#mcp-repository-operations).

Work-gate evaluation is removed everywhere. `jig status --json` schema 3 and the
`jig ui` recorder carry no work or gate fields (see
[Dashboard And Status Output](#dashboard-and-status-output)).
`jig status --freshness-timeout-ms` is still accepted for compatibility but is
hidden and ignored; it only bounded gate evaluation.

Receipt reuse is removed with it; every check run executes its targets. Checks
no longer record receipts or target freshness at all (see
[Runtime State](#runtime-state)). Older reused-evidence, `target_freshness`, and
work-check batch records remain readable.

`.jig.toml` `[work]` is still parsed and validated strictly, so existing
repositories load unchanged and the execution-authority digest does not change.
`receipt_metadata` and `tracker` keep working. `checks` and `gates` still define
the default check profile for legacy contract v2–v5 repositories and adoption's
gate preview; `iteration_profile` and `refinements` are accepted with any value but
ignored, and `jig update` drops them.
Generated repositories still render `[[work.gates]]`. See
[Configuration](configuration.md) for the accepted keys.

`--plan-id` (on `check`, `run`, `migration add`, `sqlx`, and similar commands)
and MCP `work_plan_id` are still accepted but ignored: runs no longer record a
plan. Jig no longer reads plan, session, or decision streams or
`.agent/plans/*.md`, so plans that were open at upgrade are not listed anywhere
and `state archive` no longer retains their receipts and runs.

### Tracker receipt metadata

A repository that uses the root `.beads/` directory only for issue tracking can
explicitly classify it as receipt metadata:

```toml
[work]
receipt_metadata = ["beads"]
```

This is an ownership declaration that no application, test, build or policy
check consumes that tracker store. Leave it unset if a check validates tracker
content. The conservative default includes `.beads/`; opting in excludes only
that root store from the committed and working-tree source identity behind run
input digests and the file-budget lifecycle's source check. Changing this
configuration changes later input digests. Arbitrary paths and globs are
rejected. Source, packaged documentation, runner and configuration changes still
change the source identity, as do nested fixture directories named `.beads`.
This option is separate from `repository.affected_ignore`, which
only affects target selection. It does not turn target input digests into
per-target cache keys.

## Action Input Declarations

Contract epoch 8 adds two optional action declarations, `inputs_policy` and
`source_state`. Jig validates and reports them, but it no longer records target
freshness, so neither declaration changes what a check runs or records. Action
`inputs` alone drive affected selection; see
[Repository Catalog And Check Plans](#repository-catalog-and-check-plans).

| Declaration | What it declares |
| --- | --- |
| `inputs_policy = "whole_repository"` | The complete eligible repository source can affect the result. This is the default. |
| `inputs_policy = "exhaustive"` | The declared `inputs` cover every repository file that can affect the result. |
| `source_state = "git"` | Committed, index and current source, plus the HEAD commit and branch, can affect the result. This is the default. |
| `source_state = "worktree"` | Only the current working files can affect the result; staging or committing unchanged content cannot. |

Validation:

- Both fields are rejected in pre-8 source and manifest actions, including an
  explicitly written default.
- Authored `.jig.toml` actions and resolved manifest actions must agree on the
  defaulted values. Field provenance is recorded under `inputs_policy` and
  `source_state`.
- `exhaustive` requires non-empty `inputs`, and those inputs cannot contain a
  `.git` path segment.
- Native actions reject `worktree`; they keep Git and prepared comparison
  authority.
- Unknown values are configuration errors.

Generated checks keep the defaults. `jig update` and recopy preserve explicit
authored values and their provenance. The `--projection agent-v1` inspection
projection and `scripts/jig info freshness` report effective values, and
`info freshness` can produce a reviewed patch that declares `worktree` or
`exhaustive` for selected read-only command checks.

Receipts and run records written by earlier runtimes carry a
`target_freshness` object derived from these declarations. They remain
readable, and current readers ignore that metadata.

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

## Foreground repository actions

With no selectors or `--profile`, `jig run` executes the repository’s default check
profile. Use `jig run --explain` to inspect that selection first.

`jig run [SELECTOR ...]` exposes the repository action planner and durable execution
engine used by MCP. It accepts `--profile`, `--affected BASE`, `--explain`,
`--fail-fast`, global `--json`, and the native `--comparison-*` options supported
by `jig check`. The retired `--plan-id` is accepted and ignored; the removed
`--no-receipt` is rejected. Existing command and native
actions work in their supported contract epoch; sources older than v6 receive
migration guidance. Contract v8 adds repeatable `--arg TARGET:NAME=VALUE` bindings,
for example `jig run api:migration-add --arg api:migration-add:name=create_examples
--approve-effect worktree`. The canonical target must be selected, either directly
or in the resolved dependency closure. Bindings never select targets themselves.
MCP `jig.plan_run` accepts the same values as
`"arguments": {"api:migration-add": {"name": "create_examples"}}`.

Arguments use only named strings declared by the action. Unknown, missing required,
duplicate, forbidden-empty, oversized, NUL-containing, and unselected-target
arguments fail before execution. Keys are sorted and empty target maps omitted
before hashing; string bytes, whitespace, Unicode, and embedded `=` are preserved.
An omitted optional value differs from an explicitly supplied empty string. Values
are included in immutable plans and durable run records, so these are ordinary
non-secret inputs. MCP rejects duplicate JSON keys before parsing a request.

Versions 6 and 7 retain their existing native migration `name` input contract:
the name must not be blank or start with `-`, and has no declared byte limit.
The native writer retains its existing filename slug conversion and filesystem
limits. Version 8 writes a bounded `name` declaration to both source and resolved
native migration actions. V8 names are required, nonempty, at most 200 UTF-8 bytes,
cannot start with `-`, and must contain an ASCII alphanumeric character.
For native migrations, `jig migration add NAME` uses the same epoch-specific
validation as target execution;
MCP v6+ callers use `jig.plan_run` and `jig.execute_run`. Historical
plans remain deserializable (including `arguments: {"name": "..."}`); execution
still requires revalidation against current source and configuration. A changed
epoch or declaration requires a fresh plan, without rewriting historical records.

V8 argv runners bind declared strings only to whole argument
positions, preserving literal bytes; missing optional values omit their positions.
Programs, working directories and environment entries remain checked-in literals.
Executable text without an interpreter header fails instead of invoking a shell.
V8 shell runners explicitly reference checked-in Bash commands and accept no
generic argument declarations or interpolation. An argv action's legacy tool
aliases keep `kind: "command"` but omit the shell `command` key; the owning
action supplies their program, arguments, directory and environment. Command-backed migration aliases
retain their separate `NAME` environment compatibility path after update or
recopy, when their runner becomes explicit shell. Native operations consume only
their fixed declared inputs; declarations cannot extend or weaken their contracts.

Inspect an effectful action first with `jig run api:generate --explain`. Execute it
with `jig run api:generate --approve-effect worktree`. Repeat `--approve-effect`
for `external` when the plan also requires it. The set of approvals must exactly
match the plan's worktree/external effects, including dependencies; neither missing
nor extra approvals are accepted. Explain does not create run leases or runs and
requires no effect approval. It computes selection and prepared inputs;
execution checks approvals again. An explain result is therefore not an execution
authorization.

Foreground JSON includes `ok`, `command`, `executed`, and the immutable `plan`.
Explain uses `command: "run plan"`; execution uses `command: "run"`.
Executed results also include `run`, `results`, `failed_targets`, and
`source_observations`. The canonical run result records every target's conclusion,
including targets skipped by fail-fast or cancelled before starting. Failure and
cancellation produce unsuccessful command status. Human output summarizes these
results. Unix signals use the existing cooperative CLI supervisor and owned child
cleanup; durable MCP cancellation requests are also observed. The CLI waits
cooperatively for conflicting repository execution, while MCP execution retains
nonblocking acquisition so its transport can continue accepting cancellation.

Command inventory schema version 4 adds `run` and the reason code
`repository_contract_upgrade_required` for pre-v6 repositories.
