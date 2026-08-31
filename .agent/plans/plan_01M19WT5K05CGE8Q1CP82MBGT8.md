# Seed repository file-budget policy and crash-recoverable lifecycle migration

This ExecPlan implements Bead `jig-sh-generic-monorepo-zac.8.5`. The observable result is that fresh and adopted repositories receive a seed-once authored `.jig/file-budget.toml` and a replaceable native `repo:file-budget` action, while update and recopy preserve every authored policy/action/alias/profile decision. Repositories generated with the temporary Bash Rust LOC checker migrate through two update invocations: phase one publishes native authority but retains exact recognized Bash assets; phase two retires those assets only after a fresh successful native receipt is revalidated against the staged post-update authority. Every full update is protected by a durable, fsynced Git-metadata journal whose next invocation rolls back any uncommitted transaction and only cleans up a committed one.

The exact Git baseline is `97db14b7677ad277724a4ee14e4378eeb908fd82`. This worktree intentionally contains the uncommitted Task C and Task D implementation from Beads `.8.3` and `.8.4`; preserve it because this task consumes contract v7, native file-budget configuration/results, the shared engine, and direct CLI.

## Progress

- [x] (2026-08-30 17:51Z) Revalidated Bead `.8.5` as the top actionable graph item, claimed it, read repository/crate guidance, and inspected the reviewed Task E lifecycle contract.
- [x] (2026-08-30 17:52Z) Opened structured work `plan_01M19WT5K05CGE8Q1CP82MBGT8` against the exact baseline and mapped the existing repository-model, staged-render, managed-path, and update seams.
- [x] (2026-08-30 20:20Z) Added stack-neutral deterministic policy contributions, seed-once authored publication, bounded adoption preview with human waiver blockers, native action/config rendering, and authored action/alias/profile/policy preservation.
- [x] (2026-08-30 20:20Z) Added the durable Git-metadata update journal, exclusive worktree lock, prepared/progress/committed protocol, exact rollback, failure injection, private bundles, and foreign-write preservation.
- [x] (2026-08-30 20:20Z) Added the bounded legacy identity table, phase-one registry/retention, fresh native receipt proof, staged-authority comparison, under-lock deletion revalidation, and phase-two retirement.
- [x] (2026-08-30 21:05Z) Added lifecycle/rendered/crash/adoption coverage and operator docs; rebuilt the dev binary; passed strict Clippy, 565 bootstrap tests, all eight fresh Jig gates, 2,484 source-core tests, and the final 3,242-test `scripts/jig check test`; audited formatting, diff whitespace, file limits, and every Bead criterion.

## Surprises & Discoveries

- Full updates currently call `apply_staged_render` without a transaction, so each destination is published independently. `InitMutationTransaction` provides strong in-process rollback for init and launcher repair but has no durable Git-metadata journal and cannot satisfy crash recovery by itself.
- The generated repository model still creates `repo:rust-file-loc` only for Rust backends and refreshes its command on authored round trips. Task E must introduce native file-budget authority without breaking the already-tested authored removal, runner replacement, alias, and profile preservation boundary.
- The managed-path manifest currently owns `scripts/check-rust-file-loc.sh`. The seed policy must never be added to that manifest, while phase one must deliberately keep exact legacy checker ownership until proof-authorized phase two.
- Stored full-update answers previously bypassed generated-model recognition by preserving every complete repository table. Full update now uses the managed-answer resolver, so only an exact generated projection upgrades; explicit answer files and structurally authored models remain authoritative.
- Historical checker source files usually end with LF while the Jinja-rendered destination does not. The durable table therefore needs both source and actual rendered digests for each published generation/root variant; metadata-only tests pin those identities without embedding checker source.

## Decision Log

- Decision: implement a dedicated `RepositoryUpdateTransaction` rather than extending the init transaction with crash-only state.
  Rationale: update recovery requires serialized durable preimages, staged bytes, progress, proof identity, and committed state beneath worktree-specific Git metadata. Init also supports wholly missing destinations and retained open-handle semantics with a different recovery contract; combining them would make both boundaries harder to audit.
  Date/Author: 2026-08-30 / Codex.
- Decision: classify the seed policy as an authored publication in the update plan and transaction, never as a rendered managed path.
  Rationale: its first write must be atomic with contract/action wiring, but all later init/adopt/update/recopy operations must preserve its exact bytes and must never treat its absence or content as generated ownership.
  Date/Author: 2026-08-30 / Codex.
