# Typed focused Rust checks with honest evidence

This living ExecPlan follows `.agent/PLANS.md`. It implements V04, `jig-sh-rust-validation-velocity-w0yp.4`, as an authorized prerequisite of `jig-sh-ndz2`. A user will be able to preview and execute a package/library Rust test selection through `work check --phase iteration`, retain normal receipts, and see that a focused pass does not satisfy a full-suite gate.

## Progress

- [x] (2026-09-21) Verified V04 is ready, claimed it, and opened structured work `plan_01M30YCXWYFGKYMDFEWJX8FKDM` against Git baseline `5aec3b0971891194e02202abdd4473bbbe206e67`.
- [x] (2026-09-21) Integrated completed V01 iteration phases and V03 Cargo discovery code without historical state, preserving newer native gates, compact completion, and scoped observations.
- [x] (2026-09-21) Added versioned typed Rust runner, bounded focus normalization, private raw-ID package proof, and literal execution with unsuccessful empty-selection evidence.
- [x] (2026-09-21) Connected CLI/MCP iteration selection, accumulated baseline comparison, and ordinary exact-invocation evidence; focused workflow regression validation remains in progress.
- [x] (2026-09-21) Proved real Cargo build selection and negative full-gate authority; repaired both native cycle-1 findings and passed all six gates with 4,417 tests and four skips.
- [x] (2026-09-21) Repaired all eight findings from native cycles 1–5, passed 4,426 tests (four skips) and all six gates, and confirmed fresh evidence after the test-oracle repair.
- [x] (2026-09-21) Repaired cycle 6's host-dependency finding with conservative automatic target-context fallback; pure and real-Cargo regressions fail before and pass after, with 24 Rust-policy and 11 preparation tests passing.
- [x] (2026-09-21) Corrected the regression's supported-platform fixture, passed all 4,428 tests (four skips) and all five other gates, and confirmed fresh gate/evidence/receipt inspections.
- [x] (2026-09-21) Native Codex cycle 7 reported no actionable regressions and independently passed 125 focused tests; all nine findings are fixed with no recurrence.
- [x] (2026-09-21) Finished structured work and closed the bead after clean review and fresh gates.
- [x] (2026-09-21) Committed task changes and verified a clean checkout; this final metadata update records that result in the task commit.

Restart checkpoint: V04 is complete and committed. Structured work and the bead are closed after clean native cycle 7, 125 reviewer-run focused tests, 4,428 full-suite passes (four skips), and six fresh gates; the checkout was verified clean. Select V06 next. The earlier launcher intermittency remains recorded, with user authorization for `jig-sh-7o6` if it becomes a blocker; this pass is not a hermeticity fix. No review finding recurred.

## Surprises & Discoveries

The bounded full-suite retry found a deterministic V04 fixture mistake: the supported-host inventory scans tracked test strings as well as host implementations. Correct the real-Cargo regression to alternate between the supported Linux and macOS targets, with a dependency specific to the actual host OS. This preserves the same dropped host edge and all scope controls; do not alter the inventory, obfuscate forbidden strings, or weaken the regression. The retry passed 4,003 tests before this failure, with four skips and 424 unrun.

The first cycle-7 full validation failed `replacement_recovery_survives_app_initialization_failure_without_stale_session` at its existing 12-second wait for `initializing`. That file, timeout and dev-proxy implementation have no task diff. A focused replay passed unchanged in 1.34 seconds. This proves intermittency, not host-contention causation; retain both results and do not weaken the oracle. The failed full run executed 3,964 tests (3,963 passed, one failed; four skipped) and left 464 unrun.

A subsequent run of the unchanged launcher-loss integration group passed four tests and failed two at `ready` marker waits, including the replacement test and queued-signal test. The one full-suite retry is active; root-cause diagnosis remains read-only. The user explicitly authorized repair of related test-hermeticity bead `jig-sh-7o6` if needed. No hermeticity implementation has started. If the retry blocks again, preserve V04 recoverably and obtain a clean checkout for that task rather than committing unreviewed V04 code.

