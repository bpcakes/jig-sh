# Author reviewed adoption components

This living ExecPlan follows `.agent/PLANS.md`. Its Git baseline is `fb3c110a`
on master. It implements Bead `jig-sh-generic-monorepo-zac.1.3` after the
foreground-run task was committed separately.

## Purpose / Big Picture

`jig adopt` will show which repository directories it proposes to treat as
components, with evidence and an explicit included, excluded, or review-required
state. A component is a named directory recorded in `[repository.components]`
and used by Jig's existing action planner. A raw incidental manifest must remain
visible without silently creating a component. Users can repeat
`--include-component ROOT` and `--exclude-component ROOT` to select exact roots.
The preview and the files produced by `--write` must describe the same decision.

## Progress

- [x] (2026-09-07) Audit current inference, authoring and preservation paths; verify the Bead is unblocked and open structured work.
- [x] (2026-09-07) Add deterministic manifest/workspace candidates and exact-root decisions with focused tests.
- [x] (2026-09-07) Connect selected candidates to actual authored components, preview output and existing-model preservation.
- [x] (2026-09-07) Add generic write/recopy and command-line regressions; update adoption docs (CLI validation passed).
- [x] (2026-09-07) Run focused validation and file budget, comprehensive review (two rounds), required gates and backend tests.
- [x] (2026-09-07) Finish structured work and close/sync the Bead; include this plan in the adoption implementation commit.

## Surprises & Discoveries

`RepositoryRenderModel::from_answers` currently creates an `api` component at
`.` even when inference found only incidental nested manifests. Filtering a
preview or legacy `rust_crate_roots` strings cannot solve the requested problem.
Frontend components also receive an inferred dependency on that synthetic api.
The new initial projection must remove synthetic owners and their inferred edges
when no corresponding candidate is accepted, while preserving authored edges.

Several owning modules already exceed their file budgets (`repository_model.rs`
and `answers.rs`), and `adopt_infer.rs` is 799 lines. Put cohesive logic in child
modules and extract existing related methods as necessary. Check the native file
budget before long validation, including its no-growth rule for existing debt.

## Decision Log

2026-09-07: Select exact normalized roots, not globs or runtime ignore patterns.
Reject absolute/escaping paths, glob requests, unknown roots, symlink traversal,
and a root present in both include and exclude lists before managed-file writes.
A root shared by several manifest kinds selects all candidates at that root.

2026-09-07: Root manifests, declared workspace members and fully recognized
frontend apps supply strong evidence. Fixture/example/test paths and ambiguous
raw manifests remain review-required unless explicitly included. Respect native
workspace exclusions when determining membership. IDs are assigned from the
complete candidate set before include/exclude decisions so flag order does not
rename components. Evidence is repository-relative and fixtures are generic.

2026-09-07: Accepted candidates become real ComponentSpec records. Preserve
existing inferred aggregate root commands and frontend commands on their accepted
owners; do not duplicate aggregate commands or legacy aliases onto every member.
A member can be represented without per-member actions. The repository policy
component remains present. Component exclusions do not rewrite Cargo or package
manager dependency/build graphs and do not alter runtime affected-path semantics.

2026-09-07: Explicit answer inputs remain authoritative. Add their component
intent to the review and reject conflicts with exclusions rather than silently
throwing away commands/apps. Preserve complete existing authored models during
readoption and update/recopy; selection flags must not report changes that the
copy/reconciliation path will discard. Reject incompatible selections on an
existing authored model with guidance to edit that model explicitly.

## Outcomes & Retrospective

Candidate discovery, selection, authoring and preservation are implemented. Both
review rounds are complete and all findings have recorded dispositions below.
The separate foreground-run commit is `fb3c110a`. The final default work-check
and literal backend check both passed all 3,884 workspace tests. All required
gates are fresh and passed (with the process-path gate explicitly not applicable).
The work plan and Bead are closed. This plan accompanies the adoption
implementation commit; the earlier foreground-run task remains a separate commit.