- Decision: retain a bounded metadata-only legacy generation table in Rust.
  Rationale: Task F will delete checker source and templates, so downstream migration must recognize exact historical bytes, type, and executable bit without embedding or executing the checker implementation.
  Date/Author: 2026-08-30 / Codex.
- Decision: require a successful preview before acquiring the adoption mutation lock or preparing a journal.
  Rationale: debt discovery and incomplete human waiver drafts must remain side-effect free and must block every repository mutation, including transaction metadata creation.
  Date/Author: 2026-08-30 / Codex.
- Decision: revalidate phase-two proof immediately before applying the checker removal, while recovery ignores proof and always rolls back an uncommitted journal.
  Rationale: deletion must use current authority under the held lock, but a crashed transaction must never resume deletion if source, policy, or waiver validity changes while Jig is down.
  Date/Author: 2026-08-30 / Codex.

## Outcomes & Retrospective

Fresh full harnesses now seed a strict stack-neutral authored policy and native
repository-wide action without placing the policy under managed replacement.
Adoption previews policy classification, resource sizing, current debt, legacy
markers, and human-required waiver drafts before acquiring a mutation lock.
Generated repository projections upgrade only the exact historical LOC action;
policy/action/alias/profile removals and replacements survive recopy.

Full update, recopy, mutating adoption, and launcher repair now serialize on a
worktree-specific Git-metadata lock. Versioned transaction manifests retain
bounded exact preimages and staged payloads, fsynced progress, an optional
lifecycle proof, and a committed marker. Uncommitted recovery rolls back rather
than resumes; committed recovery only cleans up; concurrent foreign bytes block
automatic recovery and remain beside the retained recovery bundle.

The Bash lifecycle uses 15 metadata-only published identities. Phase one
retains and registers recognized bytes while publishing native authority. Phase
two requires a fresh successful native receipt bound to current source,
configuration, input, policy, comparison, evaluation, and waiver validity; the
proof is checked again immediately before deletion. Missing or stale proof
retains Bash with `scripts/jig check repo:file-budget`; unknown bytes are
deowned and preserved. Crash coverage proves that an uncommitted phase-two
deletion restores Bash even if source and proof validity change while Jig is
down.

Validation completed with strict Clippy, the 565-test bootstrap suite, all eight
fresh structured-work gates, a sequential 2,484/2,484 source-core receipt after
two independently reproduced parallel-only resource flakes, and the required
`scripts/jig check test` result of 3,242/3,242 passing (2 skipped). Task `.1.2`
still owns dogfooding Jig's authored current-epoch policy; Task `.8.6` still owns
deleting the legacy Bash source/template/tests after downstream migration
capability has shipped.

## Context and orientation

`crates/jig/src/bootstrap/repository_model.rs` projects either generated answers or the authored `[repository]` model into contract actions/profiles/tools. Its `rust_file_loc.rs` module recognizes the temporary generated Rust checker and already has preservation tests for authored runner, removal, alias, and profile choices. Task E introduces the canonical native `repo:file-budget` action with legacy alias `jig.file_budget`, `inputs = ["**"]`, and typed `NativeFileBudgetConfigV1`; fresh generated models add it to the default verification profile whenever source-policy support is enabled. An authored model is authoritative: updates may refresh only exact recognized generated wiring and must preserve removal, replacement, aliases, configuration, and profile membership.

`crates/jig/src/bootstrap/repository_model.rs` also needs a stack-neutral file-budget policy model. Template adapters contribute explicit source patterns; they do not teach the runtime evaluator about stacks. Rust contributes `**/*.rs`, Go contributes `**/*.go`, and configured frontend roots contribute their selected TypeScript/JavaScript/JSX/TSX/Vue/Svelte patterns. Contributions must be deterministic and disjoint or validation must block. Known scaffold-generated or vendored paths may become reasoned exclusions only from explicit repository-model evidence. Generated version-1 policies omit byte defaults until calibrated.

`crates/jig/src/bootstrap/renderer.rs`, `staged_render.rs`, `managed_paths.rs`, and `sync.rs` produce a complete staged managed tree, compare it with the destination, and publish paths. `.jig/file-budget.toml` must be separately staged only when absent and excluded from `StagedRender.active_paths`, `retirement_paths`, and `.agent/jig-managed-paths.json`. Existing policy bytes—including invalid/custom bytes during update/recopy—are preserved without parsing or rewriting. Init refuses to overwrite an existing policy. Adoption previews existing policy and writes a seed only when absent.

