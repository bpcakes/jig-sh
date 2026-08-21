# Implement the agent-native repository model

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while implementation proceeds. This document follows `.agent/PLANS.md`.

## Purpose / Big Picture

Jig currently gives a repository one global set of tools such as `jig.test` and one singular backend identity. That is useful for a small, single-stack repository, but it cannot describe a Go API, a Rust worker, and a TypeScript application as independently addressable units. It also forces both humans and agents to infer which files a global command validates and to parse command output to understand what ran.

After this work, Jig models a workspace as components with typed actions. The canonical executable address is a target such as `api:test` or `web:lint`. A deterministic run plan resolves selectors, profiles, dependencies, and affected paths before execution. A durable run records target results and structured evidence. Work gates ask for current evidence for a target or profile rather than naming a command. Agents use four bounded MCP operations—inspect, plan, execute, and cancel—instead of discovering an unbounded tool for each action.

Versions 2 through 5 remain usable. Their manifest tools are projected into a synthetic `repo` component and retain their existing `jig.*` aliases, receipt fields, tool responses, and legacy work-gate behavior. Contract version 6 is the first authored component/action/profile contract. The normative product and UX design is `docs/agent-native-repository-model.md`.

The principal observable flows are:

    jig info component api
    jig info target api:test
    jig check
    jig check test
    jig check api:test
    jig check --profile ci --explain
    jig check --affected origin/main --explain
    jig status run RUN_ID

and, over MCP:

    jig.inspect
    jig.plan_run
    jig.execute_run
    jig.cancel_run

## Progress

- [x] (2026-08-21) Investigated the existing CLI, manifest, feature crates, runtime, receipts, gates, MCP server, templates, and compatibility tests.
- [x] (2026-08-21) Recorded and committed the accepted product model in `docs/agent-native-repository-model.md`.
- [x] (2026-08-21) Opened structured work as `plan_01M0JZ6Z8M48Y1JSFQ6C8MYXC8` and wrote this executable migration plan.
- [x] (2026-08-21 20:24Z) Slice 1: added validated repository identities, component/action/profile records, immutable plan records, separate run status/conclusion, normalized findings/results, generated JSON Schema support, and a legacy manifest serialization regression.
- [x] (2026-08-21 20:29Z) Slice 2: added one validated repository catalog, raw authored/resolved configuration digests, v6 component/action/profile inputs, and deterministic v2–v5 `repo` target projection with bidirectional aliases and a synthesized default profile.
- [x] (2026-08-21 20:44Z) Slice 3: added component/target/profile inspection, one deterministic selector and dependency planner, read-only plan explanation, legacy-profile execution, and a useful bare `jig check` while preserving unmodified named legacy check responses.
- [ ] Slice 4: add durable run state and target-aware receipt fields with old-record compatibility; commit independently.
- [ ] Slice 5: turn Rust, Go, and TypeScript feature metadata into adapter contributions and make v6 templates component-native; commit independently.
- [ ] Slice 6: add target/profile evidence gates while retaining tool gates; commit independently.
- [ ] Slice 7: expose bounded MCP inspect/plan/execute/cancel operations with strict input/output schemas and durable lookup; commit independently.
- [ ] Slice 8: add deterministic, explainable affected selection without artifact caching; commit independently.
- [ ] Run focused acceptance fixtures, full formatting, strict Clippy, workspace tests, generated-template checks, contract checks, and structured-work gates through a fresh development binary.
- [ ] Audit every acceptance item against code and test evidence, update this plan, and finish structured work.

## Surprises & Discoveries

- Observation: `RepoConfig` is intentionally strict, while the resolved manifest type is private to `jig-sh`; v6 cannot be introduced by silently ignoring new authored fields.
  Evidence: `crates/jig/src/context.rs` uses `#[serde(deny_unknown_fields)]` and independently deserializes `.jig.toml` and `.agent/jig-contract.json`.

- Observation: the current check CLI and runtime duplicate a closed enum of stack-shaped commands, and bare `jig check` is a parse error.
  Evidence: `crates/jig/src/cli/check.rs`, `crates/jig/src/command/check.rs`, `crates/jig/src/cli/command_conversion.rs`, and `crates/jig/src/runtime.rs::dispatch_check` each enumerate the current checks.

- Observation: current MCP discovery exposes one execution tool for every manifest tool and supplies only input schemas.
  Evidence: `crates/jig/src/tool_defs.rs` appends every manifest tool to `tools/list`; `crates/jig/src/mcp.rs` serializes those descriptors without an output schema.

