# Adoption Guide

## Recommended Rollout

1. Start with an existing repository that already has a stable Cargo workspace and CI.
2. Preview the harness render with `jig adopt . --rust-migration-dir migrations`. For tooling-only repos, pass `--sqlx-enabled false` instead of the migration flag. To enable `jig loop` without scripts, workflows, or agent context files, preview with `jig adopt . --minimal` and apply with `jig adopt . --minimal --write`; that stores `harness_footprint = "minimal"` so later `jig update` keeps the same thin footprint until you re-adopt without `--minimal`. When re-adoption changes the footprint and no `--answers-file` is supplied, Jig seeds known answers from the existing `.jig.toml` before applying explicit CLI overrides. Minimal contract checks still validate the contract epoch, commands, tools, and work gates; only the intentionally omitted `.mcp.json`, `scripts/jig`, and `scripts/install-jig.sh` presence checks are skipped. Release builds of `jig adopt` default internally to the official `jig-sh` template tag for that release. Unreleased or dirty local builds use templates embedded in the binary when `--template` is omitted; pass `--template /path/to/jig-sh --template-mode committed` to render from a checkout or `--vcs-ref` for remote template code.
3. For local dogfooding, commit or stash template checkout changes before rendering. If you need to test in-progress template edits, make a temporary local commit and update from that committed source.
   When testing generated launchers with `JIG_DEV_BIN`, rebuild the dev binary after changing Jig and unset the variable if its repository/profile compatibility probe no longer passes; generated launchers treat an explicit incompatible binary as a hard error instead of falling back to the cache.
4. Confirm the preview has the intended profile and template source, then run `jig adopt . --write` with the same answer overrides. Interactive writes ask for confirmation unless `--defaults` or `--no-input` is supplied. Add `--json` to `jig adopt` when you need the full detection report for automation or debugging. Release defaults point at the official portable URL, while unreleased local defaults record `embedded:jig-sh`; generated launchers reuse a contract/profile-compatible binary from managed caches. Reusing an otherwise compatible `jig` found on `PATH` requires the explicit `JIG_INSTALL_ALLOW_PATH_BINARY=1` opt-in, and the installer reports the selected absolute path on stderr. Remote runtime installs normally require the recorded immutable `_commit`; `JIG_INSTALL_ALLOW_UNPINNED_REMOTE=1` is a warned recovery override for older or damaged source metadata. When embedded provenance has no immutable commit and no compatible binary is available, `JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1` explicitly permits a warned default-branch install from `template_source_url` or the official source. Override `template_source_url` only when adopting from a local checkout, fork, or private template. Jig leaves any root `Makefile` project-owned and routes generated checks through `scripts/jig`. Review the remaining paths, commands, and `[dev]` proxy defaults such as `tld`, `lan`, and `workspace_discovery` before committing. Command-backed `*_command` values run through non-login `bash -c`, so put any required toolchain setup in the command string or in project-owned scripts. Jig rejects unknown `.jig.toml` keys; after upgrading an existing repo, remove or rename any unknown keys reported by `scripts/jig` before rerunning commands. Write mode records `.agent/.cache/adopt/adopt-last.json` with the applied report and backups for overwritten managed files, and also writes a deprecated compatibility copy to `.agent/state/adopt-last.json` for older automation during the cutover. The receipt includes `canonical_receipt_path` and `legacy_receipt_deprecated` fields so legacy readers can migrate.
5. Review the root `AGENTS.md`. Existing repo guidance is preserved; Jig inserts or updates only the `<!-- BEGIN JIG MANAGED BLOCK -->` section.
6. Add or adapt crate-level `AGENTS.md` files only where crate-specific ownership, entrypoint, or invariant guidance would be useful.
7. Run `scripts/jig doctor`. If Jig Codex skills are missing and you want this client to use them, run `scripts/jig agent bootstrap`.
8. Run the generated local checks and `scripts/jig check contract`. If web app dependencies, nested Rust projects, or other project setup must happen during bootstrap, set `bootstrap_command` explicitly; the generated default runs `cargo fetch` only when a root `Cargo.toml` exists.
9. Wire any missing project-owned scripts such as `scripts/dump-schema.sh` if schema dumps are enabled.
10. Commit the generated files and then switch CI to use the new workflows.

Before publishing a generated repo contract or wiring long-lived MCP clients to it, review [Public Contract](./public-contract.md) for the stable CLI, MCP, and manifest guarantees.

For later template updates:

```sh
jig update
```

For remote template sources, plain `jig update` advances to the remote default branch unless you pass `--vcs-ref`. Use `jig update --recopy` when you want to re-render from the stored `_commit` instead.

