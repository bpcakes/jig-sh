# Scheduled task worktree operations

## Outcome

Operators can prepare an isolated task checkout before starting its agent, inspect retained worktree storage, and recover from rejected configuration using actionable diagnostics. This is a task-filing plan; implementation has not started.

## Scope

Three independently deliverable improvements to scheduled Codex tasks and configuration loading. Use generic `ExampleProject` fixtures throughout. External scheduler memory limits, application verification retries, package installation policy, automatic worktree deletion, and automatic runtime upgrades are outside scope.

## Current-state evidence

Inspected source baseline: `a05a4b88ec7883c6ec170dd85940bfdc3cb9bdd6`.

- **Fact:** `crates/jig/src/runtime/loops/codex_task.rs::prepare_checkout` creates a detached Git worktree; the task then calls `run_codex_exec`. `context/loop_config.rs::LoopWorkflowConfig` has no preparation hook. Ignored dependency directories are absent from a fresh Git worktree.
- **Fact:** `runtime/loops/codex_task/checkout.rs::PreparedCheckout::finish` retains failed, dirty, or HEAD-changing worktrees. `docs/codex-task-operations.md` requires deliberate operator handling; acknowledgement does not delete them.
- **Fact:** `crates/jig-ui/src/dashboard/status.rs::StatusScheduledOccurrence` exposes lifecycle timestamps and a worktree path, without storage measurements.
- **Fact:** `context/config_snapshot.rs::load_config_snapshot` wraps parser errors with advice to remove typos or experimental keys, without adding the executing runtime identity. `RepoConfig` rejects unknown fields.
- **Inference:** Explicit preparation and bounded storage inspection would remove recurring operator work without transferring package-manager or publication ownership into Jig.
- **Unknown:** The preparation implementation must identify an execution path that enforces the configured task sandbox and effect authority. The T-01 implementer resolves this before enabling the hook; absence of an enforceable boundary must fail preflight, never fall back to unrestricted host execution.

## Decisions and design

- Preparation is optional, uses configuration from the trusted controller baseline, and runs after checkout creation but before agent launch. It must not widen the task's execution authority. Default behavior remains unchanged. Dependency freshness and writable cache placement belong to the repository's preparation command; Jig records whether it succeeded.
- Partial preparation is a recovery obligation. If a command started and fails, times out, or is interrupted after possible writes, retain the checkout and require attention rather than automatically replaying its side effects. Preserve existing worker at-most-once semantics, leases, and evidence.
- Storage inspection is explicitly requested and read-only. Bound traversal, do not follow symlinks, and represent partial, missing, or unreadable results honestly. Reuse recorded lifecycle timestamps for age; do not invent a persisted creation timestamp for old occurrences.
- Configuration diagnostics must preserve strict rejection and the parser's actual cause. Runtime/configuration mismatch is a conclusion only when supported by known contract or source identity; an unknown key alone does not prove that upgrading will fix it.
- Public configuration and JSON additions require the repository's compatibility/versioning path. Older configurations and recorded occurrences remain readable; an older binary must reject an unsupported preparation option clearly rather than silently skipping it.

## Execution graph

All three tasks have no prerequisite edges. Suggested delivery order is T-01, T-02, then T-03; this is priority order, not a dependency. Each is its own delivery path. Tasks may touch shared configuration/docs, so coordinate those edits if implemented concurrently.

| Plan task | Beads task | Priority |
| --- | --- | --- |
| T-01 | `jig-sh-5h5z` | P2 |
| T-02 | `jig-sh-nkqm` | P2 |
| T-03 | `jig-sh-1dmt` | P2 |

<a id="t-01"></a>

### T-01 — Prepare isolated task worktrees before agent launch

- Outcome: A configured repository preparation command can make dependencies and local writable caches available before the task agent starts.
- Context: Worktree creation and agent launch currently have no repository preparation stage; use the first two evidence items and the authority decisions above.
- Changes: `crates/jig/src/context/loop_config.rs`, workflow resolution, `runtime/loops/codex_task.rs` and its checkout/pre-execution handling, task result/receipt reporting, loop tests, `docs/configuration.md`, and `docs/codex-task-operations.md`. Reuse existing command execution, cancellation, logging, and authority mechanisms where they enforce the required boundary.
- Depends on: none
- Verify: Generic temporary-repository integration tests prove ordering, no-hook compatibility, missing/failed/timed-out/cancelled preparation, no worker launch after failure, no replay after partial preparation, retained evidence, and lease cleanup. A fixture exposes dependency inputs only after preparation, compares manifests against the actual checkout, and keeps TypeScript-like cache writes local. Test that configured sandbox restrictions remain effective. No real package downloads or paid providers are required.
- Recovery: Removing the optional configuration restores the old path for future tasks. Preserve partially prepared worktrees and evidence; do not delete, retry, or widen permissions to clear a failure.
- Done when: A task starts with prepared dependencies only after recorded preparation success, and every preparation failure leaves an actionable result with unchanged worker execution guarantees. Configuration and operations docs explain ownership, authority, and recovery.

