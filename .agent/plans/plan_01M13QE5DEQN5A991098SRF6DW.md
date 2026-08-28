# Make LOC enforcement template-owned

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must stay current as implementation proceeds. Maintain this plan in accordance with `.agent/PLANS.md` from the repository root.

## Purpose / Big Picture

After this work, Jig will supervise and record line-of-code (LOC) checks without knowing how Rust files are counted or limited. Fresh Rust repositories will receive an opinionated checker from the Rust template, but that checker will be an ordinary repository-owned command action that may be replaced or removed. A repository for another language will be able to declare a comparable command action and receive the same selection, affected-planning, process supervision, receipts, and work evidence without a new Jig schema or runtime branch.

The visible proof is twofold. First, `scripts/check-rust-file-loc.sh` will independently enforce the existing Rust policy in positional default-branch, `--changed-against`, `--staged`, and `--all` modes. Second, contract-v6 fixtures will run Rust and non-Rust LOC actions through the generic repository-action execution path while searches and CLI tests prove that Jig core no longer exposes native Rust LOC semantics.

## Progress

- [x] (2026-08-28 08:23Z) Read the root and `crates/jig/AGENTS.md` guidance, `.agent/PLANS.md`, beads epic `.7` and tasks `.7.1`/`.7.2`, and the comprehensive-review runtime.
- [x] (2026-08-28 08:23Z) Captured baseline commit `68856a09f5e976f499d86a8b86159ae57b62a393` and opened structured work plan `plan_01M13QE5DEQN5A991098SRF6DW`.
- [x] (2026-08-28 12:48Z) Implement slice `.7.1`: published the self-contained managed Rust checker, safe rendered root arguments, nine focused mode/policy integration tests, and a recopy-root regression.
- [x] (2026-08-28 15:21Z) Verify slice `.7.1` and complete three fingerprint-verified working-tree comprehensive-review rounds, addressing every actionable finding. The final full suite reached 2052 passes with one unrelated transient SQLx driver-probe failure that passed immediately in isolation; format, clippy, diff checks, focused tests, and the development build pass. Bead mutation remains unavailable because the installed `br` runtime schema is incompatible.
- [x] (2026-08-28 15:27Z) Commit the reviewed `.7.1` slice as `56bbba1` before beginning native deletion.
- [x] (2026-08-28 16:08Z) Implement slice `.7.2`: removed native LOC CLI/DTO/runtime/policy/tool-definition surfaces; routed launchers, CI, release validation, rendered fixtures, compatibility coverage, and documentation through ordinary command actions; added generic non-Rust action/evidence and authored-authority recopy proofs.
- [x] (2026-08-28) Verify slice `.7.2` and complete all three allowed fingerprint-verified working-tree comprehensive-review rounds, addressing every actionable finding. The clean configured test rerun passed 2,665 core, 440 vault, and 2 serialized vault-TUI tests; rendered backend/full/tooling fixture repositories and configured LOC, fmt, clippy, and contract actions pass.
- [x] (2026-08-28) Record that bead `.7.2` cannot be closed safely with the schema-incompatible installed `br`, preserve Beads files unchanged, and commit the reviewed slice in this commit.
- [x] (2026-08-28) Complete fingerprint-verified branch comprehensive-review round 1 with Claude and Codex. Fix literal Git pathspec handling, document the generic output-shape cutover, and strengthen exact-target legacy compatibility coverage; retain intentional moved-hint deletion and fail-closed unmatched-root behavior.
- [ ] Run full configured gates and a clean branch-scope comprehensive review against the pinned `origin/master` baseline, addressing findings for up to three rounds.
- [ ] Audit every epic acceptance criterion against current files and command evidence, close/sync epic `.7`, finish the Jig work plan, and record outcomes.

## Surprises & Discoveries

- Observation: `br 0.5.2` currently reports that the local runtime schema remains incompatible after repair, while `bv --robot-triage` reads `.beads/issues.jsonl` successfully.
  Evidence: `br list --status=open --json` returned `CONFIG_ERROR`; `bv --robot-triage --format toon` identified `.7.1` as actionable and `.7.2` as blocked by `.7.1`.
- Observation: the source repository is still contract v5 even though bootstrap already knows how to generate v6 actions and the `.7.2` bead deliberately leaves the source v6 dogfood cutover to bead `.1.2`.
  Evidence: `.jig.toml` declares `contract_version = 5`, while `crates/jig/src/bootstrap/repository_model.rs::add_rust_file_loc_action` creates the v6 `rust-file-loc` action and legacy alias.
