# Repository Guidelines

<!-- BEGIN JIG MANAGED BLOCK -->
This repository uses the shared `jig.sh` workflow. Keep repo-local business rules and ownership guidance in crate-level guides; keep generic agent workflow and repo policy here.

## Start Here

- Use this file for repo-wide defaults.
- Open [agent-map.md](./agent-map.md) before backend work.
- Read the nearest crate-level `AGENTS.md` before changing a crate when one exists.
- Use `.agent/PLANS.md` when writing an ExecPlan for a complex feature or refactor.
- Use `scripts/jig` for the typed repo contract and `scripts/jig mcp` for MCP clients.
- On a fresh machine, run `scripts/jig doctor`; follow its next step, including `scripts/jig agent bootstrap` when Jig Codex skills are missing.
- For substantial work, use `scripts/jig work start`, `scripts/jig work check`, `scripts/jig work evidence`, `scripts/jig work gates`, and `scripts/jig work finish` to keep plans, receipts, and required gates connected.
- Treat `.agent/state/*.jsonl` as append-only repo memory.

## Compatibility And Cutovers

- Prefer direct cutovers only for internal code-only changes that can ship in one coordinated deploy.
- Preserve compatibility or stage rollouts for persisted database state, queued job types, public API contracts, bookmarked routes, webhook boundaries, or source-of-truth moves that can straddle deploys.

## Backend Defaults

- Treat `crates` as Rust crate roots.
- Keep transport logic thin and business logic in the owning crate.
- Add crate-level `AGENTS.md` files when a crate has meaningful ownership, entrypoint, or invariant guidance that should travel with that crate.

## Frontend Defaults

No web apps are configured in `.jig.toml`.

## Preferred Commands

- `scripts/jig bootstrap`
- `scripts/jig doctor`
- `scripts/jig dev`
- `scripts/jig work status`
- `scripts/jig work evidence`
- `scripts/jig check test`
- `scripts/jig check fmt`
- `scripts/jig check clippy`
- `scripts/jig check contract`

## Done Means

- Run the relevant local verification for the area you changed.
- For backend changes, finish with `scripts/jig check test`.
- Review the generated diff for stale docs, policy drift, or missing dependent updates.

## Crate Guide Conventions

When a backend crate has a crate-level `AGENTS.md`, use these sections:

- `## Purpose`
- `## Key entrypoints`
- `## Edit here for X`
- `## Invariants`
- `## Common commands`
<!-- END JIG MANAGED BLOCK -->

## Dogfooding This Harness

This repo is both the `jig` source tree and an adopted `jig` harness repo. Prefer validating work through `scripts/jig` so changes exercise the same CLI, MCP, contract, and receipt paths that generated repos use.

When changing the `jig` runtime itself, build a dev binary and force the launcher to use it before running harness commands:

```sh
cargo build -p jig-sh --bin jig
export JIG_DEV_BIN=target/debug/jig
```

For substantial work, open structured work, run configured gates, then inspect gate status and receipts:

```sh
plan_id="$(scripts/jig work start --title "Describe the work" --body "Validation plan." --print-plan-id)"

scripts/jig work check --plan-id "$plan_id"
scripts/jig work gates --plan-id "$plan_id"
scripts/jig work evidence --plan-id "$plan_id"
scripts/jig work receipts --plan-id "$plan_id"
scripts/jig work status
```

Do not rely on the repo-local cached `jig` binary for runtime changes unless you have intentionally refreshed it. `JIG_DEV_BIN` is the expected local-development cutover.
