# Complete Rust-only preset docs, dogfood gates, and release acceptance

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current in accordance with `.agent/PLANS.md`.

The structured work identifier is `plan_01M1A48T6MC1QV36DWQQ7714QX`. The owning Beads task is `jig-sh-rust-only-init-presets-zc7.3.2` (B07). The exact Git baseline is commit `2039030e42fa46ab474eebd97eb2aa489d5c5e38`. The working tree also contains the completed, uncommitted B03 through B06 implementation, documentation-independent tests, and structured evidence. Preserve that dependency work: B07 documents and release-tests its finalized behavior and must not redefine the preset, help, wizard, diagnostic, report, or generated-repository contracts.

## Purpose / Big Picture

After this work, someone encountering Jig through the README, Developer UX guide, Configuration reference, CLI help, or generated repository sees one coherent Rust-only preset family. They can choose a new one-library or one-binary virtual workspace, understand that neither includes application/database/frontend state, run setup and commit the resulting Cargo lock file, and know which files Jig may update later. Existing Rust repositories are directed to `jig adopt .`, not encouraged to initialize over existing work.

Release acceptance will then prove that these are executable instructions rather than prose-only claims. A freshly built development binary will initialize generic library and CLI repositories through the public process boundary, the human and JSON output will be inspected, their generated neutral guidance will be audited, and the documented setup/check commands will pass. Only after fresh plan-scoped gates, diff/privacy review, and live dependency-state verification will B07 and the completed feature/epic containers close.

## Progress

- [x] (2026-08-30 20:02Z) Closed B06 with all eight gates fresh, revalidated the live B07 dependency and acceptance text, claimed B07, opened structured work, and captured the exact baseline and dirty dependency work.
- [x] (2026-08-30 20:02Z) Compared the current README, Developer UX, and Configuration docs with the finalized dev-binary `jig presets` and `jig init --help` output.
- [x] (2026-08-30 20:08Z) Aligned README quick start/examples with the complete five-preset guided and strict behavior, including both Rust-only layouts, setup/lock policy, absence boundary, and project ownership.
- [x] (2026-08-30 20:08Z) Aligned Developer UX init/adopt guidance and added the Rust CLI alongside the library without duplicating application-preset details.
- [x] (2026-08-30 20:08Z) Added a focused Configuration reference for Rust-only answer compatibility, setup/check/lock behavior, generated guidance, and embedded snapshot ownership.
- [x] (2026-08-30 20:13Z) Rebuilt the dev binary; passed focused descriptor/help/strict/JSON/generated-repository tests; dogfooded generic library and CLI repositories through human and JSON public invocations; passed their setup/test workflows; verified both lock files, neutral guidance, and absence boundaries; and ran the CLI as `examplecli 0.1.0`.
- [x] (2026-08-30 20:14Z) Passed and inspected all eight structured gates plus evidence/receipts, audited diff/privacy/stale snapshots and live Beads state, finished structured B07 work, closed B07, closed all three feature containers, closed the root epic after its ten-descendant rollup was closed, and ran `br sync --flush-only` after each mutation batch.

## Surprises & Discoveries

- Observation: the B05-owned CLI surfaces already express the complete family and should be verified unchanged rather than edited in B07.
  Evidence: the fresh dev binary lists `rust-react`, `go-react`, `harness-only`, `rust-library`, and `rust-cli` in order; `jig init --help` includes both Rust-only examples, says strict Rust-only presets need no database/frontend choice, and states scaffold ownership.

- Observation: README currently documents `rust-library` but still describes the guided and strict families as only Rust React, Go React, and harness-only, and it has no Rust CLI example.
  Evidence: `README.md` under “Creating and adopting repos” names three choices on line 95, lists only harness/library before application presets, and has no `rust-cli` occurrence.

- Observation: Developer UX has the same three-choice description and a library-only Rust-only section.
  Evidence: `docs/developer-ux.md` under “Initializing New Repos” says to choose `rust-react`, `go-react`, or `harness-only`, then documents `rust-library` but not `rust-cli`.

- Observation: Configuration thoroughly documents generic init transaction safety and application/frontend configuration but has no consolidated Rust-only preset answer, setup, Cargo.lock, or snapshot section.
  Evidence: searches for `rust-library` and `rust-cli` in `docs/configuration.md` return no matches, while embedded-template refresh guidance currently appears in Developer UX only.

