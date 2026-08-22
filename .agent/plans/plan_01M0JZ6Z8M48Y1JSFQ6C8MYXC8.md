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
- [x] (2026-08-21 21:21Z) Slice 4: added stale-plan validation, owned target execution, append-only run lifecycle folding, target-aware compatible receipts, terminal cancellation/fail-fast results, and `jig status run RUN_ID` lookup.
- [x] (2026-08-21 22:19Z) Slice 5: added feature-owned adapter contributions, explicit v6 components/actions/profiles and provenance, component-scoped commands, adapter-derived runtime capability checks, per-frontend execution, authored multi-stack recopy preservation, and v2–v5 template compatibility.
- [x] (2026-08-21 23:26Z) Slice 6: added exact target/profile evidence gates, same-run profile proof, contract/input/worktree freshness, one-run configured work checks, strict selector validation, compatible legacy tool gates, and focused evidence/index modules.
- [x] (2026-08-22 00:03Z) Slice 7: replaced v6 per-action MCP discovery with four strictly schema-bound repository operations, immediate durable run handles, typed catalog/run inspection, background execution, cross-process durable cancellation observation, terminal worker-error recovery, and v2–v5 compatibility.
- [x] (2026-08-22 00:55Z) Slice 8: added deterministic Git-base affected selection, validated repository-relative inputs/roots, explainable direct and component propagation, post-filter action dependencies, source-complete freshness, CLI/MCP acceptance coverage, and no artifact cache.
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

- Observation: Clap's external-subcommand fallback retains options written after an unknown target as raw selector tokens, unlike known legacy check subcommands.
  Evidence: dogfooding `check repo:contract --no-receipt` initially treated `--no-receipt` as an action id; conversion now normalizes the closed repository-check option set on either side of selectors and has a regression test.

- Observation: moving generated action commands under `[commands]` made command ownership ambiguous during adoption reconciliation; an existing generated default could overwrite an explicit answers-file override, while indiscriminately preferring the render could erase a project override.
  Evidence: full-to-minimal and minimal-to-full adoption regressions now cover both directions; reconciliation receives the exact rendered command keys made authoritative by answers-file or CLI input and otherwise preserves compatible project values.

- Observation: deriving a v6 model from legacy singular render answers during `jig update` would collapse an authored Go-plus-Rust workspace even though runtime identity was already component-native.
  Evidence: `RenderAnswers::from_answers_file` now retains a complete authored repository model and its referenced commands for recopy, and a regression round-trips distinct `api:test` and `worker:test` targets unchanged.

- Observation: syntactically valid evidence selectors could still name a missing target or profile, so configuration parsing alone was insufficient to make `jig check contract` prove that work policy was executable.
  Evidence: evidence-selector resolution now has one repository-level implementation used by contract validation, work-check planning, and gate evaluation; a v6 regression rejects an unknown profile during contract check.

- Observation: exact profile proof requires correlating target receipts by run; choosing the latest receipt independently for each target would incorrectly allow partial runs to be stitched together.
  Evidence: the receipt index groups required targets by `run_id`, selects the latest complete group, and returns a detailed partial group only when no complete group exists; focused tests run `api:test` and `web:test` separately and keep the profile blocked.

- Observation: Serde's internally tagged unit variants accept extra object fields even with `deny_unknown_fields`, so deriving an input schema was stricter than deserializing `{"kind":"workspace","unexpected":true}`.
  Evidence: `jig.inspect` now performs one explicit discriminator-aware field check before typed deserialization, and both runtime and generated-schema regressions reject unknown fields.

- Observation: an in-process cancellation registry cannot support a cancel request delivered through a reconnected or second MCP process.
  Evidence: the run worker now polls the authoritative append-only cancellation event at a bounded interval while the local registry remains a low-latency signal; a regression writes only durable state and still reaps the owned process tree.

- Observation: a closed outer inspection response with `serde_json::Value` as its payload still generates an unconstrained output schema.
  Evidence: catalog inspection now builds typed workspace/component/target/profile payloads shared by CLI serialization and MCP schema generation; an end-to-end schema regression rejects an unknown nested result field.

- Observation: doctor process tests silently stopped reaching their child probes after their shared fixture advanced to v6 without component/action/profile records, and SQLx root fields were later inserted inside the final repository profile table.
  Evidence: the current-contract doctor fixture now carries a minimal native repository model, SQLx mutates adapter provenance on both authored and resolved sides, and both exact process-cancellation tests pass.

- Observation: the existing worktree fingerprint covered dirty state but not the checked-out commit, so two different clean commits could produce the same target input digest and make old evidence appear current.
  Evidence: the fingerprint now includes the verified `HEAD` object id (or an explicit unborn marker), and a regression proves two clean commits have different fingerprints.