- Observation: Jig already owns a cross-platform, bounded-output process-tree runner with cooperative cancellation.
  Evidence: `jig_owned_process::run_owned_process_tree_with_output_limits` polls a cancellation function and safely terminates and reaps the owned process tree on Linux, macOS, and Windows.

- Observation: receipts and work-check aggregate receipts are append-only and keyed by legacy tool names; gates rely on both the tool name and a worktree fingerprint.
  Evidence: `crates/jig/src/state/records.rs`, `state/receipts.rs`, and `runtime/work/gates.rs`.

- Observation: making the existing Clap check subcommand optional preserves every established policy flag while an external-subcommand fallback accepts stack-neutral `component:action` selectors.
  Evidence: focused parser tests cover bare check, named checks, exact/multiple target selectors, profile explanation, and existing policy subcommands; the launcher/Clap allowlist test remains green.

## Decision Log

- Decision: Introduce the v6 DTOs first, without replacing legacy manifest DTOs or changing generated contract version in that slice.
  Rationale: each slice remains buildable and reviewable, and the compatibility projection can be tested before templates start emitting v6.
  Date/Author: 2026-08-21 / Codex.

- Decision: Use one normalized repository catalog inside `jig-sh`. Native v6 records populate it directly; v2–v5 tools populate it through a deterministic compatibility adapter.
  Rationale: selector resolution, planning, execution, inspection, receipts, gates, and MCP must not each grow their own version or language branch.
  Date/Author: 2026-08-21 / Codex.

- Decision: Keep stable legacy `jig.*` names as aliases to targets, not as the primary identity of v6 actions.
  Rationale: old scripts and gates remain valid while new repositories can have multiple `test` targets without collision.
  Date/Author: 2026-08-21 / Codex.

- Decision: Existing named v2–v5 checks without planning flags retain their single-tool execution response; bare check, target selectors, profiles, and `--explain` use the new planner.
  Rationale: scripts keep their stable JSON while users and agents can adopt the new repository vocabulary immediately, before a v6 template cutover.
  Date/Author: 2026-08-21 / Codex.

- Decision: Keep `.jig.toml` human-authored and `.agent/jig-contract.json` resolved. v6 runtime does not rediscover project files.
  Rationale: an agent must see a reviewable contract diff when repository behavior changes, and identical checked-in input must resolve identically.
  Date/Author: 2026-08-21 / Codex.

- Decision: Derive plan ids and configuration/input digests with canonical serialization and SHA-256, and reject execution when the plan's contract or source identity is stale.
  Rationale: re-resolving selectors at execution time could run a different set than the one an agent reviewed.
  Date/Author: 2026-08-21 / Codex.

- Decision: Persist append-only run lifecycle events in `.agent/state/runs.jsonl`; the first event contains the accepted plan and later events contain target and terminal outcomes.
  Rationale: a run remains inspectable after MCP disconnects without adding a mutable database or a second persisted plan source.
  Date/Author: 2026-08-21 / Codex.

- Decision: CLI checks wait for their run and render the collected result. MCP execution starts a worker and immediately returns the durable run handle; cancellation uses the existing owned-process cancellation path.
  Rationale: interactive shell expectations stay conventional while MCP clients can disconnect, poll, or cancel long-running work.
  Date/Author: 2026-08-21 / Codex.

- Decision: Do not implement artifact caching or infer action dependencies from component dependencies.
  Rationale: delegated build systems own caches, and the architecture graph and execution graph express different facts.
  Date/Author: 2026-08-21 / Codex.

## Outcomes & Retrospective

Implementation is in progress. This section will summarize the final contract cutover, compatibility evidence, acceptance fixture results, gate status, and any deliberately deferred work after all eight slices are complete.

## Context and Orientation

`crates/jig-contract` owns transport-neutral contract DTOs shared by core and feature crates. The new repository DTOs belong in a focused module there, re-exported from `lib.rs`; legacy `ManifestTool`, `NativeToolDescriptor`, and global tool constants remain.

`crates/jig/src/context.rs` owns strict configuration and resolved-manifest loading. A new `crates/jig/src/repository/` module will turn those version-specific inputs into one `RepositoryCatalog`. The catalog indexes components, actions, targets, profiles, and legacy aliases and carries a digest of the resolved contract. It is the only input to the selector and planning layer.

CLI parsing lives in `crates/jig/src/cli`, transport-neutral command requests in `crates/jig/src/command`, conversion in `cli/command_conversion.rs`, dispatch in `runtime.rs`, and terminal rendering in `cli/output.rs`. Existing named check subcommands remain accepted. New selectors, `--profile`, `--affected`, `--explain`, and bare invocation resolve through the repository planner.