- Observation: the complete accepted Rust-only answer family retains full-harness metadata and fixed neutral compatibility values, while authored shape authority is deliberately rejected.
  Evidence: the library and CLI wizard acceptance tests accept repository/branch/CI/template metadata, vault, status, execution, agent tooling, proxy settings, the fixed `rust_crate_roots = ["crates"]`, false SQLx/schema/application-contract fields, Rust command overrides, and inert package-manager input; their rejection tables cover Go, database/migration/schema/frontend/dev-app and authored repository/commands/work/loop authority.

- Observation: the public process boundary and generated launcher path reproduce the documented workflow without a local-template override.
  Evidence: the human library report and JSON CLI report both selected `embedded:jig-sh`, reported `db: none`, created exactly five scaffold files, and offered only setup/test next steps. Both generated repositories passed `scripts/jig setup` and `scripts/jig check test` through the absolute development binary and produced `Cargo.lock`; the CLI printed exactly `examplecli 0.1.0`.

- Observation: B07's docs-only delta did not invalidate gate-scoped source evidence, while the always-applicable contract and LOC gates executed against the full integrated diff.
  Evidence: plan batch `receipt_01M1A4T9SJSF3JMZFYVHQK2BH6` reports eight required gates passed and fresh, with two executed and six reused only after matching the current gate-scoped fingerprint. A separate fresh dev-binary repository run passed all 3,163 tests under `receipt_01M1A4SS31CME80N9VF1TGNNQ7`.

## Decision Log

- Decision: treat the dev-binary B05 help, wizard, doctor, strict diagnostic, and preset descriptors as read-only release inputs unless a direct inconsistency is found.
  Rationale: B07 explicitly verifies rather than owns those surfaces. Editing them to fit prose would blur task ownership and invalidate the already-passing B05/B06 exact tests.
  Date/Author: 2026-08-30, Codex.

- Decision: add concise Rust-only sections to the three required user documents and link concepts rather than copying the exhaustive design plan.
  Rationale: the release docs need commands, layouts, compatibility, setup, ownership, and snapshot facts. Transaction internals and exhaustive rejection matrices remain better located in code/tests and the checked-in design plan.
  Date/Author: 2026-08-30, Codex.

- Decision: dogfood the current development binary's embedded templates with omitted `--template`, using generic temporary `ExampleLibrary` and `ExampleCli` destinations and an absolute `JIG_DEV_BIN` for generated launcher commands.
  Rationale: the source changes are intentionally uncommitted, so committed local-template mode would read the old Git baseline. An unreleased dev build embeds the current working-tree templates and is the actual release-candidate boundary B07 must exercise.
  Date/Author: 2026-08-30, Codex.

- Decision: accept fingerprint-proven source-gate reuse in the B07 structured batch while also running the requested repository-level fmt, Clippy, test, contract, agent-map, and agent-guides checks directly on the fresh binary.
  Rationale: Jig marks reused gate evidence fresh only when its scoped inputs are identical. The docs changed the always-applicable gate fingerprint but did not change source-only fingerprints; direct requested checks add an independent current-worktree release pass without manufacturing redundant structured receipts.
  Date/Author: 2026-08-30, Codex.

## Outcomes & Retrospective

B07 and the complete Rust-only preset epic are closed. README, Developer UX, and Configuration now share one five-preset contract and cover both Rust-only layouts, init/adopt choice, strict answers, absence boundaries, setup/lock policy, project ownership, and snapshot maintenance. Fresh public dogfood and focused tests matched those claims; all requested repository checks passed, including 3,163/3,163 tests, and structured batch `receipt_01M1A4T9SJSF3JMZFYVHQK2BH6` has eight of eight gates fresh with no unresolved evidence. The final source/docs diff check is clean, no embedded snapshot file drift exists, and only generic fixture identities were added. Structured work closed successfully; B07 then closed, all three feature containers closed after their task rollups, and root epic `jig-sh-rust-only-init-presets-zc7` closed with all ten descendants closed. No release caveat remains, and no commit or push was performed.

## Context and Orientation

