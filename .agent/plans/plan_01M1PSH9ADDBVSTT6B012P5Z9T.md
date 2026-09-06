# Define unified dashboard contracts

This ExecPlan implements Task A (`jig-sh-l2x.1`) from `docs/plans/unified-terminal-dashboard.md`. The observable outcome is an additive, public, typed contract layer in `jig-ui` that future source and terminal tasks can consume without changing either current command route.

Implementation baseline: `129629348f0483c4665d3534b0086371c8fd524f` on branch `jig-sh-l2x` after planning commit `33db32bf`.

## Progress

- [x] Create and switch to feature branch `jig-sh-l2x`.
- [x] Commit the reviewed project plan and dependency-linked Beads graph.
- [x] Claim `jig-sh-l2x.1` and build the development `jig` binary.
- [x] Inspect the existing browser DTOs, status-v1 producer, provider-v1 contract, and Task A plan sections.
- [x] Add the namespaced dashboard contract modules without changing current routes.
- [x] Add exact schema, limit, error, identity, and provider raw-preservation tests.
- [x] Add focused scenario builders and the field-level parity registry.
- [x] Run focused tests and applicable Jig gates. Focused tests pass; the workspace gate recorded unrelated proxy timing failures and generated-project failures after `/tmp` filled.
- [x] Run comprehensive working-tree review loop 1 and fix every actionable finding.
- [x] Run comprehensive working-tree review loop 2 and fix every actionable finding.
- [x] Record validation evidence and prepare Task A for commit and Beads closure.

## Surprises & Discoveries

- The implementation baseline is newer than the planning baseline because the loop-iteration pull request merged into `master`. Its web DTO now spells exhausted-attempt fields `workflow_id` and `item_key`; Task B2 still owns the real producer-to-consumer regression and removal of duplicate decoding.
- `jig-ui` already exports legacy web DTOs at crate root. Reusing the name `PlanSnapshot` there would force an early migration and violate Task A's additive rollback boundary.
- The status producer currently builds private structs plus `serde_json::Value`; the new public types must therefore remain unused until Task B2 performs the producer cutover.
- The configured workspace gate passed contract, formatting, and Clippy, then failed in existing `jig-dev-proxy` timing tests under system contention and in generated-project tests when both `/` and `/home` reached capacity. The Task A-focused test and Clippy commands passed independently.
- Review round 1 exposed a real design flaw: recorder-bounded gate and loop DTOs could not also be the status-v1 wire projection. Dedicated typed status-v1 gate, loop, and plan-summary DTOs now preserve the established unbounded compatibility shape and omit recorder-only remediation.
- Review round 2 found contract-enforcement gaps rather than a second architectural fault: the registry existed but JSON could still claim different applied limits, the status golden was generated from the same scenario as its subject, and unsupported future gate kinds lost their tag. Root and nested contract validation, a real-producer differential test, and lossless unsupported-gate storage now close those gaps.

## Decision Log

- Put new types under public module `jig_ui::dashboard` rather than replacing root web DTOs. This preserves the current HTTP implementation and gives Tasks B2 through F one stable namespace.
- Keep machine identities and terminal display strings in a non-serializable `SelectableIdentity` whose equality and hashing use only the raw value. Snapshot JSON carries raw strings directly, preventing sanitized display text from leaking into machine contracts.
- Model every bounded nested collection and text field structurally with `BoundedRows<T>` and `BoundedText`; model root limits with a typed registry-backed map. This makes omission metadata impossible to forget at nested boundaries.
- Store accepted provider reports as decoded `jig_contract::status_provider::v1::Report` plus the original `serde_json::Value`; serialize the wrapper as the raw value only. This preserves unknown provider fields while preventing a full aggregate serialize/reparse path.
- Keep scenario builders as a documented public test-support module in this internal crate. Later view/runtime tasks can share semantic fixtures instead of writing self-confirming JSON fixtures.
- Compile scenario builders only behind the `test-support` feature; the crate's self dev-dependency enables that feature for ordinary `cargo test -p jig-ui` integration tests without shipping fixtures in production builds.
- Make timeline bounds a validated `TimelineLimit` request value and alias the legacy web constants to the new contract constants so coexistence cannot drift.
- Give recorder and plan documents distinct fixed-shape root-limit structs, and add the omitted `loop_scheduled_occurrences` limit to the authoritative project plan because both occurrence collections are recorder-retained data.
- Use typed `LimitId` values at constructors and validators rather than accepting string IDs or caller-supplied ceilings. This makes wrong-shape use a distinct error and makes `LIMIT_SPECS` the source of every fixed ceiling.
- Keep a standalone parity-capability fixture beside the contract tests instead of compiling against the project-plan markdown. Task H can revise documentation without silently redefining Task A's executable contract.
- During coexistence, retain the private status producer summary implementation but compare it field-for-field against the public dashboard implementation in the producer's own test suite. Task B2 removes the duplicate after switching the producer.
- Reject invalid requested timeline limits instead of silently clamping them. The request type now makes an out-of-range CLI value explicit to callers before collection begins.

