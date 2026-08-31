# Complete the universal file-budget epic

This ExecPlan completes feature `jig-sh-generic-monorepo-zac.8`: migrate the Jig source repository from its legacy contract-v5 Rust LOC checker to the authored contract-v7 `repo:file-budget` action, prove that action through ordinary repository authority, and delete the temporary Bash checker and its source-specific integration surfaces. The resulting checkout uses the universal native action in its default profile, CI, release validation, aliases, work gates, policy, and evidence while supported older contracts remain readable.

## Progress

- [x] Revalidated the Beads graph and confirmed the delivery order is `.1.2` followed by `.8.6`.
- [x] Opened structured work plan `plan_01M1A4MHEDYS0FASGD05K50H5F` and built a development Jig binary.
- [x] Ran embedded-template adoption previews and eliminated all 11 proposed waiver drafts through behavior-preserving source splits.
- [x] Authored `.jig/file-budget.toml`, migrated `.jig.toml` and `.agent/jig-contract.json` to contract v7, and preserved source developer actions, aliases, profiles, and work gates.
- [x] Recorded successful `repo:file-budget` receipts using canonical comparison authority and zero waivers.
- [x] Wired source CI and release validation to `repo:file-budget`, retaining pinned source-specific Linux/macOS, rendered-fixture, MSRV, and no-default-feature jobs.
- [x] Closed `.1.2`, claimed `.8.6`, and removed the Bash checker from source, templates, embedded snapshots, behavioral tests, current fixtures, aliases, and documentation.
- [x] Rebuilt after deletion and proved a newly built binary retires a registry-recognized downstream checker without containing checker source bytes.
- [x] Passed focused compatibility/lifecycle tests, the 3,229-test source gate, format, Clippy, contract, rendered-repository fixtures, and the source file-budget action.
- [x] Ran structured work gates, inspected final receipts and diff, and confirmed every required gate is fresh and passing.
- [x] Closed `.8.6` and parent feature `.8`, flushed Beads state, and finished this plan successfully.

## Surprises & Discoveries

- The first adoption preview found 11 paths whose legacy debt had grown. Splitting the relevant modules removed every proposed waiver, so the final migration did not fabricate waiver reasons, expiry dates, or authorization.
- The source policy keeps the established 400/500/800 Rust thresholds. Existing baseline debt is ratcheted by exact Git comparison authority rather than source comments or a baseline ledger.
- The dirty development checkout could not use the committed template source because HEAD predates the contract-v7 implementation. The freshly built binary correctly selected `embedded:jig-sh`.
- A strict rendered full-stack audit classified the managed 3,560-line `scripts/web-node.cjs` helper as application source. The generated web rule now excludes exactly that template-owned helper; freshly rendered backend, full-stack, and tooling-only repositories pass.
- Generic adoption initially replaced Jig's specialized Rust workflow and dropped macOS and source-specific acceptance jobs. The source-owned workflow was restored, while repository policy now invokes universal comparison authority on Linux and macOS.
- The complete source test gate exposed a stale launcher-repair test assumption: a complete contract-v7 authored model can derive the former SQLx projection. The test now verifies that intended derivation and still rejects genuinely incomplete answers.

## Decision Log

- Follow the serialized delivery order in `docs/plans/universal-file-budget.md`: file-budget epoch and source dogfood first, Bash deletion second, later argv and freshness epochs afterward.
- Preserve the generated 400/500/800 rule and refactor growing paths instead of weakening the maximum or authoring migration waivers.
- Keep legacy checker recognition as a bounded identity-only table plus the downstream registry. Never retain checker source in the post-deletion binary.
- Treat Jig's specialized CI jobs as source-owned acceptance coverage; preserve pinned actions, macOS jobs, rendered fixtures, MSRV, and no-default-feature checks.
- Use `JIG_DEV_BIN=target/debug/jig` for harness commands because this work changes the runtime and templates.

## Outcomes & Retrospective

Implementation and validation are complete. Contract v7 resolves reproducibly, `repo:file-budget` has fresh zero-error receipts with no waivers, rendered fixtures pass, and an old supported contract executes only its declared command authority without Rust-native fallback. A post-deletion binary recognizes downstream generated state through identities and the durable registry, retains it until matching native evidence exists, and then retires it transactionally. The final structured run passed all seven configured checks, its generated `verify` evidence is fresh, and the stale-semantics audit found only intentional migration recognition, compatibility coverage, documentation, and absence assertions. Task `.8.6`, parent feature `.8`, and this structured work plan are closed successfully.