- Observation: the first full Slice 8 gate exposed three stale migration tests rather than runtime failures: one string replacement no longer mutated the dynamic v6 evidence gate, and two Go Doctor fixtures still set the ignored v5 `backend_language` field while their v6 component advertised `rust`.
  Evidence: the gate-mutation test now asserts and changes the actual evidence record; the Go fixtures update authored and resolved adapter records; all three focused reruns and the complete composite test gate pass.

- Observation: `globset` 0.4.20 raises its Rust floor to 1.88 while this workspace supports Rust 1.85.
  Evidence: workspace dependency resolution pins 0.4.19; its resolved `bstr` dependency declares Rust 1.65 compatibility, strict Clippy and the complete tests pass.

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

- Decision: Every selected target receives a terminal target result and, unless explicitly disabled, a target-attributed receipt, including dependency, fail-fast, and cancellation skips. Compatibility `results` continue to list only commands that actually started.
  Rationale: durable evidence cannot leave ambiguous queued targets, while existing aggregate CLI consumers retain the meaning of their executed-result list.
  Date/Author: 2026-08-21 / Codex.

- Decision: Do not implement artifact caching or infer action dependencies from component dependencies.
  Rationale: delegated build systems own caches, and the architecture graph and execution graph express different facts.
  Date/Author: 2026-08-21 / Codex.

- Decision: Adapter defaults generate new or deliberately re-adopted v6 models, while a complete checked-in v6 repository model remains authoritative during recopy/update; malformed runtime tables fall back to repair generation.
  Rationale: stack-agnostic authored monorepos must not be projected back through a singular backend answer, but update must retain its established ability to repair invalid runtime configuration.
  Date/Author: 2026-08-21 / Codex.

- Decision: A profile evidence gate is satisfied only by one run containing successful current receipts for every current profile target; evidence from separate runs is never combined.
  Rationale: a profile represents an atomic verification claim, and cross-run stitching can hide incompatible source/configuration states or a target that was never exercised alongside its peers.
  Date/Author: 2026-08-21 / Codex.

- Decision: Keep target evidence evaluation and target receipt correlation in focused submodules while the existing gate and receipt modules retain shared orchestration and compatibility logic.
  Rationale: the new model has distinct freshness and grouping invariants; isolating them keeps the already-large legacy gate paths reviewable without duplicating shared receipt behavior.
  Date/Author: 2026-08-21 / Codex.

- Decision: Contract v6 advertises only `jig.inspect`, `jig.plan_run`, `jig.execute_run`, and `jig.cancel_run` for repository execution; manifest tools remain aliases but are neither advertised nor callable through MCP. Contracts 2 through 5 keep direct manifest tools unchanged.
  Rationale: MCP complexity stays bounded as monorepos grow without silently breaking existing clients attached to older contract epochs.
  Date/Author: 2026-08-22 / Codex.

- Decision: A durable cancellation event is authoritative across processes; the live registry is only a fast path. Once execution has published a queued handle, an internal worker error is represented by best-effort terminal `blocked` target/run results rather than leaving an apparently live handle indefinitely.
  Rationale: durable handles must remain actionable after reconnects, and post-accept failures no longer have a synchronous protocol response in which to report infrastructure errors.
  Date/Author: 2026-08-22 / Codex.

- Decision: Keep MCP resources and Tasks-extension projection as compatible later transports over the same catalog and run ids rather than prerequisites for v6.
  Rationale: the initial bounded tools work with every MCP client; transport capability negotiation should not fork repository semantics.
  Date/Author: 2026-08-22 / Codex.

- Decision: Treat `--affected` as a filter over the ordinary selector/profile candidate set. Resolve direct component inputs and configured dependent propagation first, then expand action dependencies.
  Rationale: the same selector vocabulary composes predictably with affected planning, architectural component edges never become execution edges, and an unrelated change can produce an honest empty plan.
  Date/Author: 2026-08-22 / Codex.

- Decision: Action inputs use validated repository-relative forward-slash globs. An input outside its component root is explicitly global; component roots are a most-specific fallback only when no declared input matches a path.
  Rationale: checked-in policy remains stack-neutral and inspectable while overlapping monorepo roots do not swallow paths already owned by a more precise action input.
  Date/Author: 2026-08-22 / Codex.

- Decision: Keep target freshness conservative by covering the checked-out commit and complete non-`.agent/` worktree identity; do not reuse evidence or artifacts based on affected selection.
  Rationale: false invalidation is safer than stale proof, and introducing a per-target content hash or build cache requires a separate, explicit compatibility and performance design.
  Date/Author: 2026-08-22 / Codex.

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

Work from the repository root. Keep `plan_01M0JZ6Z8M48Y1JSFQ6C8MYXC8` active and update this file after each slice.

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