Persistent JSON Lines schemas live in `crates/jig/src/state/records.rs`, with per-stream behavior under `crates/jig/src/state/`. `runs.jsonl` will be append-only and use event folding for current status. New optional receipt fields must deserialize to `None` or empty values for historical records.

Work configuration is parsed in `context/work_config.rs`, execution lives in `runtime/work/checks.rs`, and gate evaluation lives in `runtime/work/gates.rs`. A v6 evidence gate selects a target or profile plus an evidence predicate. The current `tool` form remains accepted and evaluated exactly as before.

MCP descriptors and request handling live in `tool_defs.rs`, `runtime.rs::call_tool`, and `mcp.rs`. Contract v6 will advertise only the bounded repository execution tools plus the existing bounded work-lifecycle tools; old contracts continue to advertise legacy manifest tools.

The generated source templates live under `templates/project/` and are embedded into `crates/jig/src/bootstrap/embedded_templates_snapshot.rs`. After template edits, refresh the checked-in snapshot with:

    JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh

The acceptance fixture must use generic names such as `ExampleProject`, `api`, and `web` in accordance with open-source fixture hygiene.

## Plan of Work

Slice 1 adds validated `ComponentId` and `ActionId` newtypes, structured `TargetId`, component/action/profile specifications, intent and effects, command/native runners, selection reasons, immutable `RunPlan`, separate run status and conclusion, target results, normalized findings, and aggregate run results to `jig-contract`. Serde names form the public JSON schema. Tests prove identifier validation, target display/parse round trips, effect modeling, stable DTO serialization, and status/conclusion separation. Existing DTO serialization must remain byte-for-byte stable in representative tests.

Slice 2 adds the repository catalog and indexes. A v6-shaped resolved manifest can carry components, actions, profiles, and a default profile while legacy fields remain present for compatibility. For contracts 2–5, every manifest tool becomes a deterministic `repo` target; known tool names map to readable action ids and arbitrary valid tools receive a sanitized collision-safe id. The catalog stores the original tool as an execution alias. Tests prove v5 `jig.test` maps to `repo:test`, aliases resolve in both directions, collisions fail clearly, and a multi-component v6 fixture exposes distinct `api:test` and `web:test` targets.

Slice 3 adds static inspection and the planner. Selector forms are an action across components (`test`), an exact target (`api:test`), and one `*` wildcard per side. Profiles expand to explicit target selectors. Planning validates read-only check intent/effects, expands action dependencies, topologically layers targets, records stable selection reasons, and canonicalizes ordering before deriving the plan id. Bare check selects the configured default profile. `--explain` returns or prints the same plan without executing it. Existing named check commands remain aliases and retain their command-specific flags for old contracts. Tests exercise list/info, bare check, selectors, profiles, unknown and ambiguous input, dependency cycles, stable plan ids, and no-execution explanation.

Slice 4 adds run execution and evidence persistence. A run starts with a durable queued event containing the accepted plan, transitions through running target events, records every target receipt, and ends exactly once with a terminal conclusion. Target command runners use configured command keys and the repository root or declared working directory; native runners use a closed operation id. Plans are rejected when their contract digest or source identity is stale. Receipt records gain optional `run_id`, `target`, `config_digest`, `input_digest`, and `findings` fields. Tests load pre-v6 receipt JSON, exercise successful/failing/cancelled runs, prove terminal folding, and confirm one target-aware receipt per target.

Slice 5 adds adapter descriptors and v6 generation. Rust, Go, and TypeScript feature crates publish component/action defaults rather than only global tool metadata. The bootstrap renderer writes explicit components, actions, profiles, provenance, and compatibility aliases into v6 authored and resolved files. Runtime selection of a v6 backend capability consults component adapters rather than `backend_language`; old contracts retain their current branch. Template and adopter tests cover Rust-only, Go-only, Go plus TypeScript, and multiple frontend components. The embedded snapshot and public contract documentation are updated in this slice.

Slice 6 extends work gates with target and profile evidence requirements. Target gates match the exact structured target. Profile gates require a successful current result for every target in the resolved profile from a compatible run. Freshness checks worktree identity, contract digest, and target input digest. Legacy `{ tool = "jig.test" }` gates remain valid and evaluated exactly as before. Tests prove an unrelated successful target cannot satisfy a gate, stale inputs fail it, a full current profile passes, a partial profile fails, and a v5 tool gate remains stable.

