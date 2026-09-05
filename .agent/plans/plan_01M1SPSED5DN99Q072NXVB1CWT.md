# Remove the external status-provider subsystem

This ExecPlan is a living document. Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` current while implementing it, following `.agent/PLANS.md`.

Plan ID: `plan_01M1SPSED5DN99Q072NXVB1CWT`

Tracking issue: `jig-sh-9yu`

Git baseline: `74ced4ef80d35728a9c18c2909d56e05c064afeb`

## Purpose / Big Picture

Jig currently contains an external status-provider subsystem: configured commands emit a public JSON protocol describing provider identities, rewrite work packages, blockers, acceptance checks, and evidence. An audit of the eleven top-level Jig projects under `~/Documents` found only one consumer, and that originating use case is no longer part of the product direction. The subsystem duplicates concepts rather than connecting to Jig's recorder, work plans, gates, receipts, components, actions, or Beads.

After this change, Jig has no status-provider protocol, provider configuration, provider process runner, provider aggregation, Packages view, or Blockers view. The useful local repository status command remains. The TUI has exactly four views—Status, Work, Timeline, and Health—and all four are driven by the local recorder refresh domain. A user can observe the cut by running `cargo run -p jig-sh -- ui` or `cargo run -p jig-sh -- status --tui`, and by seeing the same four tabs and local status facts in both entry points.

## Progress

- [x] (2026-09-05 21:15Z) Audited local adoption, the public protocol, configuration, runner, UI model, documentation, and the slow refresh path.
- [x] (2026-09-05 21:20Z) Opened structured work `plan_01M1SPSED5DN99Q072NXVB1CWT` and claimed Bead `jig-sh-9yu`.
- [x] (2026-09-05 22:10Z) Deleted the public provider contract, configuration surface, process execution, and generated configuration artifacts without compatibility aliases or parsers.
- [x] (2026-09-05 22:25Z) Collapsed the TUI to four local views and one recorder refresh domain, including removing the provider/package/blocker model, renderer, worker, and scheduler paths.
- [x] (2026-09-05 22:40Z) Removed provider documentation and tests; added exact schema-two, rejected-input, absence, cutover, single-domain, and local-view regression coverage.
- [x] (2026-09-05 23:59Z) Ran focused and repository gates, inspected the final diff and active-source search, recorded a passing structured-work batch, closed the work plan, and synchronized the completed Bead.

## Surprises & Discoveries

- The provider types, command runner, aggregation, and three related TUI presentations are internally cohesive, but they form a parallel product model with no integration into Jig's native planning or evidence model.
- The version-one protocol is explicitly specialized for software-rewrite work packages, including specification, implementation, and verification facets. It is not a neutral status extension point.
- Provider-free repositories still pay for the joined status refresh path because the TUI waits for provider and recorder work before publishing a snapshot.
- `RecorderRefresh` already carries both `RecorderSnapshot` and `StatusLocalSnapshot`, so the local Status view can be served by the same atomic refresh result as Work, Timeline, and Health.
- Clippy exposed that the combined refresh-result enum retained a large recorder variant after its status sibling was deleted. Boxing the recorder result makes the channel message and worker stack smaller without adding another ownership model.
- The first workspace gate run exercised a committed-template clone while the implementation was still uncommitted. The clone therefore contained the old `status.providers` template expression while the new binary correctly omitted `status` from its render context. The direct cut must be committed as one coherent version before those committed-template tests can pass; adding a legacy render value would reintroduce the compatibility surface this change removes.
- The next full gate run found one stale cross-entrypoint oracle in `ui_json_portability.rs`: it still hard-coded status schema version one. Replacing the literal with `STATUS_SCHEMA_VERSION` and asserting the removed root fields are absent made that test cover the direct cut instead of contradicting the dedicated schema-two contract test.
- Post-removal debug measurements are about 30.15 seconds for `jig status --json` and 30.61 seconds for `jig ui --json`, down from roughly 42–45 seconds but still too slow. The remaining delay is in local repository collection and repeated owned-process scans on this unusually process-heavy host, not in retained provider execution or TUI scheduling.

## Decision Log

- Decision: Remove the subsystem directly with no compatibility parser, deprecated fields, ignored configuration, hidden switches, or retained protocol types.
  Rationale: The user explicitly rejected compatibility, adoption is effectively absent, and retained compatibility code would preserve the same bug and maintenance surface.
  Date/Author: 2026-09-05 / Codex

- Decision: Retain `jig status` and the Status TUI view as local repository observability.
  Rationale: Local Git, work, loop, and harness status are useful native Jig concepts and do not depend on the rejected provider model.
  Date/Author: 2026-09-05 / Codex

- Decision: Use one local recorder refresh domain for Status, Work, Timeline, and Health.
  Rationale: It removes duplicated scheduling, process ownership, cancellation, and partial-refresh state while publishing a coherent local snapshot.
  Date/Author: 2026-09-05 / Codex

- Decision: Keep `jig status --tui --refresh-seconds` as the local interval and remove `jig ui --status-refresh-seconds`.
  Rationale: There is no longer a second status-provider refresh domain to configure.
  Date/Author: 2026-09-05 / Codex

- Decision: Bump the incompatible `jig status --json` schema version and omit the former root `providers` field entirely.
  Rationale: A direct cut should still identify an incompatible machine-readable representation truthfully rather than silently reusing its version.
  Date/Author: 2026-09-05 / Codex

## Outcomes & Retrospective

The external status-provider vertical slice is gone rather than deprecated. Jig no longer contains its public Rust DTOs, JSON schema/example, configuration parser, render answers, command runner, process limits, concurrency and phase machinery, freshness aggregation, provider summaries, package/blocker models, detail renderers, tabs, scheduler slot, worker messages, dedicated fixtures, or maintained documentation. Generated repositories do not emit `[status]`; old `[status]` input and `jig ui --status-refresh-seconds` fail as unknown input.

The retained product is smaller and native to Jig: `jig status --json` emits schema two with exactly the local repository, work, loop, and collection-error roots, and the TUI exposes Status, Work, Timeline, and Health from one atomic `RecorderRefresh`. The runtime scheduler has only recorder and plan-detail work. A final cleanup removed the now-meaningless tab parameter and mutable domain wrapper so the model API also expresses that single refresh domain directly.

Verification completed with the development binary and the repository harness. Focused `jig-ui` testing passed 40 unit and 21 contract tests; `jig-contract`, UI architecture/cutover, schema, rejected-input, generated-template, and portability regressions passed. The final structured batch passed format, strict Clippy (including `clippy::mod_module_files`), contract, file budget, 3,067 core tests, 112 frontend/scaffold tests, 443 vault tests plus the two serialized vault-TUI tests, 209 process tests, and all 3,833 workspace tests. The active-source audit leaves only negative-removal assertions and the current breaking-change note; older changelog and append-only issue/state records remain as historical evidence.

Residual risk is confined to the retained local status collector's latency on hosts with thousands of processes. Removing the joined provider phase reduced measured debug startup by roughly twelve seconds, but local JSON collection is still about thirty seconds on this machine. That performance path is separate from the deleted subsystem and should be optimized as owned-process/repository-observation work rather than by restoring another status domain.

## Context and Orientation

The provider protocol lives under `crates/jig-contract/src/status_provider.rs` and `crates/jig-contract/src/status_provider/`, with its checked-in schema, example, schema generator, and contract test under `crates/jig-contract/contracts/`, `crates/jig-contract/examples/`, and `crates/jig-contract/tests/`. `crates/jig-contract/src/lib.rs` publicly exports it.

The CLI configuration is parsed by `crates/jig/src/context/status_config.rs` and exposed through `crates/jig/src/context.rs`. Provider command execution and aggregation are rooted in `crates/jig/src/status.rs`, with helpers under `crates/jig/src/status/`. The local Git helper in `crates/jig/src/status/git.rs` also contains provider-input freshness logic, so retain only the repository-status portion. `crates/jig/src/cli/output/status.rs` formats both local and provider output today.

The UI adapter is `crates/jig/src/ui/source.rs`. The reusable dashboard types are under `crates/jig-ui/src/dashboard/`, where `status.rs` mixes local and provider data and `source.rs` declares separate recorder, status, and plan refreshes. The terminal model, renderer, worker, scheduler, and event loop under `crates/jig-ui/src/terminal/` carry the six-tab state and dual recorder/provider refresh domains. `RecorderRefresh` already contains the local recorder snapshot and local status snapshot needed by the retained views.

Configuration and generated artifacts include `.jig.toml`, `templates/project/.jig.toml.jinja`, and embedded template snapshots. Dedicated provider documentation is in `docs/status-provider.md`; maintained references also occur in the README, configuration, architecture/public-contract documentation, changelog, repository intent, and dashboard plan. Historical completed plans and append-only `.agent/state` records are evidence, not active product surfaces, and must not be rewritten merely to erase historical references.

## Plan of Work

First, delete the contract module and all protocol schema/example/test artifacts. Remove the status-provider configuration parser and template output so old `[status]` and `[[status.providers]]` input is rejected as unknown configuration rather than accepted or ignored. Remove dependencies that become unused.

Second, reduce the CLI status runner to local repository, work, loop, and harness observations. Remove provider process spawning, deadlines, protocol validation, input-freshness aggregation, and provider-specific text output. Define the next JSON schema version without a `providers` field and test that exact shape.

Third, remove all provider/package/blocker dashboard types and delete the Packages and Blockers tabs, detail model, renderers, keyboard routes, responsive layout cases, status workers, status refresh messages, and dual-domain scheduling. Make recorder refresh publish the local Status view along with Work, Timeline, and Health. Both `jig ui` and `jig status --tui` use the same four-view dashboard and one refresh interval.

Fourth, update current documentation, checked-in templates, snapshots, help tests, architecture tests, and UI tests. Delete provider-focused tests rather than weakening them. Add negative assertions proving the removed modules, configuration, CLI flag, JSON field, tabs, and worker paths do not return. Add focused tests proving local status data still reaches the Status view. Add a structural performance oracle that a provider-free TUI refresh invokes only the recorder domain, plus measured smoke timings outside flaky wall-clock unit assertions.

Finally, format and compile each affected crate, run focused architecture/cutover/UI/JSON tests, exercise both TUI entry points in a PTY, measure read-only JSON loading, run configured Jig gates with the development binary, inspect the diff for stale provider code and private fixture leakage, update this plan, finish the structured work, close the Bead, and synchronize its export.

## Concrete Steps

Run commands from `/home/aa/Documents/jig-sh`.

1. Delete the contract and configuration implementation using patches, then compile the narrowest crates:

       cargo fmt --all -- --check
       cargo check -p jig-contract
       cargo check -p jig-ui
       cargo check -p jig-sh

2. Run focused tests while iterating:

       cargo test -p jig-contract
       cargo test -p jig-ui
       cargo test -p jig-sh --test ui_architecture
       cargo test -p jig-sh --test ui_cutover
       cargo test -p jig-sh --test ui_json_portability
       cargo test -p jig-sh --test dashboard_contract

3. Build the development binary and validate repository contracts through it:

       cargo build -p jig-sh --bin jig
       JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides
       JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map
       JIG_DEV_BIN=target/debug/jig scripts/jig check contract

4. Exercise and time the retained machine-readable commands:

       /usr/bin/time -f '%e' target/debug/jig status --json >/tmp/jig-status.json
       /usr/bin/time -f '%e' target/debug/jig ui --json >/tmp/jig-ui.json

5. Search active source and documentation for stale subsystem identifiers, excluding historical append-only evidence only where necessary:

       rg -n 'status_provider|StatusProvider|status-refresh-seconds|Packages|Blockers|providers' crates templates docs README.md .jig.toml Cargo.toml

6. Run structured and repository verification:

       JIG_DEV_BIN=target/debug/jig scripts/jig work check --plan-id plan_01M1SPSED5DN99Q072NXVB1CWT
       JIG_DEV_BIN=target/debug/jig scripts/jig work gates --plan-id plan_01M1SPSED5DN99Q072NXVB1CWT
       JIG_DEV_BIN=target/debug/jig scripts/jig work evidence --plan-id plan_01M1SPSED5DN99Q072NXVB1CWT
       JIG_DEV_BIN=target/debug/jig scripts/jig check fmt
       JIG_DEV_BIN=target/debug/jig scripts/jig check clippy
       JIG_DEV_BIN=target/debug/jig scripts/jig check test

Expected successful output is zero failing tests and gates, a four-tab TUI from both entry points, local JSON without provider concepts, rejection of removed CLI/configuration inputs, and a source search containing no active implementation references.

## Validation and Acceptance

The removal is accepted only when all provider protocol artifacts and modules are absent, status configuration is absent from current templates and examples, and a configuration containing `[status]` or `[[status.providers]]` fails validation rather than being ignored.

`jig status --json` must emit its new schema version and exactly the local root concepts `ok`, `command`, `schema_version`, `observed_at_ms`, `outcome`, `repository`, `work`, `loops`, and `errors`; it must not emit providers, packages, blockers, provider identities, protocol metadata, or input freshness. The human-readable status command must still report the retained local facts.

`jig ui --json` must remain a local recorder one-shot. `jig ui --status-refresh-seconds` must fail as an unknown option. In a PTY, both `jig ui` and `jig status --tui` must show exactly Status, Work, Timeline, and Health, and a refresh must update all four from one recorder result. Tests must exercise navigation across those four views and prove no packages/blockers detail state exists.

The focused crate tests and full configured gates must pass. Measured status/UI timings must be recorded in this plan or structured evidence, with tests guarding the causal architecture rather than asserting a machine-dependent wall-clock threshold. No downstream project identifiers or paths may enter source, fixtures, plans, or generated evidence.

## Idempotence and Recovery

This is a coordinated pre-release source cut with no persisted provider state to migrate. Re-running formatters, builders, tests, schema checks, and template generation is safe. Let Cargo update `Cargo.lock`; do not hand-edit dependency checksums.

If removing a mixed provider/local file breaks a retained local feature, move only the local type or function into the nearest local module. Do not restore compatibility aliases, empty protocol results, ignored configuration, or deprecated flags as a shortcut. Do not modify downstream repositories during implementation or validation.

## Artifacts and Notes

Pre-change measurements on this repository were approximately 42–45 seconds for a debug `jig status --json`, 41–42 seconds for debug `jig ui --json`, and 31 seconds for release `jig ui --json`. Plain Git status and native work/loop queries were sub-second. The host had roughly 3,500 processes, and repeated owned-process cleanup scans consumed most of the system time while the joined refresh withheld the first TUI snapshot.

The subsystem entered in commits `ff517d32` (contract), `408742b0` (runner and aggregation), and `da943905` (original status TUI). These hashes are investigation landmarks only; the implementation is a forward deletion, not a revert, because useful local status and later TUI work must remain.

## Interfaces and Dependencies

At completion:

- `jig_contract` has no `status_provider` module or corresponding schema/example API.
- Jig configuration has no status provider accessor or accepted status provider keys.
- `crates/jig/src/status.rs` owns only local status observation and serialization.
- `DashboardSource` exposes recorder and plan data only; it has no provider status request or refresh method.
- `RecorderRefresh` is the atomic pair of recorder data and `StatusLocalSnapshot` used by local views.
- `Tab` has exactly `Status`, `Work`, `Timeline`, and `Health`.
- `DashboardOptions` has one local refresh interval; the scheduler has recorder and plan domains only.
- Status JSON root fields are `ok`, `command`, `schema_version`, `observed_at_ms`, `outcome`, `repository`, `work`, `loops`, and `errors`.

Revision note (2026-09-05): Initial plan written after adoption and architecture research established that the provider subsystem is an isolated parallel model and should be removed as a direct cut.

Revision note (2026-09-05): Completed the direct deletion, recorded the mixed-version template and stale portability-oracle findings, and documented the passing final gates and residual local-collector latency.