`crates/jig/src/bootstrap/update.rs` owns full update, recopy, and launcher-only repair. Full update must acquire one repository update lock, recover any prior journal, prepare template/model/policy/proof decisions without destination writes, build one ordered operation set, durably prepare a transaction, apply it, write `Committed`, and then perform warning-only runtime-cache refresh. Launcher repair must check/recover the same journal before mutation so it cannot cross an incomplete full update. A usable Git worktree and worktree-specific `git rev-parse --git-path` metadata location are required for phase-one/full-update mutation.

The new transaction module stores a mode-0700 directory beneath the resolved Git metadata path. Its versioned manifest contains only repository-relative destinations, transaction kind/state, ordered operations, preimage kind/digest/mode/bytes or absence, staged kind/digest/mode/bytes or removal, and optional phase-two proof identity. It flushes files and directories before advancing state. Every completed operation appends and flushes progress. Before touching a path, it verifies the destination still equals its preimage; rollback restores only paths that still equal a transaction-written state. Foreign replacements are never overwritten: recovery keeps the bundle, names exact paths, and blocks mutation with manual guidance. `Committed` means cleanup only; all other states roll back and never resume.

Legacy retirement recognizes a bounded table of generation ID, exact repository-relative path, SHA-256 digest, regular-file type, and executable-bit expectation. Phase one retains recognizable checker assets while installing native authority. A later invocation may remove only exact recognized assets after finding the latest successful non-truncated native target receipt and proving its runner/configuration/policy/comparison/evaluated-source/evaluation/time identities. That proof is checked before staging and again under the update lock against the exact receipt and the staged post-update authority. If the update changes any bound authority or governed source beyond the exact legacy deletions and deterministic manifest bookkeeping, it commits the valid update but retains Bash and prints the exact native rerun command.

## Plan of work

First, replace the generated Rust-specific policy contribution with a language-neutral file-budget contribution layer while keeping legacy recognition separate. Add deterministic policy DTOs/rendering and seed publication planning for init/adopt/update/recopy. The seed is written only when absent; an existing path must be a safely preserved regular file, and any required human waiver proposal blocks all lifecycle mutation. Add the native action with typed defaults and conservative input, update verification-profile and workflow rendering, and make authored round trips preserve removal/replacement/alias/profile/config choices. Minimal/tooling-only answers omit the feature explicitly.

Second, build `RepositoryUpdateTransaction`. Resolve and validate the Git metadata location through scrubbed Git, serialize a bounded manifest, stage exact payload/preimage files with restrictive permissions, fsync content and parent directories, and expose prepare/apply/commit/recover operations. Integrate it into full update and recopy before any managed or authored publication. Adapt `apply_staged_render` to plan/report operations without directly mutating, or give it a transaction-backed publication adapter, so the managed manifest participates in the same commit. Preserve the current conflict policy and backup reporting. Add deterministic failure hooks at preparation, every publication/removal, progress flush, and committed marker.

Third, add legacy asset recognition and receipt proof. Record the exact current generated checker generations without copying source bytes into the table. During phase one, exclude recognized legacy paths from ordinary staged retirement and preserve their manifest entries. During phase two, locate the latest native file-budget receipt, reject missing/failed/stale/expired/truncated/mismatched proof, compute the staged post-update authority/source identity, revalidate under lock immediately before preparation, and include only exact recognized deletions. Custom action/Bash bytes, removed native action, or changed authority keep all legacy assets. Recovery of any uncommitted phase two restores Bash and discards proof; the next invocation reevaluates from scratch.

Finally, add unit, integration, and subprocess crash tests matching section 19.8 of `docs/plans/universal-file-budget.md`; update templates and embedded snapshots together; document seed/preservation/migration/recovery behavior. Build `target/debug/jig`, force `JIG_DEV_BIN`, run focused tests while iterating, run every configured structured-work gate, inspect gates/evidence/receipts, run the required `scripts/jig check test`, and audit every Bead criterion before closure.

## Concrete steps