- Observation: the canonical empty-tree fallback is a Git tree object, not a commit, so validating every comparison input as `^{commit}` broke first-commit repositories.
  Evidence: `default_branch_mode_preserves_local_parent_and_empty_tree_fallbacks` initially failed after printing the empty-tree hash; explicit handling now preserves the canonical tree while ordinary missing refs still fail closed.
- Observation: v6 recopy initially broadened a generated checker's configured Rust roots from `crates` to `.`, because reloading the authored repository model derived Rust roots from the backend component root.
  Evidence: `update_recopy_refreshes_the_checker_with_configured_rust_roots` observed `rust_roots=(.)` after recopy. Resolution now retains configured `rust_crate_roots` when the authored model still contains the template `rust-file-loc` action; custom models without that action continue deriving Rust capability roots from their components.
- Observation: comprehensive-review round 1 found that ambient `diff.renames` settings could disable legacy rename detection or classify a new copy outside the candidate filter, and that the action-presence guard matched only the action id rather than the component-scoped target.
  Evidence: Claude and Codex independently identified the Git configuration gap; Codex identified the target-identity gap. The checker now passes `--find-renames`, includes `C` candidates, and tests `diff.renames=false`/`copies`; answer resolution now matches only `repo:rust-file-loc` and a `worker:rust-file-loc` regression proves component scoping.
- Observation: no real Bash 3.2 executable is installed in the current Linux environment.
  Evidence: the portability test probes `JIG_BASH_3_2` and common executable paths, reports the explicit skip, retains syntax/forbidden-feature guards, and executes the zero-root path under ordinary Bash. A stock macOS or configured CI host will execute the same test with real Bash 3.2.
- Observation: comprehensive-review round 2 found that a custom runner retaining target `repo:rust-file-loc` was still mistaken for the generated action and that parallel rename arrays made candidate-to-baseline lookup quadratic.
  Evidence: generated-action detection now also requires the exact command runner key and resolved generated command, with a same-target custom-runner regression. The checker now converts Git's NUL-delimited name-status stream into baseline/current pairs once and a 64-file bulk-rename regression exercises the linear path.
- Observation: comprehensive-review round 3 found unsafe residual control bytes/`echo` behavior, default-branch text coupling in action recognition, and vacuous success when every configured root matched no tracked path.
  Evidence: diagnostics now byte-escape every non-printable value and use `printf`, including under `xpg_echo`; shared repository-model authority recognizes the managed checker by target, runner, and script binding independent of branch text; the checker validates configured roots against the index or baseline and fails operationally when all are unmatched.
- Observation: the final full `.7.1` suite exposed a transient, scope-unrelated SQLx driver-probe startup failure after 2052 tests had passed.
  Evidence: `cargo test -p jig-sh` reported only `doctor::tests::sqlx_driver_probe_invokes_shim_safely_and_times_out` as failed (`Indeterminate("the driver probe could not start")` versus `Compatible`); the exact test passed immediately in isolation. No LOC test failed. `cargo fmt --all -- --check`, clippy with warnings denied, `git diff --check`, and a fresh binary build also passed.
- Observation: the first real generic-action invocation correctly reached `repo:rust-file-loc` but exposed three edited Rust files above the checker's absolute limit.
  Evidence: `JIG_DEV_BIN=target/debug/jig scripts/jig check rust-file-loc --no-receipt` initially reported `answers.rs` at 1008, `repository_model.rs` at 1025, and its tests at 1050 lines. Small cohesive modules now own answer accessors, managed-checker recognition, and LOC projection tests; the same launcher invocation passes with the main files at 973, 998, and 897 lines.
- Observation: the `.7.2` review rounds exposed authority bugs beyond native deletion: removing the authored action still rendered its CI job, a same-key authored direct mode could be rewritten as a branch invocation, gate previews could remain stale, and the generated workflow selected every component action named `rust-file-loc`.
  Evidence: CI and gate-preview rendering now require the exact `repo:rust-file-loc` action, the workflow invokes `scripts/jig check repo:rust-file-loc`, and recopy regressions cover action/alias/profile removal plus preservation of `--all` and configured roots.
- Observation: refreshing a generated default-branch command in `RepositoryRenderModel` was insufficient because runtime-config reconciliation could restore the pre-render command from the destination `.jig.toml`.
  Evidence: reconciliation now prefers the refreshed rendering only when both old and new values match the narrow generated single-positional-branch form. The integration test changes `main` to `master` and observes the on-disk command refresh, then changes to authored `--all` and proves a later recopy preserves it.
