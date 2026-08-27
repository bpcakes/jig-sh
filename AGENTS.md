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
- A plan captures an exact Git baseline. Default `work check` runs required gates whose configured path policy applies and records explicit not-applicable evidence for the rest; use `--gate <id>` only when deliberately force-running one gate.
- `jig-contract` validates Jig harness wiring, not the application's API contract.
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

## Open-Source Fixture Hygiene

- Never put names, paths, identifiers, or operational details from downstream, customer, or private projects in this repository.
- Use unmistakably generic fixtures such as `ExampleProject`, `ExampleVault`, and `vault-consumer-fixture` in source, tests, documentation, plans, and generated evidence.
- Check fixture and test names before running receipt-producing commands because repository paths can be captured in append-only state.
- If an accidentally captured private identifier requires historical state redaction, treat the edit as an explicit privacy migration: preserve record IDs and every unaffected field, then append a durable decision naming the affected record IDs and the reason for redaction without repeating the removed text.

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

<!-- bv-agent-instructions-v3 -->

---

## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`) for issue tracking and [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) (`bv`) for graph-aware triage. Issues are stored in `.beads/` and tracked in git. Current `br` workspaces normally export `.beads/issues.jsonl`; older `bd`/legacy workspaces may use `.beads/beads.jsonl`. `bv` auto-discovers the supported JSONL files, so agents should use `br`/`bv` commands instead of hard-coding a single filename.

### Using bv as an AI sidecar

bv is a graph-aware triage engine for Beads projects. Instead of parsing .beads/issues.jsonl / .beads/beads.jsonl directly or hallucinating graph traversal, use robot flags for deterministic, dependency-aware outputs with precomputed metrics (PageRank, betweenness, critical path, cycles, HITS, eigenvector, k-core).

**Scope boundary:** bv handles *what to work on* (triage, priority, planning). `br` handles creating, modifying, and closing beads.

**CRITICAL: Use ONLY --robot-* flags. Bare bv launches an interactive TUI that blocks your session.**

#### The Workflow: Start With Triage

**`bv --robot-triage` is your single entry point.** It returns everything you need in one call:
- `quick_ref`: at-a-glance counts + top 3 picks
- `recommendations`: ranked actionable items with scores, reasons, unblock info
- `quick_wins`: low-effort high-impact items
- `blockers_to_clear`: items that unblock the most downstream work
- `project_health`: status/type/priority distributions, graph metrics
- `commands`: copy-paste shell commands for next steps

```bash
bv --robot-triage        # THE MEGA-COMMAND: start here
bv --robot-next          # Minimal: just the single top pick + claim command

# Token-optimized output (TOON) for lower LLM context usage:
bv --robot-triage --format toon
```

Before claiming, verify current state with `br show <id> --json` or `br ready --json`. `recommendations` can include graph-important blocked or assigned work; only `quick_ref.top_picks` and non-empty `claim_command` fields represent claimable work.

#### Other bv Commands

| Command | Returns |
|---------|---------|
| `--robot-plan` | Parallel execution tracks with unblocks lists |
| `--robot-priority` | Priority misalignment detection with confidence |
| `--robot-insights` | Full metrics: PageRank, betweenness, HITS, eigenvector, critical path, cycles, k-core |
| `--robot-alerts` | Stale issues, blocking cascades, priority mismatches |
| `--robot-suggest` | Hygiene: duplicates, missing deps, label suggestions, cycle breaks |
| `--robot-diff --diff-since <ref>` | Changes since ref: new/closed/modified issues |
| `--robot-graph [--graph-format=json\|dot\|mermaid]` | Dependency graph export |

#### Scoping & Filtering

```bash
bv --robot-plan --label backend              # Scope to label's subgraph
bv --robot-insights --as-of HEAD~30          # Historical point-in-time
bv --recipe actionable --robot-plan          # Pre-filter: ready to work (no blockers)
bv --recipe high-impact --robot-triage       # Pre-filter: top PageRank scores
```

### br Commands for Issue Management

```bash
br ready --json                       # Show issues ready to work (no blockers)
br list --status=open --json          # All open issues
br show <id> --json                   # Full issue details with dependencies
br create --title="..." --type=task --priority=2 --json
br update <id> --status=in_progress --json
br close <id> --reason="Completed" --json
br close <id1> <id2> --reason="Completed" --json
br sync --flush-only                  # Export DB to JSONL after Beads mutations
```

### Workflow Pattern

1. **Triage**: Run `bv --robot-triage` to find the highest-impact actionable work
2. **Claim**: Use `br update <id> --status=in_progress --json`
3. **Work**: Implement the task
4. **Complete**: Use `br close <id> --reason="Completed" --json`
5. **Sync**: Run `br sync --flush-only` after Beads mutations so the JSONL export is current

### Key Concepts

- **Dependencies**: Issues can block other issues. `br ready --json` shows only unblocked work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers 0-4, not words)
- **Types**: task, bug, feature, epic, chore, docs, question
- **Blocking**: `br dep add <issue> <depends-on>` to add dependencies

### Git Policy

`br` never commits or pushes. Follow this repository's own git instructions before staging, committing, or pushing. If the repository says "commit only when asked," that rule overrides any generic workflow advice.

<!-- end-bv-agent-instructions -->
