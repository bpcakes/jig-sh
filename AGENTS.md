# Repository Guidelines

<!-- BEGIN JIG MANAGED BLOCK -->
This repository uses the shared `jig.sh` workflow. Keep repo-local business rules and ownership guidance in backend-level guides; keep generic agent workflow and repo policy here.

## Start Here

- Use [agent-map.md](./agent-map.md) to find relevant guides when the owning area is unclear.
- Read the nearest backend-level `AGENTS.md` before changing a package or crate when one exists.
- Use `.agent/PLANS.md` when writing an ExecPlan for a complex feature or refactor.
- Use `scripts/jig` for the typed repo contract and `scripts/jig mcp` for MCP clients. Discover targets with `scripts/jig info targets`; preview verification with `scripts/jig check --explain`.
- On a fresh machine, run `scripts/jig doctor`; follow its next step, including `scripts/jig agent bootstrap` when Jig Codex skills are missing.
- Run local services with `scripts/jig dev` to use the repository's proxy and port configuration.
- Routine investigation needs no implementation plan or unrelated tests. For small edits, run the relevant `scripts/jig check` targets.
- Use structured work for substantial implementation, durable handoffs, or when repository policy requires it: `scripts/jig work start`, `scripts/jig work check --plan-id <id>`, then `scripts/jig work finish --plan-id <id> --resolution "..."`. Short work notes can use `--body`; use `.agent/PLANS.md` to decide when an ExecPlan is needed.
- Supply the explicit plan ID for work checks, reviews, and finish, and for plain checks intended as that plan’s evidence. Use `work gates` or `work evidence` and their recovery advice when blocked; use `receipts` or `status` for history or plan discovery.
- Plans capture an exact Git baseline. `work check` reuses current passing target evidence when its freshness policy allows and reruns checks that need new evidence. Required gate policies still govern `work finish`.
- Plain `check` executes selected targets; receipt reuse belongs to `work check`. Where supported, `--affected` narrows candidates, including to a no-op, and never waives required work gates.
- `jig-contract` validates Jig harness wiring, not the application's API contract.
- Treat `.agent/state/*.jsonl` as append-only repo memory.

## Compatibility And Cutovers

- Prefer direct cutovers only for internal code-only changes that can ship in one coordinated deploy.
- Preserve compatibility or stage rollouts for persisted database state, queued job types, public API contracts, bookmarked routes, webhook boundaries, or source-of-truth moves that can straddle deploys.




## Backend Defaults



- Treat `.` as Rust crate roots.
- Add crate-level `AGENTS.md` files when a crate has meaningful ownership, entrypoint, or invariant guidance that should travel with that crate.
- Keep transport logic thin and business logic in the owning crate.




## Frontend Defaults

No web apps are configured in `.jig.toml`.


## Done Means

- Completion requires current passing evidence for the applicable checks below and the configured profiles and required work gates, including authored custom profiles and review gates. Preserve those requirements; a narrower selection does not replace them.
- For backend changes, evidence must cover the configured tests (`scripts/jig check test`).


- A qualifying pass already covering current inputs satisfies its check; do not repeat tests just to make them the last command. Run configured reviews with `scripts/jig work review --plan-id <id>`; `work check` does not supply review evidence.
- Review the generated diff for stale docs, policy drift, or missing dependent updates.

## Backend Guide Conventions

When a backend package or crate has an `AGENTS.md`, these sections are optional suggestions:

- `## Purpose`
- `## Key entrypoints`
- `## Edit here for X`
- `## Invariants`
- `## Common commands`

Use the structure that fits the area. Preserve ownership, entrypoints, invariants, and useful commands; concise guides with different headings are valid. Link to repository files when a reference must be checked. Run `scripts/jig check agent-guides` to validate local links and explicitly declared component guidance.
<!-- END JIG MANAGED BLOCK -->

## Open-Source Fixture Hygiene

- Never put names, paths, identifiers, or operational details from downstream, customer, or private projects in this repository.
- Use unmistakably generic fixtures such as `ExampleProject`, `ExampleVault`, and `vault-consumer-fixture` in source, tests, documentation, plans, and generated evidence.
- Check fixture and test names before running receipt-producing commands because repository paths can be captured in append-only state.
- If an accidentally captured private identifier requires historical state redaction, treat the edit as an explicit privacy migration: preserve record IDs and every unaffected field, then append a durable decision naming the affected record IDs and the reason for redaction without repeating the removed text.

## Dogfooding This Harness

This repo is both the `jig` source tree and an adopted `jig` harness repo. Prefer validating work through `scripts/jig` so changes exercise the same CLI, MCP, contract, and receipt paths that generated repos use.

Before changing `templates/`, read the [bootstrap guide](crates/jig/src/bootstrap/AGENTS.md) for rendering, installer, and scaffold invariants.

When changing the `jig` runtime itself, build a dev binary and force the launcher to use it before running harness commands:

```sh
cargo build -p jig-sh --bin jig
export JIG_DEV_BIN=target/debug/jig
```

For substantial work, open structured work, validate configured gates, and finish once all required evidence is current:

```sh
plan_id="$(scripts/jig work start --title "Describe the work" --body "Validation plan." --print-plan-id)"

scripts/jig work check --plan-id "$plan_id"
scripts/jig work finish --plan-id "$plan_id" --resolution "Describe the verified outcome"
```

If blocked, follow the recovery advice from `scripts/jig work gates --plan-id "$plan_id"` or `scripts/jig work evidence --plan-id "$plan_id"`. Inspect `work receipts` for command history or `work status` to find plans when needed. A repeated `work check` reuses qualifying passes; no additional final test run is required when current evidence already covers the configured tests.

Do not rely on the repo-local cached `jig` binary for runtime changes unless you have intentionally refreshed it. `JIG_DEV_BIN` is the expected local-development cutover.

<!-- bv-agent-instructions-v3 -->

---

## Beads Workflow Integration

Use `bv` for dependency-aware triage and `br` to manage issues in `.beads/`.
See the [Beads workflow reference](docs/beads-workflow.md) for commands and export recovery.

- Start triage with `bv --robot-triage`. Use only `--robot-*` flags; bare `bv` opens a blocking TUI.
- Before claiming, verify current state with `br show <id> --json` or `br ready --json`; recommendations may include blocked or assigned work.
- Claim with `br update <id> --status=in_progress --json`; close after implementation and verification with `br close <id> --reason="..." --json`.
- After Beads mutations, run `python3 scripts/beads-sync.py` to clear machine-local source metadata and export current JSONL. Do not bypass it with a direct flush. Use `python3 scripts/beads-sync.py --check` for read-only validation.
- `br` never commits or pushes. Follow repository Git instructions before staging, committing, or pushing; a commit-only-when-asked rule overrides generic workflow advice.

<!-- end-bv-agent-instructions -->