Diagnostic qualification: Nextest's evaluated local configuration assigns all six launcher-loss tests to the existing one-thread `process-signals` group. Manual `cargo test` bypasses that protection; its potential application-port collisions do not explain the required gate's earlier initialization timeout. Keep that timeout unresolved rather than treating the parallel group as a faithful reproduction. The configuration-inspection command ran no tests and changed no policy.

Native cycle 6 reproduced a Linux procedural macro's `cfg(unix)` dependency disappearing from metadata filtered for Windows, even though the macro is built for the host. A single filtered graph cannot prove the full affected-consumer set. The existing automatic-scope fallback can preserve correctness without a dual-platform graph redesign.

Native cycle 5 reproduced the cross-plan regression failing with `web` before `api`, followed by a passing rerun. The independent checks have no ordering dependency; the test's hard-coded initial order was invalid. A focused test audit requires order-insensitive exact initial membership, unchanged subsequent logs, and unchanged receipt-provenance assertions. Passing the earlier full suite did not establish reliability.

Native cycle 3 showed that Cargo metadata's `test` flag describes default participation, not explicit target eligibility. It also reproduced a feature-context mismatch after automatic package narrowing and a Cargo.lock write by unlocked metadata during preview. These require owner-layer corrections and real Cargo regression tests, not changes to receipt or executor architecture.

Native cycle 2 reproduced a force-selection mismatch introduced by routing `--explain` through phase planning: a fresh native gate preview reported two reused targets, while its corresponding explicit execution launched both. The existing unphased scheduler correctly forces explicit gates; preview must preserve that policy rather than changing execution semantics.

Native review found directory-only ownership unsound for a declared target source placed under another workspace package. The regression observed `Narrowed` before repair; the repaired real-Cargo fixture executes the affected root assertion for entrypoint and adjacent-module changes while still narrowing an unrelated member. A second regression reproduced loss of repository `.cargo/config.toml` when the component root is `backend`; shared ancestor configuration now triggers the existing broad topology fallback for all applicable components.

V01 and V03 are closed in Beads but their code is on another branch. V01 is commit `fe13c657`; V03 is `350536dc`. Their old plans, journals, and tracker exports are not this checkout's history and must not be copied. The current checkout already contains newer native-gate selection, compact completion, request-local freshness observations, and executable recovery hints that must survive integration.

Cargo metadata package IDs can include absolute file URLs. V03 validates unique portable `name@version` selectors; executable selection must preserve a provable mapping to the original ID in memory without persisting that ID. Cargo metadata's feature and platform context must match the eventual execution.

Focused tests found that Serde's internally tagged unit variants accept extra fields despite the enum's closed-schema annotation. Library and focus-declaration variants now use empty struct variants; the unknown-field regression passes. Another integration test exposed that a map keyed by the structured `TargetId` DTO cannot directly deserialize JSON object keys. Work focus now parses bounded canonical target-string keys explicitly, without changing the historical target DTO.

The existing phase scheduler reconstructed scheduled targets with empty arguments. It now retains exact focus arguments and compares prepared Rust input against the frozen preview. Full gate inspection separately prepares the default broad Rust invocation, so a focused receipt cannot be mistaken for its evidence. Typed Rust v1 deliberately requires whole-repository input authority; narrower input reuse remains outside V04.

## Decision Log

On 2026-09-21, the user authorized `jig-sh-7o6` repair if needed after the bounded retry. Read-only inspection found a possible shared application-port race between independent fixtures, but actual failure stderr was not retained and the earlier initialization-marker failure remains unexplained. Do not claim a root cause or change timeout/scheduling policy based only on this hypothesis.

On 2026-09-21, broaden automatic package impact for every explicit target context, because the normalized graph carries neither host identity nor proof of complete host edges. Reuse the existing `UnsupportedContext` reason and broad workspace disposition; leave explicit user focus and execution target unchanged. This is a conservative repair within the existing fallback contract, not an architectural expansion. Delegate a disjoint real-Cargo regression under the repository plan standard.