- Observation: the first configured full test run passed 2,664 of 2,665 core tests before an existing work-evidence assertion saw a transient null tool identity; the exact test passed immediately in isolation.
  Evidence: an independent full configured rerun passed all 2,665 core tests, followed by all 440 vault tests and both serialized vault-TUI tests. No implementation change was needed for the transient.
- Observation: branch comprehensive-review round 1 found that shell-quoting configured roots did not neutralize Git's `:(...)` pathspec magic.
  Evidence: portable repository paths permit a leading colon, so a root such as `:(exclude)crates` could broaden or narrow candidate discovery. The checker now exports `GIT_LITERAL_PATHSPECS=1`, and a regression proves an oversized Rust file outside that literal root is ignored.
- Observation: the same review suggested that default adoption of a top-level single-crate repository would retain the fallback `crates` root, but current adoption inference already prevents that case.
  Evidence: `infer_rust_crate_roots_with_metadata` returns `["."]` for a root `[package]` manifest, `AdoptInference::apply_to_answers` supplies it before rendering, and existing inference coverage asserts the value. No policy relaxation was made.
- Observation: running the generic LOC action after committing `.7.2` exposed a candidate-selection blind spot in pre-commit validation and one newly oversized test module.
  Evidence: changed mode compares the pinned baseline to committed `HEAD`, so its earlier uncommitted invocation did not select `.7.2` edits. After commit, `runtime/tests.rs` was 864 lines and failed policy. The cohesive legacy compatibility test now lives in `runtime/tests/legacy_loc.rs`, the parent is exactly 800 lines, and the same exact-target action passes.

## Decision Log

- Decision: Treat `.7.1` and `.7.2` as the two independently committed and reviewed slices.
  Rationale: the epic explicitly requires the managed replacement to land before native policy deletion, and `.7.2` has a blocking dependency on `.7.1`.
  Date/Author: 2026-08-28 / Codex
- Decision: Keep Rust roots, thresholds, exception annotations, and line-count semantics inside the managed Bash checker rather than adding contract fields or a shared LOC library.
  Rationale: these rules are repository/template policy; the epic rejects a native or schema-level generic LOC abstraction.
  Date/Author: 2026-08-28 / Codex
- Decision: Use the existing generic contract-v6 action fixtures and durable run APIs for the non-Rust proof.
  Rationale: the feature must prove parity through the existing planner, supervisor, receipt, and evidence machinery, not build a second path.
  Date/Author: 2026-08-28 / Codex
- Decision: Preserve the dedicated configured Rust roots during recopy only while the authored model retains a `rust-file-loc` action.
  Rationale: those roots are checker/template policy and must stay stable for the generated action, while a custom authored model that removes the action must retain component-owned capability inference rather than revive a legacy projection.
  Date/Author: 2026-08-28 / Codex
- Decision: Treat LOC diagnostic paths as deliberately shell-escaped and operational/script failures as exit status 2, reserving status 1 for actual policy violations.
  Rationale: newline-bearing repository paths must remain one unambiguous diagnostic line, and callers should be able to distinguish a violated LOC threshold from a broken Git or execution environment.
  Date/Author: 2026-08-28 / Codex
- Decision: Preserve configured `rust_crate_roots` only for the exact generated LOC action runner and command, not merely for its stable target id.
  Rationale: configured roots are explicit template policy while the action still invokes the managed checker through its generated runner binding; a custom runner or different implementation is an authored replacement and returns root authority to authored Rust components. The invocation's branch argument may legitimately lag a top-level default-branch rename without changing implementation ownership.
  Date/Author: 2026-08-28 / Codex
- Decision: Keep the stable `jig.rust_file_loc` string only as an authored compatibility alias while deleting every native CLI, request, runtime, and policy implementation behind that name.
  Rationale: older supported contracts can still bind the alias to a command, and current v6 repositories reach the same action through the generic selector and supervisor without retaining product-owned policy semantics.
  Date/Author: 2026-08-28 / Codex
- Decision: Distinguish broad managed-checker ownership from the narrow generated default-branch command shape.
  Rationale: `scripts/check-rust-file-loc.sh --all` still means the template checker owns configured Rust roots, but it is authored direct-mode authority and must not be rewritten during a default-branch refresh. Only one non-flag positional argument is generated branch authority.
  Date/Author: 2026-08-28 / Codex
