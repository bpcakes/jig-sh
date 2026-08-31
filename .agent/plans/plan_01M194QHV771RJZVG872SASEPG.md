# Refactor preset capabilities and scaffold project planning

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current while the work proceeds. Maintain this document in accordance with `.agent/PLANS.md`.

The structured work identifier is `plan_01M194QHV771RJZVG872SASEPG`. The owning Beads task is `jig-sh-rust-only-init-presets-zc7.1.1` (B01). The exact Git baseline is commit `13d33adacda39e0f5c2d5317016845cfd856b7d0`.

## Purpose / Big Picture

This is a behavior-preserving foundation change for `jig init`. Today several wizard, validation, and package-manager branches independently treat `rust-react` and `go-react` as the presets that require databases and frontends, while `InitScaffoldPlan` stores a mandatory backend and web-only fields directly on its top-level structure. After this change, those decisions come from one compile-time capability description, and scaffold execution dispatches through a typed project enum with only the existing Rust React and Go React variants. Users continue to see exactly the same three preset values, prompts, errors, generated files, reports, answers, and next steps.

The result is observable by running the existing init wizard and scaffold tests for Rust React, Go React, and harness-only. Those tests must remain green, while new focused tests prove the capability matrix and the two project-plan branches. No Rust-only artifact is constructed in this task; B02 owns the first such variant.

## Progress

- [x] (2026-08-30 10:53Z) Claimed B01 in both Beads stores and opened structured work `plan_01M194QHV771RJZVG872SASEPG`.
- [x] (2026-08-30 10:53Z) Built `target/debug/jig` from the exact baseline and inspected preset, wizard, scaffold-plan, report, and test entrypoints.
- [x] (2026-08-30 11:06Z) Added and tested the centralized `ScaffoldPreset` capability description without adding enum values or changing descriptors.
- [x] (2026-08-30 11:06Z) Introduced the two-variant `ScaffoldProjectPlan` boundary and moved backend/web-only state under its owning variants.
- [x] (2026-08-30 11:36Z) Ran focused suites, the complete `jig-sh` test suite, formatting, and Clippy; fixed the single Clippy-only nested-`if` warning and revalidated it.
- [x] (2026-08-30 11:40Z) Rebuilt `target/debug/jig` and passed all configured structured-work gates: six executed successfully and two were explicitly not applicable by path policy.
- [x] (2026-08-30 11:49Z) Completed comprehensive-review pass 1 at verified fingerprint `15fe0394fba0193ccea249887b2170f2a74b2a1f5b34dec3fe8e6c2b1bc977f9`; Codex was clean and Claude reported five low-severity extension/test concerns for investigation.
- [x] (2026-08-30 12:01Z) Completed comprehensive-review pass 2 at verified fingerprint `a08a0fedcf5e11baf5ae8238f8071a0a1700ccd527cab32e6f54667e2b1c4a31`; Codex was clean and Claude reported four low-severity extension/test concerns for the final fix pass.
- [x] (2026-08-30 12:12Z) Completed comprehensive-review pass 3 at verified fingerprint `7ce4b0fe06816557c3e8fcdeaddd4494c9f804b86286ab0d2530a6540fb30895`; Codex was clean and Claude found no current correctness regression, only successor-task observations with no unresolved input decision.
- [x] (2026-08-30 12:13Z) Ran the requested maximum of three comprehensive working-tree review/fix passes, investigated every open question, and found none requiring user input.
- [x] (2026-08-30 12:15Z) Recorded final evidence, finished structured work, closed B01 in canonical and branch-local Beads state, synchronized both exports, and verified B02 is ready for the final commit handoff.

## Surprises & Discoveries

- Observation: structured work creates `.agent/plans/<plan-id>.md`; this file is the required task-local ExecPlan and should be maintained instead of creating a second plan file.
  Evidence: `scripts/jig work start --print-plan-id` returned `plan_01M194QHV771RJZVG872SASEPG` and created this path with the supplied body.