On 2026-09-21, repair cycle 5's test oracle without changing executor scheduling, adding dependencies, or masking failures with retries. Preserve the immutable pre-repair tree and audit artifacts outside the repository. Validate both permitted permutations and reject missing/duplicate/new launches, then repeat actual phase tests and required validation. This distinct test reliability defect is not recurrence of any repaired production finding.

On 2026-09-21, native cycle 4 exposed an integration mismatch between imported phase snapshots and the current cross-plan gate evaluator. Repair the selected-invocation snapshot at the existing eligibility/index/proof layers instead of introducing another receipt policy. Delegate a disjoint two-plan regression. This is a distinct ordinary-reuse defect, not recurrence of explicit-gate force selection; no deeper redesign is required.

On 2026-09-21, repair the three distinct cycle-3 findings within metadata discovery and Rust scope preparation. Keep execution policy separate from read-only discovery, broaden automatic scope when feature preservation cannot be proved, and validate explicit target existence/kind without its default test flag. Delegate independent metadata repair and explicit-target regression work in disjoint files under the repository plan standard. No prior finding recurred; continue the user's validation/review loop.

On 2026-09-21, retain existing explicit gate force semantics and pass an empty passing set to the shared scheduler when previewing explicit gates. This is a policy handoff repair, not a new execution mode. Strengthen the existing native-gate preview regression to start with passing evidence, contrast default reuse, preserve no-write preview, and observe forced execution. Neither earlier disposed finding recurred, so the new finding does not meet the user's non-convergence condition.

On 2026-09-21, native review reproduced two conservative-selection gaps inherited from V03. Fix these at the ownership layers: declared Cargo target source directories must participate in package ownership, broadening on conflict, and applicable ancestor Rust configuration must reach component impact selection. Adjacent modules need the same protection as the declared entrypoint; no Rust parser or architectural redesign is needed. Regression tests must distinguish the faulty narrowed result before fixes. The independent ancestor-configuration fix and real-Cargo regression are delegated in disjoint files under the repository plan standard.

On 2026-09-21, the main agent chose a new explicitly versioned `rust_nextest_v1` runner capability, rather than inspecting authored shell commands. An older runtime rejects an unknown runner variant before execution; existing command and string-argument semantics remain unchanged. Do not activate reserved contract epochs 9/10 or unrelated work-link epoch 11.

On 2026-09-21, the main agent chose a typed `RustFocusV1` value with explicit and automatic modes. Existing action argument maps remain string-valued for compatibility: the new `rust_focus_v1` argument declaration parses and canonicalizes a bounded JSON value before planning. This is a structured value, never an argument fragment. CLI and MCP share that normalization. Full runners forbid focus arguments; focused runners explicitly declare this capability. Prepared scope and literal argv are part of the immutable planned target and freshness authority.

On 2026-09-21, prerequisite code integration was delegated in disjoint scopes as required by the repository plan standard. The main agent retains responsibility for integration, tests, and native Codex review. No historical journals are imported.

On 2026-09-21, automatic focus on a custom runner was defined as an explicit `configured_default` fallback; explicit unsupported focus remains an error. The default Nextest profile is pinned in argv so ambient `NEXTEST_PROFILE` cannot silently change selected test scope. No new runner alias is projected through legacy tools: typed execution is the sole preparation owner.

## Outcomes & Retrospective

Native cycle 7 found no actionable regressions and passed 125 focused tests. V04's nine review findings are fixed at their ownership layers, with no recurrence or design escalation. Typed explicit/automatic Rust focus, conservative Cargo impact, iteration/final separation, and exact ordinary evidence are implemented and validated. Full current validation is 4,428 passing tests with four skips and six fresh required targets. No benchmark budget was spent on these reviews; no launcher-hermeticity fix is claimed. Administrative closure and commit are next.

Update after native cycle 6: all nine findings are repaired. Fresh validation passed 4,428 tests (four skips) and all five other gates; the supported-host fixture error is corrected without policy changes. Test receipt `receipt_01M31ESXPZ3SQACPHBRMW5FFFG` and validation receipt `receipt_01M31ETJZHMP1AP16H2S2NK2V5` are current. Native cycle 7 and closure remain pending. The following older full-suite results remain historical context.

