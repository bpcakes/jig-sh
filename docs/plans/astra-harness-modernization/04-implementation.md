# Task 04 implementation

Localize specialized guidance while preserving the instructions an agent needs before
editing an owning module. Scope is task `jig-sh-9wcn.4`; tasks 05 and 14 remain separate.
Baseline: `dc00cc215933716cf5367261a23710913fe00ba7`.
Work plan: `plan_01M2A1T0W08386TM6A0HJT3V3W`.

## Progress

- [x] Inspect current guides, template renderers, guide policy, and task dependencies.
- [x] Move Beads reference; retain essential root workflow and privacy/Git rules.
- [x] Move process, vault, bootstrap, and frontend installer guidance with entry links.
- [x] Make map discovery optional in root, template, snapshot, and generator.
- [x] Validate links, movement inventory, rendering, and guide discovery.
- [x] Pass required gates on the settled worktree.
- [x] Record acceptance for structured-work completion and Beads closure.

Restart checkpoint: implementation and verification are complete. No further code edits
are needed. Finish structured work and close the Beads task if their records are still
open; consult the current work receipts before repeating checks. No adoption footprint
or persisted format changed.

## Surprises & Discoveries

The crate guide still referred to `crates/jig/src/process.rs`, which no longer exists.
Generic process execution now belongs to `crates/jig-owned-process`; its current guide
already had shorter versions of several moved obligations. Those are merged rather
than duplicated. Doctor signal-session and Bash probe rules stay in a runtime reference.

The task-02 synthetic fixtures contain root/planning guidance only, without the full
Jig harness or scoped guides. Their frozen baseline must stay unchanged. Offline path
discovery checks can use their task paths, but cannot establish measured Astra outcomes.

## Decision Log

2026-09-12: Use module guides for bootstrap and vault, with explicit links covering
sibling `.rs` entrypoints. Keep vault guidance out of ancestor runtime guides.
Use the existing owned-process crate guide for generic supervision, and a linked
reference for doctor sessions and Bash probes whose owners span modules.

2026-09-12: Keep application commands, coverage contracts, migrations, and project-owned
registry/authentication/install-script policy in generated guidance. Move install scope
and validation-stage details to the maintained adoption reference, linked via the source
repository URL so generated consumers do not acquire a new managed documentation file.

## Outcomes & Retrospective

All task-04 outcomes are implemented and verified. No model-performance improvement
is claimed; comparative Astra evaluation remains task 15.
The [sentence-level inventory](04-movement-inventory.json) records original crate
invariants and Beads reference lines, destinations, retained guidance, and rewordings.
No substantive invariant is retired.