## Outcomes & Retrospective

Task A now supplies additive recorder, plan-detail, status, source, identity, limit, error, provider, scenario, and parity contracts under `jig_ui::dashboard`. No command route, HTTP behavior, or production status producer changed. Two comprehensive review rounds completed and every reported contract or test weakness was addressed.

Focused validation passed: all 31 `jig-ui` tests, the live status-producer round-trip test, the old/new provider-summary differential test, Clippy with all targets/features, production-only Clippy without test support, formatting, diff whitespace, and the structured `jig-contract`, `source-rust-fmt`, and `rust-clippy` gates. The broad core gate twice reached more than 2,450 passing tests before unrelated infrastructure failures: first two MCP completion timeouts that passed immediately in isolation, then filesystem exhaustion while allocating test temp directories. The latter run filled `/home`; removing only Cargo artifacts for `jig-sh` and `jig-ui` recovered 14.1 GiB. Full partition validation remains deliberately assigned to Task I.

## Context and orientation

`crates/jig-ui/src/lib.rs` currently exports a loopback HTTP server and the legacy web DTOs in `model.rs`. Task A must not reroute or delete them. `crates/jig/src/status.rs` currently owns private status-v1 serialization, while `crates/jig-contract/src/status_provider/v1.rs` owns the additive third-party report protocol. The new contracts bridge those future migrations without importing repository context into `jig-ui`.

The authoritative project-plan sections are 4.3 through 4.9, 5.6, 8.3 through 8.8, 11, 13.4, 14, and 17.2 through 17.3.

## Plan of work

Create `crates/jig-ui/src/dashboard/` with small modules for source requests/errors, identities, bounded values and registries, recorder/detail snapshots, typed status partitions, and test scenarios/parity metadata. Export only the module from `lib.rs`, leaving current root exports and `UiServer` unchanged.

Define the `DashboardSource` trait with type-matched `recorder`, `status`, and `plan` methods. Define opaque recorder epochs with checked allocation helpers, the two request modes, phase notifications, fatal source errors, partial snapshot errors, and plan lookup results.

Define schema-1 recorder/detail DTOs with explicit common fields, typed nested limits, all planned timeline variants, typed gates/loops/remediation data, and independent errors. Define typed status repository/work/loop/provider partitions and the accepted-provider raw/decoded wrapper.

Check in deterministic scenario builders and JSON goldens. Add registry tests that enumerate the exact limit IDs, error scopes/codes, and every parity row from section 5.6. Assert raw identity behavior, unknown provider property round trips, literal schema envelope values, null/empty rules, and representative partial-error isolation.

## Concrete steps

1. Add `jig-contract` to `jig-ui` dependencies.
2. Add the dashboard module tree and re-export its contracts.
3. Add focused integration tests and golden fixtures under `crates/jig-ui/tests/`.
4. Run `cargo fmt --all -- --check`, `cargo test -p jig-ui`, and `cargo clippy -p jig-ui --all-targets -- -D warnings`.
5. Build `jig`, run the applicable structured work gates with `JIG_DEV_BIN=target/debug/jig`, and inspect evidence.
6. Run the requested comprehensive working-tree review; repair all actionable findings and repeat once if needed.

## Validation and acceptance

Acceptance requires `cargo test -p jig-ui` to prove schema goldens, raw-identity collisions, exact registries and limits, raw provider preservation, partial errors, and parity-registry completeness. `git diff` must show no CLI dispatch, HTTP routing, or current status-producer behavior change.

The final task diff must pass formatting, Clippy with warnings denied, relevant Jig gates, and `git diff --check`. Review fingerprints must cover all staged, unstaged, and untracked task files.

## Idempotence and recovery

All Task A production changes are additive. Before any successor consumes the module, recovery is deletion of the new module/tests/dependency and the single `lib.rs` module declaration. No state format, command route, or persisted data changes.

The `.agent/state/*.jsonl` files are append-only receipts and must never be rewritten. Beads updates are made only with `br`, followed by `br sync --flush-only`.

## Interfaces and dependencies

Task A has no delivery prerequisites. It directly unblocks B2, C, D, and E. It may use `serde`, `serde_json`, and `jig-contract`; it must not depend on `jig-sh`, repository paths, provider commands, terminal mechanics, or generated templates.

The public contract names required by the project plan are `DashboardSource`, `RecorderRequest`, `RecorderMode`, `StatusRequest`, `PlanBasis`, `RecorderEpochId`, `StatusPhase`, `SourceError`, `PlanSnapshotResult`, `RecorderRefresh`, `StatusRefresh`, `StatusSnapshot`, `StatusLocalSnapshot`, `StatusProviderSnapshot`, `AcceptedProviderReport`, `Observation<T>`, `SnapshotError`, `BoundedRows<T>`, and `BoundedText`.