Focused acceptance and required full checks are complete for all eight repairs from native review cycles 1–5; native cycle 6 and closure remain pending. Observed: 45 contract/Rust unit tests, ten real Cargo/preparation tests, 21 Cargo discovery tests, 35 phase/cross-plan regressions, eight human-output tests, and 26 native-gate/compact/scoped-observation compatibility regressions passed. The same target's default/full gate remains stale after a focused pass, closure rejects it, and forced full execution records the intentionally excluded integration compilation failure. Prepared argv/context/package/schema tampering and source changes reject replay. Missing baseline/metadata produces explicit broad automatic fallback but cannot authorize explicit focus. Eligible cross-plan phase reuse preserves original provenance without new target receipts; newer failures and plan-local exceptions remain authoritative. After the oracle repair, two fresh-process phase replays passed 26 tests each, and staged validation passed all 4,426 tests (four skips), strict Clippy, formatting, contract, file-budget, and current-source runtime checks. Evidence inspection confirms six fresh passes. Existing T01–T03 and T05–T06 work is committed. V06 Cargo resource coordination and T04 measured admission remain subsequent tasks.

## Context and Orientation

`crates/jig-contract/src/repository.rs` defines authored actions and runners. `run.rs` defines an immutable `RunPlan` and its `PlannedTarget` entries. An invocation means one exact target, runner, arguments, and prepared scope. `crates/jig/src/repository/arguments.rs` validates supplied arguments; `planner.rs` resolves a request, prepares authority, and re-derives it before execution. `repository/freshness/authority.rs` hashes exact invocation authority. `runtime/run_execution.rs` and its `target` module own supervised execution, deadlines, output budgets, cancellation, and normal receipt recording.

The imported V01 `runtime/work/checks/phase.rs` selects an iteration profile separately from final requirements. A selected pass is not permission to close work. Imported V03 `repository/cargo_discovery.rs` acquires bounded metadata, while `crates/jig-rust/src/cargo/` normalizes package graphs and computes conservative affected-package scope. Automatic impact must use the work plan's recorded baseline and all current changes, including changes in earlier commits; it must not compare only the newest commit.

## Plan of Work and Milestones

### Milestone 1: integrate prerequisites

Apply relevant code deltas from V01 and V03 using `apply_patch`; merge newer behavior in overlapping files rather than replacing them wholesale. Do not import V02 refinement, V05 narrow reuse, old `.agent` records, or old Beads exports. Integrate configuration and public-contract documentation manually. Compile the affected crates and run V01 phase and V03 discovery tests. Preserve no-phase compact `agent-v1` output. Until a versioned phase projection exists, reject an explicit phase combined with that projection before execution instead of omitting selected-scope information.

### Milestone 2: prepare a typed Rust invocation

Add shared versioned DTOs in a new `crates/jig-contract/src/rust_focus.rs`. A runner configuration records repository-relative workspace manifest, Cargo profile and optional target platform, explicit features/default-feature policy, locked/offline policy, and optional Nextest profile. A focused request carries explicit package selectors and target kind/name, optional feature override and explicit Nextest filter, or automatic package impact. Bound counts and string sizes; reject NULs, option-like identities, conflicting feature modes, invalid target combinations, and undeclared argument inputs before spawning.

Keep process-free normalization and argv construction in `jig-rust`. Extend bounded metadata acquisition to carry matching context. Validate portable selectors against the complete normalized package graph and original raw identity. For explicit selection, unavailable proof is an error with full-check recovery. Automatic selection broadens to the declared workspace scope when comparison or ownership is missing; record the reason. It must never infer test names. Lower selection to literal Cargo Nextest arguments, including package and target switches; selecting a library must use `--lib` and must not include `--workspace`.