- Observation: capability duplication is narrowly concentrated in `crates/jig/src/cli/init_wizard.rs` and the minimal-harness invariant in `crates/jig/src/bootstrap_parts/part_02.rs`; preset-specific rendering branches elsewhere compare exact identities for real output differences rather than restating general requirements.
  Evidence: `rg -n 'RustReact \| ScaffoldPreset::GoReact|ScaffoldPreset::RustReact \| ScaffoldPreset::GoReact' crates/jig/src` finds four wizard decisions and one minimal-harness decision.

- Observation: at the baseline, all current scaffold variants had frontends, but the top-level plan also owned package manager, DNS label, frontend list, and frontend notices. Leaving those fields there would force B02's non-web plan to carry fake values.
  Evidence: baseline `crates/jig/src/bootstrap/scaffold.rs` defined those fields directly on `InitScaffoldPlan` and dispatched through a mandatory `ScaffoldBackendPlan`; the implementation now nests them in `ReactScaffoldPlan` beneath the two project variants.

- Observation: the initial `cargo test -p jig-sh cli::init_wizard_tests` filter selected zero tests because the nested test module's compiled path is `cli::init_wizard::tests`.
  Evidence: `cargo test -p jig-sh -- --list | rg init_wizard` showed the exact paths. Rerunning `cargo test -p jig-sh 'cli::init_wizard::tests::'` executed 32 tests successfully.

- Observation: the focused behavior suites pass after the typed refactor.
  Evidence: the capability matrix and project-variant tests each passed; the wizard suite passed 32/32, scaffold runtime passed 32/32, and scaffold generation passed 29 with its existing network-dependent coverage test ignored.

- Observation: the exhaustive crate run is substantially slower than the focused suites because it exercises generated package-manager, vault, subprocess, and agent-supervision workflows, but it completed without failures.
  Evidence: `cargo test -p jig-sh` completed with 2051 library tests passing and 2 existing tests ignored, followed by every integration and doc-test target passing.

- Observation: Clippy required one nested preset/frontends defaulting condition to be expressed as a single conjunction after the capability refactor.
  Evidence: the first `cargo clippy -p jig-sh --all-targets -- -D warnings` reported only `clippy::collapsible-if` at `cli/init_wizard.rs:125`; the condition was collapsed without changing its predicates, the 32 wizard tests passed again, and the same Clippy command then passed.

- Observation: the repository's configured path-aware gate batch covers both the core Rust surface and generated frontend behavior affected by the scaffold-plan change.
  Evidence: batch receipt `receipt_01M196PA334GJW1K43QT4NNC9C` passed contract, Rust LOC, formatting, Clippy, 2333 core tests, and 107 frontend tests; vault and process gates were recorded not applicable, and `work gates`/`work evidence` reported no unresolved gates.

- Observation: pass 1's open questions are settled by the checked-in successor-task contracts and do not require product input. B02 explicitly requires its first non-backend plan to carry no dev-app state, while B03 owns the frozen answer handoff and exact Rust-only input rejections before vault/publication.
  Evidence: B02 (`jig-sh-rust-only-init-presets-zc7.1.2`) requires construction without frontend, database, package-manager, DNS, or dev-app state; B03 (`jig-sh-rust-only-init-presets-zc7.2.1`) owns explicit `rust-library` answer derivation/validation and requires every rejection to name the preset and offending input.

- Observation: the accepted pass-1 fixes preserve all current behavior and make the next database-free preset take the safe SQLx default through the capability table.
  Evidence: the capability, three-case Go-module error, 32 wizard, 30 scaffold-generation, and 32 scaffold-runtime tests passed; Clippy with warnings denied and `git diff --check` also passed.

- Observation: structured-work evidence was refreshed after the review fixes rather than reusing the pre-fix receipt.
  Evidence: batch receipt `receipt_01M197DQ4JV0A659T2EZ4SVT1C` passed all six applicable checks, including 2333 core and 108 frontend tests; both path-excluded gates were freshly recorded not applicable and no gate is unresolved.

- Observation: pass 2's questions also do not require user input. The misleading minimal+harness-only label predates B01 and changing it would violate this task's byte-compatible error requirement; optional-frontends/package-manager semantics are not present in B01–B05 and should be decided only with a consuming preset.
  Evidence: the baseline uses the same Rust React fallback for minimal footprint plus no/other preset, B01 explicitly requires behavior preservation, and B03/B04 declare Rust-only frontends unsupported rather than optional.