## Context and orientation

The authored repository contract lives in `.jig.toml` and `.agent/jig-contract.json`. The seed-once policy lives at `.jig/file-budget.toml`. Runtime implementation is under `crates/jig/src/runtime/file_budget.rs` and its submodules. Bootstrap and migration behavior live under `crates/jig/src/bootstrap/`, especially `adoption_file_budget.rs`, `file_budget_lifecycle.rs`, and `update_transaction.rs`.

Generated authority originates in `templates/project/`; embedded parity lives under `crates/jig/src/bootstrap/embedded_template_snapshots/`. Source CI is `.github/workflows/repo-policy.yml` and `.github/workflows/rust-tests.yml`; release validation is in `scripts/release.sh`; rendered repository coverage is in `scripts/fixtures/rendered-repos.sh` and runtime coverage is in `scripts/fixtures/runtime-smoke.sh`.

Task `.1.2` owns contract-v7 source dogfood and is closed. Task `.8.6` owns final deletion and is in progress. Parent feature `.8` closes after `.8.6` passes all gates.

## Plan of work

First migrate the source configuration through the embedded-template adoption transaction only after the preview has no human authorization requirement. Preserve custom commands, repository guidance, source-specific workflows, and the default developer surface while replacing legacy Rust LOC authority with `repo:file-budget`.

Next run the authored native action against configured comparison authority and require a fresh receipt matching source, configuration, policy, comparison, and evaluation identities. Close `.1.2` only after that proof.

Then delete the temporary checker from source, template, embedded snapshot, managed inventory, rendered fixtures, and Bash-specific tests. Preserve only intentional older-contract projection and digest/registry lifecycle recognition. Rebuild from post-deletion source and prove retirement with a downstream registry fixture rather than embedded checker bytes.

Finally run source and rendered-repository gates, inspect receipts and stale-semantics searches, close the remaining Beads, flush their JSONL export, and finish structured work.

## Concrete steps

Run from the repository root:

    cargo build -p jig-sh --bin jig
    export JIG_DEV_BIN=target/debug/jig
    scripts/jig check repo:file-budget
    scripts/jig check contract
    scripts/jig check test
    scripts/jig check fmt
    scripts/jig check clippy
    scripts/fixtures/rendered-repos.sh

For structured completion:

    scripts/jig work check --plan-id plan_01M1A4MHEDYS0FASGD05K50H5F
    scripts/jig work gates --plan-id plan_01M1A4MHEDYS0FASGD05K50H5F
    scripts/jig work evidence --plan-id plan_01M1A4MHEDYS0FASGD05K50H5F
    scripts/jig work receipts --plan-id plan_01M1A4MHEDYS0FASGD05K50H5F

## Validation and acceptance

Acceptance requires reproducible contract-v7 config and manifest; an authored seed-once policy; exact generated `repo:file-budget` authority with target-local inputs; default profile, aliases, work gates, CI, and release invocation; passing native receipts; no Rust-specific native LOC dispatch; no temporary Bash checker bytes in source, templates, or embedded snapshots; supported old-contract readability; and a newly built post-deletion binary that still recognizes and retires downstream generated assets through durable identities.

Validation includes focused Rust tests, rendered repositories, runtime coverage, `repo:file-budget`, the full source test gate, format, Clippy, contract, structured work gates, and final diff/stale-semantics inspection. Linux runs locally; existing source CI retains macOS test and Clippy coverage, and the universal file-budget job runs on both Linux and macOS.

## Idempotence and recovery

Adoption preview is read-only. Full adoption/update uses the durable update transaction and can recover from its crash journal on the next run. The seed-once policy is authored state and is not overwritten by later recopy. The Bash implementation was deleted only after a fresh native receipt and a successful phase-two retirement. Post-deletion compatibility is recovered through durable identities and the downstream registry, never by restoring source bytes.

## Interfaces and dependencies

The native action runner is `jig.file_budget` and the authored target is `repo:file-budget`. Comparison authority is supplied through ordinary action options: merge base, exact tree plus provenance, index, or strict inventory. Evidence remains in the standard target run/receipt model; there is no file-budget-specific journal. Contract v7 is the allocated file-budget epoch. Beads ordering is `.1.2 -> .8.6 -> .3.3`, with `.8.6` also completing parent feature `.8`.
