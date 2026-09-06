# Delete the legacy web UI and obsolete status TUI

This ExecPlan implements Task G (`jig-sh-l2x.8`) from `docs/plans/unified-terminal-dashboard.md`. The public cutover is already complete, so this task removes the rollback-only HTTP architecture and duplicate `jig-status-tui` package while preserving the unified `jig-ui` terminal engine and the three supported JSON entrypoints.

Implementation baseline: `0992b022` on branch `jig-sh-l2x`.

## Progress

- [x] Read repository/crate guidance, Task G and its referenced acceptance sections; claim the Bead and open structured work.
- [x] Remove the HTTP server, HTML renderers, route/query/provider API, and their now-unused dashboard-only dependencies.
- [x] Remove the obsolete `jig-status-tui` package, workspace/release/dependency wiring, and dead CLI adapters.
- [x] Add manifest- and source-backed negative deletion tests without changing generated launchers or contract epoch 7.
- [x] Run focused validation and exactly two comprehensive Claude+Codex review/fix rounds.
- [x] Close the Bead, pass the final exact-diff gates, record evidence, finish structured work, and prepare the isolated deletion commit.

## Surprises & Discoveries

- Workspace dependencies `askama`, `getrandom`, and `subtle` have consumers outside the retired dashboard (`jig-dev-proxy` and `jig-vault`). Only `jig-ui` direct dependencies are web-only and removable; the shared workspace declarations must remain.
- The old `SnapshotProvider`, route query DTOs, `crates/jig/src/ui/snapshot.rs`, and `serve_legacy` are still compiled solely as Task F's rollback boundary. The new typed source has independent recorder, plan, and status projections and does not need those interfaces.
- Agent-map discovery deliberately follows the Git index so an accidental unstaged guide deletion remains visible and fails validation. Staging the intentional `jig-status-tui/AGENTS.md` deletion before regenerating the map removes that index entry and preserves the guard.
- Round 1 found that several old web tests carried still-valid invariants. Their old DTO assertions were deleted, while read-only/no-directory-creation and refreshed gate-authority assertions were moved onto the real typed production source. A platform-neutral CLI test now covers recorder, plan, and status JSON together.
- Round 2 showed that accepting missing tracked guides in discovery would hide accidental deletions. That proposed change was reverted; the intentional crate-guide deletion is staged before map generation/checking, preserving the original guard. The final review also strengthened isolated-Git JSON coverage, exact root-field assertions, typed invalid-body behavior, refreshed repository metadata, and filesystem-tree immutability.
- Final gate runs exposed pre-existing host-contention sensitivity in unchanged process, loop, and dev-proxy tests. Each failure passed immediately in isolation; the existing `jig-sh-ccz` issue tracks the underlying owned-process cleanup class, so Task G does not broaden into unrelated harness repairs.

## Decision Log

- Treat this as one direct architectural deletion: retain the `jig-ui` package identity and its `dashboard`/`terminal` modules, but remove its old `model`, `html`, and `server` modules and APIs.
- Preserve status collection and `jig status --json`; remove only `status/tui.rs`, because both terminal entrypoints now route through `ui::run_status`.
- Put deletion assertions at a repository-level test boundary that inspects Cargo metadata/manifests and production source paths. Avoid broad token searches that would falsely reject the unrelated development proxy or generated application fixtures.
- Leave README, public guides, changelog, and generated agent-map cleanup to Task H, except source-owned crate guides or tests that must change for the deleted package to compile and validate.
- Keep source-owned crate entrypoint guides and `agent-map.md` consistent in this deletion commit because broken paths fail repository gates and misdirect the next code change; Task H still owns the broader public documentation narrative.
- Preserve agent-map's index-backed missing-guide detection. An intentional guide deletion must be represented in the Git index before regenerating the map; an unstaged accidental deletion remains a policy failure.

## Outcomes & Retrospective

Task G removed the retired HTTP dashboard and its HTML/model/provider surface, deleted the duplicate `jig-status-tui` crate and legacy CLI adapters, and left one supported `jig-ui` terminal implementation plus the three recorder contracts. Cargo metadata, release ordering, crate guides, and the generated agent map now reflect that topology.

Repository-level architecture tests prevent the deleted crate, direct web-only dependencies, server files, and retired server symbols from returning. Portable isolated-repository tests cover exact recorder/plan/status JSON roots, refreshed Git metadata, typed invalid-body errors, and read-only filesystem behavior. Exactly two comprehensive Claude+Codex review rounds were completed and every supported finding was resolved; the rejected missing-guide relaxation was reverted because it weakened agent-map policy.

All eight required work gates are fresh and passing. The final aggregate run passed 3,914 tests with two skipped and one transient retry; the final core receipt passed 3,148 tests with three transient retries. Frontend passed 112, vault passed 443 plus 2, and process passed 209. Earlier high-load attempts exposed unchanged process/loop fixture sensitivity and each observed failure passed in isolation; `jig-sh-ccz` already tracks the underlying parallel owned-process cleanup class. No production timeout, assertion, or gate was weakened.

## Context and orientation

`crates/jig-ui/src/server.rs`, `html.rs`, `html/`, and `model.rs` form the retired browser implementation. `crates/jig/src/ui.rs` and `ui/snapshot.rs` retain its CLI adapter and repository projection. `crates/jig-status-tui` is the old standalone terminal implementation whose functionality now lives under `crates/jig-ui/src/terminal`. Workspace membership and dependencies are in the root and crate Cargo manifests; publication order is in `scripts/release.sh`.

## Plan of work

First remove obsolete source/module edges and crate wiring. Refresh Cargo metadata and the lockfile, then add narrow negative regression coverage for package membership, direct dependencies, publication order, and production dashboard server tokens. Run targeted CLI/JSON and terminal tests to prove retained behavior, followed by two independent review rounds and the full configured gates.

## Validation and acceptance

`cargo metadata --locked` must show exactly one `jig-ui` package and no `jig-status-tui`. `jig-ui` must not directly depend on the web-only random/comparison dependencies. Production dashboard source must contain no listener, routes, cookies, capabilities, HTML, CSS, server module, legacy provider, or loopback port constant. `jig ui --json`, `jig ui --plan ID --json`, and `jig status --json` must remain green, as must `jig-ui`, shared/specialized TUI tests, strict Clippy, file budget, and all plan-bound gates. Generated launcher templates and `.agent/jig-contract.json` remain unchanged.

## Idempotence and recovery

No state is written or migrated by the runtime change. This task is one isolated deletion commit; reverting it restores the old server and obsolete crate wiring. Cargo metadata and lockfile regeneration are repeatable.

## Interfaces and dependencies

Retain `jig_ui::dashboard`, `jig_ui::terminal`, `RepoDashboardSource`, `ui::run`, `ui::run_status`, and status schema-1 collection. Remove `UiServer`, `SnapshotProvider`, `UiQuery`, old web DTOs/renderers, `DEFAULT_UI_PORT`, `jig_status_tui`, and their direct Cargo/release edges.