Slice 7 introduces bounded MCP descriptors with input and output schemas. `jig.inspect` reads catalogs and durable runs; `jig.plan_run` invokes the common resolver; `jig.execute_run` validates and starts the exact supplied plan; `jig.cancel_run` marks and cooperatively cancels a live run. Execution conclusions are structured success responses, while invalid arguments, stale plans, corrupt state, and infrastructure errors remain protocol errors. Contract v6 no longer adds one MCP tool per action. Tests verify the advertised tool set, schema validation, immediate run handles, polling to terminal state, cancellation cleanup, and legacy v5 tool responses.

Slice 8 adds affected selection. Changed paths come from an explicit Git base. Direct matches use action input globs and component roots; repository-global inputs are explicit; dependent propagation follows only configured component policy; action dependencies are then expanded separately. Every target carries sorted reasons with the triggering paths or targets. The input digest covers the deterministic relevant path/content identity used for freshness. Tests exercise direct API and web changes, shared-component propagation, global inputs, unrelated paths, stable ordering/reasons, and invalid Git bases. No cache lookup or artifact reuse is introduced.

Finally, add an end-to-end v6 fixture with a Go `api` and TypeScript `web`, both offering `test`. It must list distinct targets, resolve bare/default/action/exact/affected plans, execute and query a durable run, write structured receipts, satisfy only matching fresh gates, expose bounded MCP tools, and coexist with a v5 legacy fixture. Update product, public-contract, CLI, MCP, state, and gate documentation wherever the shipped behavior is observable.

## Concrete Steps