All state additions are append-only JSONL. Readers fold duplicate or repeated lifecycle observations deterministically and reject incompatible terminal events. Historical receipt and work-gate records continue to deserialize through optional/default fields. If an accepted worker returns an internal error, Jig best-effort completes unfinished targets and the run as `blocked`; if the whole server process is terminated abruptly, its last queued/running event remains inspectable rather than being deleted. Automatic orphan classification requires a durable worker lease and is deliberately not inferred from a missing in-process registry.

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

Slice 5 validation evidence:

    cargo test -p jig-contract -p jig-features -p jig-core -p jig-go -p jig-rust -p jig-sqlx -p jig-typescript
    # all passed

    JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh
    # refreshed both deterministic embedded snapshots

    cargo test -p jig-sh bootstrap --no-fail-fast
    # 566 passed; 0 failed; 1 ignored

    cargo test -p jig-sh update --no-fail-fast
    # 42 passed; 0 failed

    cargo clippy -p jig-contract -p jig-features -p jig-core -p jig-go -p jig-rust -p jig-sqlx -p jig-typescript -p jig-sh --all-targets -- -D warnings
    # passed

Slice 6 validation evidence:

    cargo test -p jig-sh gate --no-fail-fast
    # 61 passed; 0 failed

    cargo test -p jig-sh work_check --no-fail-fast
    # 23 passed; 0 failed

    cargo test -p jig-sh runtime::tests::work --no-fail-fast
    # 54 passed; 0 failed

    cargo test -p jig-sh bootstrap --no-fail-fast
    # initial run: 565 passed; 2 stale expectations failed; 1 ignored
    # both corrected v6 gate expectations then passed in focused reruns

    cargo test -p jig-sh embedded_template_snapshot_matches_live_templates
    # passed

    cargo clippy -p jig-sh --all-targets --all-features -- -D warnings
    # passed

Slice 7 validation evidence:

    cargo test -p jig-sh mcp --no-fail-fast
    # 29 focused library tests plus MCP process integrations passed

    cargo test -p jig-sh mcp_repository --no-fail-fast -- --nocapture
    # 5 passed, including durable external cancellation and owned-process cleanup

    cargo test -p jig-sh repository_input_schemas_reject_unknown_fields --no-fail-fast
    cargo test -p jig-sh repository_tools_have_closed_input_and_output_schemas --no-fail-fast
    # both passed; actual catalog and run outputs validate against their advertised schemas

    cargo test -p jig-sh doctor::tests::cancellation_during_noisy_codex_reaps_descendant_and_prevents_proxy_spawn -- --exact
    cargo test -p jig-sh doctor::tests::cancellation_during_production_sqlx_prevents_codex_and_proxy_spawns -- --exact
    # both passed after repairing the shared current-v6 fixture

    cargo fmt --all -- --check
    cargo clippy -p jig-sh --all-targets --all-features -- -D warnings
    cargo build -p jig-sh --bin jig
    # all passed

    JIG_DEV_BIN=target/debug/jig scripts/jig mcp
    # generated v6 harness smoke: exactly four repository tools, each with input/output schema;
    # jig.inspect workspace returned structuredContent.kind=workspace with two components

Slice 8 validation evidence:

    cargo test -p jig-sh affected --no-fail-fast
    # 14 passed, including Git-base safety, direct/global/root selection, propagation,
    # deterministic reasons, legacy rejection, CLI execution, and MCP planning

    cargo test -p jig-sh repository --no-fail-fast
    # 81 focused library/integration tests passed

    cargo clippy -p jig-sh --all-targets -- -D warnings
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
    JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
    # all passed through the fresh development binary

    generated Go + TypeScript v6 smoke repository
    # `check test --affected HEAD --explain` selected api:test by direct Go input
    # and web:test by the explicit api-to-dependent propagation policy, with stable reasons

    JIG_DEV_BIN=target/debug/jig scripts/jig check test
    # initial full run: 2,259/2,262 primary tests passed and exposed three stale branch fixtures
    # focused fixture corrections passed; complete rerun passed with receipt
    # receipt_01M0KFDN3THFNV45R97TDDDWQ7

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


Slice 8 complete: affected planning is a deterministic selector/profile filter over explicit Git changes, validated action input policy and component propagation; reasons remain inspectable across CLI and MCP, action dependencies stay separate, clean commits invalidate freshness, legacy contracts fail before Git resolution, and the complete repository test gate passes after closing three stale migration fixtures.


Slice 8 complete: deterministic Git-base affected selection now filters normal target candidates through validated repo-relative inputs and explicit component propagation, then expands action dependencies. CLI/MCP fixtures, strict Clippy, generated Go+TypeScript smoke, and the complete composite test gate pass; the full gate also exposed and closed three stale v6 migration fixtures.