## Context and Orientation

`crates/jig/src/bootstrap_parts/part_01.rs` defines AdoptOpts and the adopt flow:
validate destination, resolve template and answers, infer, preview, confirm and
render/copy. `bootstrap/adopt_infer.rs` owns AdoptInference. Its `scan.rs` provides
bounded manifest reads and directory/file discovery; `frontend.rs` resolves
JavaScript workspace membership and recognizes complete frontend script sets;
`topology.rs` and `crate_classification.rs` identify nonproduction Rust roots.

`bootstrap/answers.rs::AnswerResolution::from_input` resolves defaults and user
inputs into RenderAnswers. `bootstrap/repository_model.rs` constructs ordinary
components/actions/profiles and has an authored-model path that preserves them.
`bootstrap/initial_copy.rs` hands those answers to the renderer.
`bootstrap/runtime_config.rs` reconciles commands and work policy on readoption.
`bootstrap/update.rs` loads the stored answers for update and recopy. These paths
must continue to consume stored component authority without scanning for new
manifests. The repository planner and affected algorithms need no changes.

## Plan of Work

The first milestone adds `bootstrap/adopt_infer/components.rs` and focused child
tests. Define candidate records carrying root, proposed ID, ecosystem, evidence,
confidence, disposition and reason. Reuse RepoScan and existing frontend workspace
knowledge. Read Cargo workspace declarations without executing project code;
resolve supported member patterns against scanned manifests conservatively.
Keep ambiguous and malformed manifest observations visible but unselected.
Selection validation is pure over the candidate set and repository paths.

The second milestone adds adoption-only flattened CLI options and an internal,
non-CLI selection field on AnswerOpts. Extend the existing inference/answer flow
so only selected inferred apps and root owners feed the initial authored model.
Use `repository_model/adoption.rs` for the projection and `answers/adoption.rs`
for installing its component/action/command authority before rendering. Reuse
existing root/app command generation; remap generated references when needed and
remove only inferred edges to removed synthetic owners. Preserve complete prior
models and explicit answers. Update every AdoptOpts test literal directly.

The third milestone proves human/JSON parity and actual write/recopy behavior.
Use temporary ExampleProject fixtures with a declared workspace, accepted members,
incidental fixtures and a frontend app. After writing, parse the real `.jig.toml`
and manifest and load RepoContext/RepositoryCatalog; excluded/review-required
candidate IDs must be absent. Add another incidental manifest, recopy, and prove
authored components remain unchanged. Exercise errors before destination writes.
Update `README.md`, `docs/adoption.md`, `docs/developer-ux.md` and
`docs/public-contract.md` with exact-root semantics and action-inference limits.

## Concrete Steps and Validation

Run commands from the repository root. Begin with focused new tests using
`cargo nextest run -p jig-sh --lib -E 'test(adopt_components)'`; use real CLI tests
for option parsing and preview output. Successful tests must prove component
contents and rejected writes, not merely compare two views of the same struct.
Broaden to the relevant existing adoption/recopy tests after integration.

Build `cargo build -p jig-sh --bin jig`. Use the development binary for every
harness command. Before long gates, run:

    JIG_DEV_BIN=target/debug/jig scripts/jig run repo:file-budget --plan-id plan_01M1XYEPFPB8QTMZT8AM78Z6G1 --json

Run the comprehensive-review skill on the complete change, fix findings, and
repeat at most once. Record scope and limitations. Then run:

    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M1XYEPFPB8QTMZT8AM78Z6G1
    JIG_DEV_BIN=target/debug/jig scripts/jig check test
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M1XYEPFPB8QTMZT8AM78Z6G1
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M1XYEPFPB8QTMZT8AM78Z6G1
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M1XYEPFPB8QTMZT8AM78Z6G1

All required gates must be fresh and passed, then finish work, close/sync the Bead
and commit. Preserve original failure receipts; an isolated pass does not prove
a full failed gate passed. No live services or credentials are needed for fixtures.