<a id="t-02"></a>

### T-02 — Expose bounded retained-worktree storage details

- Outcome: An operator can request worktree age and storage details, including ignored build artifacts, through loop status without a manual filesystem investigation.
- Context: Existing occurrence status already contains paths and timestamps; extend observation rather than changing retention. Related task `jig-sh-dx5` concerns ledger pruning, not storage inspection, and is not a prerequisite.
- Changes: `crates/jig/src/cli/loops.rs`, command mapping, `runtime/loops/engine/status.rs`, shared status/recorder types in `crates/jig-ui`, CLI rendering, focused loop status tests, and `docs/codex-task-operations.md`.
- Depends on: none
- Verify: Fixtures include ignored `target/` files, links outside the worktree, missing/unreadable paths, and a bounded traversal that cannot finish. Human and JSON output distinguish complete measurements from partial/unknown results, state the size-accounting method, and report lifecycle age from known timestamps. Default status performs no recursive disk scan. Prove status leaves files, occurrence state, and leases unchanged and old records remain readable.
- Recovery: Inspection is optional and read-only; a measurement failure must not prevent the rest of status from being returned. No deletion or automatic garbage collection is introduced.
- Done when: Operators can identify retained worktrees consuming storage and understand their recorded lifecycle age, with explicit incomplete results and documented manual inspection/cleanup guidance.

<a id="t-03"></a>

### T-03 — Make rejected configuration diagnostics identify the runtime

- Outcome: A configuration rejection explains which runtime rejected it, the actual parser/contract problem, and a supported inspection or recovery step.
- Context: `context/config_snapshot.rs` currently gives generic removal advice. Related task `jig-sh-1wi` owns durable release selection and enforcement; this task improves diagnostics with currently available identity and does not depend on that feature.
- Changes: `crates/jig/src/context/config_snapshot.rs`, strict configuration and contract tests, existing CLI error rendering/launcher diagnostics where needed, and configuration troubleshooting documentation. Use existing build/source identity and contract support data rather than adding a new pin format.
- Depends on: none
- Verify: Cover malformed TOML, a misspelled/unknown key, a supported configuration, and genuinely unsupported contract versions in generic fixtures. Preserve parser location/cause and machine-readable error compatibility. Include actual binary identity and only known configuration/contract identity. Unknown identity is explicit; no fixture should imply that every unknown key means an outdated binary.
- Recovery: Diagnostic-only behavior preserves config bytes, exit failure, strict unknown-field checks, selected runtime, and offline operation. Never auto-remove keys or upgrade the runtime.
- Done when: An operator can identify the rejecting executable and the concrete failure without guessing, and the diagnostic suggests a valid next step without asserting an unproven version mismatch.

## Verification

For this task export, validate the plan structure and dependency graph, re-read each Beads task, confirm all three are ready, and run `python3 scripts/beads-sync.py --check`. Each implementation runs its focused tests and required repository gates, including `scripts/jig check test` for backend changes and current-source validation through `scripts/jig-dev` where applicable. Task-local ExecPlans are created during implementation if required by the eventual scope.

## Rollout and recovery

Ship the tasks separately. Preparation and storage scanning are opt-in; configuration diagnostics improve existing failure paths. Preserve existing configuration contracts, occurrence history, and receipt attribution. No filesystem cleanup, runtime installation, or live scheduler changes are part of this plan.

## Risks and open decisions

- **Preparation authority and replay:** T-01 owns this boundary. Confirm the sandbox/effect mechanism before implementation and fail closed if it cannot enforce the selected authority.
- **Status latency and misleading sizes:** T-02 chooses a documented accounting method and explicit traversal bounds; incomplete data must not be presented as a total.
- **Misdiagnosing compatibility:** T-03 uses established identity/contract evidence and keeps unknown-field errors distinct from proven version incompatibility.
- **Tracker scope:** Keep the existing release-pinning and occurrence-pruning tasks separate; no artificial blocker edges are needed. The plan/task link is the source of scope and acceptance until implementation begins.