1. Add `bootstrap/repository_model/file_budget.rs` and policy-rendering types/tests; replace generated `repo:rust-file-loc` authority with native `repo:file-budget` while retaining legacy recognizers for migration only.
2. Add seed planning to init/adopt/update/recopy and ensure `.jig/file-budget.toml` is excluded from managed-path manifests and byte-preserved after first write.
3. Add `bootstrap/update_transaction.rs` (split into focused files as needed) with Git-metadata path resolution, exclusive lock, versioned durable manifest, staged/preimage bundles, apply/progress/commit, rollback/cleanup recovery, foreign-write preservation, and failure hooks.
4. Route full update and recopy through the transaction; make startup of update/recopy/launcher repair recover before proceeding and keep post-commit cache refresh outside the repository transaction.
5. Add `bootstrap/file_budget_migration.rs` with the durable legacy metadata table, exact recognizer, native receipt proof, staged authority comparison, retirement decision/report, and rerun guidance.
6. Update repository-policy workflow, bootstrap/adoption previews, template and embedded snapshots, configuration/public-contract docs, and generic fixtures.
7. Run formatting, focused `cargo test -p jig-sh` filters, `cargo clippy -p jig-sh --all-targets -- -D warnings`, build the dev binary, run `scripts/jig work check --plan-id plan_01M19WT5K05CGE8Q1CP82MBGT8`, inspect structured evidence, run `scripts/jig check test`, and audit the final diff and graph.

## Validation and acceptance

Completion requires source and executable evidence for every item below:

- Fresh supported templates produce a strict deterministic policy and native action exactly once; mixed stacks produce disjoint explicit rules; minimal/tooling-only mode omits them when policy support is disabled.
- Adoption previews source groups, candidate/byte estimates, current debt, explicit exclusions, and required waiver proposals. Human-required waiver reason/expiry blocks every lifecycle write.
- Existing policy bytes survive init refusal, adoption, update, and recopy unchanged and never appear in managed ownership. Policy without action, custom native/command replacement, removed action/alias/profile membership, and authored native limits are all stable across recopy.
- Phase one publishes policy plus managed native contract/action/manifest in one recoverable transaction and retains exact legacy Bash assets. Phase two removes only exact recognized generated assets after a fresh successful receipt proves every required identity; every proof failure or staged identity change retains them with an exact rerun command.
- The transaction journal resides beneath the worktree-specific Git metadata path, contains no absolute host paths, is durably prepared before mutation, and records ordered progress plus a committed marker. Every uncommitted crash state rolls back and never resumes; every committed state only cleans up; both are idempotent.
- Normal failure injection restores absence/bytes/type/mode/managed metadata. Concurrent foreign bytes are preserved, the original preimage remains in the recovery bundle, exact paths and recovery instructions are reported, and further mutation is blocked.
- Phase-two crash recovery restores/retains Bash even if proof expires or governed source changes while down. A newly built binary can recognize the metadata table without relying on executable checker source.
- Focused lifecycle/rendered/crash tests pass, all required Jig gates are fresh and passing, the final `scripts/jig check test` passes through the rebuilt dev binary, and the completion audit finds no missing Task E deliverable.

## Idempotence and recovery

Policy seeding is idempotent because only a proven missing path is eligible; after first publication the bytes are authored and untouched. Transaction recovery is deliberately restart-safe: holding the same lock, committed journals are removed, while all other journals roll back transaction-owned states. Recovery does not execute staged operations or reuse a phase-two proof. If foreign mutation prevents rollback, retain the bundle and stop with exact manual recovery guidance; never delete or overwrite it automatically. `.agent/state/*.jsonl` remains append-only. Do not reset or discard the inherited Task C/D worktree.

## Interfaces and dependencies

The generated action uses `ActionRunner::Native { operation: "jig.file_budget", configuration: NativeActionConfigurationV1::FileBudget(...) }`, target `repo:file-budget`, legacy alias `jig.file_budget`, and `inputs = ["**"]`. Policy output must parse with `jig_file_budget::parse_policy_v1`. Proof consumes the existing target receipts and file-budget evidence/digests introduced by Tasks C/D; it must not create a second receipt store. Transaction file operations reuse path validation, no-follow reads, atomic no-replace publication, identity fingerprints, and scrubbed Git helpers already owned by `crates/jig`. Task `.1.2` remains responsible for authoring and proving Jig's own source policy; Task `.8.6` remains responsible for deleting Bash source/templates after dogfood.