Attach prepared Rust input to `PlannedTarget`, including exact scope, context, comparison identity, and argv. The planner re-derives this value before execution; supplied plans cannot authorize arbitrary argv. Include it in invocation identity. Execute through the existing literal owned-process boundary. Treat Nextest's no-tests exit distinctly as `empty_selection`, with unsuccessful behavioral evidence. V1 does not permit empty selection as success.

### Milestone 3: connect iteration and authority

Add typed `rust_focus` to the work request and `--rust-focus TARGET=JSON` to CLI parsing. Normalize both through one shared helper. Bind the current work-plan ID for automatic requests and reject focus outside iteration. The selected invocation preview and actual run must agree. Focused receipts use normal journal, source guards, output limits, cancellation, and failure paths. Final gate evaluation continues to derive its own fixed invocation and must reject a focused receipt as insufficient.

### Milestone 4: prove behavior and complete

Add unit tests for closed schemas and canonical literal argv. Add user-facing CLI/MCP and persisted-evidence tests for explicit scope, changing filters/features, preview parity, automatic accumulated changes, broad fallback, empty selection, and pending final requirements. A real dependency-free Cargo workspace fixture contains a passing library plus a sibling integration test that cannot compile; selecting the library must pass without building that sibling, and broad scope must fail. Command stubs alone cannot prove this acceptance. Test cancellation, output overflow, nonzero status, and source changes using ordinary existing execution seams, with no private fixture names.

## Concrete Steps

Run commands at the repository root. During development use focused `cargo test -p jig-rust`, `cargo test -p jig-contract`, and selected `cargo test -p jig-sh` filters for changed behavior. Build the current runtime with `cargo build -p jig-sh --bin jig` or `scripts/jig-dev`; routine work commands use the pinned released runtime.

After final source changes, format, stage the complete task changes, and run:

    scripts/jig check test --plan-id plan_01M30YCXWYFGKYMDFEWJX8FKDM
    scripts/jig check api:clippy api:fmt repo:contract repo:file-budget repo:source-runtime-check --plan-id plan_01M30YCXWYFGKYMDFEWJX8FKDM
    scripts/jig work check --plan-id plan_01M30YCXWYFGKYMDFEWJX8FKDM
    scripts/jig work gates --plan-id plan_01M30YCXWYFGKYMDFEWJX8FKDM --freshness-timeout-ms 30000
    scripts/jig work evidence --plan-id plan_01M30YCXWYFGKYMDFEWJX8FKDM --freshness-timeout-ms 30000
    scripts/jig work receipts --plan-id plan_01M30YCXWYFGKYMDFEWJX8FKDM

Expected for the latest repair, not yet observed: applicable gates pass with current evidence; the source-runtime gate builds and checks edited code. Stage new journal entries and run native `codex review --uncommitted` across all changes, not a review controller. Record every finding and disposition in the workflow's outside-repository temporary ledger. Genuine fixes repeat validation and review. On clean review, finish structured work before changing Beads metadata, close the bead, run `python3 scripts/beads-sync.py`, commit, and verify `git status --short` is empty.

## Validation and Acceptance

Acceptance requires one real Cargo execution showing package/library build selection without the sibling integration binary, exact preview/execution agreement, a distinct unsuccessful zero-match outcome, literal or rejected metacharacters, changing filter/features changing authority, and a negative full-gate satisfaction assertion after a focused pass. Automatic focus must include accumulated baseline changes and safely broaden on missing authority. CLI/MCP must describe equivalent selected scope and final gaps. Source/config changes after planning or during execution must prevent a current pass; cancellation/output overflow/nonzero results must leave work open.

Do not rerun `scripts/fixtures/workflow-velocity.py`: its live matrix budget is exhausted. Do not rerun `scripts/benchmark-work-inspection.py` as part of these gates: it has a separate limited measurement budget. No V04 performance claim requires either benchmark.

## Idempotence and Recovery

Old records remain readable and original receipt IDs are never rewritten. New runner and argument tags fail closed in unsupported runtimes; adoption is opt-in after upgrading the runtime and manifest together. Reverting an authored focused action to an existing full shell/argv action restores the old workflow without translating its command. A failed focused attempt does not close work; users may run the configured full check. Preserve failed receipts and retry only after correcting the cause. Do not import journals to repair integration. If review reveals deeper design misalignment, or a disposed finding recurs, stop under the user's workflow instead of redesigning without bounds.

