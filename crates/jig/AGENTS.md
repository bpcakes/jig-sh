# jig crate guide

## Purpose

`crates/jig` contains the repo-local `jig` CLI and MCP runtime used by generated repositories. It executes the generated command contract, manages append-only `.agent/state` memory, and handles template init/adopt/update flows.

## Key entrypoints

- `src/main.rs`: binary entrypoint.
- `src/lib.rs`: library entrypoint and module wiring.
- `src/cli.rs`: clap command definitions and top-level command dispatch.
- `src/runtime.rs`: command-backed tool execution plus MCP tool call dispatch.
- `src/mcp.rs`: JSON-RPC/MCP stdio server.
- `src/state.rs`: sessions, plans, receipts, and decisions stored under `.agent/state`.
- `src/ui.rs`: `jig ui` CLI adapter for the separately owned `jig-ui` server and presentation crate.
- `src/ui/snapshot.rs`: joins Jig-owned state, work-gate, and loop data for the UI provider boundary.
- `src/status.rs`: configured status-provider execution, validation, freshness, and aggregate snapshots.
- `src/status/tui.rs`: adapter from cancellable aggregate snapshots to the separately owned `jig-status-tui` crate.
- `src/runtime/vault/tui.rs`: fixed-scope, process-local credential adapter for the separately owned `jig-vault-tui` crate.
- `src/bootstrap.rs`: init/adopt/update command surface.
- `src/bootstrap/`: bootstrap support for native template rendering, git, staged renders, and template-source handling.

## Edit here for X

- Change CLI flags or subcommands: `src/cli.rs`.
- Change make-tool behavior or receipt recording around command execution: `src/runtime.rs`.
- Change MCP descriptors, schemas, or protocol handling: `src/mcp.rs`.
- Change session, plan, receipt, or decision persistence: `src/state.rs`.
- Change the data exposed by `jig ui`: `src/ui/snapshot.rs`.
- Change `jig ui` routes, query parsing, server behavior, or rendering: `crates/jig-ui/`.
- Change status provider execution or aggregate facts: `src/status.rs` and `src/status/`.
- Change terminal status navigation, refresh runtime, or rendering: `crates/jig-status-tui/`.
- Change Vault TUI navigation, forms, or rendering: `crates/jig-vault-tui/`; keep scope, environment capture, external tools, and core calls in `src/runtime/vault/tui.rs`.
- Change bounded owned-process execution or process-tree cleanup: `src/process.rs` and
  `src/process/tests.rs`.
- Change init/adopt/update behavior: `src/bootstrap.rs` and `src/bootstrap/`.
- Change git metadata captured in receipts: `src/git_receipts.rs`.

## Invariants