`README.md` is the public quick start. Its “Creating and adopting repos” section owns the shortest choice-and-command narrative. It must state that `jig init` targets a new destination while `jig adopt .` targets an existing repository. The guided terminal flow uses the same five descriptor-backed choices as `jig presets`; `--defaults` remains Rust React/no database/web, and strict automation accepts a fully specified application preset or a complete harness/Rust-only preset.

`docs/developer-ux.md` explains interaction behavior and generated developer workflow. Its “First Contact,” “Adopting Existing Repos,” and “Initializing New Repos” sections own the init/adopt distinction, guided flow, preset layouts, and project-ownership boundary. The Rust-only layouts are a root virtual Cargo workspace with one member: `crates/<repo>/src/lib.rs` for `rust-library`, or `crates/<repo>/src/main.rs` and one explicit binary target for `rust-cli`. Both use Rust 2024 and the top-level Jig Rust 1.88 baseline.

`docs/configuration.md` is the detailed renderer/config reference. It already explains transaction preflight, rollback, generic answers, generated launchers, and embedded source identity. B07 adds one bounded section for Rust-only preset compatibility: accepted common harness answers; rejected database, Go module, frontend, application-contract, SQLx/schema, dev-app, and authored repository-authority shapes; package-manager irrelevance; no public `rust-workspace` alias; setup/locked checks; project-owned scaffold files; neutral `workspace` guidance; and checked-in embedded snapshot maintenance.

The source of truth for public descriptors is `crates/jig/src/bootstrap/presets.rs`; long help and strict behavior are covered in `crates/jig/src/cli/help_tests.rs`, `init_wizard_discovery_tests.rs`, the Rust library/CLI wizard tests, doctor tests, and `crates/jig/tests/cli_json*.rs`. B07 should run those focused surfaces but should not replace their exact assertions with doc string tests unless documentation itself has a stable executable extraction boundary.

The generated Rust-only README template is `templates/scaffolds/rust-only/workspace/README.md.jinja`; checked-in packaged copies live under `crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/`. B06 proves live/snapshot raw and rendered parity. Documentation must explain that snapshot copies are release packaging inputs and are refreshed intentionally, not suggest hand-editing generated repositories or snapshots independently.

## Plan of Work

First, revise README's guided paragraph so it reflects all five public choices and distinguishes application-only follow-up prompts. Add a Rust CLI example beside the library. Describe each exact virtual-workspace layout, the absence of database/frontend/API/dev/release/license selection, the std-only CLI behavior, project ownership, and the shared setup/check/commit-lock workflow. Keep application-preset lock/dev instructions scoped to frontends so Rust-only users are not told to run `scripts/jig dev`.

Second, revise Developer UX in the same vocabulary. Make the guided flow say Rust-only selections terminate without database/frontend prompts. Present library and CLI as sibling shapes, show both commands and layouts, explicitly reject the nonexistent public `rust-workspace` spelling, and explain when to adopt an existing Rust repo. Add the shared post-init setup/check/Cargo.lock and ownership boundary before returning to application-preset details.

Third, add a Rust-only configuration subsection near the existing init transaction/answer discussion. Document preset names, effective-answer compatibility, common accepted answers, strict/no-input behavior, setup and locked checks, lock-file policy, neutral generated repository authority, project ownership on update, and embedded snapshot refresh rules. Cross-link `jig presets` as the descriptor source and avoid duplicating every internal rejection test.

Fourth, rebuild `target/debug/jig` and run focused help/preset/JSON/generated-repository tests. Create a temporary generic root. Invoke the public binary without `--template` so it consumes current embedded templates: use human output for one artifact and JSON for the other, and inspect both `jig presets` output modes as the complementary public discovery reports. In each generated repo, force its launcher through the absolute development binary, run `scripts/jig setup` and `scripts/jig check test`, and run the CLI package. Inspect root/crate guidance, `.jig.toml`, Cargo.lock, absence paths, human/JSON report identity, and Git cleanliness apart from the intentionally created lock file if setup does not commit it.