| Original guidance | Destination |
| --- | --- |
| Root Beads command reference | [Beads reference](../../beads-workflow.md); essential workflow remains at root |
| Generic process cleanup | [Owned-process guide](../../../crates/jig-owned-process/AGENTS.md) |
| Doctor sessions and Bash probes | [Process supervision](../../process-supervision.md) |
| Vault scope, input, output, lifecycle, TUI | [Vault runtime guide](../../../crates/jig/src/runtime/vault/AGENTS.md) |
| Bootstrap identity, publication, installers, scaffolds | [Bootstrap guide](../../../crates/jig/src/bootstrap/AGENTS.md) |
| Generated frontend installation detail | [Adoption reference](../../adoption.md#frontend-dependency-installation) |

## Validation and recovery

From the repository root, build `cargo build -p jig-sh --bin jig` and use
`JIG_DEV_BIN=target/debug/jig` for harness commands. Regenerate with
`scripts/jig agent-map generate`, then run `scripts/jig check agent-map` and
`scripts/jig check agent-guides`. These Jig-owned policy subcommands reject
`--plan-id`; they run separately from target evidence. Run configured verification through
`scripts/jig work check --plan-id plan_01M2A1T0W08386TM6A0HJT3V3W`.

Review frontend/no-frontend renders and existing full/minimal adoption and unmanaged
block preservation tests. Check local Markdown links and compare moved invariant
sentences with destinations. Use task-02 fixture paths for an offline guidance scope
check; no paid or comparative evaluation is required here. Record observed results
below, including failures. Repeated work checks may reuse current evidence.

## Observed verification

- Fresh dev binary built successfully.
- Frontend and no-frontend `harness-only` renders passed command, policy, optional-map,
  coverage, app metadata, and installer-reference checks. Rendered frontend guidance
  omits installation scope and validation-stage details.
- All 36 original crate invariant sentences were found verbatim in their destinations.
- Initial changed/new Markdown scan checked 37 local links with no missing targets.
- Six real source paths reached the appropriate moved rules through nearest-guide
  ancestry or explicit crate-guide references; vault scope did not leak to other paths.
- Task-02's five fixture families (16 existing file paths) resolved to the root guide
  in isolated generated-harness wrappers, with optional map lookup and no placeholder
  local-guide requirement. Frozen fixtures and model evaluation conditions are unchanged.
- `check agent-map --json`: 19 guides, no missing guides or broken links.
- `check agent-guides`: passes but selects zero guides under this repository's current
  root-component configuration. Direct section checks covered the two new module guides
  and the updated owned-process guide; existing policy tests ran in the focused suite.
- Focused nextest run: 43 passed (renderer, package-manager rendering, map policy,
  guide-preview copying, and adoption modes). The 2,805 excluded tests were outside
  this focused selection; the configured full suite subsequently passed as recorded below.
- Initial attempts to give policy subcommands `--plan-id` were rejected by argument
  validation; rerunning their supported forms succeeded. No check was bypassed.

- Embedded `update --recopy` preserved an appended ExampleProject local-policy sentinel
  while retaining the new frontend reference.
- First full gate run: 803 Rust tests passed, one renderer assertion failed because it
  still expected mandatory map reading, and fail-fast left 3,279 unrun. Updated the
  existing Rust workspace and scaffold guidance expectations for optional discovery.
  All 38 Python tests passed, but edits to this worktree during execution invalidated
  the layer's receipts. The failed records remain intact. Finish documentation edits
  before rerunning; do not treat those receipts as passing evidence.

- Final local-link scan: all 59 links resolve; both edited embedded snapshots match
  their source templates. Root template-contributor guidance also links directly to
  the bootstrap guide, and crate guidance routes scaffold toolchain checks there.
- A focused renderer run compiled before the backend wording correction landed and
  failed its old `before backend work` expectation (11 passed, 18 unrun). Both Rust
  and backend expectations are now updated in source; the next full run verifies them.

## Acceptance results

Run `run_01M2A293J53B831X21NHMZDR29` passed all six configured targets, with
validation receipt `receipt_01M2A2PPT3XZ2PGVDBQHMGYPG5`: 4,083 Rust tests passed,
three configured skips, 38 Python tests passed, plus Clippy, formatting, contract,
and file-budget checks. Source changes are uncommitted over the baseline above.
The final structured-work receipt may reuse these qualifying passes after documentation
updates; consult `work receipts` for the closure and latest validation record.

| Acceptance criterion | Observed outcome |
| --- | --- |
| Root has no Beads command encyclopedia | Full reference moved verbatim to docs; short root workflow retained |
| Sync, privacy, Git rules remain visible | Root retains claim verification, robot-only bv, sync-helper-only export, Git policy and the unchanged fixture privacy section |
| Frontend root omits installer trivia | Frontend/no-frontend renders checked; package scripts, coverage, app metadata and project-owned install policy retained |
| Critical invariants remain discoverable | Scoped vault/bootstrap guides, owned-process guide and process reference linked from entry guides; 36 invariant sentences preserved verbatim |
| No substantive guidance lost | Sentence-level movement inventory and exact Beads-body comparison passed; obsolete process paths corrected |
| Agent map stays useful and compatible | Generator/snapshot/root wording aligned; 19 guides indexed with no missing/broken map links |
| User-authored blocks survive updates | Embedded recopy preserved the ExampleProject sentinel; full/minimal adoption and ownership tests passed in the required suite |
| Guidance supports other clients | Plain Markdown and existing CLI routes; no new model, client, delegation or skill requirements |

Final diff review confirmed Rust changes are limited to map introduction strings and
existing guidance assertions. Managed-file ownership, full/minimal footprints, runtime
execution and verification policy are unchanged. All newly introduced local Markdown
links resolve, including explicit entrypoint links for sibling module files.