## Interfaces and Dependencies

Use existing serde/schemars DTO tooling, `jig-rust` pure policy, bounded owned Cargo discovery, `PlanRunRequest`, `PlannedTarget`, and ordinary target receipts. Add `RustFocusV1`, versioned runner configuration and prepared-input types; normalize the focus argument once and persist only portable identities. Creation (planning), explain, execution revalidation, success/failure/cancellation recording, reuse, and final gate collection all use the same invocation identity. Final work closure rechecks required gates independently. No daemon, shell parser, PTY service, new test runner, or persistent completion cache belongs in this task.

Revision note (2026-09-21): initial task-local plan written after inspecting the current planner, runner, freshness authority, and completed prerequisite implementations.

Revision note (2026-09-21): recorded integrated implementation, exact invocation handoff fixes, schema/transport test discoveries, compatibility decisions, and observed focused checks. Full validation/review remains pending.

Revision note (2026-09-21): staged full validation passed: 4,412 tests (four skips), strict Clippy, formatting, contract, file-budget, and current-source runtime check. Native review and task closure remain pending; no completion is claimed yet.

Revision note (2026-09-21): native review cycle 1 found two P2 scope-selection bugs. Repairs and negative regressions are in progress; initial validation does not prove the repaired state, and full checks and native review must repeat before completion.

Revision note (2026-09-21): both native findings repaired at their ownership layers, with failing-before/passing-after regression evidence. Full repaired checks and native cycle 2 remain mandatory; no architectural escalation or recurrence has occurred.

Revision note (2026-09-21): native cycle 2 independently passed 75 focused tests, found a new forced-preview policy mismatch, and did not repeat either prior finding. The small scheduler handoff repair and strengthened real-launch regression are complete; cycle-3 validation and review are mandatory. The previous 4,417-test pass is historical, not proof of this latest repair.

Revision note (2026-09-21): the repaired staged state passed all 4,417 tests with four skips and all five other gates. Fresh evidence was inspected; native cycle 3 remains the next required step, with no source changes during review.

Revision note (2026-09-21): native cycle 3 found three distinct Cargo policy defects; earlier findings did not recur. Discovery now always locks metadata without changing execution policy, automatic features broaden conservatively when needed, and explicit targets ignore default test participation. Real regressions failed before and passed after repairs. Full validation and native cycle 4 remain required.

Revision note (2026-09-21): repaired staged validation passed 4,423 tests (four skips), Clippy, formatting, contract, file-budget, and source-runtime checks. Gate/evidence inspection confirmed freshness; native cycle 4 is next.

Revision note (2026-09-21): cycle 4's distinct cross-plan selection defect is repaired using the shared receipt index. All 35 phase/cross-plan tests pass, including original-provenance reuse, newer-failure rejection, and legacy isolation. Full staged validation and native cycle 5 remain mandatory.

Revision note (2026-09-21): fifth staged validation passed 4,426 tests (four skips), strict Clippy, formatting, contract, file-budget, and current-source runtime checks. All evidence is fresh; test receipt `receipt_01M318HMP1RPG9G9SYCC362YKY` and validation receipt `receipt_01M318J82DMNYMHF0DTHABSAWX`. Native cycle 5 is next.

Revision note (2026-09-21): native cycle 5 found a scheduler-order assumption in two new tests. The repaired oracle preserves exact launch counts and no-new-launch evidence while allowing either valid initial order. Focused replay and all required checks precede native cycle 6; the earlier suite pass is not reliability proof.

Revision note (2026-09-21): sixth staged validation passed 4,426 tests (four skips) and all five other gates. Fresh test receipt `receipt_01M31AB5T8NWQAQT82H17DM0N7`; work validation receipt `receipt_01M31AC8A4B6DP6STBM6FZ1MN8`. Native cycle 6 is next; no completion is claimed yet.