## Idempotence and Recovery

Preview must not mutate managed destination files. Selection errors occur before
copying. Reuse the existing adoption backup/undo and update transaction paths;
do not add another persistence mechanism. Failed writes use existing recovery.
Retries recompute decisions from the same current source and explicit inputs;
update/recopy use stored authority. Keep `.agent/state/*.jsonl` append-only.

## Interfaces and Dependencies

Keep candidate discovery in bootstrap and reuse existing TOML/JSON parsing,
component identifiers, frontend identity helpers and conservative workspace
matching. ComponentSelectionOpts contains repeated include/exclude strings.
ComponentCandidates owns the complete proposed set and selected dispositions.
Only accepted candidates enter the authored model; reports serialize the same
records consumed by authoring. No new contract epoch, process runner or runtime
path-ignore authority is introduced.

## Implementation evidence (2026-09-07)

The initial five candidate-discovery tests passed (Nextest
`0aab38b3-644c-48de-9d92-4b1394784d4a`). Integration tests now inspect real
`.jig.toml` components, resolved contract records and RepositoryCatalog loading,
then prove recopy/readoption preserve the source after new manifests appear.
Frontend selection tests prove excluded app actions and phantom backend edges
are absent. An incidental-only repository contains only its policy component.

The broader adoption run `93c69655-751d-495d-a0ec-dc81622c4b90` passed 205 of 210
tests. Its failures exposed full/minimal capability refresh, custom-model
preservation, and fixtures relying on an implicit Rust backend. The focused
repair run `492be0dd-f818-4f88-be0a-3bf3da49a0e0` passed 13 of 14, leaving the
Clippy migration fixture's missing explicit Rust intent; that fixture is now
corrected, with rerun pending. Original logs remain in `/tmp/jig-adoption-*.log`.
These focused runs do not substitute for the required gates and final backend
check. These results preceded comprehensive review round one.

Readoption preserves component identity and project-owned graph edges.
Explicit full/minimal or SQLx capability changes still refresh generated harness
actions; a fully custom authored model bypasses generated-action refresh.
Deleting an app manifest now fails validation while retaining its authored
ownership, so the corresponding historical test was changed to assert that
behavior. Fixtures testing Rust harness lifecycle now declare Rust explicitly.

The final focused candidate/CLI/migration run
`b67c9be7-f711-4def-a1ac-4b41e9c08fde` passed all 13 tests in 4.740 seconds,
including real human/JSON preview parity and selected-component write/recopy.
The four capability/preservation repairs passed in the prior focused run;
the remaining Clippy fixture now passes as well. Broad gates remain pending.

The native file-budget check passed against the exact work-plan baseline; original receipt `receipt_01M1Y0PH956MMRNG4VR53B20V4`. Comprehensive review round one starts from a frozen checkout of this source. Required gate receipts will be collected in the active checkout.

## Review round one and repairs

The frozen checkout `/tmp/jig-adoption-review-xayi1kqp` had complete matching
initial/final scope fingerprint
`0c844e5f2286977b2e30b863e840b662a294eb0acef058cbfd2417f56bd177b2`.
Codex completed. Claude's restricted Opus adapter reached terminal failure
(`claude exited with 1`) before returning findings. This is a single-reviewer
result, not a merged review. The three actionable Codex findings were:

- High: full-to-minimal readoption retained generated per-app frontend actions
  whose checker script was retired. Recognize their declared target and inferred
  runner provenance; regress actual saved command/action removal and catalog load.
- Medium: refreshing an opaque authored component reached initial projection's
  unreachable branch. Keep those components in the preserved graph, outside
  adapter generation; regress full-to-minimal with a custom tools component.
- Medium: readoption ignored explicit command overrides. Resolve the supplied
  flag through the authored legacy alias, update its actual command key, and make
  reconciliation retain that value. Reject native/missing owners before writes.