- Keep transport layers thin; shared behavior should live in runtime, state, or bootstrap helpers.
- Preserve generated-repo compatibility for `.jig.toml`, `.agent/jig-contract.json`, and `.agent/state/*.jsonl`.
- Treat `.agent/state/*.jsonl` as append-only unless a migration path is explicit.
- Keep execution tools aligned with the generated contract manifest and template outputs.
- Do not make template update flows switch source identity implicitly.
- Vault references stay project-relative as `jig://ITEM/FIELD`; repository scope, `--global`, or `--home` selects the vault and a reference must never override that selection.
- Validate vault raw input, import sources/destinations, and lifecycle paths before passphrase capture. Revealed values and transparent child output must bypass structured emitters, JSON, MCP, and receipts; errors and recovery commands must remain value-free.
- Keep `vault exec` as transparent inherited-stdin/environment streaming with exact child status, and keep the compatible `vault run` broker constrained, buffered, capped, timed, and process-tree-owned. Successful vault capture and every spawned resolver/child must strip both reserved passphrase variables.
- Backup restore must use the static absent-target path; it may prepare missing private parent directories, but must never resolve or create the selected vault home before restore preflight and installation.
- The Vault TUI fixes one resolved scope for its lifetime, retains only a process-local credential in the CLI-owned backend, and must join its sole action worker before lock or terminal restoration. TUI action results and ordinary Ratatui frames remain metadata-only; private export and transient Peek consume plaintext only in their immediate hardened/terminal-safe sinks and never return it to the model.
- The generic owned-process runner must establish a verifiable process tree before starting work, retain the direct-child identity until descendant cleanup is confirmed, share one absolute cleanup deadline across normal/error/drop paths, and fail closed on incomplete output or unsupported supervision. While that unreaped exact child pins the PGID generation, forced confirmation must re-send group `SIGKILL` before every membership proof so a concurrently exposed member cannot outlive a one-shot signal; never retry after identity loss or reap. Linux procfs confirmation must check the deadline around every signal, enumeration, stat read, fallback membership probe, and before accepting either a live or empty result, with a re-signal between its two required empty scans. On macOS, neither a cached leader exit, `ESRCH`, nor `EPERM` proves group absence; require a fresh exact terminal observation plus an atomic sole-leader membership snapshot. Unix doctor signal sessions are serialized and reusable only after clean retirement: hold the session guard through handler restoration and restored-signal redelivery, and publish permanent poison before snapshotting signals on an unsafe retirement.
- Jig-owned Bash probes such as dependency readiness, Codex capability checks, and launcher-backed doctor diagnostics must remove startup, directory, option, trace, and byte-exact exported-function controls. Do not apply that constrained environment to agent bootstrap, committed checks, or configured development commands, which intentionally inherit the caller's ordinary environment.
- Existing-destination init must budget retained generations before acquiring snapshots. Charge a possible preimage plus one generated version per planned leaf, count repeated publications explicitly, and include directory/staging identities plus transient headroom; apply the generation cap to unique leaves plus repeats without trusting a currently missing path to remain absent.
- Generated dependency installers must distinguish repository-owned package-manager policy from hostile ambient install shaping. A successful install may be stamped only after the selected scope, lock/config authority, real-write mode, complete workspace participation, platform, dependency classes, and executable-link behavior are pinned; preserve explicit registry/authentication and install-script approval policy. Keep the checker compatible with stock Bash 3.2, propagate authority-producer failures, and use shell-owned job identity after `wait` rather than recyclable PIDs.
- Generated npm package-script execution must select exactly the configured app, require the named script, and neutralize only ambient npm routing/dependency-class selectors. Preserve explicit application environment, registry/authentication, dependency layout, peer/lifecycle policy, and every user-authored development command. All generated web and E2E workflow package scripts must enter through the public checker boundary.
- Generated Rust/React source must be rustfmt-stable and pass its generated strict Clippy gate for every supported normalized package stem, database branch, and valid migration path. Validate the 216-byte Cargo artifact boundary before destination mutation, keep rendered identifiers behind fixed aliases, narrowly scope any lint acknowledgement required by intentional formatter-stability constructs, and keep long fallback API labels DNS-safe without changing short-name output.
- Classify each `node_modules` install root independently: missing, empty, and exact ignored-only real roots share the absent proof, while any unknown/type-replaced/nested entry makes the root present and fully attested. Preserve package metadata, links, member receipt-like files, launcher bytes/modes, and the v5/v3/v2 receipt formats.
- Rust/React scaffolds require Rust 1.94. Database-enabled variants pin SQLx 0.9 and use `.sqlx`; Doctor must enforce the active Rust floor and matching SQLx CLI minor line. PostgreSQL browser E2E owns its Linux service-container runner independently of the repository-wide runner; managed Rust workflow triggers and offline environments must follow configured migration and metadata authorities.
- When editing the runtime, build `target/debug/jig` and dogfood through `JIG_DEV_BIN=target/debug/jig scripts/jig ...` so the cached repo-local binary cannot mask current code.

## Common commands

- `cargo test -p jig-sh`
- `cargo test -p jig-sh --test codex_launcher -- --nocapture` (requires a Unix PTY; set
  `JIG_ALLOW_PTY_TEST_SKIP=1` only when the environment is intentionally exempt)
- `cargo test --workspace`
- `cargo build -p jig-sh --bin jig`
- `JIG_DEV_BIN=target/debug/jig scripts/jig work status`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check contract`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides`
- `JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map`