Finally, run formatting, Clippy, test, contract, agent-map, and agent-guides through the fresh dev binary, plus plan-scoped structured checks. Inspect gate freshness, receipts, the complete diff, fixture names, docs for stale three-preset claims and `rust-workspace`, snapshot drift, open/closed Beads descendants, and untracked files. Finish structured work and close B07 only when every item is proven. Then close feature containers F1/F2/F3 and the root epic only if all their task descendants are live-closed, flush Beads state, and verify no ready item from this epic remains.

## Milestones

Milestone one is a coherent documentation contract. It is complete when README, Developer UX, and Configuration all state the same five preset names, init/adopt boundary, Rust-only layouts, absence/ownership policy, setup/check commands, and Cargo.lock rule, with no public `rust-workspace` alias.

Milestone two is fresh-binary dogfood. It is complete when public human and JSON processes create one generic library and one generic CLI from current embedded templates, their neutral guidance and reports match the docs, setup/check pass, Cargo.lock exists, and the CLI prints its exact package/version line.

Milestone three is release evidence. It is complete when focused tests and all required repository/structured gates pass with fresh evidence, the final diff/privacy/snapshot review is clean, and B07 plus every completed epic container is closed and synced accurately.

## Concrete Steps

Work from `/home/aa/.herdr/worktrees/jig-sh/worktree-silver-harbor-4827`. During documentation edits and focused verification, use:

    rg -n 'rust-library|rust-cli|rust-workspace|Cargo.lock|jig init|jig adopt' README.md docs/developer-ux.md docs/configuration.md
    cargo fmt --all -- --check
    cargo test -p jig-sh presets_summary_explains_defaults_and_ownership --lib
    cargo test -p jig-sh init_help_explains_defaults_and_the_complete_strict_preset_family --lib
    cargo test -p jig-sh strict_missing_preset_diagnostic_enumerates_every_complete_shape --lib
    cargo test -p jig-sh --test cli_json rust_only --no-fail-fast
    cargo test -p jig-sh rust_library_init_generates_exact_buildable_neutral_workspace --lib
    cargo test -p jig-sh rust_cli_init_generates_exact_buildable_runnable_neutral_workspace --lib

For dogfood, build and resolve the absolute development binary, then create disposable generic destinations and run the exact documented commands:

    cargo build -p jig-sh --bin jig
    dev_jig="$(pwd)/target/debug/jig"
    dogfood_root="$(mktemp -d)"
    "$dev_jig" init "$dogfood_root/ExampleLibrary" --preset rust-library --no-input --no-vault
    "$dev_jig" --json init "$dogfood_root/ExampleCli" --preset rust-cli --no-input --no-vault
    (cd "$dogfood_root/ExampleLibrary" && JIG_DEV_BIN="$dev_jig" scripts/jig setup && JIG_DEV_BIN="$dev_jig" scripts/jig check test)
    (cd "$dogfood_root/ExampleCli" && JIG_DEV_BIN="$dev_jig" scripts/jig setup && JIG_DEV_BIN="$dev_jig" scripts/jig check test && cargo run -p examplecli)

Run required repository and structured acceptance through the fresh binary:

    JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
    JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
    JIG_DEV_BIN=target/debug/jig scripts/jig check test
    JIG_DEV_BIN=target/debug/jig scripts/jig check contract
    JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map
    JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides
    JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M1A48T6MC1QV36DWQQ7714QX
    JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M1A48T6MC1QV36DWQQ7714QX
    JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M1A48T6MC1QV36DWQQ7714QX
    JIG_DEV_BIN=target/debug/jig scripts/jig work receipts --plan-id plan_01M1A48T6MC1QV36DWQQ7714QX

At successful completion only:

    JIG_DEV_BIN=target/debug/jig scripts/jig work finish --plan-id plan_01M1A48T6MC1QV36DWQQ7714QX --resolution 'B07 Rust-only documentation and release acceptance complete.' --outcome success
    br close jig-sh-rust-only-init-presets-zc7.3.2 --reason 'Completed Rust-only docs, dogfood, and release acceptance.' --json
    br sync --flush-only

## Validation and Acceptance

README, Developer UX, and Configuration must all name `rust-library` and `rust-cli`, must not advertise `rust-workspace`, and must state that `jig init` creates a new destination while `jig adopt .` is the path for an existing Rust repository. The guided flow has five choices; only Rust React and Go React proceed to database/frontend decisions. `--defaults` remains Rust React/no database/web. Strict Rust-only automation requires the explicit preset but no database/frontend flags.