The first work-check completed unsuccessfully: Clippy reported a collapsible
conditional in refresh, and backend tests passed 924 before two file-budget
fixture failures. Those fixtures assumed an implicit Rust backend; they now
explicitly declare Rust. The frontend partition also completed. Original failure
receipts remain append-only. The focused repair run passed 14 of 16 tests;
the two new regressions needed fixture corrections (a package named exactly
`example` is deliberately nonproduction, and omitted empty adapter lists are
semantically equivalent). Their rerun is pending; no passing full gate is claimed.

The corrected command/opaque-component regressions both passed in Nextest
`35c52baf-efa7-432b-af41-180826ee0c59` (4.419 seconds). Together with the prior
14 passes, all round-one fixes and fixture corrections have focused evidence.
The development binary was rebuilt successfully. Final review round two uses
`/tmp/jig-adoption-review2-gh71opns`, complete initial fingerprint
`7f3c20f7952d428987b21405f3a6ae4e986c5ddf2b2f3ba4e75716f4d0fac69d`.
Both independent reviewers were launched before collecting either result.
A fresh default work-check is running in the active checkout.

## Final review (round two)

Claude and Codex both completed read-only against the same frozen scope.
The parent and both reviewers verified the complete fingerprint
`7f3c20f7952d428987b21405f3a6ae4e986c5ddf2b2f3ba4e75716f4d0fac69d`.
Claude used restricted file access. No third round will be started.

Merged findings and dispositions:

- High, Claude, `answers/adoption.rs` / `adoption_commands.rs`: inferred commands
  were mistaken for explicit flags. A new regression reproduced a custom
  command being replaced by `make test`. Preserved readoption now resolves stored
  answers plus explicit flags; inferred values remain observations. Coverage also
  includes a frontend-only owner with a Makefile test target and no Rust alias.
- Medium, Codex, `repository_model/adoption_refresh.rs`: renamed authored actions
  retained their aliases while refresh added duplicate owners. Generated merge
  now respects retained alias owners, remaps profile/dependency references and
  avoids orphan generated commands. The custom-target transition regression passes.
- Medium, Claude, `answers/adoption.rs` / `policy/agent_map.rs`: root Rust authority
  caused unrelated top-level guides to be classified as Rust crate guides.
  The authored guide consumer now excludes `.` from legacy child-guide fallback;
  explicit component guides remain checked. Root source authority is intentional
  for aggregate commands and SQLx TODO scanning, and is documented rather than
  silently narrowing a declared root component to a guessed directory.
- Medium, Claude, `initial_copy.rs`: a deleted frontend's error lacked complete
  retirement instructions. The diagnostic now names its exact root, frontend_apps,
  repository component/actions and dependency/profile references. Ownership stays
  preserved; the existing missing-manifest regression checks the guidance.
- Medium, Claude, `answers/adoption.rs`: alleged adopt/update order dependence
  after legacy-answer edits. Source inspection shows changed generated projections
  are classified custom by `loaded_repository_model_is_custom`; the new coverage
  threshold edit + readoption + recopy regression proves both preserve the same
  authored actions and commands. The claimed divergent regeneration is not
  reproduced. Documentation now explicitly requires coordinated metadata and graph
  edits; changing legacy metadata alone does not replace authored commands.
- Medium, Claude, `adopt_infer.rs`: workspace membership changes need an explicit
  input-policy workflow. Current v7 templates use an authored verify profile, not
  the obsolete generated work-gate paths referenced by this finding. Removed the
  misleading automatic-refresh comment. Readoption warns when membership differs;
  docs require updating frontend_workspace_roots and authored action inputs
  together. Tests prove readoption/recopy preserve saved input authority and do not
  silently add a new component.
- Low, Claude, `selection.rs`: stale SQLx metadata survived removal of the root
  backend. Both branches now clear old metadata/signals and describe the effective
  disabled state; a JSON regression checks field/provenance agreement.
