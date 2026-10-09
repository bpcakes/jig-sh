# Repository Guidelines

<!-- BEGIN JIG MANAGED BLOCK -->
This repository uses the shared `jig.sh` workflow. Keep repo-local business rules and ownership guidance in backend-level guides; keep generic agent workflow and repo policy here.

## Start Here

- Use this file for repo-wide defaults.
- Use [agent-map.md](./agent-map.md) when you need help locating ownership guidance for backend work.
- Read the nearest backend-level `AGENTS.md` before changing a package or crate when one exists.
- Use `scripts/jig` for the typed repo contract; pass `--json` for agent automation.
- On a fresh machine, run `scripts/jig doctor`; follow its next step, including `scripts/jig agent bootstrap` when Jig Codex skills are missing, except operator-owned vault setup (see Vault).
- Discover available targets with `scripts/jig info targets`; run a focused target with `scripts/jig check COMPONENT:ACTION`. Use `--affected BASE` when selecting checks by changed paths is useful.

- Use `scripts/jig file-budget audit` for standalone source-size diagnostics; it creates no runs.

- `jig-contract` validates Jig harness wiring, not the application's API contract.
- Treat `.agent/state/*.jsonl` as append-only repo memory.
- Keep `.agent/state/runs.jsonl` as local execution history; do not stage or commit it.

## Compatibility And Cutovers

- Prefer direct cutovers only for internal code-only changes that can ship in one coordinated deploy.
- Preserve compatibility or stage rollouts for persisted database state, queued job types, public API contracts, bookmarked routes, webhook boundaries, or source-of-truth moves that can straddle deploys.




## Vault

- The vault behind `scripts/jig vault` and its passphrase are operator-owned. These rules override any Jig next step, such as from `scripts/jig doctor` or `scripts/jig info --commands`, that suggests vault setup.
- Run only `scripts/jig vault status` and, when the operator has already provided `JIG_VAULT_PASSPHRASE` to the session, `scripts/jig vault exec --env-file REFS_FILE -- COMMAND` with the refs file the operator provided and the task's command. `scripts/jig vault run` with the operator's references is the constrained alternative for short non-interactive commands. Every other vault subcommand is operator-only.
- `COMMAND` is the underlying task command itself. Never wrap `scripts/jig check`, `scripts/jig run`, or another Jig runner in `vault exec` or `vault run`: a failing target records its stdout and stderr in `.agent/state/runs.jsonl` outside vault redaction, so incidental failure output can persist an injected value.
- Never pass `--home` or `--global` to vault commands, and never set `JIG_VAULT_HOME`.
- Never delete, move, or edit the vault rollback witness in `~/.jig/vault-witness` or its journals, even to get past a rollback, fork, or pending-transaction error; report the error to the operator.
- Never request, print, inspect, test, choose, store, or set the passphrase, including with `echo` or `printenv`. Run the command, and stop if it reports a missing passphrase or prompts for one.
- Never create or edit refs files or add references. Copying the operator's refs file unchanged from the main checkout into a worktree is fine.
- Never wrap commands that print, encode, or transmit injected values, and never write revealed values to files such as `.env.local`.
- If a needed credential is unavailable, stop and ask the operator.

## Backend Defaults



- Treat `.` as Rust crate roots.
- Add crate-level `AGENTS.md` files when a crate has meaningful ownership, entrypoint, or invariant guidance that should travel with that crate.

- Keep transport logic thin and business logic in the owning crate.





## Frontend Defaults

No web apps are configured in `.jig.toml`.


## Preferred Commands

- `scripts/jig bootstrap`
- `scripts/jig doctor`

- `scripts/jig dev`

- `scripts/jig check test`
- `scripts/jig check fmt`


- `scripts/jig check clippy`

- `scripts/jig info targets`

- `scripts/jig file-budget audit`




- `scripts/jig check contract`

## Done Means

- Validate the affected behavior with focused checks. Run broader suites when shared behavior, failures, or unresolved risks warrant them.
- Once the affected behavior is verified, finish; repeat checks only for changed inputs or a concrete remaining concern.