- Observation: the final review fixes remain behavior-preserving while strengthening the next variant's compiler obligations.
  Evidence: exact capability/label and Go-module tests passed; the wizard suite now passes 33/33 including explicit Go React package-manager preflight; scaffold runtime passes 32/32; formatting, Clippy with warnings denied, and `git diff --check` pass.

- Observation: final pre-review structured evidence is fresh for the complete corrected source set.
  Evidence: batch receipt `receipt_01M197Z9J8DWV4FEMK1N18MC1F` passed all six applicable gates, including 2334 core and 108 frontend tests; the two excluded gates are freshly not applicable and no gate is unresolved.

- Observation: the third and final review found no current behavior, correctness, security, concurrency, performance, or data-loss regression. Its remaining low observations concern intentionally unconsumed capability states and the first non-React variant.
  Evidence: both reviewers matched verified fingerprint `7ce4b0fe06816557c3e8fcdeaddd4494c9f804b86286ab0d2530a6540fb30895`; Codex reported no findings/questions/gaps, and Claude explicitly verified predicate, error, answer-order, and render-context equivalence for all current presets.

## Decision Log

- Decision: use one private `ScaffoldPresetCapabilities` value returned by an exhaustive `ScaffoldPreset::capabilities()` match, with small semantic accessors for callers.
  Rationale: a single match is compile-time checked when a preset is added, while named accessors keep callers readable and prevent them from rebuilding enum unions.
  Date/Author: 2026-08-30, Codex.

- Decision: distinguish requirements from exact preset-specific behavior. The wizard and package-manager preflight use capability accessors; Go-only validation and renderer branches may still match `GoReact` because they select genuinely different Go output rather than infer a general capability.
  Rationale: converting every identity check into a boolean would erase useful types and broaden B01 beyond behavior-preserving taxonomy cleanup.
  Date/Author: 2026-08-30, Codex.

- Decision: replace the mandatory backend field with `ScaffoldProjectPlan::{RustReact, GoReact}`. Each variant owns its backend details and a shared `ReactScaffoldPlan` containing package manager, DNS label, frontends, and frontend notices. `InitScaffoldPlan` retains only naming, branch, and CI data that B02 can reuse without fake web state.
  Rationale: the enum makes current dispatch exhaustive, prevents a Rust/Go identity mismatch, and lets B02 add a non-web variant without moving the existing common fields or inventing placeholder data.
  Date/Author: 2026-08-30, Codex.

- Decision: preserve all public enum values and descriptors byte-for-byte and add no dormant renderer hint, template, persisted answer, contract type, or generated output.
  Rationale: B01 is a refactor whose rollback and compatibility oracle is the current product behavior; B02 owns the first consuming Rust-only feature.
  Date/Author: 2026-08-30, Codex.

- Decision: accept pass 1's explicit harness-only guard, exhaustive minimal-scaffold display label, independent support/requirement representation, panic-free wizard binding, and Go-module validation-oracle recommendations. Do not move dev-app/bootstrap defaults out of the React-bearing branch in B01.
  Rationale: the accepted items remove misleading future behavior while preserving all current errors. The proposed answer-default move conflicts with B02's explicit no-dev-app boundary and would prematurely decide B03's answer policy; the existing runtime suite already checks the moved Go component-root/bootstrap and Rust admin defaults.
  Date/Author: 2026-08-30, Codex.

- Decision: refine the pass-1 answer-default decision after pass 2: keep dev apps React-gated, but move `bootstrap_command` to an exhaustive project match so B02 must intentionally define the non-backend command. Remove the aggregate completeness shortcut, enforce `required => supported`, and assert exact project labels.
  Rationale: these changes preserve current bytes while strengthening B01's explicit acceptance requirement that common answer helpers dispatch exhaustively; they do not assign dev-app state to B02's non-backend plan.
  Date/Author: 2026-08-30, Codex.