Work from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`. Keep `plan_01M0JZ6Z8M48Y1JSFQ6C8MYXC8` active and update this file after each slice.

1. Implement slice 1 in `jig-contract`, run its focused tests and workspace format check, inspect the diff, and commit with a `contract`-scoped message.

       cargo test -p jig-contract
       cargo fmt --all -- --check

2. Implement and test the legacy/native repository catalog, then commit slice 2.

       cargo test -p jig-sh repository --no-fail-fast
       cargo test -p jig-sh context --no-fail-fast

3. Implement inspection and planning across CLI/command/runtime/output modules, run focused CLI/planner tests and compatibility help snapshots, then commit slice 3.

       cargo test -p jig-sh selector --no-fail-fast
       cargo test -p jig-sh plan_run --no-fail-fast
       cargo test -p jig-sh cli --no-fail-fast

4. Implement run state, execution, and receipt extensions, run focused state/execution/receipt tests, then commit slice 4.

       cargo test -p jig-sh run --no-fail-fast
       cargo test -p jig-sh receipt --no-fail-fast

5. Implement adapter contributions and v6 templates, refresh the embedded snapshot once, run feature and bootstrap/adopt tests, then commit slice 5.

       cargo test -p jig-features
       cargo test -p jig-go
       cargo test -p jig-rust
       cargo test -p jig-typescript
       JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
       cargo test -p jig-sh bootstrap --no-fail-fast

6. Implement target/profile evidence gates, run focused work and gate tests including a legacy fixture, then commit slice 6.

       cargo test -p jig-sh gate --no-fail-fast
       cargo test -p jig-sh work_check --no-fail-fast

7. Implement the bounded MCP surface and background run registry, run protocol and cancellation tests, then commit slice 7.

       cargo test -p jig-sh mcp --no-fail-fast
       cargo test -p jig-sh cancel_run --no-fail-fast

8. Implement explainable affected selection, run focused Git/selection tests, then commit slice 8.

       cargo test -p jig-sh affected --no-fail-fast
       cargo test -p jig-sh repository --no-fail-fast

9. Build the final development binary, force the launcher to use it, run the end-to-end fixture and repository gates, and inspect receipts and structured-work status.

       cargo fmt --all -- --check
       cargo clippy --workspace --all-targets --all-features -- -D warnings
       cargo test --workspace --no-fail-fast
       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract --no-receipt
       JIG_DEV_BIN=target/debug/jig scripts/jig check test --no-receipt
       JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M0JZ6Z8M48Y1JSFQ6C8MYXC8
       JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M0JZ6Z8M48Y1JSFQ6C8MYXC8
       JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M0JZ6Z8M48Y1JSFQ6C8MYXC8
       JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M0JZ6Z8M48Y1JSFQ6C8MYXC8

Each slice is staged by explicit paths, never by `git add -A`, and committed only after its focused checks pass. If a slice exposes an observable surface, its docs and tests belong in the same commit.

## Validation and Acceptance

The primary end-to-end fixture is contract version 6 with components `api` and `web`. Both define `test`; `api` uses a Go command and `web` uses a TypeScript command. A `verify` profile is the default and includes both targets. Assertions must establish:

- component and target inspection returns both components and distinct targets;
- bare check resolves the default profile;
- `test` selects both test targets and `api:test` selects exactly one;
- plan ordering, dependency edges, reasons, id, contract digest, and source identity are deterministic;
- affected selection reports direct paths and configured propagation reasons;
- executing an unchanged plan creates a durable run with separate status and conclusion;
- target receipts carry run/target/config/input/finding fields while historical receipts deserialize;
- target and profile gates require matching current evidence;
- MCP v6 advertises inspect/plan/execute/cancel rather than a tool per target and validates structured outputs; and
- a v5 fixture still executes `jig.test`, emits its stable response fields, and satisfies a legacy tool gate.

Unit tests must additionally cover invalid ids, unknown targets/profiles, wildcard matching, duplicate targets, dependency cycles, unsafe actions under `check`, stale plans, target failures, blocked dependencies, timeout, cancellation, malformed run streams, receipt compatibility, profile partial evidence, Git-base errors, and deterministic affected reasons.

The final repository state must satisfy formatting, strict all-feature/all-target Clippy, all workspace tests, generated-template equality, public contract validation, configured test gates, and the active structured-work gates through `target/debug/jig`. Review `git diff` and every slice commit for stale v5-only documentation, source fixtures containing private names, untracked generated output, or unrelated user changes.

## Idempotence and Recovery

Inspection, planning, and `--explain` are read-only and deterministic. Repeating them against the same contract and source identity returns the same plan id. Executing the same plan creates a new run id and new append-only events; it does not overwrite prior evidence. Cancellation is idempotent: an already-requested cancellation reports the current run, and a terminal run remains terminal.

All state additions are append-only JSONL. Readers fold duplicate or repeated lifecycle observations deterministically and reject incompatible terminal events. Historical receipt and work-gate records continue to deserialize through optional/default fields. If implementation fails partway through a run, the existing queued/running event remains inspectable and is classified as interrupted on the next query rather than deleted.

Template refresh is deterministic and happens only after source templates are valid. A failed generated-contract validation must not publish a partially rendered repository. Git history is the recovery mechanism for each separately committed slice; no destructive reset or broad checkout is used.

## Artifacts and Notes

Normative design:

    docs/agent-native-repository-model.md

Structured work:

    plan id:    plan_01M0JZ6Z8M48Y1JSFQ6C8MYXC8
    session id: session_01M0JZ6Z1819XXSRNJCGPPN605

Compatibility baseline before implementation:

    supported persisted contracts: 2, 3, 4, 5
    generated contract:             5
    global execution identity:      ManifestTool.name
    receipt execution identity:     tool_name
    check default:                  parse error / help
    MCP execution discovery:        one tool per manifest tool

This section will gain the final acceptance command outputs and any intentionally retained legacy limitations as evidence is collected.

## Interfaces and Dependencies

`jig-contract` will export versioned, serde-stable types equivalent to:

    ComponentId
    ActionId
    TargetId { component, action }
    ComponentSpec
    ActionSpec
    ProfileSpec
    RunPlan
    RunStatus
    RunConclusion
    Finding
    TargetRunResult
    RunResult

`jig-sh` will expose crate-internal repository services equivalent to:

    RepositoryCatalog::from_context(&RepoContext) -> Result<Self>
    RepositoryCatalog::resolve_alias(&str) -> Option<TargetId>
    plan_run(&RepoContext, &RepositoryCatalog, PlanRunRequest) -> Result<RunPlan>
    execute_run(&RepoContext, RunPlan, RunExecutionMode) -> Result<RunHandle>
    inspect_repository(&RepoContext, InspectRequest) -> Result<Value>

The exact Rust signatures may move to keep modules cohesive, but CLI and MCP must call the same planner and inspector. No transport layer may independently parse target strings, expand profiles, calculate affected targets, or decide freshness.

Configured command runners initially reuse the existing named command table in `.jig.toml`; their v6 action records reference a command key and an optional relative working directory, environment additions, timeout, and result parser. This preserves current shell-command compatibility while preventing MCP clients from supplying shell text. Native runner ids remain a closed enum or validated registry owned by Jig.

SHA-256 should reuse the workspace's existing digest dependency if available. Glob matching should reuse an existing dependency if one is already present; otherwise add one at workspace scope with anchored, repository-relative semantics and explicit tests. Process execution and cancellation must use `jig-owned-process`, not a second child-process implementation.