- Decision: Generated CI selects `repo:rust-file-loc` exactly while documentation and fixture coverage retain action-wide `rust-file-loc` examples.
  Rationale: CI is rendered only for the repository target and must not accidentally execute a second component's same-named action; action-wide selection remains an intentional generic CLI capability tested elsewhere.
  Date/Author: 2026-08-28 / Codex
- Decision: Export `GIT_LITERAL_PATHSPECS=1` for the standalone checker rather than prefixing individual configured roots with Git magic.
  Rationale: Rust roots are portable literal repository directories, not Git query expressions. A process-wide setting covers root validation and every candidate-discovery mode consistently while leaving revision parsing unchanged.
  Date/Author: 2026-08-28 / Codex

## Outcomes & Retrospective

Implementation is in progress. No epic outcome is claimed until both slices, their review loops, the branch review, configured gates, and the requirement-by-requirement audit are complete.

## Context and Orientation

Jig is a Rust workspace whose main CLI/runtime crate is `crates/jig`. The checked-in `scripts/jig` launcher selects either a built development binary through `JIG_DEV_BIN` or an installed runtime. `.jig.toml` is this repository's source configuration and `.agent/jig-contract.json` is its resolved contract. Contract v5 exposes named tools such as `jig.rust_file_loc`; contract v6 describes repositories, components, actions, profiles, commands, aliases, and evidence.

The managed checker exists in three synchronized places: source template `templates/project/scripts/check-rust-file-loc.sh.jinja`, compiled snapshot `crates/jig/src/bootstrap/embedded_template_snapshots/scripts/check-rust-file-loc.sh.jinja`, and this repository's generated `scripts/check-rust-file-loc.sh`. It is now self-contained and receives `rust_crate_roots` from bootstrap answers as safely rendered repository-relative Git pathspecs, with an empty list meaning repository-wide selection.

Before slice `.7.2`, native LOC behavior lived in `crates/jig/src/policy.rs`, with CLI types in `crates/jig/src/cli/check.rs`, conversions in `crates/jig/src/cli/command_conversion.rs`, request DTOs in `crates/jig/src/command/check.rs`, runtime dispatch in `crates/jig/src/runtime.rs`, and strict launcher metadata. Those surfaces are now removed; `check rust-file-loc` falls through to ordinary action selection, while checker-only modes remain direct script arguments.

Bootstrap v6 construction is centered in `crates/jig/src/bootstrap/repository_model.rs`. Its `add_rust_file_loc_action` method declares the repository action, command binding, inputs, and optional legacy alias. Update/recopy compares rendered defaults with authored current authority; regression tests must demonstrate that authored action replacement/removal, alias removal, and verification-profile removal are not recreated. Generic execution and evidence tests live under `crates/jig/src/runtime/tests/repository_execution.rs` and `crates/jig/src/runtime/tests/mcp/repository_execution.rs`; extend focused helpers rather than introducing an LOC-specific runtime fixture framework.

A comprehensive review means the `jig-review:comprehensive-review` workflow: Claude and native Codex inspect the same fingerprinted diff independently, reports are merged, and implementation pauses during review because reviewers are read-only. Slice reviews use working-tree scope. The final review uses branch scope, which requires a clean checkout and a pinned base commit.

## Plan of Work

Milestone one implements bead `.7.1`. Replace the delegating checker with a Bash 3.2-compatible implementation. Parse exactly one effective mode: a sole positional default branch, `--changed-against REF`, `--staged`, or `--all`. Reject missing values, extra arguments, and combined modes. Resolve the positional branch through `origin/<branch>`, `HEAD^`, or the empty-tree hash. Build Git commands with configured Rust roots, use NUL-delimited candidate and rename streams, read staged blobs from the index, and compare working-tree/all content from files. Count physical lines so empty files are zero, both LF and CRLF terminators count once, and an unterminated final segment counts. Preserve the 400, 500, 600, 800, and 1000 boundaries and the first-40-line `agentic-loc-exception:` or `@generated` rule. Preserve legacy non-growth using the previous path when a rename is detected. Print errors to standard error and exit nonzero; print warnings/information while succeeding when there are no errors.

Add a focused integration harness under `crates/jig/tests` or bootstrap checker tests that renders or copies the managed script into generic temporary Git repositories. Exercise all invocation modes, all threshold bands and exact boundaries, empty/LF/CRLF/trailing/unterminated input, new/deleted/renamed files, exception positions, generated markers, missing refs, invalid mode combinations, multiple/one/zero/dot Rust roots, and filenames containing spaces and line breaks. Run an installed Bash 3.2 executable when available and retain a syntax/feature guard plus ordinary Bash coverage when the platform cannot supply it. Assert the template and embedded snapshot stay byte-identical and that generated update/recopy refreshes default root arguments without replacing authored authority. Copy the completed template into the source generated checker so dogfooding no longer calls native LOC.