- Decision: defer the final review's optional-frontends semantics, non-React `application_contracts_enabled = Some(false)` choice, and the first `ScaffoldProjectPlan::react() == None` oracles to their first consuming tasks; do not add a placeholder optional preset or non-React variant in B01.
  Rationale: B01 forbids a test-only/non-backend placeholder; B02 owns the first private non-React construction, B03/B04 own exact Rust-only answer policy, and the current three presets have no optional-frontends state. Existing branches are fully validated and behavior-compatible.
  Date/Author: 2026-08-30, Codex.

## Outcomes & Retrospective

B01 now has one exhaustive preset-capability table and semantic accessors for project, database, frontend, Go-module, package-manager, and minimal-footprint decisions. The repeated Rust React/Go React unions are gone from wizard and validation code, while current public preset values, descriptors, prompts, errors, and output remain compatible.

`InitScaffoldPlan` now owns a typed `ScaffoldProjectPlan::{RustReact, GoReact}` boundary. Backend and React-only state live beneath their owning variants; common naming, branch, and CI state remains reusable. Answer derivation, rendering, output paths, summaries, and reports dispatch through typed project helpers, and `bootstrap_command` is exhaustive so B02 must intentionally define it when adding the first non-backend variant. No public preset, placeholder variant, renderer hint, template, contract field, or generated artifact was added.

Validation included the full `cargo test -p jig-sh` run (2051 library tests passed, 2 existing tests ignored, all integration/doc targets passed), focused capability/wizard/scaffold suites, strict Clippy, formatting, and three fresh structured gate batches. The final batch `receipt_01M197Z9J8DWV4FEMK1N18MC1F` passed all applicable gates with 2334 core and 108 frontend tests. Three independent Claude+Codex review passes were run on verified complete fingerprints; pass-1 and pass-2 low findings were resolved where B01 owned them, and pass 3 found no current regression. Remaining observations are explicitly owned by B02/B03/B04 and require no user decision.

## Context and Orientation

`ScaffoldPreset` is the public Clap enum in `crates/jig/src/bootstrap_parts/part_01.rs`. Its product metadata and helper methods live in `crates/jig/src/bootstrap/presets.rs`. `ScaffoldOpts` validation and answer defaults are implemented in `crates/jig/src/bootstrap_parts/part_02.rs`. The interactive, defaults, strict/no-terminal, and web package-manager decisions are in `crates/jig/src/cli/init_wizard.rs`.

`crates/jig/src/bootstrap/scaffold.rs` converts the selected preset and merged `AnswerOpts` into `InitScaffoldPlan`. A scaffold plan is an in-memory description of files to render; it is not serialized into `.jig.toml`. The renderer modules below `crates/jig/src/bootstrap/scaffold/` read that plan. `crates/jig/src/bootstrap/scaffold/write.rs` turns the plan identity and write classification into the JSON scaffold report. `crates/jig/src/bootstrap/init.rs` applies derived answers, preflights all output paths, renders the Jig harness, renders project files, and refreshes the agent map.

The current public values are exactly `RustReact`, `GoReact`, and `HarnessOnly`. Rust React and Go React both require a database choice and at least one frontend in strict mode, both need a web package manager, and only Go React requires a Go module. Harness-only creates no scaffold plan. “Capability” in this plan means one of these compile-time product facts; it does not mean a runtime map or configuration extension.

## Plan of Work

First, edit `crates/jig/src/bootstrap/presets.rs` to add the private capability value and accessors. The accessors must cover whether a preset has a project scaffold, supports and requires database/frontends, requires a Go module, and is already complete without project-shape options. Keep `generated_backend_language`, reserved backend names and roots, public names, descriptor order, and descriptor bytes unchanged. Add a focused matrix test over all three public variants so a future enum addition must declare every capability intentionally.

Then replace the repeated Rust React/Go React unions in `crates/jig/src/cli/init_wizard.rs` and `crates/jig/src/bootstrap_parts/part_02.rs`. Defaults, strict validation, guided prompts, Go-module prompting, minimal-harness rejection, and package-manager preflight must use the typed methods. Preserve existing error strings and prompt order. Exact Go-only restrictions remain explicit identity branches.