- Low, Claude, `scan.rs` / `selection.rs`: repeated scan and ancestor validation.
  Retain the collected scan for selection, memoize distinct parent validations
  within a scan, and deduplicate warnings. Existing symlink rejection and ownership
  tests still pass.
- Low, Claude, `components/authority.rs`: explicit future Rust roots regressed.
  Validate existing ancestors without requiring the final directory to exist;
  mark the explicit candidate as a future root. A write/catalog-load regression
  proves compatibility without creating that directory. Selection flags still
  require existing roots.
- Low, Claude, `components/authority.rs`: custom component IDs produced contradictory
  duplicate rows. Match unused discovered candidates by root/ecosystem and adopt
  the stored ID. The new preview regression requires one included candidate.

Open questions resolved: disabling SQLx retains its prerequisite relationship
with schema tooling; unsupported command/native overrides fail before writes.
The initial projection now returns an error for opaque authored components instead
of panicking if it is ever reached without a complete preserved source.

The round-two repair run passed 19 focused tests in 10.463 seconds, including
candidate selection/metadata, custom identities, guide policy, command ownership,
frontends, future roots and the legacy pre-v4 template fixture. The full work-check
preceding these repairs failed its original minimal-to-full case and later found
one more legacy fixture with implicit Rust intent; both are now corrected and
pass focused validation. Those original receipts remain retained. Fresh full gates
and the final literal backend check are still required.

## Final validation progress

The broader bootstrap/guide run `cd903fd4-5028-4689-93ed-3cff1fd724a9`
ran all 634 selected tests without fail-fast: 633 passed, with one migration-
provenance fixture lacking explicit Rust intent. That fixture now declares Rust;
its focused rerun `a4a68e0d-40e2-4e76-81b2-a73e3d14b883` passed. The expanded
frontend test was split into initial-adoption and readoption helpers to satisfy
Clippy's cognitive-complexity limit. The membership-warning assertion now checks
the actual detection_report.warnings field; its rerun
`1c045384-69f6-42ea-8588-d5b6d7e6098a` passed. Workspace/all-target/all-feature
Clippy passed after the split.

Fresh default work-check validation has passed contract, formatting, Clippy,
core tests, frontend tests and vault tests. Core Nextest
`b19c2c3a-471e-4149-8af7-54557922d4b7` passed all 3,117 selected tests;
original receipt `receipt_01M1Y4MB273PB5R69GY5R9XQ9B`. The native file-budget
check passed in `receipt_01M1Y4W5W3164H4RXP6G64BPB6`. The verification profile's
full-workspace test target is still running. Current source fingerprint is
`sha256:25caf2af3b01ebf207b090cf94f35a80179b029d2163d8b7108f0e07727c9aa9`.
Source is held unchanged through the remaining checks. Finish structured work
while evidence is fresh, before Beads closure and Git staging change the recorded
working-tree layout; tracker/state-only completion updates do not change code.


The complete default work-check passed in batch receipt
`receipt_01M1Y4VMP7P7NZ2244HX56HGFY`: six checks executed successfully and one
process-path gate was explicitly not applicable. The verify profile's workspace
run `f216739e-3760-42f8-8820-7f8e079f4e67` passed 3,884 tests with two skips;
original receipt `receipt_01M1Y5MK1HYF9FR672R5RYJVK4`.

The separate final `scripts/jig check test` attempt failed in three unchanged
scheduling regressions (receipt `receipt_01M1Y68VBVTS06GXRC9AVSGP1T`, Nextest
`bf84f924-12cf-4113-b9d0-7d4d272ec0a1`): forged-checkout schedule handling,
unconfirmed PR push, and branch lease loss after start. All three passed unchanged
in the isolated single-threaded run `2bfc48e4-9abe-4290-ac15-7695896778c6`.
This supports the existing contention investigation in `jig-sh-7o6`; it does not
turn the failed full run into a pass. The full literal backend check is now
running with `NEXTEST_TEST_THREADS=2`, keeping all tests and assertions enabled.