Jig records the exact active template-owned files in `.agent/jig-managed-paths.json`. Only paths listed by the previous valid manifest can be retired; a missing manifest grants no deletion authority. Repositories adopted before this manifest existed must run an explicit `jig adopt . --write` with their current footprint before a full `jig update`. The launcher-only migration below is the sole exception: it can recognize and replace the two legacy generated runtime scripts without establishing ownership or retiring anything. To move an older full harness to minimal, establish the full manifest first, then run `jig adopt . --minimal --write --force`. Invalid, unsafe, or symlinked manifests block adopt and update even with `--force`.

If an older wrapper refuses to run because its generated product version differs from the installed Jig release, bypass that wrapper and repair only its runtime scripts:

```sh
jig update /path/to/repo --launcher-only --force
```

Launcher-only repair treats the rendered installer as a recovery boundary. It
runs Bash, Python 3, and the standard POSIX helper commands from root-owned,
non-writable PATH entries (root-owned sticky ancestors such as the Nix store
are allowed). Install those tools in a system-managed location before using
the repair command; user-owned Homebrew prefixes and ordinary user profile
directories are intentionally excluded. Failures report the restricted PATH
that was searched.

This changes only `scripts/jig` and `scripts/install-jig.sh`; it deliberately does not create `.agent/jig-managed-paths.json`. If that manifest already exists, review and run a normal `jig update /path/to/repo --force`. If it is absent, first establish ownership and migrate the full harness with `jig adopt /path/to/repo --write --force` using the repo's current footprint and answer overrides. A normal update is safe only after that adoption creates the managed-path manifest.

A repaired v2/v3 launcher depends on the contract-compatible runtime seeded by the current Jig binary until the repository completes its full migration. A fresh clone, cache cleanup, or cacheless CI job cannot rebuild that runtime from an older pre-probe release tag; install a current Jig binary and rerun `jig update /path/to/repo --launcher-only --force`, then migrate the full harness promptly. `scripts/jig doctor` treats the unmigrated launcher as a required migration and exits nonzero while reporting these recovery steps.

Minimal adoption keeps inferred or configured `[[frontend_apps]]` and matching `[[dev.apps]]` metadata, but defers the generated TypeScript commands, contract tools, work gates, workflow, scripts, and package-script/lockfile validation. Re-adopt without `--minimal` when those frontend harness capabilities should become active.

If the repo was adopted from a local committed checkout, update that checkout to the desired commit and run:

```sh
jig update --template /path/to/jig-sh --template-mode committed
```

After editing `.jig.toml`, re-render the repo with:

```sh
jig update --recopy
```

`jig update` refuses to overwrite or remove changed template-managed files. Re-run with `--force` when the rendered output should replace those paths.

When updating SQLx repos that have `schema_dump_enabled = false`, remove stale `jig.schema_check` entries from `work.gates`; current templates render schema-check commands, tools, and gates only when schema dumps are enabled.

When moving a command-backed repo from contract v2 to v3, grep CI, scripts, docs, and agent instructions for old root check commands such as `scripts/jig fmt-check`, `scripts/jig contract-check`, and `scripts/jig agent-map check`; update them to `scripts/jig check ...` before relying on the new contract.

## What To Keep Project-Owned

- application code
- crate ownership boundaries
- crate-level agent guides
- root `AGENTS.md` content outside the Jig managed block
- schema dump implementation details
- app-specific dev orchestration
- any environment-specific onboarding or demo bootstrap flows

## First Validation Pass

After rendering, validate at minimum:

```sh
scripts/jig bootstrap
scripts/jig check contract
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
```

If `sqlx_enabled` is `true`, also validate:

```sh
scripts/jig check sqlx
```

If SQLx and schema dumps are enabled:

```sh
scripts/jig check schema
```

If web apps are configured, confirm each app has the expected package scripts before enabling the web workflow.

If you want an MCP client to discover the repo automatically, point it at the generated `.mcp.json`, which launches `scripts/jig mcp`.

On a fresh machine, start with `scripts/jig doctor`. It reports harness readiness across runtime, config, contract, required tools, agent skills, proxy status, and vault status, then prints the next setup command. `scripts/jig agent doctor` remains the focused read-only agent tooling check and exits nonzero until required setup is complete. Human-readable output is the default. Pass `--json` for stable structured automation output. The agent check requires Codex marketplace support and registered marketplace sources; plugin enablement is reported as diagnostic detail. `scripts/jig agent bootstrap` is explicit because it runs `codex plugin marketplace add` and mutates user-level Codex config. For local dogfooding with an existing sibling skills checkout, use:

```sh
scripts/jig agent bootstrap --marketplace ../jig-skills
```