Verify milestone one with focused tests, `bash -n`, direct checker scenarios, and template snapshot checks. Then run comprehensive-review over the working tree. Address actionable findings, rerun relevant tests, and repeat review if code changed materially, up to three total rounds. Once clean, close/sync bead `.7.1` if `br` is operational and commit the self-contained replacement before starting native deletion.

Milestone two implements bead `.7.2`. Delete `CheckRustFileLocOpts`, `RustFileLocRequest`, `RustFileLocInput`, all related enums/conversions/dispatch, the Rust LOC constants and helpers in `policy.rs`, dedicated native tests, help text, tool-definition constants, and launcher moved-command hints. Remove `rust-file-loc` from strict launcher metadata so `jig check rust-file-loc` can reach the existing v6 selector fallback; LOC-specific flags remain accepted only by the script. Preserve `jig.rust_file_loc` only as authored contract data where compatibility requires it.

Update generated workflows, current workflow, release validation, rendered repository scripts, bootstrap previews, documentation, and embedded launcher snapshots. Contract-v6 CI and work verification must select the action or verification profile. Add a supported older-contract fixture whose `jig.rust_file_loc` tool points directly at the self-contained script and prove execution does not recurse into native dispatch. Historical generated repositories remain source-revision pinned. Extend a generic v6 runtime fixture with an ordinary non-Rust source component and `file-loc` command action. Exercise exact and action-wide selectors, affected input selection, supervised failure, receipt identity, and required evidence identity through existing APIs.

Add update/recopy regression coverage that starts from a generated v6 Rust model, then separately replaces or removes the LOC action, removes `jig.rust_file_loc`, and removes verification-profile membership. Recopy/update must preserve those authored choices. Do not change historical `.agent/state/*.jsonl`; only new test repositories and this plan's ordinary append-only receipts may gain records.

Verify milestone two with focused CLI/bootstrap/runtime tests, searches for removed symbols and stale native instructions, `cargo test -p jig-sh`, a fresh build, and `JIG_DEV_BIN=target/debug/jig scripts/jig check test`. Run the same working-tree comprehensive-review loop for up to three rounds, then close/sync `.7.2` and commit.

Finally make the checkout clean and pin `origin/master` to its commit OID. Run configured Jig gates and a branch-scope comprehensive review over every branch commit. Address findings and repeat up to three total branch rounds. Re-run final gates after the last code change. Audit every acceptance item from `.7`, `.7.1`, and `.7.2` against a concrete file, test, search, receipt, or command result; close/sync epic `.7`; and finish the structured plan only when all evidence is current.

## Concrete Steps

Run all commands from `/home/aa/.herdr/worktrees/jig-sh/feat-codex-resume`.

Inspect and build the runtime before dogfooding:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig work status

During checker implementation, use focused commands such as:

    bash -n templates/project/scripts/check-rust-file-loc.sh.jinja
    cmp templates/project/scripts/check-rust-file-loc.sh.jinja crates/jig/src/bootstrap/embedded_template_snapshots/scripts/check-rust-file-loc.sh.jinja
    cargo test -p jig-sh rust_file_loc -- --nocapture
    scripts/check-rust-file-loc.sh --all

After each slice, run formatting and focused/full tests before review:

    cargo fmt --all -- --check
    cargo test -p jig-sh
    cargo build -p jig-sh --bin jig
    JIG_DEV_BIN=target/debug/jig scripts/jig check test

Use the comprehensive-review skill with default reviewers Claude and Codex. Slice scope is `working-tree`; the final scope is `branch --base <pinned-origin-master-oid>`. Recompute the skill's scope fingerprint before and after every review and do not merge reports if it changes.

At final verification, also run:

    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M13QE5DEQN5A991098SRF6DW
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M13QE5DEQN5A991098SRF6DW
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M13QE5DEQN5A991098SRF6DW
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M13QE5DEQN5A991098SRF6DW

Success means all applicable gates pass, non-applicable gates have explicit evidence, review reports have no unresolved actionable findings, and searches find no native LOC implementation or stale dedicated CLI references.

## Validation and Acceptance