Next, edit `crates/jig/src/bootstrap/scaffold.rs`. Add `ScaffoldProjectPlan`, `RustReactScaffoldPlan`, `GoReactScaffoldPlan`, and `ReactScaffoldPlan`. Move Rust/Go backend plans and every web-only field into the project variant. Make `InitScaffoldPlan::from_opts`, answer derivation, summaries, output paths, rendering, database/frontend integration predicates, and report accessors dispatch exhaustively through `ScaffoldProjectPlan`. Use helpers that return optional React state only where a future non-web branch can return `None`; do not add a placeholder or Rust-only branch. Update `rust_workspace.rs`, `go_workspace.rs`, and `write.rs` to use typed plan methods rather than top-level web/backend fields.

Finally, add or tighten focused tests. The capability matrix must be exact. Existing Rust React and Go React scaffold tests must prove report identity, answer defaults, files, and next steps remain unchanged. Existing harness-only tests must prove no scaffold plan or starter output. Run formatting, focused tests, the full `jig-sh` crate tests, Clippy, and the configured Jig gates. Build the development binary again before invoking `scripts/jig` for final evidence.

After implementation validation, run the comprehensive-review workflow on working-tree changes with its default independent Claude and Codex reviewers. Resolve actionable findings because the user explicitly requested implementation plus review/fix loops. Recompute the review fingerprint on every pass. Stop immediately and ask the user only if a finding exposes a product or compatibility choice not resolved by B01; otherwise fix and repeat until clean or three passes have run.

## Concrete Steps

Work from `/home/aa/.herdr/worktrees/jig-sh/worktree-silver-harbor-4827`.

Inspect and implement with:

    rg -n 'ScaffoldPreset|InitScaffoldPlan|RustReact \| ScaffoldPreset::GoReact' crates/jig/src
    cargo fmt --all -- --check
    cargo test -p jig-sh bootstrap::presets
    cargo test -p jig-sh 'cli::init_wizard::tests::'
    cargo test -p jig-sh bootstrap::tests::basic::scaffold

Run crate and configured validation with:

    cargo test -p jig-sh
    cargo clippy -p jig-sh --all-targets -- -D warnings
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M194QHV771RJZVG872SASEPG
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M194QHV771RJZVG872SASEPG
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M194QHV771RJZVG872SASEPG

The exact focused test filters may be adjusted after listing the compiled test names; record any adjustment in `Surprises & Discoveries` and keep the crate-level test as the broad oracle.

For each comprehensive review pass, use working-tree scope because implementation and structured-work files are intentionally uncommitted. Follow `/home/aa/.agents/skills/comprehensive-review/SKILL.md` and its runtime reference exactly, preserve the scope fingerprint, and record the merged findings or clean result here before editing. The maximum is three passes.

At successful completion:

    JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01M194QHV771RJZVG872SASEPG --resolution 'B01 capability and scaffold-plan refactor is behavior-preserving and all required review and validation gates pass.' --outcome success
    br close jig-sh-rust-only-init-presets-zc7.1.1 --reason 'Completed B01 behavior-preserving capability and scaffold-plan refactor.' --json
    br sync --flush-only

Synchronize the worktree-local Beads database as well, verify B02 becomes the sole ready delivery task, review the final staged diff, and commit once at the end as requested.

## Validation and Acceptance

Acceptance requires that `ScaffoldPreset::value_variants()` still returns only `rust-react`, `go-react`, and `harness-only` in the same order and that `jig presets` output remains unchanged. The wizard must retain numeric choices 1–3, default choice 1, existing strict errors, and the same database/frontend/Go prompts. Package-manager preflight must still run for both React presets and skip harness-only.

Rust React and Go React init fixtures must produce the same generated paths, report preset/database/frontend fields, derived answers, backend dev apps, and next steps as the baseline. Harness-only must continue to produce no `InitScaffoldPlan`, no scaffold report, and no starter Cargo, Go, or frontend file. There must be no new public preset, test-only project variant, template, snapshot, render hint, persistent answer, contract field, adapter, runner, or generated file.

Formatting, `cargo test -p jig-sh`, Clippy with warnings denied, structured work gates, and up to three comprehensive review passes must complete. A clean comprehensive pass ends the loop early. Any unresolved open question that changes observable product behavior requires user input and stops the task before commit.