The documented library layout is one virtual-workspace member with `src/lib.rs`; the CLI layout is one virtual-workspace member with `src/main.rs` and a binary target. Both are Rust 2024 on Jig's top-level Rust 1.88 baseline, non-publishable, license-neutral, and free of database, SQLx, application contract, frontend, API, dev-app, release-workflow, and extra-layer choices. The CLI uses only std and prints package name/version. The absence language must not falsely imply the library has or needs a parser.

The post-init workflow is `scripts/jig setup` followed by `scripts/jig check test`; setup creates `Cargo.lock`, and users commit it for both application and library artifacts. Generated Cargo manifests, source, crate guide, and scaffold README become project-owned immediately. `jig update` updates the harness and does not rewrite those files. Neutral root guidance uses workspace/crate terms and does not recommend `scripts/jig dev` for either Rust-only repo.

Configuration must describe common accepted harness answers and incompatible application-shape authority without inventing persisted preset state. It must explain that release binaries package checked-in template snapshots, that live/snapshot parity is tested, and that source template changes refresh snapshots through the explicit environment-gated Cargo command. Users must not be told to edit snapshot copies separately.

The fresh binary must produce a human library report and JSON CLI report with exact preset identities, `db: none`, no frontends, five scaffold files, no `scripts/jig dev` next step, and project-owned notes. Each generated repository must pass setup and `scripts/jig check test`; each must have `Cargo.lock`, a resolvable single member, and clean neutral guidance. The CLI must run with one exact stdout line and empty stderr.

Completion requires all requested repository checks, all structured gates fresh, no privacy-fixture violations, no stale snapshot/dependent test changes, no unresolved B07 acceptance item, and live Beads state showing every child task closed before feature/epic containers close.

## Idempotence and Recovery

Documentation edits, builds, tests, and checks are repeatable. Dogfood repositories live under one `mktemp -d` root and use only `ExampleLibrary`, `ExampleCli`, and `ExampleProject` vocabulary. They are disposable and must never be copied into tracked source or append-only state. Avoid receipt-producing commands from inside those temporary repositories so their absolute paths are not captured as work evidence.

The current source working tree contains completed B03-B06 work and must not be reset. `.agent/state/*.jsonl` remains append-only. Failed checks may append diagnostic receipts; record the authoritative passing rerun instead of deleting them. If docs reveal a genuine mismatch in B05-owned help or diagnostics, keep B07 open and treat B05 as incomplete rather than silently editing its surface. If dogfood exposes a product defect, repair it at the owning shared boundary, update this plan, rerun B06-relevant acceptance, and only then resume release closure.

The embedded snapshot refresh command mutates checked-in snapshot files and must not be run unless a live source template changed and drift is expected. For docs-only B07 changes, the snapshot diff must stay empty. No Git commit or push is authorized.

## Artifacts and Notes

Primary documentation artifacts:

- `README.md`
- `docs/developer-ux.md`
- `docs/configuration.md`

Finalized behavior inputs to verify without redefining:

- `crates/jig/src/bootstrap/presets.rs`
- `crates/jig/src/cli/help_tests.rs`
- `crates/jig/src/cli/init_wizard_discovery_tests.rs`
- `crates/jig/src/cli/init_wizard_rust_cli_tests.rs` and its parts
- `crates/jig/src/cli/init_wizard_tests.rs`
- `crates/jig/src/doctor/tests_parts/part_08.rs`
- `crates/jig/tests/cli_json.rs` and `cli_json_parts/rust_only_acceptance.rs`

Generated/dogfood inputs:

- `templates/scaffolds/rust-only/workspace/README.md.jinja`
- `crates/jig/src/bootstrap/scaffold/embedded_template_snapshots/`
- `target/debug/jig` built from the current working tree

The B06 plan and its evidence provide the prerequisite exact generated-repository, transaction, compatibility, and process proof:

- `.agent/plans/plan_01M1A1E01SMN563X28X8E9T2D5.md`
- plan-scoped batch `receipt_01M1A45930QW6VDDEMMGW0RTA2`
- full dev-binary passing test receipt `receipt_01M1A40KXQV2794B182V07DPCK`