The checker tests must prove every policy and invocation branch, not merely that representative files pass. Exact-boundary matrices cover zero lines and 400/500/600/800/1000 lines plus the first line above each boundary. CRLF and LF versions must yield the same classification. A file with an unterminated last line must count that segment. Annotation tests place markers inside and outside the first 40 physical lines. Legacy tests compare increasing, equal, and decreasing content above both hard and absolute thresholds and include a rename. Path tests use spaces and embedded newlines while Git transport remains NUL-delimited.

The bootstrap tests must prove fresh Rust generation includes the action in the default verification profile and that source and embedded checker snapshots match. They must then mutate generated v6 authority and demonstrate recopy/update does not reconstruct an authored removal or overwrite a replacement. Root tests cover no configured roots, one root, multiple roots, and `.`.

The native-removal proof is a repository-wide search with no definitions or dispatch of the removed LOC types/constants/helpers. CLI tests show that contract-v6 `jig check rust-file-loc` resolves an authored action and rejects checker-only mode flags at the Jig selector boundary. A supported older contract invokes a command-backed alias without recursion. A non-Rust v6 fixture shows affected-selection skip/run behavior, successful and failed supervised execution, and new receipt/evidence records naming the generic action target.

Final acceptance includes `cargo test -p jig-sh`, a freshly built development runtime, `JIG_DEV_BIN=target/debug/jig scripts/jig check test`, configured work gates, two completed slice review loops, and a completed branch-scope review loop. Historical state files must show append-only additions only; no prior JSONL record may be rewritten.

## Idempotence and Recovery

Template edits are safe to repeat when the source, embedded snapshot, and generated source checker are updated together. Temporary Git fixtures must use test-managed temporary directories and generic identifiers. If a test or review fails, keep the structured plan open, record the discovery here, fix the smallest root cause, and rerun the affected proof before broader gates.

Do not use the repo-local cached runtime after modifying `crates/jig`; rebuild and set `JIG_DEV_BIN=target/debug/jig`. Do not rewrite historical `.agent/state` lines. If `br` remains schema-incompatible, continue implementation using `bv` and the checked-in JSONL as read-only issue authority, record the tool blocker, and repair issue status only through a compatible `br` installation rather than hand-editing Beads records.

Branch-scope reviews require a clean checkout. Commit each reviewed slice and any final review fixes before the branch review. Do not push unless separately requested. If a reviewer changes scope unexpectedly, discard that review result and restart from a fresh fingerprint.

## Artifacts and Notes

The implementation baseline is:

    68856a09f5e976f499d86a8b86159ae57b62a393

The epic and slices are:

    feat-codex-resume-generic-monorepo-zac.7
    feat-codex-resume-generic-monorepo-zac.7.1
    feat-codex-resume-generic-monorepo-zac.7.2

The requested review policy is one comprehensive review after each slice and a final branch-scope comprehensive review, with fixes and repetition up to three rounds at each boundary.

## Interfaces and Dependencies

The managed checker remains a standalone Bash executable at `scripts/check-rust-file-loc.sh`; it may depend only on Bash 3.2-compatible syntax, Git, and ubiquitous POSIX userland available in supported generated Rust repositories. Its stable invocation surface is one of `DEFAULT_BRANCH`, `--changed-against REF`, `--staged`, or `--all`. It accepts Rust roots rendered by the template as internal generated authority rather than runtime schema.

Contract-v6 continues using existing `ActionSpec`, `TargetRef`, repository/component models, profiles, commands, aliases, and evidence references. The Rust checker action remains `rust-file-loc`; `jig.rust_file_loc` may remain only as a manifest alias. No `FileLocPolicy`, language registry, LOC schema, new runner, or new evidence DTO may be introduced.

Revision note (2026-08-28): Expanded the initial one-line body into a self-contained implementation and verification plan after reading epic `.7`, both slice acceptance contracts, current native policy, bootstrap action construction, repository guidance, and comprehensive-review requirements.

Revision note (2026-08-28): Recorded the implemented `.7.1` checker/test boundary and the empty-tree and recopy-root defects found by focused regression tests.


Completed slice .7.1 after three fingerprint-verified comprehensive-review rounds. All actionable findings were fixed. Focused LOC/bootstrap tests, fmt, clippy (-D warnings), diff check, and a fresh jig binary build pass. The full jig-sh suite passed 2052 tests before one unrelated SQLx doctor probe startup failure; that exact test passed immediately in isolation. br 0.5.2 remains schema-incompatible, so bead status could not be mutated safely.