## Idempotence and Recovery

Source edits and tests are safe to repeat. `cargo fmt`, builds, and tests are idempotent. Structured state is append-only; never delete or rewrite `.agent/state/*.jsonl` records. If a test exposes behavior drift, restore the typed dispatch to reproduce the previous branch semantics rather than weakening the test. If a review fingerprint changes while reviewers run, discard that pass, wait for all reviewers to stop, capture a new fingerprint, and rerun without counting the drifted reports as a completed review.

If final validation fails, leave B01 in progress, update this ExecPlan with the exact failure and remaining work, and do not close or commit. If user input is needed, record the question and evidence here before stopping.

## Artifacts and Notes

The baseline build completed successfully before production edits:

    Finished `dev` profile [unoptimized + debuginfo] target(s)
    plan_01M194QHV771RJZVG872SASEPG

The baseline source search found five repeated two-preset capability unions: four in `crates/jig/src/cli/init_wizard.rs` and one in `crates/jig/src/bootstrap_parts/part_02.rs`. At that baseline, `InitScaffoldPlan` had a mandatory `ScaffoldBackendPlan` and direct `repo_dns_label`, `package_manager`, `frontends`, and `custom_frontend_notices` fields; the implementation now replaces that shape with the typed project variants described below.

## Interfaces and Dependencies

No new dependency is allowed. In `crates/jig/src/bootstrap/presets.rs`, define a crate-private capability representation and semantic `const fn` accessors on `ScaffoldPreset`. Callers must not access capability fields directly or reconstruct preset unions.

In `crates/jig/src/bootstrap/scaffold.rs`, the final structural boundary must be equivalent to:

    struct InitScaffoldPlan {
        project: ScaffoldProjectPlan,
        requested_repo_name: String,
        repo_name: String,
        package_name: String,
        module_name: String,
        default_branch: String,
        ci_github_runner: String,
    }

    enum ScaffoldProjectPlan {
        RustReact(RustReactScaffoldPlan),
        GoReact(GoReactScaffoldPlan),
    }

    struct RustReactScaffoldPlan {
        backend: RustScaffoldPlan,
        react: ReactScaffoldPlan,
    }

    struct GoReactScaffoldPlan {
        backend: GoScaffoldPlan,
        react: ReactScaffoldPlan,
    }

    struct ReactScaffoldPlan {
        repo_dns_label: String,
        package_manager: String,
        frontends: Vec<FrontendScaffold>,
        custom_frontend_notices: Vec<String>,
    }

Exact private names may change if implementation evidence demands it, but the ownership and exhaustiveness properties must not. Public CLI types, JSON schemas, serialized answers, template contexts, and generated repository contracts remain unchanged.

Revision note (2026-08-30, Codex): replaced the one-line structured-work body with the initial self-contained B01 ExecPlan after source inspection. The plan records the exact baseline, typed boundary, behavioral oracles, review loop, and recovery rules required to resume this task from this file alone.

Revision note (2026-08-30, Codex): updated progress and evidence after implementation validation, corrected the focused wizard test filter, and recorded the one Clippy-driven syntax cleanup before comprehensive review.

Revision note (2026-08-30, Codex): recorded the fresh development-binary gate batch and its exact receipt before freezing comprehensive-review pass 1.

Revision note (2026-08-30, Codex): recorded the verified pass-1 consolidation and resolved its two open questions from the checked-in B02/B03 contracts before making review fixes.

Revision note (2026-08-30, Codex): recorded pass-1 fixes and focused revalidation before refreshing structured gate evidence and freezing pass 2.

Revision note (2026-08-30, Codex): recorded the verified pass-2 consolidation, the compatibility-based resolution of its open questions, and the bounded final-fix decisions before source edits.

Revision note (2026-08-30, Codex): recorded final-fix validation before refreshing receipts and freezing the third and final comprehensive review pass.

Revision note (2026-08-30, Codex): completed the plan with the verified third-pass consolidation, successor-task deferrals, final validation totals, and implementation outcome before structured-work closure.