Test compilation refreshed the development binary's native build identity, making
the earlier native contract gate stale despite unchanged source. After the final
backend check, force only `jig-contract` to collect its current signature, then
inspect every required gate's freshness before finishing. No further review round
or source change is planned.


## Completion audit

- Human/JSON parity and read-only preview: the real CLI regression
  `adopt_components_cli_human_and_json_preview_agree` checks displayed decision
  lines against JSON, explicit inclusion, and absence of `.jig.toml` writes.
- Deterministic ownership and exclusions: candidate unit regressions assert
  included workspace members, native exclusions, review-required incidental
  Rust/Node/Go manifests, stable IDs after normalized selections, conservative
  unsupported-glob handling, and atomic rejection of unknown/escaping/conflicting
  roots. Symlink traversal is rejected using a tracked-file fixture.
- Actual authoring: `adopt_components_write_selected_roots_and_preserve_them_on_recopy_and_readoption`
  checks exact stored component IDs/roots and resolved component count, loads
  RepositoryCatalog, and verifies recopy/readoption after adding a manifest retain
  the saved model. The incidental-only fixture proves no default backend appears.
- Existing authored ownership and compatibility: transition regressions cover
  opaque components, custom IDs and command/target aliases, frontend exclusion,
  full/minimal capability changes, explicit future roots and pre-write errors.
- Scope: the adoption diff does not change repository planner or affected-path
  code. README, adoption, developer UX and public-contract docs describe the new
  flags and preservation limits. The original prefix of every changed state JSONL
  file remains byte-identical to HEAD; appended records parse successfully.
- Workflow: both permitted comprehensive review rounds are recorded above. All
  actionable findings have code/test repairs; claims not reproduced have explicit
  source reasoning and regression evidence. Required gates passed on the final
  source; literal backend retry and native gate freshness remain to be finalized.


## Final validation result

The final literal backend command passed:

    NEXTEST_TEST_THREADS=2 JIG_DEV_BIN=target/debug/jig scripts/jig check test

All five targets succeeded: Clippy, formatting, backend tests, contract and native
file budget. Nextest `b694d929-ce1b-45a8-b071-a938803d37eb` ran all 3,884 tests:
3,884 passed, two skipped, 1,618.497 seconds. Original test receipt
`receipt_01M1Y7ZB5J4191BRXAA6EPT9SZ`, execution run
`run_01M1Y6DAC6V6XX59HKF20PV0GB`. The process is terminal and the log reports
`Jig check: passed`. Lower concurrency changed scheduling only; no test assertion,
selection or source file changed during this retry.

The explicit native contract gate refresh passed in
`receipt_01M1Y80G8XH4VBQ64YTZD79BAJ` (batch
`receipt_01M1Y80MY9QNS06N32SPY93EES`). Fresh `work gates` and `work evidence`
queries both report `gates_ok: true`, `overall: passed`, with no failed, missing,
stale, unknown or unsupported required evidence. Verify, native contract, format,
Clippy, core, frontend and vault gates are fresh/passed. The process-specific gate
is fresh/not-applicable because its configured path scope is unchanged. `work
receipts` reports the twenty plan-associated receipts, retaining earlier failures.
The exact validated source fingerprint remains
`sha256:25caf2af3b01ebf207b090cf94f35a80179b029d2163d8b7108f0e07727c9aa9`.

This final evidence completes the acceptance audit above. The earlier scheduling
failures remain an open contention investigation in `jig-sh-7o6`; the successful
retry is not a claim to have fixed them. No third review round was performed.


The structured work plan closed successfully before tracker/staging changes, with
closure receipt `receipt_01M1Y82NWYZRWSAQZTVE47H5P9`. Bead `jig-sh-generic-monorepo-zac.1.3`
is closed and its export is synchronized. The existing contention Bead's notes
now preserve this task's original failure and successful unchanged retry evidence.
Only tracker/plan/state bookkeeping changed after final source validation.
