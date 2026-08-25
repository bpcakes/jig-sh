# Agent-Native Repository Product Model

Status: accepted for implementation on 2026-08-21.

## Product statement

Jig turns a repository's local conventions into a versioned, typed, inspectable
contract that developers, coding agents, and CI can execute. Every execution
produces durable evidence tied to the exact repository state it validated.

Jig is a repository API and evidence control plane. It is not a replacement for
Cargo, Go tooling, package managers, Nx, Turborepo, Dagger, Taskfile, or a CI
provider. It gives those systems one stack-neutral interface and connects their
results to repository guidance, structured work, and completion gates.

## Problem

The contract through version 5 models a repository as one global collection of
tools and commands. Its configuration has a singular backend language, separate
frontend application records, and language-shaped command keys. Stable tool
names such as `jig.test` and `jig.fmt_check` do not identify the code they
operate on. A repository containing a Go API, a Rust worker, and a TypeScript
frontend therefore cannot represent all three test or format actions without
colliding or teaching Jig's core CLI about every stack combination.

Adding another language-specific branch would preserve that limitation. The
contract instead needs to model what is in the repository, what each part can
do, what a requested execution resolves to, and what evidence it produced.

## Core model

The repository model has the following relationships:

    Workspace
     |-- Components ---------------- component dependency and affected graph
     |    `-- Actions
     |         `-- Target = component:action
     |-- Profiles ------------------ named target selections
     `-- RunPlan --> Run --> TargetRuns --> Evidence
                                              |
                                     Gates evaluate evidence
                                              |
                                          Work plan

A **workspace** is the repository, its version-control state, defaults, and
resolved Jig contract.

A **component** is an addressable software unit. An application, service,
library, worker, command-line program, infrastructure package, or repository-
wide policy area can all be components. A component has a stable id, one root,
optional tags, component dependencies, and optional scoped agent guidance. The
reserved `repo` component owns operations that genuinely apply to the entire
repository.

An **action** is a typed capability offered by a component. Examples include
`test`, `lint`, `build`, `serve`, `generate`, `migrate`, and `contract`. An
action declares its semantic intent, effects, runner, inputs, dependencies,
timeout, and result parser. `task` is deliberately not the product term because
it is already overloaded by build tools, agents, and asynchronous protocols.

A **target** is one concrete component and action pair. Its canonical text form
is `component:action`, for example `api:test`, `web:lint`, or `repo:contract`.
The structured form always carries `component` and `action` separately so
callers never need to parse the text address.

A **profile** is a named, version-controlled target selection such as `quick`,
`verify`, `ci`, or `release`. A profile is configuration, not an execution. The
word `suite` is not used for static selection because test frameworks and CI
systems commonly use it for runtime aggregates.

A **run plan** is an immutable resolution of selectors and repository state
into exact targets, dependency edges, selection reasons, effects, inputs, and
execution policy. Its id is derived from its canonical contents. Executing a
plan after its contract or source identity changes must fail as stale rather
than silently resolve a different request.

A **run** is a durable execution instance. It contains target runs and remains
queryable after a client disconnects. Execution status is separate from its
terminal conclusion:

    status: queued | running | completed
    conclusion: success | failure | cancelled | timed_out | blocked | skipped

`blocked` means the target could not start because a declared prerequisite,
runner, or configuration value was unavailable. It is distinct from a command
that ran and found invalid code.

**Evidence** is the structured output of a target run: findings, log and
artifact references, timestamps, exit information, receipt identity, source
identity, configuration digest, input digest, and worktree fingerprint. Raw
logs remain available, but consumers must not need to parse human text to learn
the target, result, affected files, or normalized findings.

A **gate** is a policy over evidence. It can require a target or profile to have
a successful, current result, or require an agent review to contain no finding
at or above a threshold. A gate does not name a command and does not execute
work itself.

An **adapter** discovers or contributes component and action defaults for a
stack or delegated runner. Rust, Go, TypeScript, SQLx, Nx, Turborepo, and
Dagger integrations are adapters. Core planning and execution do not branch on
language.

## Graph rules

The component dependency graph and target execution graph are separate acyclic
graphs; catalog loading rejects cycles in either graph.
Component dependencies identify architectural relationships and help map
changed files to affected components. Action dependencies determine ordering
and concurrency for one run. A component dependency never implies that two
actions depend on one another. For example, `api` may depend on `shared` while
`api:test` and `shared:test` remain independent and run in parallel.

Affected selection begins with changed paths, matches action inputs and
component roots, includes declared repository-global inputs, and then follows
only the explicitly configured dependent propagation policy. Every selected
target records a stable reason such as `direct_input`, `component_dependency`,
`unclaimed_input`, `action_dependency`, `profile`, or `explicit`.

`--affected BASE` is a modifier over the ordinary candidate set: Jig first
resolves the explicit selectors or profile (using the default profile when
neither was supplied), then keeps candidates on affected components. Selection
is deliberately component-granular: action inputs contribute to their
component's combined path authority, and a path matching any of those inputs
retains every candidate target on that component. Per-action inputs explain and
scope component ownership; they do not independently prune sibling actions. It is
valid for that filter to produce an empty no-op plan when there are no changes
or when every claimed change belongs outside the candidate set. Only after filtering does
Jig add declared action dependencies, so component dependency policy never
silently becomes execution ordering.

The changed-path set is the sorted union of committed changes from the merge
base of `BASE` and `HEAD` plus staged, unstaged, untracked, and ignored
`.env`/`.env.*` worktree paths whose containing directory is not itself ignored.
An ignored dotenv has no committed baseline, so its presence is conservatively
reported on every affected plan. Jig prunes wholly ignored directories so build
products and dependency caches cannot enter source identity or turn every
target precondition into a recursive generated-tree scan. A repository that
intentionally keeps a dotenv under an otherwise ignored directory must unignore
the containing path. Generated repositories counter the remaining unavoidable
uncertainty with reviewed default ignore patterns while retaining observed
dotenv contents in the source fingerprint; removing those defaults or declaring
a dotenv as an explicit action input intentionally makes its presence keep the
owning component's candidates. The complete `.agent/` harness and runtime tree
is excluded and therefore cannot be used as a non-root component root or an
explicit action input. Action input globs use validated, repository-relative
forward-slash syntax. An input outside its component root
is an explicit repository-global input. If no action input matches a changed
path, Jig falls back to the most-specific containing component root. A root
component (`root = "."`) with declared inputs is intentionally input-authoritative
and does not receive this fallback. If no input or eligible component root
claims a changed path, Jig fails closed by retaining every candidate with an
`unclaimed_input` reason instead of silently treating the run as successful.
Repositories can classify genuinely non-impacting paths with the reviewed
`repository.affected_ignore` glob list. Ignore patterns are contract authority,
cannot match `.jig.toml` or `scripts/jig`, and are applied before the fail-closed
component-root ownership rule. An explicit action input takes precedence over
an ignore so reviewed dependency authority cannot be shadowed.
Dependent
propagation follows reverse `depends_on` edges only while each source component
opts into `propagate_affected_to_dependents`, and explanations retain the
originating component and path.

## Action safety and runners

Every action declares an intent and effects independently:

    intent: check | generate | serve | operate
    effects: read_only | worktree | process | external

`jig check` can select only read-only check actions. A generator or autofix
produces a reviewable changeset and never silently edits the worktree through a
read-only check. Long-running services use `jig dev`. Other declared actions
use an action-specific command or the MCP `jig.plan_run` and `jig.execute_run`
tools, which expose and require approval for their effects before execution.

A runner is an implementation detail of an action. Initial runner kinds are a
configured argv command and a Jig-native operation. Command runners declare
argv, working directory, explicit environment additions, and timeout; they do
not require an agent to supply arbitrary shell text. A delegated runner may
later pass an already resolved selection to an existing monorepo engine. When a
repository already owns a correct graph or cache, Jig delegates instead of
building a second one.

Execution verifies the complete repository source before and after each
started target so a read-only target cannot silently mutate inputs used by a
later target. An adjacent read-only target reuses the preceding postcondition;
worktree-mutating targets and unobserved gaps take a fresh precondition. The
execution phase therefore performs at most two source observations per started
target and reports their actual `count` and `elapsed_ms` as
`source_observations` in structured check output. This cost is intentionally
linear in executed targets because coalescing postconditions would lose exact
effect attribution and could let a later target run against mutated source.

## Human command-line experience

The common path uses semantic verbs and one selector language:

    jig check
    jig check test
    jig check api:test
    jig check 'web:*'
    jig check --affected origin/main
    jig check --profile ci --explain

    jig info component api
    jig info target api:test
    jig status run RUN_ID

Bare `jig check` runs the workspace's default verification profile. An
unqualified action name such as `test` selects that action across components.
A qualified selector addresses an exact target or uses a simple `*` wildcard
for either side. Named profiles are selected with `--profile`, not an ambiguous
positional token. `--explain` resolves and prints the plan without execution.
JSON output uses the same resolver and schema as MCP.

Affected planning is available only to component-native contract-v6
repositories, where the required inputs and propagation policy are inspectable.
The same request and reasons are available through CLI JSON and `jig.plan_run`.

Independent checks are grouped into dependency layers and currently run in a
deterministic sequence within each layer. All failures are collected by default
so a developer or agent can repair a complete batch; `--fail-fast` is an
explicit override. CI does not silently choose different targets; it selects a
checked-in profile or the same explicit selectors.

`jig info` is the static discovery surface for workspace, component, action,
target, profile, and configuration provenance. `jig status` is the dynamic
surface for runs, work, gates, loops, and configured providers. The distinction
prevents another overlapping inspection command.

## Agent and MCP experience

The MCP server exposes a small, stable tool surface rather than one tool for
every target:

- `jig.inspect` reads workspace, component, target, profile, and durable run
  information. Inspecting a nonterminal run also reconciles it to a blocked
  terminal result when its process-owned worker lease has disappeared. Bounded
  `jig.work_*` tools own work lifecycle information.
- `jig.plan_run` resolves selectors and closed per-target arguments, then
  returns an immutable run plan without executing it. Effectful actions require
  explicit selectors.
- `jig.execute_run` executes an unchanged plan and returns a durable run handle;
  worktree and external effects require exact `approved_effects` acknowledgement.
- `jig.cancel_run` requests cancellation of a running execution.
- Structured work lifecycle tools remain a separate, bounded `jig.work_*`
  namespace.

MCP resources are a compatible later projection, not a prerequisite for the
repository model. A future client-capability-aware surface can publish:

    jig://workspace
    jig://components/COMPONENT_ID
    jig://targets/COMPONENT_ID:ACTION_ID
    jig://runs/RUN_ID
    jig://work/PLAN_ID
    jig://guidance/COMPONENT_ID

Tools have strict input and output JSON schemas. The canonical response is
`structuredContent`; a text rendering is included only for compatibility. A
target conclusion of `failure` is a successful execution result, not an MCP
protocol error. Invalid arguments, stale plans, state corruption, and runtime
infrastructure failures before a run handle is accepted use the protocol's
error channels. If an accepted worker later fails internally, Jig best-effort
closes its unfinished targets and run as `blocked` for durable inspection.

Jig owns durable run ids and lifecycle state. The initial transport works for
every client by returning a run id and polling through `jig.inspect`. A later
Tasks-extension projection can reuse that id without changing the underlying
repository or run model.

Every effectful call is constrained by the checked-in action contract. Plans
include closed runner arguments and declared effects, and execution verifies
the plan's contract digest, source identity, and explicit worktree/external
effect approvals before starting. Jig never exposes a general agent-supplied
shell command.

## Authored configuration and resolved contract

`.jig.toml` remains the human-authored configuration. Contract version 6 adds
component, action, and profile configuration while retaining the strict unknown-
field policy. `.agent/jig-contract.json` becomes the resolved, versioned
intermediate representation consumed by the runtime and automation.

Adapters run during `jig init`, `jig adopt`, and `jig update`. They may infer
candidate components and actions from authoritative project files, but the
resolved contract records each field's provenance as declared, inferred,
inherited, or overridden. Runtime execution never performs hidden discovery
that could change behavior without a reviewable contract diff.

Contract versions 2 through 5 remain readable. For those contracts, the
runtime synthesizes a `repo` component and maps each legacy manifest tool onto
a compatible repo-scoped action. Existing command names, tool calls, receipts,
and work gates keep working. Contract version 6 templates emit native component
and action records and may use target-aware gates.

The singular `backend_language` and legacy language command keys remain
accepted for version 5 and earlier migrations. Version 6 does not use them as
the source of runtime identity. Rust, Go, TypeScript, and SQL integrations
contribute adapter metadata and component-scoped actions instead.

## Evidence, gates, and structured work

Receipt records gain optional `run_id`, structured target identity,
configuration digest, input digest, and normalized findings. Existing JSONL
records remain readable and append-only. A run produces one receipt for every
target result and an aggregate receipt only when compatibility requires it.

Gate configuration references a target or profile plus an evidence predicate.
Check and agent-review executions share the target result envelope. Review
evidence additionally records finding severity and source metadata. Gate
freshness compares the current worktree, contract digest, and target input
digest with the recorded evidence. Structured work links its plan to runs and
evidence but does not own or duplicate action definitions.

## Caching policy

Affected selection is part of contract version 6 because it reduces work only
after inputs and graph propagation are inspectable. Jig-level artifact caching
is not part of this migration. Cargo, Go, package managers, Nx, Turborepo, and
Dagger already provide stack-specific caches; duplicating those caches would
expand Jig into a build system. The run model retains configuration and input
digests so a future evidence-reuse policy can be added only after proving a
product need and defining safe freshness semantics.

Current target input digests conservatively include the committed non-`.agent/`
source tree and the complete non-`.agent/` staged, unstaged, and untracked
worktree projection together with the target's declared inputs. This safely
invalidates evidence when relevant content changes but can also invalidate it
for an unrelated source change. Commits containing only append-only `.agent/`
state do not invalidate the evidence they record. The digest is freshness proof,
not an artifact-cache key.

## Implementation migration

The implementation proceeds in eight independently testable and separately
committed slices:

1. Add `ComponentId`, `ActionId`, `TargetId`, `RunPlan`, and normalized result
   DTOs alongside the legacy contract.
2. Synthesize legacy tools as `repo` targets and preserve legacy aliases.
3. Add component and target inspection, selector resolution, `--explain`, and a
   useful bare `jig check`.
4. Extend append-only receipts with run identity, target identity, contract and
   input digests, and structured findings.
5. Convert Rust, Go, and TypeScript feature metadata into adapter contributions
   and remove the singular backend assumption from version 6 runtime identity.
6. Migrate work gates to evidence requirements over targets or profiles while
   retaining legacy tool gates.
7. Replace per-action MCP exposure for version 6 with inspect, plan, execute,
   and cancel tools plus output schemas and durable run lookup.
8. Add explainable affected selection after component inputs and both graphs
   are available. Do not add a Jig artifact cache in this migration.

Each slice must preserve versions 2 through 5, update generated templates and
public documentation when its surface becomes observable, add focused tests,
and keep the repository contract check passing.

## Acceptance

The migration is complete when a contract version 6 fixture containing at least
a Go API and TypeScript web component can do all of the following:

- list both components and their distinct `api:test` and `web:test` targets;
- run bare `jig check` through a default profile;
- resolve `jig check test`, `jig check api:test`, and an affected selection into
  deterministic plans with selection reasons;
- execute a plan and query a durable run with separate status and conclusion;
- record target-aware receipts without changing old receipt deserialization;
- satisfy a target-aware work gate only with current matching evidence;
- expose bounded MCP inspect/plan/execute/cancel tools with validated structured
  results; and
- continue to execute a version 5 `jig.test` tool and legacy work gate without
  changing their stable response fields.

The full workspace format, strict Clippy, tests, generated fixture checks,
contract check, and structured-work gates must pass through the freshly built
development binary.

Durable run lookup scans the complete active `runs.jsonl` stream for the
requested identity so it can reject duplicate or out-of-order lifecycle roots;
the cheap identity prefilter reduces decoding, not I/O. Operators should use
run archival to bound that active journal rather than weakening validation with
an early-stop index.

## Design influences

The separation between component and execution graphs follows the proven
project/task distinction used by monorepo systems such as Nx. Scoped target
addresses, affected selection, and dry-run plans follow the useful common core
of Nx, Turborepo, and moon. Semantic `check`, `generate`, and service verbs plus
typed effects follow Dagger's agent-oriented API direction. Structured status,
conclusion, and findings follow the GitHub Checks model. The MCP split between
resources, typed tools, structured results, and durable tasks informs the agent
surface. These influences shape the interface; they do not make Jig depend on
any of those products.