- Review the generated diff for stale docs, policy drift, or missing dependent updates.

## Backend Guide Conventions

When a backend package or crate has an `AGENTS.md`, these sections are optional suggestions:

- `## Purpose`
- `## Key entrypoints`
- `## Edit here for X`
- `## Invariants`
- `## Common commands`

Use the structure that fits the area. Preserve ownership, entrypoints, invariants, and useful commands. Link to repository files when a reference must be checked. Run `scripts/jig check agent-guides` to validate local links and explicitly declared component guidance.

<!-- END JIG MANAGED BLOCK -->

## Open-Source Fixture Hygiene

- Never put names, paths, identifiers, or operational details from downstream, customer, or private projects in this repository.
- Use unmistakably generic fixtures such as `ExampleProject`, `ExampleVault`, and `vault-consumer-fixture` in source, tests, documentation, plans, and generated evidence.
- Check fixture and test names before running state-writing commands because repository paths can be captured in append-only state.
- If an accidentally captured private identifier requires historical state redaction, treat the edit as an explicit privacy migration: preserve record IDs and every unaffected field, then record the affected record IDs and the reason for redaction in the commit message without repeating the removed text.

## Splitting Oversized Rust Files

- When a Rust file outgrows its file budget, split it into modules named for what they hold, with explicit imports and only the visibility callers need.
- Never splice slices back with `include!` or name files by position (`part_NN.rs`, `tail.rs`, `*_parts/`): they share one namespace and escape `cargo fmt`. `scripts/check-rust-module-splits.py`, run by `scripts/check-rust-format.sh`, rejects both.

## Dogfooding This Harness

This repo is both the `jig` source tree and an adopted `jig` harness repo. Prefer validating work through `scripts/jig` so changes exercise the same CLI, contract, and run-history paths that generated repos use.

Follow [local validation](docs/local-validation.md) to choose between focused checks, the preflight profile, and the full `verify` profile. For ordinary Rust changes, test with `scripts/jig check repo:source-affected-test`, which runs only the tests the change can affect; keep the full `verify` profile for broad changes before handoff.

In this source checkout, `scripts/jig` uses the released runtime selected by `.jig/source-runtime-version`. Routine checks remain available while the source is changing or does not compile. Rust tests still compile and exercise the edited source.

Use the development entrypoint when validating behavior of the current `jig` implementation:

```sh
scripts/jig-dev check contract
scripts/jig-dev --json info
```

`scripts/jig-dev` incrementally builds the workspace binary and passes the resulting executable through the normal launcher. It respects Cargo's configured target directory and fails if the build fails. No environment override is needed for routine development.

For runtime, launcher, template, or build configuration changes, use `scripts/jig check repo:source-runtime-check` when validating the current implementation through the launcher. The same target is available in the `verify` profile. `JIG_DEV_BIN` remains an explicit override for an already-built binary; its freshness is the caller's responsibility.

The managed Vault rules protect the operator's vault. Vault tests in this source tree create their own throwaway vault homes, rollback witness roots, and test-only passphrases. When a manual check of edited vault behavior needs a vault, use a throwaway `--home` directory in a scratch location outside the repository with a test-support build, `cargo run --locked -p jig-sh --features jig-vault/test-utils -- vault ...`, which keeps the rollback witness beside that home. `scripts/jig-dev` and release builds record every format 3 vault they open, including a throwaway one, in the per-user witness `~/.jig/vault-witness`. Never use the operator's vault, witness, or passphrase.

<!-- bv-agent-instructions-v3 -->

---

## Beads Workflow Integration

Use the [Beads workflow reference](docs/beads-workflow.md) when selecting tracked work or updating an existing task. A direct request does not require creating an issue.

- Use `bv --robot-triage` for selection; never run bare `bv` (it starts an interactive TUI).
- Verify candidates with `br show <id> --json` or `br ready --json` before claiming.
- Use `br` for issue mutations, then run `python3 scripts/beads-sync.py` to export without machine-local paths. Do not bypass the helper with a direct flush.
- Follow repository git policy; Beads commands never commit or push.

<!-- end-bv-agent-instructions -->
